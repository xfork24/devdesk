#[path = "github_projects.rs"]
mod github_projects;
#[path = "governance.rs"]
mod governance;
#[path = "integrations.rs"]
mod integrations;
#[path = "model_providers.rs"]
mod model_providers;
#[path = "permission_resources.rs"]
mod permission_resources;
#[path = "runtime_setup.rs"]
mod runtime_setup;
// Project selection pins every request to one Engine and therefore one database.
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

type Active = Option<(String, Arc<Engine>)>;
pub struct Workspace {
    pub global: Arc<GlobalStore>,
    codex_auth: codex_auth::CodexAuthentication,
    configuration: Arc<Engine>,
    adapters: ports::ExecutionAdapters,
    active: RwLock<Active>,
    opened: Mutex<BTreeMap<String, Arc<Engine>>>,
    switching: Mutex<()>,
    runtime_setup: runtime_setup::RuntimeSetup,
}
fn engine(store: Arc<dyn EngineeringStore>, adapters: &ports::ExecutionAdapters) -> Arc<Engine> {
    let artifacts = Arc::new(FileArtifacts::new(PathBuf::from(store.root())));
    let tools = Arc::new(
        devdesk_adapters::integrations::NativeTools::new(store.root())
            .expect("existing execution root"),
    );
    Arc::new(Engine::new(store, artifacts, adapters.clone()).with_tool_executor(tools))
}
fn workspace_defaults(e: &Engine) -> Result<()> {
    let s = e.store.snapshot()?;
    let mut targets = std::collections::BTreeSet::new();
    if s.configuration(DEFAULT_HARNESS_ID).is_ok() {
        targets.insert(DEFAULT_HARNESS_ID.to_owned());
    }
    if s.configuration("builtin-harness-dsh-multi-provider-v1")
        .is_ok()
    {
        targets.insert("builtin-harness-dsh-multi-provider-v1".to_owned());
    }
    for r in s
        .governance
        .iter()
        .filter(|r| r.kind == "agent" && r.definition_id.starts_with("builtin-agent-"))
    {
        let a: AgentComposition = r.decode()?;
        if a.permission_selection.is_some() {
            targets.insert(a.harness_ref);
        }
    }
    for r in s.governance.iter().filter(|r| {
        r.kind == "harness"
            && r.definition_id.starts_with("builtin-harness-")
            && !s.governance.iter().any(|n| {
                n.kind == r.kind && n.definition_id == r.definition_id && n.version > r.version
            })
    }) {
        targets.insert(r.id.clone());
    }
    let mut changes = Vec::new();
    for (kind, id, profile) in targets
        .into_iter()
        .map(|id| {
            (
                PermissionTargetKind::Harness,
                id,
                "permission-harness-full-access-v2-r1",
            )
        })
        .chain(std::iter::once((
            PermissionTargetKind::Sandbox,
            DEFAULT_PERMISSION_SANDBOX.into(),
            "permission-sandbox-full-access-v2-r1",
        )))
    {
        let target = permission_target(&s, kind, &id)?;
        if active_permission_binding(&s, &target).is_none() {
            changes.push(json!({"target":target,"permission_revision_id":profile,"expected_revision":0,"enforcement_contract":permission_profiles::permission_contract(&s,&target)?}));
        }
    }
    if !changes.is_empty() {
        e.execute(
            "permission.bindings.apply",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"bindings":changes}),
            },
        )?;
    }
    Ok(())
}

