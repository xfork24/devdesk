//! Application use cases and ports. Adapters cannot advance domain state.
mod advances;
mod agent_tasks;
mod builtin_defaults;
mod configuration;
mod default_catalog;
mod default_tasks;
mod shipped_prose;
pub use configuration::{
    project_configuration_references, retired as configuration_retired,
    visible as configuration_visible,
};
mod conversation_runtime;
mod conversations;
mod effects;
mod integrations;
pub use integrations::{
    freeze_integrations, integration_deletion_targets, integration_feature_lock,
};
mod governance;
mod live_merges;
mod live_runs;
mod model_approvals;
mod permission_resources;
mod permissions;
pub use permission_resources::{insert_permission_profiles, permission_profile_export};
mod permission_switch;
pub use permission_switch::permission_switch_id;
pub mod ports;
mod project_configuration;
mod proposals;
mod repositories;
mod workflow_definition;
mod workflow_runtime;
mod workflow_prompts;
// Compatibility names for existing adapters and contract fixtures.
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
fn binding(s: &EngineeringState) -> BindingLock {
    BindingLock {
        agent: s
            .definitions
            .iter()
            .find(|d| d.id == "builtin-agent-developer-v1")
            .unwrap()
            .id
            .clone(),
        skills: s
            .definitions
            .iter()
            .filter(|d| d.family == "skill")
            .take(1)
            .map(|d| d.id.clone())
            .collect(),
        standard: s
            .definitions
            .iter()
            .find(|d| d.family == "standard")
            .unwrap()
            .id
            .clone(),
        harness: String::new(),
        sandbox: String::new(),
    }
}
fn revision(s: &EngineeringState, id: &str) -> Option<u64> {
    s.projects
        .iter()
        .find(|x| x.id == id)
        .map(|x| x.revision)
        .or_else(|| s.features.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.tasks.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.attempts.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.merges.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.contexts.iter().find(|x| x.id == id).map(|x| x.revision))
        .or_else(|| s.effects.iter().find(|x| x.id == id).map(|x| x.revision))
}
fn mutable_feature(s: &EngineeringState, fid: &str) -> Result<()> {
    require(
        s.feature(fid)?.status != Status::Completed,
        "已完成 Feature 已冻结，请创建关联的新 Feature",
    )
}
fn effective_runs<'a>(s: &'a EngineeringState, a: &Attempt) -> Vec<&'a Run> {
    s.latest_runs(Some(&a.id), None)
        .into_iter()
        .filter(|r| !r.node.starts_with("review:"))
        .collect()
}
fn criteria(payload: &Value, body: &str) -> Result<Vec<AcceptanceCriterion>> {
    let items: Vec<AcceptanceCriterion> = match payload.get("acceptance") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| err("invalid_params", "acceptance 应为 key/description 列表"))?,
        None => vec![AcceptanceCriterion {
            key: "AC-1".into(),
            description: body.into(),
        }],
    };
    require(
        !items.is_empty()
            && items
                .iter()
                .all(|c| !c.key.trim().is_empty() && !c.description.trim().is_empty())
            && items
                .iter()
                .map(|c| &c.key)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == items.len(),
        "验收条目必须非空且标识唯一",
    )?;
    Ok(items)
}
fn same_document_stream(document: &Document, kind: &str, title: &str) -> bool {
    document.kind == kind
        && (kind != "doc"
            || (document.title == "Structured Task Plan") == (title == "Structured Task Plan"))
}
fn document_ids(s: &EngineeringState, fid: &str, tid: Option<&str>) -> Vec<Id> {
    s.documents
        .iter()
        .filter(|d| {
            s.document_in_feature(d, &fid)
                && (d.task_id.is_none() || d.task_id.as_deref() == tid)
                && !s.documents.iter().any(|n| {
                    n.supersedes.as_deref() == Some(&d.id)
                        && same_document_stream(n, &d.kind, &d.title)
                })
        })
        .map(|d| d.id.clone())
        .collect()
}
fn git_context(s: &EngineeringState, fid: &str, tid: Option<&str>) -> Result<GitContext> {
    let f = s.feature(fid)?;
    let project = s
        .projects
        .iter()
        .find(|p| p.id == f.project_id)
        .ok_or_else(|| err("not_found", "Project 不存在"))?;
    let workspace = project.workspace_definition.clone().unwrap_or(Workspace {
        repository: String::new(),
        path: project.workspace.clone(),
        mainline: "main".into(),
    });
    require(
        project.execution_mode == "live",
        "历史项目只读，不能创建执行范围",
    )?;

    let location = |owner: &str| -> Result<CodeLocation> {
        let w = s
            .repository_worktrees
            .iter()
            .find(|w| w.project_id == project.id && w.owner_id == owner)
            .ok_or_else(|| err("blocked", "受管理 worktree 尚未就绪，请核对仓库操作"))?;
        Ok(CodeLocation {
            owner: owner.into(),
            branch: w.branch.clone(),
            worktree: w.path.clone(),
        })
    };
    Ok(GitContext {
        project: project.id.clone(),
        workspace,
        source: location(tid.unwrap_or(fid))?,
        target: location(if tid.is_some() { fid } else { &project.id })?,
        origin: Origin::External,
    })
}
/// Seed shared configuration without creating project or execution facts.
pub fn seed_configuration(s: &mut EngineeringState) {
    let existing = s.governance.clone();
    for body in [
        include_str!("../../../permissions/presets/container-full-access.json"),
        include_str!("../../../permissions/presets/read-only.json"),
        include_str!("../../../permissions/presets/task-implementation.json"),
        include_str!("../../../permissions/presets/test-author.json"),
        include_str!("../../../permissions/presets/isolated-tests.json"),
        include_str!("../../../permissions/presets/legacy-restricted-v1.json"),
    ] {
        let p: Permission = serde_json::from_str(body).expect("embedded Permission preset");
        if !s.permissions.iter().any(|old| old.id == p.id) {
            s.permissions.push(p);
        }
    }
    if s.definitions.is_empty() {
        for (family, name, version, body) in [
            (
                "spec",
                "Internal Design Spec",
                1,
                include_str!("../../../resources/configuration/specs/legacy-planning.md"),
            ),
            (
                "spec",
                "Internal Design Spec",
                2,
                include_str!("../../../resources/configuration/specs/legacy-planning.md"),
            ),
            (
                "spec",
                "Alternative Design Spec",
                1,
                include_str!("../../../resources/configuration/specs/architecture.md"),
            ),
            (
                "skill",
                "TDD",
                1,
                include_str!("../../../resources/configuration/skills/engineering-method.md"),
            ),
            (
                "standard",
                "工程默认 Standard",
                1,
                include_str!("../../../resources/configuration/standards/engineering.md"),
            ),
        ] {
            let slots = if family == "standard" {
                s.definitions
                    .iter()
                    .filter(|d| d.family == "spec" && d.version == 1)
                    .take(1)
                    .flat_map(|d| {
                        ["planning", "implementation", "merge"].map(|purpose| SpecSlot {
                            purpose: purpose.into(),
                            spec: d.id.clone(),
                        })
                    })
                    .collect()
            } else {
                vec![]
            };
            let rules = if family == "spec" {
                if name == "Alternative Design Spec" {
                    vec!["sections".into()]
                } else if version == 2 {
                    vec!["traceability".into(), "failure_paths".into()]
                } else {
                    vec!["traceability".into()]
                }
            } else {
                vec![]
            };
            let predecessor = if family == "spec" && version > 1 {
                s.definitions
                    .iter()
                    .rev()
                    .find(|d| d.family == family && d.name == name)
                    .map(|d| d.id.clone())
            } else {
                None
            };
            s.definitions.push(Definition {
                id: id(),
                family: family.into(),
                name: name.into(),
                version,
                supersedes: predecessor,
                body: body.into(),
                slots,
                rules,
                agent_profile: None,
            });
        }
    }
    for (key, name, role, instructions, tool_policy, allowed_work, output_schemas) in [
        (
            "analyst",
            "需求分析师",
            AgentRole::Analyst,
            "Clarify requirements, constraints and acceptance criteria. Produce proposals; do not mutate engineering state or code.",
            ToolPolicy::ReadOnly,
            vec![WorkType::Discussion],
            vec!["requirement/v1", "feature/v1"],
        ),
        (
            "architect",
            "架构师",
            AgentRole::Architect,
            "Explain architecture, interfaces and tradeoffs. Derive design and task/test plans from frozen requirement revisions.",
            ToolPolicy::ReadOnly,
            vec![WorkType::Discussion, WorkType::Design],
            vec!["design/v1", "task_plan/v1", "test_plan/v1"],
        ),
        (
            "developer",
            "开发",
            AgentRole::Developer,
            "Implement only the frozen Task scope inside its sandbox. Preserve evidence and report changes; never approve your own delivery.",
            ToolPolicy::TaskWrite,
            vec![WorkType::Discussion, WorkType::Implementation],
            vec!["implementation/v1"],
        ),
        (
            "tester",
            "测试",
            AgentRole::Tester,
            "Derive tests from explicit acceptance criteria. Distinguish assertion failure from environment failure and retain command/revision evidence.",
            ToolPolicy::TestWrite,
            vec![WorkType::Discussion, WorkType::Testing],
            vec!["test_plan/v1", "test_result/v1"],
        ),
        (
            "reviewer",
            "评审",
            AgentRole::Reviewer,
            "Independently review frozen delivery and candidate evidence. Return per-check decisions and reasons; do not modify code or approve on behalf of a human.",
            ToolPolicy::ReadOnly,
            vec![WorkType::Discussion, WorkType::Review],
            vec!["review/v1"],
        ),
    ] {
        let identity = format!("builtin-agent-{key}-v1");
        if !s.definitions.iter().any(|d| d.id == identity) {
            s.definitions.push(Definition {
                id: identity,
                family: "agent".into(),
                name: name.into(),
                version: 1,
                supersedes: None,
                body: instructions.into(),
                slots: vec![],
                rules: vec![],
                agent_profile: Some(AgentProfile {
                    permission_overrides: None,
                    role,
                    skill_revisions: vec![],
                    tool_policy,
                    allowed_work,
                    output_schemas: output_schemas.into_iter().map(str::to_owned).collect(),
                }),
            });
        }
    }
    governance::seed_governance(s);
    permission_resources::seed_permission_resources(s);
    default_tasks::seed_task_workflows(s);
    builtin_defaults::discard_reintroduced_history(s, &existing);
    shipped_prose::upgrade(s);
}

