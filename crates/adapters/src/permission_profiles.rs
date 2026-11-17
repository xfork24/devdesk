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
        || r.r#type != kind
        || r.content_hash != target.content_hash
        || r.content_hash != content_digest(&r.refs, &r.data)
    {
        return Err(err("revision_conflict", "权限目标摘要不一致"));
    }
    match target.kind {
        PermissionTargetKind::Harness => {
            let h: HarnessDefinition = r.decode()?;
            let provider: HarnessProvider = serde_json::from_value(json!(h.provider))
                .map_err(|_| err("permission_unsupported", "未知 Harness"))?;
            harness_execution_recipe(s, r)?;
            // Execution build identity remains frozen separately in RuntimeBinding.
            // A model-catalog/UI/schema change does not change permission semantics.
            Ok(format!(
                "{}-native-v2:{}",
                provider.as_str(),
                permission_semantics_digest(provider)
            ))
        }
        PermissionTargetKind::Sandbox => {
            let b: SandboxDefinition = r.decode()?;
            if b.provider != "docker" || b.runtime_version.is_empty() || b.enforcement.is_empty() {
                return Err(err("permission_unsupported", "未知 Sandbox 执行合同"));
            }
            Ok(format!(
                "docker-worker/v2:{}",
                permission_semantics_digest(HarnessProvider::Codex)
            ))
        }
    }
}

fn permission_semantics_digest(provider: HarnessProvider) -> &'static str {
    // Only the expected contract compiled into this binary is shared. Source
    // revisions, bindings, scopes and live assets are still checked on each use.
    static CODEX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    static DSH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    match provider {
        HarnessProvider::Codex => &CODEX,
        HarnessProvider::Dsh => &DSH,
    }
    .get_or_init(|| compute_permission_semantics_digest(provider))
}

fn compute_permission_semantics_digest(provider: HarnessProvider) -> String {
    let mut supported = json!(permissions::capabilities(provider));
    supported.as_object_mut().unwrap().remove("version");
    supported.as_object_mut().unwrap().remove("recipe_digest");
    permission_content_hash(
        &json!({"revision":"portable-permissions/2","provider":provider,"capabilities":supported,"assets":permissions::asset_digests(),"manifest":include_str!("../../../permissions/backends/manifest.json")}),
    )
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
            || bound.profile_hash
                != content_digest(&References::new(), &permission_profile_data(&profile))
            || bound.binding_hash != content_digest(&References::new(), &json!(bound.binding))
            || bound.binding.permission_revision_id != profile.id
            || bound.binding.target.kind != profile.target_kind
            || bound.binding.enforcement_contract
                != if bound.binding_revision_id == bound.profile.id {
                    permission_contract(s, &bound.binding.target)?;
                    format!("direct-profile:{}", bound.binding.target.content_hash)
                } else {
                    permission_contract(s, &bound.binding.target)?
                }
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
    let work_type = scope.as_ref().map_or(WorkType::Discussion, |s| s.work_type);
    let (writable_paths, read_only_paths) =
        profile_pair_workspace_access(&lock.harness.profile, &lock.sandbox.profile);
    let scope = ExecutionScope {
        work_type,
        writable_paths,
        read_only_paths,
    };
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
    let resolved = |bound: &BoundPermission| ResolvedPermission {
        reference: PermissionRef {
            revision_id: bound.profile.id.clone(),
        },
        policy: bound.profile.policy.portable(),
        sources: vec![format!("binding:{}", bound.binding_revision_id)],
    };
    let mut result = permissions::compile_resolved_permissions(
        provider,
        scope,
        resolved(&lock.harness),
        resolved(&lock.sandbox),
        approval,
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
            binding_hash: content_digest(&References::new(), &json!(binding)),
            profile_hash: content_digest(&References::new(), &permission_profile_data(&p)),
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
                r.r#type == "harness"
                    && template_record_current(s, r)
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
        permission_target(
            s,
            PermissionTargetKind::Sandbox,
            devdesk_core::current_template_defaults(s)
                .as_ref()
                .and_then(|d| d["sandbox_ref"].as_str())
                .ok_or_else(|| err("configuration_required", "默认 Sandbox 未配置"))?,
        )?
    };
    let defaults = devdesk_core::current_template_defaults(s);
    let default = |key: &str| -> Result<PermissionProfile> {
        s.configuration(
            defaults
                .as_ref()
                .and_then(|d| d[key].as_str())
                .ok_or_else(|| err("configuration_required", format!("默认配置缺少 {key}")))?,
        )?
        .permission_profile()
    };
    let lock = VersionedPermissionLock {
        harness: bound(
            harness_target,
            if target.kind == PermissionTargetKind::Harness {
                checked.clone()
            } else {
                default("harness_permission_ref")?
            },
        )?,
        sandbox: bound(
            sandbox_target,
            if target.kind == PermissionTargetKind::Sandbox {
                checked
            } else {
                default("sandbox_permission_ref")?
            },
        )?,
    };
    let compiled = compile_permission_profiles(s, &lock, None)?;
    Ok(
        json!({"compatible":true,"scope_checked":true,"execution_scope_required":false,"enforcement_contract":permission_contract(s,target)?,"enforcement":compiled.enforcement,"side_effects":false}),
    )
}