impl Workspace {
    fn open_conversation(&self, e: &Engine, command: Command) -> Result<Value> {
        self.open_role_session(e, command, false)
    }
    fn open_role_session(
        &self,
        e: &Engine,
        mut command: Command,
        engineering: bool,
    ) -> Result<Value> {
        require(!command.command_id.is_empty(), "command_id 不能为空")?;
        require(
            command.payload.get("binding").is_none(),
            "会话绑定由协调器解析，不能由客户端提供",
        )?;
        let fingerprint = command_fingerprint("session.open", &command);
        if let Some(previous) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(previous);
        }
        self.resolve_agent_composition(e, &mut command)?;
        e.validate_workflow_command("session.open", &command)?;
        let state = e.store.snapshot()?;
        let agent = command.payload["agent"]
            .as_str()
            .ok_or_else(|| err("invalid_params", "需要 Agent 修订 ID"))?;
        let definition = state
            .definitions
            .iter()
            .find(|d| d.id == agent && d.agent_profile.is_some())
            .ok_or_else(|| err("not_found", "Agent 角色修订不存在"))?;
        let profile = definition.agent_profile.as_ref().unwrap();
        let work_type: WorkType = command
            .payload
            .get("work_type")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|_| err("invalid_params", "无效工作类型"))?
            .unwrap_or(WorkType::Discussion);
        require(
            engineering || [WorkType::Discussion, WorkType::Design].contains(&work_type),
            "工程实施/测试/正式评审须从冻结的 Run 入口启动",
        )?;
        require(
            profile.allowed_work.contains(&work_type),
            "角色不支持当前工作类型",
        )?;
        let provider: HarnessProvider = serde_json::from_value(command.payload["provider"].clone())
            .map_err(|_| err("invalid_configuration", "Agent 缺少 Harness 后端"))?;
        let installation =
            self.ensure_runtime(provider, command.payload["installation_id"].as_str())?;
        if let Some(agent) = command.payload["agent_configuration"]["harness_revision_id"].as_str()
        {
            let h: HarnessDefinition = state.configuration(agent)?.decode()?;
            require(
                if h.recipe_digest.is_some() {
                    installation
                        .key
                        .version
                        .starts_with(&format!("{}+docker.", h.runtime_version))
                } else {
                    installation.key.version == h.runtime_version
                },
                "安装版本与 Agent 锁定的 Harness 版本不匹配",
            )?;
        }
        permissions::validate_runtime_version(&installation)?;
        require(
            self.global.runtime_integrity(&installation.id)?.verified,
            "运行时镜像内容或平台未通过核验",
        )?;
        let model = command.payload["model"]
            .as_str()
            .ok_or_else(|| err("invalid_configuration", "Agent 缺少 Model 修订"))?
            .to_owned();
        require(
            !model.trim().is_empty() && model.len() <= 160,
            "模型 ID 无效",
        )?;
        let policy =
            if [WorkType::Discussion, WorkType::Design, WorkType::Review].contains(&work_type) {
                ToolPolicy::ReadOnly
            } else {
                profile.tool_policy
            };
        require(
            engineering || command.payload.get("writable_paths").is_none(),
            "写目录只能由正式工程入口解析",
        )?;
        let writable_paths: Vec<String> = if policy == ToolPolicy::ReadOnly {
            vec![]
        } else {
            serde_json::from_value(command.payload["writable_paths"].clone())
                .map_err(|_| err("invalid_params", "缺少冻结写目录"))?
        };
        let read_only_paths = if policy == ToolPolicy::TaskWrite {
            vec!["tests".to_string()]
        } else {
            vec![]
        };
        require(
            command.payload.get("permission_overrides").is_none(),
            "旧权限覆盖已移除",
        )?;
        let composition: AgentComposition = state.configuration(agent)?.decode()?;
        let selection = composition
            .permission_selection
            .as_ref()
            .ok_or_else(|| err("permission_unconfigured", "Agent 必须选择版本化权限"))?;
        let lock = if engineering {
            let owner = command.target.as_deref().unwrap_or("");
            let feature = state
                .tasks
                .iter()
                .find(|t| t.id == owner)
                .map_or(owner, |t| t.feature_id.as_str());
            frozen_permission_lock(&state, feature, agent)?
        } else {
            resolve_permission_lock(&state, &composition.harness_ref, selection)?
        };
        permission_profiles::verify_permission_installation(&state, &lock, &installation)?;
        let resolved_permissions = permission_profiles::compile_permission_profiles(
            &state,
            &lock,
            Some(ExecutionScope {
                work_type,
                writable_paths: writable_paths.clone(),
                read_only_paths: read_only_paths.clone(),
            }),
        )?;
        let adapter = harness::managed_adapter_identity(provider);
        let agent_configuration: Option<AgentExecutionConfiguration> = command
            .payload
            .get("agent_configuration")
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|_| err("invalid_params", "无效 Agent 执行配置"))
            })
            .transpose()?;
        let model_connection = if provider == HarnessProvider::Dsh {
            let a = agent_configuration
                .as_ref()
                .ok_or_else(|| err("invalid_configuration", "缺少 Agent 执行配置"))?;
            let m: ModelDefinition = state.configuration(&a.model_revision_id)?.decode()?;
            let h: HarnessDefinition = state.configuration(&a.harness_revision_id)?.decode()?;
            require(m.model == model, "Model 修订与执行模型不一致")?;
            let connection = resolve_model_connection(&m, &h)?;
            require(
                resolved_permissions.sandbox.policy.network != NetworkPermission::Disabled,
                "Sandbox 禁网，无法连接模型服务",
            )?;
            connection
        } else {
            None
        };
        let credential_ref = match provider {
            HarnessProvider::Codex => Some(self.codex_auth.credential_reference()?),
            HarnessProvider::Dsh => {
                let p = model_connection
                    .as_ref()
                    .ok_or_else(|| err("invalid_configuration", "缺少模型连接"))?
                    .provider;
                if p.requires_key() {
                    let vault = credentials::CredentialVault::new(Path::new(&self.global.root()))?;
                    let status = vault.status_model_reference(
                        p,
                        self.global.model_credential_reference(p)?.as_deref(),
                    )?;
                    require(
                        status.available,
                        "系统凭据库不可用，请检查共享凭据助手或 Secret Service",
                    )?;
                    require(status.configured, format!("请先配置 {} 凭据", p.label()))?;
                    vault.verify_model_reference(p, &status.reference)?;
                    Some(status.reference)
                } else {
                    None
                }
            }
        };
        let model_profile_digest = model_connection
            .as_ref()
            .map(|c| {
                devdesk_adapters::model_providers::dsh_model_patch(c, &model)
                    .and_then(|patch| configuration_digest(&json!(patch)))
            })
            .transpose()?;
        let model_parameters = agent_configuration
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少 Agent 执行配置"))?
            .model_parameters
            .clone();
        let mut binding = RuntimeBinding {
            model_connection,
            model_profile_digest,
            agent_configuration,
            permissions: Some(resolved_permissions),
            installation: installation.clone(),
            adapter_version: adapter,
            model,
            model_parameters,
            tool_policy: policy,
            writable_paths,
            read_only_paths,
            work_type,
            configuration_digest: String::new(),
            credential_ref,
        };
        binding.configuration_digest = Self::permission_runtime_digest(definition, &binding)?;
        command.payload["binding"] = json!(binding);
        e.execute_resolved("session.open", command, &fingerprint)
    }
    fn resolve_live_attempt(&self, e: &Engine, mut command: Command) -> Result<Value> {
        require(
            command.payload.get("live_sessions").is_none(),
            "阶段 Session 由宿主解析，不能由客户端指定",
        )?;
        let fingerprint = command_fingerprint("attempt.start", &command);
        if let Some(result) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(result);
        }
        e.validate_workflow_command("attempt.start", &command)?;
        let state = e.store.snapshot()?;
        let target = command
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少 Attempt target"))?;
        e.validate_attempt_cancellation(target)?;
        let task = state.tasks.iter().find(|t| t.id == target);
        let phase_value = if task.is_some() {
            Some("implementation")
        } else {
            command.payload["phase"].as_str()
        }
        .ok_or_else(|| err("invalid_params", "Feature 执行必须选择确切阶段"))?
        .to_owned();
        let phase = phase_value.as_str();
        let stage_roles: Vec<(&str, AgentRole, WorkType)> = match phase {
            "requirement_definition" => vec![(
                "requirement_validate",
                AgentRole::Analyst,
                WorkType::Discussion,
            )],
            "solution_planning" => vec![
                ("plan_validate", AgentRole::Architect, WorkType::Design),
                (
                    "test_plan_validate",
                    AgentRole::Tester,
                    WorkType::Discussion,
                ),
            ],
            "implementation" => vec![
                ("test_author", AgentRole::Tester, WorkType::Testing),
                ("red", AgentRole::Tester, WorkType::Discussion),
                ("implement", AgentRole::Developer, WorkType::Implementation),
                ("green", AgentRole::Tester, WorkType::Discussion),
                ("docs", AgentRole::Developer, WorkType::Discussion),
                ("verify", AgentRole::Tester, WorkType::Discussion),
            ],
            "closure" => vec![
                ("test_author", AgentRole::Tester, WorkType::Testing),
                ("feature_tests", AgentRole::Tester, WorkType::Discussion),
                ("docs", AgentRole::Developer, WorkType::Discussion),
                ("verify", AgentRole::Tester, WorkType::Discussion),
            ],
            _ => return Err(err("invalid_params", "未知阶段")),
        };
        let mut stage_roles: Vec<(String, AgentRole, WorkType)> = stage_roles
            .into_iter()
            .map(|(n, r, w)| (n.into(), r, w))
            .collect();
        if let Some(recipe) = e.workflow_recipe(
            &state,
            target,
            phase,
            command.payload["workflow_step_instance_id"].as_str(),
        ) {
            stage_roles = recipe
                .into_iter()
                .map(|n| {
                    let work = if n.key == "implement" {
                        WorkType::Implementation
                    } else if n.key == "test_author" {
                        WorkType::Testing
                    } else if n.role == AgentRole::Architect {
                        WorkType::Design
                    } else {
                        WorkType::Discussion
                    };
                    (n.key, n.role, work)
                })
                .collect();
        }
        let feature = task.map_or(target, |t| t.feature_id.as_str());
        let frozen = state.feature_configuration(feature);
        if command.payload["workflow_step_instance_id"].is_null() {
            if let Some(id) = e.workflow_step_id(&state, target, "attempt.execute", Some(phase))? {
                command.payload["workflow_step_instance_id"] = json!(id);
            }
        }
        let mut sessions = std::collections::BTreeMap::new();
        for (stage, role, work_type) in stage_roles {
            let key = match role {
                AgentRole::Analyst => "analyst",
                AgentRole::Architect => "architect",
                AgentRole::Developer => "developer",
                AgentRole::Tester => "tester",
                AgentRole::Reviewer => "reviewer",
            };
            let root = format!("builtin-agent-{key}-v1");
            let explicit = e.workflow_agent_for(
                &state,
                target,
                "attempt.execute",
                Some(phase),
                Some(&stage),
                command.payload["workflow_step_instance_id"].as_str(),
            )?;
            let agent = if let Some(chosen) = &explicit {
                state
                    .definitions
                    .iter()
                    .find(|d| &d.id == chosen)
                    .ok_or_else(|| err("not_found", "冻结 Agent 不存在"))?
            } else if let Some(lock) = &frozen {
                let chosen = lock
                    .agent_refs
                    .get(key)
                    .ok_or_else(|| err("configuration_required", "流程未绑定所需 Agent"))?;
                state
                    .definitions
                    .iter()
                    .find(|d| &d.id == chosen)
                    .ok_or_else(|| err("not_found", "冻结 Agent 不存在"))?
            } else {
                state
                    .definitions
                    .iter()
                    .filter(|d| {
                        d.agent_profile.as_ref().is_some_and(|p| p.role == role)
                            && state.agent_identity(&d.id).is_ok_and(|id| id == root)
                    })
                    .max_by_key(|d| d.version)
                    .ok_or_else(|| err("blocked", "预置工程角色缺失"))?
            };
            let paths = if work_type == WorkType::Implementation {
                vec![".".to_string()]
            } else if work_type == WorkType::Testing {
                let recipe = state
                    .documents
                    .iter()
                    .rev()
                    .find(|d| {
                        d.kind == "test"
                            && if task.is_some() {
                                d.task_id.as_deref() == Some(target)
                            } else {
                                d.task_id.is_none() && state.document_in_feature(d, target)
                            }
                    })
                    .and_then(|d| d.test_recipe.as_ref())
                    .ok_or_else(|| err("configuration_required", "Task Test 缺少实际测试配方"))?;
                recipe.validate()?;
                let mut paths = std::collections::BTreeSet::new();
                for file in &recipe.files {
                    let parent = file.rsplit_once('/').map(|p| p.0).unwrap_or("");
                    require(
                        parent.starts_with("tests/") && parent != "tests/",
                        "测试作者配方需使用独立 tests/<suite>/ 目录",
                    )?;
                    paths.insert(parent.to_string());
                }
                paths.into_iter().collect()
            } else {
                vec![]
            };
            let mut payload =
                json!({"agent":agent.id,"work_type":work_type,"writable_paths":paths});
            payload["workflow_recipe_node"] = json!(stage);
            for name in [
                "provider",
                "installation_id",
                "model",
                "workflow_step_instance_id",
            ] {
                if let Some(value) = command.payload.get(name) {
                    payload[name] = value.clone();
                }
            }
            let opened = self.open_role_session(
                e,
                Command {
                    command_id: format!("{}:session:{stage}", command.command_id),
                    target: command.target.clone(),
                    expected_revision: command.expected_revision,
                    payload,
                },
                true,
            )?;
            sessions.insert(
                stage.to_string(),
                opened["id"].as_str().unwrap().to_string(),
            );
        }
        command.payload["live_sessions"] = json!(sessions);
        e.execute_resolved("attempt.start", command, &fingerprint)
    }
    fn resolve_live_review(&self, e: &Engine, mut command: Command) -> Result<Value> {
        require(
            command.payload.get("review_session").is_none(),
            "Reviewer Session 由宿主解析",
        )?;
        let fingerprint = command_fingerprint("review.request", &command);
        if let Some(value) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(value);
        }
        e.validate_workflow_command("review.request", &command)?;
        let state = e.store.snapshot()?;
        let delivery = state.delivery(command.target.as_deref().unwrap_or(""))?;
        let owner = delivery.task_id.as_deref().unwrap_or(&delivery.feature_id);
        let revision = if let Some(task) = &delivery.task_id {
            state.task(task)?.revision
        } else {
            state.feature(owner)?.revision
        };
        let frozen = state.feature_configuration(&delivery.feature_id);
        let action = if delivery.merge_id.is_some() {
            "merge.review"
        } else {
            "delivery.review"
        };
        if command.payload["workflow_step_instance_id"].is_null() {
            if let Some(id) = e.workflow_step_id(&state, owner, action, None)? {
                command.payload["workflow_step_instance_id"] = json!(id);
            }
        }
        let explicit = e.workflow_agent_for(
            &state,
            owner,
            action,
            None,
            None,
            command.payload["workflow_step_instance_id"].as_str(),
        )?;
        let agent = if let Some(chosen) = &explicit {
            state
                .definitions
                .iter()
                .find(|d| &d.id == chosen)
                .ok_or_else(|| err("not_found", "冻结 Reviewer 不存在"))?
        } else if let Some(lock) = &frozen {
            let chosen = lock
                .agent_refs
                .get("reviewer")
                .ok_or_else(|| err("configuration_required", "未绑定 Reviewer Agent"))?;
            state
                .definitions
                .iter()
                .find(|d| &d.id == chosen)
                .ok_or_else(|| err("not_found", "冻结 Reviewer 不存在"))?
        } else {
            state
                .definitions
                .iter()
                .filter(|d| {
                    d.agent_profile
                        .as_ref()
                        .is_some_and(|p| p.role == AgentRole::Reviewer)
                        && state
                            .agent_identity(&d.id)
                            .is_ok_and(|root| root == "builtin-agent-sol-high-reviewer-v3")
                })
                .max_by_key(|d| d.version)
                .ok_or_else(|| err("blocked", "缺少独立 Reviewer"))?
        };
        let mut payload = json!({"agent":agent.id,"work_type":"review"});
        for key in [
            "provider",
            "installation_id",
            "model",
            "workflow_step_instance_id",
        ] {
            if let Some(value) = command.payload.get(key) {
                payload[key] = value.clone();
            }
        }
        let session = self.open_role_session(
            e,
            Command {
                command_id: format!("{}:review-session", command.command_id),
                target: Some(owner.into()),
                expected_revision: Some(revision),
                payload,
            },
            true,
        )?;
        command.payload["review_session"] = session["id"].clone();
        e.execute_resolved("review.request", command, &fingerprint)
    }
    fn resolve_live_merge(&self, e: &Engine, mut command: Command) -> Result<Value> {
        require(
            command.payload.get("merge_session").is_none(),
            "候选 Session 由宿主解析",
        )?;
        let fingerprint = command_fingerprint("merge.request", &command);
        if let Some(value) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(value);
        }
        e.validate_workflow_command("merge.request", &command)?;
        let state = e.store.snapshot()?;
        let owner = command.target.as_deref().unwrap_or("");
        let revision = if let Some(task) = state.tasks.iter().find(|t| t.id == owner) {
            task.revision
        } else {
            state.feature(owner)?.revision
        };
        let feature = state
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        if command.payload["workflow_step_instance_id"].is_null() {
            if let Some(id) = e.workflow_step_id(&state, owner, "merge.prepare", None)? {
                command.payload["workflow_step_instance_id"] = json!(id);
            }
        }
        let agent = e
            .workflow_agent_for(
                &state,
                owner,
                "merge.prepare",
                None,
                None,
                command.payload["workflow_step_instance_id"].as_str(),
            )?
            .or_else(|| {
                state
                    .feature_configuration(feature)
                    .and_then(|c| c.agent_refs.get("tester").cloned())
            })
            .unwrap_or_else(|| "builtin-agent-sol-high-tester-v3".into());
        let mut payload = json!({"agent":agent,"work_type":"discussion"});
        for key in [
            "provider",
            "installation_id",
            "model",
            "workflow_step_instance_id",
        ] {
            if let Some(value) = command.payload.get(key) {
                payload[key] = value.clone();
            }
        }
        let session = self.open_role_session(
            e,
            Command {
                command_id: format!("{}:merge-session", command.command_id),
                target: Some(owner.into()),
                expected_revision: Some(revision),
                payload,
            },
            true,
        )?;
        command.payload["merge_session"] = session["id"].clone();
        e.execute_resolved("merge.request", command, &fingerprint)
    }
    pub fn open(root: &Path) -> Result<Arc<Self>> {
        Self::open_with_adapters(root, execution_adapters(root)?)
    }
    pub fn open_with_adapters(
        root: &Path,
        adapters: ports::ExecutionAdapters,
    ) -> Result<Arc<Self>> {
        let global = Arc::new(GlobalStore::open(root)?);
        let configuration = engine(global.clone(), &adapters);
        configuration.bootstrap()?;
        configuration.recover_tool_receipts()?;
        global.reload_permission_presets(&Command {
            command_id: id(),
            target: None,
            expected_revision: None,
            payload: json!({}),
        })?;
        workspace_defaults(&configuration)?;
        let workspace = Arc::new(Self {
            codex_auth: codex_auth::CodexAuthentication::new(root)?,
            global,
            configuration,
            adapters,
            active: RwLock::new(None),
            opened: Mutex::new(BTreeMap::new()),
            switching: Mutex::new(()),
            runtime_setup: runtime_setup::RuntimeSetup::default(),
        });
        let mut offset = 0;
        loop {
            let (projects, total) = workspace.global.projects(offset, 100)?;
            for project in projects {
                workspace.load_project(&project.id, false)?;
            }
            offset += 100;
            if offset >= total {
                break;
            }
        }
        if let Some(id) = workspace.global.active_project()? {
            let e = workspace.load_project(&id, false)?;
            *workspace.active.write().unwrap() = Some((id, e));
        }
        Ok(workspace)
    }
    pub fn active_id(&self) -> Option<String> {
        self.active
            .read()
            .unwrap()
            .as_ref()
            .map(|(id, _)| id.clone())
    }
    pub fn active_engine(&self, expected: Option<&str>) -> Result<Arc<Engine>> {
        let active = self.active.read().unwrap();
        if let Some(expected) = expected {
            if active.as_ref().is_none_or(|(id, _)| id != expected) {
                return Err(err("project_changed", "活动项目已改变，请刷新后重试"));
            }
        }
        active
            .as_ref()
            .map(|(_, e)| e.clone())
            .ok_or_else(|| err("project_required", "请先创建或打开项目"))
    }
    fn sync_configuration(&self, e: &Engine) -> Result<()> {
        let existing = e.store.snapshot()?;
        // One global directory indexes project-owned Role identities without changing their owner or revision.
        self.global.transact(None, &mut |directory| {
            for role in existing
                .governance
                .iter()
                .filter(|r| r.kind == "role" || r.kind == "role_retirement")
            {
                if let Some(old) = directory.governance.iter().find(|r| r.id == role.id) {
                    require(old.content_hash == role.content_hash, "Role 目录摘要冲突")?;
                } else {
                    directory.governance.push(role.clone());
                }
            }
            Ok(Value::Null)
        })?;
        let shared = self.global.snapshot()?;
        if existing.projects.iter().any(|p| p.execution_mode != "live") {
            return Ok(());
        }
        if shared
            .governance
            .iter()
            .filter(|r| {
                r.immutable
                    && (r.kind == "role"
                        || r.kind == "role_retirement"
                        || devdesk_core::configuration_visible(
                            &shared,
                            r,
                            e.store.project_id().as_deref(),
                        ))
            })
            .all(|r| {
                existing
                    .governance
                    .iter()
                    .any(|v| v.id == r.id && v.content_hash == r.content_hash)
            })
            && shared.definitions.iter().all(|d| {
                existing.definitions.iter().any(|v| {
                    v.id == d.id
                        && serde_json::to_value(v).unwrap() == serde_json::to_value(d).unwrap()
                })
            })
            && shared
                .harnesses
                .iter()
                .all(|h| existing.harnesses.iter().any(|v| v.id == h.id))
            && shared
                .sandboxes
                .iter()
                .all(|b| existing.sandboxes.iter().any(|v| v.id == b.id))
        {
            return Ok(());
        }
        e.store.transact(None, &mut |s| {
            for r in shared.governance.iter().filter(|r| {
                r.immutable
                    && (r.kind == "role"
                        || r.kind == "role_retirement"
                        || devdesk_core::configuration_visible(
                            &shared,
                            r,
                            e.store.project_id().as_deref(),
                        ))
            }) {
                if let Some(old) = s.governance.iter().find(|v| v.id == r.id) {
                    require(old.content_hash == r.content_hash, "共享配置摘要冲突")?;
                } else {
                    s.governance.push(r.clone());
                }
            }
            // Revisions are immutable. Copies keep historical foreign keys self-contained.
            for d in &shared.definitions {
                if let Some(old) = s.definitions.iter().find(|old| old.id == d.id) {
                    require(
                        serde_json::to_value(old).unwrap() == serde_json::to_value(d).unwrap(),
                        "共享定义修订发生冲突",
                    )?;
                } else {
                    s.definitions.push(d.clone());
                }
            }
            for h in &shared.harnesses {
                if !s.harnesses.iter().any(|old| old.id == h.id) {
                    s.harnesses.push(h.clone());
                }
            }
            for b in &shared.sandboxes {
                if !s.sandboxes.iter().any(|old| old.id == b.id) {
                    s.sandboxes.push(b.clone());
                }
            }
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    fn load_project(&self, id: &str, creating: bool) -> Result<Arc<Engine>> {
        let root = PathBuf::from(self.global.root());
        if !creating {
            require(self.global.project(id)?.is_some(), "项目不存在")?;
            require(
                SqliteStore::project_path(&root, id)?
                    .join("engineering.sqlite3")
                    .is_file(),
                "项目数据库缺失，不能自动重建",
            )?;
        }
        if let Some(e) = self.opened.lock().unwrap().get(id).cloned() {
            return Ok(e);
        }
        let e = engine(
            Arc::new(SqliteStore::open_project(&root, id)?),
            &self.adapters,
        );
        if !creating {
            require(
                e.store.snapshot()?.projects.len() == 1,
                "项目数据库未包含有效 Project",
            )?;
        }
        self.sync_configuration(&e)?;
        e.bootstrap()?;
        e.recover_tool_receipts()?;
        if e.store
            .snapshot()?
            .projects
            .iter()
            .all(|p| p.execution_mode == "live")
        {
            // Re-evaluate native facts once on reopen before the idle-project filter.
            e.evaluate_workflows()?;
        }
        // Repair a stale directory summary after a crash between the two independent commits.
        if let Some(project) = e.store.snapshot()?.projects.first() {
            self.global.publish_project(project)?;
        }
        self.opened.lock().unwrap().insert(id.into(), e.clone());
        Ok(e)
    }
    fn select(&self, id: &str, e: Arc<Engine>) -> Result<()> {
        self.global.activate(id)?;
        *self.active.write().unwrap() = Some((id.into(), e));
        Ok(())
    }
    fn global_conversation_dispatch(&self, method: &str, params: Value) -> Result<Value> {
        require(params["project_id"].is_null(), "全局讨论不能同时指定项目")?;
        let existing = {
            self.opened
                .lock()
                .unwrap()
                .get(GLOBAL_DISCUSSION_NAMESPACE)
                .cloned()
        };
        let e = if let Some(e) = existing {
            e
        } else {
            let root = SqliteStore::global_discussion_path(Path::new(&self.global.root()))?;
            let e = engine(Arc::new(SqliteStore::open(&root)?), &self.adapters);
            self.sync_configuration(&e)?;
            e.bootstrap()?;
            self.opened
                .lock()
                .unwrap()
                .insert(GLOBAL_DISCUSSION_NAMESPACE.into(), e.clone());
            e
        };
        self.sync_configuration(&e)?;
        if let Some(result) = self.experience_dispatch(method, &params, &e) {
            return result;
        }
        if method == "session.open" {
            require(params["target"] == "global", "全局讨论必须以 global 为目标")?;
            return self.open_conversation(
                &e,
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?,
            );
        }
        if method == "conversation.send" {
            let c = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            if let Some(result) = self.send_permission_conversation(&e, c)? {
                return Ok(result);
            }
        }
        if method.starts_with("query.") || ["session.list", "session.read"].contains(&method) {
            let name = match method {
                "session.list" => "query.sessions",
                "session.read" => "query.session",
                _ => method,
            };
            let mut result = e.query(name, &params, self.global.concurrency()?)?;
            if name == "query.session" {
                if let Some(id) = params["id"].as_str() {
                    result["permission_status"] = self.permission_session_status(&e, id)?;
                }
            }
            return Ok(result);
        }
        e.execute(
            method,
            serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?,
        )
    }
    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        if [
            "query.permissions",
            "permission.create",
            "permission.revise",
            "permission.bind",
            "permission.preflight",
            "definition.revise",
            "binding.update",
            "query.default_configuration",
            "workflow.task_upgrade",
        ]
        .contains(&method)
        {
            return Err(err("method_not_found", "接口不存在"));
        }

        if params["scope"] == "global"
            && [
                "session.open",
                "session.list",
                "session.read",
                "query.sessions",
                "query.session",
                "conversation.send",
                "conversation.cancel",
                "execution.retry_cleanup",
                "interaction.answer",
                "query.agent_experience",
                "experience.restore",
            ]
            .contains(&method)
        {
            return self.global_conversation_dispatch(method, params);
        }
        if let Some(result) = self.model_provider_api(method, &params) {
            return result;
        }
        if method == "query.configuration_defaults" {
            return self
                .configuration
                .configuration_defaults(params["project_id"].as_str());
        }
        if method == "runtime.prepare_harness" {
            let id = params["id"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "需要 Harness 修订"))?;
            let engine = if self
                .global
                .snapshot()?
                .governance
                .iter()
                .any(|r| r.id == id)
            {
                self.configuration.clone()
            } else {
                self.active_engine(params["project_id"].as_str())?
            };
            let state = engine.store.snapshot()?;
            let harness = state.configuration(id)?;
            require(harness.kind == "harness", "需要 Harness 修订")?;
            return Ok(json!(self.materialize_harness(&engine, harness)?));
        }
        if method == "query.history" && params["scope"] == "global" {
            return self
                .configuration
                .query(method, &params, self.global.concurrency()?);
        }
        if let Some(result) = self.integration_dispatch(method, &params) {
            return result;
        }
        if let Some(result) = self.governance_dispatch(method, &params) {
            return result;
        }
        if method == "project.open_github" {
            return self.open_github_project(params);
        }
        if method == "runtime.preparation_status" {
            return Ok(self.runtime_setup.status());
        }
        if method == "runtime.prepare" {
            let provider = serde_json::from_value(params["provider"].clone())
                .map_err(|_| err("invalid_params", "需要 Harness provider"))?;
            return Ok(json!(self.ensure_runtime(provider, None)?));
        }
        if let Some(result) = self.permission_resource_api(method, &params) {
            return result;
        }
        if method == "github.diagnose" {
            let repo = params["repository"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "需要 repository"))?;
            let branch = params["branch"].as_str().unwrap_or("main");
            return Ok(json!(github::GitHubPullRequests.diagnose(repo, branch)?));
        }
        if method == "runtime.discover_codex" {
            return Ok(json!(self.global.discover_codex()?));
        }
        if method == "runtime.discover_docker" {
            let provider: HarnessProvider = serde_json::from_value(params["provider"].clone())
                .map_err(|_| err("invalid_params", "需要 provider"))?;
            return Ok(json!(self.global.discover_docker_runtime(provider)?));
        }
        if method.starts_with("codex.auth.") {
            let prepared;
            let installation = if let Some(id) = params["installation_id"].as_str() {
                id
            } else {
                prepared = self.ensure_codex_auth()?;
                &prepared.id
            };
            return match method {
                "codex.auth.status" => self.codex_auth.status(&self.global, installation),
                "codex.auth.models" => self.codex_auth.models(&self.global, installation),
                "codex.auth.start" => self.codex_auth.start(
                    &self.global,
                    installation,
                    params["device"].as_bool().unwrap_or(false),
                ),
                "codex.auth.cancel" => self.codex_auth.cancel(
                    &self.global,
                    installation,
                    params["login_id"]
                        .as_str()
                        .ok_or_else(|| err("invalid_params", "需要 login_id"))?,
                ),
                _ => Err(err("method_not_found", "未知 Codex 认证命令")),
            };
        }
        if method == "settings.update" {
            let n = params
                .get("global_concurrency")
                .map(|v| {
                    v.as_u64()
                        .filter(|n| (1..=32).contains(n))
                        .ok_or_else(|| err("invalid_params", "全局并发额度需为 1—32"))
                })
                .transpose()?;
            let provider = params
                .get("default_harness")
                .map(|v| {
                    serde_json::from_value::<HarnessProvider>(v.clone())
                        .map_err(|_| err("invalid_params", "默认 Harness 需为 codex 或 dsh"))
                })
                .transpose()?;
            self.global
                .update_preferences(n.map(|n| n as usize), provider)?;
            return Ok(
                json!({"global_concurrency":self.global.concurrency()?, "default_harness":self.global.default_harness()?}),
            );
        }
        if method == "query.runtime_installations" {
            return Ok(
                json!({"installations":self.global.runtime_installations()?, "default_harness":self.global.default_harness()?}),
            );
        }
        if method == "query.runtime_options" {
            let installations: Vec<_> = self
                .global
                .runtime_installations()?
                .into_iter()
                .map(|runtime| {
                    let supported = runtime.key.os == "linux"
                        && runtime.key.arch == std::env::consts::ARCH
                        && matches!(runtime.artifact, RuntimeArtifact::DockerImage { .. })
                        && permissions::validate_runtime_version(&runtime).is_ok();
                    let reason = if supported {
                        None
                    } else {
                        Some(if runtime.key.provider == HarnessProvider::Codex {
                            "本机 Codex 用于登录及模型目录；项目执行使用隔离的 Docker worker"
                        } else {
                            "此安装不是项目执行支持的 Docker worker"
                        })
                    };
                    let mut value = json!(runtime);
                    value["execution_supported"] = json!(supported);
                    value["usage_reason"] = json!(reason);
                    value
                })
                .collect();
            let vault = credentials::CredentialVault::new(Path::new(&self.global.root()))?;
            let providers:Vec<_>=DshModelProvider::ALL.into_iter().map(|provider|{
                let reference=self.global.model_credential_reference(provider).ok().flatten();
                let status=vault.status_model_reference(provider,reference.as_deref()).ok();
                json!({"id":provider.as_str(),"label":provider.label(),"configured":status.as_ref().is_some_and(|s|s.configured),"available":status.as_ref().is_some_and(|s|s.available),"auth_kind":if provider.requires_key(){"api_key"}else{"none"}})
            }).collect();
            return Ok(
                json!({"default_harness":self.global.default_harness()?,"host_os":std::env::consts::OS,"host_arch":std::env::consts::ARCH,
                "installations":installations,"model_providers":providers,"recipes":[
                    {"provider":"codex","version":DEFAULT_CODEX_VERSION,"os":"linux","arch":std::env::consts::ARCH,"recipe_digest":default_codex_recipe_digest()},
                    {"provider":"dsh","version":DEFAULT_DSH_VERSION,"os":"linux","arch":std::env::consts::ARCH,"recipe_digest":default_dsh_recipe_digest()}
                ]}),
            );
        }
        if method == "runtime.register" {
            let registration = serde_json::from_value(params)
                .map_err(|e| err("invalid_params", format!("无效的运行时登记: {e}")))?;
            return Ok(json!(self.global.register_runtime(registration)?));
        }
        if method == "runtime.integrity" {
            let id = params["id"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "需要安装 ID"))?;
            return Ok(json!(self.global.runtime_integrity(id)?));
        }
        if method == "project.create" {
            let command: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            let _switch = self.switching.lock().unwrap();
            let id = self.global.reserve_project(&command)?;
            let e = self.load_project(&id, true)?;
            self.sync_configuration(&e)?;
            let result = e.execute(method, command)?;
            self.global.publish_project(
                e.store
                    .snapshot()?
                    .projects
                    .first()
                    .ok_or_else(|| err("storage", "项目未创建"))?,
            )?;
            self.select(&id, e)?;
            return Ok(result);
        }
        if method == "project.open" {
            let c: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            require(!c.command_id.is_empty(), "command_id 不能为空")?;
            let id = c
                .target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "需要 Project target"))?;
            let _switch = self.switching.lock().unwrap();
            self.global.validate_open_command(&c)?;
            let e = self.load_project(id, false)?;
            self.sync_configuration(&e)?;
            self.global.activate_command(&c)?;
            *self.active.write().unwrap() = Some((id.into(), e));
            return Ok(json!({"id":id}));
        }
        if ["agent.create", "agent.revise"].contains(&method) {
            let c =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            return self.configuration.execute(method, c);
        }
        let expected = params["project_id"]
            .as_str()
            .or_else(|| params["payload"]["project_id"].as_str());
        let e = match self.active_engine(expected) {
            Ok(e) => e,
            Err(error)
                if error.code == "project_required"
                    && [
                        "query.workspace",
                        "query.history",
                        "query.events",
                        "query.runtimes",
                    ]
                    .contains(&method) =>
            {
                self.configuration.clone()
            }
            Err(error) => return Err(error),
        };
        if !method.starts_with("query.") {
            require(
                e.store
                    .snapshot()?
                    .projects
                    .iter()
                    .all(|p| p.execution_mode == "live"),
                "历史项目只读，请创建新的 GitHub 项目",
            )?;
        }
        if e.store.project_id().is_some() {
            self.sync_configuration(&e)?;
        }
        if method == "run.retry" {
            let state = e.store.snapshot()?;
            if let Some(run) = state.runs.iter().find(|r| {
                Some(r.id.as_str()) == params["target"].as_str() && r.origin == Origin::External
            }) {
                let binding = state
                    .sessions
                    .iter()
                    .find(|s| s.id == run.session_id)
                    .and_then(|s| s.runtime_binding.as_ref())
                    .ok_or_else(|| err("blocked", "旧运行时锁缺失"))?;
                require(
                    binding.adapter_version
                        == harness::managed_adapter_identity(binding.installation.key.provider),
                    "冻结适配器构建已更新，不能在技术重试时更换配置，请新建 Attempt",
                )?;
                if run.writes {
                    let a = state.attempt(
                        run.attempt_id
                            .as_deref()
                            .ok_or_else(|| err("blocked", "写 Run 缺少 Attempt"))?,
                    )?;
                    let context = a
                        .git_context
                        .as_ref()
                        .ok_or_else(|| err("blocked", "旧代码定位缺失"))?;
                    let head = state
                        .heads
                        .iter()
                        .find(|h| h.id == context.source.owner)
                        .ok_or_else(|| err("blocked", "代码头缺失"))?;
                    git::CliGit::new(Path::new(&self.global.root()))?
                        .validate_view(context, &head.revision)?;
                }
            }
        }
        if method == "conversation.send" {
            let command: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            if let Some(result) = self.send_permission_conversation(&e, command)? {
                return Ok(result);
            }
            let sid = params["target"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "需要 Session target"))?;
            if let Some(binding) = e
                .store
                .snapshot()?
                .sessions
                .iter()
                .find(|s| s.id == sid)
                .and_then(|s| s.runtime_binding.as_ref())
            {
                require(
                    binding.adapter_version
                        == harness::managed_adapter_identity(binding.installation.key.provider),
                    "适配器构建已更新，请点击开始/续用讨论创建带快照的后继会话",
                )?;
            }
        }
        if let Some(result) = self.experience_dispatch(method, &params, &e) {
            return result;
        }
        if method == "attempt.start"
            && e.store
                .snapshot()?
                .projects
                .first()
                .is_some_and(|p| p.execution_mode == "live")
        {
            let c: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            return self.resolve_live_attempt(&e, c);
        }
        if method == "merge.request"
            && e.store
                .snapshot()?
                .projects
                .first()
                .is_some_and(|p| p.execution_mode == "live")
        {
            return self.resolve_live_merge(
                &e,
                serde_json::from_value(params.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?,
            );
        }
        if method == "review.request" {
            let state = e.store.snapshot()?;
            if state.deliveries.iter().any(|d| {
                Some(d.id.as_str()) == params["target"].as_str() && d.origin == Origin::External
            }) {
                return self.resolve_live_review(
                    &e,
                    serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                );
            }
        }
        if method == "feature.create" {
            let mut params = params.clone();
            if e.store
                .snapshot()?
                .projects
                .first()
                .is_some_and(|p| p.execution_mode == "live")
            {
                params["payload"]["require_configuration"] = json!(true);
            }
            let c =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            return e.execute(method, c);
        }
        if method == "session.open" {
            let command = serde_json::from_value(params)
                .map_err(|_| err("invalid_params", "无效 Session Command"))?;
            return self.open_conversation(&e, command);
        }
        if ["session.list", "session.read"].contains(&method) {
            let mut result = e.query(
                if method == "session.list" {
                    "query.sessions"
                } else {
                    "query.session"
                },
                &params,
                self.project_limit(&e, self.global.concurrency()?)?,
            )?;
            if method == "session.read" {
                if let Some(sid) = params["id"].as_str() {
                    result["permission_status"] = self.permission_session_status(&e, sid)?;
                }
            }
            return Ok(result);
        }
        let global_limit = self.global.concurrency()?;
        if method.starts_with("query.") {
            let mut result = e.query(method, &params, self.project_limit(&e, global_limit)?)?;
            if method == "query.session" {
                if let Some(sid) = params["id"].as_str() {
                    result["permission_status"] = self.permission_session_status(&e, sid)?;
                }
            }
            if method == "query.workspace" {
                let offset = params["project_offset"].as_u64().unwrap_or(0) as usize;
                let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
                let (mut projects, total) = if params["github_only"] == true {
                    self.global.github_projects(offset, limit)?
                } else {
                    self.global.projects(offset, limit)?
                };
                let current = e.store.snapshot()?.projects.first().cloned();
                if let Some(project) = &current {
                    self.global.publish_project(project)?;
                    if let Some(row) = projects.iter_mut().find(|p| p.id == project.id) {
                        *row = project.clone();
                    }
                }
                result["projects"] = json!(projects);
                result["total_projects"] = json!(total);
                result["active_project_id"] = json!(e.store.project_id());
                result["execution_mode"] = json!(
                    current
                        .as_ref()
                        .map(|p| p.execution_mode.as_str())
                        .unwrap_or("live")
                );
                let configuration_status = e.project_configuration_status()?;
                result["configuration_ready"] = configuration_status["ready"].clone();
                result["configuration_status"] = configuration_status;
                result["active_project"] = json!(current);
                result["project_data_root"] = if e.store.project_id().is_some() {
                    json!(e.store.root())
                } else {
                    Value::Null
                };
                result["project_database"] = if e.store.project_id().is_some() {
                    json!(PathBuf::from(e.store.root()).join("engineering.sqlite3"))
                } else {
                    Value::Null
                };
                result["global_database"] =
                    json!(PathBuf::from(self.global.root()).join("global.sqlite3"));
                result["data_root"] = json!(self.global.root());
                result["global_concurrency"] = json!(global_limit);
                result["default_harness"] = json!(self.global.default_harness()?);
                result["runtime_installations"] = json!(self.global.runtime_installations()?);
                let state = e.store.snapshot()?;
                let project = e.store.project_id();
                let material_offset =
                    params["project_document_offset"].as_u64().unwrap_or(0) as usize;
                result["project_documents"] = json!(
                    state
                        .documents
                        .iter()
                        .filter(|d| d.project_id.is_some() && d.project_id == project)
                        .rev()
                        .skip(material_offset)
                        .take(limit)
                        .collect::<Vec<_>>()
                );
                result["total_project_documents"] = json!(
                    state
                        .documents
                        .iter()
                        .filter(|d| d.project_id.is_some() && d.project_id == project)
                        .count()
                );
                result["project_snapshots"] = json!(
                    state
                        .snapshots
                        .iter()
                        .filter(|s| s.project_id.is_some() && s.project_id == project)
                        .rev()
                        .take(limit)
                        .collect::<Vec<_>>()
                );
                result["project_context"] = json!(
                    state
                        .contexts
                        .iter()
                        .find(|c| Some(c.owner_id.as_str()) == project.as_deref())
                );
            }
            return Ok(result);
        }
        let c = serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
        let result = e.execute(method, c)?;
        if let Some(project) = e.store.snapshot()?.projects.first() {
            self.global.publish_project(project)?;
        }
        Ok(result)
    }
    pub fn event_engine(&self, expected: Option<&str>) -> Result<(String, Arc<Engine>)> {
        match self.active_engine(expected) {
            Ok(e) => Ok((e.store.project_id().unwrap(), e)),
            Err(error) if error.code == "project_required" => {
                Ok(("global".into(), self.configuration.clone()))
            }
            Err(error) => Err(error),
        }
    }
    pub fn tick(&self) -> Result<()> {
        self.configuration.drive_tool_calls()?;
        let engines: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let engines = engines
            .into_iter()
            .map(|e| {
                let native = e
                    .store
                    .snapshot()?
                    .projects
                    .iter()
                    .all(|p| p.execution_mode == "live");
                e.store
                    .has_pending_work()
                    .map(|pending| (e, pending && native))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(e, pending)| pending.then_some(e))
            .collect::<Vec<_>>();
        let limit = self.global.concurrency()?;
        let mut failure = None;
        let mut running: Vec<usize> = engines
            .iter()
            .map(|e| {
                Ok(e.store
                    .snapshot()?
                    .runs
                    .iter()
                    .filter(|r| r.status == Status::Running)
                    .count()
                    + e.conversation_running()?)
            })
            .collect::<Result<_>>()?;
        let mut total: usize = running.iter().sum();
        // The scheduler is single-threaded across projects. Other running projects consume quota.
        for (index, e) in engines.iter().enumerate() {
            if let Err(error) = self.tick_workflow_engine(e) {
                failure.get_or_insert(error);
            }
            if let Err(error) = e.tick_repositories() {
                failure.get_or_insert(error);
            }
            if let Err(error) = e.tick_conversations(limit.saturating_sub(total - running[index])) {
                failure.get_or_insert(error);
            }
            if let Err(error) = e.tick(
                limit
                    .saturating_sub(total - running[index])
                    .saturating_sub(e.conversation_running()?),
            ) {
                failure.get_or_insert(error);
            }
            if let Err(error) = self.tick_workflow_engine(e) {
                failure.get_or_insert(error);
            }
            let experiences: Vec<_> = e
                .store
                .snapshot()?
                .governance
                .into_iter()
                .filter(|r| r.kind == "experience")
                .collect();
            if !experiences.is_empty() {
                let known = self.global.snapshot()?;
                if experiences
                    .iter()
                    .any(|r| !known.governance.iter().any(|old| old.id == r.id))
                {
                    self.global.transact(None, &mut |s| {
                        for r in &experiences {
                            if !s.governance.iter().any(|old| old.id == r.id) {
                                s.governance.push(r.clone());
                            }
                        }
                        Ok(Value::Null)
                    })?;
                }
            }
            let next = e
                .store
                .snapshot()?
                .runs
                .iter()
                .filter(|r| r.status == Status::Running)
                .count()
                + e.conversation_running()?;
            total = total - running[index] + next;
            running[index] = next;
        }
        failure.map_or(Ok(()), Err)
    }
    pub fn stop_owned_runtime(&self) -> Result<()> {
        let engines: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let mut failure = None;
        for engine in engines {
            if let Err(e) = engine.stop_owned_runtime() {
                failure.get_or_insert(e);
            }
        }
        failure.map_or(Ok(()), Err)
    }
    fn project_limit(&self, engine: &Arc<Engine>, limit: usize) -> Result<usize> {
        let engines: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let mut others = 0;
        for other in engines {
            if !Arc::ptr_eq(engine, &other) {
                others += other
                    .store
                    .snapshot()?
                    .runs
                    .iter()
                    .filter(|run| run.status == Status::Running)
                    .count();
            }
        }
        Ok(limit.saturating_sub(others))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn cmd(
        w: &Workspace,
        method: &str,
        scope: Option<&str>,
        target: Option<&str>,
        key: &str,
        payload: Value,
    ) -> Value {
        w.dispatch(
            method,
            json!({"project_id":scope,"command_id":key,"target":target,"payload":payload}),
        )
        .unwrap()
    }
    pub(super) fn create(w: &Workspace, title: &str) -> String {
        cmd(
            w,
            "project.create",
            None,
            None,
            &devdesk_domain::id(),
            json!({"title":title}),
        )["id"]
            .as_str()
            .unwrap()
            .into()
    }
    pub(super) fn select(w: &Workspace, id: &str) {
        cmd(
            w,
            "project.open",
            None,
            Some(id),
            &devdesk_domain::id(),
            json!({}),
        );
    }
    #[test]
    fn project_component_preferences_survive_restart_and_reject_raw_model_fields() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "Component preferences");
        let preferences = json!({"default":{"provider":"codex","model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID},"roles":[]});
        w.dispatch("project.update",json!({"project_id":project,"command_id":id(),"target":project,"expected_revision":1,"payload":{"runtime_preferences":preferences}})).unwrap();
        let mut invalid = preferences.clone();
        invalid["default"]["model"] = json!("raw-model");
        assert!(w.dispatch("project.update",json!({"project_id":project,"command_id":id(),"target":project,"expected_revision":2,"payload":{"runtime_preferences":invalid}})).is_err());
        drop(w);
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        assert_eq!(
            json!(
                w.active_engine(Some(&project))
                    .unwrap()
                    .store
                    .snapshot()
                    .unwrap()
                    .projects[0]
                    .runtime_preferences
            ),
            preferences
        );
    }

    #[test]
    fn switching_projects_isolates_state_artifacts_and_idempotency() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "项目 A");
        let engine_a = w.active_engine(Some(&a)).unwrap();
        crate::test_runtime::configure_project(&engine_a, &a);
        let fa = cmd(
            &w,
            "feature.create",
            Some(&a),
            None,
            "same-feature-command",
            json!({"project_id":a,"title":"A Feature"}),
        );
        let fid = fa["id"].as_str().unwrap();
        w.dispatch("document.revise",json!({"project_id":a,"command_id":"doc-a","target":fid,"expected_revision":1,"payload":{"kind":"requirement","title":"A 需求","body":"项目 A 专属内容"}})).unwrap();
        let artifact = engine_a.store.snapshot().unwrap().artifacts[0].clone();
        let b = create(&w, "项目 B");
        let engine_b = w.active_engine(Some(&b)).unwrap();
        crate::test_runtime::configure_project(&engine_b, &b);
        let fb = cmd(
            &w,
            "feature.create",
            Some(&b),
            None,
            "same-feature-command",
            json!({"project_id":b,"title":"B Feature"}),
        );
        assert_ne!(fa, fb);
        assert_ne!(engine_a.store.root(), engine_b.store.root());
        let view = w
            .dispatch("query.workspace", json!({"project_id":b}))
            .unwrap();
        assert_eq!(view["active_project_id"], b);
        assert_eq!(view["features"].as_array().unwrap().len(), 1);
        assert_eq!(view["features"][0]["title"], "B Feature");
        assert_eq!(view["total_projects"], 2);
        let page = w
            .dispatch("query.workspace", json!({"project_id":b,"limit":1}))
            .unwrap();
        assert_eq!(page["projects"].as_array().unwrap().len(), 1);
        assert_eq!(page["projects"][0]["id"], a);
        assert_eq!(page["active_project"]["id"], b);
        assert!(
            engine_b
                .query("query.artifact", &json!({"id":artifact.id}), 4)
                .is_err()
        );
        assert!(
            w.dispatch("query.workspace", json!({"project_id":a}))
                .is_err()
        );
        assert!(w.dispatch("feature.create",json!({"project_id":a,"command_id":"stale","payload":{"project_id":a,"title":"不能串写"}})).is_err());
        // A previously pinned request still writes to A after the active pointer changes to B.
        engine_a
            .execute(
                "feature.create",
                Command {
                    command_id: "in-flight-a".into(),
                    target: None,
                    expected_revision: None,
                    payload: json!({"project_id":a,"title":"已绑定的 A 请求"}),
                },
            )
            .unwrap();
        assert_eq!(engine_b.store.snapshot().unwrap().features.len(), 1);
        select(&w, &a);
        assert_eq!(
            cmd(
                &w,
                "feature.create",
                Some(&a),
                None,
                "same-feature-command",
                json!({"project_id":a,"title":"A Feature"})
            ),
            fa
        );
        assert_eq!(
            engine_a.artifacts.read(&artifact).unwrap(),
            "项目 A 专属内容".as_bytes()
        );
        assert_eq!(engine_a.store.snapshot().unwrap().projects.len(), 1);
        assert_eq!(
            w.dispatch("query.workspace", json!({})).unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn global_settings_definitions_and_active_project_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let old = w
            .global
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|r| r.kind == "skill")
            .unwrap()
            .clone();
        let b = create(&w, "B");
        let mut data = old.data.clone();
        data["body"] = json!("共享新修订");
        let new=w.dispatch("skill.revise",json!({"command_id":"global-definition","target":old.id,"expected_revision":old.revision,"payload":{"name":old.name,"data":data}})).unwrap();
        w.dispatch("settings.update", json!({"global_concurrency":3}))
            .unwrap();
        select(&w, &a);
        let a_state = w.active_engine(None).unwrap().store.snapshot().unwrap();
        assert!(
            a_state
                .definitions
                .iter()
                .any(|d| d.id == new["id"].as_str().unwrap())
        );
        assert_eq!(
            a_state
                .definitions
                .iter()
                .find(|d| d.id == old.id)
                .unwrap()
                .body,
            old.data["body"].as_str().unwrap()
        );
        drop(w);
        let w = Workspace::open(dir.path()).unwrap();
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
        assert_eq!(w.global.concurrency().unwrap(), 3);
        assert_eq!(
            w.dispatch("query.workspace", json!({})).unwrap()["total_projects"],
            2
        );
        select(&w, &b);
        assert!(
            w.active_engine(None)
                .unwrap()
                .store
                .snapshot()
                .unwrap()
                .definitions
                .iter()
                .any(|d| d.id == new["id"].as_str().unwrap())
        );
        assert!(!dir.path().join("runtime.json").exists());
        assert!(!dir.path().join("engineering.sqlite3").exists());
    }
    #[test]
    fn failed_open_preserves_active_project_and_creation_resumes_reserved_identity() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let c = Command {
            command_id: "reserved".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"恢复创建"}),
        };
        let reserved = w.global.reserve_project(&c).unwrap();
        drop(w);
        let w = Workspace::open(dir.path()).unwrap();
        assert_eq!(
            cmd(
                &w,
                "project.create",
                None,
                None,
                &c.command_id,
                c.payload.clone()
            )["id"],
            reserved
        );
        select(&w, &a);
        assert!(
            w.dispatch(
                "project.open",
                json!({"command_id":"missing","target":devdesk_domain::id(),"payload":{}})
            )
            .is_err()
        );
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
        std::fs::rename(
            dir.path()
                .join("projects")
                .join(&reserved)
                .join("engineering.sqlite3"),
            dir.path().join("missing.sqlite3"),
        )
        .unwrap();
        assert!(
            w.dispatch(
                "project.open",
                json!({"command_id":"deleted","target":reserved,"payload":{}})
            )
            .is_err()
        );
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
    }
}

