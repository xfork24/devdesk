//! Pure domain model: no I/O or adapter dependencies.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Id = String;
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
pub fn require(ok: bool, message: &str) -> Result<()> {
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
    Simulated,
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
    pub id: Id,
    pub title: String,
    pub workspace: String,
    pub execution_mode: String,
    pub binding: BindingLock,
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
    pub binding: BindingLock,
    pub predecessor: Option<Id>,
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub requirement_id: Option<Id>,
    pub design_id: Option<Id>,
    pub origin: Origin,
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub phase: String,
    pub status: Status,
    pub input_version: u64,
    pub documents: Vec<Id>,
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
    pub writes: bool,
    pub worktree: String,
    pub outcome: String,
    pub cancel_requested: bool,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub result: Option<Value>,
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
    pub origin: Origin,
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
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub kind: String,
    pub content: String,
    pub binding: BindingLock,
    pub documents: Vec<Id>,
    pub delivery_id: Option<Id>,
    pub inherited_from: Option<Id>,
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
    pub feature_id: Id,
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
        if d.stale
            || self
                .feature(&d.feature_id)
                .is_ok_and(|f| f.input_version != d.input_version)
        {
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
        require(!d.documents.is_empty(), "交付缺少文档")?;
        for did in &d.documents {
            require(
                self.documents
                    .iter()
                    .any(|x| &x.id == did && x.feature_id == d.feature_id),
                "交付文档引用不完整",
            )?;
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
            require(
                r.attempt_id == d.attempt_id && r.merge_id == d.merge_id,
                "证据不属于本次交付",
            )?;
        }
        if d.kind == "implementation" {
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
                d.candidate
                    .as_ref()
                    .is_some_and(|c| gr["candidate"] == *c && rr["candidate"] == *c),
                "TDD 候选版本不一致",
            )?;
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
        require(
            b.harness == "sim-harness-v1" && b.sandbox == "sim-sandbox-v1",
            "首版仅支持模拟 Harness/Sandbox",
        )
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
        "模拟证据不能满足 live 门禁",
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
    fn simulated_never_live() {
        assert!(evidence_origin("live", Origin::Simulated).is_err());
        assert!(evidence_origin("simulation", Origin::Simulated).is_ok());
    }
}
