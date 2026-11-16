#[cfg(test)]
use crate::default_fixture::*;
#[path = "configuration_read.rs"]
mod configuration_read;
#[path = "github_projects.rs"]
mod github_projects;
#[path = "governance.rs"]
mod governance;
#[cfg(test)]
#[path = "history_tests.rs"]
mod history_tests;
#[path = "integrations.rs"]
mod integrations;
#[path = "model_providers.rs"]
mod model_providers;
#[path = "permission_resources.rs"]
mod permission_resources;
#[path = "runtime_setup.rs"]
mod runtime_setup;
#[path = "templates.rs"]
mod templates;
#[cfg(test)]
#[path = "workflow_live.rs"]
mod workflow_live;
// Project selection pins every request to one Engine and therefore one database.
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
};

pub struct Workspace {
    pub global: Arc<GlobalStore>,
    codex_auth: codex_auth::CodexAuthentication,
    configuration: Arc<Engine>,
    adapters: ports::ExecutionAdapters,
    opened: Mutex<BTreeMap<String, Arc<Engine>>>,
    switching: Mutex<()>,
    template_gate: Arc<Mutex<()>>,
    runtime_setup: session_resources::RuntimePreparation,
    configuration_views: Mutex<Vec<(String, Arc<ConfigurationCatalogSnapshot>)>>,
    catalog_instance: String,
    event_hubs: Mutex<BTreeMap<String, std::sync::Weak<crate::event_hub::EventHub>>>,
    scheduler_next: std::sync::atomic::AtomicUsize,
    directory_next: std::sync::atomic::AtomicUsize,
}
pub struct RpcOutcome {
    pub value: Value,
    pub commit: Option<Value>,
}
pub(super) fn engine(
    store: Arc<dyn EngineeringStore>,
    adapters: &ports::ExecutionAdapters,
) -> Arc<Engine> {
    let root = PathBuf::from(store.root());
    let data_root = root
        .ancestors()
        .find(|p| p.join("global.sqlite3").is_file())
        .unwrap_or(&root)
        .to_owned();
    let artifacts = Arc::new(FileArtifacts::new(PathBuf::from(store.root())));
    let tools = Arc::new(
        devdesk_adapters::integrations::NativeTools::new(store.root())
            .expect("existing execution root"),
    );
    Arc::new(
        Engine::new(store, artifacts, adapters.clone())
            .with_context_reader(Arc::new(experience::Reader::new(data_root)))
            .with_tool_executor(tools)
            .with_configuration_backend(Arc::new(devdesk_adapters::ConfigurationBackend)),
    )
}
fn workspace_defaults(e: &Engine) -> Result<()> {
    e.ensure_default_permissions(&permission_profiles::permission_contract)
}

