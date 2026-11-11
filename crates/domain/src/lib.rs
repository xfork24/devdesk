mod standards;
pub use standards::*;
mod source;
pub use source::*;
mod integrations;
pub use integrations::*;
mod configuration;
pub use configuration::*;
mod orchestration;
mod template_catalog;
mod workflow;
mod workflow_change;
pub use orchestration::*;
pub use template_catalog::*;
pub use workflow::*;
pub use workflow_change::*;
mod deliverable;
pub use deliverable::*;
mod agent_task;
pub use agent_task::*;
mod permission;
pub use permission::*;
mod permission_profile;
pub use permission_profile::*;
mod testing;
pub use testing::*;
mod repository;
pub use repository::*;
mod execution;
pub use execution::*;
mod runtime;
pub use runtime::*;
mod model_provider;
pub use model_provider::*;
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
/// Isolated execution namespace for dataset-wide discussions, never a Project row.
pub const GLOBAL_DISCUSSION_NAMESPACE: &str = "00000000-0000-4000-8000-000000000000";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialOwner {
    Global,
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
        (None, None, None) => Ok(MaterialOwner::Global),
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
    Completed,
    Prepared,
    Running,
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
    External,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub title: String,
    pub workspace: String,
    pub workspace_definition: Option<Workspace>,
    pub execution_mode: String,
    pub concurrency: u32,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: Id,
    pub project_id: Id,
    pub title: String,
    pub status: Status,
    pub revision: u64,
    pub input_version: u64,
    pub predecessor: Option<Id>,
    #[serde(default)]
    pub source_materials: Vec<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub feature_id: Id,
    pub title: String,
    pub status: Status,
    pub revision: u64,
    pub inherited_snapshot: Option<Id>,
    pub dependencies: Vec<Id>,
    pub worktree: String,
    pub scope: TaskScope,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskScope {
    #[serde(default)]
    pub input_refs: Vec<Id>,
    pub acceptance: Vec<String>,
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
    #[serde(default)]
    pub input_refs: Vec<Id>,
    pub origin: Origin,
    pub artifact_id: Option<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub id: Id,
    pub family: String,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub body: String,
    #[serde(default)]
    pub agent_profile: Option<AgentProfile>,
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
    pub documents: Vec<Id>,
    pub inherited_from: Option<Id>,
    pub capture: SnapshotCapture,
    #[serde(default)]
    pub runtime_bindings: Vec<RuntimeBinding>,
    #[serde(default)]
    pub execution_instances: Vec<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHead {
    pub id: Id,
    pub project_id: Id,
    pub revision: String,
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
    pub orchestration_wakeups: Vec<OrchestrationWakeup>,
    #[serde(default)]
    pub orchestration_leases: Vec<OrchestrationLease>,
    #[serde(default)]
    pub governance: Vec<ConfigurationRecord>,
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
    #[serde(skip)]
    pub harnesses: Vec<Harness>,
    #[serde(skip)]
    pub sandboxes: Vec<Sandbox>,
    pub projects: Vec<Project>,
    pub features: Vec<Feature>,
    pub tasks: Vec<Task>,
    pub documents: Vec<Document>,
    #[serde(skip)]
    pub definitions: Vec<Definition>,
    pub sessions: Vec<Session>,
    pub contexts: Vec<Context>,
    pub snapshots: Vec<Snapshot>,
    pub heads: Vec<GitHead>,
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
                    && f.source_materials.contains(&document.id)
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
    fn only_current_provenance_is_accepted() {
        assert!(serde_json::from_str::<Origin>("\"simulated\"").is_err());
        assert_eq!(
            serde_json::from_str::<Origin>("\"external\"").unwrap(),
            Origin::External
        );
    }
}

/// Exact provenance of the captured execution context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotCapture {
    #[serde(default)]
    pub step_execution_ids: Vec<Id>,
    pub sessions: Vec<Id>,
    pub captured_at: Option<u64>,
    pub reason: String,
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