pub fn verify_permission_installation(
    s: &EngineeringState,
    lock: &VersionedPermissionLock,
    installation: &RuntimeInstallation,
) -> Result<()> {
    let harness_record = s.configuration(&lock.harness.binding.target.revision_id)?;
    let h: HarnessDefinition = harness_record.decode()?;
    let b: SandboxDefinition = s
        .configuration(&lock.sandbox.binding.target.revision_id)?
        .decode()?;
    let platform = format!("{}/{}", installation.key.os, installation.key.arch);
    // The installation must originate from the exact recipe the permission target pinned.
    let selected = compose_execution_recipe(
        harness_record,
        s.configuration(&lock.sandbox.binding.target.revision_id)?,
    )?;
    let recipe_match = permissions::installation_recipe(installation) == selected.recipe_digest();
    if installation.key.provider.as_str() != h.provider
        || !recipe_match
        || !b.platforms.contains(&platform)
        || !matches!(installation.artifact, RuntimeArtifact::DockerImage { .. })
    {
        return Err(err(
            "permission_unsupported",
            "实际运行时构建配方、平台或镜像类型不满足权限目标",
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PermissionPresetSource {
    pub path: PathBuf,
    pub builtin_path: PathBuf,
    pub is_builtin: bool,
    pub revision: u64,
}

fn permission_preset_directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(err("invalid_params", "预设来源必须是绝对目录"));
    }
    reject_symlink(path)?;
    let path = fs::canonicalize(path).map_err(storage)?;
    if !path.is_dir() {
        return Err(err("invalid_params", "预设来源不是目录"));
    }
    Ok(path)
}

fn preset_source_selection(s: &EngineeringState) -> Option<&ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|r| r.r#type == "permission_source_selection")
        .max_by_key(|r| r.version)
}

fn preset_source(s: &EngineeringState, builtin: PathBuf) -> PermissionPresetSource {
    let path = preset_source_selection(s)
        .and_then(|r| r.data["path"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| builtin.clone());
    PermissionPresetSource {
        is_builtin: path == builtin,
        revision: preset_source_selection(s).map_or(0, |r| r.version),
        path,
        builtin_path: builtin,
    }
}

fn select_preset_source(s: &mut EngineeringState, path: Option<&Path>) {
    let previous = preset_source_selection(s);
    let data = json!({"path":path});
    if previous.is_none() && path.is_none() || previous.is_some_and(|r| r.data == data) {
        return;
    }
    let version = previous.map_or(1, |r| r.version + 1);
    let supersedes = previous.map(|r| r.id.clone());
    s.governance.push(ConfigurationRecord {
        id: id(),
        r#type: "permission_source_selection".into(),
        definition_id: "permission-preset-source".into(),
        scope: Scope::global(),
        name: "权限预设目录".into(),
        version,
        supersedes,
        content_hash: content_digest(&References::new(), &data),
        refs: References::new(),
        data,
        immutable: true,
        revision: 1,
        created_at: 0,
    });
}

struct PermissionPresetCommand<'a> {
    command: &'a Command,
    fingerprint: &'a str,
    origin: Value,
}

impl GlobalStore {
    fn builtin_permission_preset_source(&self) -> Result<PathBuf> {
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let path = parent.join("resources/configuration/permissions/profiles");
            if path.is_dir() {
                return permission_preset_directory(&path);
            }
        }
        // A copied/read-only executable must not depend on its build checkout.
        // Materialize only packaged preset data, separately from user-selected
        // editable directories. Keep old content-addressed generations intact.
        let files = factory_bundle::packaged_permission_presets()?;
        let generation = configuration_hash(&json!(files));
        let mut root = PathBuf::from(self.root());
        for part in ["packaged-permission-presets", &generation] {
            root.push(part);
            reject_symlink(&root)?;
            fs::create_dir_all(&root).map_err(storage)?;
        }
        for (name, body) in files {
            let path = root.join(name);
            reject_symlink(&path)?;
            if fs::read(&path).is_ok_and(|bytes| bytes == body.as_bytes()) {
                continue;
            }
            let temporary = root.join(format!(".{}.tmp", id()));
            let write = (|| {
                use std::io::Write;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(storage)?;
                file.write_all(body.as_bytes()).map_err(storage)?;
                drop(file);
                fs::rename(&temporary, &path).map_err(storage)
            })();
            if write.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            write?;
        }
        permission_preset_directory(&root)
    }

    pub fn permission_preset_source(&self) -> Result<PermissionPresetSource> {
        self.permission_preset_source_at(&self.configuration_snapshot()?.state)
    }

    pub fn permission_preset_source_at(
        &self,
        state: &EngineeringState,
    ) -> Result<PermissionPresetSource> {
        Ok(preset_source(
            state,
            self.builtin_permission_preset_source()?,
        ))
    }

    pub fn select_permission_preset_source(&self, c: &Command) -> Result<Value> {
        if c.command_id.is_empty() {
            return Err(err("invalid_params", "command_id 不能为空"));
        }
        let fp = command_fingerprint("permission.source.select", c);
        if let Some(previous) = self.replay(&c.command_id, &fp)? {
            return Ok(previous);
        }
        let path = permission_preset_directory(Path::new(
            c.payload["path"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "需要预设目录"))?,
        ))?;
        let builtin = self.builtin_permission_preset_source()?;
        let before = self.configuration_snapshot()?;
        let current = preset_source_selection(&before.state).map_or(0, |r| r.version);
        if c.expected_revision != Some(current) {
            return Err(err("revision_conflict", "预设来源已改变，请重新载入"));
        }
        let transfer = devdesk_core::configuration_transfer_witness(self)?;
        let mut next = before.state.as_ref().clone();
        select_preset_source(&mut next, if path == builtin { None } else { Some(&path) });
        let result = json!(preset_source(&next, builtin));
        self.commit_permission_catalog(
            &before,
            next,
            PermissionPresetCommand {
                command: c,
                fingerprint: &fp,
                origin: json!({"kind":"human"}),
            },
            result,
            transfer,
        )
    }

    pub fn reload_permission_presets(&self, c: &Command) -> Result<Value> {
        self.reload_permission_presets_from(c, false)
    }
    pub fn initialize_permission_presets(&self, c: &Command) -> Result<Value> {
        self.reload_permission_presets_from(c, true)
    }
    fn reload_permission_presets_from(&self, c: &Command, initializing: bool) -> Result<Value> {
        if c.command_id.is_empty() {
            return Err(err("invalid_params", "command_id 不能为空"));
        }
        let request = command_fingerprint("permission.profile.reload", c);
        let fp = if initializing {
            configuration_hash(&("initialize-permission-presets/v1", &request))
        } else {
            request
        };
        if let Some(previous) = self.replay(&c.command_id, &fp)? {
            return Ok(previous);
        }
        let builtin = match c.payload.get("builtin") {
            None => false,
            Some(Value::Bool(value)) => *value,
            _ => return Err(err("invalid_params", "builtin 必须为布尔值")),
        };
        let before = self.configuration_snapshot()?;
        let source_revision = preset_source_selection(&before.state).map_or(0, |r| r.version);
        if c.expected_revision != Some(source_revision) {
            return Err(err("revision_conflict", "预设来源已改变，请重新载入"));
        }
        let transfer = devdesk_core::configuration_transfer_witness(self)?;
        let builtin_path = self.builtin_permission_preset_source()?;
        let source = preset_source(&before.state, builtin_path.clone());
        let root = permission_preset_directory(if builtin { &builtin_path } else { &source.path })?;
        let mut profiles = Vec::new();
        let mut origins = Vec::new();
        let mut bytes = 0;
        {
            let mut files: Vec<_> = fs::read_dir(&root)
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
                    if let (Some(identity), Some(revision)) =
                        (p["identity"].as_str(), p["revision"].as_u64())
                    {
                        origins.push((source_reference(identity, revision)?,json!({"source_path":path,"file_sha256":format!("{:x}",Sha256::digest(&body))})));
                    }
                }
                if let Some(batch) = value["profiles"].as_array() {
                    profiles.extend(batch.clone());
                } else {
                    profiles.push(value);
                }
            }
        }
        let mut next = before.state.as_ref().clone();
        let s = &mut next;
        let result = insert_permission_profiles(s, &profiles, "presets")?;
        for (profile, data) in &origins {
            let key = format!(
                "permission-file-origin:{}",
                permission_content_hash(&json!({"profile":profile,"source":data}))
            );
            if !s.governance.iter().any(|r| r.id == key) {
                s.governance.push(ConfigurationRecord {
                    id: key,
                    r#type: "permission_file_origin".into(),
                    definition_id: profile.clone(),
                    scope: Scope::global(),
                    name: "预设文件来源".into(),
                    version: 1,
                    supersedes: None,
                    content_hash: content_digest(&References::new(), data),
                    refs: References::new(),
                    data: data.clone(),
                    immutable: true,
                    revision: 1,
                    created_at: 0,
                });
            }
        }
        if builtin {
            select_preset_source(s, None);
        }
        devdesk_core::register_template_members(s, template_catalog_generation(s));
        self.commit_permission_catalog(&before,next,PermissionPresetCommand{command:c,fingerprint:&fp,origin:if initializing{json!({"kind":"runtime_coordinator","operation":"initialize_permission_presets"})}else{json!({"kind":"human"})}},result,transfer)
    }

    fn commit_permission_catalog(
        &self,
        before: &ConfigurationCatalogSnapshot,
        next: EngineeringState,
        request: PermissionPresetCommand<'_>,
        result: Value,
        transfer: devdesk_core::concurrency::ResourceVersion,
    ) -> Result<Value> {
        use devdesk_core::concurrency::*;
        let c = request.command;
        let fingerprint = request.fingerprint;
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            devdesk_core::prepare_configuration_publication(self, before, &candidate)?;
        let key = ResourceKey::new(
            "governance",
            format!("permission-command:{}", configuration_hash(&c.command_id)),
        );
        let data = json!({"origin":request.origin,"request_digest":fingerprint,"target":c.target,"expected_revision":c.expected_revision});
        let row = ConfigurationRecord {
            id: key.id.clone(),
            r#type: "permission_command".into(),
            definition_id: c.command_id.clone(),
            scope: Scope::global(),
            name: "权限配置命令".into(),
            version: 1,
            supersedes: None,
            content_hash: content_digest(&References::new(), &data),
            refs: References::new(),
            data,
            immutable: true,
            revision: 1,
            created_at: 0,
        };
        let mut guards = publication.read_set.versions().to_vec();
        guards.push(transfer);
        guards.push(ResourceVersion {
            resource: key.clone(),
            revision: 0,
        });
        let mut changes = publication.changes;
        changes.push(ResourceChange {
            resource: key,
            value: row.to_storage(),
        });
        self.commit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: fingerprint.into(),
            read_set: ReadSet::new(guards)?,
            changes,
            result,
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        })
        .map(|r| r.result)
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn shared_embedded_contract_matches_each_provider_without_sharing_policy() {
        for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
            assert_eq!(
                permission_semantics_digest(provider),
                compute_permission_semantics_digest(provider)
            );
        }
        assert_ne!(
            permission_semantics_digest(HarnessProvider::Codex),
            permission_semantics_digest(HarnessProvider::Dsh)
        );
        let mut caller_assets = permissions::asset_digests();
        caller_assets.clear();
        assert!(!permissions::asset_digests().is_empty());
    }
}