impl Workspace {
    fn open_conversation(&self, e: &Engine, command: Command) -> Result<Value> {
        e.open_prepared_session(&self.session_resources(), command)
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
        if template_catalog_generation(&global.snapshot()?) == TEMPLATE_INITIAL_GENERATION
            && global.permission_preset_source()?.is_builtin
        {
            global.reload_permission_presets(&Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({}),
            })?;
        }
        workspace_defaults(&configuration)?;
        let workspace = Arc::new(Self {
            codex_auth: codex_auth::CodexAuthentication::new(root)?,
            global,
            configuration,
            adapters,
            opened: Mutex::new(BTreeMap::new()),
            switching: Mutex::new(()),
            template_gate: Arc::new(Mutex::new(())),
            runtime_setup: session_resources::RuntimePreparation::default(),
            configuration_views: Default::default(),
            catalog_instance: id(),
            event_hubs: Default::default(),
            scheduler_next: Default::default(),
            directory_next: Default::default(),
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
        Ok(workspace)
    }
    pub fn project_engine(&self, project_id: Option<&str>) -> Result<Arc<Engine>> {
        let project_id =
            project_id.ok_or_else(|| err("project_required", "请求需要明确的 project_id"))?;
        require(
            project_id != GLOBAL_DISCUSSION_NAMESPACE,
            "全局讨论需要明确的讨论作用域",
        )?;
        if self.global.project(project_id)?.is_none() {
            return Err(err("not_found", "项目不存在或尚未创建完成"));
        }
        self.opened
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(project_id)
            .cloned()
            .ok_or_else(|| err("not_found", "项目尚未打开"))
    }
    fn sync_configuration(&self, e: &Engine) -> Result<()> {
        e.sync_shared_configuration(self.global.as_ref())
    }
    fn load_project(&self, id: &str, creating: bool) -> Result<Arc<Engine>> {
        let _span = performance::Span::new("project.open");
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
        let e = Arc::new(
            Arc::try_unwrap(e)
                .map_err(|_| err("storage", "装配项目服务失败"))?
                .with_manager_services(ManagerServices {
                    shared: self.configuration.clone(),
                    inventory: self.global.clone(),
                    gate: self.template_gate.clone(),
                }),
        );
        if !creating {
            require(
                e.store.snapshot()?.projects.len() == 1,
                "项目数据库未包含有效 Project",
            )?;
        }
        self.configuration.recover_configuration_transfers(&e)?;
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
        match e.store.project_metadata() {
            Ok(project) => self.global.publish_project(&project)?,
            Err(error) if creating && error.code == "not_found" => {}
            Err(error) => return Err(error),
        }
        self.opened.lock().unwrap().insert(id.into(), e.clone());
        Ok(e)
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
        if (method == "session.open" || method == "experience.restore")
            && let Some(instance) = params["payload"]["agent_instance_id"].as_str()
        {
            let sources = experience::discussion_catalog(Path::new(&self.global.root()))?;
            e.import_discussion_agent(&sources, instance)?;
        }
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
            if name == "query.session"
                && let Some(id) = params["id"].as_str()
            {
                result["permission_status"] = self.permission_session_status(&e, id)?;
            }
            return Ok(result);
        }
        e.execute(
            method,
            serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?,
        )
    }
    #[cfg(test)]
    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        self.dispatch_with_receipt(method, params)
            .map(|outcome| outcome.value)
    }
    pub fn dispatch_with_receipt(&self, method: &str, params: Value) -> Result<RpcOutcome> {
        if method == "credentials.configure" {
            let command = devdesk_core::credential_updates::parse(&params)?;
            let vault = credentials::CredentialVault::new(Path::new(&self.global.root()))?;
            let receipt = devdesk_core::credential_updates::configure(
                self.global.as_ref(),
                &vault,
                &command,
                &InvocationContext::human(None),
            )?;
            return Ok(RpcOutcome {
                value: receipt.result,
                commit: Some(
                    json!({"namespace":self.global.namespace(),"versions":receipt.versions,"cursor":receipt.cursor}),
                ),
            });
        }
        if method == "query.execution_workspace" {
            return self.execution_workspace(&params).map(|value| RpcOutcome {
                value,
                commit: None,
            });
        }
        if [
            "query.orchestration_summary",
            "query.workflow_driver_summary",
            "query.orchestration_decision",
            "query.workflow_management_summary",
            "query.workflow_progress",
        ]
        .contains(&method)
        {
            if method == "query.workflow_progress" {
                devdesk_core::read_models::fields(&params, &["project_id", "id", "stage_offset"])?;
            } else if method == "query.workflow_management_summary" {
                devdesk_core::read_models::fields(&params, &["project_id", "id", "child_offset"])?;
            } else if method != "query.orchestration_summary" {
                devdesk_core::read_models::fields(&params, &["project_id", "id"])?;
            }
            let project = devdesk_core::read_models::identifier(&params, "project_id")?;
            let engine = self.project_engine(Some(&project))?;
            let value = match method {
                "query.orchestration_summary" => json!(engine.store.orchestration_summary(
                    &devdesk_core::read_models::ControllerQuery::parse(&params)?
                )?),
                "query.workflow_driver_summary" => json!(engine.store.workflow_driver_summary(
                    &devdesk_core::read_models::identifier(&params, "id")?
                )?),
                "query.workflow_progress" => {
                    let offset = params
                        .get("stage_offset")
                        .map(|value| {
                            value
                                .as_u64()
                                .filter(|n| *n <= 1_000_000)
                                .ok_or_else(|| err("invalid_params", "阶段页偏移无效"))
                        })
                        .transpose()?
                        .unwrap_or(0);
                    json!(engine.store.workflow_progress(
                        &devdesk_core::read_models::identifier(&params, "id")?,
                        offset
                    )?)
                }
                "query.workflow_management_summary" => {
                    let offset = params
                        .get("child_offset")
                        .map(|value| {
                            value
                                .as_u64()
                                .filter(|offset| *offset <= 1_000_000)
                                .ok_or_else(|| err("invalid_params", "子流程页偏移无效"))
                        })
                        .transpose()?
                        .unwrap_or(0);
                    json!(engine.store.workflow_management_summary(
                        &devdesk_core::read_models::identifier(&params, "id")?,
                        offset
                    )?)
                }
                _ => {
                    json!(engine.store.workflow_decision(
                        &devdesk_core::read_models::identifier(&params, "id")?
                    )?)
                }
            };
            return Ok(RpcOutcome {
                value,
                commit: None,
            });
        }
        if method == "runtime.register" {
            require(
                params.as_object().is_some_and(|fields| {
                    fields.keys().all(|key| {
                        ["command_id", "target", "expected_revision", "payload"]
                            .contains(&key.as_str())
                    })
                }),
                "安装登记包含未知字段",
            )?;
            let command: Command = serde_json::from_value(params)
                .map_err(|error| err("invalid_params", error.to_string()))?;
            let receipt = self
                .global
                .register_runtime_command(&command, &InvocationContext::human(None))?;
            return Ok(RpcOutcome {
                value: receipt.result,
                commit: Some(
                    json!({"namespace":self.global.namespace(),"versions":receipt.versions,"cursor":receipt.cursor}),
                ),
            });
        }
        if method == "query.settings" {
            return Ok(RpcOutcome {
                value: json!(devdesk_core::settings::read(self.global.as_ref())?),
                commit: None,
            });
        }
        if method == "settings.update" {
            require(
                params.as_object().is_some_and(|fields| {
                    fields.keys().all(|key| {
                        [
                            "command_id",
                            "target",
                            "expected_revision",
                            "payload",
                            "preconditions",
                        ]
                        .contains(&key.as_str())
                    })
                }),
                "设置请求包含未知字段",
            )?;
            let preconditions = devdesk_core::settings::preconditions(&params)?;
            let command: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            let receipt = devdesk_core::settings::save(
                self.global.as_ref(),
                &command,
                &preconditions,
                &InvocationContext::human(None),
            )?;
            return Ok(RpcOutcome {
                value: receipt.result,
                commit: Some(
                    json!({"namespace":self.global.namespace(),"versions":receipt.versions,"cursor":receipt.cursor}),
                ),
            });
        }
        self.dispatch_legacy(method, params)
            .map(|value| RpcOutcome {
                value,
                commit: None,
            })
    }
    fn dispatch_legacy(&self, method: &str, params: Value) -> Result<Value> {
        if method == "query.ui_bootstrap" {
            return self.ui_bootstrap(&params);
        }
        if method == "query.project_directory" {
            return self.project_directory(&params);
        }
        if let Some(result) = self.configuration_read(method, &params) {
            return result;
        }

        let coordinated = template_definition_write(method)
            || template_catalog_query(method)
            || method == "configuration.templates.reset"
            || ["project.configuration.update", "work.configuration.update"].contains(&method)
            || matches!(
                method,
                "project.create" | "project.open_github" | "workflow.start" | "feature.create"
            );
        let _guard = if coordinated {
            let _waiting = performance::Span::new("catalog.gate_wait");
            Some(
                self.template_gate
                    .lock()
                    .map_err(|e| err("storage", e.to_string()))?,
            )
        } else {
            None
        };
        if let Some(result) = self.template_dispatch(method, &params) {
            return result;
        }
        if template_definition_write(method) {
            if method == "skill.import_package" {
                let command: Command = serde_json::from_value(params.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                let engine = self.integration_engine(method, &params)?;
                if let Some(value) = engine.replay_integration_operation(
                    method,
                    &command,
                    &InvocationContext::human(engine.store.project_id()),
                )? {
                    return Ok(value);
                }
            } else if let Ok(command) = serde_json::from_value::<Command>(params.clone()) {
                let fingerprint = command_fingerprint(method, &command);
                if let Some(saved) = self.global.replay(&command.command_id, &fingerprint)? {
                    return Ok(saved);
                }
                if let Ok(engine) = self.project_engine(params["project_id"].as_str())
                    && let Some(saved) = engine.store.replay(&command.command_id, &fingerprint)?
                {
                    return Ok(saved);
                }
            }
            let state =
                self.global
                    .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
                        "governance",
                        TEMPLATE_CATALOG_STATE,
                    )])?;
            let generation = state[0]
                .value
                .as_ref()
                .and_then(|row| row["data"]["generation"].as_u64())
                .unwrap_or(TEMPLATE_INITIAL_GENERATION);
            if (params.get("expected_catalog_generation").is_some()
                || generation > TEMPLATE_INITIAL_GENERATION)
                && params["expected_catalog_generation"].as_u64() != Some(generation)
            {
                return Err(err("catalog_changed", "模板目录已重置，请重新载入后再编辑"));
            }
        }
        let params_for_sources = params.clone();
        let mut result = self.dispatch_inner(method, params)?;
        if template_catalog_query(method) {
            let global = self.global.snapshot()?;
            let local = if params_for_sources["local"] == true
                || params_for_sources["project_id"].is_string()
            {
                self.project_engine(params_for_sources["project_id"].as_str())
                    .ok()
                    .and_then(|e| e.store.snapshot().ok())
            } else {
                None
            };
            let directory = local.as_ref().unwrap_or(&global);
            let sources=result["records"].as_array().map(|rows|rows.iter().filter_map(|row|{
                let r=directory.configuration(row["id"].as_str()?).ok().or_else(||global.configuration(row["id"].as_str()?).ok())?;
                let member=template_catalog_member(directory,r).or_else(||template_catalog_member(&global,r));
                let origin=if !template_record_current(directory,r){"archived"}else if member.is_some_and(|m|m.data["template_key"].is_string()){if member.unwrap().data["source_hash"]==template_source_hash(r) {"default"}else{"modified"}}else if member.is_some_and(|m|m.data["origin"]=="existing"){"existing"}else{"custom"};
                Some((r.id.clone(),json!({"origin":origin,"template_key":member.map(|m|&m.data["template_key"])})))
            }).collect::<serde_json::Map<String,Value>>()).unwrap_or_default();
            if let Some(object) = result.as_object_mut() {
                object.insert(
                    "catalog_generation".into(),
                    json!(template_catalog_generation(&global)),
                );
                object.insert("template_sources".into(), json!(sources));
            }
        }
        Ok(result)
    }
    fn dispatch_inner(&self, method: &str, params: Value) -> Result<Value> {
        if method == "query.discussion_agents" {
            let records = experience::discussion_catalog(Path::new(&self.global.root()))?;
            let offset = params["offset"].as_u64().unwrap_or(0) as usize;
            let limit = params["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
            let requested: Option<Vec<String>> = params
                .get("refs")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| err("invalid_params", e.to_string()))?;
            require(
                requested.as_ref().is_none_or(|refs| refs.len() <= 64),
                "一次最多读取 64 个确切修订",
            )?;
            let records: Vec<_> = records
                .into_iter()
                .filter(|r| requested.as_ref().is_none_or(|refs| refs.contains(&r.id)))
                .collect();
            let page = records
                .iter()
                .skip(offset)
                .take(limit)
                .map(|r| {
                    let mut row = if params["view"] == "index" {
                        configuration_selection_record(r)
                    } else {
                        json!(r)
                    };
                    if r.r#type == "agent_instance"
                        && let Some(project) = r
                            .scope
                            .project_id()
                            .and_then(|pid| self.global.project(pid).ok().flatten())
                    {
                        row["origin_project_name"] = json!(project.title);
                    }
                    row
                })
                .collect::<Vec<_>>();
            return Ok(json!({"records":page,"total":records.len()}));
        }
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
                "query.tool_calls",
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
                self.project_engine(params["project_id"].as_str())?
            };
            let state = engine.store.snapshot()?;
            let harness = state.configuration(id)?;
            require(harness.r#type == "harness", "需要 Harness 修订")?;
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
            let mut status = self.runtime_setup.status();
            status["intents"] = json!(self.global.runtime_preparation_records()?);
            return Ok(status);
        }
        if method == "runtime.preparation_cancel" {
            return self
                .global
                .cancel_runtime_preparation(params["recipe_digest"].as_str().unwrap_or(""));
        }
        if method == "runtime.preparation_reconcile" {
            let digest = params["recipe_digest"].as_str().unwrap_or("");
            let provider = self.global.preparation_provider(digest)?;
            return self.runtime_setup.run(&provider, |_| {
                self.global.reconcile_runtime_preparation(digest)
            });
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
        if method == "codex.auth.preparation_status" {
            return Ok(self.runtime_setup.status());
        }
        if method == "codex.auth.preparation_cancel" {
            self.runtime_setup.cancel_auth();
            return Ok(json!({"cancel_requested":true}));
        }
        if method == "codex.auth.status" && params["installation_id"].is_null() {
            return Ok(self.codex_auth.local_status());
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
                "codex.auth.start" => {
                    require(!self.runtime_setup.auth_cancelled(), "登录准备已取消")?;
                    let result = self.codex_auth.start_observed(
                        &self.global,
                        installation,
                        params["device"].as_bool().unwrap_or(false),
                        &|message| self.runtime_setup.auth_progress(message),
                    );
                    if self.runtime_setup.auth_cancelled() {
                        if let Ok(value) = &result
                            && let Some(login) = value["loginId"].as_str()
                        {
                            self.codex_auth.cancel(&self.global, installation, login)?;
                        }
                        Err(err("cancelled", "登录已取消，所属认证请求已停止"))
                    } else {
                        result
                    }
                }
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
        if method == "query.runtime_installations" {
            let settings = devdesk_core::settings::read(self.global.as_ref())?;
            return Ok(json!({"installations":self.global.runtime_installations()?,
                "default_harness":settings.default_harness,"settings_versions":settings.versions}));
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
            let providers:Vec<_>=self.model_provider_facts()?.into_iter().map(|(_,p)|{
                let reference=self.global.model_credential_reference(&p).ok().flatten();
                let status=vault.status_model_reference(&p,reference.as_deref()).ok();
                json!({"id":p.id,"label":p.label,"configured":status.as_ref().is_some_and(|s|s.configured),"available":status.as_ref().is_some_and(|s|s.available),"auth_kind":if p.requires_key{"api_key"}else{"none"}})
            }).collect();
            let recipe_state =
                if let Ok(engine) = self.project_engine(params["project_id"].as_str()) {
                    engine.store.snapshot()?
                } else {
                    self.global.snapshot()?
                };
            let recipes: Vec<_> = recipe_state.governance.iter().filter(|r|r.r#type == "harness" && !devdesk_core::configuration_retired(&recipe_state, r))
                .filter_map(|r| harness_execution_recipe(&recipe_state,r).ok().map(|d|json!({"reference":r.id,"name":r.name,"provider":d.provider,"version":d.version,"recipe_digest":d.recipe_digest(),"os":"linux","arch":std::env::consts::ARCH}))).collect();
            return Ok(
                json!({"default_harness":self.global.default_harness()?,"host_os":std::env::consts::OS,"host_arch":std::env::consts::ARCH,
                "installations":installations,"model_providers":providers,"recipes":recipes,"drivers":devdesk_core::driver_capabilities()}),
            );
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
            let id = self
                .global
                .reserve_project(&command, &devdesk_core::InvocationContext::human(None))?;
            let e = self.load_project(&id, true)?;
            self.sync_configuration(&e)?;
            let result = e.execute(method, command)?;
            self.global.publish_project(&e.store.project_metadata()?)?;
            return Ok(result);
        }
        if method == "project.open" {
            return Err(err(
                "method_not_found",
                "项目选择属于前端；请用 project_id 发起查询或命令",
            ));
        }
        if ["agent.create", "agent.revise"].contains(&method) {
            let c =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            return self.configuration.execute(method, c);
        }
        let expected = params["project_id"]
            .as_str()
            .or_else(|| params["payload"]["project_id"].as_str());
        let e = match self.project_engine(expected) {
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
            if method == "session.read"
                && let Some(sid) = params["id"].as_str()
            {
                result["permission_status"] = self.permission_session_status(&e, sid)?;
            }
            return Ok(result);
        }
        let global_limit = self.global.concurrency()?;
        if method.starts_with("query.") {
            let mut result = e.query(method, &params, self.project_limit(&e, global_limit)?)?;
            if method == "query.session"
                && let Some(sid) = params["id"].as_str()
            {
                result["permission_status"] = self.permission_session_status(&e, sid)?;
            }
            if method == "query.workspace" {
                let offset = params["project_offset"].as_u64().unwrap_or(0) as usize;
                let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
                let (mut projects, total) = if params["github_only"] == true {
                    self.global.github_projects(offset, limit)?
                } else {
                    self.global.projects(offset, limit)?
                };
                let current = if e.store.project_id().is_some() {
                    Some(e.store.project_metadata()?)
                } else {
                    None
                };
                if let Some(project) = &current
                    && let Some(row) = projects.iter_mut().find(|p| p.id == project.id)
                {
                    *row = project.clone();
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
                let settings = devdesk_core::settings::read(self.global.as_ref())?;
                result["global_concurrency"] = json!(settings.global_concurrency);
                result["default_harness"] = json!(settings.default_harness);
                result["settings_versions"] = json!(settings.versions);
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
        self.global.publish_project(&e.store.project_metadata()?)?;
        Ok(result)
    }
    pub(crate) fn subscribe_configuration_changes(
        &self,
        listener: std::sync::Weak<dyn Fn() + Send + Sync>,
    ) {
        for engine in self.opened.lock().unwrap().values() {
            engine.store.subscribe_changes(listener.clone());
        }
    }
    pub fn event_engine(&self, expected: Option<&str>) -> Result<(String, Arc<Engine>)> {
        match self.project_engine(expected) {
            Ok(e) => Ok((e.store.project_id().unwrap(), e)),
            Err(error) if error.code == "project_required" => {
                Ok(("global".into(), self.configuration.clone()))
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) fn event_hub(
        self: &Arc<Self>,
        scope: &str,
        engine: Arc<Engine>,
    ) -> Result<Arc<crate::event_hub::EventHub>> {
        let mut hubs = self
            .event_hubs
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        hubs.retain(|_, hub| hub.strong_count() > 0);
        if let Some(hub) = hubs.get(scope).and_then(std::sync::Weak::upgrade) {
            return Ok(hub);
        }
        require(hubs.len() < 128, "活跃事件作用域超过预算")?;
        let hub = crate::event_hub::EventHub::new(self, engine, scope.to_owned());
        hubs.insert(scope.to_owned(), Arc::downgrade(&hub));
        Ok(hub)
    }
    pub fn tick(&self, should_stop: impl Fn() -> bool) -> Result<()> {
        if should_stop() {
            return Ok(());
        }
        self.configuration.drive_tool_calls()?;
        if should_stop() {
            return Ok(());
        }
        let opened: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let mut failure = None;
        // Repair directory summaries independently of pending workflow work.
        // The rotation includes idle projects; frontend queries never publish.
        if !opened.is_empty() {
            let start = self.directory_next.load(Ordering::Relaxed) % opened.len();
            let started = std::time::Instant::now();
            let mut visited = 0;
            for engine in opened.iter().cycle().skip(start).take(opened.len().min(8)) {
                if should_stop()
                    || (visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25))
                {
                    break;
                }
                visited += 1;
                if engine.store.project_id().is_some() {
                    match engine.store.project_metadata() {
                        Ok(project) => {
                            if let Err(error) = self.global.publish_project(&project) {
                                failure.get_or_insert(error);
                            }
                        }
                        Err(error) if error.code == "not_found" => {}
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                }
                if let Err(error) = engine.publish_experience_index(self.global.as_ref()) {
                    failure.get_or_insert(error);
                }
            }
            self.directory_next.fetch_add(visited, Ordering::Relaxed);
        }
        let mut engines = vec![];
        let mut running = vec![];
        let mut total = 0;
        for engine in opened {
            if should_stop() {
                return Ok(());
            }
            let hints = engine.store.scheduling_hints()?;
            // Every live writer consumes global quota, including work not selected
            // for this bounded scheduler round.
            total += hints.running_conversations;
            if hints.pending && hints.live {
                running.push(hints.running_conversations);
                engines.push(engine);
            }
        }
        if !engines.is_empty() {
            let start = self.scheduler_next.load(Ordering::Relaxed) % engines.len();
            engines.rotate_left(start);
            running.rotate_left(start);
        }
        let limit = self.global.concurrency()?;
        let started = std::time::Instant::now();
        let mut visited = 0;
        // The scheduler is single-threaded across projects. Other running projects consume quota.
        for (index, e) in engines.iter().enumerate() {
            // Finish a project's current stages, then rotate to the next project
            // on the next round. A slow first project cannot always start first.
            if visited >= 8
                || (visited > 0 && started.elapsed() >= std::time::Duration::from_millis(250))
            {
                break;
            }
            // Finish the current scheduler stage, then leave remaining work queued.
            // The Host waits for this tick before stopping and sealing owned writers.
            if should_stop() {
                break;
            }
            visited += 1;
            if let Err(error) = self.tick_workflow_engine(e, &should_stop) {
                failure.get_or_insert(error);
            }
            if should_stop() {
                break;
            }
            if let Err(error) =
                e.tick_until(limit.saturating_sub(total - running[index]), &should_stop)
            {
                failure.get_or_insert(error);
            }
            if should_stop() {
                break;
            }
            if let Err(error) = self.tick_workflow_engine(e, &should_stop) {
                failure.get_or_insert(error);
            }
            if should_stop() {
                break;
            }
            if let Err(error) = e
                .store
                .project_metadata()
                .and_then(|project| self.global.publish_project(&project))
            {
                failure.get_or_insert(error);
            }
            let next = e.conversation_running()?;
            total = total - running[index] + next;
            running[index] = next;
        }
        self.scheduler_next.fetch_add(visited, Ordering::Relaxed);
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
                others += other.conversation_running()?;
            }
        }
        Ok(limit.saturating_sub(others))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn idle_rounds_publish_experience_metadata_without_restoring_project_history() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&workspace, "Idle experience metadata");
        let engine = workspace.project_engine(Some(&project)).unwrap();
        engine.store.transact(None,&mut|state|{
            let data=json!({"project_id":project,"project_name":"Idle experience metadata","agent_instance_id":"fixture-agent","snapshot_id":"fixture-snapshot","session_id":"fixture-session","captured_at":1});
            state.governance.push(ConfigurationRecord {id:"fixture-experience".into(),r#type:"experience".into(),definition_id:"fixture-snapshot".into(),scope:Scope::project(&project),name:"Fixture metadata".into(),version:1,supersedes:None,content_hash:content_digest(&References::new(),&data),refs:References::new(),data,immutable:true,revision:1,created_at:1});
            Ok(Value::Null)
        }).unwrap();
        assert!(!engine.store.scheduling_hints().unwrap().pending);
        let trace = performance::Trace::start();
        workspace.tick(|| false).unwrap();
        let report = trace.finish();
        let key = devdesk_core::experience_directory::resource_key(
            &engine.store.namespace(),
            "fixture-experience",
        );
        let row = workspace.global.read_resources(&[key]).unwrap().remove(0);
        let row = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
        assert_eq!(row.data["value"]["snapshot_id"], "fixture-snapshot");
        for stage in [
            "project.full_snapshot",
            "global.full_snapshot",
            "project.transaction",
            "global.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(report["spans"][stage].is_null(), "{report}");
        }
    }
    #[test]
    fn scheduler_polls_each_writer_once_and_shares_available_quota() {
        struct CountingHarness(AtomicUsize);
        impl ports::Harness for CountingHarness {
            fn open_session(&self, request: &ports::SessionStart) -> Result<ports::SessionReceipt> {
                ports::Harness::open_session(&crate::test_runtime::IdleHarness, request)
            }
            fn dispatch_turn(&self, _: &ports::TurnDispatch) -> Result<()> {
                Ok(())
            }
            fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(vec![])
            }
        }
        let root = tempfile::tempdir().unwrap();
        let harness = Arc::new(CountingHarness(AtomicUsize::new(0)));
        let mut adapters = crate::test_runtime::adapters();
        adapters.harness = harness.clone();
        let workspace = Workspace::open_with_adapters(root.path(), adapters).unwrap();
        workspace.global.set_concurrency(3).unwrap();
        let first = create(&workspace, "First");
        let first = workspace.project_engine(Some(&first)).unwrap();
        let first_id = first.store.project_id().unwrap();
        crate::test_runtime::queue_conversation(&first, &first_id);
        workspace.tick(|| false).unwrap();
        assert_eq!(first.conversation_running().unwrap(), 1);
        harness.0.store(0, Ordering::SeqCst);
        // Existing writers are counted once. Poll the original writer and admit
        // another in the same round, then poll each exactly once next round.
        crate::test_runtime::queue_conversation(&first, &first_id);
        workspace.tick(|| false).unwrap();
        assert_eq!(first.conversation_running().unwrap(), 2);
        assert_eq!(harness.0.load(Ordering::SeqCst), 1);
        harness.0.store(0, Ordering::SeqCst);
        workspace.tick(|| false).unwrap();
        assert_eq!(harness.0.load(Ordering::SeqCst), 2);
        let second = create(&workspace, "Second");
        let second = workspace.project_engine(Some(&second)).unwrap();
        let second_id = second.store.project_id().unwrap();
        crate::test_runtime::queue_conversation(&second, &second_id);
        crate::test_runtime::queue_conversation(&second, &second_id);
        for _ in 0..8 {
            workspace.tick(|| false).unwrap();
            let total =
                first.conversation_running().unwrap() + second.conversation_running().unwrap();
            assert!(total <= 3);
            if total == 3 {
                break;
            }
        }
        assert!(first.conversation_running().unwrap() >= 1);
        assert!(second.conversation_running().unwrap() >= 1);
        assert_eq!(
            first.conversation_running().unwrap() + second.conversation_running().unwrap(),
            3
        );
        assert_eq!(
            [first, second]
                .iter()
                .map(|engine| engine
                    .store
                    .snapshot()
                    .unwrap()
                    .turns
                    .iter()
                    .filter(|turn| turn.status == TurnStatus::Queued)
                    .count())
                .sum::<usize>(),
            1
        );
    }
    #[test]
    fn workspace_reads_do_not_publish_directory_state_and_idle_ticks_repair_it() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let created=workspace.dispatch("project.create",json!({"command_id":"directory-project","payload":{"title":"Original directory title"}})).unwrap();
        let project = created["id"].as_str().unwrap();
        let engine = workspace.project_engine(Some(project)).unwrap();
        let current = engine.store.project_metadata().unwrap();
        engine
            .execute(
                "project.update",
                Command {
                    command_id: "background-title".into(),
                    target: Some(project.into()),
                    expected_revision: Some(current.revision),
                    payload: json!({"title":"Background authoritative title"}),
                },
            )
            .unwrap();
        let trace = performance::Trace::start();
        let value = workspace
            .dispatch("query.workspace", json!({"project_id":project}))
            .unwrap();
        let report = trace.finish();
        assert_eq!(
            value["active_project"]["title"],
            "Background authoritative title"
        );
        assert!(report["spans"]["directory.publish"].is_null(), "{report}");
        assert_eq!(
            workspace.global.project(project).unwrap().unwrap().title,
            "Original directory title"
        );
        workspace.tick(|| false).unwrap();
        assert_eq!(
            workspace.global.project(project).unwrap().unwrap().title,
            "Background authoritative title"
        );
    }
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
            &devdesk_core::id(),
            json!({"title":title}),
        )["id"]
            .as_str()
            .unwrap()
            .into()
    }
    pub(super) fn read_project(w: &Workspace, id: &str) -> Value {
        w.dispatch("query.workspace", json!({"project_id":id}))
            .unwrap()
    }

    #[test]
    fn runtime_registration_uses_finite_receipts_and_rejects_another_creation() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let registration = json!({"key":{"provider":"codex","version":"protocol-fixture","os":"linux","arch":"aarch64"},"artifact":{"kind":"oci_image","reference":format!("example.com/fixture/runtime@sha256:{}","a".repeat(64))},"source":"metadata fixture; not executed","license":"test"});
        let params =
            json!({"command_id":"register-fixture","expected_revision":0,"payload":registration});
        let trace = performance::Trace::start();
        let first = workspace
            .dispatch_with_receipt("runtime.register", params.clone())
            .unwrap();
        let repeated = workspace
            .dispatch_with_receipt("runtime.register", params.clone())
            .unwrap();
        let measurement = trace.finish();
        assert_eq!(first.value, repeated.value);
        assert_eq!(first.commit, repeated.commit);
        assert_eq!(first.commit.as_ref().unwrap()["versions"][0]["revision"], 1);
        for span in [
            "global.full_snapshot",
            "global.transaction",
            "project.full_snapshot",
            "catalog.gate_wait",
        ] {
            assert!(measurement["spans"][span].is_null(), "{measurement}");
        }
        let mut another = params.clone();
        another["command_id"] = json!("another-creation");
        assert_eq!(
            workspace
                .dispatch("runtime.register", another)
                .unwrap_err()
                .code,
            "revision_conflict"
        );
        let mut missing = params;
        missing["command_id"] = json!("missing");
        missing.as_object_mut().unwrap().remove("expected_revision");
        assert_eq!(
            workspace
                .dispatch("runtime.register", missing)
                .unwrap_err()
                .code,
            "precondition_required"
        );
    }

    #[test]
    fn execution_summaries_bypass_the_catalog_gate_and_legacy_project_writer() {
        use std::{sync::mpsc, time::Duration};
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(
            Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap(),
        );
        let project = create(&workspace, "Read admission");
        let engine = workspace.project_engine(Some(&project)).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            engine.store.transact(None, &mut |_| {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(Value::Null)
            })
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let gate = workspace.template_gate.lock().unwrap();
        let (result_tx, result_rx) = mpsc::channel();
        let reader_workspace = workspace.clone();
        let reader = std::thread::spawn(move || {
            let trace = performance::Trace::start();
            let summary = reader_workspace.dispatch(
                "query.orchestration_summary",
                json!({"project_id":project,"id":"missing"}),
            );
            let driver = reader_workspace.dispatch(
                "query.workflow_driver_summary",
                json!({"project_id":project,"id":"missing"}),
            );
            let decision = reader_workspace.dispatch(
                "query.orchestration_decision",
                json!({"project_id":project,"id":"missing"}),
            );
            let management = reader_workspace.dispatch(
                "query.workflow_management_summary",
                json!({"project_id":project,"id":"missing"}),
            );
            let progress = reader_workspace.dispatch(
                "query.workflow_progress",
                json!({"project_id":project,"id":"missing"}),
            );
            let execution = reader_workspace
                .dispatch("query.execution_workspace", json!({"project_id":project}));
            let hints = reader_workspace
                .project_engine(Some(&project))
                .unwrap()
                .store
                .scheduling_hints();
            result_tx
                .send((
                    summary,
                    driver,
                    decision,
                    management,
                    progress,
                    execution,
                    hints,
                    trace.finish(),
                ))
                .unwrap();
        });
        // Test admission independently of the locks; this is not a latency benchmark.
        let result = result_rx.recv_timeout(Duration::from_secs(2));
        drop(gate);
        release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        reader.join().unwrap();
        let (summary, driver, decision, management, progress, execution, hints, trace) =
            result.expect("summary reads waited for a business lock");
        assert!(summary.unwrap()["record"].is_null());
        assert_eq!(driver.unwrap_err().code, "not_found");
        assert_eq!(decision.unwrap_err().code, "not_found");
        assert_eq!(management.unwrap_err().code, "not_found");
        assert_eq!(progress.unwrap_err().code, "not_found");
        assert_eq!(execution.unwrap()["kind"], "execution_workspace");
        assert_eq!(hints.unwrap().running_conversations, 0);
        for span in [
            "catalog.build",
            "catalog.gate_wait",
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "source.restore_reference",
        ] {
            assert!(trace["spans"][span].is_null(), "{span}: {trace}");
        }
    }

    #[test]
    fn project_updates_reject_obsolete_or_unrecognized_execution_settings() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "Explicit Agent configuration");
        for payload in [
            json!({"runtime_preferences":{"default":{},"roles":[]}}),
            json!({"role_models":{}}),
        ] {
            assert!(w.dispatch("project.update",json!({"project_id":project,"command_id":id(),"target":project,"expected_revision":1,"payload":payload})).is_err());
        }
        let state = w
            .project_engine(Some(&project))
            .unwrap()
            .store
            .snapshot()
            .unwrap();
        assert_eq!(state.projects[0].revision, 1);
        assert!(
            json!(state.projects[0])
                .get("runtime_preferences")
                .is_none()
        );
    }

    #[test]
    fn switching_projects_isolates_state_artifacts_and_idempotency() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "项目 A");
        let engine_a = w.project_engine(Some(&a)).unwrap();
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
        w.dispatch("document.revise",json!({"project_id":a,"command_id":"doc-a","target":fid,"expected_revision":engine_a.store.snapshot().unwrap().feature(fid).unwrap().revision,"payload":{"kind":"requirement","title":"A 需求","body":"项目 A 专属内容"}})).unwrap();
        let artifact = engine_a.store.snapshot().unwrap().artifacts[0].clone();
        let b = create(&w, "项目 B");
        let engine_b = w.project_engine(Some(&b)).unwrap();
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
        assert_eq!(read_project(&w, &a)["active_project_id"], a);
        w.dispatch("feature.create",json!({"project_id":a,"command_id":"independent-a","payload":{"project_id":a,"title":"A 的独立请求"}})).unwrap();
        // Requests for A keep writing to A while another frontend reads B.
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
        read_project(&w, &a);
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
            0
        );
        assert_eq!(
            read_project(&w, &a)["features"].as_array().unwrap().len(),
            3
        );
    }
    #[test]
    fn global_settings_definitions_and_project_routing_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let old = w
            .global
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|r| r.r#type == "skill")
            .unwrap()
            .clone();
        let b = create(&w, "B");
        let mut data = old.data.clone();
        data["body"] = json!("共享新修订");
        let new=w.dispatch("skill.revise",json!({"command_id":"global-definition","target":old.id,"expected_revision":old.revision,"payload":{"name":old.name,"data":data}})).unwrap();
        let versions = devdesk_core::settings::read(w.global.as_ref())
            .unwrap()
            .versions;
        let versions: Vec<_> = versions
            .into_iter()
            .filter(|v| v.resource.id == "global_concurrency")
            .collect();
        w.dispatch("settings.update", json!({"command_id":"set-limit","preconditions":versions,"payload":{"global_concurrency":3}})).unwrap();
        read_project(&w, &a);
        let a_state = w
            .project_engine(Some(&a))
            .unwrap()
            .store
            .snapshot()
            .unwrap();
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
        assert_eq!(read_project(&w, &a)["active_project_id"], a);
        assert_eq!(w.global.concurrency().unwrap(), 3);
        assert_eq!(
            w.dispatch("query.workspace", json!({})).unwrap()["total_projects"],
            2
        );
        read_project(&w, &b);
        assert!(
            w.project_engine(Some(&b))
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
    fn invalid_project_reads_preserve_other_projects_and_creation_resumes_reserved_identity() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let c = Command {
            command_id: "reserved".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"恢复创建"}),
        };
        let reserved = w
            .global
            .reserve_project(&c, &devdesk_core::InvocationContext::human(None))
            .unwrap();
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
        read_project(&w, &a);
        assert!(
            w.dispatch("query.workspace", json!({"project_id":devdesk_core::id()}))
                .is_err()
        );
        assert_eq!(read_project(&w, &a)["active_project_id"], a);
        std::fs::rename(
            dir.path()
                .join("projects")
                .join(&reserved)
                .join("engineering.sqlite3"),
            dir.path().join("missing.sqlite3"),
        )
        .unwrap();
        assert_eq!(read_project(&w, &a)["active_project_id"], a);
        drop(w);
        assert!(Workspace::open(dir.path()).is_err());
        assert!(
            !dir.path()
                .join("projects")
                .join(&reserved)
                .join("engineering.sqlite3")
                .exists()
        );
        assert!(
            dir.path()
                .join("projects")
                .join(&a)
                .join("engineering.sqlite3")
                .is_file()
        );
    }
}

