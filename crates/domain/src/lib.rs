mod permission;
pub use permission::*;
mod testing;
pub use testing::*;
mod repository;
pub use repository::*;
mod execution;
pub use execution::*;
mod runtime;
pub use runtime::*;
mod agents;
pub use agents::*;
mod conversation;
pub use conversation::*;
mod governance;
pub use governance::*;
mod defaults;
pub use defaults::*;
// Pure domain model: no I/O or adapter dependencies.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Id = String;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialOwner {
    Project { id: Id },
    Feature { id: Id },
    Task { feature: Id, id: Id },
}
pub fn material_owner(
    project: Option<&str>,
    feature: Option<&str>,
    task: Option<&str>,
) -> Result<MaterialOwner> {
    match (project, feature, task) {
        (Some(id), None, None) => Ok(MaterialOwner::Project { id: id.into() }),
        (None, Some(id), None) => Ok(MaterialOwner::Feature { id: id.into() }),
        (None, Some(feature), Some(id)) => Ok(MaterialOwner::Task {
            feature: feature.into(),
            id: id.into(),
        }),
        _ => Err(err(
            "blocked",
            "材料必须明确且唯一归属 Project、Feature 或 Task",
        )),
    }
}
pub fn id() -> Id {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn err(code: &str, message: impl Into<String>) -> Error {
    Error {
        code: code.into(),
        message: message.into(),
    }
}
pub fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(err("blocked", message))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Draft,
    Planning,
    Ready,
    Developing,
    Validating,
    Accepted,
    Merging,
    Completed,
    Prepared,
    Running,
    WaitingReview,
    Rejected,
    Failed,
    Cancelled,
    Interrupted,
    Queued,
    Succeeded,
    Active,
    Released,
    CleanupFailed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Human,
    /// Historical provenance only; no execution provider can create new simulated work.
    Simulated,
    External,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Pass,
    ChangesRequested,
    Inconclusive,
    NotReviewable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingLock {
    pub agent: Id,
    pub skills: Vec<Id>,
    pub standard: Id,
    pub harness: String,
    pub sandbox: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub key: String,
    pub passed: bool,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub permission_overrides: Option<PermissionOverrides>,
    pub id: Id,
    pub title: String,
    pub workspace: String,
    pub workspace_definition: Option<Workspace>,
    pub execution_mode: String,
    pub binding: BindingLock,
    pub concurrency: u32,
    pub revision: u64,
    #[serde(default)]
    pub runtime_preferences: Option<ProjectRuntimePreferences>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: Id,
    pub project_id: Id,
    pub title: String,
    pub status: Status,
    pub revision: u64,
    pub input_version: u64,
    pub binding: BindingLock,
    pub predecessor: Option<Id>,
    #[serde(default)]
    pub source_requirement: Option<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub feature_id: Id,
    pub title: String,
    pub status: Status,
    pub revision: u64,
    pub binding: BindingLock,
    pub inherited_snapshot: Id,
    pub dependencies: Vec<Id>,
    pub worktree: String,
    pub scope: TaskScope,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskScope {
    pub requirement: Option<Id>,
    pub design: Option<Id>,
    pub acceptance: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub key: String,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Id,
    pub feature_id: Option<Id>,
    #[serde(default)]
    pub project_id: Option<Id>,
    pub task_id: Option<Id>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub requirement_id: Option<Id>,
    pub design_id: Option<Id>,
    pub origin: Origin,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub verifies: Vec<String>,
    pub artifact_id: Option<Id>,
    #[serde(default)]
    pub test_recipe: Option<TestRecipe>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSlot {
    pub purpose: String,
    pub spec: Id,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub id: Id,
    pub family: String,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub body: String,
    pub slots: Vec<SpecSlot>,
    pub rules: Vec<String>,
    #[serde(default)]
    pub agent_profile: Option<AgentProfile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub phase: String,
    pub baseline: Option<String>,
    #[serde(default)]
    pub source_base: Option<String>,
    #[serde(default)]
    pub live_sessions: std::collections::BTreeMap<String, Id>,
    pub git_context: Option<GitContext>,
    pub status: Status,
    pub input_version: u64,
    pub documents: Vec<Id>,
    pub outputs: Vec<Id>,
    pub scope: Option<TaskScope>,
    pub binding: BindingLock,
    pub context_content: String,
    pub context_revision: u64,
    pub paused: bool,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: Id,
    pub attempt_id: Option<Id>,
    pub merge_id: Option<Id>,
    pub node: String,
    pub dependencies: Vec<String>,
    pub status: Status,
    pub retry_of: Option<Id>,
    pub session_id: Id,
    pub binding: BindingLock,
    pub binding_verified: bool,
    pub writes: bool,
    pub worktree: String,
    pub outcome: String,
    pub cancel_requested: bool,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub result: Option<Value>,
    #[serde(default)]
    pub result_artifact: Option<Id>,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Id,
    pub owner_id: Id,
    pub agent: Id,
    pub harness: String,
    pub sandbox: String,
    pub snapshot_id: Option<Id>,
    pub instance: Option<ExecutionInstance>,
    pub origin: Origin,
    #[serde(default)]
    pub project_id: Option<Id>,
    #[serde(default)]
    pub feature_id: Option<Id>,
    #[serde(default)]
    pub task_id: Option<Id>,
    #[serde(default)]
    pub scope: Option<SessionScope>,
    #[serde(default)]
    pub lifecycle: Option<SessionLifecycle>,
    #[serde(default)]
    pub runtime_binding: Option<RuntimeBinding>,
    #[serde(default)]
    pub external_session: Option<String>,
    #[serde(default)]
    pub predecessor: Option<Id>,
    #[serde(default)]
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub id: Id,
    pub owner_id: Id,
    pub content: String,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: Id,
    pub feature_id: Option<Id>,
    #[serde(default)]
    pub project_id: Option<Id>,
    pub task_id: Option<Id>,
    pub kind: String,
    pub content: String,
    pub binding: BindingLock,
    pub documents: Vec<Id>,
    pub delivery_id: Option<Id>,
    pub inherited_from: Option<Id>,
    pub capture: SnapshotCapture,
    #[serde(default)]
    pub runtime_bindings: Vec<RuntimeBinding>,
    #[serde(default)]
    pub execution_instances: Vec<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub attempt_id: Option<Id>,
    pub merge_id: Option<Id>,
    pub kind: String,
    pub input_version: u64,
    pub documents: Vec<Id>,
    pub evidence: Vec<Id>,
    pub artifact_id: Id,
    #[serde(default)]
    pub source_deliveries: Vec<Id>,
    pub candidate: Option<String>,
    pub stale: bool,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: Id,
    pub delivery_id: Id,
    pub decision: Decision,
    pub checks: Vec<Check>,
    pub actor: String,
    pub run_id: Option<Id>,
    pub reason: String,
    pub assignment: Option<ReviewAssignment>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Override {
    pub id: Id,
    pub delivery_id: Id,
    pub actor: String,
    pub reason: String,
    pub items: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Merge {
    pub id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub status: Status,
    pub revision: u64,
    pub input_version: u64,
    pub source: String,
    pub source_delivery: Id,
    pub target: String,
    pub expected_target: String,
    pub candidate: String,
    pub committed_revision: Option<String>,
    pub change_request: Option<ChangeRequestRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHead {
    pub id: Id,
    pub project_id: Id,
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: Id,
    pub run_id: Id,
    pub kind: String,
    pub status: Status,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Id,
    pub feature_id: Option<Id>,
    #[serde(default)]
    pub project_id: Option<Id>,
    pub task_id: Option<Id>,
    pub relative_path: String,
    pub sha256: String,
    pub media_type: String,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub target: Id,
    pub kind: String,
    pub detail: String,
    pub time: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineeringState {
    #[serde(default)]
    pub governance: Vec<ConfigurationRecord>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub turns: Vec<ConversationTurn>,
    #[serde(default)]
    pub conversation_events: Vec<ConversationEvent>,
    #[serde(default)]
    pub interactions: Vec<PendingInteraction>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
    #[serde(default)]
    pub executions: Vec<RuntimeExecution>,
    #[serde(default)]
    pub repository_operations: Vec<RepositoryOperation>,
    #[serde(default)]
    pub repository_worktrees: Vec<RepositoryWorktree>,
    pub advances: Vec<MergeAdvance>,
    pub effects: Vec<EffectRecord>,
    pub harnesses: Vec<Harness>,
    pub sandboxes: Vec<Sandbox>,
    pub projects: Vec<Project>,
    pub features: Vec<Feature>,
    pub tasks: Vec<Task>,
    pub documents: Vec<Document>,
    pub definitions: Vec<Definition>,
    pub attempts: Vec<Attempt>,
    pub runs: Vec<Run>,
    pub sessions: Vec<Session>,
    pub contexts: Vec<Context>,
    pub snapshots: Vec<Snapshot>,
    pub deliveries: Vec<Delivery>,
    pub reviews: Vec<Review>,
    pub overrides: Vec<Override>,
    pub merges: Vec<Merge>,
    pub heads: Vec<GitHead>,
    pub resources: Vec<Resource>,
    pub artifacts: Vec<Artifact>,
    pub events: Vec<Event>,
}
impl EngineeringState {
    pub fn document_in_feature(&self, document: &Document, feature: &str) -> bool {
        if document.feature_id.as_deref() == Some(feature) {
            return true;
        }
        self.features
            .iter()
            .find(|f| f.id == feature)
            .is_some_and(|f| {
                document.project_id.as_ref() == Some(&f.project_id)
                    && f.source_requirement.as_deref() == Some(document.id.as_str())
            })
    }
    pub fn feature(&self, id: &str) -> Result<&Feature> {
        self.features
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| err("not_found", "Feature 不存在"))
    }
    pub fn task(&self, id: &str) -> Result<&Task> {
        self.tasks
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| err("not_found", "Task 不存在"))
    }
    pub fn attempt(&self, id: &str) -> Result<&Attempt> {
        self.attempts
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| err("not_found", "Attempt 不存在"))
    }
    pub fn delivery(&self, id: &str) -> Result<&Delivery> {
        self.deliveries
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| err("not_found", "Delivery 不存在"))
    }
    pub fn merge(&self, id: &str) -> Result<&Merge> {
        self.merges
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| err("not_found", "Merge 不存在"))
    }
    pub fn latest_runs(&self, attempt_id: Option<&str>, merge_id: Option<&str>) -> Vec<&Run> {
        let mut result: Vec<&Run> = vec![];
        for r in self.runs.iter().filter(|r| {
            (attempt_id.is_some() && r.attempt_id.as_deref() == attempt_id)
                || (merge_id.is_some() && r.merge_id.as_deref() == merge_id)
        }) {
            result.retain(|x| x.node != r.node);
            result.push(r);
        }
        result
    }
    pub fn accepted(&self, d: &Delivery) -> bool {
        if d.stale || !self.delivery_inputs_current(d) {
            return false;
        }
        let pass = self
            .reviews
            .iter()
            .rev()
            .find(|r| r.delivery_id == d.id)
            .is_some_and(|r| r.decision == Decision::Pass && r.checks.iter().all(|c| c.passed));
        let waived = self
            .overrides
            .iter()
            .filter(|o| o.delivery_id == d.id)
            .flat_map(|o| o.items.iter())
            .collect::<Vec<_>>();
        let review_ok = pass || waived.contains(&&"review".to_string());
        let tests_ok = d.evidence.iter().all(|id| {
            self.runs.iter().find(|r| &r.id == id).is_some_and(|r| {
                r.status == Status::Succeeded || waived.contains(&&format!("test:{id}"))
            })
        });
        review_ok && tests_ok
    }
    pub fn planning_accepted(&self, fid: &str) -> bool {
        if self.feature_configuration(fid).is_some() {
            return self.deliveries.iter().any(|plan| {
                plan.feature_id == fid
                    && plan.kind == "plan"
                    && self.accepted(plan)
                    && self.deliveries.iter().any(|req| {
                        req.feature_id == fid
                            && req.kind == "requirement"
                            && self.accepted(req)
                            && req
                                .documents
                                .iter()
                                .filter(|id| {
                                    self.documents
                                        .iter()
                                        .any(|d| &d.id == *id && d.kind == "requirement")
                                })
                                .all(|id| plan.documents.contains(id))
                    })
            });
        }
        self.feature(fid).is_ok_and(|f| {
            self.attempts.iter().any(|a| {
                a.feature_id == fid
                    && a.task_id.is_none()
                    && a.phase == "planning"
                    && a.input_version == f.input_version
                    && a.status == Status::Accepted
            })
        })
    }
    pub fn validate_evidence(&self, d: &Delivery) -> Result<()> {
        let f = self.feature(&d.feature_id)?;
        evidence_origin(
            &self
                .projects
                .iter()
                .find(|p| p.id == f.project_id)
                .ok_or_else(|| err("not_found", "Project 不存在"))?
                .execution_mode,
            d.origin,
        )?;
        require(
            d.attempt_id.is_some() != d.merge_id.is_some(),
            "Delivery 必须且只能归属 Attempt 或 Merge",
        )?;
        if let Some(aid) = &d.attempt_id {
            let a = self.attempt(aid)?;
            require(
                a.feature_id == d.feature_id
                    && a.task_id == d.task_id
                    && a.input_version == d.input_version,
                "Delivery 与 Attempt 的归属或版本不一致",
            )?;
            require(
                if a.phase == "planning" {
                    ["requirement", "plan"].contains(&d.kind.as_str())
                } else if a.phase == "requirement_definition" {
                    d.kind == "requirement"
                } else if a.phase == "solution_planning" {
                    d.kind == "plan"
                } else {
                    d.kind == a.phase
                },
                "Delivery 阶段与 Attempt 不一致",
            )?;
        }
        if let Some(mid) = &d.merge_id {
            let m = self.merge(mid)?;
            require(
                m.feature_id == d.feature_id
                    && m.task_id == d.task_id
                    && m.input_version == d.input_version
                    && d.kind == "merge_candidate",
                "Delivery 与 Merge 的归属、阶段或版本不一致",
            )?;
        }
        if d.origin == Origin::External && d.kind == "closure" {
            let completed: Vec<_> = self
                .required_tasks(&d.feature_id)
                .into_iter()
                .filter(|t| t.status == Status::Completed)
                .collect();
            require(
                !completed.is_empty() && d.source_deliveries.len() == completed.len(),
                "Feature 收尾必须冻结每个已接纳 Task 的来源交付",
            )?;
            let mut task_ids = std::collections::HashSet::new();
            for id in &d.source_deliveries {
                let source = self.delivery(id)?;
                require(
                    source.kind == "implementation"
                        && source.origin == Origin::External
                        && source.feature_id == d.feature_id
                        && source.task_id.as_ref().is_some_and(|tid| {
                            completed.iter().any(|t| &t.id == tid) && task_ids.insert(tid)
                        }),
                    "Feature 来源交付归属错误或重复",
                )?;
                require(
                    self.merges.iter().any(|m| {
                        m.source_delivery == source.id
                            && m.status == Status::Completed
                            && m.change_request
                                .as_ref()
                                .is_some_and(|pr| pr.provider == "github")
                    }),
                    "来源 Task 尚未完成远端接纳",
                )?;
                self.validate_evidence(source)?;
            }
        }
        if d.origin == Origin::External && d.kind == "merge_candidate" {
            let m = self.merge(d.merge_id.as_deref().unwrap_or(""))?;
            require(
                d.source_deliveries == vec![m.source_delivery.clone()],
                "候选必须冻结实际源交付",
            )?;
        }
        require(!d.documents.is_empty(), "交付缺少文档")?;
        for did in &d.documents {
            require(
                self.documents
                    .iter()
                    .any(|x| &x.id == did && self.document_in_feature(x, &d.feature_id)),
                "交付文档引用不完整",
            )?;
        }
        for doc in self
            .documents
            .iter()
            .filter(|doc| d.documents.contains(&doc.id))
        {
            require(
                doc.task_id.is_none() || doc.task_id == d.task_id,
                "交付不能引用其他 Task 的文档",
            )?;
            if ["design", "test"].contains(&doc.kind.as_str()) {
                require(
                    doc.requirement_id.as_ref().is_some_and(|id| {
                        d.documents.contains(id)
                            && self.documents.iter().any(|req| {
                                &req.id == id
                                    && req.kind == "requirement"
                                    && self.document_in_feature(req, &d.feature_id)
                            })
                    }),
                    "交付中的设计/测试必须引用同次冻结需求",
                )?;
            }
            if doc.kind == "test" {
                require(
                    doc.design_id.as_ref().is_some_and(|id| {
                        d.documents.contains(id)
                            && self.documents.iter().any(|design| {
                                &design.id == id
                                    && design.kind == "design"
                                    && self.document_in_feature(design, &d.feature_id)
                            })
                    }),
                    "交付测试必须引用同次冻结设计",
                )?;
            }
            if doc.kind == "requirement" {
                self.acceptance_keys(&doc.id)?;
            }
            if doc.kind == "test" {
                let req = doc
                    .requirement_id
                    .as_ref()
                    .ok_or_else(|| err("blocked", "Test 缺少需求修订"))?;
                let keys = self.acceptance_keys(req)?;
                require(
                    !doc.verifies.is_empty() && doc.verifies.iter().all(|k| keys.contains(k)),
                    "Test 必须映射有效验收条目",
                )?;
            }
        }
        let has = |kind: &str| {
            d.documents.iter().any(|id| {
                self.documents
                    .iter()
                    .any(|doc| &doc.id == id && doc.kind == kind)
            })
        };
        match d.kind.as_str() {
            "requirement" => require(has("requirement"), "Requirement 交付缺少需求")?,
            "plan" => {
                require(
                    has("requirement") && has("design") && has("test"),
                    "计划交付必须包含 Requirement、Design、Test",
                )?;
                for doc in self
                    .documents
                    .iter()
                    .filter(|doc| d.documents.contains(&doc.id))
                {
                    if ["design", "test"].contains(&doc.kind.as_str()) {
                        require(
                            doc.requirement_id.as_ref().is_some_and(|id| {
                                d.documents.contains(id)
                                    && self
                                        .documents
                                        .iter()
                                        .any(|r| &r.id == id && r.kind == "requirement")
                            }),
                            "设计/测试必须引用本次冻结需求",
                        )?;
                    }
                    if doc.kind == "test" {
                        require(
                            doc.design_id.as_ref().is_some_and(|id| {
                                d.documents.contains(id)
                                    && self
                                        .documents
                                        .iter()
                                        .any(|r| &r.id == id && r.kind == "design")
                            }),
                            "测试必须引用本次冻结设计",
                        )?;
                    }
                }
            }
            "implementation" | "closure" => require(
                has("test") && has("doc"),
                "实现/收尾交付必须包含测试定义与文档",
            )?,
            "merge_candidate" => require(
                d.merge_id.is_some() && !d.evidence.is_empty(),
                "候选交付缺少 Merge 与验证证据",
            )?,
            _ => return Err(err("blocked", "未知正式交付阶段")),
        }
        for rid in &d.evidence {
            let r = self
                .runs
                .iter()
                .find(|r| &r.id == rid)
                .ok_or_else(|| err("blocked", "证据引用不完整"))?;
            evidence_origin(
                &self
                    .projects
                    .iter()
                    .find(|p| p.id == f.project_id)
                    .unwrap()
                    .execution_mode,
                r.origin,
            )?;
            if ["red", "green", "verify", "feature_tests", "merge_verify"]
                .contains(&r.node.as_str())
            {
                let result: TestResult = serde_json::from_value(
                    r.result
                        .clone()
                        .ok_or_else(|| err("blocked", "测试证据缺少结果"))?,
                )
                .map_err(|_| err("blocked", "测试证据结构不完整"))?;
                require(
                    result.operation_id == r.id
                        && result.test_phase == r.node
                        && result.origin == r.origin,
                    "测试证据操作、阶段或来源不匹配",
                )?;
                if r.origin == Origin::External {
                    let instance = self
                        .executions
                        .iter()
                        .find(|e| {
                            e.run_id.as_ref() == Some(&r.id)
                                && e.kind == RuntimeExecutionKind::Tests
                        })
                        .ok_or_else(|| err("blocked", "真实测试缺少执行实例"))?;
                    require(
                        result.environment == instance.id
                            && instance.writer_revoked
                            && instance.protocol_artifact.is_some(),
                        "真实测试证据缺少环境/停止/封存证明",
                    )?;
                    let raw = r.result.as_ref().unwrap();
                    require(
                        raw["container_id"].as_str() == instance.container_id.as_deref()
                            && raw["image_digest"] == instance.image_digest,
                        "真实测试容器/镜像证据不符",
                    )?;
                } else {
                    let instance = self
                        .sessions
                        .iter()
                        .find(|session| session.id == r.session_id)
                        .and_then(|session| session.instance.as_ref())
                        .ok_or_else(|| err("blocked", "测试证据缺少确定环境实例"))?;
                    require(result.environment == instance.id, "测试证据环境不匹配")?;
                }
                let expected = if r.node == "red" {
                    d.attempt_id
                        .as_deref()
                        .and_then(|id| self.attempt(id).ok())
                        .and_then(|a| a.baseline.as_ref())
                } else {
                    d.candidate.as_ref()
                };
                require(
                    expected.is_some_and(|revision| revision == &result.candidate),
                    "测试证据代码版本不匹配",
                )?;
                require(
                    !result.test_definitions.is_empty()
                        && result.test_definitions.iter().all(|id| {
                            d.documents.contains(id)
                                && self
                                    .documents
                                    .iter()
                                    .any(|test| &test.id == id && test.kind == "test")
                        }),
                    "测试结果必须引用交付内的测试定义",
                )?;
            }
            require(
                r.attempt_id == d.attempt_id && r.merge_id == d.merge_id,
                "证据不属于本次交付",
            )?;
        }
        if ["plan", "closure"].contains(&d.kind.as_str()) {
            for req in self
                .documents
                .iter()
                .filter(|doc| d.documents.contains(&doc.id) && doc.kind == "requirement")
            {
                let keys = self.acceptance_keys(&req.id)?;
                require(
                    keys.iter().all(|key| {
                        self.documents.iter().any(|test| {
                            d.documents.contains(&test.id)
                                && test.kind == "test"
                                && test.requirement_id.as_ref() == Some(&req.id)
                                && test.verifies.contains(key)
                        })
                    }),
                    "计划/收尾测试未覆盖全部验收条目",
                )?;
            }
        }
        if d.kind == "implementation" {
            let attempt = self.attempt(
                d.attempt_id
                    .as_deref()
                    .ok_or_else(|| err("blocked", "实现交付缺少 Attempt"))?,
            )?;
            let scope = attempt
                .scope
                .as_ref()
                .ok_or_else(|| err("blocked", "历史 Attempt 缺少冻结范围，需新建执行"))?;
            self.validate_scope(&d.feature_id, scope)?;
            require(
                scope.acceptance.iter().all(|key| {
                    self.documents.iter().any(|test| {
                        d.documents.contains(&test.id)
                            && test.kind == "test"
                            && test.task_id == d.task_id
                            && test.requirement_id == scope.requirement
                            && test.design_id == scope.design
                            && test.verifies.contains(key)
                    })
                }),
                "Task 测试未覆盖冻结的实现范围",
            )?;
            let run = |node: &str| {
                self.runs
                    .iter()
                    .find(|r| d.evidence.contains(&r.id) && r.node == node)
                    .ok_or_else(|| err("blocked", format!("缺少 TDD {node} 证据")))
            };
            let red = run("red")?;
            let implementation = run("implement")?;
            let green = run("green")?;
            let verify = run("verify")?;
            require(
                red.status == Status::Succeeded
                    && implementation.status == Status::Succeeded
                    && verify.status == Status::Succeeded,
                "TDD 结构证据不完整",
            )?;
            require(
                red.result.as_ref().is_some_and(|v| {
                    v["expected_failure"] == true && v["assertion_failed"] == true
                }),
                "Red 必须是预期断言失败",
            )?;
            require(
                red.finished
                    .zip(implementation.started)
                    .is_some_and(|(a, b)| a <= b)
                    && implementation
                        .finished
                        .zip(green.started)
                        .is_some_and(|(a, b)| a <= b)
                    && green
                        .finished
                        .zip(verify.started)
                        .is_some_and(|(a, b)| a <= b),
                "TDD 证据顺序不成立",
            )?;
            let rr = red
                .result
                .as_ref()
                .ok_or_else(|| err("blocked", "Red 无结果"))?;
            let gr = green
                .result
                .as_ref()
                .ok_or_else(|| err("blocked", "Green 无结果"))?;
            require(
                rr["test_definitions"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
                    && rr["test_definitions"] == gr["test_definitions"],
                "Red/Green 测试定义不一致",
            )?;
            require(
                d.candidate.as_ref().is_some_and(|c| {
                    gr["candidate"] == *c
                        && d.attempt_id
                            .as_deref()
                            .and_then(|id| self.attempt(id).ok())
                            .and_then(|a| a.baseline.as_ref())
                            .is_some_and(|base| rr["candidate"] == *base)
                }),
                "TDD 候选版本不一致",
            )?;
        }
        if d.kind == "closure" {
            for node in ["feature_tests", "docs", "verify"] {
                require(
                    d.evidence.iter().any(|id| {
                        self.runs
                            .iter()
                            .any(|run| &run.id == id && run.node == node)
                    }),
                    "收尾缺少完整测试、文档或验证证据",
                )?;
            }
        }
        if let Some(mid) = &d.merge_id {
            let m = self.merge(mid)?;
            require(
                d.candidate.as_deref() == Some(&m.candidate),
                "Merge 证据候选不一致",
            )?;
            require(
                self.accepted(self.delivery(&m.source_delivery)?),
                "源交付已失效",
            )?;
            for rid in &d.evidence {
                let r = self.runs.iter().find(|r| &r.id == rid).unwrap();
                require(
                    r.result
                        .as_ref()
                        .is_some_and(|v| v["candidate"] == m.candidate),
                    "候选验证版本不一致",
                )?;
            }
        }
        Ok(())
    }
    pub fn validate_binding(&self, b: &BindingLock) -> Result<()> {
        for (id, family) in std::iter::once((&b.agent, "agent"))
            .chain(std::iter::once((&b.standard, "standard")))
            .chain(b.skills.iter().map(|x| (x, "skill")))
        {
            require(
                self.definitions
                    .iter()
                    .any(|d| &d.id == id && d.family == family),
                "绑定必须引用存在的确定定义版本",
            )?;
        }
        let standard = self
            .definitions
            .iter()
            .find(|d| d.id == b.standard)
            .ok_or_else(|| err("blocked", "Standard 缺失"))?;
        require(
            ["planning", "implementation", "merge"]
                .iter()
                .all(|purpose| {
                    standard.slots.iter().any(|slot| {
                        &slot.purpose == purpose
                            && self.definitions.iter().any(|d| {
                                d.id == slot.spec && d.family == "spec" && !d.rules.is_empty()
                            })
                    })
                }),
            "Standard 必须提供规划、实现、Merge 的 Spec 用途 slot",
        )?;
        let harness = self
            .harnesses
            .iter()
            .find(|h| h.id == b.harness)
            .ok_or_else(|| err("blocked", "Harness 定义缺失"))?;
        let sandbox = self
            .sandboxes
            .iter()
            .find(|env| env.id == b.sandbox)
            .ok_or_else(|| err("blocked", "Sandbox 定义缺失"))?;
        require(
            ["structured-output", "cancel", "poll"]
                .iter()
                .all(|cap| harness.capabilities.iter().any(|value| value == cap)),
            "Harness 缺少执行/取消/结构化结果能力",
        )?;
        let external = ["codex", "dsh"].contains(&harness.provider.as_str())
            && harness.origin == Origin::External
            && sandbox.provider == "docker"
            && sandbox.origin == Origin::External
            && sandbox.image.as_ref().is_some_and(|id| {
                id.strip_prefix("sha256:")
                    .or_else(|| {
                        id.rsplit_once("@sha256:")
                            .filter(|(name, _)| {
                                !name.is_empty() && !name.chars().any(char::is_whitespace)
                            })
                            .map(|(_, digest)| digest)
                    })
                    .is_some_and(|digest| {
                        digest.len() == 64
                            && digest
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
            });
        require(external, "绑定必须是原生 Harness 与固定 Docker 镜像能力")
    }
}
pub fn valid_dag(nodes: &[(String, Vec<String>)]) -> Result<()> {
    let mut done = vec![];
    require(
        nodes
            .iter()
            .map(|x| &x.0)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == nodes.len(),
        "DAG 节点重复",
    )?;
    loop {
        let next = nodes
            .iter()
            .filter(|(n, deps)| !done.contains(n) && deps.iter().all(|d| done.contains(d)))
            .map(|x| x.0.clone())
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        done.extend(next);
    }
    require(done.len() == nodes.len(), "DAG 有环或引用不存在的节点")
}
pub fn evidence_origin(mode: &str, origin: Origin) -> Result<()> {
    require(
        mode == "simulation" || origin != Origin::Simulated,
        "历史模拟证据不能满足原生执行门禁",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cycles_and_missing_dependencies() {
        assert!(
            valid_dag(&[
                ("a".into(), vec!["b".into()]),
                ("b".into(), vec!["a".into()])
            ])
            .is_err()
        );
        assert!(valid_dag(&[("a".into(), vec!["missing".into()])]).is_err());
    }
    #[test]
    fn allows_parallel_dag() {
        assert!(
            valid_dag(&[
                ("a".into(), vec![]),
                ("b".into(), vec![]),
                ("c".into(), vec!["a".into(), "b".into()])
            ])
            .is_ok()
        );
    }
    #[test]
    fn historical_provenance_is_readable_but_cannot_satisfy_native_gates() {
        assert!(evidence_origin("live", Origin::Simulated).is_err());
        assert!(evidence_origin("simulation", Origin::Simulated).is_ok());
    }
}

/// Capture provenance; legacy migration intentionally does not invent a Session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotCapture {
    pub attempt: Option<Id>,
    pub runs: Vec<Id>,
    pub sessions: Vec<Id>,
    pub resources: Vec<Id>,
    pub captured_at: Option<u64>,
    pub reason: String,
}
impl EngineeringState {
    pub fn acceptance_keys(&self, requirement: &str) -> Result<Vec<String>> {
        let req = self
            .documents
            .iter()
            .find(|d| d.id == requirement && d.kind == "requirement")
            .ok_or_else(|| err("blocked", "需求修订不存在"))?;
        require(!req.acceptance.is_empty(), "需求必须定义结构化验收条目")?;
        let keys: Vec<_> = req.acceptance.iter().map(|c| c.key.clone()).collect();
        require(
            keys.iter().collect::<std::collections::HashSet<_>>().len() == keys.len()
                && req
                    .acceptance
                    .iter()
                    .all(|c| !c.key.trim().is_empty() && !c.description.trim().is_empty()),
            "验收条目标识必须唯一且非空",
        )?;
        Ok(keys)
    }
    pub fn validate_scope(&self, fid: &str, scope: &TaskScope) -> Result<()> {
        let req = scope
            .requirement
            .as_ref()
            .ok_or_else(|| err("blocked", "Task 缺少需求范围"))?;
        let design = scope
            .design
            .as_ref()
            .ok_or_else(|| err("blocked", "Task 缺少设计范围"))?;
        let keys = self.acceptance_keys(req)?;
        require(
            self.documents
                .iter()
                .any(|d| &d.id == req && self.document_in_feature(d, fid)),
            "Task 需求归属错误",
        )?;
        require(
            self.documents.iter().any(|d| {
                &d.id == design
                    && self.document_in_feature(d, fid)
                    && d.kind == "design"
                    && d.requirement_id.as_ref() == Some(req)
            }),
            "Task 设计与需求版本不匹配",
        )?;
        require(
            !scope.acceptance.is_empty() && scope.acceptance.iter().all(|k| keys.contains(k)),
            "Task 范围必须引用已定义验收条目",
        )
    }
}

impl EngineeringState {
    /// Agent revisions share one identity; a new revision is not an independent reviewer.
    pub fn agent_identity(&self, revision: &str) -> Result<Id> {
        let mut current = revision;
        let mut seen = std::collections::HashSet::new();
        loop {
            require(seen.insert(current.to_owned()), "Agent 修订链有环")?;
            let definition = self
                .definitions
                .iter()
                .find(|d| d.id == current && d.family == "agent")
                .ok_or_else(|| err("blocked", "Agent 身份修订缺失"))?;
            match definition.supersedes.as_deref() {
                Some(previous) => current = previous,
                None => return Ok(definition.id.clone()),
            }
        }
    }
}
