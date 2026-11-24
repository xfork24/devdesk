//! Application use cases and ports. Adapters cannot advance domain state.
#![forbid(unsafe_code)]
pub use agent_context::context_tool_definition;
mod invocation;
pub use invocation::InvocationContext;
mod catalog_sync;
pub mod concurrency;
mod configuration_catalog;
mod configuration_publication;
pub use configuration_publication::{
    ConfigurationPublication, configuration_transfer_witness, prepare_configuration_publication,
};
pub mod credential_updates;
pub mod experience_directory;
pub mod integration_operations;
pub mod project_creation;
pub mod project_reservation;
pub mod project_updates;
mod provider_tools;
pub use provider_tools::{ConversationRuntimeFacts, ProviderTurn};
mod conversation_captures;
mod conversation_controls;
mod conversation_observations;
mod conversation_start;
mod conversation_submission;
mod conversation_workers;
mod in_flight;
mod interaction_delivery;
mod manager_publication;
mod material_commands;
mod native_context_calls;
mod proposal_commands;
mod repository_runtime;
mod runtime_callbacks;
mod sealed_commands;
mod session_commands;
mod work_owners;
mod workspace_commands;
mod workspace_continuation;
pub use conversation_captures::ConversationCaptureFacts;
pub use conversation_start::{ConversationAuthority, ConversationStartSource};
pub use native_context_calls::ContextCallFacts;
pub mod read_models;
pub mod repository_dispatch;
pub mod repository_projection;
pub mod repository_updates;
pub mod rpc_contracts;
pub mod runtime_registry;
pub mod settings;
pub mod tool_calls;
mod tool_creation;
pub use configuration_catalog::*;
pub use tool_creation::PreparedConfiguredTool;
mod configuration_queries;
pub use configuration_queries::{
    configuration_index, configuration_records, configuration_reference_usages,
    configuration_selection_record, configuration_summary, project_configuration_context,
    summarize_configuration_result,
};
mod default_permissions;
mod resource_receipts;
mod session_assembly;
pub use default_permissions::template_permission_defaults;
mod builtin_defaults;
mod configuration;
mod configuration_changes;
mod configuration_transfers;
pub use configuration_changes::{
    ConfigurationChangeSet, ConfigurationDiagnostic, ConfigurationOperation,
    check_configuration_retirements, configuration_fingerprint, configuration_identity_usage,
    configuration_usage,
};
mod configured_runtime;
mod configured_workflows;
#[cfg(test)]
mod controller_event_tests;
mod controller_events;
mod controller_interaction_reads;
mod controller_interactions;
mod controllers;
pub use controller_events::append_workflow_events;
mod manager_workspace;
pub use manager_workspace::{render_project_artifact, render_project_record};
mod spec_lint;
pub use manager_workspace::{MANAGER_TOOLS, ManagerServices, read_project_knowledge};
mod controller_calls;
mod controller_lifecycle;
mod controller_runtime;
mod delivery_recovery;
#[cfg(test)]
mod human_governance;
mod orchestration;
mod platform_cleanup;
mod platform_delivery;
mod platform_facts;
mod platform_observations;
mod platform_preparation;
mod platform_publish;
mod platform_reads;
mod platform_reconciliation;
mod platform_replies;
mod platform_runtime;
mod platform_tools;
mod project_standards;
mod publication_retry;
mod standards;
mod workflow_driver;
mod workflow_driving;
mod workflow_eligibility;
mod workflow_failure;
mod workflow_segment_actions;
mod workflow_segments;
pub use controllers::CONTROL_TOOLS;
pub use standards::{compare_work_standards, work_standard_choices, work_standard_selection};
pub use workflow_eligibility::{WorkflowActionEligibility, WorkflowStepActions};
mod workflow_change_commit;
mod workflow_change_preview;
mod workflow_review;
mod workflow_rework;
pub use configured_runtime::task_input as render_source_task;
pub use configured_runtime::{
    ConfiguredAgentRequest, ConfiguredToolRequest, ConfiguredWorkflowInstance,
};
pub use configured_workflows::{ConfiguredWorkflowPlan, compile_workflow_sources};
pub use platform_preparation::PreparedConfiguredPlatform;
pub use platform_tools::PlatformToolRequest;
mod command_catalog;
pub mod frozen_views;
pub mod template_reset_jobs;
mod templates;
pub use command_catalog::*;
pub use templates::{ConfigurationBundle, install_configuration_bundle};
mod driver_capabilities;
pub use driver_capabilities::{
    DriverCapabilities, driver_capabilities, driver_supports, install_driver_capabilities,
};
mod environment;
pub use configuration::{
    project_configuration_references, retired as configuration_retired,
    visible as configuration_visible,
};
use environment::now;
pub use environment::{id, with_application_environment};
pub use templates::{
    copy_template_catalog, current_template_defaults, initialize_template_catalog,
    register_template_members, reset_template_catalog, restore_template_catalog,
    template_bundle_info, template_catalog_counts, template_catalog_has_custom_history,
    template_catalog_pristine, template_factory_counts, template_hard_reset_generation,
    update_factory_catalog,
};
mod conversation_runtime;
mod conversations;
mod integrations;
pub use integrations::{
    freeze_discussion_integrations, freeze_integrations, integration_deletion_targets,
};
mod agent_context;
pub use orchestration::read_workflow_outputs;
mod governance;
mod live_runs;
mod permission_resources;
pub use permission_resources::{
    apply_permission_bindings, insert_permission_profiles, permission_profile_export,
};
mod permission_switch;
pub use permission_switch::permission_switch_id;
mod bootstrap_recovery;
mod controller_definitions;
mod conversation_admission;
mod formal_inputs;
mod formal_sessions;
pub mod ports;
mod worker_reconciliation;
mod workflow_authorization;
mod workflow_budget;
mod workflow_children;
mod workflow_commits;
mod workflow_control_reads;
mod workflow_controls;
mod workflow_detail;
mod workflow_dispatch;
mod workflow_evaluation;
mod workflow_execution_controls;
mod workflow_facts;
mod workflow_material_inputs;
mod workflow_materials;
mod workflow_observation_facts;
mod workflow_outputs;
mod workflow_readiness;
mod workflow_reading;
mod workflow_reconciliation;
mod workflow_recovery;
mod workflow_repair_attempts;
mod workflow_resources;
mod workflow_retries;
mod workflow_rule_hints;
mod workflow_rules;
mod workflow_runtime;
mod workflow_start;
pub use workflow_runtime::{PreparedWorkflowIntent, WorkflowIntentFacts, WorkflowIntentPage};
mod workflow_runtime_facts;
// Shared domain types and execution ports.
use devdesk_domain::*;
pub use ports::{Git as GitPort, Harness as HarnessPort, TestRequest, Tests as TestRunnerPort};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub use ports::{ArtifactStore, EngineeringStore};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub command_id: String,
    pub target: Option<String>,
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub payload: Value,
}
pub fn command_fingerprint(method: &str, c: &Command) -> String {
    serde_json::to_string(&(method, &c.target, &c.expected_revision, &c.payload)).unwrap()
}
/// Independent coordinator stages; background occupancy is not a business lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulingStage {
    Controllers,
    Tools,
    Platform,
    Repositories,
    Conversations,
    Workflows,
    PlanChanges,
    Segments,
    Drivers,
}
impl SchedulingStage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Controllers => "controllers",
            Self::Tools => "tools",
            Self::Platform => "platform",
            Self::Repositories => "repositories",
            Self::Conversations => "conversations",
            Self::Workflows => "workflows",
            Self::PlanChanges => "plan_changes",
            Self::Segments => "segments",
            Self::Drivers => "drivers",
        }
    }
    pub const ALL: &'static [Self] = &[
        Self::Conversations,
        Self::Workflows,
        Self::Controllers,
        Self::Tools,
        Self::Platform,
        Self::Repositories,
        Self::PlanChanges,
        Self::Segments,
        Self::Drivers,
    ];
}
pub struct Engine {
    configuration_reset: std::sync::Mutex<()>,
    pub(crate) effect_in_flight: Arc<in_flight::InFlight>,
    pub(crate) runtime_callback_cursors: std::sync::Mutex<std::collections::BTreeMap<String, Id>>,
    pub(crate) experience_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) workflow_intent_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) provider_tool_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) provider_poll_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) conversation_capture_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) conversation_active_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) conversation_queued_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) context_call_cursor: std::sync::Mutex<Option<Id>>,
    pub(crate) tool_cursor: std::sync::Mutex<Option<Id>>,
    pub context_reader: Option<Arc<dyn ports::AgentContextReader>>,
    pub manager_services: Option<ManagerServices>,
    pub configuration_backend: Option<Arc<dyn ports::ConfigurationBackend>>,
    pub tool_executor: Option<Arc<dyn ports::ToolExecutor>>,
    pub domain_reader: Option<Arc<dyn ports::DomainReader>>,
    pub store: Arc<dyn EngineeringStore>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub harness: Arc<dyn ports::Harness>,
    pub git: Arc<dyn ports::Git>,
    pub tests: Arc<dyn ports::Tests>,
    pub pull_requests: Option<Arc<dyn ports::PullRequests>>,
}
fn text(v: &Value, k: &str) -> Result<String> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_params", format!("缺少 {k}")))
}
fn opt(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn event(s: &mut EngineeringState, target: &str, kind: &str, detail: &str) {
    s.events.push(Event {
        id: s.events.last().map_or(1, |e| e.id + 1),
        target: target.into(),
        kind: kind.into(),
        detail: detail.into(),
        time: now(),
    });
}
fn revision(s: &EngineeringState, id: &str) -> Option<u64> {
    s.projects
        .iter()
        .find(|x| x.id == id)
        .map(|x| x.revision)
        .or_else(|| s.features.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.tasks.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.contexts.iter().find(|x| x.id == id).map(|x| x.revision))
}
/// Seed shared configuration without creating project or execution facts.
pub fn seed_configuration(s: &mut EngineeringState) {
    initialize_template_catalog(s).expect("validated embedded template catalog");
}

impl Engine {
    /// Assemble an engine through the same contracts for all provider implementations.
    pub fn new(
        store: Arc<dyn EngineeringStore>,
        artifacts: Arc<dyn ArtifactStore>,
        adapters: ports::ExecutionAdapters,
    ) -> Self {
        Self {
            configuration_reset: std::sync::Mutex::new(()),
            runtime_callback_cursors: std::sync::Mutex::new(Default::default()),
            effect_in_flight: Default::default(),
            experience_cursor: std::sync::Mutex::new(None),
            workflow_intent_cursor: std::sync::Mutex::new(None),
            provider_tool_cursor: std::sync::Mutex::new(None),
            provider_poll_cursor: std::sync::Mutex::new(None),
            conversation_capture_cursor: std::sync::Mutex::new(None),
            conversation_active_cursor: std::sync::Mutex::new(None),
            conversation_queued_cursor: std::sync::Mutex::new(None),
            context_call_cursor: std::sync::Mutex::new(None),
            tool_cursor: std::sync::Mutex::new(None),
            context_reader: None,
            manager_services: None,
            configuration_backend: None,
            tool_executor: None,
            domain_reader: None,
            store,
            artifacts,
            harness: adapters.harness,
            git: adapters.git,
            tests: adapters.tests,
            pull_requests: adapters.pull_requests,
        }
    }
    pub fn with_manager_services(mut self, services: ManagerServices) -> Self {
        self.manager_services = Some(services);
        self
    }
    pub fn with_context_reader(mut self, reader: Arc<dyn ports::AgentContextReader>) -> Self {
        self.context_reader = Some(reader);
        self
    }
    pub fn with_configuration_backend(
        mut self,
        backend: Arc<dyn ports::ConfigurationBackend>,
    ) -> Self {
        self.configuration_backend = Some(backend);
        self
    }
    pub fn execute(&self, method: &str, c: Command) -> Result<Value> {
        self.execute_in_context(
            method,
            c,
            &InvocationContext::human(self.store.project_id()),
        )
    }
    /// Trusted coordinator resolves configuration after checking the original request's replay key.
    /// RPC callers cannot provide this override.
    pub fn execute_resolved(
        &self,
        method: &str,
        c: Command,
        original_fingerprint: &str,
    ) -> Result<Value> {
        require(
            [
                "session.open",
                "permission.bindings.apply",
                "integration.tools.publish",
                "skill.import",
            ]
            .contains(&method),
            "该命令不支持绑定解析",
        )?;
        self.execute_internal(
            method,
            c,
            Some(original_fingerprint),
            &InvocationContext::human(self.store.project_id()),
        )
    }
    pub fn execute_in_context(
        &self,
        method: &str,
        c: Command,
        context: &InvocationContext,
    ) -> Result<Value> {
        self.execute_internal(method, c, None, context)
    }
    fn execute_internal(
        &self,
        method: &str,
        c: Command,
        original_fingerprint: Option<&str>,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            !matches!(context.actor(), WorkflowActor::Coordinator { .. }),
            "系统事实投影不能调用公开业务写入",
        )?;
        if let Some(value) =
            crate::conversation_controls::control(self.store.as_ref(), method, &c, context)?
        {
            return Ok(value);
        }
        if let Some(value) = crate::tool_calls::control(self.store.as_ref(), method, &c, context)? {
            return Ok(value);
        }
        if method == "project.create" && self.store.project_id().is_some() {
            return crate::project_creation::create(self.store.as_ref(), &c, context)
                .map(|receipt| receipt.result);
        }
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let fingerprint = original_fingerprint
            .map(str::to_owned)
            .unwrap_or_else(|| command_fingerprint(method, &c));
        if let Some(result) = self.native_workspace_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) = self.native_repository_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) = self.native_controller_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) = self.native_workflow_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) = self.native_material_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) = self.native_proposal_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) =
            self.native_conversation_submission(method, &c, &fingerprint, context)?
        {
            return Ok(result);
        }
        if let Some(result) = self.native_session_command(method, &c, &fingerprint, context)? {
            return Ok(result);
        }
        if let Some(result) =
            self.native_configuration_command(method, &c, &fingerprint, context)?
        {
            return Ok(result);
        }
        Err(err(
            "method_not_found",
            format!("未知或未声明原生资源提交的命令 {method}"),
        ))
    }
    fn apply_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        if let Some(result) = self.configuration_change_command(method, c, s, actor)? {
            return Ok(result);
        }
        if let Some(result) = self.standard_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.configured_workflow_command(method, c, s)? {
            return Ok(result);
        }

        if let Some(target) = &c.target
            && let Some(actual) = revision(s, target)
            && c.expected_revision != Some(actual)
            && !(method == "conversation.send" && crate::controllers::authorized_send(s, c))
        {
            return Err(err(
                "revision_conflict",
                format!("当前 revision={actual}，请刷新后重试"),
            ));
        }
        let target = c.target.as_deref().unwrap_or("");
        require(
            s.projects.iter().all(|p| p.execution_mode == "live"),
            "历史项目只读，请创建新的 GitHub 项目",
        )?;
        require(c.payload.get("scenario").is_none(), "不再支持模拟场景参数")?;
        if let Some(result) = self.integration_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.permission_resource_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.governance_command(method, c, s)? {
            event(s, target, method, &format!("command_id={}", c.command_id));
            return Ok(result);
        }
        Err(err("method_not_found", format!("未知配置命令 {method}")))
    }
    pub fn run_scheduling_stage(
        &self,
        stage: SchedulingStage,
        allowance: usize,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        if should_stop() {
            return Ok(());
        }
        let result = match stage {
            SchedulingStage::Controllers => self.drive_controllers(),
            SchedulingStage::Tools => self.drive_tool_calls(),
            SchedulingStage::Platform => self.drive_platform_tools(),
            SchedulingStage::Repositories => self.tick_repositories(),
            SchedulingStage::Conversations => {
                self.tick_conversations_with_stop(allowance, should_stop)
            }
            SchedulingStage::Workflows => self.evaluate_native_workflows(),
            SchedulingStage::PlanChanges => self.drive_workflow_changes(),
            SchedulingStage::Segments => self.drive_workflow_segments(),
            SchedulingStage::Drivers => self.drive_rust_workflows(),
        };
        result.map_err(|error| {
            error.with_context(
                "Engine::run_scheduling_stage",
                json!({"project_id":self.store.project_id(),"stage":stage.name()}),
            )
        })
    }
    pub fn tick(&self, global_limit: usize) -> Result<()> {
        self.tick_until(global_limit, &|| false)
    }
    /// `project_allowance` includes this project's existing writers. Admission
    /// subtracts those writers exactly once, inside the conversation stage.
    /// Complete each owned stage before honoring shutdown so its receipts and
    /// writer revocation remain recoverable.
    pub fn tick_until(
        &self,
        project_allowance: usize,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        let stages: &[&dyn Fn() -> Result<()>] = &[
            &|| self.drive_controllers(),
            &|| self.drive_tool_calls(),
            &|| self.drive_platform_tools(),
            &|| self.tick_repositories(),
            &|| self.tick_conversations(project_allowance),
            &|| self.evaluate_configured_workflows(),
            &|| self.drive_workflow_changes(),
            &|| self.drive_workflow_segments(),
            &|| self.drive_rust_workflows(),
            &|| self.drive_controllers(),
        ];
        for stage in stages {
            if should_stop() {
                break;
            }
            stage()?;
        }
        Ok(())
    }
    pub fn query(&self, method: &str, p: &Value, global_limit: usize) -> Result<Value> {
        if method == "query.feature_branches" {
            read_models::fields(p, &["project_id", "feature_id", "task_offset"])?;
            let project = read_models::identifier(p, "project_id")?;
            let feature = read_models::identifier(p, "feature_id")?;
            require(
                self.store.project_id().as_deref() == Some(&project),
                "分支查询项目不匹配",
            )?;
            let offset = match p.get("task_offset") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .filter(|n| *n <= 1_000_000)
                    .ok_or_else(|| err("invalid_params", "无效的 task_offset"))?,
            };
            return self.store.feature_branches(&feature, offset);
        }
        if method == "query.project_test_configuration" {
            return Ok(json!(self.discover_project_tests()?));
        }
        if [
            "query.repository",
            "query.runtimes",
            "query.history",
            "query.tool_calls",
        ]
        .contains(&method)
        {
            return self.store.operational_read(method, p);
        }
        if method == "query.artifact" {
            let id = text(p, "id")?;
            if self.store.namespace() == concurrency::StoreNamespace::Configuration {
                return Err(err("not_found", "Artifact 不存在"));
            }
            return self
                .store
                .read_resources(&[concurrency::ResourceKey::new("artifact", id)])?
                .into_iter()
                .next()
                .and_then(|row| row.value)
                .ok_or_else(|| err("not_found", "Artifact 不存在"));
        }
        if method == "query.workspace" {
            if self.store.project_id().is_some() {
                return self.store.workspace_reading(p);
            }
            return Ok(
                json!({"projects":[],"features":[],"total_features":0,"total_projects":0,"configurations":[],"definitions":[],"delivery_types":[],"harnesses":[],"sandboxes":[],"detail":null,"repository_ready":false,"data_root":self.store.root(),"execution_mode":"live","global_concurrency":global_limit}),
            );
        }

        if method == "query.events" {
            return Ok(json!(self.store.events_after(
                p["cursor"].as_u64().unwrap_or(0),
                p["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize
            )?));
        }
        if method == "query.workflow_executions" {
            return self.workflow_executions(p);
        }
        if method == "query.workflow_record" {
            return self.workflow_record(p);
        }
        if method == "query.workflow_contexts" {
            return self.workflow_contexts(p);
        }
        if method == "query.workflow_materials" {
            return self.workflow_materials(p);
        }
        if let Some(result) = self.configuration_change_query(method, p) {
            return result;
        }
        if method == "query.spec_lint" {
            return self.specification_lint(p);
        }
        if method == "query.project_knowledge" {
            let project = self
                .store
                .project_id()
                .ok_or_else(|| err("project_required", "需要项目"))?;
            return self.manager_knowledge("project.overview", &project, p);
        }
        if method == "query.workflow_driver" {
            if p["view"] == "summary" {
                return Ok(json!(self.store.workflow_driver_summary(&text(p, "id")?)?));
            }
            return self.workflow_driver_query(p);
        }
        if method == "query.orchestration" {
            return self.controller_workspace_view(p);
        }
        if let Some(result) = self.workflow_control_query(method, p) {
            return result;
        }
        if let Some(result) = self.configured_runtime_query(method, p) {
            return result;
        }
        if let Some(result) = self.standard_query(method, p) {
            return result;
        }
        if let Some(result) = self.configured_workflow_query(method, p) {
            return result;
        }
        if let Some(result) = self.integration_query(method, p) {
            return result;
        }
        if let Some(result) = self.configuration_query(method, p) {
            return result;
        }
        if let Some(result) = self.governance_query(method, p) {
            return result;
        }
        if let Some(result) = self.conversation_query(method, p) {
            return result;
        }
        Err(err("method_not_found", "未知查询"))
    }
}
