//! Independent configuration resources and explicit workflow specifications.
use crate::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DefinitionVisibility {
    #[default]
    Global,
    Projects {
        project_ids: Vec<Id>,
    },
}
impl DefinitionVisibility {
    pub fn validate(&self) -> Result<()> {
        if let Self::Projects { project_ids } = self {
            require(
                !project_ids.is_empty() && project_ids.len() <= 1000,
                "请选择明确的项目范围",
            )?;
            let unique: std::collections::BTreeSet<_> = project_ids.iter().collect();
            require(
                unique.len() == project_ids.len() && project_ids.iter().all(|id| !id.is_empty()),
                "项目范围不得重复或为空",
            )?;
        }
        Ok(())
    }
    pub fn permits(&self, project: Option<&str>) -> bool {
        match self {
            Self::Global => true,
            Self::Projects { project_ids } => {
                project.is_some_and(|id| project_ids.iter().any(|p| p == id))
            }
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDefinition {
    pub model_ref: Id,
    pub harness_ref: Id,
    #[serde(default)]
    pub tools: Vec<Id>,
    #[serde(default)]
    pub skills: Vec<Id>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSelector {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_kind: Option<String>,
}
impl ActionSelector {
    pub fn validate(&self) -> Result<()> {
        require(
            workflow_action_catalog()
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(self.kind)),
            "规范选择器指向未知动作",
        )?;
        require(
            self.phase.as_ref().is_none_or(|s| !s.is_empty())
                && self.delivery_kind.as_ref().is_none_or(|s| !s.is_empty()),
            "规范选择器的上下文不能为空字符串",
        )
    }
    pub fn matches(&self, kind: &str, phase: Option<&str>, delivery_kind: Option<&str>) -> bool {
        self.kind == kind
            && self.phase.as_deref().is_none_or(|p| Some(p) == phase)
            && self
                .delivery_kind
                .as_deref()
                .is_none_or(|p| Some(p) == delivery_kind)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecificationDefinition {
    pub body: String,
    #[serde(default)]
    pub applicable_actions: Vec<ActionSelector>,
    #[serde(default)]
    pub rules: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRule {
    pub selector: ActionSelector,
    pub required_spec_refs: Vec<Id>,
    pub min_matches: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardDefinition {
    pub body: String,
    pub baseline_rules: Vec<BaselineRule>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecificationBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<WorkflowTaskDefinition>,
    pub path: String,
    pub step: String,
    pub action: String,
    pub phase: Option<String>,
    pub delivery_kind: Option<String>,
    pub role_ref: Option<Id>,
    pub spec_refs: Vec<Id>,
    pub checks: BTreeMap<Id, Vec<String>>,
    pub output_contract: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineCheck {
    pub standard_ref: Id,
    pub valid: bool,
    pub errors: Vec<String>,
}
pub fn check_baseline(
    id: &str,
    standard: &StandardDefinition,
    bindings: &[SpecificationBinding],
) -> Result<BaselineCheck> {
    require(
        !standard.body.trim().is_empty() && !standard.baseline_rules.is_empty(),
        "Standard 需要正文与覆盖规则",
    )?;
    let mut errors = vec![];
    for (index, rule) in standard.baseline_rules.iter().enumerate() {
        rule.selector.validate()?;
        require(
            rule.required_spec_refs.iter().all(|id| !id.is_empty())
                && rule
                    .required_spec_refs
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == rule.required_spec_refs.len(),
            "基线 Spec 引用不得为空或重复",
        )?;
        require(
            rule.min_matches > 0 && !rule.required_spec_refs.is_empty(),
            "基线需要非零节点下限与明确 Spec",
        )?;
        let matches: Vec<_> = bindings
            .iter()
            .filter(|b| {
                rule.selector
                    .matches(&b.action, b.phase.as_deref(), b.delivery_kind.as_deref())
            })
            .collect();
        if (matches.len() as u64) < rule.min_matches {
            errors.push(format!(
                "规则 {}: {} 至少需要 {} 个匹配节点，实际 {}",
                index + 1,
                rule.selector.kind,
                rule.min_matches,
                matches.len()
            ));
        }
        for binding in matches {
            for spec in &rule.required_spec_refs {
                if !binding.spec_refs.contains(spec) {
                    errors.push(format!("{}: 缺少 Spec {spec}", binding.path));
                }
            }
        }
    }
    Ok(BaselineCheck {
        standard_ref: id.into(),
        valid: errors.is_empty(),
        errors,
    })
}

pub fn validate_workflow_v2(doc: &WorkflowDocument) -> Result<Vec<String>> {
    require(
        ["feature", "task", "project_intake", "subworkflow"].contains(&doc.process_kind.as_str()),
        "无效流程用途",
    )?;
    require(
        !doc.steps.is_empty() && doc.steps.len() <= 1000,
        "流程需要 1—1000 个节点",
    )?;
    let stages: std::collections::BTreeSet<_> = doc.stages.iter().map(|s| s.key.as_str()).collect();
    require(stages.len() == doc.stages.len(), "阶段 key 重复")?;
    let keys: std::collections::BTreeSet<_> = doc.steps.iter().map(|s| s.key.as_str()).collect();
    require(keys.len() == doc.steps.len(), "步骤 key 重复")?;
    let catalog = workflow_action_catalog();
    for step in &doc.steps {
        require(
            !step.key.is_empty()
                && step.key.len() <= 80
                && step
                    .key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
            "步骤 key 格式错误",
        )?;
        require(stages.contains(step.stage.as_str()), "阶段不存在")?;
        require(
            step.contract_version == 1
                && catalog
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(step.kind)),
            "未知动作或契约版本",
        )?;
        crate::governance::validate_condition(&step.condition, &keys)?;
        require(step.retry_limit <= 10, "自动重试上限不能大于 10")?;
        require(
            step.rework_scope.iter().all(|s| keys.contains(s.as_str())),
            "返工范围包含未知步骤",
        )?;
        if [
            "agent.interact",
            "attempt.execute",
            "delivery.review",
            "merge.prepare",
            "merge.review",
        ]
        .contains(&step.kind.as_str())
        {
            require(
                step.role_ref.as_ref().is_some_and(|r| !r.is_empty()) && !step.spec_refs.is_empty(),
                format!("{}: 请选择 Role 和 Spec", step.key),
            )?;
        }
        if step.kind == "agent.interact" {
            require(
                !step.interaction.goal.trim().is_empty()
                    && (1..=100).contains(&step.interaction.max_turns)
                    && ["requirement", "feature", "design", "task_plan", "test_plan"]
                        .contains(&step.interaction.output_kind.as_str()),
                "讨论目标、轮数或输出契约不支持",
            )?;
        }
        if step.kind == "workflow.call" {
            require(step.subworkflow_ref.is_some(), "请选择子流程版本")?;
        }
        for input in step.inputs.values() {
            require(
                if input.step == "$input" {
                    doc.input_ports.contains(&input.port)
                } else {
                    keys.contains(input.step.as_str()) && input.step != step.key
                },
                "输入引用未知步骤或端口",
            )?;
        }
        require(
            step.spec_refs
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == step.spec_refs.len(),
            "Spec 引用重复",
        )?;
        if step.kind == "attempt.execute" {
            let phase = step
                .parameters
                .get("phase")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let required: &[&str] = match phase {
                "requirement_definition" => &["requirement_validate"],
                "solution_planning" => &["plan_validate", "test_plan_validate"],
                "implementation" => &["test_author", "red", "implement", "green", "docs", "verify"],
                "closure" => &["test_author", "feature_tests", "docs", "verify"],
                _ => return Err(err("invalid_workflow", "未知执行阶段")),
            };
            for key in required {
                require(
                    step.execution_recipe.iter().any(|n| n.key == *key),
                    format!("{phase}: 缺少执行节点 {key}"),
                )?;
            }
        }
        if !step.execution_recipe.is_empty() {
            valid_dag(
                &step
                    .execution_recipe
                    .iter()
                    .map(|n| (n.key.clone(), n.dependencies.clone()))
                    .collect::<Vec<_>>(),
            )?;
            for node in &step.execution_recipe {
                require(
                    node.role_ref.is_some(),
                    format!("执行节点 {} 缺少 Role", node.key),
                )?;
                require(
                    [
                        "test_author",
                        "red",
                        "implement",
                        "green",
                        "docs",
                        "verify",
                        "feature_tests",
                        "requirement_validate",
                        "plan_validate",
                        "test_plan_validate",
                    ]
                    .contains(&node.key.as_str()),
                    "未知执行节点",
                )?;
                require(
                    match node.key.as_str() {
                        "implement" => node.role == AgentRole::Developer && node.writes,
                        "test_author" => node.role == AgentRole::Tester && node.writes,
                        "docs" => node.role == AgentRole::Developer && !node.writes,
                        "requirement_validate" => node.role == AgentRole::Analyst && !node.writes,
                        "plan_validate" => node.role == AgentRole::Architect && !node.writes,
                        _ => node.role == AgentRole::Tester && !node.writes,
                    },
                    "执行配方职责或写范围不兼容",
                )?;
            }
            if step
                .parameters
                .get("phase")
                .and_then(serde_json::Value::as_str)
                == Some("implementation")
            {
                for required in ["test_author", "red", "implement", "green", "docs", "verify"] {
                    require(
                        step.execution_recipe.iter().any(|n| n.key == required),
                        format!("缺少 TDD 节点 {required}"),
                    )?;
                }
                for (node, deps) in [
                    ("implement", vec!["red"]),
                    ("green", vec!["implement"]),
                    ("verify", vec!["green", "docs"]),
                ] {
                    require(
                        step.execution_recipe
                            .iter()
                            .find(|n| n.key == node)
                            .is_some_and(|n| {
                                deps.iter().all(|d| n.dependencies.iter().any(|v| v == d))
                            }),
                        "TDD 节点依赖不完整",
                    )?;
                }
            }
        }
    }
    for port in doc.output_ports.values() {
        require(keys.contains(port.step.as_str()), "输出引用未知步骤")?;
    }
    validate_workflow_ports(doc)?;
    topological(
        &doc.steps
            .iter()
            .map(|s| (s.key.clone(), s.dependencies.clone()))
            .collect::<Vec<_>>(),
    )
}

/// Validate ports against declared action contracts; workflow.call ports are checked after expansion.
pub fn validate_workflow_ports(doc: &WorkflowDocument) -> Result<()> {
    fn output(doc: &WorkflowDocument, step: &str, port: &str) -> bool {
        let Some(source) = doc.steps.iter().find(|s| s.key == step) else {
            return false;
        };
        let ports: &[&str] = match source.kind.as_str() {
            "agent.interact" => &["session", "proposal", "document"],
            "proposal.adopt" => &["proposal", "document"],
            "attempt.execute" => &["attempt", "delivery"],
            "delivery.review" => &["delivery"],
            "merge.prepare" | "merge.review" => &["merge", "delivery"],
            "merge.apply" => &["merge"],
            "snapshot.capture" => &["snapshot"],
            "tasks.foreach" => &["group"],
            "workflow.call" => {
                return source
                    .parameters
                    .get("call_outputs")
                    .is_none_or(|p| p.get(port).is_some());
            }
            _ => &[],
        };
        ports.contains(&port)
    }
    for step in &doc.steps {
        let mut ancestors = std::collections::BTreeSet::new();
        let mut queue = step.dependencies.clone();
        while let Some(key) = queue.pop() {
            if ancestors.insert(key.clone()) {
                if let Some(s) = doc.steps.iter().find(|s| s.key == key) {
                    queue.extend(s.dependencies.clone());
                }
            }
        }
        for (name, input) in &step.inputs {
            require(
                if input.step == "$input" {
                    doc.input_ports.contains(&input.port)
                } else {
                    ancestors.contains(&input.step) && output(doc, &input.step, &input.port)
                },
                format!("{}: 输入 {name} 端口错误或来源不在依赖链上", step.key),
            )?;
        }
    }
    for port in doc.output_ports.values() {
        require(output(doc, &port.step, &port.port), "输出端口不存在")?;
    }
    Ok(())
}
