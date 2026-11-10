use super::*;
use devdesk_adapters::permission_profiles::{
    compile_permission_profiles, permission_contract, preflight_profile,
};

impl Workspace {
    pub(super) fn renew_unstarted_feature_permissions(&self, e: &Engine) -> Result<()> {
        e.store.transact(None, &mut |s| {
            let locks: Vec<_> = s.governance.iter().filter(|r| ["feature_permission_lock", "feature_permission_renewal"].contains(&r.kind.as_str())
                && !s.governance.iter().any(|next| ["feature_permission_lock", "feature_permission_renewal"].contains(&next.kind.as_str()) && next.definition_id == r.definition_id && next.version > r.version)).cloned().collect();
            for old in locks {
                let Some(feature)=old.data["feature_id"].as_str() else { continue; };
                if s.sessions.iter().any(|v|v.feature_id.as_deref()==Some(feature))
                    || s.attempts.iter().any(|v|v.feature_id==feature) { continue; }
                let Ok(lock)=serde_json::from_value::<VersionedPermissionLock>(old.data["lock"].clone()) else { continue; };
                if compile_permission_profiles(s,&lock,None).is_ok() { continue; }
                let renewal=(||->Result<VersionedPermissionLock>{
                    Ok(VersionedPermissionLock{
                        harness:devdesk_adapters::permission_profiles::renew_permission_bound(s,&lock.harness)?,
                        sandbox:devdesk_adapters::permission_profiles::renew_permission_bound(s,&lock.sandbox)?
                    })
                })();
                let Ok(next_lock)=renewal else { continue; };
                // This repair is limited to a feature that has never opened an execution session.
                // Its policy, profile and exact targets remain identical; only the contract is revalidated.
                if next_lock.harness.binding.enforcement_contract==lock.harness.binding.enforcement_contract
                    && next_lock.sandbox.binding.enforcement_contract==lock.sandbox.binding.enforcement_contract { continue; }
                for bound in [&next_lock.harness,&next_lock.sandbox] {
                    if s.governance.iter().any(|r|r.id==bound.binding_revision_id) { continue; }
                    let data=json!(bound.binding);
                    s.governance.push(ConfigurationRecord{id:bound.binding_revision_id.clone(),kind:"permission_binding".into(),definition_id:bound.binding_revision_id.clone(),scope:Scope::Global,name:bound.profile.name.clone(),version:1,supersedes:None,content_hash:configuration_hash(&data),data,immutable:true,revision:1,created_at:0});
                }
                let mut next=old.clone(); next.kind="feature_permission_renewal".into(); next.version+=1; next.supersedes=Some(old.id.clone());
                next.data["lock"]=json!(next_lock);
                next.data["renewal"]=json!({"source":old.id,"reason":"unstarted_feature_contract_revalidation","policy_changed":false,"target_changed":false});
                next.content_hash=configuration_hash(&next.data); next.id=format!("feature-permission-renewal:{}",next.content_hash);
                s.governance.push(next);
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }
    pub(super) fn permission_runtime_digest(
        agent: &Definition,
        b: &RuntimeBinding,
    ) -> Result<String> {
        let mut configuration = json!({"installation":b.installation,"adapter":b.adapter_version,"agent":agent,"agent_configuration":b.agent_configuration,
            "model":b.model,"model_connection":b.model_connection,"model_profile_digest":b.model_profile_digest,"policy":b.tool_policy,"writable_paths":b.writable_paths,"read_only_paths":b.read_only_paths,"work_type":b.work_type,
            "sandbox":"docker-worker/v1","credential_ref":b.credential_ref,"permissions":b.permissions});
        if !b.model_parameters.is_empty() {
            configuration["model_parameters"] = json!(b.model_parameters);
        }
        configuration_digest(&configuration)
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
            r.kind == "permission_switch"
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
            return Ok(None);
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
        let a: AgentComposition = s.configuration(&session.agent)?.decode()?;
        let selection = agent_permission_selection(&s, &session.agent, &a)
            .ok_or_else(|| err("invalid_configuration", "缺少命名权限选择"))?;
        let lock = resolve_permission_lock(&s, &a.harness_ref, &selection)?;
        devdesk_adapters::permission_profiles::verify_permission_installation(
            &s,
            &lock,
            &binding.installation,
        )?;
        let integrations = devdesk_core::integration_feature_lock(
            &s,
            &session.agent,
            session.feature_id.as_deref(),
        )
        .map(Ok)
        .unwrap_or_else(|| {
            devdesk_core::freeze_integrations(&s, &a, session.project_id.as_deref())
        })?;
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
            r.kind == "permission_switch" && r.definition_id == sid && r.data["status"] != "adopted"
        });
        let frozen = session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.permissions.as_ref())
            .and_then(|p| p.binding_lock.as_ref());
        let desired = s
            .configuration(&session.agent)
            .ok()
            .and_then(|r| r.decode::<AgentComposition>().ok())
            .and_then(|a| {
                agent_permission_selection(&s, &session.agent, &a).map(|choice| (a, choice))
            })
            .map(|(a, choice)| resolve_permission_lock(&s, &a.harness_ref, &choice));
        let active = s
            .turns
            .iter()
            .any(|t| t.session_id == sid && t.status.active());
        let status = if pending.is_some_and(|r| r.data["status"] == "needs_reconciliation") {
            "needs_reconciliation"
        } else {
            match &desired {
                None => "legacy_fixed",
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
            "permission.source.register",
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
                    let targets:Vec<_>=s.governance.iter().filter(|r|["harness","sandbox"].contains(&r.kind.as_str()) && r.immutable)
                        .filter_map(|r| {
                            let kind=if r.kind=="harness" {PermissionTargetKind::Harness}else{PermissionTargetKind::Sandbox};
                            let target=permission_target(&s,kind,&r.id).ok()?;
                            let contract=permission_contract(&s,&target);
                            let active=active_permission_binding(&s,&target);
                            let registered:Vec<_>=installations.iter().filter(|i|r.kind=="harness" && json!(i.key.provider)==r.data["provider"] &&
                                (json!(i.key.version)==r.data["runtime_version"] || (r.data["recipe_digest"].is_string() && i.key.version.starts_with(&format!("{}+docker.",r.data["runtime_version"].as_str().unwrap_or("")))))).collect();
                            let materializations:Vec<_>=s.governance.iter().filter(|m|m.kind=="harness_materialization" && m.definition_id==r.id).map(|m|&m.data).collect();
                            let allowed_permission_ids: Vec<_> = s.governance.iter().filter(|p|p.kind=="permission_profile")
                                .filter_map(|p|p.decode::<PermissionProfile>().ok().filter(|profile|preflight_profile(&s,&target,profile).is_ok()).map(|profile|profile.id)).collect();
                            Some(json!({"target":target,"name":r.name,"version":r.version,"provider":r.data["provider"],"runtime_version":r.data["runtime_version"],
                                "platforms":if r.kind=="sandbox" {r.data["platforms"].clone()}else{json!(["linux/aarch64","linux/x86_64"])},"recipe_digest":r.data["recipe_digest"],"registered_installations":registered,"materializations":materializations,
                                "supported":contract.is_ok(),"enforcement_contract":contract.as_ref().ok(),"unsupported_reason":contract.as_ref().err().map(|e|&e.message),
                                "allowed_permission_ids":allowed_permission_ids,"active":active.map(|a|json!({"revision":a.version,"binding_revision_id":a.data["binding_revision_id"]}))}))
                        }).collect();
                    let records = |kind: &str| {
                        s.governance
                            .iter()
                            .filter(|r| r.kind == kind)
                            .collect::<Vec<_>>()
                    };
                    let agents:Vec<_>=records("agent").into_iter().filter(|r|!s.governance.iter().any(|n|n.kind=="agent" && n.definition_id==r.definition_id && n.version>r.version))
                        .filter_map(|r|r.decode::<AgentComposition>().ok().map(|a| {
                            let selection=agent_permission_selection(&s,&r.id,&a);
                            json!({"id":r.id,"name":r.name,"harness_ref":a.harness_ref,"sandbox_ref":selection.as_ref().map(|v|v.sandbox_ref.as_str()),
                                "harness_mode":selection.as_ref().map(|v|match v.harness {PermissionSelection::FollowTarget=>"follow_target",_=>"pinned"}).unwrap_or("legacy_fixed"),
                                "sandbox_mode":selection.as_ref().map(|v|match v.sandbox {PermissionSelection::FollowTarget=>"follow_target",_=>"pinned"}).unwrap_or("legacy_fixed")})
                        })).collect();
                    Ok(
                        json!({"profiles":records("permission_profile").iter().map(|r|&r.data).collect::<Vec<_>>(),
                        "bindings":records("permission_binding"),"origins":records("permission_origin").into_iter().chain(records("permission_file_origin")).collect::<Vec<_>>(),"migrations":records("permission_migration").into_iter().chain(records("permission_migration_renewal")).collect::<Vec<_>>(),"targets":targets,"agents":agents,
                        "sources":self.global.permission_preset_sources()?,"default_sandbox_ref":DEFAULT_PERMISSION_SANDBOX,
                        "legacy_agents":s.governance.iter().filter(|r|r.kind=="agent" && r.data.get("permission_selection").is_none()).map(|r|json!({"id":r.id,"name":r.name,"mode":"legacy_fixed"})).collect::<Vec<_>>()}),
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
                "permission.source.register" => {
                    let c: Command = serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    if c.command_id.is_empty() {
                        return Err(err("invalid_params", "command_id 不能为空"));
                    }
                    let fp = command_fingerprint(method, &c);
                    if let Some(previous) = self.global.replay(&c.command_id, &fp)? {
                        return Ok(previous);
                    }
                    let path = Path::new(
                        c.payload["path"]
                            .as_str()
                            .ok_or_else(|| err("invalid_params", "需要绝对目录"))?,
                    );
                    if !path.is_absolute()
                        || path
                            .symlink_metadata()
                            .map_err(|e| err("invalid_params", e.to_string()))?
                            .file_type()
                            .is_symlink()
                    {
                        return Err(err("invalid_params", "预设来源必须是明确的普通绝对目录"));
                    }
                    let path = std::fs::canonicalize(path)
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    if !path.is_dir() {
                        return Err(err("invalid_params", "预设来源不是目录"));
                    }
                    self.global.transact(Some((&c.command_id, &fp)), &mut |s| {
                        let data = json!({"path":path});
                        let key = format!("permission-source:{}", configuration_hash(&data));
                        if !s.governance.iter().any(|r| r.id == key) {
                            s.governance.push(ConfigurationRecord {
                                id: key.clone(),
                                kind: "permission_source".into(),
                                definition_id: key.clone(),
                                scope: Scope::Global,
                                name: "权限预设目录".into(),
                                version: 1,
                                supersedes: None,
                                content_hash: configuration_hash(&data),
                                data,
                                immutable: true,
                                revision: 1,
                                created_at: 0,
                            });
                        }
                        Ok(json!({"id":key,"path":path}))
                    })
                }
                "permission.binding.preflight" => {
                    if params.get("selection").is_some() {
                        let selection: AgentPermissionSelection =
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
                        preflight_profile(&s, &target, &p.decode()?)
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
                    let replay_store: &dyn EngineeringStore = match original_scope {
                        Scope::Global => self.global.as_ref(),
                        Scope::Project { project_id } => local
                            .as_ref()
                            .filter(|e| {
                                e.store.project_id().as_deref() == Some(project_id.as_str())
                            })
                            .ok_or_else(|| err("invalid_scope", "需要所属项目"))?
                            .store
                            .as_ref(),
                        Scope::Feature { .. } => {
                            return Err(err("invalid_scope", "Feature 不能持有可变目标默认"));
                        }
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
                            .decode()?;
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
                        Some(Scope::Project { project_id }) => local
                            .as_ref()
                            .filter(|e| {
                                e.store.project_id().as_deref() == Some(project_id.as_str())
                            })
                            .ok_or_else(|| err("invalid_scope", "绑定需要明确所属项目"))?
                            .execute_resolved(method, c, &fp),
                        Some(Scope::Feature { .. }) => {
                            Err(err("invalid_scope", "Feature 作用域不能作为可变目标默认"))
                        }
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
            if expected == 0 {
                changes.push(json!({"target":permission_target(&s,PermissionTargetKind::Sandbox,DEFAULT_PERMISSION_SANDBOX).unwrap(),"permission_revision_id":"permission-sandbox-full-access-v2-r1","expected_revision":0}));
            }
            w.dispatch(
                "permission.bindings.apply",
                json!({"command_id":id(),"payload":{"bindings":changes}}),
            )
            .unwrap()
        };
        apply("permission-harness-full-access-v2-r1", 0);
        let selection = AgentPermissionSelection {
            sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
            harness: PermissionSelection::FollowTarget,
            sandbox: PermissionSelection::FollowTarget,
        };
        let mut data = w
            .global
            .snapshot()
            .unwrap()
            .configuration(&default_configured_agent_id("analyst"))
            .unwrap()
            .data
            .clone();
        data["permission_selection"] = json!(selection);
        let agent=w.dispatch("agent.create",json!({"command_id":id(),"payload":{"name":"Followed runtime analyst","data":data}})).unwrap()["id"].as_str().unwrap().to_owned();
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
                    version: "0.167.0".into(),
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
        apply("permission-harness-read-only-v2-r1", 1);
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
        let h=w.dispatch("harness.create",json!({"project_id":project,"command_id":id(),"payload":{"name":"Project Codex","scope":{"kind":"project","project_id":project},"data":{"provider":"codex","runtime_version":"0.167.0","model_providers":["openai"],"capabilities":[]}}})).unwrap();
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
        let params = json!({"project_id":project,"command_id":id(),"payload":{"bindings":[{"target":target,"permission_revision_id":"permission-harness-read-only-v2-r1","expected_revision":0}]}});
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
        let preflight=w.dispatch("permission.binding.preflight",json!({"target":target,"permission_revision_id":"permission-harness-read-only-v2-r1"})).unwrap();
        assert_eq!(preflight["scope_checked"], false);
        let request = json!({"command_id":id(),"payload":{"bindings":[{"target":target,"permission_revision_id":"permission-harness-read-only-v2-r1","expected_revision":0}]}});
        let first = w
            .dispatch("permission.bindings.apply", request.clone())
            .unwrap();
        assert_eq!(
            w.dispatch("permission.bindings.apply", request).unwrap(),
            first
        );
        assert_eq!(w.dispatch("permission.profile.create",json!({"command_id":id(),"payload":{
            "identity":"must-not-be-created","name":"Invalid envelope","target_kind":"harness",
            "policy":catalog["profiles"][0]["policy"],"unknown":true
        }})).unwrap_err().code,"invalid_params");
        let invalid=w.dispatch("permission.profile.create",json!({"command_id":id(),"payload":{"identity":"unsupported-test","name":"Unsupported","target_kind":"harness","policy":{"filesystem":{"mode":"read_only"},"network":{"mode":"enabled"},"tools":{"mode":"restricted","allow":["bash"],"deny":[]},"approval":"never"}}})).unwrap();
        assert_eq!(
            w.dispatch(
                "permission.binding.preflight",
                json!({"target":target,"permission_revision_id":invalid["id"]})
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
            1
        );
    }
}

#[cfg(test)]
mod contract_renewal_tests {
    use super::*;
    #[test]
    fn unstarted_feature_renews_only_the_contract_and_preserves_original_lock() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let pid = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Contract renewal"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let e = w.active_engine(Some(&pid)).unwrap();
        let state = e.store.snapshot().unwrap();
        let agent = default_configured_agent_id("analyst");
        let a: AgentComposition = state.configuration(&agent).unwrap().decode().unwrap();
        let selection = agent_permission_selection(&state, &agent, &a).unwrap();
        let mut lock = resolve_permission_lock(&state, &a.harness_ref, &selection).unwrap();
        lock.harness.binding.enforcement_contract =
            "codex-native-v1:codex-app-server/v1+build.0000000000000000".into();
        lock.sandbox.binding.enforcement_contract = format!("docker-worker/v1:{}", "0".repeat(64));
        for bound in [&mut lock.harness, &mut lock.sandbox] {
            bound.binding_hash = permission_content_hash(&bound.binding);
            bound.binding_revision_id = format!("legacy-binding-{}", bound.binding_hash);
        }
        let fid = id();
        let record_id = id();
        e.store
            .transact(None, &mut |s| {
                for bound in [&lock.harness, &lock.sandbox] {
                    let data = json!(bound.binding);
                    s.governance.push(ConfigurationRecord {
                        id: bound.binding_revision_id.clone(),
                        kind: "permission_binding".into(),
                        definition_id: bound.binding_revision_id.clone(),
                        scope: Scope::Global,
                        name: "Legacy grant".into(),
                        version: 1,
                        supersedes: None,
                        content_hash: configuration_hash(&data),
                        data,
                        immutable: true,
                        revision: 1,
                        created_at: 0,
                    });
                }
                let data = json!({"feature_id":fid,"agent_revision_id":agent,"lock":lock});
                s.governance.push(ConfigurationRecord {
                    id: record_id.clone(),
                    kind: "feature_permission_lock".into(),
                    definition_id: format!("{fid}:{agent}"),
                    scope: Scope::Global,
                    name: "Frozen permission".into(),
                    version: 1,
                    supersedes: None,
                    content_hash: configuration_hash(&data),
                    data,
                    immutable: true,
                    revision: 1,
                    created_at: 0,
                });
                Ok(Value::Null)
            })
            .unwrap();
        let original = e
            .store
            .snapshot()
            .unwrap()
            .configuration(&record_id)
            .unwrap()
            .clone();
        assert!(compile_permission_profiles(&e.store.snapshot().unwrap(), &lock, None).is_err());
        w.renew_unstarted_feature_permissions(&e).unwrap();
        let after = e.store.snapshot().unwrap();
        let renewed = frozen_permission_lock(&after, &fid, &agent).unwrap();
        assert_eq!(renewed.harness.profile, lock.harness.profile);
        assert_eq!(renewed.harness.binding.target, lock.harness.binding.target);
        compile_permission_profiles(&after, &renewed, None).unwrap();
        assert_eq!(
            json!(after.configuration(&record_id).unwrap()),
            json!(original)
        );
        let count = after.governance.len();
        w.renew_unstarted_feature_permissions(&e).unwrap();
        assert_eq!(count, e.store.snapshot().unwrap().governance.len());
    }
}
