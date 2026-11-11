//! Application use cases and ports. Adapters cannot advance domain state.
mod agent_tasks;
mod builtin_defaults;
mod configuration;
mod configured_runtime;
mod configured_workflows;
mod controllers;
mod orchestration;
mod platform_tools;
mod standards;
pub use controllers::CONTROL_TOOLS;
mod workflow_changes;
pub use configured_runtime::task_input as render_source_task;
pub use configured_runtime::{
    ConfiguredAgentRequest, ConfiguredToolRequest, ConfiguredWorkflowInstance,
};
pub use configured_workflows::{ConfiguredWorkflowPlan, compile_workflow_sources};
pub use platform_tools::PlatformToolRequest;
mod templates;
pub use configuration::{
    project_configuration_references, retired as configuration_retired,
    visible as configuration_visible,
};
pub use templates::{
    copy_template_catalog, current_template_defaults, initialize_template_catalog,
    register_template_members, restore_template_catalog, template_bundle_info,
    template_catalog_counts, template_catalog_pristine, template_factory_counts,
};
mod conversation_runtime;
mod conversations;
mod history;
mod integrations;
pub use integrations::{freeze_integrations, integration_deletion_targets};
mod governance;
mod live_runs;
mod permission_resources;
pub use permission_resources::{
    apply_permission_bindings, insert_permission_profiles, permission_profile_export,
};
mod permission_switch;
pub use permission_switch::permission_switch_id;
pub mod ports;
mod proposals;
mod repositories;
mod workflow_runtime;
// Shared domain types and execution ports.
use devdesk_domain::*;
pub use ports::{Git as GitPort, Harness as HarnessPort, TestRequest, Tests as TestRunnerPort};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub trait EngineeringStore: Send + Sync {
    /// Lookup through the same idempotency contract without creating a missing operation.
    fn replay(&self, id: &str, fingerprint: &str) -> Result<Option<Value>> {
        let mut missing = false;
        let result = self.transact(Some((id, fingerprint)), &mut |_| {
            missing = true;
            Err(err("operation_absent", "操作尚未登记"))
        });
        if missing { Ok(None) } else { result.map(Some) }
    }
    /// Conservative scheduling hint; providers without an indexed query remain always eligible.
    fn has_pending_work(&self) -> Result<bool> {
        Ok(true)
    }
    fn snapshot(&self) -> Result<EngineeringState>;
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value>;
    fn root(&self) -> String;
    fn project_id(&self) -> Option<String> {
        None
    }
}
pub trait ArtifactStore: Send + Sync {
    fn seal_scoped(&self, owner: &MaterialOwner, category: &str, body: &str) -> Result<Artifact> {
        match owner {
            MaterialOwner::Feature { id } => self.seal(id, None, category, body),
            MaterialOwner::Task { feature, id } => self.seal(feature, Some(id), category, body),
            MaterialOwner::Global | MaterialOwner::Project { .. } => {
                Err(err("unsupported", "ArtifactStore 未提供 Project 产物封存"))
            }
        }
    }
    fn seal(
        &self,
        feature: &str,
        task: Option<&str>,
        category: &str,
        body: &str,
    ) -> Result<Artifact>;
    fn read(&self, artifact: &Artifact) -> Result<Vec<u8>>;
}
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
pub struct Engine {
    pub tool_executor: Option<Arc<dyn ports::ToolExecutor>>,
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
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
fn mutable_feature(s: &EngineeringState, fid: &str) -> Result<()> {
    require(
        s.feature(fid)?.status != Status::Completed,
        "已完成 Feature 已冻结，请创建关联的新 Feature",
    )
}
fn document_ids(s: &EngineeringState, fid: &str, tid: Option<&str>) -> Vec<Id> {
    s.documents
        .iter()
        .filter(|d| {
            s.document_in_feature(d, &fid)
                && (d.task_id.is_none() || d.task_id.as_deref() == tid)
                && !s
                    .documents
                    .iter()
                    .any(|n| n.supersedes.as_deref() == Some(&d.id) && n.kind == d.kind)
        })
        .map(|d| d.id.clone())
        .collect()
}
/// Seed shared configuration without creating project or execution facts.
pub fn seed_configuration(s: &mut EngineeringState) {
    initialize_template_catalog(s).expect("validated embedded template catalog");
}

impl Engine {
    fn revise_material(&self, s: &mut EngineeringState, owner: &str, p: &Value) -> Result<Value> {
        let (project, feature, task) = if s.projects.iter().any(|r| r.id == owner) {
            (Some(owner.to_owned()), None, None)
        } else if let Some(t) = s.tasks.iter().find(|t| t.id == owner) {
            (None, Some(t.feature_id.clone()), Some(t.id.clone()))
        } else {
            s.feature(owner)?;
            (None, Some(owner.to_owned()), None)
        };
        let material = material_owner(project.as_deref(), feature.as_deref(), task.as_deref())?;
        let kind = text(p, "kind")?;
        let body = text(p, "body")?;
        let previous = opt(p, "supersedes");
        let version = if let Some(id) = &previous {
            let old = s
                .documents
                .iter()
                .find(|d| {
                    d.id == *id
                        && d.project_id == project
                        && d.feature_id == feature
                        && d.task_id == task
                        && d.kind == kind
                })
                .ok_or_else(|| err("scope_violation", "前版材料归属或类型不匹配"))?;
            require(
                !s.documents
                    .iter()
                    .any(|d| d.supersedes.as_ref() == Some(id)),
                "材料已有后继版本",
            )?;
            old.version + 1
        } else {
            1
        };
        let artifact = self.artifacts.seal_scoped(&material, "materials", &body)?;
        let artifact_id = artifact.id.clone();
        s.artifacts.push(artifact);
        let did = id();
        s.documents.push(Document {
            id: did.clone(),
            project_id: project,
            feature_id: feature,
            task_id: task,
            kind,
            title: text(p, "title")?,
            body,
            version,
            supersedes: previous,
            input_refs: serde_json::from_value(p.get("input_refs").cloned().unwrap_or(json!([])))
                .map_err(|e| err("invalid_params", e.to_string()))?,
            origin: Origin::Human,
            artifact_id: Some(artifact_id),
        });
        if let Some(project) = s.projects.iter_mut().find(|r| r.id == owner) {
            project.revision += 1;
        }
        if let Some(feature) = s.features.iter_mut().find(|r| r.id == owner) {
            feature.revision += 1;
        }
        if let Some(task) = s.tasks.iter_mut().find(|r| r.id == owner) {
            task.revision += 1;
        }
        Ok(json!({"id":did}))
    }
    fn save_material_snapshot(
        &self,
        s: &mut EngineeringState,
        target: &str,
        p: &Value,
    ) -> Result<Value> {
        let (project, feature, task) =
            if let Some(project) = s.projects.iter().find(|v| v.id == target) {
                (Some(project.id.clone()), None, None)
            } else if let Some(task) = s.tasks.iter().find(|v| v.id == target) {
                (None, Some(task.feature_id.clone()), Some(task.id.clone()))
            } else {
                (None, Some(s.feature(target)?.id.clone()), None)
            };
        let owner = material_owner(project.as_deref(), feature.as_deref(), task.as_deref())?;
        let documents: Vec<Id> =
            serde_json::from_value(p.get("documents").cloned().unwrap_or_else(|| {
                json!(
                    s.documents
                        .iter()
                        .filter(|d| d.project_id == project
                            && d.feature_id == feature
                            && (d.task_id == task || d.task_id.is_none()))
                        .map(|d| &d.id)
                        .collect::<Vec<_>>()
                )
            }))
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let snapshot = Snapshot {
            id: id(),
            project_id: project,
            feature_id: feature,
            task_id: task,
            kind: "manual".into(),
            content: s
                .contexts
                .iter()
                .find(|c| c.owner_id == target)
                .map(|c| c.content.clone())
                .unwrap_or_default(),
            documents,
            inherited_from: None,
            capture: SnapshotCapture {
                step_execution_ids: vec![],
                sessions: vec![],
                captured_at: Some(now()),
                reason: opt(p, "reason").unwrap_or_else(|| "User selected materials".into()),
            },
            runtime_bindings: vec![],
            execution_instances: vec![],
        };
        let artifact = self.artifacts.seal_scoped(
            &owner,
            "snapshots",
            &serde_json::to_string_pretty(&snapshot)
                .map_err(|e| err("serialization", e.to_string()))?,
        )?;
        let result = json!({"id":snapshot.id});
        s.artifacts.push(artifact);
        s.snapshots.push(snapshot);
        Ok(result)
    }
    /// Assemble an engine through the same contracts for all provider implementations.
    pub fn new(
        store: Arc<dyn EngineeringStore>,
        artifacts: Arc<dyn ArtifactStore>,
        adapters: ports::ExecutionAdapters,
    ) -> Self {
        Self {
            tool_executor: None,
            store,
            artifacts,
            harness: adapters.harness,
            git: adapters.git,
            tests: adapters.tests,
            pull_requests: adapters.pull_requests,
        }
    }
    pub fn bootstrap(&self) -> Result<()> {
        if self
            .store
            .snapshot()?
            .projects
            .iter()
            .any(|p| p.execution_mode != "live")
        {
            return Ok(());
        }
        self.store.transact(None, &mut |s| {
            seed_configuration(s);
            for r in s
                .governance
                .iter_mut()
                .filter(|r| r.kind == "tool_call" && r.data["status"] == "dispatched")
            {
                let mut d = r.data.clone();
                d["status"] = json!("needs_reconciliation");
                d["remote_effect"] = json!("unknown");
                governance::update_record(r, d);
            }
            // Reconcile persisted workflow references once after opening, including
            // idle instances left waiting by an older evaluator.
            for record in s.governance.iter_mut().filter(|r| {
                r.kind == "workflow_instance"
                    && r.data["status"] != "completed"
                    && r.data["status"] != "cancelled"
            }) {
                let mut data = record.data.clone();
                data["dirty"] = json!(true);
                governance::update_record(record, data);
            }
            for op in s
                .repository_operations
                .iter_mut()
                .filter(|op| op.status == RepositoryOperationStatus::Dispatched)
            {
                op.status = RepositoryOperationStatus::NeedsReconciliation;
                op.reason = "restart_after_dispatch".into();
                op.revision += 1;
            }
            for op in s
                .repository_operations
                .iter_mut()
                .filter(|op| op.status == RepositoryOperationStatus::Prepared)
            {
                op.status = RepositoryOperationStatus::NeedsReconciliation;
                op.reason = "startup_pending_intent_requires_reconcile".into();
                op.revision += 1;
            }
            for session in s.sessions.iter_mut().filter(|v| {
                v.scope.is_some()
                    && v.external_session.is_some()
                    && !matches!(
                        v.lifecycle,
                        Some(SessionLifecycle::Closed | SessionLifecycle::RequiresSuccessor)
                    )
            }) {
                let uncertain = s.turns.iter().any(|t| {
                    t.session_id == session.id
                        && t.status.active()
                        && t.status != TurnStatus::Queued
                }) || s
                    .executions
                    .iter()
                    .any(|e| e.session_id == session.id && !e.writer_revoked);
                if uncertain {
                    session.lifecycle = Some(SessionLifecycle::NeedsReconciliation);
                    session.revision += 1;
                } else if session.lifecycle == Some(SessionLifecycle::NeedsReconciliation) {
                    session.lifecycle = Some(SessionLifecycle::Idle);
                    session.revision += 1;
                }
            }
            for turn in s.turns.iter_mut().filter(|t| {
                [
                    TurnStatus::Dispatching,
                    TurnStatus::Running,
                    TurnStatus::WaitingInteraction,
                ]
                .contains(&t.status)
            }) {
                turn.status = TurnStatus::NeedsReconciliation;
                turn.revision += 1;
                if let Some(session) = s.sessions.iter_mut().find(|v| v.id == turn.session_id) {
                    session.lifecycle = Some(SessionLifecycle::NeedsReconciliation);
                    session.revision += 1;
                }
            }
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    pub fn execute(&self, method: &str, c: Command) -> Result<Value> {
        self.execute_internal(method, c, None)
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
        self.execute_internal(method, c, Some(original_fingerprint))
    }
    fn execute_internal(
        &self,
        method: &str,
        c: Command,
        original_fingerprint: Option<&str>,
    ) -> Result<Value> {
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let fingerprint = original_fingerprint
            .map(str::to_owned)
            .unwrap_or_else(|| command_fingerprint(method, &c));
        let mut action = |s: &mut EngineeringState| {
            if template_definition_write(method) {
                if let Some(target) = c.target.as_deref().and_then(|id| s.configuration(id).ok()) {
                    require(
                        template_record_current(s, target),
                        "模板目录已重置，请重新载入后再编辑",
                    )?;
                }
            }

            let result = self.apply_command(method, &c, s)?;
            self.governance_after_command(method, &c, s, &result)?;
            register_template_members(s, template_catalog_generation(s));
            Ok(result)
        };
        let result = self
            .store
            .transact(Some((&c.command_id, &fingerprint)), &mut action)?;
        // Long operations return their durable ID; the scheduler owns supplier dispatch.
        Ok(result)
    }
    fn apply_command(&self, method: &str, c: &Command, s: &mut EngineeringState) -> Result<Value> {
        if let Some(result) = self.orchestration_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.configured_runtime_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.standard_command(method, c, s)? {
            return Ok(result);
        }
        if let Some(result) = self.configured_workflow_command(method, c, s)? {
            return Ok(result);
        }

        if let Some(target) = &c.target {
            if let Some(actual) = revision(s, target) {
                if c.expected_revision != Some(actual)
                    && !(method == "conversation.send" && crate::controllers::authorized_send(s, c))
                {
                    return Err(err(
                        "revision_conflict",
                        format!("当前 revision={actual}，请刷新后重试"),
                    ));
                }
            }
        }
        let target = c.target.as_deref().unwrap_or("");
        require(
            s.projects.iter().all(|p| p.execution_mode == "live"),
            "历史项目只读，请创建新的 GitHub 项目",
        )?;
        require(c.payload.get("scenario").is_none(), "不再支持模拟场景参数")?;
        let p = &c.payload;
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
        if let Some(result) = self.repository_command(method, c, s)? {
            event(s, target, method, &format!("command_id={}", c.command_id));
            return Ok(result);
        }
        if let Some(result) = self.proposal_command(method, c, s)? {
            event(s, target, method, &format!("command_id={}", c.command_id));
            return Ok(result);
        }
        if let Some(result) = self.conversation_command(method, &c, s)? {
            event(s, target, method, &format!("command_id={}", c.command_id));
            return Ok(result);
        }
        let result = match method {
            "agent.create" | "agent.revise" => {
                return Err(err(
                    "invalid_params",
                    "Agent 必须通过 Role、Capability 与 permission_selection 创建",
                ));
            }
            "project.create" => {
                let scoped = self.store.project_id();
                require(
                    scoped.is_none() || s.projects.is_empty(),
                    "项目数据库只能保存一个 Project",
                )?;
                let pid = scoped.unwrap_or_else(id);
                let title = text(p, "title")?;
                s.projects.push(Project {
                    id: pid.clone(),
                    title,
                    workspace: opt(p, "workspace").unwrap_or_default(),
                    workspace_definition: Some(Workspace {
                        repository: opt(p, "repository").unwrap_or_default(),
                        path: opt(p, "workspace").unwrap_or_default(),
                        mainline: opt(p, "mainline").unwrap_or_else(|| "main".into()),
                    }),
                    execution_mode: "live".into(),
                    concurrency: 2,
                    revision: 1,
                });
                s.contexts.push(Context {
                    id: id(),
                    owner_id: pid.clone(),
                    content: String::new(),
                    revision: 1,
                });

                require(
                    p["use_builtin_defaults"] != true,
                    "请显式选择 Workflow 并配置项目角色，不再自动采用套件",
                )?;
                json!({"id":pid})
            }
            "project.update" => {
                require(
                    p.as_object().is_some_and(|fields| {
                        fields.keys().all(|key| {
                            ["title", "workspace", "concurrency", "workspace_definition"]
                                .contains(&key.as_str())
                        })
                    }),
                    "Project 更新包含不支持的字段；执行配置请通过 Workflow 和 Agent 管理",
                )?;
                let project = s
                    .projects
                    .iter_mut()
                    .find(|x| x.id == target)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?;
                if let Some(title) = opt(p, "title") {
                    project.title = title;
                }
                if let Some(workspace) = opt(p, "workspace") {
                    project.workspace = workspace.clone();
                    if let Some(definition) = project.workspace_definition.as_mut() {
                        definition.path = workspace;
                    }
                }
                if let Some(n) = p.get("concurrency").and_then(Value::as_u64) {
                    require((1..=32).contains(&n), "并发额度需为 1—32")?;
                    project.concurrency = n as u32;
                }
                if let Some(value) = p.get("workspace_definition") {
                    let workspace: Workspace = serde_json::from_value(value.clone())
                        .map_err(|_| err("invalid_params", "无效 Workspace 定义"))?;
                    require(!workspace.mainline.trim().is_empty(), "mainline 不可为空")?;
                    project.workspace = workspace.path.clone();
                    project.workspace_definition = Some(workspace);
                }
                project.revision += 1;
                json!({"id":target})
            }
            "feature.create" => {
                let pid = text(p, "project_id")?;
                let project = s
                    .projects
                    .iter()
                    .find(|x| x.id == pid)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?;
                require(
                    !s.repository_operations.iter().any(|op| {
                        op.project_id == pid
                            && matches!(op.action, RepositoryAction::Connect { .. })
                            && op.status != RepositoryOperationStatus::Completed
                    }),
                    "仓库登记尚未完成或需核对，不能创建 Feature",
                )?;
                let fid = id();
                let live = project.execution_mode == "live";
                let base = s
                    .heads
                    .iter()
                    .find(|h| h.id == format!("main:{pid}"))
                    .map(|h| h.revision.clone())
                    .unwrap_or_default();
                let predecessor = opt(p, "predecessor");
                if let Some(prev) = &predecessor {
                    require(
                        s.feature(prev)?.project_id == pid,
                        "前序 Feature 必须属于相同 Project",
                    )?;
                }
                let source_materials: Vec<Id> =
                    serde_json::from_value(p.get("source_materials").cloned().unwrap_or(json!([])))
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                require(
                    source_materials.iter().all(|id| {
                        s.documents
                            .iter()
                            .any(|d| &d.id == id && d.project_id.as_ref() == Some(&pid))
                    }),
                    "Feature 输入材料必须属于当前项目",
                )?;
                s.features.push(Feature {
                    source_materials,
                    id: fid.clone(),
                    project_id: pid.clone(),
                    title: text(p, "title")?,
                    status: Status::Draft,
                    revision: 1,
                    input_version: 1,
                    predecessor,
                });
                s.heads.push(GitHead {
                    id: fid.clone(),
                    project_id: pid.clone(),
                    revision: base.clone(),
                });
                s.contexts.push(Context {
                    id: id(),
                    owner_id: fid.clone(),
                    content: "Feature 初始上下文".into(),
                    revision: 1,
                });
                if live && !base.is_empty() {
                    self.enqueue_worktree(s, &pid, &fid, WorktreeKind::Feature, &base);
                }
                self.freeze_feature_configuration(s, &fid, p)?;
                json!({"id":fid})
            }
            "document.revise" => self.revise_material(s, target, p)?,
            "context.update" => {
                let x = s
                    .contexts
                    .iter()
                    .find(|x| x.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "Context 不存在"))?;
                let fid = if let Some(t) = s.tasks.iter().find(|t| t.id == x.owner_id) {
                    require(t.status != Status::Completed, "已完成 Task 已冻结")?;
                    t.feature_id.clone()
                } else {
                    x.owner_id.clone()
                };
                if !s.projects.iter().any(|p| p.id == fid) {
                    mutable_feature(s, &fid)?;
                }
                let next = text(p, "content")?;
                let previous = x.content.clone();
                let previous_revision = x.revision;
                let x = s.contexts.iter_mut().find(|x| x.id == target).unwrap();
                x.content = next.clone();
                x.revision += 1;
                event(s,target,"context.revised",&json!({"previous_revision":previous_revision,"revision":previous_revision+1,"previous":previous,"content":next}).to_string());
                json!({"id":target})
            }
            "snapshot.save" => self.save_material_snapshot(s, target, p)?,
            _ => return Err(err("method_not_found", format!("未知命令 {method}"))),
        };
        event(s, target, method, &format!("command_id={}", c.command_id));
        Ok(result)
    }
    pub fn tick(&self, global_limit: usize) -> Result<()> {
        self.drive_controllers()?;
        self.drive_tool_calls()?;
        self.drive_platform_tools()?;
        self.tick_repositories()?;
        self.tick_conversations(global_limit)?;
        self.evaluate_configured_workflows()?;
        self.drive_workflow_changes()?;
        self.drive_workflow_segments()?;
        self.drive_controllers()
    }
    pub fn query(&self, method: &str, p: &Value, global_limit: usize) -> Result<Value> {
        if method == "query.orchestration" {
            let s = self.store.snapshot()?;
            let row = s.governance.iter().rev().find(|r| {
                r.kind == "workflow_controller"
                    && (p["id"].as_str() == Some(r.id.as_str())
                        || p["root_instance_id"]
                            .as_str()
                            .is_some_and(|id| r.data["root_instance_id"] == id)
                        || p["owner_id"]
                            .as_str()
                            .is_some_and(|id| r.data["owner_id"] == id))
            });
            if let Some(row) = row {
                if let Some(project) = p["project_id"].as_str() {
                    require(row.scope.project() == Some(project), "编排控制器跨项目")?;
                }
                let sid = row.data["session_id"].as_str();
                return Ok(
                    json!({"record":row,"messages":s.conversation_events.iter().filter(|e|Some(e.session_id.as_str())==sid && e.kind==ConversationEventKind::Assistant).rev().take(20).collect::<Vec<_>>(),"calls":s.governance.iter().filter(|r|r.kind=="controller_tool_call"&&r.definition_id==row.id).rev().take(30).collect::<Vec<_>>(),"decisions":s.governance.iter().filter(|r|r.kind=="workflow_pending_decision"&&s.configuration(&r.definition_id).is_ok_and(|instance| instance.data["root_instance_id"]==row.data["root_instance_id"])).rev().take(20).collect::<Vec<_>>() }),
                );
            }
            return Ok(json!({"record":null,"messages":[],"calls":[],"decisions":[]}));
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
        let s = self.store.snapshot()?;
        let offset = p.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = p
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 100) as usize;
        match method {
            "query.repository" => Ok(
                json!({"project":s.projects.first(),"operations":s.repository_operations.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_operations":s.repository_operations.len(),"worktrees":s.repository_worktrees.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_worktrees":s.repository_worktrees.len()}),
            ),
            "query.runtimes" => Ok(json!({"harnesses":s.harnesses,"sandboxes":s.sandboxes})),
            "query.workspace" => {
                let workflows: Vec<_> = s
                    .governance
                    .iter()
                    .filter(|r| r.kind == "workflow_instance")
                    .collect();
                let steps: Vec<_> = workflows
                    .iter()
                    .flat_map(|r| r.data["steps"].as_array().into_iter().flatten())
                    .collect();
                let filter = p["work_filter"].as_str().unwrap_or("");
                let features: Vec<_> = s
                    .features
                    .iter()
                    .filter(|f| p["project_id"].as_str().is_none_or(|id| f.project_id == id))
                    .filter(|f| match filter {
                        "tasks" => s.tasks.iter().any(|t| t.feature_id == f.id),
                        "running" | "waiting_confirmation" | "blocked" => workflows
                            .iter()
                            .filter(|r| r.data["owner_id"] == f.id)
                            .any(|r| {
                                r.data["steps"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .any(|step| match filter {
                                        "running" => step["status"] == "running",
                                        "waiting_confirmation" => {
                                            step["status"] == "waiting_confirmation"
                                        }
                                        _ => ["failed", "needs_reconciliation", "waiting_input"]
                                            .contains(&step["status"].as_str().unwrap_or("")),
                                    })
                            }),
                        _ => true,
                    })
                    .collect();
                let detail = p["feature_id"]
                    .as_str()
                    .map(|fid| {
                        self.feature_detail(&s, fid, limit, global_limit, &p["detail_pages"])
                    })
                    .transpose()?;
                Ok(
                    json!({"projects":s.projects,"features":features.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_features":features.len(),"total_projects":s.projects.len(),
                    "summary":{"features":s.features.len(),"tasks":s.tasks.len(),"running":steps.iter().filter(|r|r["status"]=="running").count(),"waiting_confirmation":steps.iter().filter(|r|r["status"]=="waiting_confirmation").count(),"blocked":steps.iter().filter(|r|["failed","needs_reconciliation","waiting_input"].contains(&r["status"].as_str().unwrap_or(""))).count()},
                    "configurations":s.governance.iter().filter(|r|["workflow_instance","project_configuration","work_configuration"].contains(&r.kind.as_str())).collect::<Vec<_>>(),
                    "delivery_types":s.governance.iter().filter(|r|r.kind=="delivery_type"&&template_record_current(&s,r)).collect::<Vec<_>>(),"definitions":s.definitions.iter().filter(|d|template_projection_current(&s,d)).collect::<Vec<_>>(),"harnesses":s.harnesses,"sandboxes":s.sandboxes,"detail":detail,"data_root":self.store.root(),"execution_mode":"live",
                    "repository_ready":s.repository_worktrees.iter().any(|w|w.kind==WorktreeKind::Mainline),"global_concurrency":global_limit}),
                )
            }
            "query.history" => Ok(history::query(&s, p, offset, limit)),
            "query.events" => {
                let cursor = p.get("cursor").and_then(Value::as_u64).unwrap_or(0);
                Ok(json!(
                    s.events
                        .iter()
                        .filter(|e| e.id > cursor)
                        .take(limit)
                        .collect::<Vec<_>>()
                ))
            }
            "query.artifact" => {
                let id = text(p, "id")?;
                let a = s
                    .artifacts
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or_else(|| err("not_found", "Artifact 不存在"))?;
                Ok(json!(a))
            }
            _ => Err(err("method_not_found", "未知查询")),
        }
    }
    fn feature_detail(
        &self,
        s: &EngineeringState,
        fid: &str,
        limit: usize,
        _global_limit: usize,
        pages: &Value,
    ) -> Result<Value> {
        let feature = s.feature(fid)?;
        let offset = |key: &str| pages[key].as_u64().unwrap_or(0) as usize * limit;
        let tasks: Vec<_> = s.tasks.iter().filter(|t| t.feature_id == fid).collect();
        let documents: Vec<_> = s
            .documents
            .iter()
            .filter(|d| s.document_in_feature(d, fid))
            .collect();
        let snapshots: Vec<_> = s
            .snapshots
            .iter()
            .filter(|v| v.feature_id.as_deref() == Some(fid))
            .collect();
        let sessions: Vec<_> = s
            .sessions
            .iter()
            .filter(|v| v.feature_id.as_deref() == Some(fid))
            .collect();
        Ok(
            json!({"feature":feature,"tasks":tasks.iter().skip(offset("tasks")).take(limit).collect::<Vec<_>>(),"documents":documents.iter().rev().skip(offset("documents")).take(limit).collect::<Vec<_>>(),
            "snapshots":snapshots.iter().rev().skip(offset("snapshots")).take(limit).collect::<Vec<_>>(),"contexts":s.contexts.iter().filter(|c|c.owner_id==fid||tasks.iter().any(|t|t.id==c.owner_id)).collect::<Vec<_>>(),
            "sessions":sessions,"pending_interactions":s.interactions.iter().filter(|i|i.status==InteractionStatus::Pending&&sessions.iter().any(|v|v.id==i.session_id)).collect::<Vec<_>>(),
            "artifacts":s.artifacts.iter().filter(|a|a.feature_id.as_deref()==Some(fid)).rev().skip(offset("artifacts")).take(limit).collect::<Vec<_>>(),"heads":s.heads.iter().filter(|h|h.project_id==feature.project_id).collect::<Vec<_>>(),
            "totals":{"tasks":tasks.len(),"documents":documents.len(),"snapshots":snapshots.len()}}),
        )
    }
}