#[cfg(test)]
mod adapter_injection_tests {
    use super::tests::{cmd, create, select};
    use super::*;
    #[test]
    fn injected_providers_are_shared_after_opening_another_project() {
        let dir = tempfile::tempdir().unwrap();
        let providers = crate::test_runtime::adapters();
        let harness = providers.harness.clone();
        let w = Workspace::open_with_adapters(dir.path(), providers).unwrap();
        let mut engines = vec![];
        for title in ["A", "B"] {
            let pid = w
                .dispatch(
                    "project.create",
                    json!({"command_id":id(),"payload":{"title":title}}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let e = w.active_engine(Some(&pid)).unwrap();
            let (_, tid) = crate::test_runtime::queue_conversation(&e, &pid);
            w.tick().unwrap();
            assert!(e.store.snapshot().unwrap().turns.iter().any(|t| t.id == tid
                && [TurnStatus::Dispatching, TurnStatus::Running].contains(&t.status)));
            engines.push(e);
        }
        for e in engines {
            assert!(Arc::ptr_eq(&e.harness, &harness));
        }
    }
    #[test]
    fn switching_keeps_inactive_native_conversation_and_shared_quota() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        w.global.set_concurrency(1).unwrap();
        let mut engines = vec![];
        for title in ["A", "B"] {
            let pid = w
                .dispatch(
                    "project.create",
                    json!({"command_id":id(),"payload":{"title":title}}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let e = w.active_engine(Some(&pid)).unwrap();
            crate::test_runtime::queue_conversation(&e, &pid);
            w.tick().unwrap();
            engines.push(e);
        }
        assert_eq!(engines[0].conversation_running().unwrap(), 1);
        assert_eq!(engines[1].conversation_running().unwrap(), 0);
        assert_eq!(
            engines[1].store.snapshot().unwrap().turns[0].status,
            TurnStatus::Queued
        );
    }
    #[test]
    fn project_owned_role_has_one_global_history_and_multiple_project_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let one = create(&w, "One");
        let two = create(&w, "Two");
        let other = create(&w, "Other");
        let role=w.dispatch("role.create",json!({"command_id":id(),"payload":{"name":"Shared specialist","scope":{"kind":"project","project_id":one},"data":{"execution_role":"analyst","instructions":"# Shared duty","visibility":{"kind":"projects","project_ids":[one,two]}}}})).unwrap();
        for pid in [&one, &two, &other] {
            select(&w, pid);
            let roles = w
                .dispatch(
                    "query.roles",
                    json!({"local":true,"project_id":pid,"limit":100}),
                )
                .unwrap();
            assert_eq!(
                roles["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["id"] == role["id"]),
                pid != &other
            );
        }
        let revised=w.dispatch("role.revise",json!({"command_id":id(),"target":role["id"],"expected_revision":role["revision"],"payload":{"name":"Narrowed specialist","data":{"execution_role":"analyst","instructions":"# Shared duty v2","visibility":{"kind":"projects","project_ids":[one]}}}})).unwrap();
        select(&w, &two);
        let roles = w
            .dispatch(
                "query.roles",
                json!({"local":true,"project_id":two,"include_history":true,"limit":100}),
            )
            .unwrap();
        assert!(
            !roles["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["definition_id"] == role["definition_id"])
        );
        let global = w.global.snapshot().unwrap();
        assert_eq!(
            global
                .governance
                .iter()
                .filter(|r| r.kind == "role" && json!(r.definition_id) == role["definition_id"])
                .count(),
            2
        );
        assert_eq!(
            global
                .configuration(revised["id"].as_str().unwrap())
                .unwrap()
                .scope,
            Scope::Project { project_id: one }
        );
    }
    #[test]
    fn composition_preserves_separate_harness_and_sandbox_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let pid = create(&w, "Permission references");
        let create_resource = |method: &str, payload: Value| {
            w.dispatch(method, json!({"command_id":id(),"payload":payload}))
                .unwrap()
        };
        let harness = create_resource(
            "harness.create",
            json!({"name":"Explicit pinned Harness","data":{"provider":"codex","runtime_version":"0.167.0","model_providers":["openai"],"capabilities":[]}}),
        );
        let capability = create_resource(
            "capability.create",
            json!({"name":"Explicit capability","data":{"model_ref":DEFAULT_MODEL_ID,"harness_ref":harness["id"],"tools":[DEFAULT_NATIVE_TOOL_ID],"skills":[]}}),
        );
        let agent = create_resource(
            "agent.create",
            json!({"name":"Permission pair","data":{"role_ref":default_role_id("analyst"),"capability_ref":capability["id"],"permission_selection":default_permission_selection()}}),
        );
        let e = w.active_engine(Some(&pid)).unwrap();
        w.sync_configuration(&e).unwrap();
        let mut command = Command {
            command_id: id(),
            target: Some(pid),
            expected_revision: Some(1),
            payload: json!({"agent":agent["id"]}),
        };
        w.resolve_agent_composition(&e, &mut command).unwrap();
        assert!(command.payload.get("permission_overrides").is_none());
        let state = e.store.snapshot().unwrap();
        let a: AgentComposition = state
            .configuration(agent["id"].as_str().unwrap())
            .unwrap()
            .decode()
            .unwrap();
        assert!(a.permission_selection.is_some());
    }
    #[test]
    fn visibility_narrowing_blocks_new_use_and_preserves_frozen_agent_instances() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let one = create(&w, "Visible");
        let two = create(&w, "Frozen");
        let e = w.active_engine(Some(&two)).unwrap();
        crate::test_runtime::configure_project(&e, &two);
        let feature = cmd(
            &w,
            "feature.create",
            Some(&two),
            None,
            "frozen-feature",
            json!({"project_id":two,"title":"Frozen before scope change"}),
        );
        let fid = feature["id"].as_str().unwrap();
        let frozen = json!(
            e.store
                .snapshot()
                .unwrap()
                .feature_configuration(fid)
                .unwrap()
        );
        w.dispatch("role.revise",json!({"command_id":id(),"target":default_role_id("analyst"),"expected_revision":1,"payload":{"name":"Narrow analyst","data":{"execution_role":"analyst","instructions":"# Narrow responsibility","visibility":{"kind":"projects","project_ids":[one]}}}})).unwrap();
        w.sync_configuration(&e).unwrap();
        assert_eq!(e.project_configuration_status().unwrap()["ready"], false);
        let mut command = Command {
            command_id: id(),
            target: Some(two.clone()),
            expected_revision: Some(2),
            payload: json!({"agent":default_configured_agent_id("analyst")}),
        };
        assert!(w.resolve_agent_composition(&e, &mut command).is_err());
        let payload = json!({"name":"Continuation instance","scope":{"kind":"project","project_id":two},"agent_revision_id":default_configured_agent_id("analyst")});
        assert!(
            e.execute(
                "agent.instance.create",
                Command {
                    command_id: id(),
                    target: None,
                    expected_revision: None,
                    payload: payload.clone()
                }
            )
            .is_err()
        );
        let mut payload = payload;
        payload["frozen_feature_id"] = json!(fid);
        let instance = e
            .execute(
                "agent.instance.create",
                Command {
                    command_id: id(),
                    target: None,
                    expected_revision: None,
                    payload,
                },
            )
            .unwrap();
        assert_eq!(
            instance["data"]["agent_revision_id"],
            default_configured_agent_id("analyst")
        );
        assert_eq!(
            json!(
                e.store
                    .snapshot()
                    .unwrap()
                    .feature_configuration(fid)
                    .unwrap()
            ),
            frozen
        );
    }
}

#[cfg(test)]
mod global_discussion_tests {
    use super::*;
    #[test]
    fn dataset_discussions_are_separate_from_project_selection_and_require_an_unambiguous_scope() {
        let root = tempfile::tempdir().unwrap();
        let w =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let empty = w
            .dispatch(
                "query.sessions",
                json!({"scope":"global","owner_id":"global"}),
            )
            .unwrap();
        assert_eq!(empty["total"], 0);
        assert!(w.active_id().is_none());
        assert!(
            SqliteStore::read_global_discussion_snapshot(root.path())
                .unwrap()
                .projects
                .is_empty()
        );
        let project = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Separate selection"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        w.dispatch("query.sessions", json!({"scope":"global"}))
            .unwrap();
        assert_eq!(w.active_id().as_deref(), Some(project.as_str()));
        assert!(
            w.dispatch(
                "query.sessions",
                json!({"scope":"global","project_id":project})
            )
            .is_err()
        );
    }
}
