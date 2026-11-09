//! Runtime profile files and the v2 compiler. Imports never execute policy assets.
use super::*;

pub fn permission_contract(s: &EngineeringState, target: &PermissionTargetRef) -> Result<String> {
    let r = s.configuration(&target.revision_id)?;
    let kind = if target.kind == PermissionTargetKind::Harness {
        "harness"
    } else {
        "sandbox"
    };
    if !r.immutable
        || r.kind != kind
        || r.content_hash != target.content_hash
        || r.content_hash != configuration_hash(&r.data)
    {
        return Err(err("revision_conflict", "权限目标摘要不一致"));
    }
    match target.kind {
        PermissionTargetKind::Harness => {
            let h: HarnessDefinition = r.decode()?;
            let provider: HarnessProvider = serde_json::from_value(json!(h.provider))
                .map_err(|_| err("permission_unsupported", "未知 Harness"))?;
            if h.runtime_version != permissions::capabilities(provider).version {
                return Err(err(
                    "permission_unsupported",
                    "该 Harness 版本尚未验证权限执行合同",
                ));
            }
            Ok(format!(
                "{}-native-v1:{}",
                provider.as_str(),
                harness::managed_adapter_identity(provider)
            ))
        }
        PermissionTargetKind::Sandbox => {
            let b: SandboxDefinition = r.decode()?;
            if b.provider != "docker"
                || b.runtime_version != "1"
                || b.enforcement != "docker-worker/v1"
            {
                return Err(err("permission_unsupported", "未知 Sandbox 执行合同"));
            }
            Ok(format!(
                "docker-worker/v1:{:x}",
                Sha256::digest(include_bytes!("docker.rs"))
            ))
        }
    }
}

pub fn compile_permission_profiles(
    s: &EngineeringState,
    lock: &VersionedPermissionLock,
    scope: Option<ExecutionScope>,
) -> Result<ExecutionPermissions> {
    for bound in [&lock.harness, &lock.sandbox] {
        let mut profile = bound.profile.clone();
        profile.normalize()?;
        if profile != bound.profile
            || bound.profile_hash != permission_content_hash(&profile)
            || bound.binding_hash != permission_content_hash(&bound.binding)
            || bound.binding.permission_revision_id != profile.id
            || bound.binding.target.kind != profile.target_kind
            || bound.binding.enforcement_contract != permission_contract(s, &bound.binding.target)?
        {
            return Err(err(
                "permission_asset_mismatch",
                "权限 profile、目标或执行合同漂移",
            ));
        }
    }
    let harness: HarnessDefinition = s
        .configuration(&lock.harness.binding.target.revision_id)?
        .decode()?;
    let provider: HarnessProvider =
        serde_json::from_value(json!(harness.provider)).map_err(storage)?;
    let h = lock.harness.profile.policy.portable();
    let approval = lock
        .harness
        .profile
        .policy
        .approval
        .ok_or_else(|| err("invalid_params", "缺少审批策略"))?;
    if approval == ApprovalPolicy::OnRequest
        && (h.filesystem != FilesystemPermission::FullAccess
            || h.network != NetworkPermission::Enabled)
    {
        return Err(err(
            "permission_unsupported",
            "此受限 profile 无法证明审批绕过原生沙箱后仍维持最大权限范围",
        ));
    }
    // Binding feasibility is distinct from an authorized execution scope. The inferred
    // roots prove only that some scope can enforce this profile; dispatch always supplies its real scope.
    let scope = scope.unwrap_or_else(|| {
        let mut inferred = ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec![],
            read_only_paths: vec![],
        };
        if let FilesystemPermission::Scoped { rules } = &h.filesystem {
            for r in rules {
                if let Some(p) = r.path.strip_prefix("/work/") {
                    if r.access == FileAccess::ReadWrite {
                        inferred.writable_paths.push(p.into());
                    }
                    if r.access == FileAccess::Read {
                        inferred.read_only_paths.push(p.into());
                    }
                } else if r.path == "/work" && r.access == FileAccess::ReadWrite {
                    inferred.writable_paths.push(".".into());
                }
            }
        }
        inferred
    });
    for path in scope.writable_paths.iter().chain(&scope.read_only_paths) {
        if path != "."
            && (path.is_empty()
                || path.len() > 4096
                || path.starts_with('/')
                || path.ends_with('/')
                || path.contains(['\\', '\n', '\r', '\0'])
                || path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".."))
        {
            return Err(err(
                "invalid_params",
                "ExecutionScope 必须使用规范的工作区相对路径",
            ));
        }
    }
    if scope.writable_paths.len() > 128 || scope.read_only_paths.len() > 128 {
        return Err(err("invalid_params", "ExecutionScope 路径数量超出范围"));
    }
    let definitions: Vec<_> = [&lock.harness, &lock.sandbox]
        .into_iter()
        .map(|b| Permission {
            id: format!("profile-v2:{}", b.profile.id),
            identity: b.profile.identity.clone(),
            name: b.profile.name.clone(),
            version: 1,
            supersedes: None,
            schema_version: 1,
            policy: b.profile.policy.portable(),
        })
        .collect();
    let config = PermissionConfiguration {
        revision: 1,
        harnesses: [(
            provider.as_str().into(),
            PermissionBinding {
                permission: Some(PermissionRef {
                    revision_id: definitions[0].id.clone(),
                }),
                overrides: PermissionPatch::default(),
            },
        )]
        .into(),
        sandbox: PermissionBinding {
            permission: Some(PermissionRef {
                revision_id: definitions[1].id.clone(),
            }),
            overrides: PermissionPatch::default(),
        },
        approval,
    };
    let mut result = permissions::resolve_execution_permissions(
        &definitions,
        &config,
        provider,
        scope,
        None,
        None,
        None,
    )?;
    result.harness.reference.revision_id = lock.harness.profile.id.clone();
    result.sandbox.reference.revision_id = lock.sandbox.profile.id.clone();
    result.harness.sources = vec![format!("binding:{}", lock.harness.binding_revision_id)];
    result.sandbox.sources = vec![format!("binding:{}", lock.sandbox.binding_revision_id)];
    result.enforcement.compiler_version = "permission-compiler/v2".into();
    result.binding_lock = Some(lock.clone());
    Ok(result)
}

