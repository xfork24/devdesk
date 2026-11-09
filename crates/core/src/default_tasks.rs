//! Shipped task text is ordinary Workflow content, never a dispatch-time fallback.
use crate::*;
macro_rules! task {
    ($name:literal) => {
        include_str!(concat!(
            "../../../resources/configuration/tasks/",
            $name,
            ".md"
        ))
    };
}
pub(crate) fn legacy_goal(key: &str, task: bool) -> Option<&'static str> {
    Some(match key {
        "requirement_discussion" | "requirement_adopt" => task!("prd"),
        "requirement_execute" => task!("attempt-requirement"),
        "requirement_review" => task!("review-requirement"),
        "design_discussion" | "design_adopt" => task!("design"),
        "task_discussion" | "task_adopt" => task!("planning"),
        "test_discussion" | "test_adopt" => task!("discussion-tests"),
        "solution_execute" => task!("attempt-planning"),
        "solution_review" => task!("review-plan"),
        "implementation" => task!("attempt-implementation"),
        "review" => task!("review-task"),
        "closure" => task!("attempt-closure"),
        "closure_review" => task!("review-feature"),
        "merge_review" if task => task!("review-candidate-task"),
        "merge_review" => task!("review-candidate-feature"),
        _ => return None,
    })
}
pub(crate) fn legacy_node_prompts(phase: &str) -> Value {
    match phase {
        "requirement_definition" => json!({"requirement_validate":task!("validate-requirement")}),
        "solution_planning" => {
            json!({"plan_validate":task!("validate-plan"),"test_plan_validate":task!("validate-tests")})
        }
        "implementation" => {
            json!({"test_author":task!("author-task-tests"),"implement":task!("implement"),"docs":task!("docs")})
        }
        "closure" => json!({"test_author":task!("author-feature-tests"),"docs":task!("docs")}),
        _ => json!({}),
    }
}
fn phase(doc: &WorkflowDocument, step: &WorkflowStep, seen: &mut Vec<String>) -> String {
    if seen.contains(&step.key) {
        return String::new();
    }
    seen.push(step.key.clone());
    if step.kind == "agent.interact" {
        return step.interaction.output_kind.clone();
    }
    if let Some(p) = step.parameters.get("phase").and_then(Value::as_str) {
        return p.into();
    }
    step.dependencies
        .iter()
        .filter_map(|key| doc.steps.iter().find(|s| &s.key == key))
        .map(|s| phase(doc, s, seen))
        .find(|p| !p.is_empty())
        .unwrap_or_default()
}
pub(crate) fn upgrade_document(
    doc: &WorkflowDocument,
    preserve_goal: bool,
) -> Result<WorkflowDocument> {
    require(
        matches!(doc.schema_version, 2 | 3),
        "请先将旧 v1 流程显式转换为 Role/Spec 版本",
    )?;
    if doc.schema_version == 3 {
        validate_workflow(doc)?;
        return Ok(doc.clone());
    }
    let mut next = doc.clone();
    next.schema_version = 3;
    for step in &mut next.steps {
        step.contract_version = if action_requires_task(&step.kind) {
            2
        } else {
            1
        };
        if action_requires_task(&step.kind) {
            let p = phase(doc, step, &mut vec![]);
            let body = match step.kind.as_str() {
                _ if preserve_goal && !step.interaction.goal.trim().is_empty() => {
                    step.interaction.goal.as_str()
                }
                "agent.interact" => match step.interaction.output_kind.as_str() {
                    "requirement" => task!("prd"),
                    "design" => task!("design"),
                    "task_plan" => task!("planning"),
                    "test_plan" => task!("discussion-tests"),
                    "feature" => task!("feature"),
                    _ => {
                        return Err(err(
                            "task_upgrade_required",
                            "未知讨论输出，需要人工填写任务",
                        ));
                    }
                },
                "attempt.execute" => match p.as_str() {
                    "requirement_definition" => task!("attempt-requirement"),
                    "solution_planning" => task!("attempt-planning"),
                    "implementation" => task!("attempt-implementation"),
                    "closure" => task!("attempt-closure"),
                    _ => return Err(err("task_upgrade_required", "未知阶段，无法证明任务映射")),
                },
                "delivery.review" => match p.as_str() {
                    "requirement_definition" | "requirement" => task!("review-requirement"),
                    "solution_planning" | "plan" => task!("review-plan"),
                    "implementation" => task!("review-task"),
                    "closure" => task!("review-feature"),
                    _ => {
                        return Err(err(
                            "task_upgrade_required",
                            "无法唯一确定交付评审边界，请先补齐依赖",
                        ));
                    }
                },
                "merge.review" if doc.process_kind == "task" => task!("review-candidate-task"),
                "merge.review" => task!("review-candidate-feature"),
                _ => return Err(err("task_upgrade_required", "没有已验证的旧动作任务映射")),
            };
            step.task_instructions = Some(body.into());
            let node_prompts = step
                .parameters
                .get("agent_prompts")
                .cloned()
                .unwrap_or(json!({}));
            for node in &mut step.execution_recipe {
                node.task_instructions = if recipe_node_is_agent(&node.key) {
                    Some(
                        match node_prompts[&node.key].as_str() {
                            Some(prompt) => prompt,
                            None => match node.key.as_str() {
                                "requirement_validate" => task!("validate-requirement"),
                                "plan_validate" => task!("validate-plan"),
                                "test_plan_validate" => task!("validate-tests"),
                                "test_author" if p == "closure" => task!("author-feature-tests"),
                                "test_author" => task!("author-task-tests"),
                                "implement" => task!("implement"),
                                "docs" => task!("docs"),
                                _ => {
                                    return Err(err("task_upgrade_required", "未知 Agent 配方节点"));
                                }
                            },
                        }
                        .into(),
                    )
                } else {
                    None
                };
            }
            step.parameters.remove("agent_prompts");
        }
        step.interaction.goal.clear();
    }
    validate_workflow(&next)?;
    Ok(next)
}
pub(crate) fn seed_task_workflows(s: &mut EngineeringState) {
    for (old, id) in [
        (DEFAULT_TASK_WORKFLOW_V2_ID, DEFAULT_TASK_WORKFLOW_V3_ID),
        (
            DEFAULT_FEATURE_WORKFLOW_V2_ID,
            DEFAULT_FEATURE_WORKFLOW_V3_ID,
        ),
    ] {
        if s.governance.iter().any(|r| r.id == id) {
            continue;
        }
        let parent = s.configuration(old).expect("embedded v2 workflow").clone();
        let mut doc =
            upgrade_document(&parent.decode().unwrap(), false).expect("embedded v3 tasks");
        if doc.process_kind == "feature" {
            doc.subworkflow_refs
                .insert("task".into(), DEFAULT_TASK_WORKFLOW_V3_ID.into());
        }
        let previous = s
            .governance
            .iter()
            .filter(|r| r.kind == "workflow" && r.definition_id == parent.definition_id)
            .max_by_key(|r| r.version)
            .map(|r| r.id.clone())
            .unwrap_or(parent.id);
        let mut r = crate::governance::record(
            "workflow",
            parent.definition_id,
            parent.scope,
            parent.name,
            4,
            Some(previous),
            json!(doc),
            true,
        );
        r.id = id.into();
        r.created_at = 0;
        s.governance.push(r);
    }
}

pub(crate) fn upgrade_draft(doc: &WorkflowDocument) -> Result<(WorkflowDocument, Vec<String>)> {
    require(
        matches!(doc.schema_version, 2 | 3),
        "请先将 v1 流程转换为显式 Role/Spec 版本",
    )?;
    match upgrade_document(doc, true) {
        Ok(doc) => Ok((doc, vec![])),
        Err(error) => {
            let mut next = doc.clone();
            next.schema_version = 3;
            for step in &mut next.steps {
                step.contract_version = if action_requires_task(&step.kind) {
                    2
                } else {
                    1
                };
                step.task_instructions =
                    if step.kind == "agent.interact" && !step.interaction.goal.trim().is_empty() {
                        Some(step.interaction.goal.clone())
                    } else {
                        None
                    };
                step.interaction.goal.clear();
                for node in &mut step.execution_recipe {
                    node.task_instructions = None;
                }
            }
            Ok((
                next,
                vec![format!(
                    "任务映射需要人工补充：{}。请填写全部 Agent 任务后发布。",
                    error.message
                )],
            ))
        }
    }
}