#[cfg(test)]
mod adapter_injection_tests {
    use super::tests::{cmd, create, read_project};
    use super::*;
    #[test]
    fn shutdown_during_tick_preserves_queued_work_and_cleans_up_the_started_writer() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct StopOnDispatch {
            stopping: AtomicBool,
            dispatched: AtomicUsize,
            stopped: AtomicUsize,
            cleaned: AtomicUsize,
        }
        impl ports::Harness for StopOnDispatch {
            fn open_session(&self, request: &ports::SessionStart) -> Result<ports::SessionReceipt> {
                ports::Harness::open_session(&crate::test_runtime::IdleHarness, request)
            }
            fn dispatch_turn(&self, _: &ports::TurnDispatch) -> Result<()> {
                self.dispatched.fetch_add(1, Ordering::SeqCst);
                self.stopping.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
                Ok(vec![])
            }
            fn stop_session(
                &self,
                _: &Session,
                execution: &RuntimeExecution,
            ) -> Result<ports::WorkerStop> {
                self.stopped.fetch_add(1, Ordering::SeqCst);
                Ok(ports::WorkerStop {
                    operation_id: execution.operation_id.clone(),
                    writer_revoked: true,
                    mode: "shutdown-test".into(),
                    requires_successor: true,
                })
            }
            fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
                self.cleaned.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn session_log(&self, _: &RuntimeExecution) -> Result<Option<String>> {
                Ok(Some("synthetic shutdown protocol".into()))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let harness = Arc::new(StopOnDispatch {
            stopping: AtomicBool::new(false),
            dispatched: AtomicUsize::new(0),
            stopped: AtomicUsize::new(0),
            cleaned: AtomicUsize::new(0),
        });
        let mut providers = crate::test_runtime::adapters();
        providers.harness = harness.clone();
        let w = Workspace::open_with_adapters(dir.path(), providers).unwrap();
        let mut engines = vec![];
        for title in ["A", "B"] {
            let pid = create(&w, title);
            let engine = w.project_engine(Some(&pid)).unwrap();
            crate::test_runtime::queue_conversation(&engine, &pid);
            engines.push(engine);
        }
        harness.stopping.store(true, Ordering::SeqCst);
        w.tick(|| harness.stopping.load(Ordering::SeqCst)).unwrap();
        assert_eq!(harness.dispatched.load(Ordering::SeqCst), 0);
        harness.stopping.store(false, Ordering::SeqCst);
        w.tick(|| harness.stopping.load(Ordering::SeqCst)).unwrap();
        assert_eq!(
            harness.dispatched.load(Ordering::SeqCst),
            1,
            "shutdown during one project must prevent dispatch in the next project"
        );
        w.stop_owned_runtime().unwrap();
        assert_eq!(harness.stopped.load(Ordering::SeqCst), 1);
        assert_eq!(harness.cleaned.load(Ordering::SeqCst), 1);
        let states: Vec<_> = engines
            .iter()
            .map(|e| e.store.snapshot().unwrap())
            .collect();
        assert_eq!(
            states
                .iter()
                .filter(|s| s.turns[0].status == TurnStatus::Queued)
                .count(),
            1
        );
        let started = states.iter().find(|s| !s.executions.is_empty()).unwrap();
        assert_eq!(started.turns[0].status, TurnStatus::NeedsReconciliation);
        assert!(started.executions[0].writer_revoked);
        assert_eq!(started.executions[0].status, Status::Released);
        assert!(
            started
                .artifacts
                .iter()
                .any(|artifact| Some(&artifact.id)
                    == started.executions[0].protocol_artifact.as_ref())
        );
    }
    #[test]
    fn bounded_scheduler_rounds_rotate_across_more_than_one_project_budget() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let settings = w.dispatch("query.settings", json!({})).unwrap();
        let guard = settings["versions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|guard| guard["resource"]["id"] == "global_concurrency")
            .unwrap();
        w.dispatch("settings.update",json!({"command_id":"scheduler-quota","preconditions":[guard],"payload":{"global_concurrency":32}})).unwrap();
        let mut engines = Vec::new();
        for n in 0..12 {
            let pid = create(&w, &format!("Scheduler project {n}"));
            let engine = w.project_engine(Some(&pid)).unwrap();
            crate::test_runtime::queue_conversation(&engine, &pid);
            engines.push(engine);
        }
        // A round has at most eight project visits and a time budget checked
        // between project stages. All twelve must progress over later rounds.
        w.tick(|| false).unwrap();
        let started = engines
            .iter()
            .filter(|engine| engine.conversation_running().unwrap() > 0)
            .count();
        assert!(started > 0 && started <= 8, "{started}");
        for _ in 0..24 {
            if engines
                .iter()
                .all(|engine| engine.conversation_running().unwrap() > 0)
            {
                break;
            }
            w.tick(|| false).unwrap();
        }
        assert!(
            engines
                .iter()
                .all(|engine| engine.conversation_running().unwrap() > 0)
        );
        assert!(w.scheduler_next.load(Ordering::Relaxed) >= 12);
        // IdleHarness is a test-only protocol port; no native worker was started.
    }
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
            let e = w.project_engine(Some(&pid)).unwrap();
            let (_, tid) = crate::test_runtime::queue_conversation(&e, &pid);
            for _ in 0..4 {
                if e.conversation_running().unwrap() > 0 {
                    break;
                }
                w.tick(|| false).unwrap();
            }
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
            let e = w.project_engine(Some(&pid)).unwrap();
            crate::test_runtime::queue_conversation(&e, &pid);
            w.tick(|| false).unwrap();
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
        let role=w.dispatch("role.create",json!({"command_id":id(),"payload":{"name":"Shared specialist","scope":format!("project:{one}"),"data":{"instructions":"# Shared duty","visibility":{"kind":"projects","project_ids":[one,two]}}}})).unwrap();
        for pid in [&one, &two, &other] {
            read_project(&w, pid);
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
        let revised=w.dispatch("role.revise",json!({"command_id":id(),"target":role["id"],"expected_revision":role["revision"],"payload":{"name":"Narrowed specialist","data":{"instructions":"# Shared duty v2","visibility":{"kind":"projects","project_ids":[one]}}}})).unwrap();
        read_project(&w, &two);
        let roles = w
            .dispatch(
                "query.roles",
                json!({"local":true,"project_id":two,"include_history":true,"limit":100}),
            )
            .unwrap();
        assert!(
            roles["records"]
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
                .filter(|r| r.r#type == "role" && json!(r.definition_id) == role["definition_id"])
                .count(),
            2
        );
        assert_eq!(
            global
                .configuration(revised["id"].as_str().unwrap())
                .unwrap()
                .scope,
            Scope::project(one)
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
            json!({"name":"Explicit pinned Harness","model_providers":["builtin-model-provider-openai:1"],"data":w.project_engine(Some(&pid)).unwrap().store.snapshot().unwrap().configuration("builtin-harness-codex-managed:1").unwrap().data}),
        );
        let capability = create_resource(
            "capability.create",
            json!({"name":"Explicit capability","model_ref":DEFAULT_MODEL_ID,"harness_ref":harness["id"],"tools":[DEFAULT_NATIVE_TOOL_ID],"skills":[],"data":{}}),
        );
        let agent = create_resource(
            "agent.create",
            json!({"name":"Permission pair","role_ref":default_role_id("analyst"),"capability_ref":capability["id"],"data":{}}),
        );
        let e = w.project_engine(Some(&pid)).unwrap();
        w.sync_configuration(&e).unwrap();
        let mut command = Command {
            command_id: id(),
            target: Some(pid),
            expected_revision: Some(1),
            payload: json!({"agent":agent["id"]}),
        };
        e.resolve_session_composition(
            &crate::test_runtime::ResourceFixture {
                store: w.global.as_ref(),
            },
            &mut command,
        )
        .unwrap();
        assert!(command.payload.get("permission_overrides").is_none());
        let state = e.store.snapshot().unwrap();
        state.resolve_agent(agent["id"].as_str().unwrap()).unwrap();
        assert!(permission_selection(&state).ok().is_some());
    }
    #[test]
    fn role_revisions_do_not_change_agents_pinned_to_the_previous_source() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let one = create(&w, "Visible");
        let two = create(&w, "Frozen");
        let e = w.project_engine(Some(&two)).unwrap();
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
        let frozen = json!(e.store.snapshot().unwrap().frozen_test_plan(fid).unwrap());
        w.dispatch("role.revise",json!({"command_id":id(),"target":default_role_id("analyst"),"expected_revision":1,"payload":{"name":"Narrow analyst","data":{"instructions":"# Narrow responsibility","visibility":{"kind":"projects","project_ids":[one]}}}})).unwrap();
        w.sync_configuration(&e).unwrap();
        assert_eq!(e.project_configuration_status().unwrap()["ready"], true);
        let mut command = Command {
            command_id: id(),
            target: Some(two.clone()),
            expected_revision: Some(2),
            payload: json!({"agent":default_configured_agent_id("analyst")}),
        };
        assert!(
            e.resolve_session_composition(
                &crate::test_runtime::ResourceFixture {
                    store: w.global.as_ref(),
                },
                &mut command,
            )
            .inspect_err(|error| eprintln!("compose failed: {error:?}"))
            .is_ok()
        );
        let payload = json!({"name":"Continuation instance","scope":format!("project:{two}"),"agent_revision_id":default_configured_agent_id("analyst")});
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
            .is_ok()
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
            json!(e.store.snapshot().unwrap().frozen_test_plan(fid).unwrap()),
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
        assert!(w.project_engine(None).is_err());
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
        assert_eq!(
            w.project_engine(Some(&project)).unwrap().store.project_id(),
            Some(project.clone())
        );
        assert!(
            w.dispatch(
                "query.sessions",
                json!({"scope":"global","project_id":project})
            )
            .is_err()
        );
    }
}

#[cfg(test)]
pub(super) trait FrozenConfiguredPlanForTests {
    fn frozen_test_plan(&self, feature: &str) -> Option<devdesk_core::ConfiguredWorkflowPlan>;
}
#[cfg(test)]
impl FrozenConfiguredPlanForTests for EngineeringState {
    fn frozen_test_plan(&self, feature: &str) -> Option<devdesk_core::ConfiguredWorkflowPlan> {
        self.governance
            .iter()
            .find(|r| {
                r.r#type == "workflow_instance"
                    && r.data["owner_id"] == feature
                    && r.data["parent_execution_id"].is_null()
            })
            .and_then(|r| serde_json::from_value(r.data["plan"].clone()).ok())
    }
}

impl Workspace {
    pub(super) fn session_resources(&self) -> session_resources::SessionResourceAdapter<'_> {
        session_resources::SessionResourceAdapter {
            global: &self.global,
            codex_auth: &self.codex_auth,
            preparation: &self.runtime_setup,
        }
    }
}

#[cfg(test)]
impl Workspace {
    pub(super) fn loading_profile(root: &Path) -> devdesk_domain::Result<Arc<Self>> {
        let global = Arc::new(GlobalStore::open(root)?);
        let adapters = crate::test_runtime::adapters();
        let configuration = engine(global.clone(), &adapters);
        let mut opened = std::collections::BTreeMap::new();
        let (projects, total) = global.projects(0, 100)?;
        devdesk_domain::require(total <= 100, "性能入口最多打开 100 个项目")?;
        for project in projects {
            let store = Arc::new(SqliteStore::open_project(root, &project.id)?);
            opened.insert(project.id, engine(store, &adapters));
        }
        Ok(Arc::new(Self {
            global,
            configuration,
            codex_auth: devdesk_adapters::codex_auth::CodexAuthentication::new(root)?,
            adapters,
            opened: Mutex::new(opened),
            switching: Mutex::new(()),
            template_gate: Arc::new(Mutex::new(())),
            runtime_setup: Default::default(),
            configuration_views: Default::default(),
            catalog_instance: id(),
            event_hubs: Default::default(),
            scheduler_next: Default::default(),
            directory_next: Default::default(),
        }))
    }
}
