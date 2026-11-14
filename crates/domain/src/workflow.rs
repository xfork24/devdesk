//! Configuration-owned work, independent of business stages and role names.
use crate::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_STEP_LIMIT: usize = 1000;
pub const WORKFLOW_DEPTH_LIMIT: usize = 8;
/// Metadata recorded on persisted run instances; acceptance of stored instances is
/// structural (a steps array), never a version comparison.
pub const WORKFLOW_INSTANCE_SCHEMA: u64 = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDefinition {
    pub schema_version: u64,
    #[serde(default)]
    pub action_requirements: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub stages: Vec<StageDefinition>,
    pub steps: Vec<StepDefinition>,
    #[serde(default)]
    pub inputs: BTreeMap<String, PortDefinition>,
    #[serde(default)]
    pub outputs: BTreeMap<String, PortSource>,
    #[serde(default)]
    pub completion: CompletionRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDefinition {
    #[serde(default)]
    pub node_id: Id,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default_role_ref: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDefinition {
    #[serde(default)]
    pub node_id: Id,
    pub key: String,
    pub name: String,
    pub stage: String,
    pub executor: StepExecutor,
    #[serde(default)]
    pub task_instructions: Option<String>,
    #[serde(default)]
    pub required_skill_refs: Vec<Id>,
    #[serde(default)]
    pub spec_refs: Vec<Id>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputBinding>,
    #[serde(default)]
    pub outputs: BTreeMap<String, PortDefinition>,
    #[serde(default)]
    pub access: StepAccess,
    #[serde(default)]
    pub budget: ExecutionBudget,
    #[serde(default)]
    pub condition: CompletionRule,
    #[serde(default)]
    pub completion: CompletionRule,
    #[serde(default)]
    pub confirmation: ConfirmationPolicy,
    #[serde(default)]
    pub allow_completion_override: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepExecutor {
    Agent {
        #[serde(default)]
        role_ref: Option<Id>,
    },
    Tool {
        tool_ref: Id,
        #[serde(default)]
        arguments: BTreeMap<String, InputBinding>,
        #[serde(default)]
        output_mapping: BTreeMap<String, String>,
    },
    Workflow {
        workflow_ref: Id,
        #[serde(default)]
        foreach: Option<CollectionExecution>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        work_binding: Option<WorkBinding>,
    },
    Control,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkBinding {
    pub input: String,
    #[serde(default)]
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionExecution {
    pub source: PortSource,
    pub item_input: String,
    pub max_concurrency: u64,
    pub min_items: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortDefinition {
    pub delivery_ref: Id,
    #[serde(default = "required")]
    pub required: bool,
    #[serde(default)]
    pub content_checks: bool,
}
fn required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortSource {
    pub step: String,
    pub port: String,
    #[serde(default)]
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputBinding {
    Source { source: PortSource },
    Constant { value: Value },
    Material { material_ref: Id },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepAccess {
    #[serde(default)]
    pub workspace_input: Option<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_only_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_refs: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBudget {
    pub max_turns: u64,
    pub timeout_seconds: u64,
    pub retry_limit: u64,
}
impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_turns: 8,
            timeout_seconds: 300,
            retry_limit: 3,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationPolicy {
    #[default]
    Automatic,
    Human,
}

/// Rules combine explicit values and trusted observations, never arbitrary code.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionRule {
    #[default]
    Always,
    All {
        rules: Vec<CompletionRule>,
    },
    Any {
        rules: Vec<CompletionRule>,
    },
    Exists {
        source: PortSource,
    },
    Equals {
        source: PortSource,
        value: Value,
    },
    Same {
        left: PortSource,
        right: PortSource,
    },
    Covers {
        available: PortSource,
        required: PortSource,
    },
    Before {
        first: String,
        second: String,
    },
    Independent {
        author: String,
        reviewer: String,
    },
    Outcome {
        step: String,
        outcome: String,
    },
    Approved {
        step: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowFacts {
    pub values: BTreeMap<(String, String), Value>,
    pub executions: BTreeMap<String, ExecutionFact>,
    pub approvals: BTreeSet<String>,
}

/// Created only by the coordinator from accepted executor receipts.
#[derive(Debug, Clone)]
pub struct ExecutionFact {
    pub operation_id: Id,
    pub input_digest: String,
    pub identity_root: Option<Id>,
    pub started: u64,
    pub finished: u64,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCheckDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assertion: Option<MachineAssertion>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub evidence_required: bool,
    pub key: String,
    pub name: String,
    pub instructions: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCheckResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<EvidenceLocation>,
    pub key: String,
    pub passed: bool,
    pub evidence: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCheckReport {
    pub approve: bool,
    pub summary: String,
    pub checks: Vec<ContentCheckResult>,
}
impl ContentCheckReport {
    pub fn validate(&self, expected: &[ContentCheckDefinition]) -> Result<()> {
        require(
            !expected.is_empty() && !self.summary.trim().is_empty(),
            "内容判断需要明确检查项和摘要",
        )?;
        let keys: BTreeSet<_> = self.checks.iter().map(|c| c.key.as_str()).collect();
        require(
            self.checks.len() == expected.len()
                && keys.len() == self.checks.len()
                && expected.iter().all(|c| keys.contains(c.key.as_str())),
            "内容报告的检查项与当前 Spec 不一致",
        )?;
        require(
            self.checks.iter().all(|c| !c.evidence.trim().is_empty()),
            "检查项需要明确判断依据",
        )?;
        for check in expected {
            let result = self.checks.iter().find(|r| r.key == check.key).unwrap();
            require(
                result.locations.len() <= 16 && result.evidence.len() <= 32768,
                "证据超出数量或文本边界",
            )?;
            require(
                !(check.evidence_required || check.assertion.is_some())
                    || !result.locations.is_empty(),
                "该检查项需要可定位的冻结输入证据",
            )?;
        }
        require(
            self.approve == self.checks.iter().all(|c| c.passed),
            "模型决定与检查项结论不一致",
        )
    }
    pub fn validate_evidence(
        &self,
        expected: &[ContentCheckDefinition],
        authorized: &[EvidenceTarget],
    ) -> Result<()> {
        self.validate(expected)?;
        require(
            self.checks.iter().map(|c| c.locations.len()).sum::<usize>() <= 32,
            "证据位置总数超过 32",
        )?;
        let bytes = authorized.iter().try_fold(0usize, |sum, target| {
            serde_json::to_vec(&target.value)
                .map(|bytes| sum.saturating_add(bytes.len()))
                .map_err(|e| err("invalid_evidence", e.to_string()))
        })?;
        require(
            authorized.len() <= 128 && bytes <= SPEC_VALUE_LIMIT,
            "证据输入超过检查边界",
        )?;
        for check in expected {
            let result = self.checks.iter().find(|r| r.key == check.key).unwrap();
            let targets = result
                .locations
                .iter()
                .map(|location| location.verify(authorized))
                .collect::<Result<Vec<_>>>()?;
            if let Some(assertion) = &check.assertion {
                let observed = targets
                    .iter()
                    .map(|target| assertion.evaluate(&target.value))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|passed| passed);
                require(
                    observed == result.passed,
                    "模型报告与机器检查的实际结论不一致",
                )?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredSpecification {
    pub schema_version: u64,
    pub body: String,
    #[serde(default)]
    pub checks: Vec<ContentCheckDefinition>,
}
impl ConfiguredSpecification {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.body.trim().is_empty() && self.body.len() <= TASK_TEXT_LIMIT,
            "Spec 需要非空正文",
        )?;
        require(
            self.checks.len() <= 128
                && serde_json::to_vec(&self.checks)
                    .map_err(|e| err("invalid_spec", e.to_string()))?
                    .len()
                    <= TASK_TEXT_LIMIT,
            "Spec 检查项超过数量或文本上限",
        )?;
        let mut keys = BTreeSet::new();
        for check in &self.checks {
            if let Some(assertion) = &check.assertion {
                assertion.validate()?;
            }
            valid_key(&check.key)?;
            require(
                keys.insert(&check.key)
                    && !check.name.trim().is_empty()
                    && !check.instructions.trim().is_empty(),
                "检查项需要唯一标识、名称及要求",
            )?;
        }
        Ok(())
    }
}

impl WorkflowFacts {
    pub fn value(&self, source: &PortSource) -> Option<&Value> {
        self.values
            .get(&(source.step.clone(), source.port.clone()))
            .and_then(|value| {
                if source.pointer.is_empty() {
                    Some(value)
                } else {
                    value.pointer(&source.pointer)
                }
            })
    }
}

impl CompletionRule {
    pub fn evaluate(&self, facts: &WorkflowFacts) -> bool {
        match self {
            Self::Always => true,
            Self::All { rules } => rules.iter().all(|r| r.evaluate(facts)),
            Self::Any { rules } => rules.iter().any(|r| r.evaluate(facts)),
            Self::Exists { source } => facts.value(source).is_some(),
            Self::Equals { source, value } => facts.value(source) == Some(value),
            Self::Same { left, right } => facts
                .value(left)
                .zip(facts.value(right))
                .is_some_and(|(a, b)| a == b),
            Self::Covers {
                available,
                required,
            } => facts
                .value(available)
                .and_then(Value::as_array)
                .zip(facts.value(required).and_then(Value::as_array))
                .is_some_and(|(available, required)| {
                    required.iter().all(|item| available.contains(item))
                }),
            Self::Before { first, second } => facts
                .executions
                .get(first)
                .zip(facts.executions.get(second))
                .is_some_and(|(a, b)| a.finished <= b.started),
            Self::Independent { author, reviewer } => facts
                .executions
                .get(author)
                .zip(facts.executions.get(reviewer))
                .and_then(|(a, b)| a.identity_root.as_ref().zip(b.identity_root.as_ref()))
                .is_some_and(|(a, b)| a != b),
            Self::Outcome { step, outcome } => facts
                .executions
                .get(step)
                .is_some_and(|e| &e.outcome == outcome),
            Self::Approved { step } => facts.approvals.contains(step),
        }
    }

    fn validate(
        &self,
        doc: &WorkflowDefinition,
        allowed: &BTreeSet<&str>,
        depth: usize,
    ) -> Result<()> {
        require(depth <= 16, "完成规则嵌套超过限制")?;
        let step_ref =
            |key: &str| require(allowed.contains(key), format!("规则引用未就绪步骤 {key}"));
        match self {
            Self::Always => Ok(()),
            Self::All { rules } | Self::Any { rules } => {
                require(
                    !rules.is_empty() && rules.len() <= 32,
                    "规则组合需要 1—32 个条件",
                )?;
                for rule in rules {
                    rule.validate(doc, allowed, depth + 1)?;
                }
                Ok(())
            }
            Self::Exists { source } | Self::Equals { source, .. } => {
                doc.source(source, allowed).map(|_| ())
            }
            Self::Same { left, right } => {
                doc.source(left, allowed)?;
                doc.source(right, allowed)?;
                Ok(())
            }
            Self::Covers {
                available,
                required,
            } => {
                doc.source(available, allowed)?;
                doc.source(required, allowed)?;
                Ok(())
            }
            Self::Before { first, second } => {
                step_ref(first)?;
                step_ref(second)?;
                require(first != second, "先后规则不能引用同一步骤")
            }
            Self::Independent { author, reviewer } => {
                step_ref(author)?;
                step_ref(reviewer)?;
                require(author != reviewer, "独立性规则不能引用同一步骤")
            }
            Self::Outcome { step, outcome } => {
                step_ref(step)?;
                require(!outcome.trim().is_empty(), "执行结果条件不能为空")
            }
            Self::Approved { step } => step_ref(step),
        }
    }
}

impl WorkflowDefinition {
    pub fn role<'a>(&'a self, step: &'a StepDefinition) -> Option<&'a str> {
        match &step.executor {
            StepExecutor::Agent { role_ref } => role_ref.as_deref().or_else(|| {
                self.stages
                    .iter()
                    .find(|s| s.key == step.stage)
                    .and_then(|s| s.default_role_ref.as_deref())
            }),
            _ => None,
        }
    }

    pub fn ancestors<'a>(&'a self, step: &'a StepDefinition) -> BTreeSet<&'a str> {
        let mut found = BTreeSet::new();
        let mut queue: Vec<&str> = step.dependencies.iter().map(String::as_str).collect();
        while let Some(key) = queue.pop() {
            if found.insert(key) {
                if let Some(parent) = self.steps.iter().find(|s| s.key == key) {
                    queue.extend(parent.dependencies.iter().map(String::as_str));
                }
            }
        }
        found
    }

    pub fn source<'a>(
        &'a self,
        source: &PortSource,
        allowed: &BTreeSet<&str>,
    ) -> Result<&'a PortDefinition> {
        require(
            source.pointer.is_empty()
                || (source.pointer.starts_with('/') && source.pointer.len() <= 1024),
            "无效 JSON Pointer",
        )?;
        if source.step == "$input" {
            return self
                .inputs
                .get(&source.port)
                .ok_or_else(|| err("invalid_workflow", "未声明的流程输入"));
        }
        require(
            allowed.contains(source.step.as_str()),
            format!("输入来源 {} 不在依赖链中", source.step),
        )?;
        self.steps
            .iter()
            .find(|s| s.key == source.step)
            .and_then(|s| s.outputs.get(&source.port))
            .ok_or_else(|| {
                err(
                    "invalid_workflow",
                    format!("输出端口不存在: {}/{}", source.step, source.port),
                )
            })
    }

    pub fn validate(&self) -> Result<Vec<String>> {
        require(
            !self.steps.is_empty() && self.steps.len() <= WORKFLOW_STEP_LIMIT,
            "流程需要 1—1000 个步骤",
        )?;
        let stages: BTreeSet<_> = self.stages.iter().map(|s| s.key.as_str()).collect();
        let keys: BTreeSet<_> = self.steps.iter().map(|s| s.key.as_str()).collect();
        require(
            stages.len() == self.stages.len() && keys.len() == self.steps.len(),
            "流程标识重复",
        )?;
        let mut nodes = BTreeSet::new();
        for node in self
            .stages
            .iter()
            .map(|s| &s.node_id)
            .chain(self.steps.iter().map(|s| &s.node_id))
            .filter(|id| !id.is_empty())
        {
            require(nodes.insert(node), "流程节点 ID 重复")?;
        }
        for stage in &self.stages {
            valid_key(&stage.key)?;
            require(!stage.name.trim().is_empty(), "阶段名称不能为空")?;
        }
        let order = topological(
            &self
                .steps
                .iter()
                .map(|s| (s.key.clone(), s.dependencies.clone()))
                .collect::<Vec<_>>(),
        )?;
        let mut task_bytes = 0;
        for step in &self.steps {
            valid_key(&step.key)?;
            require(
                !step.name.trim().is_empty() && stages.contains(step.stage.as_str()),
                "步骤名称或所属阶段无效",
            )?;
            let ancestors = self.ancestors(step);
            match &step.executor {
                StepExecutor::Agent { .. } => {
                    require(
                        self.role(step).is_some_and(|r| !r.is_empty()),
                        format!("{}: 请选择执行 Role", step.name),
                    )?;
                    let task = step.task_instructions.as_deref().unwrap_or("");
                    require(
                        !task.trim().is_empty() && task.len() <= TASK_TEXT_LIMIT,
                        "Agent 任务必填且最多 32 KiB",
                    )?;
                    task_bytes += task.len();
                }
                executor => {
                    require(
                        step.task_instructions.is_none() && step.required_skill_refs.is_empty(),
                        "非 Agent 步骤不接受 Agent 任务和 Skill",
                    )?;
                    match executor {
                        StepExecutor::Tool {
                            tool_ref,
                            arguments,
                            output_mapping,
                        } => {
                            for (port, pointer) in output_mapping {
                                require(
                                    step.outputs.contains_key(port)
                                        && (pointer.is_empty() || pointer.starts_with('/')),
                                    "工具输出映射必须指向声明的端口和合法 JSON Pointer",
                                )?;
                            }
                            require(!tool_ref.is_empty(), "请选择工具修订")?;
                            for value in arguments.values() {
                                if let InputBinding::Source { source } = value {
                                    self.source(source, &ancestors)?;
                                }
                            }
                        }
                        StepExecutor::Workflow {
                            workflow_ref,
                            foreach,
                            work_binding,
                        } => {
                            if let Some(binding) = work_binding {
                                require(
                                    step.inputs.contains_key(&binding.input)
                                        || foreach
                                            .as_ref()
                                            .is_some_and(|c| c.item_input == binding.input),
                                    "工作配置调用必须引用声明输入或集合成员",
                                )?;
                                require(
                                    binding.pointer.is_empty() || binding.pointer.starts_with('/'),
                                    "工作引用需要合法 JSON Pointer",
                                )?;
                            }
                            require(!workflow_ref.is_empty(), "请选择子流程修订")?;
                            if let Some(collection) = foreach {
                                self.source(&collection.source, &ancestors)?;
                                valid_key(&collection.item_input)?;
                                require(
                                    (1..=32).contains(&collection.max_concurrency)
                                        && collection.min_items <= 1000,
                                    "集合执行预算超出范围",
                                )?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            require(
                (1..=100).contains(&step.budget.max_turns)
                    && (1..=86400).contains(&step.budget.timeout_seconds)
                    && step.budget.retry_limit <= 10,
                "执行预算超出平台限制",
            )?;
            for (name, input) in &step.inputs {
                valid_key(name)?;
                if let InputBinding::Source { source } = input {
                    self.source(source, &ancestors)?;
                }
            }
            for (name, output) in &step.outputs {
                valid_key(name)?;
                require(!output.delivery_ref.is_empty(), "输出缺少交付类型")?;
            }
            for path in step
                .access
                .writable_paths
                .iter()
                .chain(&step.access.read_only_paths)
            {
                valid_resource_path(path)?;
            }
            if let Some(path) = &step.access.working_directory {
                valid_resource_path(path)?;
            }
            if !step.access.writable_paths.is_empty() {
                require(
                    step.access
                        .workspace_input
                        .as_ref()
                        .is_some_and(|key| step.inputs.contains_key(key)),
                    "写执行需要明确工作空间输入",
                )?;
            }
            let unique = |refs: &[Id]| {
                refs.iter().collect::<BTreeSet<_>>().len() == refs.len()
                    && refs.iter().all(|r| !r.is_empty())
            };
            require(
                unique(&step.required_skill_refs)
                    && unique(&step.spec_refs)
                    && unique(&step.access.tool_refs),
                "配置引用不能为空或重复",
            )?;
            step.condition.validate(self, &ancestors, 0)?;
            let mut completion_scope = ancestors;
            completion_scope.insert(&step.key);
            step.completion.validate(self, &completion_scope, 0)?;
        }
        require(
            task_bytes <= TASK_WORKFLOW_LIMIT,
            "流程任务正文合计超过 1 MiB",
        )?;
        for port in self.outputs.values() {
            self.source(port, &keys)?;
        }
        self.completion.validate(self, &keys, 0)?;
        Ok(order)
    }
}

fn valid_key(key: &str) -> Result<()> {
    require(
        !key.is_empty()
            && key.len() <= 80
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "标识需为 1—80 个字母、数字、下划线或连字符",
    )
}
fn valid_resource_path(path: &str) -> Result<()> {
    require(
        !path.is_empty()
            && !path.starts_with(['/', '-'])
            && !path.contains(['\\', '\n', '\r', '\0'])
            && (path == "."
                || path
                    .split('/')
                    .all(|p| !p.is_empty() && p != "." && p != "..")),
        "执行路径必须位于明确授权的工作空间中",
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepExecutionStatus {
    Queued,
    Running,
    WaitingInput,
    WaitingConfirmation,
    AwaitingAcceptance,
    Completed,
    Failed,
    Cancelled,
    Skipped,
    NeedsReconciliation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepReworkContext {
    pub previous_execution_id: Id,
    pub reason: String,
    pub outputs: BTreeMap<String, Id>,
}

/// A new business round starts from an already sealed, platform-owned workspace.
/// This changes execution material, never the immutable Workflow or old execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReworkWorkspaceCheckpoint {
    pub previous_workspace: Value,
    pub output_id: Id,
    pub workspace: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepExecution {
    #[serde(default)]
    pub plan_revision_id: Id,
    #[serde(default)]
    pub node_id: Id,
    #[serde(default)]
    pub contract_digest: String,
    pub id: Id,
    pub workflow_instance_id: Id,
    pub workflow_revision_id: Id,
    pub path: String,
    pub step: String,
    pub round: u64,
    pub sequence: u64,
    pub status: StepExecutionStatus,
    pub input_digest: String,
    #[serde(default)]
    pub budget_wait_started: Option<u64>,
    #[serde(default)]
    pub budget_waited_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rework_workspace: Option<ReworkWorkspaceCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rework: Option<StepReworkContext>,
    pub inputs: BTreeMap<String, Value>,
    /// Whitelisted runtime facts frozen with the actual Agent task, separate from model output.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub runtime_context: Value,
    pub outputs: BTreeMap<String, Id>,
    pub agent_revision_id: Option<Id>,
    pub session_id: Option<Id>,
    pub turn_id: Option<Id>,
    pub tool_call_id: Option<Id>,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredAgentTask {
    pub renderer_version: String,
    pub turn_id: Id,
    pub agent_revision_id: Id,
    pub source: Value,
    pub task: Value,
    pub rendered_input: String,
    pub digest: String,
}
impl ConfiguredAgentTask {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.renderer_version.is_empty()
                && self.task["renderer_version"] == self.renderer_version
                && configuration_hash(&self.task) == self.digest,
            "通用任务摘要或渲染合同不匹配",
        )?;
        let rendered: Value = serde_json::from_str(&self.rendered_input)
            .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
        require(
            rendered == self.task
                && self.task["source"]["execution_id"] == self.source["execution_id"],
            "通用任务正文与来源不匹配",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workflow() -> WorkflowDefinition {
        serde_json::from_value(json!({"schema_version":6,"stages":[{"key":"investigate","name":"安全调查","default_role_ref":"custom-researcher"}],
            "steps":[{"key":"discover","name":"发现风险","stage":"investigate","executor":{"type":"agent"},
            "task_instructions":"使用当前调研 Skill 分析输入并提交风险。","outputs":{"risks":{"delivery_ref":"custom-risk-json"}}},
            {"key":"inspect","name":"核对风险","stage":"investigate","executor":{"type":"agent","role_ref":"custom-auditor"},
            "task_instructions":"按当前 Spec 核对输入的风险。","dependencies":["discover"],
            "inputs":{"risks":{"type":"source","source":{"step":"discover","port":"risks"}}}}]})).unwrap()
    }

    #[test]
    fn arbitrary_business_steps_inherit_roles_without_named_actions() {
        let doc = workflow();
        assert_eq!(doc.validate().unwrap(), ["discover", "inspect"]);
        assert_eq!(doc.role(&doc.steps[0]), Some("custom-researcher"));
        assert_eq!(doc.role(&doc.steps[1]), Some("custom-auditor"));
        assert!(
            serde_json::to_value(&doc).unwrap()["steps"][0]
                .get("kind")
                .is_none()
        );
    }

    #[test]
    fn ports_dependencies_tasks_and_resource_ranges_are_real_constraints() {
        let mut doc = workflow();
        doc.steps[1].dependencies.clear();
        assert!(doc.validate().is_err());
        let mut doc = workflow();
        doc.steps[0].task_instructions = Some(" ".into());
        assert!(doc.validate().is_err());
        let mut doc = workflow();
        doc.steps[0].access.writable_paths = vec!["../outside".into()];
        assert!(doc.validate().is_err());
        let mut doc = workflow();
        doc.steps[0].executor = StepExecutor::Control;
        assert!(doc.validate().is_err());
        let mut doc = workflow();
        doc.steps[0].dependencies.push("inspect".into());
        assert!(doc.validate().is_err());
    }

    #[test]
    fn completion_rules_use_actual_identity_and_order_not_business_labels() {
        let doc = workflow();
        let rule: CompletionRule = serde_json::from_value(json!({"op":"all","rules":[
            {"op":"before","first":"discover","second":"inspect"},
            {"op":"independent","author":"discover","reviewer":"inspect"},
            {"op":"approved","step":"inspect"}]}))
        .unwrap();
        rule.validate(&doc, &doc.steps.iter().map(|s| s.key.as_str()).collect(), 0)
            .unwrap();
        let fact = |identity: &str, started, finished| ExecutionFact {
            operation_id: "test-operation".into(),
            input_digest: "digest".into(),
            identity_root: Some(identity.into()),
            started,
            finished,
            outcome: "succeeded".into(),
        };
        let mut facts = WorkflowFacts::default();
        facts
            .executions
            .insert("discover".into(), fact("same-agent", 1, 2));
        facts
            .executions
            .insert("inspect".into(), fact("same-agent", 3, 4));
        facts.approvals.insert("inspect".into());
        assert!(!rule.evaluate(&facts));
        facts.executions.get_mut("inspect").unwrap().identity_root =
            Some("independent-agent".into());
        assert!(rule.evaluate(&facts));
        facts.executions.get_mut("inspect").unwrap().started = 1;
        assert!(!rule.evaluate(&facts));
    }

    #[test]
    fn missing_values_never_satisfy_equality_or_coverage() {
        let source = PortSource {
            step: "work".into(),
            port: "result".into(),
            pointer: "/items".into(),
        };
        let mut facts = WorkflowFacts::default();
        let rule = CompletionRule::Same {
            left: source.clone(),
            right: source.clone(),
        };
        assert!(!rule.evaluate(&facts));
        facts.values.insert(
            ("work".into(), "result".into()),
            json!({"items":["security","performance"]}),
        );
        assert!(rule.evaluate(&facts));
        assert!(
            CompletionRule::Equals {
                source,
                value: json!(["security", "performance"])
            }
            .evaluate(&facts)
        );
    }
}

impl EngineeringState {
    pub fn validate_configured_tasks(&self, before: &Self) -> Result<()> {
        // An immutable source plan is shared by many executions. Parse and hash
        // each contract once per transaction rather than once per execution.
        let mut contracts: BTreeMap<Id, BTreeMap<Id, String>> = BTreeMap::new();
        let mut normative_workflows: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
        for record in self
            .governance
            .iter()
            .filter(|r| r.r#type == "configured_execution_task")
        {
            let task: ConfiguredAgentTask = record.decode()?;
            task.validate()?;
            let turn = self
                .turns
                .iter()
                .find(|t| t.id == task.turn_id)
                .ok_or_else(|| err("task_contract_mismatch", "任务轮次不存在"))?;
            let session = self
                .sessions
                .iter()
                .find(|s| s.id == turn.session_id)
                .ok_or_else(|| err("task_contract_mismatch", "任务会话不存在"))?;
            require(
                record.definition_id == task.turn_id
                    && turn.input == task.rendered_input
                    && session.agent == task.agent_revision_id,
                "任务、输入和 Agent 归属不一致",
            )?;
        }
        for record in self
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance")
        {
            require(
                record.data["steps"].is_array(),
                "过时 Workflow 运行数据需要显式离线升级",
            )?;
            if let Some(old) = before.governance.iter().find(|r| r.id == record.id) {
                for key in [
                    "owner_id",
                    "project_id",
                    "parent_execution_id",
                    "root_instance_id",
                ] {
                    require(
                        record.data[key] == old.data[key],
                        format!("运行时冻结配置不可改变: {key}"),
                    )?;
                }
            }
            let plan = self.configuration(
                record.data["active_plan_revision_id"]
                    .as_str()
                    .unwrap_or(""),
            )?;
            require(
                plan.r#type == "workflow_plan_revision"
                    && plan.immutable
                    && plan.data["plan"] == record.data["plan"]
                    && plan.data["inputs"] == record.data["inputs"]
                    && plan.data["role_agent_bindings"] == record.data["role_agent_bindings"],
                "实例内容与不可变运行计划不匹配",
            )?;
            let standard = plan.data["standard_ref"]
                .as_str()
                .ok_or_else(|| err("invalid_standard", "运行计划缺少 Standard"))?;
            require(
                plan.data["standard_ref"] == record.data["plan"]["standard_ref"]
                    && plan.data["owner_id"] == record.data["owner_id"],
                "运行计划的规范采用与工作不一致",
            )?;
            let scope = Scope::project(record.data["project_id"].as_str().unwrap_or(""));
            require(
                self.configuration(standard)?.scope.visible_from(&scope),
                "Standard 源不可见",
            )?;
            if !normative_workflows.contains_key(standard) {
                normative_workflows.insert(
                    standard.into(),
                    standard_snapshot(self, standard, &scope)?
                        .nodes
                        .into_iter()
                        .filter(|n| n.kind == "workflow")
                        .map(|n| n.reference)
                        .collect(),
                );
            }
            require(
                plan.data["workflow_ref"]
                    .as_str()
                    .is_some_and(|r| normative_workflows[standard].contains(r)),
                "运行 Workflow 不在所采用的 Standard 闭包中",
            )?;
            let steps: Vec<StepExecution> = serde_json::from_value(record.data["steps"].clone())
                .map_err(|e| err("invalid_workflow", e.to_string()))?;
            let mut ids = BTreeSet::new();
            for step in steps {
                if let Some(previous) = before
                    .governance
                    .iter()
                    .find(|r| r.id == record.id)
                    .and_then(|r| r.data["steps"].as_array())
                    .and_then(|a| a.iter().find(|e| e["id"] == step.id))
                {
                    if !previous["started"].is_null() {
                        let current = serde_json::to_value(&step)
                            .map_err(|e| err("invalid_workflow", e.to_string()))?;
                        for key in [
                            "inputs",
                            "rework_workspace",
                            "rework",
                            "input_digest",
                            "plan_revision_id",
                            "node_id",
                            "contract_digest",
                            "path",
                            "workflow_revision_id",
                        ] {
                            require(
                                current[key] == previous[key],
                                format!("已派发执行的 {key} 不可修改"),
                            )?;
                        }
                    }
                }
                let source_plan = self.configuration(&step.plan_revision_id)?;
                if !contracts.contains_key(&source_plan.id) {
                    let source: WorkflowDefinition =
                        serde_json::from_value(source_plan.data["plan"]["document"].clone())
                            .map_err(|e| err("invalid_workflow", e.to_string()))?;
                    contracts.insert(
                        source_plan.id.clone(),
                        source
                            .steps
                            .iter()
                            .map(|s| (s.node_id.clone(), configuration_hash(s)))
                            .collect(),
                    );
                }
                require(
                    contracts[&source_plan.id].get(&step.node_id) == Some(&step.contract_digest),
                    "执行的冻结合同与计划不匹配",
                )?;
                require(
                    ids.insert(step.id.clone()) && step.workflow_instance_id == record.id,
                    "步骤身份重复或归属错误",
                )?;
                if let Some(rework) = &step.rework {
                    require(
                        self.governance.iter().any(|old| {
                            old.r#type == "workflow_execution_history"
                                && old.definition_id == rework.previous_execution_id
                                && old.data["workflow_instance_id"] == record.id
                                && old.data["step"] == step.step
                        }),
                        "返工来源执行不属于当前步骤历史",
                    )?;
                    for output in rework.outputs.values() {
                        require(
                            self.governance.iter().any(|old| {
                                old.r#type == "workflow_execution_history"
                                    && old.data["workflow_instance_id"] == record.id
                                    && old.data["step"] == step.step
                                    && old.data["outputs"].as_object().is_some_and(|outputs| {
                                        outputs.values().any(|id| id == output)
                                    })
                            }),
                            "返工证据不属于当前步骤历史",
                        )?;
                    }
                }
                if let Some(checkpoint) = &step.rework_workspace {
                    let output = self.configuration(&checkpoint.output_id)?;
                    require(
                        output.r#type == "workflow_output"
                            && output.data["value"] == checkpoint.workspace
                            && output.data["provenance"]["kind"] == "platform"
                            && output.data["provenance"]["tool"] == "workspace.snapshot"
                            && output.data["provenance"]["pointer"] == ""
                            && checkpoint.workspace["owner_id"] == record.data["owner_id"]
                            && checkpoint.previous_workspace["owner_id"]
                                == checkpoint.workspace["owner_id"]
                            && checkpoint.previous_workspace["workspace_id"]
                                == checkpoint.workspace["workspace_id"],
                        "返工检查点与真实平台快照或工作区归属不匹配",
                    )?;
                }
                for output in step.outputs.values() {
                    let output = self.configuration(output)?;
                    require(
                        output.r#type == "workflow_output"
                            && output.data["execution_id"] == step.id
                            && output.data["input_digest"] == step.input_digest,
                        "步骤产物版本不匹配",
                    )?;
                }
            }
        }
        for row in self
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_controller")
        {
            let controller: WorkflowController = row.decode()?;
            controller.policy.validate()?;
            require(
                controller.policy_digest == configuration_hash(&controller.policy),
                "编排策略摘要不一致",
            )?;
            if let Some(previous) = before.governance.iter().find(|r| r.id == row.id) {
                for key in [
                    "project_id",
                    "owner_id",
                    "agent_revision_id",
                    "role_ref",
                    "policy",
                    "policy_digest",
                ] {
                    require(
                        row.data[key] == previous.data[key],
                        "编排授权身份不可原地扩大",
                    )?;
                }
                if previous.data["root_instance_id"].is_string() {
                    require(
                        row.data["root_instance_id"] == previous.data["root_instance_id"],
                        "编排根实例不可改绑",
                    )?;
                }
                require(
                    row.data["epoch"].as_u64() >= previous.data["epoch"].as_u64(),
                    "编排代次不能回退",
                )?;
            }
        }
        for previous in &before.governance {
            let fields: &[&str] = match previous.r#type.as_str() {
                "controller_tool_call" => &[
                    "method",
                    "arguments",
                    "session_id",
                    "turn_id",
                    "worker_operation_id",
                    "request_id",
                    "call_id",
                    "tool_ref",
                    "epoch",
                ],
                "workflow_segment" => &["plan_revision_id", "steps", "actor", "max_operations"],
                "workflow_change" => &["changeset", "preview", "actor"],
                "workflow_pending_decision" => &["plan_revision_id", "request", "actor"],
                "controller_input" => &["input"],
                _ => continue,
            };
            let current = self.configuration(&previous.id)?;
            require(
                current.r#type == previous.r#type
                    && current.definition_id == previous.definition_id,
                "编排来源记录不能删除或改绑",
            )?;
            for field in fields {
                require(
                    current.data[field] == previous.data[field],
                    "编排请求与授权来源不可改写",
                )?;
            }
            if previous.r#type == "controller_tool_call"
                && ["succeeded", "failed"].contains(&previous.data["status"].as_str().unwrap_or(""))
            {
                require(current.data == previous.data, "编排调用终态不可改写")?;
            }
            if previous.r#type == "workflow_segment" {
                require(
                    current.data["operations"].as_u64() >= previous.data["operations"].as_u64(),
                    "委托预算不能回退",
                )?;
            }
        }
        for record in self
            .governance
            .iter()
            .filter(|r| r.r#type == "platform_call")
        {
            if let Some(old) = before.governance.iter().find(|r| r.id == record.id) {
                require(
                    record.data["request"] == old.data["request"],
                    "平台工具的冻结请求不可改写",
                )?;
                if ["succeeded", "failed", "cancelled"]
                    .contains(&old.data["status"].as_str().unwrap_or(""))
                {
                    require(record.data == old.data, "平台工具终态回执不可改写")?;
                }
            }
        }
        Ok(())
    }
}

impl EngineeringState {
    /// A paused ancestor suspends the whole invocation subtree, preserving each child's own pause choice.
    pub fn workflow_instance_suspended(&self, instance_id: &str) -> bool {
        let mut current = Some(instance_id);
        for _ in 0..=WORKFLOW_DEPTH_LIMIT {
            let Some(row) = current.and_then(|id| {
                self.governance
                    .iter()
                    .find(|r| r.id == id && r.r#type == "workflow_instance")
            }) else {
                return false;
            };
            if row.data["paused"] == true {
                return true;
            }
            current = row.data["parent_execution_id"]
                .as_str()
                .and_then(|execution| {
                    self.governance.iter().find(|r| {
                        r.r#type == "workflow_instance"
                            && r.data["steps"].as_array().is_some_and(|steps| {
                                steps.iter().any(|step| step["id"] == execution)
                            })
                    })
                })
                .map(|r| r.id.as_str());
        }
        true
    }
}