impl Engine {
    fn revise_project_requirement(
        &self,
        s: &mut EngineeringState,
        project: &str,
        p: &Value,
    ) -> Result<Value> {
        require(
            opt(p, "kind").is_none_or(|kind| kind == "requirement"),
            "Project 讨论成果目前仅接受 Requirement 修订",
        )?;
        require(
            p.get("feature_id").is_none() && p.get("task_id").is_none(),
            "Project Requirement 不得携带 Feature/Task 归属",
        )?;
        let body = text(p, "body")?;
        let acceptance = criteria(p, &body)?;
        let previous = opt(p, "supersedes");
        let version = if let Some(id) = &previous {
            let old = s
                .documents
                .iter()
                .find(|d| {
                    &d.id == id
                        && d.project_id.as_deref() == Some(project)
                        && d.kind == "requirement"
                })
                .ok_or_else(|| err("not_found", "Project Requirement 前版不存在"))?;
            require(
                !s.documents
                    .iter()
                    .any(|d| d.supersedes.as_ref() == Some(id)),
                "Requirement 已有后继，请采用当前修订",
            )?;
            old.version + 1
        } else {
            1
        };
        let mut artifact = self.artifacts.seal_scoped(
            &MaterialOwner::Project { id: project.into() },
            "documents",
            &body,
        )?;
        artifact.origin = Origin::Human;
        let artifact_id = artifact.id.clone();
        s.artifacts.push(artifact);
        let did = id();
        s.documents.push(Document {
            test_recipe: None,
            id: did.clone(),
            feature_id: None,
            project_id: Some(project.into()),
            task_id: None,
            kind: "requirement".into(),
            title: text(p, "title")?,
            body,
            version,
            supersedes: previous,
            requirement_id: None,
            design_id: None,
            origin: Origin::Human,
            acceptance,
            verifies: vec![],
            artifact_id: Some(artifact_id),
        });
        s.projects
            .iter_mut()
            .find(|p| p.id == project)
            .unwrap()
            .revision += 1;
        Ok(json!({"id":did}))
    }
    fn snapshot_project(
        &self,
        s: &mut EngineeringState,
        project: &str,
        p: &Value,
    ) -> Result<Value> {
        let owner = s.projects.iter().find(|p| p.id == project).unwrap();
        let binding = owner.binding.clone();
        let documents: Vec<Id> = if let Some(list) = p.get("documents") {
            serde_json::from_value(list.clone())
                .map_err(|_| err("invalid_params", "documents 必须为 ID 数组"))?
        } else {
            s.documents
                .iter()
                .filter(|d| d.project_id.as_deref() == Some(project))
                .map(|d| d.id.clone())
                .collect()
        };
        require(
            documents.iter().all(|id| {
                s.documents
                    .iter()
                    .any(|d| &d.id == id && d.project_id.as_deref() == Some(project))
            }),
            "Project Snapshot 不得引用其他范围的材料",
        )?;
        let content = s
            .contexts
            .iter()
            .find(|c| c.owner_id == project)
            .map(|c| c.content.clone())
            .unwrap_or_default();
        let sid = id();
        let snapshot = Snapshot {
            execution_instances: vec![],
            runtime_bindings: vec![],
            id: sid.clone(),
            feature_id: None,
            project_id: Some(project.into()),
            task_id: None,
            kind: "project_context".into(),
            content,
            binding,
            documents,
            delivery_id: None,
            inherited_from: None,
            capture: SnapshotCapture {
                attempt: None,
                runs: vec![],
                sessions: s
                    .sessions
                    .iter()
                    .filter(|session| session.owner_id == project)
                    .map(|session| session.id.clone())
                    .collect(),
                resources: vec![],
                captured_at: Some(now()),
                reason: opt(p, "reason").unwrap_or_else(|| "Project discussion context".into()),
            },
        };
        let mut artifact = self.artifacts.seal_scoped(
            &MaterialOwner::Project { id: project.into() },
            "snapshots",
            &serde_json::to_string_pretty(&snapshot)
                .map_err(|e| err("serialization", e.to_string()))?,
        )?;
        artifact.origin = Origin::Human;
        s.artifacts.push(artifact);
        s.snapshots.push(snapshot);
        self.index_experience(s, &sid)?;
        Ok(json!({"id":sid}))
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
            for r in s.governance.iter_mut().filter(|r| r.kind=="tool_call" && r.data["status"]=="dispatched") { let mut d=r.data.clone();d["status"]=json!("needs_reconciliation");d["remote_effect"]=json!("unknown");governance::update_record(r,d); }
            configuration::migrate_capabilities(s);
            self.freeze_legacy_features(s);
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
            for op in s.repository_operations.iter_mut().filter(|op|op.status==RepositoryOperationStatus::Dispatched){op.status=RepositoryOperationStatus::NeedsReconciliation;op.reason="restart_after_dispatch".into();op.revision+=1;}
            for op in s.repository_operations.iter_mut().filter(|op|op.status==RepositoryOperationStatus::Prepared){op.status=RepositoryOperationStatus::NeedsReconciliation;op.reason="startup_pending_intent_requires_reconcile".into();op.revision+=1;}
            for session in s.sessions.iter_mut().filter(|v|v.scope.is_some() && v.external_session.is_some() && !matches!(v.lifecycle,Some(SessionLifecycle::Closed|SessionLifecycle::RequiresSuccessor))){
                let uncertain=s.turns.iter().any(|t|t.session_id==session.id&&t.status.active()&&t.status!=TurnStatus::Queued)||s.executions.iter().any(|e|e.session_id==session.id&&!e.writer_revoked);
                if uncertain {session.lifecycle=Some(SessionLifecycle::NeedsReconciliation);session.revision+=1;}
                else if session.lifecycle==Some(SessionLifecycle::NeedsReconciliation){session.lifecycle=Some(SessionLifecycle::Idle);session.revision+=1;}
            }
            for turn in s.turns.iter_mut().filter(|t|[TurnStatus::Dispatching,TurnStatus::Running,TurnStatus::WaitingInteraction].contains(&t.status)) {
                turn.status=TurnStatus::NeedsReconciliation;turn.revision+=1;
                if let Some(session)=s.sessions.iter_mut().find(|v|v.id==turn.session_id) {session.lifecycle=Some(SessionLifecycle::NeedsReconciliation);session.revision+=1;}
            }
            for intent in s.advances.iter_mut().filter(|a|a.status==AdvanceStatus::Dispatched) {intent.status=AdvanceStatus::NeedsReconciliation;intent.reason="reconcile_requested".into();intent.revision+=1;}
            let interrupted = s
                .runs
                .iter()
                .filter(|r| r.status == Status::Running)
                .map(|r| r.id.clone())
                .collect::<Vec<_>>();
            for rid in interrupted {
                let r = s.runs.iter_mut().find(|r| r.id == rid).unwrap();
                r.status = Status::Interrupted;
                r.finished = Some(now());
                if let Some(aid) = r.attempt_id.clone() {
                    if let Some(a) = s.attempts.iter_mut().find(|a| {
                        a.id == aid && a.status == Status::Running && !r.node.starts_with("review:")
                    }) {
                        a.status = Status::Interrupted;
                        a.revision += 1;
                    }
                }
                if let Some(instance) = s
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.as_mut())
                {
                    instance.status = Status::Interrupted;
                    instance.finished = Some(now());
                }
                let interrupted_run = s.runs.iter().find(|r|r.id==rid).unwrap().clone();
                let (fid,tid)=if let Some(aid)=&interrupted_run.attempt_id {let a=s.attempt(aid)?;(a.feature_id.clone(),a.task_id.clone())}else{let m=s.merge(interrupted_run.merge_id.as_deref().unwrap())?;(m.feature_id.clone(),m.task_id.clone())};
                let mut report=self.artifacts.seal(&fid,tid.as_deref(),"runs",&format!("# Interrupted Run {rid}\norigin={:?}\n结果未确认；恢复仅核对外部事实，不自动接纳。",interrupted_run.origin))?;report.origin=interrupted_run.origin;s.runs.iter_mut().find(|r|r.id==rid).unwrap().result_artifact=Some(report.id.clone());s.artifacts.push(report);
                for effect in s.effects.iter_mut().filter(|e|e.run_id==rid&&[EffectStatus::Pending,EffectStatus::Observed].contains(&e.status)&&e.kind!=EffectKind::Cleanup) {effect.status=EffectStatus::NeedsReconciliation;effect.reason="restart_native_observation_only".into();effect.revision+=1;}
                event(
                    s,
                    &rid,
                    "run.interrupted",
                    "内核重启，执行结果未确认；需要创建重试",
                );
            }
            Ok(json!({"ok":true}))
        })?;
        self.drain_advances()?;
        Ok(())
    }
    pub fn execute(&self, method: &str, c: Command) -> Result<Value> {
        self.execute_internal(method, c, None)
    }
    pub fn validate_attempt_cancellation(&self, owner: &str) -> Result<()> {
        let s = self.store.snapshot()?;
        let worktree = s.repository_worktrees.iter().find(|w| w.owner_id == owner)
            .map(|w| w.path.as_str()).unwrap_or("");
        if s.run_cancellation_pending(worktree) {
            return Err(err("run_cancellation_pending", "等待旧 Run 取消确认后再启动"));
        }
        Ok(())
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
                "attempt.start",
                "review.request",
                "merge.request",
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
            let result = self.apply_command(method, &c, s)?;
            self.governance_after_command(method, &c, s, &result)?;
            Ok(result)
        };
        let result = self
            .store
            .transact(Some((&c.command_id, &fingerprint)), &mut action)?;
        // Long operations return their durable ID; the scheduler owns supplier dispatch.
        Ok(result)
    }
    fn apply_command(&self, method: &str, c: &Command, s: &mut EngineeringState) -> Result<Value> {
        self.enforce_workflow_action(s, method, c)?;
        if let Some(target) = &c.target {
            if let Some(actual) = revision(s, target) {
                if c.expected_revision != Some(actual) {
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
        if let Some(result) = self.permission_command(method, c, s) {
            return result;
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
        if [
            "attempt.start",
            "feature.plan",
            "task.start",
            "feature.close",
            "merge.request",
        ]
        .contains(&method)
        {
            let project = if let Some(task) = s.tasks.iter().find(|t| t.id == target) {
                Some(s.feature(&task.feature_id)?.project_id.as_str())
            } else {
                s.features
                    .iter()
                    .find(|f| f.id == target)
                    .map(|f| f.project_id.as_str())
            };
            require(
                !s.projects
                    .iter()
                    .any(|p| Some(p.id.as_str()) == project && p.execution_mode == "live")
                    || (method == "attempt.start" && p.get("live_sessions").is_some())
                    || (method == "merge.request" && p.get("merge_session").is_some()),
                "工程执行需要宿主解析的原生 Session",
            )?;
        }
        let result = match method {
            "agent.create" | "agent.revise" => {
                let old = if method == "agent.revise" {
                    let old = s
                        .definitions
                        .iter()
                        .find(|d| d.id == target && d.family == "agent")
                        .cloned()
                        .ok_or_else(|| err("not_found", "Agent 修订不存在"))?;
                    require(
                        c.expected_revision == Some(old.version),
                        "需要明确 Agent expected_revision",
                    )?;
                    require(
                        !s.definitions
                            .iter()
                            .any(|d| d.supersedes.as_deref() == Some(target)),
                        "Agent 已有后继修订，请使用当前修订",
                    )?;
                    Some(old)
                } else {
                    None
                };
                let profile: AgentProfile = serde_json::from_value(p["profile"].clone())
                    .map_err(|_| err("invalid_params", "需要完整的 Agent profile"))?;
                profile.validate()?;
                if let Some(old) = &old {
                    require(
                        old.agent_profile
                            .as_ref()
                            .is_some_and(|p| p.role == profile.role),
                        "不可通过修订改变角色身份，请创建新的 Agent",
                    )?;
                }
                require(
                    profile.skill_revisions.iter().all(|id| {
                        s.definitions
                            .iter()
                            .any(|d| &d.id == id && d.family == "skill")
                    }),
                    "角色 Skill 必须引用明确修订",
                )?;
                let did = id();
                s.definitions.push(Definition {
                    id: did.clone(),
                    family: "agent".into(),
                    name: text(p, "name")?,
                    version: old.as_ref().map_or(1, |d| d.version + 1),
                    supersedes: old.map(|d| d.id),
                    body: text(p, "instructions")?,
                    slots: vec![],
                    rules: vec![],
                    agent_profile: Some(profile),
                });
                json!({"id":did})
            }
            "project.create" => {
                let scoped = self.store.project_id();
                require(
                    scoped.is_none() || s.projects.is_empty(),
                    "项目数据库只能保存一个 Project",
                )?;
                let pid = scoped.unwrap_or_else(id);
                let title = text(p, "title")?;
                let b = binding(s);
                s.projects.push(Project {
                    permission_overrides: None,
                    runtime_preferences: None,
                    id: pid.clone(),
                    title,
                    workspace: opt(p, "workspace").unwrap_or_default(),
                    workspace_definition: Some(Workspace {
                        repository: opt(p, "repository").unwrap_or_default(),
                        path: opt(p, "workspace").unwrap_or_default(),
                        mainline: opt(p, "mainline").unwrap_or_else(|| "main".into()),
                    }),
                    execution_mode: "live".into(),
                    binding: b,
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
                let project = s
                    .projects
                    .iter_mut()
                    .find(|x| x.id == target)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?;
                if let Some(title) = opt(p, "title") {
                    project.title = title;
                }
                if let Some(value) = p.get("permission_overrides") {
                    project.permission_overrides = serde_json::from_value(value.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                }
                if let Some(value) = p.get("runtime_preferences") {
                    let preferences: Option<ProjectRuntimePreferences> =
                        serde_json::from_value(value.clone())
                            .map_err(|_| err("invalid_params", "无效项目运行时偏好"))?;
                    if let Some(p) = &preferences {
                        p.validate()?;
                    }
                    project.runtime_preferences = preferences;
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
                let b = project.binding.clone();
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
                let source_requirement = opt(p, "source_requirement");
                if let Some(id) = &source_requirement {
                    require(
                        s.documents.iter().any(|d| {
                            &d.id == id
                                && d.project_id.as_ref() == Some(&pid)
                                && d.kind == "requirement"
                        }),
                        "Feature 必须引用同项目的 Requirement 修订",
                    )?;
                }
                s.features.push(Feature {
                    source_requirement,
                    id: fid.clone(),
                    project_id: pid.clone(),
                    title: text(p, "title")?,
                    status: Status::Draft,
                    revision: 1,
                    input_version: 1,
                    binding: b,
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
            "document.revise" => {
                if s.projects.iter().any(|project| project.id == target) {
                    self.revise_project_requirement(s, target, p)?
                } else {
                    require(
                        !target.is_empty(),
                        "document.revise 必须指定 Feature target 和 expected_revision",
                    )?;
                    let fid = s.feature(target)?.id.clone();
                    require(
                        opt(p, "feature_id").is_none_or(|id| id == fid),
                        "Feature target 与 payload 不一致",
                    )?;
                    mutable_feature(s, &fid)?;
                    let task_id = opt(p, "task_id");
                    if let Some(tid) = &task_id {
                        require(s.task(tid)?.feature_id == fid, "Task 不属于当前 Feature")?;
                        require(
                            s.task(tid)?.status != Status::Completed,
                            "已合入 Task 已冻结",
                        )?;
                    }
                    let kind = text(p, "kind")?;
                    require(
                        ["requirement", "design", "test", "doc"].contains(&kind.as_str()),
                        "未知文档类型",
                    )?;
                    require(
                        task_id.is_none() || !["requirement", "design"].contains(&kind.as_str()),
                        "Requirement/Design 必须归属 Feature",
                    )?;
                    let old = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| {
                            s.document_in_feature(d, &fid)
                                && d.task_id == task_id
                                && same_document_stream(
                                    d,
                                    &kind,
                                    p["title"].as_str().unwrap_or("修订文档"),
                                )
                        })
                        .cloned();
                    let did = id();
                    let mut artifact = self.artifacts.seal(
                        &fid,
                        task_id.as_deref(),
                        "documents",
                        &text(p, "body")?,
                    )?;
                    artifact.origin = Origin::Human;
                    let artifact_id = Some(artifact.id.clone());
                    s.artifacts.push(artifact);
                    let req = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| s.document_in_feature(d, &fid) && d.kind == "requirement")
                        .map(|d| d.id.clone());
                    let design = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| s.document_in_feature(d, &fid) && d.kind == "design")
                        .map(|d| d.id.clone());
                    s.documents.push(Document {
                        test_recipe: if kind == "test" {
                            p.get("test_recipe")
                                .map(|v| {
                                    serde_json::from_value::<TestRecipe>(v.clone())
                                        .map_err(|_| err("invalid_params", "无效测试配方"))
                                })
                                .transpose()?
                        } else {
                            None
                        },
                        project_id: None,
                        id: did.clone(),
                        feature_id: Some(fid.clone()),
                        task_id: task_id.clone(),
                        kind: kind.clone(),
                        title: opt(p, "title").unwrap_or_else(|| "修订文档".into()),
                        body: text(p, "body")?,
                        version: old.as_ref().map_or(1, |d| d.version + 1),
                        supersedes: old.map(|d| d.id),
                        requirement_id: req.clone(),
                        design_id: design,
                        origin: Origin::Human,
                        acceptance: if kind == "requirement" {
                            criteria(p, &text(p, "body")?)?
                        } else {
                            vec![]
                        },
                        verifies: if kind == "test" {
                            match p.get("verifies") {
                                Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
                                    err("invalid_params", "verifies 应为验收条目列表")
                                })?,
                                None => req
                                    .as_deref()
                                    .map(|r| s.acceptance_keys(r))
                                    .transpose()?
                                    .unwrap_or_default(),
                            }
                        } else {
                            vec![]
                        },
                        artifact_id,
                    });
                    let f = s.features.iter_mut().find(|f| f.id == fid).unwrap();
                    let global_input = task_id.is_none()
                        && ["requirement", "design", "test"].contains(&kind.as_str());
                    if global_input {
                        f.input_version += 1;
                    }
                    f.revision += 1;
                    if ![Status::Draft, Status::Planning].contains(&f.status) {
                        f.status = Status::Developing;
                    }
                    let completed = s
                        .tasks
                        .iter()
                        .filter(|t| t.feature_id == fid && t.status == Status::Completed)
                        .map(|t| t.id.clone())
                        .collect::<Vec<_>>();
                    let preserve_requirement =
                        s.feature_configuration(&fid).is_some() && kind != "requirement";
                    for d in s.deliveries.iter_mut().filter(|d| {
                        d.feature_id == fid
                            && !(preserve_requirement && d.kind == "requirement")
                            && d.task_id.as_ref().is_none_or(|id| !completed.contains(id))
                            && (global_input
                                || (task_id.is_some() && d.task_id == task_id)
                                || (d.task_id.is_none()
                                    && ["closure", "merge_candidate"].contains(&d.kind.as_str())))
                    }) {
                        d.stale = true;
                    }
                    for a in s.attempts.iter_mut().filter(|a| {
                        a.feature_id == fid
                            && !(preserve_requirement && a.phase == "requirement_definition")
                            && (global_input
                                || (task_id.is_some() && a.task_id == task_id)
                                || a.phase == "closure")
                            && [Status::Prepared, Status::Running, Status::WaitingReview]
                                .contains(&a.status)
                    }) {
                        a.paused = true;
                        if !global_input {
                            a.status = Status::Interrupted;
                        }
                        a.revision += 1;
                    }
                    let affected_attempts: Vec<_> = s
                        .attempts
                        .iter()
                        .filter(|a| {
                            a.feature_id == fid
                                && !(preserve_requirement && a.phase == "requirement_definition")
                                && (global_input
                                    || (task_id.is_some() && a.task_id == task_id)
                                    || a.phase == "closure")
                                && a.paused
                        })
                        .map(|a| a.id.clone())
                        .collect();
                    for r in s.runs.iter_mut().filter(|r| {
                        r.attempt_id
                            .as_ref()
                            .is_some_and(|id| affected_attempts.contains(id))
                    }) {
                        if r.status == Status::Running {
                            r.cancel_requested = true;
                        }
                        if r.status == Status::Queued {
                            r.status = Status::Cancelled;
                            r.finished = Some(now());
                        }
                    }
                    for t in s.tasks.iter_mut().filter(|t| {
                        t.feature_id == fid
                            && t.status != Status::Completed
                            && (global_input || task_id.as_ref() == Some(&t.id))
                    }) {
                        if [Status::Accepted, Status::Merging].contains(&t.status) {
                            t.status = Status::Ready;
                            t.revision += 1;
                        }
                    }
                    if global_input {
                        self.invalidate_merges(s, &fid, None);
                    } else {
                        if let Some(tid) = task_id.as_deref() {
                            self.invalidate_merges(s, &fid, Some(tid));
                        }
                        self.invalidate_feature_merges(s, &fid);
                    }
                    event(
                        s,
                        &fid,
                        "evidence.stale",
                        "输入修订，旧证据保留但不再满足当前门禁",
                    );
                    json!({"id":did})
                }
            }
            "definition.revise" => {
                let old = s
                    .definitions
                    .iter()
                    .find(|d| d.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "定义不存在"))?;
                let mut d = old.clone();
                d.id = id();
                d.supersedes = Some(old.id);
                d.version = s
                    .definitions
                    .iter()
                    .filter(|x| x.name == old.name && x.family == old.family)
                    .map(|x| x.version)
                    .max()
                    .unwrap_or(0)
                    + 1;
                d.name = opt(p, "name").unwrap_or(old.name);
                d.body = text(p, "body")?;
                if let Some(slots) = p.get("slots") {
                    d.slots = serde_json::from_value(slots.clone())
                        .map_err(|_| err("invalid_params", "slots 应为版本 ID 列表"))?;
                    for slot in &d.slots {
                        require(
                            ["planning", "implementation", "merge"]
                                .contains(&slot.purpose.as_str())
                                && s.definitions
                                    .iter()
                                    .any(|x| x.id == slot.spec && x.family == "spec"),
                            "Standard slot 必须包含用途并引用 Spec 版本",
                        )?;
                    }
                    require(
                        d.slots
                            .iter()
                            .map(|x| &x.purpose)
                            .collect::<std::collections::HashSet<_>>()
                            .len()
                            == d.slots.len(),
                        "Standard 用途 slot 不可重复",
                    )?;
                }
                if let Some(rules) = p.get("rules") {
                    d.rules = serde_json::from_value(rules.clone())
                        .map_err(|_| err("invalid_params", "rules 应为内置规则名列表"))?;
                    require(
                        d.rules.iter().all(|r| {
                            ["traceability", "failure_paths", "sections"].contains(&r.as_str())
                        }),
                        "不支持任意规范脚本",
                    )?;
                }
                let did = d.id.clone();
                s.definitions.push(d);
                json!({"id":did})
            }
            "binding.update" => {
                let fid = s
                    .tasks
                    .iter()
                    .find(|t| t.id == target)
                    .map(|t| t.feature_id.as_str())
                    .unwrap_or(target);
                require(
                    s.feature_configuration(fid).is_none(),
                    "Feature 创建时的配置已冻结；请创建关联后继 Feature",
                )?;
                let b: BindingLock = serde_json::from_value(p.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                s.validate_binding(&b)?;
                if let Some(t) = s.tasks.iter().find(|x| x.id == target).cloned() {
                    mutable_feature(s, &t.feature_id)?;
                    require(t.status != Status::Completed, "Task 已冻结")?;
                    let t = s.tasks.iter_mut().find(|x| x.id == target).unwrap();
                    t.binding = b;
                    t.revision += 1;
                } else if let Some(f) = s.features.iter_mut().find(|x| x.id == target) {
                    require(f.status != Status::Completed, "Feature 已冻结")?;
                    f.binding = b;
                    f.revision += 1;
                } else {
                    let x = s
                        .projects
                        .iter_mut()
                        .find(|x| x.id == target)
                        .ok_or_else(|| err("not_found", "绑定 owner 不存在"))?;
                    x.binding = b;
                    x.revision += 1;
                }
                json!({"id":target})
            }
            "task.create" => {
                let f = s.feature(target)?.clone();
                require(
                    [Status::Ready, Status::Developing].contains(&f.status),
                    "规划必须通过 Review",
                )?;
                require(s.planning_accepted(&f.id), "当前输入版本规划尚未接受")?;
                let snap = s
                    .snapshots
                    .iter()
                    .rev()
                    .find(|x| x.feature_id.as_deref() == Some(f.id.as_str()) && x.kind == "initial")
                    .cloned()
                    .ok_or_else(|| err("blocked", "缺少初始 Snapshot"))?;
                let deps: Vec<String> =
                    serde_json::from_value(p.get("dependencies").cloned().unwrap_or(json!([])))
                        .map_err(|_| err("invalid_params", "dependencies 应为 Task ID 列表"))?;
                for dep in &deps {
                    require(s.task(dep)?.feature_id == f.id, "依赖必须属于同 Feature")?;
                }
                let live = s
                    .projects
                    .iter()
                    .any(|p| p.id == f.project_id && p.execution_mode == "live");
                let tid = id();
                let requirement = snap
                    .documents
                    .iter()
                    .rev()
                    .find(|id| {
                        s.documents
                            .iter()
                            .any(|d| &d.id == *id && d.kind == "requirement")
                    })
                    .cloned();
                let design_ref = snap
                    .documents
                    .iter()
                    .rev()
                    .find(|id| {
                        s.documents
                            .iter()
                            .any(|d| &d.id == *id && d.kind == "design")
                    })
                    .cloned();
                let mut scope = if let Some(scope) = p.get("scope") {
                    serde_json::from_value(scope.clone())
                        .map_err(|_| err("invalid_params", "无效 Task scope"))?
                } else {
                    TaskScope {
                        acceptance: requirement
                            .as_deref()
                            .map(|r| s.acceptance_keys(r))
                            .transpose()?
                            .unwrap_or_default(),
                        requirement,
                        design: design_ref,
                    }
                };
                if let Some(keys) = p.get("acceptance_keys") {
                    scope.acceptance = serde_json::from_value(keys.clone())
                        .map_err(|_| err("invalid_params", "acceptance_keys 应为条目标识列表"))?;
                }
                s.validate_scope(&f.id, &scope)?;
                require(
                    scope
                        .requirement
                        .as_ref()
                        .is_some_and(|id| snap.documents.contains(id))
                        && scope
                            .design
                            .as_ref()
                            .is_some_and(|id| snap.documents.contains(id)),
                    "Task 范围必须来自当前已接受 Snapshot",
                )?;
                s.tasks.push(Task {
                    id: tid.clone(),
                    feature_id: f.id.clone(),
                    title: text(p, "title")?,
                    status: if live {
                        Status::Prepared
                    } else {
                        Status::Ready
                    },
                    revision: 1,
                    binding: f.binding,
                    inherited_snapshot: snap.id.clone(),
                    dependencies: deps,
                    worktree: format!("pending-worktree/{tid}"),
                    scope: scope.clone(),
                });
                if s.projects
                    .iter()
                    .any(|p| p.id == f.project_id && p.execution_mode == "live")
                {
                    let base = s
                        .heads
                        .iter()
                        .find(|h| h.id == f.id)
                        .ok_or_else(|| err("blocked", "Feature 缺少代码头"))?
                        .revision
                        .clone();
                    self.enqueue_worktree(s, &f.project_id, &tid, WorktreeKind::Task, &base);
                }
                s.contexts.push(Context {
                    id: id(),
                    owner_id: tid.clone(),
                    content: snap.content,
                    revision: 1,
                });
                let req = s
                    .documents
                    .iter()
                    .rev()
                    .find(|d| s.document_in_feature(d, &f.id) && d.kind == "requirement")
                    .map(|d| d.id.clone());
                let design = s
                    .documents
                    .iter()
                    .rev()
                    .find(|d| s.document_in_feature(d, &f.id) && d.kind == "design")
                    .map(|d| d.id.clone());
                let test_body = s
                    .documents
                    .iter()
                    .rev()
                    .find(|d| {
                        s.document_in_feature(d, &f.id) && d.kind == "test" && d.task_id.is_none()
                    })
                    .ok_or_else(|| err("blocked", "规划缺少测试定义"))?
                    .body
                    .clone();
                let source_origin = s
                    .documents
                    .iter()
                    .rev()
                    .find(|d| {
                        s.document_in_feature(d, &f.id) && d.kind == "test" && d.task_id.is_none()
                    })
                    .map(|d| d.origin)
                    .unwrap_or(Origin::Human);
                let mut test_artifact =
                    self.artifacts
                        .seal(&f.id, Some(&tid), "documents", &test_body)?;
                if live {
                    test_artifact.origin = source_origin;
                }
                let test_artifact_id = Some(test_artifact.id.clone());
                s.artifacts.push(test_artifact);
                s.documents.push(Document {
                    test_recipe: p
                        .get("test_recipe")
                        .filter(|v| !v.is_null())
                        .map(|v| {
                            serde_json::from_value::<TestRecipe>(v.clone())
                                .map_err(|_| err("invalid_params", "无效 Task 测试配方"))
                        })
                        .transpose()?
                        .or_else(|| {
                            s.documents
                                .iter()
                                .rev()
                                .find(|d| {
                                    s.document_in_feature(d, &f.id)
                                        && d.kind == "test"
                                        && d.task_id.is_none()
                                })
                                .and_then(|d| d.test_recipe.clone())
                        }),
                    project_id: None,
                    id: id(),
                    feature_id: Some(f.id.clone()),
                    task_id: Some(tid.clone()),
                    kind: "test".into(),
                    title: "Task 验收测试定义".into(),
                    body: s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| {
                            s.document_in_feature(d, &f.id)
                                && d.kind == "test"
                                && d.task_id.is_none()
                        })
                        .ok_or_else(|| err("blocked", "规划缺少测试定义"))?
                        .body
                        .clone(),
                    version: 1,
                    supersedes: None,
                    requirement_id: req,
                    design_id: design,
                    origin: source_origin,
                    acceptance: vec![],
                    verifies: scope.acceptance.clone(),
                    artifact_id: test_artifact_id,
                });
                let fm = s.features.iter_mut().find(|x| x.id == f.id).unwrap();
                fm.status = Status::Developing;
                fm.revision += 1;
                json!({"id":tid})
            }
            "task.replan" => {
                let task = s.task(target)?.clone();
                mutable_feature(s, &task.feature_id)?;
                require(
                    task.status != Status::Completed,
                    "已完成 Task 应通过后续 Task 承接变化",
                )?;
                require(s.planning_accepted(&task.feature_id), "当前规划尚未接受")?;
                require(
                    !s.attempts.iter().any(|a| {
                        a.task_id.as_deref() == Some(target)
                            && a.status == Status::Running
                            && !a.paused
                    }),
                    "请先暂停或取消活动 Attempt",
                )?;
                let snapshot = s
                    .snapshots
                    .iter()
                    .rev()
                    .find(|snap| {
                        snap.feature_id.as_deref() == Some(task.feature_id.as_str())
                            && snap.task_id.is_none()
                            && snap.kind == "initial"
                    })
                    .unwrap()
                    .clone();
                let requirement = snapshot
                    .documents
                    .iter()
                    .find(|id| {
                        s.documents
                            .iter()
                            .any(|d| &d.id == *id && d.kind == "requirement")
                    })
                    .cloned();
                let design = snapshot
                    .documents
                    .iter()
                    .find(|id| {
                        s.documents
                            .iter()
                            .any(|d| &d.id == *id && d.kind == "design")
                    })
                    .cloned();
                let acceptance = if let Some(v) = p.get("acceptance_keys") {
                    serde_json::from_value(v.clone())
                        .map_err(|_| err("invalid_params", "验收范围格式错误"))?
                } else {
                    requirement
                        .as_deref()
                        .map(|r| s.acceptance_keys(r))
                        .transpose()?
                        .unwrap_or_default()
                };
                let scope = TaskScope {
                    requirement,
                    design,
                    acceptance,
                };
                s.validate_scope(&task.feature_id, &scope)?;
                let old = s
                    .documents
                    .iter()
                    .rev()
                    .find(|d| d.task_id.as_deref() == Some(target) && d.kind == "test")
                    .cloned();
                let test = snapshot
                    .documents
                    .iter()
                    .filter_map(|id| s.documents.iter().find(|d| &d.id == id && d.kind == "test"))
                    .next()
                    .ok_or_else(|| err("blocked", "规划缺少测试"))?
                    .clone();
                let artifact =
                    self.artifacts
                        .seal(&task.feature_id, Some(target), "documents", &test.body)?;
                let artifact_id = Some(artifact.id.clone());
                s.artifacts.push(artifact);
                s.documents.push(Document {
                    test_recipe: p
                        .get("test_recipe")
                        .filter(|v| !v.is_null())
                        .map(|v| {
                            serde_json::from_value::<TestRecipe>(v.clone())
                                .map_err(|_| err("invalid_params", "无效测试配方"))
                        })
                        .transpose()?
                        .or_else(|| old.as_ref().and_then(|d| d.test_recipe.clone()))
                        .or_else(|| test.test_recipe.clone()),
                    project_id: None,
                    id: id(),
                    feature_id: Some(task.feature_id.clone()),
                    task_id: Some(target.into()),
                    kind: "test".into(),
                    title: test.title,
                    body: test.body,
                    version: old.as_ref().map_or(1, |d| d.version + 1),
                    supersedes: old.map(|d| d.id),
                    requirement_id: scope.requirement.clone(),
                    design_id: scope.design.clone(),
                    origin: Origin::Human,
                    acceptance: vec![],
                    verifies: scope.acceptance.clone(),
                    artifact_id,
                });
                let affected: Vec<_> = s
                    .attempts
                    .iter()
                    .filter(|a| {
                        a.task_id.as_deref() == Some(target)
                            && [Status::Running, Status::Prepared, Status::WaitingReview]
                                .contains(&a.status)
                    })
                    .map(|a| a.id.clone())
                    .collect();
                for a in s.attempts.iter_mut().filter(|a| affected.contains(&a.id)) {
                    a.status = Status::Interrupted;
                    a.paused = true;
                    a.revision += 1;
                }
                for r in s.runs.iter_mut().filter(|r| {
                    r.attempt_id
                        .as_ref()
                        .is_some_and(|id| affected.contains(id))
                }) {
                    if r.status == Status::Running {
                        r.cancel_requested = true;
                    } else if r.status == Status::Queued {
                        r.status = Status::Cancelled;
                        r.finished = Some(now());
                    }
                }
                for d in s.deliveries.iter_mut().filter(|d| {
                    d.feature_id == task.feature_id
                        && (d.task_id.as_deref() == Some(target)
                            || (d.task_id.is_none()
                                && ["closure", "merge_candidate"].contains(&d.kind.as_str())))
                }) {
                    d.stale = true;
                }
                let task = s.tasks.iter_mut().find(|t| t.id == target).unwrap();
                task.scope = scope;
                task.inherited_snapshot = snapshot.id;
                task.status = Status::Ready;
                task.revision += 1;
                let fid = task.feature_id.clone();
                self.invalidate_merges(s, &fid, Some(target));
                self.invalidate_feature_merges(s, &fid);
                json!({"id":target})
            }
            "attempt.start" => {
                let (fid, tid, phase, mut b, worktree) =
                    if let Some(t) = s.tasks.iter().find(|t| t.id == target).cloned() {
                        mutable_feature(s, &t.feature_id)?;
                        require(t.status != Status::Completed, "Task 已完成")?;
                        let current = document_ids(s, &t.feature_id, Some(&t.id));
                        require(
                            t.scope
                                .requirement
                                .as_ref()
                                .is_some_and(|id| current.contains(id))
                                && t.scope
                                    .design
                                    .as_ref()
                                    .is_some_and(|id| current.contains(id)),
                            "Task 范围已过期，请采用当前规划范围后新建 Attempt",
                        )?;
                        require(
                            s.planning_accepted(&t.feature_id),
                            "当前输入版本规划尚未接受",
                        )?;
                        for dep in &t.dependencies {
                            require(
                                s.task(dep)?.status == Status::Completed,
                                "Task 依赖尚未完成",
                            )?;
                        }
                        (
                            t.feature_id,
                            Some(t.id),
                            "implementation".to_string(),
                            t.binding,
                            t.worktree,
                        )
                    } else {
                        let f = s.feature(target)?.clone();
                        mutable_feature(s, &f.id)?;
                        let phase = opt(p, "phase").unwrap_or_else(|| {
                            if [Status::Draft, Status::Planning].contains(&f.status) {
                                "planning".into()
                            } else {
                                "closure".into()
                            }
                        });
                        require(
                            [
                                "planning",
                                "requirement_definition",
                                "solution_planning",
                                "closure",
                            ]
                            .contains(&phase.as_str()),
                            "未知 Feature 阶段",
                        )?;
                        if phase == "planning" {
                            require(
                                !s.planning_accepted(&f.id),
                                "当前规划已接受；变更输入后才能重新规划",
                            )?;
                        }
                        if phase == "closure" {
                            require(s.planning_accepted(&f.id), "当前输入规划尚未接受")?;
                            let ts = s.required_tasks(&f.id);
                            require(
                                !ts.is_empty() && ts.iter().all(|t| t.status == Status::Completed),
                                "全部必需 Task 完成后才能收尾",
                            )?;
                        }
                        if phase == "requirement_definition" {
                            require(
                                document_ids(s, &f.id, None).iter().any(|id| {
                                    s.documents
                                        .iter()
                                        .any(|d| &d.id == id && d.kind == "requirement")
                                }),
                                "请先采纳需求",
                            )?;
                        }
                        if phase == "solution_planning" {
                            require(
                                s.deliveries.iter().any(|d| {
                                    d.feature_id == f.id && d.kind == "requirement" && s.accepted(d)
                                }),
                                "需求交付评审通过后才能执行方案规划",
                            )?;
                        }
                        (
                            f.id.clone(),
                            None,
                            phase,
                            f.binding,
                            s.repository_worktrees
                                .iter()
                                .find(|w| w.owner_id == f.id)
                                .map(|w| w.path.clone())
                                .unwrap_or_default(),
                        )
                    };
                require(
                    !s.attempts.iter().any(|a| {
                        a.feature_id == fid
                            && a.task_id == tid
                            && [Status::Running, Status::Prepared, Status::WaitingReview]
                                .contains(&a.status)
                            && s.attempt_inputs_current(a)
                            && a.phase == phase
                    }),
                    "已有活动 Attempt；请等待评审或取消",
                )?;
                if s.run_cancellation_pending(&worktree) {
                    return Err(err("run_cancellation_pending", "等待旧 Run 取消确认后再启动"));
                }
                let live = s.projects.iter().any(|p| {
                    p.id == s.feature(&fid).unwrap().project_id && p.execution_mode == "live"
                });
                require(live, "历史项目不能启动工程执行")?;
                let live_sessions: std::collections::BTreeMap<String, Id> =
                    serde_json::from_value(p["live_sessions"].clone())
                        .map_err(|_| err("blocked", "Attempt 缺少宿主解析的阶段 Session"))?;
                if live && ["planning", "solution_planning"].contains(&phase.as_str()) {
                    let docs = document_ids(s, &fid, None);
                    require(
                        ["requirement", "design", "test"].iter().all(|kind| {
                            docs.iter().any(|id| {
                                s.documents.iter().any(|d| &d.id == id && d.kind == *kind)
                            })
                        }),
                        "真实规划需要先采纳 Requirement、Design、TestPlan",
                    )?;
                    require(
                        s.proposals.iter().any(|p| {
                            p.target.as_deref() == Some(&fid)
                                && p.status == ProposalStatus::Accepted
                                && matches!(p.content, ProposalContent::TaskPlan { .. })
                                && p.adopted_objects.iter().all(|id| docs.contains(id))
                        }),
                        "真实规划需要已采纳的结构化 TaskPlan",
                    )?;
                    require(
                        docs.iter()
                            .filter_map(|id| {
                                s.documents.iter().find(|d| &d.id == id && d.kind == "test")
                            })
                            .all(|d| d.test_recipe.is_some()),
                        "真实 TestPlan 必须有可执行测试配方",
                    )?;
                    require(s.proposals.iter().filter(|p|p.target.as_deref()==Some(&fid)&&p.status==ProposalStatus::Accepted&&p.adopted_objects.iter().all(|id|docs.contains(id))).any(|p|matches!(&p.content,ProposalContent::TaskPlan{tasks,..} if tasks.iter().all(|t|t.test_recipe.is_some()))),"真实 TaskPlan 的每个条目都必须有独立测试配方")?;
                }
                if live {
                    self.validate_live_sessions(
                        s,
                        tid.as_deref().unwrap_or(&fid),
                        &phase,
                        &live_sessions,
                        p["workflow_step_instance_id"].as_str(),
                    )?;
                    let primary = match phase.as_str() {
                        "planning" => "design",
                        "requirement_definition" => "requirement_validate",
                        "solution_planning" => "plan_validate",
                        "implementation" => "implement",
                        _ => "docs",
                    };
                    b = self.live_binding(s, &live_sessions[primary], &b.standard)?;
                }
                s.validate_binding(&b)?;
                let runtime = s.harnesses.iter().find(|h| h.id == b.harness).unwrap();
                require(
                    runtime.origin == Origin::External,
                    "项目执行模式与 Harness 来源不符",
                )?;
                self.invalidate_merges(s, &fid, tid.as_deref());
                for d in s.deliveries.iter_mut().filter(|d| {
                    d.feature_id == fid
                        && d.task_id == tid
                        && (d.kind == phase || d.merge_id.is_some())
                }) {
                    d.stale = true;
                }
                let aid = id();
                let input = s.feature(&fid)?.input_version;
                let documents = document_ids(s, &fid, tid.as_deref());
                s.attempts.push(Attempt {
                    live_sessions: live_sessions.clone(),
                    source_base: None,
                    id: aid.clone(),
                    feature_id: fid.clone(),
                    task_id: tid.clone(),
                    phase: phase.clone(),
                    baseline: None,
                    git_context: Some(git_context(s, &fid, tid.as_deref())?),
                    status: Status::Running,
                    input_version: input,
                    documents,
                    outputs: vec![],
                    scope: tid.as_ref().map(|id| s.task(id).unwrap().scope.clone()),
                    binding: b.clone(),
                    context_content: s
                        .contexts
                        .iter()
                        .find(|c| c.owner_id == tid.as_deref().unwrap_or(&fid))
                        .map(|c| c.content.clone())
                        .unwrap_or_default(),
                    context_revision: s
                        .contexts
                        .iter()
                        .find(|c| c.owner_id == tid.as_deref().unwrap_or(&fid))
                        .map(|c| c.revision)
                        .unwrap_or_default(),
                    paused: false,
                    revision: 1,
                });
                let baseline = s
                    .heads
                    .iter()
                    .find(|h| h.id == tid.as_deref().unwrap_or(&fid))
                    .map(|h| h.revision.clone())
                    .ok_or_else(|| err("blocked", "代码头尚未核对"))?;
                let attempt = s.attempts.iter_mut().find(|a| a.id == aid).unwrap();
                attempt.source_base = live.then(|| baseline.clone());
                attempt.baseline = if live && phase == "implementation" {
                    None
                } else {
                    Some(baseline)
                };
                let builtin_nodes: Vec<(&str, Vec<&str>, bool)> = match phase.as_str() {
                    "requirement_definition" => vec![("requirement_validate", vec![], false)],
                    "solution_planning" => vec![
                        ("plan_validate", vec![], false),
                        ("test_plan_validate", vec!["plan_validate"], false),
                    ],
                    "planning" => vec![
                        ("research", vec![], false),
                        ("requirement", vec!["research"], true),
                        ("design", vec!["requirement"], true),
                        ("task_plan", vec!["design"], false),
                        ("test_plan", vec!["design"], false),
                    ],
                    "implementation" => vec![
                        ("test_author", vec![], true),
                        ("red", vec!["test_author"], false),
                        ("implement", vec!["red"], true),
                        ("green", vec!["implement"], false),
                        ("docs", vec!["implement"], false),
                        ("verify", vec!["green", "docs"], false),
                    ],
                    _ => vec![
                        ("test_author", vec![], true),
                        ("feature_tests", vec!["test_author"], false),
                        ("docs", vec!["feature_tests"], false),
                        ("verify", vec!["feature_tests", "docs"], false),
                    ],
                };
                let nodes: Vec<(String, Vec<String>, bool)> = if let Some(recipe) = self
                    .workflow_recipe(
                        s,
                        tid.as_deref().unwrap_or(&fid),
                        &phase,
                        p["workflow_step_instance_id"].as_str(),
                    ) {
                    recipe
                        .into_iter()
                        .map(|n| (n.key, n.dependencies, n.writes))
                        .collect()
                } else {
                    builtin_nodes
                        .into_iter()
                        .map(|(key, deps, writes)| {
                            (
                                key.into(),
                                deps.into_iter().map(str::to_owned).collect(),
                                writes,
                            )
                        })
                        .collect()
                };
                valid_dag(
                    &nodes
                        .iter()
                        .map(|(n, d, _)| (n.to_string(), d.iter().map(|s| s.to_string()).collect()))
                        .collect::<Vec<_>>(),
                )?;
                for (node, deps, writes) in nodes {
                    self.add_live_run(s, &aid, &node, deps, writes && phase != "planning")?;
                }
                if let Some(tid) = tid {
                    let t = s.tasks.iter_mut().find(|x| x.id == tid).unwrap();
                    t.status = Status::Running;
                    if live {
                        t.binding = b.clone();
                    }
                    t.revision += 1;
                } else {
                    let f = s.features.iter_mut().find(|x| x.id == fid).unwrap();
                    if live {
                        f.binding = b.clone();
                    }
                    f.status = if ["planning", "requirement_definition", "solution_planning"]
                        .contains(&phase.as_str())
                    {
                        Status::Planning
                    } else {
                        Status::Validating
                    };
                    f.revision += 1;
                }
                json!({"id":aid,"operation_id":aid})
            }
            "attempt.pause" => {
                let a = s
                    .attempts
                    .iter_mut()
                    .find(|a| a.id == target)
                    .ok_or_else(|| err("not_found", "Attempt 不存在"))?;
                require(
                    [Status::Running, Status::Interrupted].contains(&a.status),
                    "当前 Attempt 不可暂停",
                )?;
                a.paused = p.get("paused").and_then(Value::as_bool).unwrap_or(true);
                a.revision += 1;
                json!({"id":target})
            }
            "attempt.cancel" => {
                let a = s
                    .attempts
                    .iter_mut()
                    .find(|a| a.id == target)
                    .ok_or_else(|| err("not_found", "Attempt 不存在"))?;
                require(
                    [
                        Status::Running,
                        Status::WaitingReview,
                        Status::Interrupted,
                        Status::Failed,
                    ]
                    .contains(&a.status),
                    "Attempt 不可取消",
                )?;
                a.status = Status::Cancelled;
                a.revision += 1;
                for r in s
                    .runs
                    .iter_mut()
                    .filter(|r| r.attempt_id.as_deref() == Some(target))
                {
                    if r.status == Status::Running {
                        r.cancel_requested = true;
                    } else if r.status == Status::Queued {
                        r.status = Status::Cancelled;
                        r.finished = Some(now());
                    }
                }
                json!({"id":target})
            }
            "run.cancel" => {
                let r = s
                    .runs
                    .iter_mut()
                    .find(|r| r.id == target)
                    .ok_or_else(|| err("not_found", "Run 不存在"))?;
                require(
                    [Status::Queued, Status::Running].contains(&r.status),
                    "Run 已结束",
                )?;
                if r.status == Status::Queued {
                    r.status = Status::Cancelled;
                    r.finished = Some(now());
                } else {
                    r.cancel_requested = true;
                }
                json!({"id":target})
            }
            "run.retry" => {
                let old = s
                    .runs
                    .iter()
                    .find(|r| r.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "Run 不存在"))?;
                require(
                    !s.effects.iter().any(|e| {
                        e.run_id == old.id && e.status == EffectStatus::NeedsReconciliation
                    }),
                    "先核对旧 Run 外部结果",
                )?;
                require(
                    old.binding_verified,
                    "历史 Run 的完整执行锁未记录，不能推定身份配置；请创建新 Attempt/Merge/Review",
                )?;
                require(
                    [Status::Failed, Status::Cancelled, Status::Interrupted].contains(&old.status),
                    "仅失败/取消/中断 Run 可重试",
                )?;
                require(
                    s.latest_runs(old.attempt_id.as_deref(), old.merge_id.as_deref())
                        .iter()
                        .any(|r| r.node == old.node && r.id == old.id),
                    "此 Run 已被重试替代",
                )?;
                require(
                    !s.deliveries.iter().any(|d| d.evidence.contains(&old.id)),
                    "执行已冻结进 Delivery，修订必须新 Attempt/Merge",
                )?;
                let mut affected = vec![old.node.clone()];
                loop {
                    let next = s
                        .latest_runs(old.attempt_id.as_deref(), old.merge_id.as_deref())
                        .into_iter()
                        .filter(|r| {
                            !affected.contains(&r.node)
                                && r.dependencies.iter().any(|d| affected.contains(d))
                        })
                        .map(|r| r.node.clone())
                        .collect::<Vec<_>>();
                    if next.is_empty() {
                        break;
                    }
                    affected.extend(next);
                }
                let descendants = s
                    .latest_runs(old.attempt_id.as_deref(), old.merge_id.as_deref())
                    .into_iter()
                    .filter(|r| r.node != old.node && affected.contains(&r.node))
                    .cloned()
                    .collect::<Vec<_>>();
                require(
                    !descendants.iter().any(|r| r.status == Status::Running),
                    "等待下游执行结束或取消确认后再重试",
                )?;
                self.retry_live_runs(s, &old, &descendants)?
            }
            "delivery.submit" => {
                let a = s.attempt(target)?.clone();
                require(
                    [Status::WaitingReview, Status::Failed].contains(&a.status),
                    "执行尚未结束",
                )?;
                require(
                    s.attempt_inputs_current(&a),
                    "Attempt 输入已失效，需新 Attempt",
                )?;
                let kind = if a.phase == "requirement_definition" {
                    "requirement".into()
                } else if a.phase == "solution_planning" {
                    "plan".into()
                } else if a.phase == "planning" {
                    opt(p, "kind").unwrap_or_else(|| "requirement".into())
                } else {
                    a.phase.clone()
                };
                require(
                    (a.phase == "planning" && ["requirement", "plan"].contains(&kind.as_str()))
                        || (a.phase == "requirement_definition" && kind == "requirement")
                        || (a.phase == "solution_planning" && kind == "plan")
                        || kind == a.phase,
                    "无效交付边界",
                )?;
                require(
                    !s.deliveries
                        .iter()
                        .any(|d| d.attempt_id.as_deref() == Some(target) && d.kind == kind),
                    "该边界已提交，修订需新 Attempt",
                )?;
                let runs: Vec<_> = effective_runs(s, &a)
                    .into_iter()
                    .filter(|r| {
                        kind != "requirement"
                            || a.phase == "requirement_definition"
                            || ["research", "requirement"].contains(&r.node.as_str())
                    })
                    .collect();
                if kind == "plan" {
                    require(
                        s.deliveries.iter().any(|d| {
                            (d.attempt_id.as_deref() == Some(&a.id)
                                || (a.phase == "solution_planning" && d.feature_id == a.feature_id))
                                && d.kind == "requirement"
                                && s.accepted(d)
                        }),
                        "需求交付通过 Review 后才能提交计划",
                    )?;
                }
                require(
                    runs.iter().all(|r| {
                        r.status == Status::Succeeded
                            || r.result
                                .as_ref()
                                .is_some_and(|v| v["failure_type"] == "assertion")
                    }),
                    "存在未完成或技术失败的 Run",
                )?;
                let evidence = runs.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
                let docs = document_ids(s, &a.feature_id, a.task_id.as_deref())
                    .into_iter()
                    .filter(|id| {
                        kind != "requirement"
                            || s.documents
                                .iter()
                                .any(|d| &d.id == id && d.kind == "requirement")
                    })
                    .collect();
                let did = self.make_delivery(
                    s,
                    &a.feature_id,
                    a.task_id.as_deref(),
                    Some(&a.id),
                    None,
                    &kind,
                    a.input_version,
                    docs,
                    evidence,
                    Some(if a.source_base.is_some() {
                        if ["implementation", "closure"].contains(&a.phase.as_str()) {
                            effective_runs(s, &a)
                                .iter()
                                .find(|r| {
                                    r.node
                                        == if a.phase == "closure" {
                                            "test_author"
                                        } else {
                                            "implement"
                                        }
                                })
                                .and_then(|r| r.result.as_ref())
                                .and_then(|v| v["code_revision"].as_str())
                                .ok_or_else(|| err("blocked", "实现缺少实际代码封存回执"))?
                                .to_string()
                        } else {
                            a.source_base.clone().unwrap()
                        }
                    } else {
                        self.git.execution_revision(&a, "delivery")?
                    }),
                )?;
                if let Some(tid) = a.task_id {
                    let t = s.tasks.iter_mut().find(|t| t.id == tid).unwrap();
                    t.status = Status::WaitingReview;
                    t.revision += 1;
                }
                json!({"id":did})
            }
            "review.submit" => {
                let d = s.delivery(target)?.clone();
                self.review(
                    s,
                    &d,
                    serde_json::from_value(p.get("decision").cloned().unwrap_or(json!("pass")))
                        .map_err(|_| err("invalid_params", "未知评审决定"))?,
                    &text(p, "reason")?,
                    "human:local",
                    None,
                )?;
                json!({"id":target})
            }
            "review.request" => {
                let d = s.delivery(target)?.clone();
                require(!d.stale, "交付已失效")?;
                require(
                    !s.runs.iter().any(|r| {
                        r.node == format!("review:{}", d.id)
                            && [Status::Queued, Status::Running].contains(&r.status)
                    }),
                    "评审已在执行",
                )?;
                require(d.origin == Origin::External, "历史交付不能创建原生评审")?;

                let sid = text(p, "review_session")?;
                let session = s
                    .sessions
                    .iter()
                    .find(|v| v.id == sid)
                    .ok_or_else(|| err("blocked", "Reviewer Session 缺失"))?;
                require(
                    session.owner_id == d.task_id.as_deref().unwrap_or(&d.feature_id)
                        && session.runtime_binding.as_ref().is_some_and(|b| {
                            b.work_type == WorkType::Review && b.tool_policy == ToolPolicy::ReadOnly
                        })
                        && s.definitions
                            .iter()
                            .find(|v| v.id == session.agent)
                            .and_then(|v| v.agent_profile.as_ref())
                            .is_some_and(|p| p.role == AgentRole::Reviewer),
                    "Reviewer 范围/角色/权限不符",
                )?;
                let source = if let Some(mid) = &d.merge_id {
                    s.delivery(&s.merge(mid)?.source_delivery)?
                } else {
                    &d
                };
                let standard = s
                    .attempt(source.attempt_id.as_deref().unwrap())?
                    .binding
                    .standard
                    .clone();
                let binding = self.live_binding(s, &sid, &standard)?;
                let rid = id();
                s.runs.push(Run {
                    id: rid.clone(),
                    attempt_id: d.attempt_id.clone(),
                    merge_id: d.merge_id.clone(),
                    node: format!("review:{}", d.id),
                    dependencies: vec![],
                    status: Status::Queued,
                    retry_of: None,
                    session_id: sid,
                    binding,
                    binding_verified: true,
                    writes: false,
                    worktree: "review-readonly".into(),
                    outcome: "external".into(),
                    cancel_requested: false,
                    started: None,
                    finished: None,
                    result: None,
                    result_artifact: None,
                    origin: Origin::External,
                });
                return Ok(json!({"id":rid,"operation_id":rid}));
            }
            "override.accept" => {
                let d = s.delivery(target)?.clone();
                mutable_feature(s, &d.feature_id)?;
                if let Some(tid) = &d.task_id {
                    require(
                        s.task(tid)?.status != Status::Completed,
                        "已完成 Task 已冻结",
                    )?;
                }
                require(
                    !d.stale && s.delivery_inputs_current(&d),
                    "失效证据不能 Override",
                )?;
                self.check_merge_target(s, &d)?;
                s.validate_evidence(&d)?;
                if let Some(aid) = &d.attempt_id {
                    require(
                        s.attempt(aid)?.status != Status::Cancelled,
                        "已取消 Attempt 不可接纳",
                    )?;
                }
                let items: Vec<String> =
                    serde_json::from_value(p.get("items").cloned().unwrap_or(json!(["review"])))
                        .map_err(|_| err("invalid_params", "items 应为条目列表"))?;
                require(!items.is_empty(), "必须选择豁免条目")?;
                if items.iter().any(|item| item == "review")
                    && (s.feature_configuration(&d.feature_id).is_some_and(|lock| lock.compiler_version >= 3)
                        || d.evidence.iter().any(|id|s.runs.iter().find(|r|&r.id==id)
                            .and_then(|r|r.result.as_ref()).is_some_and(|result|result["approval_required"]==true)))
                {
                    require(s.runs.iter().rev().find(|r|r.node==format!("review:{}",d.id))
                        .is_some_and(|r|r.status==Status::Succeeded && r.result.as_ref().is_some_and(|result|
                            result["approval_required"]==true && result["model_approve"].is_boolean())),
                        "请等待模型完成内容评审并给出 Approve 结论后，再执行人类覆盖")?;
                }
                for item in &items {
                    require(
                        item == "review"
                            || d.evidence.iter().any(|rid| {
                                item == &format!("test:{rid}")
                                    && s.runs.iter().find(|r| &r.id == rid).is_some_and(|r| {
                                        r.result
                                            .as_ref()
                                            .is_some_and(|v| v["failure_type"] == "assertion")
                                    })
                            }),
                        "仅可豁免 Review 或断言质量条件",
                    )?;
                }
                let oid = id();
                s.overrides.push(Override {
                    id: oid.clone(),
                    delivery_id: d.id.clone(),
                    actor: "human:local".into(),
                    reason: text(p, "reason")?,
                    items,
                });
                self.reconcile(s, &d)?;
                json!({"id":oid})
            }
            "merge.request" => {
                return self.prepare_live_merge(s, c);
            }
            "merge.apply" => {
                let m = s.merge(target)?.clone();
                require(m.status != Status::Completed, "Merge 已完成")?;
                require(
                    m.input_version == s.feature(&m.feature_id)?.input_version,
                    "需求已变化，请重新构造候选",
                )?;
                let d = s
                    .deliveries
                    .iter()
                    .rev()
                    .find(|d| d.merge_id.as_deref() == Some(target))
                    .ok_or_else(|| err("blocked", "缺少候选交付"))?
                    .clone();
                self.check_merge_target(s, &d)?;
                s.validate_evidence(&d)?;
                require(s.accepted(&d), "候选验证及 Review 尚未接受")?;
                require(
                    !s.advances.iter().any(|advance| {
                        advance.id != m.id
                            && advance.target == m.target
                            && ![AdvanceStatus::Applied, AdvanceStatus::NotApplied]
                                .contains(&advance.status)
                    }),
                    "目标存在待确认的推进操作",
                )?;
                if let Some(intent) = s.advances.iter().find(|intent| intent.id == m.id) {
                    require(
                        intent.candidate == m.candidate
                            && intent.expected_target == m.expected_target
                            && intent.delivery == d.id,
                        "推进请求与既有意图不一致",
                    )?;
                    if intent.status == AdvanceStatus::NotApplied {
                        let current = s.advances.iter_mut().find(|a| a.id == m.id).unwrap();
                        current.status = AdvanceStatus::Prepared;
                        current.receipt = None;
                        current.reason = "explicit_retry_after_confirmed_not_applied".into();
                        current.revision += 1;
                        let merge = s.merges.iter_mut().find(|merge| merge.id == m.id).unwrap();
                        merge.status = Status::Accepted;
                        merge.revision += 1;
                    }
                } else {
                    s.advances.push(MergeAdvance {
                        id: m.id.clone(),
                        context: s
                            .delivery(&m.source_delivery)?
                            .attempt_id
                            .as_deref()
                            .and_then(|id| s.attempt(id).ok())
                            .and_then(|a| a.git_context.clone()),
                        provider: self.git.identity_for(
                            s.delivery(&m.source_delivery)?
                                .attempt_id
                                .as_deref()
                                .and_then(|id| s.attempt(id).ok())
                                .and_then(|a| a.git_context.as_ref()),
                        ),
                        target: m.target.clone(),
                        expected_target: m.expected_target.clone(),
                        candidate: m.candidate.clone(),
                        delivery: d.id.clone(),
                        status: AdvanceStatus::Prepared,
                        receipt: None,
                        revision: 1,
                        reason: "推进意图已持久化".into(),
                    });
                }
                json!({"id":target,"operation_id":target,"accepted":true})
            }
            "merge.capture_snapshot" => {
                let m = s.merge(target)?.clone();
                require(
                    m.status == Status::Completed,
                    "只有远端已核验的接纳可保存快照",
                )?;
                let d = s
                    .deliveries
                    .iter()
                    .find(|d| d.merge_id.as_deref() == Some(target))
                    .ok_or_else(|| err("blocked", "候选交付缺失"))?
                    .clone();
                let source = s.delivery(&m.source_delivery)?;
                let binding = s
                    .attempt(source.attempt_id.as_deref().unwrap())?
                    .binding
                    .clone();
                let sid = self.snapshot(
                    s,
                    &m.feature_id,
                    m.task_id.as_deref(),
                    "remote_receipt",
                    binding,
                    Some(d.id),
                )?;
                json!({"id":sid})
            }
            "merge.reconcile" => {
                let merge = s.merge(target)?;
                require(merge.status != Status::Completed, "Merge 已完成")?;
                let intent = s
                    .advances
                    .iter_mut()
                    .find(|intent| intent.id == target)
                    .ok_or_else(|| err("not_found", "推进意图不存在"))?;
                require(
                    intent.status == AdvanceStatus::NeedsReconciliation,
                    "当前推进无需核对",
                )?;
                intent.reason = "reconcile_requested".into();
                intent.revision += 1;
                json!({"id":target,"operation_id":target})
            }
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
            "snapshot.save" => {
                if s.projects.iter().any(|project| project.id == target) {
                    self.snapshot_project(s, target, p)?
                } else {
                    let (fid, tid, b) = if let Some(t) = s.tasks.iter().find(|t| t.id == target) {
                        (t.feature_id.clone(), Some(t.id.clone()), t.binding.clone())
                    } else {
                        let f = s.feature(target)?;
                        (f.id.clone(), None, f.binding.clone())
                    };
                    mutable_feature(s, &fid)?;
                    if let Some(tid) = &tid {
                        require(
                            s.task(tid)?.status != Status::Completed,
                            "已完成 Task 已冻结",
                        )?;
                    }
                    let sid = self.snapshot(s, &fid, tid.as_deref(), "manual", b, None)?;
                    json!({"id":sid})
                }
            }
            _ => return Err(err("method_not_found", format!("未知命令 {method}"))),
        };
        event(s, target, method, &format!("command_id={}", c.command_id));
        Ok(result)
    }
    fn invalidate_merges(&self, s: &mut EngineeringState, fid: &str, tid: Option<&str>) {
        let ids = s
            .merges
            .iter_mut()
            .filter(|m| {
                m.feature_id == fid
                    && tid.is_none_or(|id| m.task_id.as_deref() == Some(id))
                    && m.status != Status::Completed
            })
            .map(|m| {
                m.status = Status::Interrupted;
                m.revision += 1;
                m.id.clone()
            })
            .collect::<Vec<_>>();
        for r in s
            .runs
            .iter_mut()
            .filter(|r| r.merge_id.as_ref().is_some_and(|id| ids.contains(id)))
        {
            if r.status == Status::Queued {
                r.status = Status::Cancelled;
                r.finished = Some(now());
            } else if r.status == Status::Running {
                r.cancel_requested = true;
            }
        }
    }
    fn invalidate_feature_merges(&self, s: &mut EngineeringState, fid: &str) {
        let ids: Vec<_> = s
            .merges
            .iter_mut()
            .filter(|m| m.feature_id == fid && m.task_id.is_none() && m.status != Status::Completed)
            .map(|m| {
                m.status = Status::Interrupted;
                m.revision += 1;
                m.id.clone()
            })
            .collect();
        for run in s
            .runs
            .iter_mut()
            .filter(|run| run.merge_id.as_ref().is_some_and(|id| ids.contains(id)))
        {
            if run.status == Status::Running {
                run.cancel_requested = true;
            } else if run.status == Status::Queued {
                run.status = Status::Cancelled;
                run.finished = Some(now());
            }
        }
    }

    fn make_delivery(
        &self,
        s: &mut EngineeringState,
        fid: &str,
        tid: Option<&str>,
        aid: Option<&str>,
        mid: Option<&str>,
        kind: &str,
        input: u64,
        documents: Vec<Id>,
        evidence: Vec<Id>,
        candidate: Option<String>,
    ) -> Result<String> {
        let did = id();
        require(
            !evidence.is_empty()
                && evidence.iter().all(|id| {
                    s.runs
                        .iter()
                        .any(|r| &r.id == id && r.origin == Origin::External)
                }),
            "交付需要原生执行证据",
        )?;
        let origin = Origin::External;
        let source_deliveries: Vec<Id> = if let Some(mid) = mid {
            vec![s.merge(mid)?.source_delivery.clone()]
        } else if kind == "closure" {
            s.required_tasks(fid)
                .into_iter()
                .filter(|t| t.status == Status::Completed)
                .filter_map(|t| {
                    s.merges
                        .iter()
                        .rev()
                        .find(|m| {
                            m.task_id.as_ref() == Some(&t.id) && m.status == Status::Completed
                        })
                        .map(|m| m.source_delivery.clone())
                })
                .collect()
        } else {
            vec![]
        };
        let source = serde_json::to_value(origin)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let evidence_records: Vec<_> = evidence
            .iter()
            .filter_map(|id| s.runs.iter().find(|r| &r.id == id))
            .collect();
        let body = format!(
            "# 执行与交付报告\n\nDelivery: {did}\n类型: {kind}\n输入版本: {input}\norigin={source}\n\n## 冻结输入与证据\n```json\n{}\n```\n",
            serde_json::to_string_pretty(
                &json!({"documents":documents,"evidence":evidence_records,"candidate":candidate,"source_deliveries":source_deliveries})
            )
            .unwrap()
        );
        let mut artifact = self.artifacts.seal(fid, tid, "deliveries", &body)?;
        artifact.origin = origin;
        let artifact_id = artifact.id.clone();
        let mut manifest=self.artifacts.seal(fid,tid,"manifests",&serde_json::to_string_pretty(&json!({"schema_version":1,"delivery_id":did,"report_artifact":artifact,"input_version":input,"documents":documents,"evidence":evidence,"source_deliveries":source_deliveries,"candidate":candidate,"origin":origin,"attempt":aid.and_then(|id|s.attempts.iter().find(|a|a.id==id))})).map_err(|e|err("serialization",e.to_string()))?)?;
        manifest.origin = origin;
        s.artifacts.push(manifest);
        s.artifacts.push(artifact);
        let delivery = Delivery {
            source_deliveries,
            id: did.clone(),
            feature_id: fid.into(),
            task_id: tid.map(str::to_owned),
            attempt_id: aid.map(str::to_owned),
            merge_id: mid.map(str::to_owned),
            kind: kind.into(),
            input_version: input,
            documents,
            evidence,
            artifact_id,
            candidate,
            stale: false,
            origin,
        };
        s.validate_evidence(&delivery)?;
        s.deliveries.push(delivery);
        event(s, &did, "delivery.created", kind);
        Ok(did)
    }
    pub(crate) fn check_merge_target(&self, s: &EngineeringState, d: &Delivery) -> Result<()> {
        if let Some(mid) = &d.merge_id {
            let m = s.merge(mid)?;
            require(
                s.heads
                    .iter()
                    .any(|h| h.id == m.target && h.revision == m.expected_target),
                "目标版本变化，必须重新构造候选、验证并 Review",
            )?;
        }
        Ok(())
    }
    fn review(
        &self,
        s: &mut EngineeringState,
        d: &Delivery,
        decision: Decision,
        reason: &str,
        actor: &str,
        run: Option<String>,
    ) -> Result<()> {
        mutable_feature(s, &d.feature_id)?;
        if let Some(tid) = &d.task_id {
            require(
                s.task(tid)?.status != Status::Completed,
                "已完成 Task 已冻结",
            )?;
        }
        require(
            !d.stale && s.delivery_inputs_current(&d),
            "交付已失效，需重新交付",
        )?;
        self.check_merge_target(s, d)?;
        s.validate_evidence(d)?;
        require(
            !s.reviews.iter().any(|r| {
                r.delivery_id == d.id
                    && [Decision::Pass, Decision::ChangesRequested].contains(&r.decision)
            }),
            "该交付已有最终评审决定；修订需新 Attempt，或显式质量 Override",
        )?;
        if let Some(aid) = &d.attempt_id {
            require(
                ![Status::Rejected, Status::Cancelled].contains(&s.attempt(aid)?.status),
                "已退回 Attempt 不可复活，需新 Attempt",
            )?;
        }
        if let Some(rid) = run.as_ref() {
            let reviewer = s
                .runs
                .iter()
                .find(|r| &r.id == rid)
                .ok_or_else(|| err("blocked", "Reviewer Run 缺失"))?;
            let author_delivery = if let Some(mid) = &d.merge_id {
                s.delivery(&s.merge(mid)?.source_delivery)?
            } else {
                d
            };
            let author = s.attempt(
                author_delivery
                    .attempt_id
                    .as_deref()
                    .ok_or_else(|| err("blocked", "源 Attempt 缺失"))?,
            )?;
            require(
                s.agent_identity(&reviewer.binding.agent)?
                    != s.agent_identity(&author.binding.agent)?
                    && reviewer.node == format!("review:{}", d.id)
                    && reviewer.binding.standard == author.binding.standard,
                "评审执行必须使用独立且匹配交付的身份锁",
            )?;
        }
        let mut checks = vec![
            Check {
                key: "version".into(),
                passed: !d.stale,
                evidence: format!("input_version={}", d.input_version),
            },
            Check {
                key: "evidence".into(),
                passed: d.evidence.iter().all(|id| {
                    s.runs
                        .iter()
                        .find(|r| &r.id == id)
                        .is_some_and(|r| r.status == Status::Succeeded)
                }),
                evidence: format!("{} 个执行记录；origin={:?}", d.evidence.len(), d.origin),
            },
            Check {
                key: "quality".into(),
                passed: decision == Decision::Pass,
                evidence: reason.into(),
            },
        ];
        let lock = if let Some(aid) = &d.attempt_id {
            s.attempt(aid)?.binding.clone()
        } else {
            let m = s.merge(d.merge_id.as_deref().unwrap())?;
            let source = s.delivery(&m.source_delivery)?;
            s.attempt(source.attempt_id.as_deref().unwrap())?
                .binding
                .clone()
        };
        let specification_checks =
            if let Some(specs) = self.delivery_specifications(s, d, run.as_deref())? {
                specs
            } else {
                let standard = s
                    .definitions
                    .iter()
                    .find(|x| x.id == lock.standard)
                    .ok_or_else(|| err("blocked", "Standard 锁缺失"))?;
                let purpose = if d.merge_id.is_some() {
                    "merge"
                } else if ["requirement", "plan"].contains(&d.kind.as_str()) {
                    "planning"
                } else {
                    "implementation"
                };
                let slot = standard
                    .slots
                    .iter()
                    .find(|x| x.purpose == purpose)
                    .ok_or_else(|| err("blocked", "Standard 缺少对应用途 slot"))?;
                let spec = s
                    .definitions
                    .iter()
                    .find(|x| x.id == slot.spec)
                    .ok_or_else(|| err("blocked", "Spec 锁缺失"))?;
                vec![(
                    spec.clone(),
                    format!("standard={} slot={purpose}", standard.id),
                )]
            };
        for (spec, source) in &specification_checks {
            // Content judgments use the configured model task and the exact Skill/Spec
            // bodies. Legacy keyword probes are not quality gates for the new report contract.
            if run.as_ref().and_then(|id|s.runs.iter().find(|r| &r.id==id))
                .and_then(|r|r.result.as_ref()).is_some_and(|result|result["approval_required"]==true)
            { continue; }
            for rule in &spec.rules {
                let documents = d
                    .documents
                    .iter()
                    .filter_map(|id| s.documents.iter().find(|x| &x.id == id))
                    .collect::<Vec<_>>();
                let passed = match rule.as_str() {
                    "traceability" => {
                        documents.iter().any(|x| x.kind == "requirement")
                            && documents
                                .iter()
                                .filter(|x| x.kind == "design")
                                .all(|x| x.requirement_id.is_some())
                    }
                    "failure_paths" => documents
                        .iter()
                        .any(|x| x.body.contains("错误") || x.body.contains("失败")),
                    "sections" => documents.iter().any(|x| x.body.contains("##")),
                    _ => false,
                };
                checks.push(Check {
                    key: format!("spec:{}:v{}:{rule}", spec.name, spec.version),
                    passed,
                    evidence: format!("{source} spec={}", spec.id),
                });
            }
        }
        if let Some(run) = run.as_ref().and_then(|id| {
            s.runs
                .iter()
                .find(|r| &r.id == id && r.origin == Origin::External)
        }) {
            let model_checks: Vec<Check> = serde_json::from_value(
                run.result
                    .as_ref()
                    .and_then(|v| v.get("checks"))
                    .cloned()
                    .ok_or_else(|| err("adapter_contract", "真实评审缺少逐项检查"))?,
            )
            .map_err(|_| err("adapter_contract", "真实评审检查结构错误"))?;
            require(
                !model_checks.is_empty()
                    && model_checks
                        .iter()
                        .all(|c| !c.key.is_empty() && !c.evidence.is_empty()),
                "真实评审检查不完整",
            )?;
            checks.extend(model_checks);
        }
        let rid = id();
        let mut report=self.artifacts.seal(&d.feature_id,d.task_id.as_deref(),"reviews",&format!("# Review 报告\n\nDelivery: {}\nactor: {actor}\n决定: {decision:?}\n理由: {reason}\n证据来源: origin={}（评审决定不改变证据来源）\n\n{}",d.id,serde_json::to_value(d.origin).unwrap().as_str().unwrap(),serde_json::to_string_pretty(&checks).unwrap()))?;
        report.origin = if run.is_none() {
            Origin::Human
        } else {
            d.origin
        };
        s.artifacts.push(report);
        s.reviews.push(Review {
            id: rid,
            delivery_id: d.id.clone(),
            decision,
            checks,
            actor: actor.into(),
            run_id: run.clone(),
            reason: reason.into(),
            assignment: Some(ReviewAssignment {
                delivery: d.id.clone(),
                spec: specification_checks
                    .first()
                    .ok_or_else(|| err("blocked", "评审缺少规范"))?
                    .0
                    .id
                    .clone(),
                spec_refs: specification_checks
                    .iter()
                    .map(|(spec, _)| spec.id.clone())
                    .collect(),
                reviewer: if let Some(rid) = run.as_ref() {
                    let r = s.runs.iter().find(|r| &r.id == rid).unwrap();
                    Reviewer::Agent {
                        agent: r.binding.agent.clone(),
                        run: rid.clone(),
                        session: r.session_id.clone(),
                    }
                } else {
                    Reviewer::Human {
                        actor: actor.into(),
                    }
                },
            }),
        });
        if decision == Decision::ChangesRequested {
            if let Some(aid) = &d.attempt_id {
                let a = s.attempts.iter_mut().find(|x| &x.id == aid).unwrap();
                a.status = Status::Rejected;
                a.revision += 1;
            }
        }
        if decision == Decision::ChangesRequested {
            if let Some(mid) = &d.merge_id {
                let m = s.merges.iter_mut().find(|m| &m.id == mid).unwrap();
                m.status = Status::Rejected;
                m.revision += 1;
            }
        }
        self.reconcile(s, d)
    }
    pub(crate) fn reconcile(&self, s: &mut EngineeringState, d: &Delivery) -> Result<()> {
        if !s.accepted(d) {
            return Ok(());
        }
        if let Some(mid) = &d.merge_id {
            let m = s.merges.iter_mut().find(|x| &x.id == mid).unwrap();
            m.status = Status::Accepted;
            m.revision += 1;
            return Ok(());
        }
        let a = s.attempt(d.attempt_id.as_deref().unwrap())?.clone();
        if a.status == Status::Accepted {
            return Ok(());
        }
        let required = if a.phase == "requirement_definition" {
            vec!["requirement"]
        } else if a.phase == "solution_planning" {
            vec!["plan"]
        } else if a.phase == "planning" {
            vec!["requirement", "plan"]
        } else {
            vec![a.phase.as_str()]
        };
        if !required.iter().all(|kind| {
            s.deliveries
                .iter()
                .any(|x| x.attempt_id.as_deref() == Some(&a.id) && x.kind == *kind && s.accepted(x))
        }) {
            if a.phase == "planning" && d.kind == "requirement" {
                let am = s.attempts.iter_mut().find(|x| x.id == a.id).unwrap();
                am.status = Status::Running;
                am.revision += 1;
            }
            return Ok(());
        }
        let am = s.attempts.iter_mut().find(|x| x.id == a.id).unwrap();
        am.status = Status::Accepted;
        am.revision += 1;
        self.snapshot(
            s,
            &a.feature_id,
            a.task_id.as_deref(),
            if ["planning", "solution_planning"].contains(&a.phase.as_str()) {
                "initial"
            } else {
                "final"
            },
            a.binding,
            Some(d.id.clone()),
        )?;
        if let Some(tid) = a.task_id {
            let t = s.tasks.iter_mut().find(|x| x.id == tid).unwrap();
            t.status = Status::Accepted;
            t.revision += 1;
        } else {
            let f = s
                .features
                .iter_mut()
                .find(|x| x.id == a.feature_id)
                .unwrap();
            f.status = if ["planning", "solution_planning"].contains(&a.phase.as_str()) {
                Status::Ready
            } else if a.phase == "requirement_definition" {
                Status::Planning
            } else {
                Status::Accepted
            };
            f.revision += 1;
        }
        Ok(())
    }
    fn snapshot(
        &self,
        s: &mut EngineeringState,
        fid: &str,
        tid: Option<&str>,
        kind: &str,
        b: BindingLock,
        delivery: Option<Id>,
    ) -> Result<String> {
        let sid = id();
        let owner = tid.unwrap_or(fid);
        let context = s
            .contexts
            .iter()
            .find(|c| c.owner_id == owner)
            .map(|c| c.content.clone())
            .unwrap_or_default();
        let content = delivery
            .as_ref()
            .and_then(|id| s.deliveries.iter().find(|d| &d.id == id))
            .and_then(|d| d.attempt_id.as_deref())
            .and_then(|id| s.attempt(id).ok())
            .map(|a| a.context_content.clone())
            .unwrap_or(context);
        let inherited = tid
            .and_then(|tid| s.tasks.iter().find(|t| t.id == tid))
            .map(|t| t.inherited_snapshot.clone());
        let capture_attempt = delivery
            .as_ref()
            .and_then(|id| s.deliveries.iter().find(|d| &d.id == id))
            .and_then(|d| d.attempt_id.clone());
        let capture_merge = delivery
            .as_ref()
            .and_then(|id| s.deliveries.iter().find(|d| &d.id == id))
            .and_then(|d| d.merge_id.clone());
        let captured_runs: Vec<_> = s
            .runs
            .iter()
            .filter(|r| {
                (capture_attempt.is_some() && r.attempt_id == capture_attempt)
                    || (capture_merge.is_some() && r.merge_id == capture_merge)
            })
            .collect();
        let capture = SnapshotCapture {
            attempt: capture_attempt.clone(),
            runs: captured_runs.iter().map(|r| r.id.clone()).collect(),
            sessions: captured_runs
                .iter()
                .map(|r| r.session_id.clone())
                .chain(
                    s.sessions
                        .iter()
                        .filter(|session| session.owner_id == owner)
                        .map(|session| session.id.clone()),
                )
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            resources: s
                .resources
                .iter()
                .filter(|res| captured_runs.iter().any(|r| r.id == res.run_id))
                .map(|r| r.id.clone())
                .collect(),
            captured_at: Some(now()),
            reason: kind.into(),
        };
        let documents = delivery
            .as_ref()
            .and_then(|id| s.deliveries.iter().find(|d| &d.id == id))
            .map(|d| d.documents.clone())
            .unwrap_or_else(|| document_ids(s, fid, tid));
        let execution_instances = s
            .executions
            .iter()
            .filter(|e| {
                e.run_id
                    .as_ref()
                    .is_some_and(|id| capture.runs.contains(id))
            })
            .map(|e| e.id.clone())
            .collect();
        s.snapshots.push(Snapshot {
            execution_instances,
            runtime_bindings: capture
                .sessions
                .iter()
                .filter_map(|id| {
                    s.sessions
                        .iter()
                        .find(|session| &session.id == id)
                        .and_then(|session| session.runtime_binding.clone())
                })
                .collect(),
            project_id: None,
            id: sid.clone(),
            feature_id: Some(fid.into()),
            task_id: tid.map(str::to_owned),
            kind: kind.into(),
            content,
            binding: b,
            documents,
            capture,
            delivery_id: delivery,
            inherited_from: inherited,
        });
        self.index_experience(s, &sid)?;
        let mode = s
            .projects
            .iter()
            .find(|p| p.id == s.feature(fid).unwrap().project_id)
            .map(|p| p.execution_mode.as_str())
            .unwrap_or("live");
        let mut manifest = self.artifacts.seal(
            fid,
            tid,
            "snapshots",
            &serde_json::to_string_pretty(
                &json!({"schema_version":1,"snapshot":s.snapshots.last(),"execution_mode":mode,"merge":capture_merge.as_ref().and_then(|id|s.merges.iter().find(|m|&m.id==id)),"repository_receipts":s.repository_operations.iter().filter(|op|match &op.action{RepositoryAction::PullRequest{merge,..}|RepositoryAction::PublishRef{merge,..}=>Some(merge)==capture_merge.as_ref(),_=>false}).collect::<Vec<_>>()}),
            )
            .map_err(|e| err("serialization", e.to_string()))?,
        )?;
        if mode == "live" {
            manifest.origin = Origin::External;
        }
        s.artifacts.push(manifest);
        Ok(sid)
    }
    pub fn tick(&self, global_limit: usize) -> Result<()> {
        if self
            .store
            .snapshot()?
            .projects
            .iter()
            .any(|p| p.execution_mode != "live")
        {
            return Ok(());
        }
        self.drive_tool_calls()?;
        self.reconcile_native_tests()?;
        self.drain_advances()?;
        let snapshot = self.store.snapshot()?;
        let mut outputs = vec![];
        for run in snapshot
            .runs
            .iter()
            .filter(|r| r.status == Status::Running && r.origin == Origin::External)
        {
            match self.poll_live_run(&snapshot, run) {
                Ok(Some(output)) => outputs.push((run.clone(), output)),
                Ok(None) => {}
                Err(error)
                    if run.origin == Origin::External
                        && !snapshot
                            .turns
                            .iter()
                            .any(|t| t.run_id.as_ref() == Some(&run.id))
                        && !snapshot
                            .executions
                            .iter()
                            .any(|e| e.run_id.as_ref() == Some(&run.id)) =>
                {
                    outputs.push((run.clone(),json!({"status":"failed","failure_type":"input_contract","code":error.code,"message":error.message,"not_dispatched":true,"origin":"external"})));
                }
                Err(error) => return Err(error),
            }
        }
        let mut action = |s: &mut EngineeringState| {
            for (observed_run, output) in &outputs {
                let Some(r) = s.runs.iter().find(|r| r.id == observed_run.id).cloned() else {
                    continue;
                };
                if r.status != Status::Running {
                    continue;
                }
                require(
                    json!(r.binding) == json!(observed_run.binding),
                    "执行绑定在外部调用期间变化",
                )?;
                let invalid = if let Some(aid) = &r.attempt_id {
                    let a = s.attempt(aid)?;
                    !s.attempt_inputs_current(&a)
                        || (!r.node.starts_with("review:")
                            && [Status::Cancelled, Status::Interrupted, Status::Rejected]
                                .contains(&a.status))
                } else {
                    let m = s.merge(r.merge_id.as_deref().unwrap())?;
                    (if r.node.starts_with("review:") {
                        ![Status::WaitingReview, Status::Accepted].contains(&m.status)
                    } else {
                        m.status != Status::Running
                    }) || m.input_version != s.feature(&m.feature_id)?.input_version
                        || !s
                            .heads
                            .iter()
                            .any(|h| h.id == m.target && h.revision == m.expected_target)
                };
                if (r.cancel_requested || invalid) && output["cancellation_confirmed"] != true {
                    s.runs
                        .iter_mut()
                        .find(|run| run.id == r.id)
                        .unwrap()
                        .cancel_requested = true;
                    self.finish_effects(
                        s,
                        &r.id,
                        EffectStatus::Discarded,
                        "调用期间取消或输入失效，保留回执但不接纳",
                    );
                    continue;
                }
                let mut output = output.clone();
                if let Some(instance) = s
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.as_mut())
                {
                    let conflict = [
                        ("external_session", &instance.external_session),
                        ("external_environment", &instance.external_environment),
                    ]
                    .iter()
                    .any(|(key, slot)| {
                        output[*key]
                            .as_str()
                            .is_some_and(|value| slot.as_ref().is_some_and(|old| old != value))
                    });
                    if conflict {
                        output = json!({"status":"failed","failure_type":"adapter_contract","message":"供应商实例身份变化"});
                    } else {
                        if let Some(value) = output["external_session"].as_str() {
                            instance.external_session = Some(value.into());
                        }
                        if let Some(value) = output["external_environment"].as_str() {
                            instance.external_environment = Some(value.into());
                        }
                    }
                }
                let status: Status =
                    serde_json::from_value(output["status"].clone()).unwrap_or(Status::Failed);
                let status = if status == Status::Succeeded && output["status"] == "failed" {
                    Status::Failed
                } else {
                    status
                };
                {
                    let rm = s.runs.iter_mut().find(|x| x.id == r.id).unwrap();
                    rm.status = status;
                    rm.finished = Some(now());
                    rm.result = Some(output.clone());
                }
                if status == Status::Succeeded {
                    if r.node.starts_with("review:") {
                        let did = r.node.trim_start_matches("review:");
                        let d = s.delivery(did)?.clone();
                        let decision = serde_json::from_value(
                            output
                                .get("review_decision")
                                .cloned()
                                .unwrap_or(Value::Null),
                        )
                        .map_err(|_| err("adapter_contract", "Reviewer 缺少结构化决定"))?;
                        if let Err(e) = self.review(
                            s,
                            &d,
                            decision,
                            output["summary"].as_str().unwrap_or("独立原生 Reviewer"),
                            "agent:native-reviewer",
                            Some(r.id.clone()),
                        ) {
                            let rm = s.runs.iter_mut().find(|x| x.id == r.id).unwrap();
                            rm.status = Status::Failed;
                            rm.result =
                                Some(json!({"failure_type":"stale_review","message":e.message}));
                        }
                    } else if let Some(aid) = &r.attempt_id {
                        self.produce_documents(s, aid, &output)?;
                    }
                }
                let (fid, tid) = if let Some(aid) = &r.attempt_id {
                    let a = s.attempt(aid)?;
                    (a.feature_id.clone(), a.task_id.clone())
                } else {
                    let m = s.merge(r.merge_id.as_deref().unwrap())?;
                    (m.feature_id.clone(), m.task_id.clone())
                };
                let mut log=self.artifacts.seal(&fid,tid.as_deref(),"runs",&format!("# Run {}\n\nnode={}\norigin={}\nstarted={:?}\nfinished={}\nstatus={status:?}\n\n```json\n{}\n```\n",r.id,r.node,serde_json::to_value(r.origin).unwrap().as_str().unwrap(),r.started,now(),serde_json::to_string_pretty(&output).unwrap()))?;
                log.origin = r.origin;
                s.runs
                    .iter_mut()
                    .find(|v| v.id == r.id)
                    .unwrap()
                    .result_artifact = Some(log.id.clone());
                s.artifacts.push(log);
                self.finish_effects(s, &r.id, EffectStatus::Applied, "回执已接纳，产物已封存");
                if let Some(instance) = s
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.as_mut())
                {
                    instance.status = status;
                    instance.finished = Some(now());
                }
                event(s, &r.id, "run.finished", &format!("{status:?}"));
            }
            let attempts = s
                .attempts
                .iter()
                .filter(|a| a.status == Status::Running)
                .cloned()
                .collect::<Vec<_>>();
            for a in attempts {
                let rs = effective_runs(s, &a);
                let all = rs.iter().all(|r| {
                    r.status == Status::Succeeded
                        || r.result
                            .as_ref()
                            .is_some_and(|v| v["failure_type"] == "assertion")
                });
                let failed = rs.iter().any(|r| {
                    [Status::Failed, Status::Cancelled, Status::Interrupted].contains(&r.status)
                        && !r
                            .result
                            .as_ref()
                            .is_some_and(|v| v["failure_type"] == "assertion")
                });
                let requirement_gate = a.phase == "planning"
                    && rs
                        .iter()
                        .any(|r| r.node == "requirement" && r.status == Status::Succeeded)
                    && !s.deliveries.iter().any(|d| {
                        d.attempt_id.as_deref() == Some(&a.id)
                            && d.kind == "requirement"
                            && s.accepted(d)
                    });
                if all || failed || requirement_gate {
                    let am = s.attempts.iter_mut().find(|x| x.id == a.id).unwrap();
                    am.status = if all || requirement_gate {
                        Status::WaitingReview
                    } else {
                        Status::Failed
                    };
                    am.revision += 1;
                }
            }
            let obsolete = s
                .merges
                .iter()
                .filter(|m| {
                    m.status == Status::Running
                        && (!s.delivery(&m.source_delivery).is_ok_and(|d| s.accepted(d))
                            || !s
                                .heads
                                .iter()
                                .any(|h| h.id == m.target && h.revision == m.expected_target))
                })
                .map(|m| m.id.clone())
                .collect::<Vec<_>>();
            for mid in obsolete {
                let m = s.merges.iter_mut().find(|m| m.id == mid).unwrap();
                m.status = Status::Interrupted;
                m.revision += 1;
                event(
                    s,
                    &mid,
                    "merge.invalidated",
                    "源或目标变化，需重新构造与验证",
                );
            }
            let merges = s
                .merges
                .iter()
                .filter(|m| m.status == Status::Running)
                .cloned()
                .collect::<Vec<_>>();
            for m in merges {
                let rs = s
                    .latest_runs(None, Some(&m.id))
                    .into_iter()
                    .filter(|r| !r.node.starts_with("review:"))
                    .collect::<Vec<_>>();
                if rs.iter().all(|r| {
                    r.status == Status::Succeeded
                        || r.result
                            .as_ref()
                            .is_some_and(|v| v["failure_type"] == "assertion")
                }) && (!rs.iter().any(|r| r.origin == Origin::External)
                    || m.change_request
                        .as_ref()
                        .is_some_and(|p| p.provider == "github"))
                    && !s
                        .deliveries
                        .iter()
                        .any(|d| d.merge_id.as_deref() == Some(&m.id))
                {
                    let evidence = rs.iter().map(|r| r.id.clone()).collect();
                    let docs = s.delivery(&m.source_delivery)?.documents.clone();
                    self.make_delivery(
                        s,
                        &m.feature_id,
                        m.task_id.as_deref(),
                        None,
                        Some(&m.id),
                        "merge_candidate",
                        m.input_version,
                        docs,
                        evidence,
                        Some(m.candidate.clone()),
                    )?;
                    let mm = s.merges.iter_mut().find(|x| x.id == m.id).unwrap();
                    mm.status = Status::WaitingReview;
                    mm.revision += 1;
                }
            }
            let queue = s
                .runs
                .iter()
                .filter(|r| r.status == Status::Queued && r.origin == Origin::External)
                .cloned()
                .collect::<Vec<_>>();
            for r in queue {
                if let Some(reason) = self.blocked(s, &r, global_limit) {
                    let _ = reason;
                    continue;
                }
                let rm = s.runs.iter_mut().find(|x| x.id == r.id).unwrap();
                rm.status = Status::Running;
                rm.started = Some(now());
                if let Some(instance) = s
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.as_mut())
                {
                    instance.status = Status::Running;
                    instance.started = Some(now());
                }

                event(s, &r.id, "run.started", &r.node);
            }
            Ok(json!({"ok":true}))
        };
        self.store.transact(None, &mut action)?;
        self.drain_live_cleanup()?;
        Ok(())
    }
    fn blocked(&self, s: &EngineeringState, r: &Run, limit: usize) -> Option<String> {
        let owner = r
            .attempt_id
            .as_ref()
            .and_then(|id| s.attempt(id).ok())
            .map(|a| a.task_id.as_deref().unwrap_or(&a.feature_id))
            .or_else(|| {
                r.merge_id
                    .as_ref()
                    .and_then(|id| s.merge(id).ok())
                    .map(|m| m.task_id.as_deref().unwrap_or(&m.feature_id))
            });
        if owner.is_some_and(|owner| s.workflow_paused(owner)) {
            return Some("工作流已暂停".into());
        }
        let (fid, paused) = if let Some(aid) = &r.attempt_id {
            let a = s.attempt(aid).ok()?;
            let reviewer = r.node.starts_with("review:");
            if s.turns.iter().any(|t| {
                t.session_id == r.session_id
                    && t.status.active()
                    && t.run_id.as_ref() != Some(&r.id)
            }) {
                return Some("等待角色 Session 的当前轮次结束".into());
            }

            (
                a.feature_id.clone(),
                !reviewer && (a.paused || a.status != Status::Running),
            )
        } else {
            (
                s.merge(r.merge_id.as_deref()?).ok()?.feature_id.clone(),
                false,
            )
        };
        if r.node == "design" {
            if let Some(aid) = &r.attempt_id {
                if !s.deliveries.iter().any(|d| {
                    d.attempt_id.as_ref() == Some(aid) && d.kind == "requirement" && s.accepted(d)
                }) {
                    return Some("等待 Requirement Delivery Review".into());
                }
            }
        }
        if paused {
            return Some("Attempt 暂停、失败或等待 Review".into());
        }
        if !r.dependencies.iter().all(|dep| {
            s.latest_runs(r.attempt_id.as_deref(), r.merge_id.as_deref())
                .iter()
                .any(|x| {
                    &x.node == dep
                        && (x.status == Status::Succeeded
                            || x.result
                                .as_ref()
                                .is_some_and(|v| v["failure_type"] == "assertion"))
                })
        }) {
            return Some("等待 DAG 依赖".into());
        }
        let running = s
            .runs
            .iter()
            .filter(|r| r.status == Status::Running)
            .collect::<Vec<_>>();
        if running.len() >= limit {
            return Some("全局并发额度".into());
        }
        let f = s.feature(&fid).ok()?;
        let project = s.projects.iter().find(|p| p.id == f.project_id)?;
        let count = running
            .iter()
            .filter(|r| {
                let rf = if let Some(aid) = &r.attempt_id {
                    s.attempt(aid).map(|a| a.feature_id.clone()).ok()
                } else {
                    r.merge_id
                        .as_ref()
                        .and_then(|mid| s.merge(mid).map(|m| m.feature_id.clone()).ok())
                };
                rf.is_some_and(|id| s.feature(&id).is_ok_and(|f| f.project_id == project.id))
            })
            .count();
        if count >= project.concurrency as usize {
            return Some("项目并发额度".into());
        }
        if r.writes
            && s.executions.iter().any(|e| {
                !e.writer_revoked
                    && e.run_id.as_ref().is_some_and(|id| {
                        s.runs.iter().any(|owner| {
                            &owner.id == id && owner.id != r.id && owner.worktree == r.worktree
                        })
                    })
            })
        {
            return Some("旧原生执行者尚未证明停止".into());
        }
        if r.writes
            && s.resources.iter().any(|res| {
                [Status::Active, Status::CleanupFailed].contains(&res.status)
                    && s.runs.iter().any(|owner| {
                        owner.id == res.run_id
                            && owner.id != r.id
                            && owner.worktree == r.worktree
                            && owner.writes
                            && owner.status != Status::Running
                    })
            })
        {
            return Some("旧执行资源尚未核对或释放".into());
        }
        if r.writes && running.iter().any(|x| x.writes && x.worktree == r.worktree) {
            return Some("逻辑 worktree 写许可".into());
        }
        None
    }
    fn produce_documents(&self, s: &mut EngineeringState, aid: &str, output: &Value) -> Result<()> {
        let Some(document) = output.get("document") else {
            return Ok(());
        };
        let kind = text(document, "kind")?;
        require(
            ["requirement", "design", "test", "doc"].contains(&kind.as_str()),
            "供应商返回未知文档类型",
        )?;
        let body = text(document, "body")?;
        let title = text(document, "title")?;
        let a = s.attempt(aid)?.clone();
        if !s.attempt_inputs_current(&a) {
            return Ok(());
        }
        if kind == "requirement"
            && s.documents
                .iter()
                .any(|d| s.document_in_feature(d, &a.feature_id) && d.kind == kind)
        {
            if let Some(req) = s
                .documents
                .iter()
                .rev()
                .find(|d| s.document_in_feature(d, &a.feature_id) && d.kind == "requirement")
            {
                let id = req.id.clone();
                if !a.documents.contains(&id) {
                    s.attempts
                        .iter_mut()
                        .find(|x| x.id == aid)
                        .unwrap()
                        .outputs
                        .push(id);
                }
            }
            return Ok(());
        }
        let old = s
            .documents
            .iter()
            .rev()
            .find(|d| {
                s.document_in_feature(d, &a.feature_id)
                    && d.task_id == a.task_id
                    && same_document_stream(d, &kind, &title)
            })
            .cloned();
        let req = s
            .documents
            .iter()
            .rev()
            .find(|d| {
                s.document_in_feature(d, &a.feature_id)
                    && d.kind == "requirement"
                    && (a.documents.contains(&d.id) || a.outputs.contains(&d.id))
            })
            .map(|d| d.id.clone());
        let design = s
            .documents
            .iter()
            .rev()
            .find(|d| {
                s.document_in_feature(d, &a.feature_id)
                    && d.kind == "design"
                    && (a.documents.contains(&d.id) || a.outputs.contains(&d.id))
            })
            .map(|d| d.id.clone());
        let origin = Origin::External;
        let mut artifact = self.artifacts.seal(
            &a.feature_id,
            a.task_id.as_deref(),
            "documents",
            &format!(
                "# {title}\n\norigin={}\n\n{body}",
                serde_json::to_value(origin).unwrap().as_str().unwrap()
            ),
        )?;
        artifact.origin = origin;
        let artifact_id = Some(artifact.id.clone());
        s.artifacts.push(artifact);
        let acceptance = if kind == "requirement" {
            criteria(document, &body)?
        } else {
            vec![]
        };
        let verifies = if kind == "test" {
            req.as_deref()
                .map(|r| s.acceptance_keys(r))
                .transpose()?
                .unwrap_or_default()
        } else {
            vec![]
        };
        let document_id = id();
        s.attempts
            .iter_mut()
            .find(|x| x.id == aid)
            .unwrap()
            .outputs
            .push(document_id.clone());
        s.documents.push(Document {
            test_recipe: None,
            project_id: None,
            id: document_id,
            feature_id: Some(a.feature_id),
            task_id: a.task_id,
            kind,
            title,
            body,
            version: old.as_ref().map_or(1, |d| d.version + 1),
            supersedes: old.map(|d| d.id),
            requirement_id: req,
            design_id: design,
            origin,
            acceptance,
            verifies,
            artifact_id,
        });
        Ok(())
    }
    pub fn query(&self, method: &str, p: &Value, global_limit: usize) -> Result<Value> {
        if let Some(result) = self.task_query(method, p) {
            return result;
        }
        if let Some(result) = self.integration_query(method, p) {
            return result;
        }
        if let Some(result) = self.workflow_configuration_query(method, p) {
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
                let pid = opt(p, "project_id");
                let fid = opt(p, "feature_id");
                let features = s
                    .features
                    .iter()
                    .filter(|f| pid.as_ref().is_none_or(|id| &f.project_id == id))
                    .filter(|f| match p["work_filter"].as_str().unwrap_or("") {
                        "tasks" => s.tasks.iter().any(|t| t.feature_id == f.id),
                        "running" => s.runs.iter().any(|r| {
                            r.status == Status::Running
                                && r.attempt_id.as_ref().is_some_and(|id| {
                                    s.attempt(id).is_ok_and(|a| a.feature_id == f.id)
                                })
                        }),
                        "waiting_review" => s.deliveries.iter().any(|d| {
                            d.feature_id == f.id
                                && !d.stale
                                && s.delivery_inputs_current(d)
                                && !s.reviews.iter().any(|r| {
                                    r.delivery_id == d.id
                                        && [Decision::Pass, Decision::ChangesRequested]
                                            .contains(&r.decision)
                                })
                                && !s.accepted(d)
                        }),
                        "waiting_confirmation" | "blocked" => s
                            .governance
                            .iter()
                            .filter(|r| {
                                r.kind == "workflow_instance"
                                    && r.data["status"] != "cancelled"
                                    && r.data["feature_id"] == f.id
                            })
                            .filter_map(|r| r.decode::<WorkflowRuntime>().ok())
                            .flat_map(|r| r.steps)
                            .any(|step| {
                                if p["work_filter"] == "waiting_confirmation" {
                                    step.status == "waiting_confirmation"
                                } else {
                                    ["blocked", "failed"].contains(&step.status.as_str())
                                }
                            }),
                        _ => true,
                    })
                    .collect::<Vec<_>>();
                let detail = if let Some(fid) = &fid {
                    {
                        let mut detail =
                            self.feature_detail(&s, fid, limit, global_limit, &p["detail_pages"])?;
                        if let Some(tid) = p["task_id"].as_str() {
                            let t = s.task(tid)?;
                            require(t.feature_id == *fid, "Task 不属于当前 Feature")?;
                            if !detail["tasks"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|r| r["id"] == tid)
                            {
                                detail["tasks"].as_array_mut().unwrap().push(json!(t));
                            }
                            detail["attempts"] = json!(
                                s.attempts
                                    .iter()
                                    .filter(|a| a.task_id.as_deref() == Some(tid))
                                    .rev()
                                    .take(limit)
                                    .collect::<Vec<_>>()
                            );
                            let docs = document_ids(&s, fid, Some(tid));
                            detail["documents"] = json!(
                                s.documents
                                    .iter()
                                    .filter(|d| docs.contains(&d.id))
                                    .rev()
                                    .take(limit)
                                    .collect::<Vec<_>>()
                            );
                            detail["snapshots"] = json!(
                                s.snapshots
                                    .iter()
                                    .filter(|snap| snap.task_id.as_deref() == Some(tid))
                                    .rev()
                                    .take(limit)
                                    .collect::<Vec<_>>()
                            );
                            detail["contexts"] = json!(
                                s.contexts
                                    .iter()
                                    .filter(|context| context.owner_id == tid)
                                    .collect::<Vec<_>>()
                            );
                            detail["runs"] = json!(
                                s.runs
                                    .iter()
                                    .filter(|r| r.attempt_id.as_ref().is_some_and(|id| s
                                        .attempt(id)
                                        .is_ok_and(|a| a.task_id.as_deref() == Some(tid)))
                                        || r.merge_id.as_ref().is_some_and(|id| s
                                            .merge(id)
                                            .is_ok_and(|m| m.task_id.as_deref() == Some(tid))))
                                    .rev()
                                    .take(limit * 5)
                                    .collect::<Vec<_>>()
                            );
                        }
                        Some(detail)
                    }
                } else {
                    None
                };
                Ok(
                    json!({"projects":s.projects.iter().skip(p.get("project_offset").and_then(Value::as_u64).unwrap_or(0) as usize).take(limit).collect::<Vec<_>>(),"features":features.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_features":features.len(),"summary":{"features":s.features.len(),"tasks":s.tasks.len(),"running":s.runs.iter().filter(|r|r.status==Status::Running).count(),"waiting_review":s.deliveries.iter().filter(|d|!d.stale&&s.delivery_inputs_current(d)&&!s.reviews.iter().any(|r|r.delivery_id==d.id&&[Decision::Pass,Decision::ChangesRequested].contains(&r.decision))&&!s.accepted(d)).count(),"waiting_confirmation":s.governance.iter().filter(|r|r.kind=="workflow_instance"&&r.data["status"]!="cancelled").filter_map(|r|r.decode::<WorkflowRuntime>().ok()).flat_map(|r|r.steps).filter(|step|step.status=="waiting_confirmation").count(),"blocked":s.governance.iter().filter(|r|r.kind=="workflow_instance"&&r.data["status"]!="cancelled").filter_map(|r|r.decode::<WorkflowRuntime>().ok()).flat_map(|r|r.steps).filter(|step|["blocked","failed"].contains(&step.status.as_str())).count()},"configurations":s.governance.iter().filter(|r|["feature_lock","legacy_feature_lock","workflow_instance","project_configuration"].contains(&r.kind.as_str())).collect::<Vec<_>>(),"total_projects":s.projects.len(),"definitions":s.definitions,"harnesses":s.harnesses,"sandboxes":s.sandboxes,"detail":detail,"data_root":self.store.root(),"execution_mode":"live","repository_ready":s.projects.iter().any(|p| p.execution_mode=="live" && s.repository_worktrees.iter().any(|w|w.project_id==p.id && w.kind==WorktreeKind::Mainline)),"global_concurrency":global_limit}),
                )
            }
            "query.history" => {
                let target = opt(p, "target");
                let events = s
                    .events
                    .iter()
                    .rev()
                    .filter(|e| target.as_ref().is_none_or(|id| &e.target == id))
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                Ok(json!({"events":events,"total":s.events.len()}))
            }
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
        global_limit: usize,
        pages: &Value,
    ) -> Result<Value> {
        let f = s.feature(fid)?;
        let page_offset = |key: &str| pages[key].as_u64().unwrap_or(0) as usize * limit;
        let attempts = s
            .attempts
            .iter()
            .filter(|a| a.feature_id == fid)
            .rev()
            .skip(page_offset("attempts"))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let merges = s
            .merges
            .iter()
            .filter(|m| m.feature_id == fid)
            .rev()
            .skip(page_offset("merges"))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let ids = attempts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>();
        let mids = merges.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
        let runs = s
            .runs
            .iter()
            .filter(|r| {
                r.attempt_id.as_deref().is_some_and(|id| ids.contains(&id))
                    || r.merge_id.as_deref().is_some_and(|id| mids.contains(&id))
            })
            .rev()
            .skip(page_offset("runs") * 5)
            .take(limit * 5)
            .collect::<Vec<_>>();
        let deliveries = s
            .deliveries
            .iter()
            .filter(|d| d.feature_id == fid)
            .rev()
            .skip(page_offset("deliveries"))
            .take(limit)
            .collect::<Vec<_>>();
        let dids = deliveries.iter().map(|d| d.id.as_str()).collect::<Vec<_>>();
        let tasks = s
            .tasks
            .iter()
            .filter(|t| t.feature_id == fid)
            .skip(page_offset("tasks"))
            .take(limit)
            .collect::<Vec<_>>();
        let mut actions = serde_json::Map::new();
        for target in std::iter::once(f.id.as_str())
            .chain(tasks.iter().map(|t| t.id.as_str()))
            .chain(attempts.iter().map(|a| a.id.as_str()))
            .chain(deliveries.iter().map(|d| d.id.as_str()))
            .chain(merges.iter().map(|m| m.id.as_str()))
        {
            actions.insert(target.into(), self.actions(s, target));
        }
        let effects: Vec<_> = s
            .effects
            .iter()
            .filter(|effect| {
                s.runs.iter().any(|r| {
                    r.id == effect.run_id
                        && (r.attempt_id.as_deref().is_some_and(|id| {
                            s.attempts.iter().any(|a| a.id == id && a.feature_id == fid)
                        }) || r.merge_id.as_deref().is_some_and(|id| {
                            s.merges.iter().any(|m| m.id == id && m.feature_id == fid)
                        }))
                })
            })
            .rev()
            .collect();
        for effect in effects.iter().skip(page_offset("effects")).take(limit) {
            actions.insert(effect.id.clone(), self.actions(s, &effect.id));
        }
        let blocked = runs
            .iter()
            .filter(|r| r.status == Status::Queued && r.origin == Origin::External)
            .map(|r| (r.id.clone(), self.blocked(s, r, global_limit)))
            .collect::<std::collections::BTreeMap<_, _>>();
        Ok(
            json!({"mainline_sync":s.repository_operations.iter().filter(|op| matches!(op.action, RepositoryAction::RefreshMainline { .. })).rev().take(3).collect::<Vec<_>>(),"advances":s.advances.iter().filter(|advance|s.merges.iter().any(|m|m.id==advance.id&&m.feature_id==fid)).rev().take(limit).collect::<Vec<_>>(),"effects":effects.iter().skip(page_offset("effects")).take(limit).collect::<Vec<_>>(),"totals":{"effects":effects.len(),"tasks":s.tasks.iter().filter(|x|x.feature_id==fid).count(),"attempts":s.attempts.iter().filter(|x|x.feature_id==fid).count(),"merges":s.merges.iter().filter(|x|x.feature_id==fid).count(),"deliveries":s.deliveries.iter().filter(|x|x.feature_id==fid).count(),"documents":s.documents.iter().filter(|x|s.document_in_feature(x,fid)).count(),"snapshots":s.snapshots.iter().filter(|x|x.feature_id.as_deref()==Some(fid)).count(),"artifacts":s.artifacts.iter().filter(|x|x.feature_id.as_deref()==Some(fid)).count(),"runs":s.runs.iter().filter(|r|r.attempt_id.as_deref().is_some_and(|id|ids.contains(&id))||r.merge_id.as_deref().is_some_and(|id|mids.contains(&id))).count()},"feature":f,"tasks":tasks,"attempts":attempts,"runs":runs,"documents":s.documents.iter().filter(|d|s.document_in_feature(d,fid)).rev().skip(page_offset("documents")).take(limit).collect::<Vec<_>>(),"deliveries":deliveries,"reviews":s.reviews.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).rev().take(limit).collect::<Vec<_>>(),"overrides":s.overrides.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).collect::<Vec<_>>(),"merges":merges,"snapshots":s.snapshots.iter().filter(|x|x.feature_id.as_deref()==Some(fid)).rev().skip(page_offset("snapshots")).take(limit).collect::<Vec<_>>(),"contexts":s.contexts.iter().filter(|x|x.owner_id==fid||tasks.iter().any(|t|t.id==x.owner_id)).collect::<Vec<_>>(),"artifacts":s.artifacts.iter().filter(|a|a.feature_id.as_deref()==Some(fid)).rev().skip(page_offset("artifacts")).take(limit).collect::<Vec<_>>(),"sessions":s.sessions.iter().filter(|x|x.feature_id.as_deref()==Some(fid)||runs.iter().any(|r|r.session_id==x.id)).collect::<Vec<_>>(),"pending_interactions":s.interactions.iter().filter(|i|i.status==InteractionStatus::Pending&&s.sessions.iter().any(|session|session.id==i.session_id&&session.feature_id.as_deref()==Some(fid))).collect::<Vec<_>>(),"merge_progress":merges.iter().map(|m|json!({"merge":m.id,"local":s.advances.iter().find(|a|a.id==m.id),"remote_operations":s.repository_operations.iter().filter(|op|match &op.action{RepositoryAction::PublishRef{merge,..}|RepositoryAction::PullRequest{merge,..}=>merge==&m.id,_=>false}).collect::<Vec<_>>()})).collect::<Vec<_>>(),"resources":s.resources.iter().filter(|x|runs.iter().any(|r|r.id==x.run_id)).collect::<Vec<_>>(),"heads":s.heads.iter().filter(|h|h.project_id==f.project_id).collect::<Vec<_>>(),"allowed_actions":actions,"blocked_reasons":blocked,"accepted_deliveries":deliveries.iter().filter(|d|s.accepted(d)).map(|d|&d.id).collect::<Vec<_>>()}),
        )
    }
    fn actions(&self, s: &EngineeringState, target: &str) -> Value {
        let mut allowed = vec![];
        let mut blocked = vec![];
        if s.projects.iter().any(|p| p.execution_mode != "live") {
            return json!({"allowed":[],"blocked":["历史项目只读"]});
        }
        if let Some(f) = s.features.iter().find(|x| x.id == target) {
            if f.status != Status::Completed {
                allowed.extend(["document.revise", "binding.update", "snapshot.save"]);
                if !s.planning_accepted(&f.id) {
                    allowed.push("attempt.start");
                }
                if [Status::Ready, Status::Developing].contains(&f.status) {
                    allowed.push("task.create");
                }
                if s.tasks.iter().any(|t| t.feature_id == f.id)
                    && s.tasks
                        .iter()
                        .filter(|t| t.feature_id == f.id)
                        .all(|t| t.status == Status::Completed)
                {
                    allowed.push("attempt.start");
                }
                if [Status::Accepted, Status::Merging].contains(&f.status) {
                    allowed.push("merge.request");
                }
            } else {
                blocked.push("已完成 Feature 已冻结");
            }
        }
        if let Some(t) = s.tasks.iter().find(|x| x.id == target) {
            if t.status != Status::Completed {
                allowed.extend([
                    "attempt.start",
                    "binding.update",
                    "snapshot.save",
                    "task.replan",
                ]);
                if [Status::Accepted, Status::Merging].contains(&t.status) {
                    allowed.push("merge.request");
                }
            }
        }
        if let Some(a) = s.attempts.iter().find(|x| x.id == target) {
            if [Status::WaitingReview, Status::Failed].contains(&a.status) {
                allowed.push("delivery.submit");
            }
            if a.status == Status::Running {
                allowed.push("attempt.pause");
            }
            if ![Status::Accepted, Status::Rejected, Status::Cancelled].contains(&a.status) {
                allowed.push("attempt.cancel");
            }
        }
        if let Some(d) = s.deliveries.iter().find(|x| x.id == target) {
            if !d.stale && self.check_merge_target(s, d).is_ok() {
                if !s.reviews.iter().any(|r| {
                    r.delivery_id == d.id
                        && [Decision::Pass, Decision::ChangesRequested].contains(&r.decision)
                }) {
                    allowed.extend(["review.submit", "review.request"]);
                }
                if !s.accepted(d) {
                    allowed.push("override.accept");
                }
            } else {
                blocked.push("交付证据或目标版本已失效");
            }
        }
        if let Some(m) = s.merges.iter().find(|x| x.id == target) {
            if s.advances
                .iter()
                .any(|a| a.id == m.id && a.status == AdvanceStatus::NeedsReconciliation)
            {
                allowed.push("merge.reconcile");
            }

            if m.status != Status::Completed
                && !s
                    .advances
                    .iter()
                    .any(|a| a.id == m.id && a.status != AdvanceStatus::NotApplied)
                && s.deliveries.iter().any(|d| {
                    d.merge_id.as_deref() == Some(target)
                        && s.accepted(d)
                        && self.check_merge_target(s, d).is_ok()
                })
            {
                allowed.push("merge.apply");
            } else {
                blocked.push("候选验证/Review 未完成或目标已变化");
            }
        }
        let owner = s
            .attempts
            .iter()
            .find(|a| a.id == target)
            .map(|a| a.task_id.as_deref().unwrap_or(&a.feature_id))
            .or_else(|| {
                s.deliveries
                    .iter()
                    .find(|d| d.id == target)
                    .map(|d| d.task_id.as_deref().unwrap_or(&d.feature_id))
            })
            .or_else(|| {
                s.merges
                    .iter()
                    .find(|m| m.id == target)
                    .map(|m| m.task_id.as_deref().unwrap_or(&m.feature_id))
            })
            .unwrap_or(target);
        let feature = s
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        if s.governance
            .iter()
            .any(|r| r.kind == "legacy_feature_lock" && r.definition_id == feature)
        {
            allowed.retain(|method| {
                ![
                    "binding.update",
                    "attempt.start",
                    "merge.request",
                    "review.request",
                    "task.create",
                ]
                .contains(method)
            });
            blocked.push("历史 Feature 已保留原配置；新执行请使用完整版本化配置创建后继 Feature");
        }
        if let Some(lock) = s.feature_configuration(feature) {
            allowed.retain(|method| *method != "binding.update");
            let runtime = s
                .governance
                .iter()
                .rev()
                .find(|r| {
                    r.kind == "workflow_instance"
                        && r.definition_id == owner
                        && r.data["status"] != "cancelled"
                })
                .and_then(|r| r.decode::<WorkflowRuntime>().ok());
            let phases: Vec<_> = runtime
                .as_ref()
                .map(|r| {
                    let plan = if r.task_id.is_some() {
                        &lock.task_plan
                    } else {
                        &lock.plan
                    };
                    plan.steps
                        .iter()
                        .filter(|step| {
                            step.kind == "attempt.execute"
                                && r.steps.iter().any(|s| {
                                    s.key == step.key
                                        && !["completed", "failed", "skipped"]
                                            .contains(&s.status.as_str())
                                })
                                && step.dependencies.iter().all(|dep| {
                                    r.steps.iter().any(|s| {
                                        s.key == *dep
                                            && ["completed", "skipped"].contains(&s.status.as_str())
                                    })
                                })
                        })
                        .filter_map(|s| {
                            s.parameters
                                .get("phase")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !phases.is_empty() && !allowed.contains(&"attempt.start") {
                allowed.push("attempt.start");
            }
            allowed.retain(|method| {
                let c = Command {
                    command_id: "preview".into(),
                    target: Some(target.into()),
                    expected_revision: None,
                    payload: json!({"phase":phases.first()}),
                };
                self.enforce_workflow_action(s, method, &c).is_ok()
            });
            return json!({"allowed":allowed,"blocked":blocked,"attempt_phases":phases});
        }
        json!({"allowed":allowed,"blocked":blocked,"attempt_phases":if let Some(f)=s.features.iter().find(|f|f.id==target){if !s.planning_accepted(&f.id){vec!["planning"]}else if s.tasks.iter().any(|t|t.feature_id==f.id)&&s.tasks.iter().filter(|t|t.feature_id==f.id).all(|t|t.status==Status::Completed){vec!["closure"]}else{vec![]}}else{vec!["implementation"]}})
    }
}

#[cfg(test)]
mod document_stream_tests {
    use super::*;
    #[test]
    fn runtime_document_does_not_hide_the_structured_task_plan() {
        let mut state = EngineeringState::default();
        let make = |id: &str, title: &str, supersedes: Option<&str>| Document {
            id: id.into(),
            feature_id: Some("feature".into()),
            project_id: None,
            task_id: None,
            kind: "doc".into(),
            title: title.into(),
            body: "body".into(),
            version: 1,
            supersedes: supersedes.map(str::to_owned),
            requirement_id: None,
            design_id: None,
            origin: Origin::External,
            acceptance: vec![],
            verifies: vec![],
            artifact_id: None,
            test_recipe: None,
        };
        state
            .documents
            .push(make("plan", "Structured Task Plan", None));
        // Historical writers incorrectly linked their output to the planning stream.
        state
            .documents
            .push(make("notes", "Implementation notes", Some("plan")));
        assert_eq!(document_ids(&state, "feature", None), vec!["plan", "notes"]);
        state
            .documents
            .push(make("new-plan", "Structured Task Plan", Some("plan")));
        assert_eq!(
            document_ids(&state, "feature", None),
            vec!["notes", "new-plan"]
        );
    }
}