pub fn preflight_profile(
    s: &EngineeringState,
    target: &PermissionTargetRef,
    profile: &PermissionProfile,
) -> Result<Value> {
    if permission_target(s, target.kind, &target.revision_id)? != *target {
        return Err(err("revision_conflict", "预检目标摘要不一致"));
    }
    let mut checked = profile.clone();
    checked.normalize()?;
    if checked.target_kind != target.kind {
        return Err(err("permission_inapplicable", "profile 不适用于该目标类型"));
    }
    let bound = |target: PermissionTargetRef, p: PermissionProfile| -> Result<BoundPermission> {
        let binding = VersionedPermissionBinding {
            enforcement_contract: permission_contract(s, &target)?,
            target,
            permission_revision_id: p.id.clone(),
        };
        Ok(BoundPermission {
            binding_revision_id: "preview".into(),
            binding_hash: permission_content_hash(&binding),
            profile_hash: permission_content_hash(&p),
            binding,
            profile: p,
        })
    };
    let harness_target = if target.kind == PermissionTargetKind::Harness {
        target.clone()
    } else {
        let r = s
            .governance
            .iter()
            .find(|r| {
                r.kind == "harness"
                    && r.data["provider"] == "codex"
                    && permission_contract(
                        s,
                        &PermissionTargetRef {
                            kind: PermissionTargetKind::Harness,
                            revision_id: r.id.clone(),
                            content_hash: r.content_hash.clone(),
                        },
                    )
                    .is_ok()
            })
            .ok_or_else(|| {
                err(
                    "permission_unsupported",
                    "没有可验证的 Harness 合同用于组合预检",
                )
            })?;
        permission_target(s, PermissionTargetKind::Harness, &r.id)?
    };
    let sandbox_target = if target.kind == PermissionTargetKind::Sandbox {
        target.clone()
    } else {
        permission_target(s, PermissionTargetKind::Sandbox, DEFAULT_PERMISSION_SANDBOX)?
    };
    let default = |id: &str| -> Result<PermissionProfile> { s.configuration(id)?.decode() };
    let lock = VersionedPermissionLock {
        harness: bound(
            harness_target,
            if target.kind == PermissionTargetKind::Harness {
                checked.clone()
            } else {
                default("permission-harness-full-access-v2-r1")?
            },
        )?,
        sandbox: bound(
            sandbox_target,
            if target.kind == PermissionTargetKind::Sandbox {
                checked
            } else {
                default("permission-sandbox-full-access-v2-r1")?
            },
        )?,
    };
    let compiled = compile_permission_profiles(s, &lock, None)?;
    Ok(
        json!({"compatible":true,"scope_checked":false,"execution_scope_required":true,"enforcement_contract":permission_contract(s,target)?,"enforcement":compiled.enforcement,"side_effects":false}),
    )
}

