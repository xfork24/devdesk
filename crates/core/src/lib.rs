//! Application use cases and ports. Adapters cannot advance domain state.
use devdesk_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub trait EngineeringStore: Send + Sync {
    fn snapshot(&self) -> Result<EngineeringState>;
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value>;
    fn root(&self) -> String;
}
pub trait ArtifactStore: Send + Sync {
    fn seal(
        &self,
        feature: &str,
        task: Option<&str>,
        category: &str,
        body: &str,
    ) -> Result<Artifact>;
    fn read(&self, artifact: &Artifact) -> Result<Vec<u8>>;
}
pub trait HarnessPort: Send + Sync {
    fn configure(&self, node: &str, scenario: &str) -> String;
    fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>>;
}
pub trait GitPort: Send + Sync {
    fn candidate(&self, source: &str, target: &str, operation: &str) -> Result<String>;
    fn advance(&self, actual: &str, expected: &str, candidate: &str) -> Result<String>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRequest {
    pub phase: String,
    pub candidate: String,
    pub test_definitions: Vec<Id>,
    pub environment: String,
    pub origin: Origin,
}
pub trait TestRunnerPort: Send + Sync {
    fn validate(&self, request: &TestRequest, outcome: &str) -> Value;
}
pub trait SandboxPort: Send + Sync {
    fn resource(&self, run_id: &str) -> Resource;
    fn cleanup(&self, resource: &mut Resource, fail: bool);
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub command_id: String,
    pub target: Option<String>,
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub payload: Value,
}
pub struct Engine {
    pub store: Arc<dyn EngineeringStore>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub harness: Arc<dyn HarnessPort>,
    pub git: Arc<dyn GitPort>,
    pub tests: Arc<dyn TestRunnerPort>,
    pub sandbox: Arc<dyn SandboxPort>,
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
            .find(|d| d.family == "agent" && d.name == "工程 Agent")
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
        harness: "sim-harness-v1".into(),
        sandbox: "sim-sandbox-v1".into(),
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
fn document_ids(s: &EngineeringState, fid: &str, tid: Option<&str>) -> Vec<Id> {
    s.documents
        .iter()
        .filter(|d| {
            d.feature_id == fid
                && (d.task_id.is_none() || d.task_id.as_deref() == tid)
                && !s
                    .documents
                    .iter()
                    .any(|n| n.supersedes.as_deref() == Some(&d.id))
        })
        .map(|d| d.id.clone())
        .collect()
}
impl Engine {
    pub fn bootstrap(&self) -> Result<()> {
        self.store.transact(None, &mut |s| {
            if s.definitions.is_empty() {
                for (family, name, version, body) in [
                    (
                        "spec",
                        "Internal Design Spec",
                        1,
                        "需求可追踪；设计说明边界；测试覆盖验收条件",
                    ),
                    (
                        "spec",
                        "Internal Design Spec",
                        2,
                        "需求可追踪；设计说明边界；测试覆盖验收条件；明确失败路径",
                    ),
                    (
                        "spec",
                        "Alternative Design Spec",
                        1,
                        "按问题、约束、方案、验证组织设计",
                    ),
                    (
                        "skill",
                        "TDD",
                        1,
                        "Red → 实现 → Green；证据绑定候选与测试定义",
                    ),
                    ("agent", "工程 Agent", 1, "模拟工程执行身份"),
                    ("agent", "评审 Agent", 1, "独立模拟评审身份"),
                    (
                        "standard",
                        "工程默认 Standard",
                        1,
                        "规划、实现、候选交付均需证据与 Review",
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
                    s.definitions.push(Definition {
                        id: id(),
                        family: family.into(),
                        name: name.into(),
                        version,
                        supersedes: None,
                        body: body.into(),
                        slots,
                        rules,
                    });
                }
            }
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
                for res in s
                    .resources
                    .iter_mut()
                    .filter(|x| x.run_id == rid && x.status == Status::Active)
                {
                    self.sandbox.cleanup(res, false);
                }
                event(
                    s,
                    &rid,
                    "run.interrupted",
                    "内核重启，执行结果未确认；需要创建重试",
                );
            }
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    pub fn execute(&self, method: &str, c: Command) -> Result<Value> {
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let fingerprint =
            serde_json::to_string(&(method, &c.target, &c.expected_revision, &c.payload)).unwrap();
        let mut action = |s: &mut EngineeringState| {
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
            let p = &c.payload;
            let result = match method {
                "project.create" => {
                    let pid = id();
                    let title = text(p, "title")?;
                    let b = binding(s);
                    s.projects.push(Project {
                        id: pid.clone(),
                        title,
                        workspace: opt(p, "workspace").unwrap_or_default(),
                        execution_mode: "simulation".into(),
                        binding: b,
                        concurrency: 2,
                        revision: 1,
                    });
                    s.heads.push(GitHead {
                        id: format!("main:{pid}"),
                        project_id: pid.clone(),
                        revision: "sim-rev-root".into(),
                    });
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
                    if let Some(workspace) = opt(p, "workspace") {
                        project.workspace = workspace;
                    }
                    if let Some(n) = p.get("concurrency").and_then(Value::as_u64) {
                        require((1..=32).contains(&n), "并发额度需为 1—32")?;
                        project.concurrency = n as u32;
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
                    let fid = id();
                    let b = project.binding.clone();
                    let predecessor = opt(p, "predecessor");
                    if let Some(prev) = &predecessor {
                        require(
                            s.feature(prev)?.project_id == pid,
                            "前序 Feature 必须属于相同 Project",
                        )?;
                    }
                    s.features.push(Feature {
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
                        project_id: pid,
                        revision: "sim-rev-root".into(),
                    });
                    s.contexts.push(Context {
                        id: id(),
                        owner_id: fid.clone(),
                        content: "Feature 初始上下文".into(),
                        revision: 1,
                    });
                    json!({"id":fid})
                }
                "document.revise" => {
                    let fid = if s.features.iter().any(|f| f.id == target) {
                        target.to_string()
                    } else {
                        text(p, "feature_id")?
                    };
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
                    let old = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| d.feature_id == fid && d.task_id == task_id && d.kind == kind)
                        .cloned();
                    let did = id();
                    let mut artifact = self.artifacts.seal(
                        &fid,
                        task_id.as_deref(),
                        "documents",
                        &text(p, "body")?,
                    )?;
                    artifact.origin = Origin::Human;
                    s.artifacts.push(artifact);
                    let req = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| d.feature_id == fid && d.kind == "requirement")
                        .map(|d| d.id.clone());
                    let design = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| d.feature_id == fid && d.kind == "design")
                        .map(|d| d.id.clone());
                    s.documents.push(Document {
                        id: did.clone(),
                        feature_id: fid.clone(),
                        task_id,
                        kind,
                        title: opt(p, "title").unwrap_or_else(|| "修订文档".into()),
                        body: text(p, "body")?,
                        version: old.as_ref().map_or(1, |d| d.version + 1),
                        supersedes: old.map(|d| d.id),
                        requirement_id: req,
                        design_id: design,
                        origin: Origin::Human,
                    });
                    let f = s.features.iter_mut().find(|f| f.id == fid).unwrap();
                    f.input_version += 1;
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
                    for d in s.deliveries.iter_mut().filter(|d| {
                        d.feature_id == fid
                            && d.task_id.as_ref().is_none_or(|id| !completed.contains(id))
                    }) {
                        d.stale = true;
                    }
                    for a in s.attempts.iter_mut().filter(|a| {
                        a.feature_id == fid
                            && [Status::Prepared, Status::Running, Status::WaitingReview]
                                .contains(&a.status)
                    }) {
                        a.paused = true;
                        a.revision += 1;
                    }
                    for t in s
                        .tasks
                        .iter_mut()
                        .filter(|t| t.feature_id == fid && t.status != Status::Completed)
                    {
                        if [Status::Accepted, Status::Merging].contains(&t.status) {
                            t.status = Status::Ready;
                            t.revision += 1;
                        }
                    }
                    self.invalidate_merges(s, &fid, None);
                    event(
                        s,
                        &fid,
                        "evidence.stale",
                        "输入修订，旧证据保留但不再满足当前门禁",
                    );
                    json!({"id":did})
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
                        .find(|x| x.feature_id == f.id && x.kind == "initial")
                        .cloned()
                        .ok_or_else(|| err("blocked", "缺少初始 Snapshot"))?;
                    let deps: Vec<String> =
                        serde_json::from_value(p.get("dependencies").cloned().unwrap_or(json!([])))
                            .map_err(|_| err("invalid_params", "dependencies 应为 Task ID 列表"))?;
                    for dep in &deps {
                        require(s.task(dep)?.feature_id == f.id, "依赖必须属于同 Feature")?;
                    }
                    let tid = id();
                    s.tasks.push(Task {
                        id: tid.clone(),
                        feature_id: f.id.clone(),
                        title: text(p, "title")?,
                        status: Status::Ready,
                        revision: 1,
                        binding: f.binding,
                        inherited_snapshot: snap.id.clone(),
                        dependencies: deps,
                        worktree: format!("logical-worktree/{tid}"),
                    });
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
                        .find(|d| d.feature_id == f.id && d.kind == "requirement")
                        .map(|d| d.id.clone());
                    let design = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| d.feature_id == f.id && d.kind == "design")
                        .map(|d| d.id.clone());
                    s.documents.push(Document {
                        id: id(),
                        feature_id: f.id.clone(),
                        task_id: Some(tid.clone()),
                        kind: "test".into(),
                        title: "Task 验收测试定义".into(),
                        body: s
                            .documents
                            .iter()
                            .rev()
                            .find(|d| {
                                d.feature_id == f.id && d.kind == "test" && d.task_id.is_none()
                            })
                            .ok_or_else(|| err("blocked", "规划缺少测试定义"))?
                            .body
                            .clone(),
                        version: 1,
                        supersedes: None,
                        requirement_id: req,
                        design_id: design,
                        origin: Origin::Simulated,
                    });
                    let fm = s.features.iter_mut().find(|x| x.id == f.id).unwrap();
                    fm.status = Status::Developing;
                    fm.revision += 1;
                    json!({"id":tid})
                }
                "attempt.start" => {
                    let (fid, tid, phase, b, worktree) = if let Some(t) =
                        s.tasks.iter().find(|t| t.id == target).cloned()
                    {
                        mutable_feature(s, &t.feature_id)?;
                        require(t.status != Status::Completed, "Task 已完成")?;
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
                            ["planning", "closure"].contains(&phase.as_str()),
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
                            let ts = s
                                .tasks
                                .iter()
                                .filter(|t| t.feature_id == f.id)
                                .collect::<Vec<_>>();
                            require(
                                !ts.is_empty() && ts.iter().all(|t| t.status == Status::Completed),
                                "全部必需 Task 完成后才能收尾",
                            )?;
                        }
                        (
                            f.id.clone(),
                            None,
                            phase,
                            f.binding,
                            format!("logical-feature/{}", f.id),
                        )
                    };
                    require(
                        !s.attempts.iter().any(|a| {
                            a.feature_id == fid
                                && a.task_id == tid
                                && [Status::Running, Status::Prepared, Status::WaitingReview]
                                    .contains(&a.status)
                                && a.input_version == s.feature(&fid).unwrap().input_version
                        }),
                        "已有活动 Attempt；请等待评审或取消",
                    )?;
                    require(
                        !s.runs.iter().any(|r| {
                            r.worktree == worktree
                                && r.status == Status::Running
                                && r.cancel_requested
                        }),
                        "等待旧 Run 取消确认后再启动",
                    )?;
                    s.validate_binding(&b)?;
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
                        id: aid.clone(),
                        feature_id: fid.clone(),
                        task_id: tid.clone(),
                        phase: phase.clone(),
                        status: Status::Running,
                        input_version: input,
                        documents,
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
                    let nodes: Vec<(&str, Vec<&str>, bool)> = match phase.as_str() {
                        "planning" => vec![
                            ("research", vec![], false),
                            ("requirement", vec!["research"], true),
                            ("design", vec!["requirement"], true),
                            ("task_plan", vec!["design"], false),
                            ("test_plan", vec!["design"], false),
                        ],
                        "implementation" => vec![
                            ("red", vec![], false),
                            ("implement", vec!["red"], true),
                            ("green", vec!["implement"], false),
                            ("docs", vec!["implement"], true),
                            ("verify", vec!["green", "docs"], false),
                        ],
                        _ => vec![
                            ("feature_tests", vec![], false),
                            ("docs", vec![], true),
                            ("verify", vec!["feature_tests", "docs"], false),
                        ],
                    };
                    valid_dag(
                        &nodes
                            .iter()
                            .map(|(n, d, _)| {
                                (n.to_string(), d.iter().map(|s| s.to_string()).collect())
                            })
                            .collect::<Vec<_>>(),
                    )?;
                    for (node, deps, writes) in nodes {
                        self.add_run(
                            s,
                            Some(&aid),
                            None,
                            node,
                            deps.iter().map(|s| s.to_string()).collect(),
                            writes,
                            &worktree,
                            &b,
                            None,
                            opt(p, "scenario").as_deref().unwrap_or("success"),
                        );
                    }
                    if let Some(tid) = tid {
                        let t = s.tasks.iter_mut().find(|x| x.id == tid).unwrap();
                        t.status = Status::Running;
                        t.revision += 1;
                    } else {
                        let f = s.features.iter_mut().find(|x| x.id == fid).unwrap();
                        f.status = if phase == "planning" {
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
                "run.cancel" | "run.continue" => {
                    let r = s
                        .runs
                        .iter_mut()
                        .find(|r| r.id == target)
                        .ok_or_else(|| err("not_found", "Run 不存在"))?;
                    require(
                        [Status::Queued, Status::Running].contains(&r.status),
                        "Run 已结束",
                    )?;
                    if method == "run.continue" {
                        require(r.outcome == "hold", "该 Run 未处于受控等待点")?;
                        r.outcome = "success".into();
                    } else if r.status == Status::Queued {
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
                        [Status::Failed, Status::Cancelled, Status::Interrupted]
                            .contains(&old.status),
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
                    let b = if let Some(aid) = &old.attempt_id {
                        let a = s.attempt(aid)?.clone();
                        require(
                            a.input_version == s.feature(&a.feature_id)?.input_version,
                            "输入已改变，必须创建新 Attempt",
                        )?;
                        require(
                            ![Status::Rejected, Status::Accepted, Status::Cancelled]
                                .contains(&a.status),
                            "Attempt 已终结，需创建新 Attempt",
                        )?;
                        let am = s.attempts.iter_mut().find(|x| x.id == *aid).unwrap();
                        am.status = Status::Running;
                        am.revision += 1;
                        a.binding
                    } else {
                        let m = s.merge(old.merge_id.as_deref().unwrap())?;
                        require(
                            m.status == Status::Running
                                && m.input_version == s.feature(&m.feature_id)?.input_version
                                && s.accepted(s.delivery(&m.source_delivery)?)
                                && s.heads
                                    .iter()
                                    .any(|h| h.id == m.target && h.revision == m.expected_target),
                            "Merge 已失效或结束，请重新构造候选",
                        )?;
                        s.feature(&m.feature_id)?.binding.clone()
                    };
                    let rid = self.add_run(
                        s,
                        old.attempt_id.as_deref(),
                        old.merge_id.as_deref(),
                        &old.node,
                        old.dependencies.clone(),
                        old.writes,
                        &old.worktree,
                        &b,
                        Some(old.id.clone()),
                        "success",
                    );
                    for r in descendants {
                        if r.status != Status::Queued {
                            self.add_run(
                                s,
                                r.attempt_id.as_deref(),
                                r.merge_id.as_deref(),
                                &r.node,
                                r.dependencies,
                                r.writes,
                                &r.worktree,
                                &b,
                                Some(r.id),
                                "success",
                            );
                        }
                    }
                    json!({"id":rid,"operation_id":rid})
                }
                "delivery.submit" => {
                    let a = s.attempt(target)?.clone();
                    require(
                        [Status::WaitingReview, Status::Failed].contains(&a.status),
                        "执行尚未结束",
                    )?;
                    require(
                        a.input_version == s.feature(&a.feature_id)?.input_version,
                        "Attempt 输入已失效，需新 Attempt",
                    )?;
                    let kind = if a.phase == "planning" {
                        opt(p, "kind").unwrap_or_else(|| "requirement".into())
                    } else {
                        a.phase.clone()
                    };
                    require(
                        (a.phase == "planning" && ["requirement", "plan"].contains(&kind.as_str()))
                            || kind == a.phase,
                        "无效交付边界",
                    )?;
                    require(
                        !s.deliveries
                            .iter()
                            .any(|d| d.attempt_id.as_deref() == Some(target) && d.kind == kind),
                        "该边界已提交，修订需新 Attempt",
                    )?;
                    let runs = effective_runs(s, &a);
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
                    let docs = document_ids(s, &a.feature_id, a.task_id.as_deref());
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
                        Some(format!("sim-rev-source-{}", a.id)),
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
                    let mut b = s.feature(&d.feature_id)?.binding.clone();
                    let author = if let Some(aid) = &d.attempt_id {
                        s.attempt(aid)?.binding.agent.clone()
                    } else {
                        let m = s.merge(d.merge_id.as_deref().unwrap())?;
                        let source = s.delivery(&m.source_delivery)?;
                        s.attempt(source.attempt_id.as_deref().unwrap())?
                            .binding
                            .agent
                            .clone()
                    };
                    b.agent = s
                        .definitions
                        .iter()
                        .find(|x| x.family == "agent" && x.id != author)
                        .ok_or_else(|| err("blocked", "缺少独立 Reviewer 身份"))?
                        .id
                        .clone();
                    let rid = self.add_run(
                        s,
                        d.attempt_id.as_deref(),
                        d.merge_id.as_deref(),
                        &format!("review:{}", d.id),
                        vec![],
                        false,
                        "review-readonly",
                        &b,
                        None,
                        opt(p, "scenario").as_deref().unwrap_or("success"),
                    );
                    json!({"id":rid,"operation_id":rid})
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
                        !d.stale && d.input_version == s.feature(&d.feature_id)?.input_version,
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
                    let items: Vec<String> = serde_json::from_value(
                        p.get("items").cloned().unwrap_or(json!(["review"])),
                    )
                    .map_err(|_| err("invalid_params", "items 应为条目列表"))?;
                    require(!items.is_empty(), "必须选择豁免条目")?;
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
                    let (fid, tid) = if let Some(t) = s.tasks.iter().find(|x| x.id == target) {
                        require(
                            [Status::Accepted, Status::Merging].contains(&t.status),
                            "Task 交付尚未接受",
                        )?;
                        (t.feature_id.clone(), Some(t.id.clone()))
                    } else {
                        let f = s.feature(target)?;
                        require(
                            [Status::Accepted, Status::Merging].contains(&f.status),
                            "Feature 收尾交付尚未接受",
                        )?;
                        (f.id.clone(), None)
                    };
                    mutable_feature(s, &fid)?;
                    let f = s.feature(&fid)?.clone();
                    require(
                        !s.merges.iter().any(|m| {
                            m.feature_id == fid
                                && m.task_id == tid
                                && [Status::Running, Status::WaitingReview, Status::Accepted]
                                    .contains(&m.status)
                                && m.input_version == f.input_version
                                && s.delivery(&m.source_delivery).is_ok_and(|d| s.accepted(d))
                                && !s
                                    .deliveries
                                    .iter()
                                    .any(|d| d.merge_id.as_deref() == Some(&m.id) && d.stale)
                                && s.heads
                                    .iter()
                                    .any(|h| h.id == m.target && h.revision == m.expected_target)
                        }),
                        "已有有效 Merge，请查看该候选",
                    )?;
                    let mid = id();
                    let target_head = if tid.is_some() {
                        fid.clone()
                    } else {
                        format!("main:{}", f.project_id)
                    };
                    let expected = s
                        .heads
                        .iter()
                        .find(|h| h.id == target_head)
                        .unwrap()
                        .revision
                        .clone();
                    let source_delivery = s
                        .deliveries
                        .iter()
                        .rev()
                        .find(|d| {
                            d.feature_id == fid
                                && d.task_id == tid
                                && d.kind
                                    == if tid.is_some() {
                                        "implementation"
                                    } else {
                                        "closure"
                                    }
                                && s.accepted(d)
                        })
                        .ok_or_else(|| err("blocked", "缺少当前已接受源交付"))?
                        .clone();
                    s.validate_evidence(&source_delivery)?;
                    let source = source_delivery
                        .candidate
                        .clone()
                        .ok_or_else(|| err("blocked", "源交付缺少候选版本"))?;
                    let candidate = self.git.candidate(&source, &expected, &mid)?;
                    let m = Merge {
                        id: mid.clone(),
                        feature_id: fid.clone(),
                        task_id: tid.clone(),
                        status: Status::Running,
                        revision: 1,
                        input_version: f.input_version,
                        source,
                        source_delivery: source_delivery.id,
                        target: target_head,
                        expected_target: expected,
                        candidate,
                        committed_revision: None,
                    };
                    s.merges.push(m);
                    if let Some(tid) = &tid {
                        let t = s.tasks.iter_mut().find(|t| &t.id == tid).unwrap();
                        t.status = Status::Merging;
                        t.revision += 1;
                    } else {
                        let f = s.features.iter_mut().find(|x| x.id == fid).unwrap();
                        f.status = Status::Merging;
                        f.revision += 1;
                    }
                    self.add_run(
                        s,
                        None,
                        Some(&mid),
                        "merge_verify",
                        vec![],
                        false,
                        "merge-readonly",
                        &f.binding,
                        None,
                        opt(p, "scenario").as_deref().unwrap_or("success"),
                    );
                    json!({"id":mid,"operation_id":mid})
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
                    let actual = s
                        .heads
                        .iter()
                        .find(|h| h.id == m.target)
                        .unwrap()
                        .revision
                        .clone();
                    let committed = self
                        .git
                        .advance(&actual, &m.expected_target, &m.candidate)?;
                    s.heads
                        .iter_mut()
                        .find(|h| h.id == m.target)
                        .unwrap()
                        .revision = committed.clone();
                    let mm = s.merges.iter_mut().find(|x| x.id == target).unwrap();
                    mm.status = Status::Completed;
                    mm.committed_revision = Some(committed);
                    mm.revision += 1;
                    for other in s.merges.iter_mut().filter(|x| {
                        x.target == m.target && x.id != m.id && x.status != Status::Completed
                    }) {
                        for d in s
                            .deliveries
                            .iter_mut()
                            .filter(|d| d.merge_id.as_deref() == Some(&other.id))
                        {
                            d.stale = true;
                        }
                    }
                    if let Some(tid) = m.task_id {
                        let t = s.tasks.iter_mut().find(|t| t.id == tid).unwrap();
                        t.status = Status::Completed;
                        t.revision += 1;
                    } else {
                        let f = s
                            .features
                            .iter_mut()
                            .find(|f| f.id == m.feature_id)
                            .unwrap();
                        f.status = Status::Completed;
                        f.revision += 1;
                    }
                    json!({"id":target,"simulation_completed":true})
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
                    mutable_feature(s, &fid)?;
                    let x = s.contexts.iter_mut().find(|x| x.id == target).unwrap();
                    x.content = text(p, "content")?;
                    x.revision += 1;
                    json!({"id":target})
                }
                "snapshot.save" => {
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
                _ => return Err(err("method_not_found", format!("未知命令 {method}"))),
            };
            event(s, target, method, &format!("command_id={}", c.command_id));
            Ok(result)
        };
        self.store
            .transact(Some((&c.command_id, &fingerprint)), &mut action)
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
    fn add_run(
        &self,
        s: &mut EngineeringState,
        aid: Option<&str>,
        mid: Option<&str>,
        node: &str,
        deps: Vec<String>,
        writes: bool,
        worktree: &str,
        b: &BindingLock,
        retry: Option<String>,
        scenario: &str,
    ) -> String {
        let rid = id();
        let sid = id();
        let owner = aid.or(mid).unwrap().to_string();
        let snapshot_id = s
            .attempts
            .iter()
            .find(|a| Some(a.id.as_str()) == aid)
            .and_then(|a| a.task_id.as_ref())
            .and_then(|tid| s.tasks.iter().find(|t| &t.id == tid))
            .map(|t| t.inherited_snapshot.clone());
        s.sessions.push(Session {
            id: sid.clone(),
            owner_id: owner,
            agent: b.agent.clone(),
            harness: b.harness.clone(),
            sandbox: b.sandbox.clone(),
            snapshot_id,
            origin: Origin::Simulated,
        });
        let outcome = self.harness.configure(node, scenario);
        s.runs.push(Run {
            id: rid.clone(),
            attempt_id: aid.map(str::to_owned),
            merge_id: mid.map(str::to_owned),
            node: node.into(),
            dependencies: deps,
            status: Status::Queued,
            retry_of: retry,
            session_id: sid,
            writes,
            worktree: worktree.into(),
            outcome: outcome.into(),
            cancel_requested: false,
            started: None,
            finished: None,
            result: None,
            origin: Origin::Simulated,
        });
        rid
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
        let body=format!("# 执行与交付报告\n\nDelivery: {did}\n类型: {kind}\n输入版本: {input}\norigin=simulated\n\n这是模拟工程结果，不代表真实代码合入。\n\n## 冻结输入与证据\n```json\n{}\n```\n",serde_json::to_string_pretty(&json!({"documents":documents,"evidence":evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>(),"candidate":candidate})).unwrap());
        let artifact = self.artifacts.seal(fid, tid, "deliveries", &body)?;
        let artifact_id = artifact.id.clone();
        let manifest=self.artifacts.seal(fid,tid,"manifests",&serde_json::to_string_pretty(&json!({"schema_version":1,"delivery_id":did,"report_artifact":artifact,"input_version":input,"documents":documents,"evidence":evidence,"candidate":candidate,"origin":"simulated","attempt":aid.and_then(|id|s.attempts.iter().find(|a|a.id==id))})).map_err(|e|err("serialization",e.to_string()))?)?;
        s.artifacts.push(manifest);
        s.artifacts.push(artifact);
        let delivery = Delivery {
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
            origin: Origin::Simulated,
        };
        s.validate_evidence(&delivery)?;
        s.deliveries.push(delivery);
        event(s, &did, "delivery.created", kind);
        Ok(did)
    }
    fn check_merge_target(&self, s: &EngineeringState, d: &Delivery) -> Result<()> {
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
            !d.stale && d.input_version == s.feature(&d.feature_id)?.input_version,
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
                evidence: format!("{} 个模拟执行记录", d.evidence.len()),
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
                evidence: format!("standard={} slot={purpose} spec={}", standard.id, spec.id),
            });
        }
        let rid = id();
        let report=self.artifacts.seal(&d.feature_id,d.task_id.as_deref(),"reviews",&format!("# Review 报告\n\nDelivery: {}\nactor: {actor}\n决定: {decision:?}\n理由: {reason}\n证据来源: origin=simulated（人类决定不改变证据来源）\n\n{}",d.id,serde_json::to_string_pretty(&checks).unwrap()))?;
        s.artifacts.push(report);
        s.reviews.push(Review {
            id: rid,
            delivery_id: d.id.clone(),
            decision,
            checks,
            actor: actor.into(),
            run_id: run,
            reason: reason.into(),
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
    fn reconcile(&self, s: &mut EngineeringState, d: &Delivery) -> Result<()> {
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
        let required = if a.phase == "planning" {
            vec!["requirement", "plan"]
        } else {
            vec![a.phase.as_str()]
        };
        if !required.iter().all(|kind| {
            s.deliveries
                .iter()
                .any(|x| x.attempt_id.as_deref() == Some(&a.id) && x.kind == *kind && s.accepted(x))
        }) {
            return Ok(());
        }
        let am = s.attempts.iter_mut().find(|x| x.id == a.id).unwrap();
        am.status = Status::Accepted;
        am.revision += 1;
        self.snapshot(
            s,
            &a.feature_id,
            a.task_id.as_deref(),
            if a.phase == "planning" {
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
            f.status = if a.phase == "planning" {
                Status::Ready
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
        let content = s
            .contexts
            .iter()
            .find(|c| c.owner_id == owner)
            .map(|c| c.content.clone())
            .unwrap_or_default();
        let inherited = tid
            .and_then(|tid| s.tasks.iter().find(|t| t.id == tid))
            .map(|t| t.inherited_snapshot.clone());
        s.snapshots.push(Snapshot {
            id: sid.clone(),
            feature_id: fid.into(),
            task_id: tid.map(str::to_owned),
            kind: kind.into(),
            content,
            binding: b,
            documents: s
                .documents
                .iter()
                .filter(|d| {
                    d.feature_id == fid && (d.task_id.is_none() || d.task_id.as_deref() == tid)
                })
                .map(|d| d.id.clone())
                .collect(),
            delivery_id: delivery,
            inherited_from: inherited,
        });
        let manifest=self.artifacts.seal(fid,tid,"snapshots",&serde_json::to_string_pretty(&json!({"schema_version":1,"snapshot":s.snapshots.last(),"execution_mode":"simulation"})).map_err(|e|err("serialization",e.to_string()))?)?;
        s.artifacts.push(manifest);
        Ok(sid)
    }
    pub fn tick(&self, global_limit: usize) -> Result<()> {
        let mut action = |s: &mut EngineeringState| {
            let running = s
                .runs
                .iter()
                .filter(|r| r.status == Status::Running)
                .cloned()
                .collect::<Vec<_>>();
            for r in running {
                let input = if r.node.starts_with("review:") {
                    let d = s.delivery(r.node.trim_start_matches("review:"))?;
                    let artifact = s
                        .artifacts
                        .iter()
                        .find(|a| a.id == d.artifact_id)
                        .ok_or_else(|| err("blocked", "交付 Artifact 缺失"))?;
                    json!({"delivery":d,"documents":d.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"evidence":d.evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>(),"artifact":artifact,"artifact_body":String::from_utf8_lossy(&self.artifacts.read(artifact)?).into_owned()})
                } else if let Some(aid) = &r.attempt_id {
                    let a = s.attempt(aid)?;
                    json!({"phase":a.phase,"context":{"content":a.context_content,"revision":a.context_revision},"initial_snapshot":s.sessions.iter().find(|session|session.id==r.session_id).and_then(|session|session.snapshot_id.as_ref()).and_then(|id|s.snapshots.iter().find(|snapshot|&snapshot.id==id)),"documents":a.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>()})
                } else {
                    json!({})
                };
                let result = match self.harness.poll(&r, &input, now()) {
                    Ok(Some(v)) => v,
                    Ok(None) => continue,
                    Err(e) => {
                        json!({"status":"failed","failure_type":"adapter_error","message":e.message})
                    }
                };
                let valid = result
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|v| ["succeeded", "failed", "cancelled"].contains(&v))
                    && (!r.node.starts_with("review:")
                        || result["status"] != "succeeded"
                        || serde_json::from_value::<Decision>(
                            result
                                .get("review_decision")
                                .cloned()
                                .unwrap_or(Value::Null),
                        )
                        .is_ok()
                            && result["review_delivery"] == r.node.trim_start_matches("review:"))
                    && result.get("document").is_none_or(|d| {
                        d["kind"]
                            .as_str()
                            .is_some_and(|k| ["requirement", "design", "test", "doc"].contains(&k))
                            && d["body"].as_str().is_some_and(|v| !v.is_empty())
                            && d["title"].as_str().is_some_and(|v| !v.is_empty())
                    });
                let status = if r.cancel_requested || result["status"] == "cancelled" {
                    Status::Cancelled
                } else if !valid || result["status"] == "failed" {
                    Status::Failed
                } else {
                    Status::Succeeded
                };
                let mut output = if !valid {
                    json!({"failure_type":"adapter_contract","message":"模拟供应商缺少 status 字段"})
                } else {
                    result
                };
                if status == Status::Succeeded
                    && ["red", "green", "feature_tests", "verify", "merge_verify"]
                        .contains(&r.node.as_str())
                {
                    let candidate = if let Some(mid) = &r.merge_id {
                        s.merge(mid)?.candidate.clone()
                    } else {
                        let a = s.attempt(r.attempt_id.as_deref().unwrap())?;
                        format!("sim-rev-source-{}", a.id)
                    };
                    let documents = if let Some(aid) = &r.attempt_id {
                        s.attempt(aid)?.documents.clone()
                    } else {
                        let m = s.merge(r.merge_id.as_deref().unwrap())?;
                        s.delivery(&m.source_delivery)?.documents.clone()
                    };
                    let request = TestRequest {
                        phase: r.node.clone(),
                        candidate: candidate.clone(),
                        test_definitions: documents
                            .iter()
                            .filter(|did| {
                                s.documents
                                    .iter()
                                    .any(|d| &d.id == *did && d.kind == "test")
                            })
                            .cloned()
                            .collect(),
                        environment: s
                            .sessions
                            .iter()
                            .find(|session| session.id == r.session_id)
                            .ok_or_else(|| err("blocked", "Session 缺失"))?
                            .sandbox
                            .clone(),
                        origin: Origin::Simulated,
                    };
                    output = self.tests.validate(&request, &r.outcome);
                    if !output["status"]
                        .as_str()
                        .is_some_and(|v| ["succeeded", "failed"].contains(&v))
                        || output["candidate"] != request.candidate
                        || output["test_definitions"] != json!(request.test_definitions)
                        || output["origin"] != json!(request.origin)
                        || output["environment"] != request.environment
                    {
                        output = json!({"status":"failed","failure_type":"adapter_contract","message":"测试结果缺字段或定义/版本/来源不匹配"});
                    }
                }
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
                for res in s.resources.iter_mut().filter(|x| x.run_id == r.id) {
                    self.sandbox.cleanup(res, output["cleanup_failed"] == true);
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
                            "独立模拟 Reviewer 基于冻结交付评审；origin=simulated",
                            "agent:sim-reviewer",
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
                let log=self.artifacts.seal(&fid,tid.as_deref(),"runs",&format!("# Run {}\n\nnode={}\norigin=simulated\nstarted={:?}\nfinished={}\nstatus={status:?}\n\n```json\n{}\n```\n",r.id,r.node,r.started,now(),serde_json::to_string_pretty(&output).unwrap()))?;
                s.artifacts.push(log);
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
                if all || failed {
                    let am = s.attempts.iter_mut().find(|x| x.id == a.id).unwrap();
                    am.status = if all {
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
                }) && !s
                    .deliveries
                    .iter()
                    .any(|d| d.merge_id.as_deref() == Some(&m.id))
                {
                    let evidence = rs.iter().map(|r| r.id.clone()).collect();
                    let docs = s
                        .documents
                        .iter()
                        .filter(|d| d.feature_id == m.feature_id)
                        .map(|d| d.id.clone())
                        .collect();
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
                .filter(|r| r.status == Status::Queued)
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
                s.resources.push(self.sandbox.resource(&r.id));
                event(s, &r.id, "run.started", &r.node);
            }
            Ok(json!({"ok":true}))
        };
        self.store.transact(None, &mut action)?;
        Ok(())
    }
    fn blocked(&self, s: &EngineeringState, r: &Run, limit: usize) -> Option<String> {
        let (fid, paused) = if let Some(aid) = &r.attempt_id {
            let a = s.attempt(aid).ok()?;
            let reviewer = r.node.starts_with("review:");
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
        if a.input_version != s.feature(&a.feature_id)?.input_version {
            return Ok(());
        }
        if kind == "requirement"
            && s.documents
                .iter()
                .any(|d| d.feature_id == a.feature_id && d.kind == kind)
        {
            return Ok(());
        }
        let old = s
            .documents
            .iter()
            .rev()
            .find(|d| d.feature_id == a.feature_id && d.task_id == a.task_id && d.kind == kind)
            .cloned();
        let req = s
            .documents
            .iter()
            .rev()
            .find(|d| d.feature_id == a.feature_id && d.kind == "requirement")
            .map(|d| d.id.clone());
        let design = s
            .documents
            .iter()
            .rev()
            .find(|d| d.feature_id == a.feature_id && d.kind == "design")
            .map(|d| d.id.clone());
        let artifact = self.artifacts.seal(
            &a.feature_id,
            a.task_id.as_deref(),
            "documents",
            &format!("# {title}\n\norigin=simulated\n\n{body}"),
        )?;
        s.artifacts.push(artifact);
        s.documents.push(Document {
            id: id(),
            feature_id: a.feature_id,
            task_id: a.task_id,
            kind,
            title,
            body,
            version: old.as_ref().map_or(1, |d| d.version + 1),
            supersedes: old.map(|d| d.id),
            requirement_id: req,
            design_id: design,
            origin: Origin::Simulated,
        });
        Ok(())
    }
    pub fn query(&self, method: &str, p: &Value, global_limit: usize) -> Result<Value> {
        let s = self.store.snapshot()?;
        let offset = p.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = p
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 100) as usize;
        match method {
            "query.workspace" => {
                let pid = opt(p, "project_id");
                let fid = opt(p, "feature_id");
                let features = s
                    .features
                    .iter()
                    .filter(|f| pid.as_ref().is_none_or(|id| &f.project_id == id))
                    .collect::<Vec<_>>();
                let detail = if let Some(fid) = &fid {
                    Some(self.feature_detail(&s, fid, limit, global_limit, &p["detail_pages"])?)
                } else {
                    None
                };
                Ok(
                    json!({"projects":s.projects.iter().skip(p.get("project_offset").and_then(Value::as_u64).unwrap_or(0) as usize).take(limit).collect::<Vec<_>>(),"features":features.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_features":features.len(),"total_projects":s.projects.len(),"definitions":s.definitions,"detail":detail,"data_root":self.store.root(),"execution_mode":"simulation","global_concurrency":global_limit}),
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
        let blocked = runs
            .iter()
            .filter(|r| r.status == Status::Queued)
            .map(|r| (r.id.clone(), self.blocked(s, r, global_limit)))
            .collect::<std::collections::BTreeMap<_, _>>();
        Ok(
            json!({"totals":{"tasks":s.tasks.iter().filter(|x|x.feature_id==fid).count(),"attempts":s.attempts.iter().filter(|x|x.feature_id==fid).count(),"merges":s.merges.iter().filter(|x|x.feature_id==fid).count(),"deliveries":s.deliveries.iter().filter(|x|x.feature_id==fid).count(),"documents":s.documents.iter().filter(|x|x.feature_id==fid).count(),"snapshots":s.snapshots.iter().filter(|x|x.feature_id==fid).count(),"artifacts":s.artifacts.iter().filter(|x|x.feature_id==fid).count(),"runs":s.runs.iter().filter(|r|r.attempt_id.as_deref().is_some_and(|id|ids.contains(&id))||r.merge_id.as_deref().is_some_and(|id|mids.contains(&id))).count()},"feature":f,"tasks":tasks,"attempts":attempts,"runs":runs,"documents":s.documents.iter().filter(|d|d.feature_id==fid).rev().skip(page_offset("documents")).take(limit).collect::<Vec<_>>(),"deliveries":deliveries,"reviews":s.reviews.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).rev().take(limit).collect::<Vec<_>>(),"overrides":s.overrides.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).collect::<Vec<_>>(),"merges":merges,"snapshots":s.snapshots.iter().filter(|x|x.feature_id==fid).rev().skip(page_offset("snapshots")).take(limit).collect::<Vec<_>>(),"contexts":s.contexts.iter().filter(|x|x.owner_id==fid||tasks.iter().any(|t|t.id==x.owner_id)).collect::<Vec<_>>(),"artifacts":s.artifacts.iter().filter(|a|a.feature_id==fid).rev().skip(page_offset("artifacts")).take(limit).collect::<Vec<_>>(),"sessions":s.sessions.iter().filter(|x|runs.iter().any(|r|r.session_id==x.id)).collect::<Vec<_>>(),"resources":s.resources.iter().filter(|x|runs.iter().any(|r|r.id==x.run_id)).collect::<Vec<_>>(),"heads":s.heads.iter().filter(|h|h.project_id==f.project_id).collect::<Vec<_>>(),"allowed_actions":actions,"blocked_reasons":blocked,"accepted_deliveries":deliveries.iter().filter(|d|s.accepted(d)).map(|d|&d.id).collect::<Vec<_>>()}),
        )
    }
    fn actions(&self, s: &EngineeringState, target: &str) -> Value {
        let mut allowed = vec![];
        let mut blocked = vec![];
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
                allowed.extend(["attempt.start", "binding.update", "snapshot.save"]);
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
            if m.status != Status::Completed
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
        json!({"allowed":allowed,"blocked":blocked,"attempt_phases":if let Some(f)=s.features.iter().find(|f|f.id==target){if !s.planning_accepted(&f.id){vec!["planning"]}else if s.tasks.iter().any(|t|t.feature_id==f.id)&&s.tasks.iter().filter(|t|t.feature_id==f.id).all(|t|t.status==Status::Completed){vec!["closure"]}else{vec![]}}else{vec!["implementation"]}})
    }
}
