use super::*;
use devdesk_adapters::permission_profiles::{
    compile_permission_profiles, permission_contract, preflight_profile,
};

impl Workspace {
    pub(super) fn permission_runtime_digest(
        agent: &Definition,
        b: &RuntimeBinding,
    ) -> Result<String> {
        devdesk_adapters::runtime_digest(agent, b)
    }
    pub(super) fn send_permission_conversation(
        &self,
        e: &Engine,
        c: Command,
    ) -> Result<Option<Value>> {
        let fp = command_fingerprint("conversation.send", &c);
        if let Some(previous) = e.store.replay(&c.command_id, &fp)? {
            return Ok(Some(previous));
        }
        let s = e.store.snapshot()?;
        if let Some(transition) = s.governance.iter().find(|r| {
            r.r#type == "permission_switch"
                && Some(r.definition_id.as_str()) == c.target.as_deref()
                && r.data["status"] == "adopted"
                && r.data["command"]["payload"]["client_message_id"]
                    == c.payload["client_message_id"]
        }) {
            let next = transition.data["effective_session_id"]
                .as_str()
                .ok_or_else(|| err("invalid_configuration", "后继会话缺失"))?
                .to_owned();
            let result=e.store.transact(Some((&c.command_id,&fp)),&mut |s| {
                let turn=s.turns.iter().find(|t|t.session_id==next && json!(t.client_message_id)==c.payload["client_message_id"]).ok_or_else(||err("needs_reconciliation","切换输入记录缺失"))?;
                if json!(turn.input)!=c.payload["input"] {return Err(err("revision_conflict","客户端消息 ID 被不同输入复用"));}
                Ok(json!({"id":turn.id,"operation_id":turn.id,"replayed":true,"effective_session_id":next}))
            })?;
            return Ok(Some(result));
        }
        let session = s
            .sessions
            .iter()
            .find(|v| Some(v.id.as_str()) == c.target.as_deref())
            .ok_or_else(|| err("not_found", "Session 不存在"))?;
        let binding = session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少执行绑定"))?;
        if binding
            .permissions
            .as_ref()
            .and_then(|p| p.binding_lock.as_ref())
            .is_none()
        {
            return Err(err("permission_unconfigured", "会话必须使用版本化权限绑定"));
        }
        if binding.adapter_version
            != harness::managed_adapter_identity(binding.installation.key.provider)
        {
            return Err(err(
                "permission_asset_mismatch",
                "冻结适配器已变化，需要显式后继会话",
            ));
        }
        if let Ok(intent) = s.configuration(&permission_switch_id(&c.command_id)) {
            let next: RuntimeBinding = serde_json::from_value(intent.data["binding"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
            return e.send_with_permission_binding(c, next).map(Some);
        }
        let a = s.resolve_agent(&session.agent)?;
        let selection = permission_selection(&s)
            .ok()
            .ok_or_else(|| err("invalid_configuration", "缺少命名权限选择"))?;
        let lock = resolve_permission_lock(&s, &a.harness_ref, &selection)?;
        devdesk_adapters::permission_profiles::verify_permission_installation(
            &s,
            &lock,
            &binding.installation,
        )?;
        let integrations = if s
            .governance
            .iter()
            .any(|r| r.r#type == "configured_session" && r.definition_id == session.id)
        {
            binding
                .agent_configuration
                .as_ref()
                .and_then(|c| c.integration_lock.clone())
                .ok_or_else(|| err("configuration_required", "步骤会话接入锁缺失"))?
        } else {
            devdesk_core::freeze_integrations(&s, &a, session.project_id.as_deref())?
        };
        let integrations_unchanged = binding
            .agent_configuration
            .as_ref()
            .and_then(|c| c.integration_lock.as_ref())
            .is_some_and(|frozen| frozen == &integrations)
            || (integrations == IntegrationLock::default()
                && binding
                    .agent_configuration
                    .as_ref()
                    .and_then(|c| c.integration_lock.as_ref())
                    .is_none());
        if integrations_unchanged
            && Some(&lock)
                == binding
                    .permissions
                    .as_ref()
                    .and_then(|p| p.binding_lock.as_ref())
        {
            return e.execute("conversation.send", c).map(Some);
        }
        let permissions = compile_permission_profiles(
            &s,
            &lock,
            Some(ExecutionScope {
                work_type: binding.work_type,
                writable_paths: binding.writable_paths.clone(),
                read_only_paths: binding.read_only_paths.clone(),
            }),
        )?;
        let mut next = binding.clone();
        next.permissions = Some(permissions);
        if let Some(config) = &mut next.agent_configuration {
            config.integration_lock = Some(integrations);
        }
        next.configuration_digest = Self::permission_runtime_digest(
            s.definitions
                .iter()
                .find(|d| d.id == session.agent)
                .ok_or_else(|| err("not_found", "冻结 Agent 定义不存在"))?,
            &next,
        )?;
        e.send_with_permission_binding(c, next).map(Some)
    }

    pub(super) fn permission_session_status(&self, e: &Engine, sid: &str) -> Result<Value> {
        let s = e.store.snapshot()?;
        let session = s
            .sessions
            .iter()
            .find(|v| v.id == sid)
            .ok_or_else(|| err("not_found", "Session 不存在"))?;
        let pending = s.governance.iter().find(|r| {
            r.r#type == "permission_switch"
                && r.definition_id == sid
                && r.data["status"] != "adopted"
        });
        let frozen = session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.permissions.as_ref())
            .and_then(|p| p.binding_lock.as_ref());
        let desired = s
            .configuration(&session.agent)
            .ok()
            .and_then(|r| s.resolve_agent(&r.id).ok())
            .and_then(|a| permission_selection(&s).ok().map(|choice| (a, choice)))
            .map(|(a, choice)| resolve_permission_lock(&s, &a.harness_ref, &choice));
        let active = s
            .turns
            .iter()
            .any(|t| t.session_id == sid && t.status.active());
        let status = if pending.is_some_and(|r| r.data["status"] == "needs_reconciliation") {
            "needs_reconciliation"
        } else {
            match &desired {
                None => "unconfigured",
                Some(Err(_)) => "unconfigured",
                Some(Ok(v)) if frozen == Some(v) => "adopted",
                Some(Ok(_)) if active => "pending",
                _ => "ready_to_switch",
            }
        };
        Ok(
            json!({"status":status,"frozen":frozen,"desired":desired.as_ref().and_then(|r|r.as_ref().ok()),"reason":pending.and_then(|r|r.data["reason"].as_str()).or_else(||desired.as_ref().and_then(|r|r.as_ref().err()).map(|e|e.message.as_str())),"pending_command":pending.map(|r|&r.data["command"])}),
        )
    }

    pub(super) fn permission_resource_api(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        if ![
            "query.permission_resources",
            "permission.profile.create",
            "permission.profile.revise",
            "permission.profile.import",
            "permission.profile.export",
            "permission.profile.reload",
            "permission.source.select",
            "permission.source.browse",
            "permission.bindings.apply",
            "permission.binding.preflight",
        ]
        .contains(&method)
        {
            return None;
        }
        Some((|| {
            let local = if let Some(pid) = params["project_id"].as_str() {
                let e = self.active_engine(Some(pid))?;
                self.sync_configuration(&e)?;
                Some(e)
            } else {
                None
            };
            let s = if let Some(e) = &local {
                e.store.snapshot()?
            } else {
                self.global.snapshot()?
            };
            match method {
                "query.permission_resources" => {
                    let installations = self.global.runtime_installations()?;
                    let targets:Vec<_>=s.governance.iter().filter(|r|["harness","sandbox"].contains(&r.r#type.as_str()) && r.immutable && template_record_current(&s,r))
                        .filter_map(|r| {
                            let kind=if r.r#type=="harness" {PermissionTargetKind::Harness}else{PermissionTargetKind::Sandbox};
                            let target=permission_target(&s,kind,&r.id).ok()?;
                            let contract=permission_contract(&s,&target);
                            let active=active_permission_binding(&s,&target);
                            let registered: Vec<_> = installations.iter().filter(|installation| {
                                r.r#type == "harness"
                                    && json!(installation.key.provider) == r.data["provider"]
                                    && r.data["recipe_digest"].as_str().is_some_and(|digest| {
                                        devdesk_adapters::permissions::installation_recipe(installation) == digest
                                    })
                            }).collect();
                            let materializations:Vec<_>=s.governance.iter().filter(|m|m.r#type=="harness_materialization" && m.definition_id==r.id).map(|m|&m.data).collect();
                            let allowed_permission_ids: Vec<_> = s.governance.iter().filter(|p|p.r#type=="permission_profile" && template_record_current(&s,p))
                                .filter_map(|p|p.permission_profile().ok().filter(|profile|preflight_profile(&s,&target,profile).is_ok()).map(|profile|profile.id)).collect();
                            Some(json!({"target":target,"name":r.name,"version":r.version,"provider":r.data["provider"],"runtime_version":r.data["runtime_version"],
                                "platforms":if r.r#type=="sandbox" {r.data["platforms"].clone()}else{json!(["linux/aarch64","linux/x86_64"])},"recipe_digest":r.data["recipe_digest"],"registered_installations":registered,"materializations":materializations,
                                "supported":contract.is_ok(),"enforcement_contract":contract.as_ref().ok(),"unsupported_reason":contract.as_ref().err().map(|e|&e.message),
                                "allowed_permission_ids":allowed_permission_ids,"active":active.map(|a|json!({"revision":a.version,"binding_revision_id":a.data["binding_revision_id"]}))}))
                        }).collect();
                    let records = |kind: &str| {
                        s.governance
                            .iter()
                            .filter(|r| r.r#type == kind && template_record_current(&s, r))
                            .collect::<Vec<_>>()
                    };
                    Ok(
                        json!({"profiles":records("permission_profile").iter().map(|r|r.permission_profile()).collect::<Result<Vec<_>>>()?,
                        "bindings":records("permission_binding"),"origins":records("permission_origin").into_iter().chain(records("permission_file_origin")).collect::<Vec<_>>(),"targets":targets,
                        "source":self.global.permission_preset_source()?,"default_sandbox_ref":current_template_defaults(&s).map(|d| d["sandbox_ref"].clone())}),
                    )
                }
                "permission.profile.export" => permission_profile_export(
                    &s,
                    params["id"]
                        .as_str()
                        .ok_or_else(|| err("invalid_params", "需要 profile id"))?,
                ),
                "permission.profile.reload" => {
                    let c: Command = serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    self.global.reload_permission_presets(&c)
                }
                "permission.source.select" => {
                    let c: Command = serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    self.global.select_permission_preset_source(&c)
                }
                "permission.source.browse" => {
                    let source = self.global.permission_preset_source()?;
                    let path = match params.get("path") {
                        None => PathBuf::from(self.global.root()),
                        Some(Value::String(path)) => PathBuf::from(path),
                        _ => return Err(err("invalid_params", "目录路径必须是字符串")),
                    };
                    if !path.is_absolute() {
                        return Err(err("invalid_params", "需要绝对目录"));
                    }
                    let path = std::fs::canonicalize(path)
                        .map_err(|e| err("invalid_params", format!("无法打开文件夹：{e}")))?;
                    let mut directories = Vec::new();
                    for entry in std::fs::read_dir(&path)
                        .map_err(|e| err("invalid_params", format!("无法读取文件夹：{e}")))?
                    {
                        let entry = entry.map_err(|e| err("invalid_params", e.to_string()))?;
                        if entry.file_type().is_ok_and(|kind| kind.is_dir())
                            && let Some(name) = entry.file_name().to_str()
                        {
                            directories.push(json!({"name":name,"path":entry.path()}));
                        }
                    }
                    directories.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                    let mut locations = vec![
                        json!({"name":"数据目录","path":self.global.root()}),
                        json!({"name":"内置预设","path":source.builtin_path}),
                        json!({"name":"根目录","path":Path::new(&path).ancestors().last()}),
                    ];
                    if let Some(home) = std::env::var_os("HOME") {
                        locations.insert(0, json!({"name":"主目录","path":PathBuf::from(home)}));
                    }
                    Ok(
                        json!({"path":path,"parent":path.parent(),"directories":directories,"locations":locations}),
                    )
                }
                "permission.binding.preflight" => {
                    if params.get("selection").is_some() {
                        let selection: ExecutionPermissionSelection =
                            serde_json::from_value(params["selection"].clone())
                                .map_err(|e| err("invalid_params", e.to_string()))?;
                        let lock = resolve_permission_lock(
                            &s,
                            params["harness_ref"]
                                .as_str()
                                .ok_or_else(|| err("invalid_params", "需要 Harness 修订"))?,
                            &selection,
                        )?;
                        let scope: Option<ExecutionScope> = params
                            .get("scope")
                            .map(|v| {
                                serde_json::from_value(v.clone())
                                    .map_err(|e| err("invalid_params", e.to_string()))
                            })
                            .transpose()?;
                        let checked = scope.is_some();
                        let resolved = compile_permission_profiles(&s, &lock, scope)?;
                        Ok(
                            json!({"resolved":resolved,"configuration_digest":configuration_digest(&json!(resolved))?,"scope_checked":checked,"execution_scope_required":!checked,"side_effects":false}),
                        )
                    } else {
                        let target: PermissionTargetRef =
                            serde_json::from_value(params["target"].clone())
                                .map_err(|e| err("invalid_params", e.to_string()))?;
                        let p = s.configuration(
                            params["permission_revision_id"]
                                .as_str()
                                .ok_or_else(|| err("invalid_params", "需要权限修订"))?,
                        )?;
                        preflight_profile(&s, &target, &p.permission_profile()?)
                    }
                }
                "permission.bindings.apply" => {
                    let mut c: Command = serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    let fp = command_fingerprint(method, &c);
                    let first_target = c.payload["bindings"]
                        .as_array()
                        .and_then(|v| v.first())
                        .and_then(|v| v["target"]["revision_id"].as_str())
                        .ok_or_else(|| err("invalid_params", "需要非空 bindings"))?;
                    let original_scope = &s.configuration(first_target)?.scope;
                    let replay_store: &dyn EngineeringStore = if original_scope.is_global() {
                        self.global.as_ref()
                    } else if original_scope.is_feature() {
                        return Err(err("invalid_scope", "Feature 不能持有可变目标默认"));
                    } else {
                        local
                            .as_ref()
                            .filter(|e| {
                                e.store.project_id().as_deref() == original_scope.project_id()
                            })
                            .ok_or_else(|| err("invalid_scope", "需要所属项目"))?
                            .store
                            .as_ref()
                    };
                    if let Some(previous) = replay_store.replay(&c.command_id, &fp)? {
                        return Ok(previous);
                    }
                    let bindings = c.payload["bindings"]
                        .as_array_mut()
                        .ok_or_else(|| err("invalid_params", "需要 bindings"))?;
                    let mut binding_scope = None;
                    for b in bindings {
                        let target: PermissionTargetRef =
                            serde_json::from_value(b["target"].clone())
                                .map_err(|e| err("invalid_params", e.to_string()))?;
                        let profile: PermissionProfile = s
                            .configuration(
                                b["permission_revision_id"]
                                    .as_str()
                                    .ok_or_else(|| err("invalid_params", "需要权限修订"))?,
                            )?
                            .permission_profile()?;
                        preflight_profile(&s, &target, &profile)?;
                        let scope = s.configuration(&target.revision_id)?.scope.clone();
                        if binding_scope
                            .as_ref()
                            .is_some_and(|previous| previous != &scope)
                        {
                            return Err(err(
                                "invalid_scope",
                                "同一原子批次必须属于同一配置库作用域",
                            ));
                        }
                        binding_scope = Some(scope);
                        b["enforcement_contract"] = json!(permission_contract(&s, &target)?);
                    }
                    match binding_scope {
                        Some(scope) if scope.is_feature() => {
                            Err(err("invalid_scope", "Feature 作用域不能作为可变目标默认"))
                        }
                        Some(scope) if !scope.is_global() => local
                            .as_ref()
                            .filter(|e| e.store.project_id().as_deref() == scope.project_id())
                            .ok_or_else(|| err("invalid_scope", "绑定需要明确所属项目"))?
                            .execute_resolved(method, c, &fp),
                        _ => self.configuration.execute_resolved(method, c, &fp),
                    }
                }
                _ => self.configuration.execute(
                    method,
                    serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                ),
            }
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_folder_browsing_selection_and_builtin_recovery_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let custom = root.join("团队 预设");
        std::fs::create_dir(&custom).unwrap();
        std::fs::write(root.join("not-a-folder.json"), "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&custom, root.join("linked-folder")).unwrap();
        let listing = w.dispatch("permission.source.browse", json!({})).unwrap();
        assert_eq!(listing["path"], json!(root));
        assert!(
            listing["directories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["name"] == "团队 预设")
        );
        assert!(
            listing["directories"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d["name"] != "not-a-folder.json" && d["name"] != "linked-folder")
        );
        assert!(
            w.dispatch("permission.source.browse", json!({"path":"relative"}))
                .is_err()
        );
        assert!(
            w.dispatch("permission.source.browse", json!({"path":false}))
                .is_err()
        );
        let catalog = w.dispatch("query.permission_resources", json!({})).unwrap();
        let bindings = catalog["bindings"].clone();
        assert_eq!(catalog["source"]["is_builtin"], true);
        let c = json!({"command_id":id(),"payload":{"path":custom}});
        let selected = w.dispatch("permission.source.select", c.clone()).unwrap();
        assert_eq!(selected["path"], json!(custom));
        assert_eq!(selected["is_builtin"], false);
        assert_eq!(w.dispatch("permission.source.select", c).unwrap(), selected);
        drop(w);

        // A moved custom folder must not prevent opening settings to recover it.
        std::fs::remove_dir(&custom).unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        assert_eq!(
            w.dispatch("query.permission_resources", json!({})).unwrap()["source"]["path"],
            json!(custom)
        );
        assert!(
            w.dispatch(
                "permission.profile.reload",
                json!({"command_id":id(),"payload":{}})
            )
            .is_err()
        );
        w.dispatch(
            "permission.profile.reload",
            json!({"command_id":id(),"payload":{"builtin":true}}),
        )
        .unwrap();
        let restored = w.dispatch("query.permission_resources", json!({})).unwrap();
        assert_eq!(restored["source"], catalog["source"]);
        assert_eq!(restored["bindings"], bindings);
    }

    #[test]
    fn send_adopts_the_new_target_binding_in_a_successor_without_mutating_history() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Binding adoption"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let apply = |profile: &str, expected: u64| {
            let s = w.global.snapshot().unwrap();
            let mut changes = vec![
                json!({"target":permission_target(&s,PermissionTargetKind::Harness,DEFAULT_HARNESS_ID).unwrap(),"permission_revision_id":profile,"expected_revision":expected}),
            ];
            if false {
                changes.push(json!({"target":permission_target(&s,PermissionTargetKind::Sandbox,DEFAULT_PERMISSION_SANDBOX).unwrap(),"permission_revision_id":"builtin-permission-sandbox-full-access:1","expected_revision":0}));
            }
            w.dispatch(
                "permission.bindings.apply",
                json!({"command_id":id(),"payload":{"bindings":changes}}),
            )
            .unwrap()
        };
        apply("builtin-permission-harness-full-access:1", 1);
        let selection = ExecutionPermissionSelection {
            sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
            harness: PermissionSelection::FollowTarget,
            sandbox: PermissionSelection::FollowTarget,
        };
        let data = json!(
            w.global
                .snapshot()
                .unwrap()
                .configuration(&default_configured_agent_id("analyst"))
                .unwrap()
                .refs
        );
        // This fixture authors an explicit binding without an integration lock.
        // Keep its capability native-only; platform input readers are frozen by
        // actual Workflow sessions and belong to their separate integration tests.
        let sources = w.global.snapshot().unwrap();
        let capability_source = sources
            .configuration(data["capability_ref"].as_str().unwrap())
            .unwrap()
            .clone();
        let mut refs = capability_source.refs.clone();
        refs.insert("tools".into(), json!([DEFAULT_NATIVE_TOOL_ID]));
        let mut payload = json!({"command_id":id(),"payload":{"name":"Native permission fixture","scope":"global","data":capability_source.data}});
        for (key, value) in &refs {
            payload["payload"][key] = value.clone();
        }
        let capability = w.dispatch("capability.create", payload).unwrap();
        let agent=w.dispatch("agent.create",json!({"command_id":id(),"payload":{"name":"Followed runtime analyst","role_ref":data["role_ref"],"capability_ref":capability["id"],"data":{}}})).unwrap()["id"].as_str().unwrap().to_owned();
        let e = w.active_engine(Some(&project)).unwrap();
        w.sync_configuration(&e).unwrap();
        let state = e.store.snapshot().unwrap();
        let lock = resolve_permission_lock(&state, DEFAULT_HARNESS_ID, &selection).unwrap();
        let permissions = compile_permission_profiles(
            &state,
            &lock,
            Some(ExecutionScope {
                work_type: WorkType::Discussion,
                writable_paths: vec![],
                read_only_paths: vec![],
            }),
        )
        .unwrap();
        let binding = RuntimeBinding {
            model_connection: None,
            model_profile_digest: None,
            agent_configuration: None,
            model_parameters: Default::default(),
            permissions: Some(permissions),
            installation: RuntimeInstallation {
                id: "explicit-port-fixture".into(),
                key: RuntimeKey {
                    provider: HarnessProvider::Codex,
                    version: format!(
                        "{}+recipe.{}",
                        devdesk_adapters::runtime_preparation::CODEX_VERSION,
                        devdesk_adapters::runtime_preparation::default_codex_recipe_digest()
                    ),
                    os: "linux".into(),
                    arch: "aarch64".into(),
                },
                artifact: RuntimeArtifact::DockerImage {
                    image_id: format!("sha256:{}", "a".repeat(64)),
                },
                source: "injected port fixture, no Docker/model".into(),
                license: "test".into(),
            },
            adapter_version: harness::managed_adapter_identity(HarnessProvider::Codex),
            model: "fixture".into(),
            tool_policy: ToolPolicy::ReadOnly,
            writable_paths: vec![],
            read_only_paths: vec![],
            work_type: WorkType::Discussion,
            configuration_digest: "a".repeat(64),
            credential_ref: None,
        };
        let original = e
            .execute(
                "session.open",
                Command {
                    command_id: id(),
                    target: Some(project.clone()),
                    expected_revision: Some(state.projects[0].revision),
                    payload: json!({"agent":agent,"binding":binding}),
                },
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        apply("builtin-permission-harness-read-only:1", 2);
        let params = json!({"project_id":project,"command_id":id(),"target":original,"expected_revision":1,"payload":{"input":"Use the new permissions","client_message_id":"one-message"}});
        let sent = w.dispatch("conversation.send", params.clone()).unwrap();
        assert_ne!(sent["effective_session_id"], original);
        assert_eq!(
            w.dispatch("conversation.send", params.clone()).unwrap(),
            sent
        );
        let mut duplicate = params.clone();
        duplicate["command_id"] = json!(id());
        let replay = w.dispatch("conversation.send", duplicate.clone()).unwrap();
        assert_eq!(replay["id"], sent["id"]);
        assert_eq!(replay["effective_session_id"], sent["effective_session_id"]);
        assert_eq!(w.dispatch("conversation.send", duplicate).unwrap(), replay);
        let snapshot = e.store.snapshot().unwrap();
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|s| s.id == original)
                .unwrap()
                .runtime_binding
                .as_ref(),
            Some(&binding)
        );
        let next = snapshot
            .sessions
            .iter()
            .find(|s| s.id == sent["effective_session_id"].as_str().unwrap())
            .unwrap();
        let b = next.runtime_binding.as_ref().unwrap();
        let same = json!({"project_id":project,"command_id":id(),"target":next.id,"expected_revision":next.revision,"payload":params["payload"]});
        let replay = w.dispatch("conversation.send", same.clone()).unwrap();
        assert_eq!(w.dispatch("conversation.send", same).unwrap(), replay);
        assert_eq!(
            b.permissions.as_ref().unwrap().harness.policy.filesystem,
            FilesystemPermission::ReadOnly
        );
        assert_eq!(
            b.configuration_digest,
            Workspace::permission_runtime_digest(
                snapshot.definitions.iter().find(|d| d.id == agent).unwrap(),
                b
            )
            .unwrap()
        );
        let view = w
            .dispatch("query.session", json!({"project_id":project,"id":next.id}))
            .unwrap();
        assert_eq!(view["permission_status"]["status"], "adopted");
    }
    #[test]
    fn project_target_bindings_remain_in_their_own_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Local permission targets"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let h=w.dispatch("harness.create",json!({"project_id":project,"command_id":id(),"payload":{"name":"Project Codex","scope":format!("project:{project}"),"model_providers":["builtin-model-provider-openai:1"],"data":{"provider":"codex","runtime_version":devdesk_adapters::runtime_preparation::CODEX_VERSION,"recipe_digest":devdesk_adapters::runtime_preparation::default_codex_recipe_digest(),"capabilities":[]}}})).unwrap();
        let catalog = w
            .dispatch("query.permission_resources", json!({"project_id":project}))
            .unwrap();
        let target = catalog["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["target"]["revision_id"] == h["id"])
            .unwrap()["target"]
            .clone();
        let params = json!({"project_id":project,"command_id":id(),"payload":{"bindings":[{"target":target,"permission_revision_id":"builtin-permission-harness-read-only:1","expected_revision":0}]}});
        let saved = w
            .dispatch("permission.bindings.apply", params.clone())
            .unwrap();
        assert_eq!(
            w.dispatch("permission.bindings.apply", params).unwrap(),
            saved
        );
        assert!(
            !w.dispatch("query.permission_resources", json!({})).unwrap()["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["target"] == target)
        );
        assert_eq!(
            w.dispatch("query.permission_resources", json!({"project_id":project}))
                .unwrap()["targets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["target"] == target)
                .unwrap()["active"]["revision"],
            1
        );
    }
    #[test]
    fn profile_binding_api_replays_the_original_request_and_rejects_unsupported_rules() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let catalog = w.dispatch("query.permission_resources", json!({})).unwrap();
        let target = catalog["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["provider"] == "codex" && r["supported"] == true)
            .unwrap()["target"]
            .clone();
        let preflight = w
            .dispatch(
                "permission.binding.preflight",
                json!({"target":target,"permission_revision_id":"builtin-permission-harness-read-only:1"}),
            )
            .unwrap();
        assert_eq!(preflight["scope_checked"], false);
        let request = json!({"command_id":id(),"payload":{"bindings":[{"target":target,"permission_revision_id":"builtin-permission-harness-read-only:1","expected_revision":1}]}});
        let first = w
            .dispatch("permission.bindings.apply", request.clone())
            .unwrap();
        assert_eq!(
            w.dispatch("permission.bindings.apply", request).unwrap(),
            first
        );
        assert_eq!(w.dispatch("permission.profile.create",json!({"command_id":id(),"payload":{
            "identity":"must-not-be-created","name":"Invalid envelope","target_kind":"harness",
            "policy":catalog["profiles"][0]["data"]["policy"],"unknown":true
        }})).unwrap_err().code,"invalid_params");
        let invalid=w.dispatch("permission.profile.create",json!({"command_id":id(),"payload":{"identity":"unsupported-test","name":"Unsupported","target_kind":"harness","policy":{"filesystem":{"mode":"read_only"},"network":{"mode":"enabled"},"tools":{"mode":"restricted","allow":["bash"],"deny":[]},"approval":"never"}}})).unwrap();
        assert_eq!(
            w.dispatch(
                "permission.binding.preflight",
                json!({"target":target,"permission_revision_id":format!("{}:{}",invalid["identity"].as_str().unwrap(),invalid["revision"].as_u64().unwrap())})
            )
            .unwrap_err()
            .code,
            "permission_unsupported"
        );
        let restored = w.dispatch("query.permission_resources", json!({})).unwrap();
        assert_eq!(
            restored["targets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["target"] == target)
                .unwrap()["active"]["revision"],
            2
        );
    }
}