pub fn verify_permission_installation(
    s: &EngineeringState,
    lock: &VersionedPermissionLock,
    installation: &RuntimeInstallation,
) -> Result<()> {
    let h: HarnessDefinition = s
        .configuration(&lock.harness.binding.target.revision_id)?
        .decode()?;
    let b: SandboxDefinition = s
        .configuration(&lock.sandbox.binding.target.revision_id)?
        .decode()?;
    let platform = format!("{}/{}", installation.key.os, installation.key.arch);
    if installation.key.provider.as_str() != h.provider
        || installation.key.version.split('+').next() != Some(h.runtime_version.as_str())
        || !b.platforms.contains(&platform)
        || !matches!(installation.artifact, RuntimeArtifact::DockerImage { .. })
    {
        return Err(err(
            "permission_unsupported",
            "实际运行时版本、平台或镜像类型不满足权限目标",
        ));
    }
    Ok(())
}

impl GlobalStore {
    /// Additive projections preserve immutable Agents and historical serialization.
    pub fn migrate_permission_selections(&self) -> Result<()> {
        self.transact(None,&mut |s| {
            let agents:Vec<_>=s.governance.iter().filter(|r|r.kind=="agent" && r.data.get("permission_selection").is_none()
                && !s.governance.iter().any(|m|m.kind=="permission_migration" && m.definition_id==r.id)).cloned().collect();
            for agent in agents {
                let a:AgentComposition=agent.decode()?;
                let converted=(||->Result<(Vec<PermissionProfile>,Vec<BoundPermission>,AgentPermissionSelection)> {
                    let old_h=s.permissions.iter().find(|p|Some(&p.id)==a.permissions.first()).ok_or_else(||err("not_found","旧 Harness 权限不存在"))?;
                    let old_b=s.permissions.iter().find(|p|Some(&p.id)==a.permissions.last()).ok_or_else(||err("not_found","旧 Sandbox 权限不存在"))?;
                    if old_h.id=="permission-legacy-restricted-v1" {return Err(err("permission_unsupported","保留 legacy 动态范围与凭据边界"));}
                    let mut profiles=Vec::new();let mut bounds=Vec::new();
                    for (kind,old,target_id) in [(PermissionTargetKind::Harness,old_h,a.harness_ref.as_str()),(PermissionTargetKind::Sandbox,old_b,DEFAULT_PERMISSION_SANDBOX)] {
                        let policy=ProfilePolicy{filesystem:old.policy.filesystem.clone(),network:old.policy.network.clone(),tools:if kind==PermissionTargetKind::Harness {Some(old.policy.tools.clone())}else{None},approval:if kind==PermissionTargetKind::Harness {Some(a.approval)}else{None}};
                        let identity=format!("legacy-profile-{}",configuration_hash(&json!({"kind":kind,"policy":policy})));
                        let mut profile=PermissionProfile{schema_version:2,id:format!("{identity}-r1"),identity,name:format!("兼容固定 {kind:?} 权限"),version:1,supersedes:None,target_kind:kind,policy};
                        profile.normalize()?;
                        let target=permission_target(s,kind,target_id)?;
                        preflight_profile(s,&target,&profile)?;
                        let binding=VersionedPermissionBinding{enforcement_contract:permission_contract(s,&target)?,target,permission_revision_id:profile.id.clone()};
                        let hash=permission_content_hash(&binding);
                        bounds.push(BoundPermission{binding_revision_id:format!("legacy-binding-{hash}"),binding_hash:hash,binding,profile:profile.clone(),profile_hash:permission_content_hash(&profile)});
                        profiles.push(profile);
                    }
                    let selection=AgentPermissionSelection{sandbox_ref:DEFAULT_PERMISSION_SANDBOX.into(),harness:PermissionSelection::Pinned{binding_revision_id:bounds[0].binding_revision_id.clone()},sandbox:PermissionSelection::Pinned{binding_revision_id:bounds[1].binding_revision_id.clone()}};
                    Ok((profiles,bounds,selection))
                })();
                let data=match converted {
                    Ok((profiles,bounds,selection))=>{
                        insert_permission_profiles(s,&profiles.iter().map(|p|json!(p)).collect::<Vec<_>>(),"legacy_conversion")?;
                        for b in bounds {if !s.governance.iter().any(|r|r.id==b.binding_revision_id) {
                            let data=json!(b.binding);s.governance.push(ConfigurationRecord{id:b.binding_revision_id.clone(),kind:"permission_binding".into(),definition_id:b.binding_revision_id,scope:Scope::Global,name:b.profile.name,version:1,supersedes:None,content_hash:configuration_hash(&data),data,immutable:true,revision:1,created_at:0});
                        }}
                        json!({"status":"converted_fixed","selection":selection,"original_permissions":a.permissions,"original_approval":a.approval})
                    }
                    Err(e)=>json!({"status":"legacy_fixed","reason":e.message,"original_permissions":a.permissions,"original_approval":a.approval}),
                };
                s.governance.push(ConfigurationRecord{id:format!("permission-migration:{}",agent.id),kind:"permission_migration".into(),definition_id:agent.id,scope:agent.scope,name:agent.name,version:1,supersedes:None,content_hash:configuration_hash(&data),data,immutable:true,revision:1,created_at:0});
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }

    pub fn permission_preset_sources(&self) -> Result<Vec<PathBuf>> {
        let mut roots = vec![Path::new(&self.root()).join("permissions/presets")];
        let mut packaged = false;
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let path = parent.join("permissions/presets");
            packaged = path.exists();
            roots.push(path);
        }
        if !packaged {
            roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../permissions/presets"));
        }
        for r in self
            .snapshot()?
            .governance
            .iter()
            .filter(|r| r.kind == "permission_source")
        {
            if let Some(p) = r.data["path"].as_str() {
                roots.push(PathBuf::from(p));
            }
        }
        let mut canonical = Vec::new();
        for root in roots {
            if root.exists() {
                reject_symlink(&root)?;
                let p = fs::canonicalize(root).map_err(storage)?;
                if !canonical.contains(&p) {
                    canonical.push(p);
                }
            }
        }
        Ok(canonical)
    }
    pub fn reload_permission_presets(&self, c: &Command) -> Result<Value> {
        if c.command_id.is_empty() {
            return Err(err("invalid_params", "command_id 不能为空"));
        }
        let fp = command_fingerprint("permission.profile.reload", c);
        if let Some(previous) = self.replay(&c.command_id, &fp)? {
            return Ok(previous);
        }
        let roots = self.permission_preset_sources()?;
        let mut profiles = Vec::new();
        let mut origins = Vec::new();
        let mut bytes = 0;
        for root in &roots {
            let mut files: Vec<_> = fs::read_dir(root)
                .map_err(storage)?
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(storage)?;
            files.sort_by_key(|v| v.file_name());
            for entry in files {
                let path = entry.path();
                if path.extension().is_none_or(|v| v != "json") {
                    continue;
                }
                reject_symlink(&path)?;
                let meta = fs::metadata(&path).map_err(storage)?;
                if !meta.is_file() || meta.len() > 1024 * 1024 {
                    return Err(err("invalid_params", "预设必须是不超过 1MiB 的 JSON 文件"));
                }
                bytes += meta.len();
                if bytes > 4 * 1024 * 1024 {
                    return Err(err("invalid_params", "预设导入超过 4MiB"));
                }
                let body = fs::read(&path).map_err(storage)?;
                let value: Value = serde_json::from_slice(&body).map_err(storage)?;
                let entries = value["profiles"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![value.clone()]);
                for p in &entries {
                    if let Some(id) = p["id"].as_str() {
                        origins.push((id.to_owned(),json!({"source_path":path,"file_sha256":format!("{:x}",Sha256::digest(&body))})));
                    }
                }
                if let Some(batch) = value["profiles"].as_array() {
                    profiles.extend(batch.clone());
                } else {
                    profiles.push(value);
                }
            }
        }
        self.transact(Some((&c.command_id, &fp)), &mut |s| {
            let result = insert_permission_profiles(s, &profiles, "presets")?;
            for (profile, data) in &origins {
                let key = format!(
                    "permission-file-origin:{}",
                    permission_content_hash(&json!({"profile":profile,"source":data}))
                );
                if !s.governance.iter().any(|r| r.id == key) {
                    s.governance.push(ConfigurationRecord {
                        id: key,
                        kind: "permission_file_origin".into(),
                        definition_id: profile.clone(),
                        scope: Scope::Global,
                        name: "预设文件来源".into(),
                        version: 1,
                        supersedes: None,
                        content_hash: configuration_hash(data),
                        data: data.clone(),
                        immutable: true,
                        revision: 1,
                        created_at: 0,
                    });
                }
            }
            Ok(result)
        })
    }
}
