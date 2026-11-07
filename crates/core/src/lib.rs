//! Application use cases and ports. Adapters cannot advance domain state.
mod advances;
mod effects;
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
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    fn configure(&self, node: &str, scenario: &str) -> String;
    fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>>;
    fn reconcile(&self, _run: &Run) -> Result<Option<Value>> {
        Ok(None)
    }
}
pub trait GitPort: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    /// Read/freeze the code revision for an execution stage; simulation syntax belongs to the adapter.
    fn execution_revision(&self, attempt: &Attempt, phase: &str) -> Result<String>;
    fn reconcile_advance(
        &self,
        _context: &GitContext,
        _operation_id: &str,
    ) -> Result<AdvanceObservation> {
        Ok(AdvanceObservation::Unknown)
    }
    fn candidate(
        &self,
        context: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String>;
    fn advance(
        &self,
        context: &GitContext,
        actual: &str,
        expected: &str,
        candidate: &str,
        operation_id: &str,
    ) -> Result<String>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRequest {
    pub operation_id: Id,
    pub phase: String,
    pub candidate: String,
    pub test_definitions: Vec<Id>,
    pub environment: String,
    pub origin: Origin,
    #[serde(default)]
    pub definitions: Vec<Document>,
    #[serde(default)]
    pub code: Option<GitContext>,
    #[serde(default)]
    pub runtime: Option<ExecutionInstance>,
}
pub trait TestRunnerPort: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    fn validate(&self, request: &TestRequest, outcome: &str) -> Value;
    fn reconcile(&self, _request: &TestRequest) -> Result<Option<Value>> {
        Ok(None)
    }
}
pub trait SandboxPort: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    /// Pure resource intent; implementations must not create external resources here.
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
    let source = if let Some(tid) = tid {
        let t = s.task(tid)?;
        CodeLocation {
            owner: tid.into(),
            branch: format!("task/{tid}"),
            worktree: t.worktree.clone(),
        }
    } else {
        CodeLocation {
            owner: fid.into(),
            branch: format!("feature/{fid}"),
            worktree: format!("logical-feature/{fid}"),
        }
    };
    let target = if tid.is_some() {
        CodeLocation {
            owner: fid.into(),
            branch: format!("feature/{fid}"),
            worktree: format!("logical-feature/{fid}"),
        }
    } else {
        CodeLocation {
            owner: format!("main:{}", project.id),
            branch: workspace.mainline.clone(),
            worktree: workspace.path.clone(),
        }
    };
    Ok(GitContext {
        project: project.id.clone(),
        workspace,
        source,
        target,
        origin: Origin::Simulated,
    })
}
impl Engine {
    pub fn bootstrap(&self) -> Result<()> {
        self.store.transact(None, &mut |s| {
            if s.harnesses.is_empty() {
                s.harnesses.push(Harness {
                    id: "sim-harness-v1".into(),
                    provider: "simulation".into(),
                    version: "1".into(),
                    capabilities: vec!["structured-output".into(), "cancel".into(), "poll".into()],
                    origin: Origin::Simulated,
                });
            }
            if s.sandboxes.is_empty() {
                s.sandboxes.push(Sandbox {
                    id: "sim-sandbox-v1".into(),
                    provider: "simulation".into(),
                    version: "1".into(),
                    platform: "logical".into(),
                    image: None,
                    network: false,
                    origin: Origin::Simulated,
                });
            }
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
                    let predecessor = if family=="spec"&&version>1 {s.definitions.iter().rev().find(|d|d.family==family&&d.name==name).map(|d|d.id.clone())}else{None};
                    s.definitions.push(Definition {
                        id: id(),
                        family: family.into(),
                        name: name.into(),
                        version,
                        supersedes: predecessor,
                        body: body.into(),
                        slots,
                        rules,
                    });
                }
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
                let report=self.artifacts.seal(&fid,tid.as_deref(),"runs",&format!("# Interrupted Run {rid}\norigin=simulated\n结果未确认；恢复仅核对外部事实，不自动接纳。"))?;s.artifacts.push(report);
                let mut unresolved=false;
                for effect in s.effects.iter_mut().filter(|e|e.run_id==rid&&[EffectStatus::Pending,EffectStatus::Observed].contains(&e.status)&&e.kind!=EffectKind::Cleanup) {effect.status=EffectStatus::NeedsReconciliation;effect.reason="reconcile_requested".into();effect.revision+=1;unresolved=true;}
                if !unresolved {self.enqueue_cleanup(s,&rid,false);}
                event(
                    s,
                    &rid,
                    "run.interrupted",
                    "内核重启，执行结果未确认；需要创建重试",
                );
            }
            Ok(json!({"ok":true}))
        })?;
        self.reconcile_effects()?;
        self.drain_advances()?;
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
                "effect.reconcile" => {
                    let effect = s
                        .effects
                        .iter_mut()
                        .find(|e| e.id == target)
                        .ok_or_else(|| err("not_found", "外部操作不存在"))?;
                    require(
                        effect.status == EffectStatus::NeedsReconciliation,
                        "该操作无需核对",
                    )?;
                    effect.reason = "reconcile_requested".into();
                    effect.revision += 1;
                    effect.updated = now();
                    json!({"id":target,"operation_id":target})
                }
                "effect.retry_cleanup" => {
                    let old = s
                        .effects
                        .iter()
                        .find(|e| e.id == target)
                        .ok_or_else(|| err("not_found", "清理操作不存在"))?
                        .clone();
                    require(
                        old.kind == EffectKind::Cleanup && old.status == EffectStatus::Failed,
                        "仅失败清理可重试",
                    )?;
                    let resource = old.request["resource"]["id"]
                        .as_str()
                        .ok_or_else(|| err("blocked", "清理资源引用缺失"))?;
                    require(
                        s.resources
                            .iter()
                            .any(|r| r.id == resource && r.status == Status::CleanupFailed),
                        "资源已释放或当前不可重试",
                    )?;
                    require(
                        !s.effects.iter().any(|e| {
                            e.kind == EffectKind::Cleanup
                                && e.request["resource"]["id"] == resource
                                && [EffectStatus::Pending, EffectStatus::Observed]
                                    .contains(&e.status)
                        }),
                        "清理重试已在进行",
                    )?;
                    let key = format!("{}:retry:{}", old.id, c.command_id);
                    let mut request = old.request.clone();
                    request["fault_injection"] = json!(false);
                    let mut next = old.clone();
                    next.id = key.clone();
                    next.request = request;
                    next.receipt = None;
                    next.status = EffectStatus::Pending;
                    next.revision = 1;
                    next.created = now();
                    next.updated = now();
                    next.reason = format!("retry_of={}", old.id);
                    s.effects.push(next);
                    json!({"id":key,"operation_id":key})
                }
                "project.create" => {
                    let pid = id();
                    let title = text(p, "title")?;
                    let b = binding(s);
                    s.projects.push(Project {
                        id: pid.clone(),
                        title,
                        workspace: opt(p, "workspace").unwrap_or_default(),
                        workspace_definition: Some(Workspace {
                            repository: opt(p, "repository").unwrap_or_default(),
                            path: opt(p, "workspace").unwrap_or_default(),
                            mainline: opt(p, "mainline").unwrap_or_else(|| "main".into()),
                        }),
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
                    let artifact_id = Some(artifact.id.clone());
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
                    for d in s.deliveries.iter_mut().filter(|d| {
                        d.feature_id == fid
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
                        scope.acceptance = serde_json::from_value(keys.clone()).map_err(|_| {
                            err("invalid_params", "acceptance_keys 应为条目标识列表")
                        })?;
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
                        status: Status::Ready,
                        revision: 1,
                        binding: f.binding,
                        inherited_snapshot: snap.id.clone(),
                        dependencies: deps,
                        worktree: format!("logical-worktree/{tid}"),
                        scope: scope.clone(),
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
                    let test_body = s
                        .documents
                        .iter()
                        .rev()
                        .find(|d| d.feature_id == f.id && d.kind == "test" && d.task_id.is_none())
                        .ok_or_else(|| err("blocked", "规划缺少测试定义"))?
                        .body
                        .clone();
                    let test_artifact =
                        self.artifacts
                            .seal(&f.id, Some(&tid), "documents", &test_body)?;
                    let test_artifact_id = Some(test_artifact.id.clone());
                    s.artifacts.push(test_artifact);
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
                            snap.feature_id == task.feature_id
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
                        .filter_map(|id| {
                            s.documents.iter().find(|d| &d.id == id && d.kind == "test")
                        })
                        .next()
                        .ok_or_else(|| err("blocked", "规划缺少测试"))?
                        .clone();
                    let artifact = self.artifacts.seal(
                        &task.feature_id,
                        Some(target),
                        "documents",
                        &test.body,
                    )?;
                    let artifact_id = Some(artifact.id.clone());
                    s.artifacts.push(artifact);
                    s.documents.push(Document {
                        id: id(),
                        feature_id: task.feature_id.clone(),
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
                    let (fid, tid, phase, b, worktree) = if let Some(t) =
                        s.tasks.iter().find(|t| t.id == target).cloned()
                    {
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
                    let baseline = self.git.execution_revision(s.attempt(&aid)?, "red")?;
                    s.attempts
                        .iter_mut()
                        .find(|a| a.id == aid)
                        .unwrap()
                        .baseline = Some(baseline);
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
                    let _owner_binding = if let Some(aid) = &old.attempt_id {
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
                    let b = old.binding.clone();
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
                    let runs: Vec<_> = effective_runs(s, &a)
                        .into_iter()
                        .filter(|r| {
                            kind != "requirement"
                                || ["research", "requirement"].contains(&r.node.as_str())
                        })
                        .collect();
                    if kind == "plan" {
                        require(
                            s.deliveries.iter().any(|d| {
                                d.attempt_id.as_deref() == Some(&a.id)
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
                        Some(self.git.execution_revision(&a, "delivery")?),
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
                    let mut b = if let Some(aid) = &d.attempt_id {
                        s.attempt(aid)?.binding.clone()
                    } else {
                        let m = s.merge(d.merge_id.as_deref().unwrap())?;
                        let source = s.delivery(&m.source_delivery)?;
                        s.attempt(source.attempt_id.as_deref().unwrap())?
                            .binding
                            .clone()
                    };
                    let author = s.agent_identity(&b.agent)?;
                    b.agent = s
                        .definitions
                        .iter()
                        .find(|x| {
                            x.family == "agent"
                                && s.agent_identity(&x.id)
                                    .is_ok_and(|identity| identity != author)
                        })
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
                    let merge_binding = s
                        .attempt(
                            source_delivery
                                .attempt_id
                                .as_deref()
                                .ok_or_else(|| err("blocked", "源交付缺少 Attempt 锁"))?,
                        )?
                        .binding
                        .clone();
                    let source = source_delivery
                        .candidate
                        .clone()
                        .ok_or_else(|| err("blocked", "源交付缺少候选版本"))?;
                    let source_attempt =
                        s.attempt(source_delivery.attempt_id.as_deref().unwrap())?;
                    let context = source_attempt.git_context.as_ref().ok_or_else(|| {
                        err("blocked", "历史 Attempt 缺少确定仓库定位，请新建执行")
                    })?;
                    let candidate = self.git.candidate(context, &source, &expected, &mid)?;
                    let m = Merge {
                        id: mid.clone(),
                        feature_id: fid.clone(),
                        task_id: tid.clone(),
                        status: Status::Running,
                        revision: 1,
                        input_version: f.input_version,
                        source: source.clone(),
                        source_delivery: source_delivery.id,
                        target: target_head,
                        expected_target: expected.clone(),
                        candidate,
                        committed_revision: None,
                        change_request: Some(ChangeRequestRef {
                            provider: "simulation".into(),
                            repository: f.project_id.clone(),
                            external_id: format!("sim-change-{mid}"),
                            source_revision: source.clone(),
                            target_revision: expected.clone(),
                        }),
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
                        &merge_binding,
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
                    } else {
                        s.advances.push(MergeAdvance {
                            id: m.id.clone(),
                            context: s
                                .delivery(&m.source_delivery)?
                                .attempt_id
                                .as_deref()
                                .and_then(|id| s.attempt(id).ok())
                                .and_then(|a| a.git_context.clone()),
                            provider: self.git.identity(),
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
                    mutable_feature(s, &fid)?;
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
        let result = self
            .store
            .transact(Some((&c.command_id, &fingerprint)), &mut action)?;
        // Long operations return their durable ID; the scheduler owns supplier dispatch.
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
            instance: Some(ExecutionInstance {
                id: id(),
                run_id: rid.clone(),
                harness: s
                    .harnesses
                    .iter()
                    .find(|h| h.id == b.harness)
                    .unwrap()
                    .clone(),
                sandbox: s
                    .sandboxes
                    .iter()
                    .find(|env| env.id == b.sandbox)
                    .unwrap()
                    .clone(),
                external_session: None,
                external_environment: None,
                status: Status::Prepared,
                started: None,
                finished: None,
            }),
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
            binding: b.clone(),
            binding_verified: true,
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
            run_id: run.clone(),
            reason: reason.into(),
            assignment: Some(ReviewAssignment {
                delivery: d.id.clone(),
                spec: spec.id.clone(),
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
        let captured_runs: Vec<_> = s
            .runs
            .iter()
            .filter(|r| capture_attempt.is_some() && r.attempt_id == capture_attempt)
            .collect();
        let capture = SnapshotCapture {
            attempt: capture_attempt.clone(),
            runs: captured_runs.iter().map(|r| r.id.clone()).collect(),
            sessions: captured_runs.iter().map(|r| r.session_id.clone()).collect(),
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
        s.snapshots.push(Snapshot {
            id: sid.clone(),
            feature_id: fid.into(),
            task_id: tid.map(str::to_owned),
            kind: kind.into(),
            content,
            binding: b,
            documents,
            capture,
            delivery_id: delivery,
            inherited_from: inherited,
        });
        let manifest=self.artifacts.seal(fid,tid,"snapshots",&serde_json::to_string_pretty(&json!({"schema_version":1,"snapshot":s.snapshots.last(),"execution_mode":"simulation"})).map_err(|e|err("serialization",e.to_string()))?)?;
        s.artifacts.push(manifest);
        Ok(sid)
    }
    pub fn tick(&self, global_limit: usize) -> Result<()> {
        self.reconcile_effects()?;
        self.drain_advances()?;
        let snapshot = self.store.snapshot()?;
        let mut outputs = vec![];
        for run in snapshot.runs.iter().filter(|r| r.status == Status::Running) {
            if let Some(output) = self.poll_result(&snapshot, run)? {
                outputs.push((run.clone(), output));
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
                    a.input_version != s.feature(&a.feature_id)?.input_version
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
                self.finish_effects(s, &r.id, EffectStatus::Applied, "回执已接纳，产物已封存");
                self.enqueue_cleanup(s, &r.id, output["cleanup_failed"] == true);
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
                }) && !s
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
                if let Some(instance) = s
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.as_mut())
                {
                    instance.status = Status::Running;
                    instance.started = Some(now());
                }
                s.resources.push(self.sandbox.resource(&r.id));
                event(s, &r.id, "run.started", &r.node);
            }
            Ok(json!({"ok":true}))
        };
        self.store.transact(None, &mut action)?;
        self.drain_cleanup()?;
        Ok(())
    }
    fn poll_result(&self, s: &EngineeringState, r: &Run) -> Result<Option<Value>> {
        let mut input = if r.node.starts_with("review:") {
            let d = s.delivery(r.node.trim_start_matches("review:"))?;
            let artifact = s
                .artifacts
                .iter()
                .find(|a| a.id == d.artifact_id)
                .ok_or_else(|| err("blocked", "交付 Artifact 缺失"))?;
            json!({"delivery":d,"documents":d.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"evidence":d.evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>(),"artifact":artifact,"artifact_body":String::from_utf8_lossy(&self.artifacts.read(artifact)?).into_owned()})
        } else if let Some(aid) = &r.attempt_id {
            let a = s.attempt(aid)?;
            json!({"phase":a.phase,"context":{"content":a.context_content,"revision":a.context_revision},"initial_snapshot":s.sessions.iter().find(|session|session.id==r.session_id).and_then(|session|session.snapshot_id.as_ref()).and_then(|id|s.snapshots.iter().find(|snapshot|&snapshot.id==id)),"frozen_inputs":a.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"stage_outputs":a.outputs.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"documents":a.documents.iter().chain(a.outputs.iter()).filter(|id|!a.outputs.iter().any(|next|s.documents.iter().any(|d|&d.id==next&&d.supersedes.as_ref()==Some(*id)))).filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>()})
        } else {
            json!({})
        };
        let standard = s
            .definitions
            .iter()
            .find(|definition| definition.id == r.binding.standard)
            .ok_or_else(|| err("blocked", "执行 Standard 锁缺失"))?;
        let definitions: Vec<_> = s
            .definitions
            .iter()
            .filter(|d| {
                d.id == r.binding.agent
                    || d.id == r.binding.standard
                    || r.binding.skills.contains(&d.id)
                    || standard.slots.iter().any(|slot| slot.spec == d.id)
            })
            .collect();
        let code_attempt = if let Some(aid) = r.attempt_id.as_deref() {
            s.attempt(aid).ok()
        } else {
            r.merge_id
                .as_deref()
                .and_then(|id| s.merge(id).ok())
                .and_then(|m| s.delivery(&m.source_delivery).ok())
                .and_then(|d| d.attempt_id.as_deref())
                .and_then(|id| s.attempt(id).ok())
        };
        input["code"] = json!(code_attempt.and_then(|a| a.git_context.as_ref()));
        input["code_baseline"] = json!(code_attempt.and_then(|a| a.baseline.as_ref()));
        if let Some(mid) = &r.merge_id {
            input["merge"] = json!(s.merge(mid)?);
        }
        input["execution_binding"] = json!(r.binding);
        input["definitions"] = json!(definitions);
        input["runtime"] = json!(
            s.sessions
                .iter()
                .find(|session| session.id == r.session_id)
                .and_then(|session| session.instance.as_ref())
        );
        let result = match self.poll_harness(r, &input) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(e) => {
                json!({"status":"failed","failure_type":"adapter_error","message":e.message})
            }
        };
        if r.cancel_requested && result["cancellation_confirmed"] != true {
            // A success arriving after cancel is not a cancellation acknowledgement.
            // Keep the resource permit until the adapter confirms termination.
            return Ok(None);
        }
        let valid = result
            .get("session_id")
            .is_none_or(|id| *id == r.session_id)
            && result.get("run_id").is_none_or(|id| *id == r.id)
            && result
                .get("origin")
                .is_none_or(|origin| *origin == json!(r.origin))
            && result
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
                if r.node == "red" {
                    a.baseline.clone().ok_or_else(|| {
                        err("blocked", "历史 Attempt 缺少代码基线，请新建 Attempt")
                    })?
                } else {
                    self.git.execution_revision(a, &r.node)?
                }
            };
            let documents = if let Some(aid) = &r.attempt_id {
                s.attempt(aid)?.documents.clone()
            } else {
                let m = s.merge(r.merge_id.as_deref().unwrap())?;
                s.delivery(&m.source_delivery)?.documents.clone()
            };
            let request = TestRequest {
                operation_id: r.id.clone(),
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
                    .instance
                    .as_ref()
                    .ok_or_else(|| err("blocked", "历史 Session 缺少确定环境实例，请新建执行"))?
                    .id
                    .clone(),
                origin: Origin::Simulated,
                definitions: documents
                    .iter()
                    .filter_map(|id| s.documents.iter().find(|d| &d.id == id && d.kind == "test"))
                    .cloned()
                    .collect(),
                code: code_attempt.and_then(|a| a.git_context.clone()),
                runtime: s
                    .sessions
                    .iter()
                    .find(|session| session.id == r.session_id)
                    .and_then(|session| session.instance.clone())
                    .map(|mut runtime| {
                        if let Some(id) = output["external_environment"].as_str() {
                            runtime.external_environment = Some(id.into());
                        }
                        if let Some(id) = output["external_session"].as_str() {
                            runtime.external_session = Some(id.into());
                        }
                        runtime
                    }),
            };
            output = self.test_effect(r, &request)?;
            if serde_json::from_value::<TestResult>(output.clone()).is_err()
                || output["operation_id"] != request.operation_id
                || !output["status"]
                    .as_str()
                    .is_some_and(|v| ["succeeded", "failed"].contains(&v))
                || output["candidate"] != request.candidate
                || output["test_definitions"] != json!(request.test_definitions)
                || output["origin"] != json!(request.origin)
                || output["environment"] != request.environment
                || output["test_phase"] != request.phase
            {
                output = json!({"status":"failed","failure_type":"adapter_contract","message":"测试结果缺字段或定义/版本/来源不匹配"});
            }
        }

        if status != Status::Succeeded {
            output["status"] = json!(status);
        }
        Ok(Some(output))
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
        if a.input_version != s.feature(&a.feature_id)?.input_version {
            return Ok(());
        }
        if kind == "requirement"
            && s.documents
                .iter()
                .any(|d| d.feature_id == a.feature_id && d.kind == kind)
        {
            if let Some(req) = s
                .documents
                .iter()
                .rev()
                .find(|d| d.feature_id == a.feature_id && d.kind == "requirement")
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
            .find(|d| d.feature_id == a.feature_id && d.task_id == a.task_id && d.kind == kind)
            .cloned();
        let req = s
            .documents
            .iter()
            .rev()
            .find(|d| {
                d.feature_id == a.feature_id
                    && d.kind == "requirement"
                    && (a.documents.contains(&d.id) || a.outputs.contains(&d.id))
            })
            .map(|d| d.id.clone());
        let design = s
            .documents
            .iter()
            .rev()
            .find(|d| {
                d.feature_id == a.feature_id
                    && d.kind == "design"
                    && (a.documents.contains(&d.id) || a.outputs.contains(&d.id))
            })
            .map(|d| d.id.clone());
        let artifact = self.artifacts.seal(
            &a.feature_id,
            a.task_id.as_deref(),
            "documents",
            &format!("# {title}\n\norigin=simulated\n\n{body}"),
        )?;
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
            id: document_id,
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
            acceptance,
            verifies,
            artifact_id,
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
            "query.archive_preview" => {
                let fid = text(p, "feature_id")?;
                let f = s.feature(&fid)?;
                Ok(json!(ArchivePreview {
                    feature: fid.clone(),
                    artifacts: s
                        .artifacts
                        .iter()
                        .filter(|a| a.feature_id == fid)
                        .map(|a| a.id.clone())
                        .collect(),
                    snapshots: s
                        .snapshots
                        .iter()
                        .filter(|a| a.feature_id == fid)
                        .map(|a| a.id.clone())
                        .collect(),
                    blocked_reasons: vec![if f.status != Status::Completed {
                        "Feature 尚未完成".into()
                    } else {
                        "Archive 导出、冷数据删除与恢复尚未实现".into()
                    }],
                    executable: false
                }))
            }
            "query.runtimes" => Ok(json!({"harnesses":s.harnesses,"sandboxes":s.sandboxes})),
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
                    json!({"projects":s.projects.iter().skip(p.get("project_offset").and_then(Value::as_u64).unwrap_or(0) as usize).take(limit).collect::<Vec<_>>(),"features":features.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_features":features.len(),"total_projects":s.projects.len(),"definitions":s.definitions,"harnesses":s.harnesses,"sandboxes":s.sandboxes,"detail":detail,"data_root":self.store.root(),"execution_mode":"simulation","global_concurrency":global_limit}),
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
            .filter(|r| r.status == Status::Queued)
            .map(|r| (r.id.clone(), self.blocked(s, r, global_limit)))
            .collect::<std::collections::BTreeMap<_, _>>();
        Ok(
            json!({"advances":s.advances.iter().filter(|advance|s.merges.iter().any(|m|m.id==advance.id&&m.feature_id==fid)).rev().take(limit).collect::<Vec<_>>(),"effects":effects.iter().skip(page_offset("effects")).take(limit).collect::<Vec<_>>(),"totals":{"effects":effects.len(),"tasks":s.tasks.iter().filter(|x|x.feature_id==fid).count(),"attempts":s.attempts.iter().filter(|x|x.feature_id==fid).count(),"merges":s.merges.iter().filter(|x|x.feature_id==fid).count(),"deliveries":s.deliveries.iter().filter(|x|x.feature_id==fid).count(),"documents":s.documents.iter().filter(|x|x.feature_id==fid).count(),"snapshots":s.snapshots.iter().filter(|x|x.feature_id==fid).count(),"artifacts":s.artifacts.iter().filter(|x|x.feature_id==fid).count(),"runs":s.runs.iter().filter(|r|r.attempt_id.as_deref().is_some_and(|id|ids.contains(&id))||r.merge_id.as_deref().is_some_and(|id|mids.contains(&id))).count()},"feature":f,"tasks":tasks,"attempts":attempts,"runs":runs,"documents":s.documents.iter().filter(|d|d.feature_id==fid).rev().skip(page_offset("documents")).take(limit).collect::<Vec<_>>(),"deliveries":deliveries,"reviews":s.reviews.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).rev().take(limit).collect::<Vec<_>>(),"overrides":s.overrides.iter().filter(|r|dids.contains(&r.delivery_id.as_str())).collect::<Vec<_>>(),"merges":merges,"snapshots":s.snapshots.iter().filter(|x|x.feature_id==fid).rev().skip(page_offset("snapshots")).take(limit).collect::<Vec<_>>(),"contexts":s.contexts.iter().filter(|x|x.owner_id==fid||tasks.iter().any(|t|t.id==x.owner_id)).collect::<Vec<_>>(),"artifacts":s.artifacts.iter().filter(|a|a.feature_id==fid).rev().skip(page_offset("artifacts")).take(limit).collect::<Vec<_>>(),"sessions":s.sessions.iter().filter(|x|runs.iter().any(|r|r.session_id==x.id)).collect::<Vec<_>>(),"resources":s.resources.iter().filter(|x|runs.iter().any(|r|r.id==x.run_id)).collect::<Vec<_>>(),"heads":s.heads.iter().filter(|h|h.project_id==f.project_id).collect::<Vec<_>>(),"allowed_actions":actions,"blocked_reasons":blocked,"accepted_deliveries":deliveries.iter().filter(|d|s.accepted(d)).map(|d|&d.id).collect::<Vec<_>>()}),
        )
    }
    fn actions(&self, s: &EngineeringState, target: &str) -> Value {
        let mut allowed = vec![];
        let mut blocked = vec![];
        if let Some(effect) = s.effects.iter().find(|e| e.id == target) {
            if effect.status == EffectStatus::NeedsReconciliation {
                allowed.push("effect.reconcile");
            }
            if effect.kind == EffectKind::Cleanup && effect.status == EffectStatus::Failed {
                let resource = effect.request["resource"]["id"].as_str().unwrap_or("");
                if s.resources
                    .iter()
                    .any(|r| r.id == resource && r.status == Status::CleanupFailed)
                    && !s.effects.iter().any(|other| {
                        other.kind == EffectKind::Cleanup
                            && other.request["resource"]["id"] == resource
                            && [EffectStatus::Pending, EffectStatus::Observed]
                                .contains(&other.status)
                    })
                {
                    allowed.push("effect.retry_cleanup");
                }
            }
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
        json!({"allowed":allowed,"blocked":blocked,"attempt_phases":if let Some(f)=s.features.iter().find(|f|f.id==target){if !s.planning_accepted(&f.id){vec!["planning"]}else if s.tasks.iter().any(|t|t.feature_id==f.id)&&s.tasks.iter().filter(|t|t.feature_id==f.id).all(|t|t.status==Status::Completed){vec!["closure"]}else{vec![]}}else{vec!["implementation"]}})
    }
}
