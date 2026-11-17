//! Human-facing evidence references remain bound to the exact approved action.
use crate::*;

pub(crate) fn instance_ancestors(state: &EngineeringState, id: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut parent = state
        .configuration(id)
        .ok()
        .and_then(|r| r.data["parent_execution_id"].as_str());
    while let Some(execution_id) = parent {
        let Some(owner) = state.governance.iter().find(|r| {
            r.r#type == "workflow_instance"
                && r.data["steps"]
                    .as_array()
                    .is_some_and(|steps| steps.iter().any(|e| e["id"] == execution_id))
        }) else {
            break;
        };
        if ancestors.contains(&owner.id) {
            break;
        }
        ancestors.push(owner.id.clone());
        parent = owner.data["parent_execution_id"].as_str();
    }
    ancestors
}

pub(crate) fn decision_view(state: &EngineeringState, row: &ConfigurationRecord) -> Value {
    let mut view = json!(row);
    let request = &row.data["request"];
    let current = state
        .configuration(&row.definition_id)
        .is_ok_and(|instance| {
            if instance.data["active_plan_revision_id"] != row.data["plan_revision_id"] {
                return false;
            }
            if let Some(id) = request["arguments"]["execution_id"].as_str() {
                return instance.data["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|s| {
                        s["id"] == id
                            && s["input_digest"] == request["arguments"]["input_digest"]
                            && s["outputs"] == request["arguments"]["outputs"]
                            && ["waiting_confirmation", "awaiting_acceptance"]
                                .contains(&s["status"].as_str().unwrap_or(""))
                    });
            }
            true
        });
    view["current"] = json!(current);
    view["ancestor_instance_ids"] = json!(instance_ancestors(state, &row.definition_id));
    view
}

pub(crate) fn pending_decisions(state: &EngineeringState, root: &str) -> Vec<Value> {
    state
        .governance
        .iter()
        .filter(|r| {
            r.r#type == "workflow_pending_decision"
                && r.data["status"] == "pending"
                && state
                    .configuration(&r.definition_id)
                    .is_ok_and(|instance| instance.data["root_instance_id"] == root)
        })
        .map(|r| decision_view(state, r))
        .filter(|v| v["current"] == true)
        .collect()
}

/// Gates exist even before the model authors a decision request. Keep both
/// human entry points bound to the same current execution and control version.
pub(crate) fn approval_context(state: &EngineeringState, root: &str) -> Result<Vec<Value>> {
    let mut context = pending_decisions(state, root);
    for approval in step_approvals(state, root)?
        .into_iter()
        .filter(|a| a["human_required"] == true)
    {
        let arguments = &approval["arguments"];
        if context
            .iter()
            .any(|d| d["data"]["request"]["arguments"]["execution_id"] == arguments["execution_id"])
        {
            continue;
        }
        context.push(json!({"id":arguments["execution_id"],"revision":arguments["control_version"],"approval_source":"workflow_step",
            "definition_id":approval["instance_id"],"current":true,
            "data":{"status":"pending","plan_revision_id":arguments["plan_revision_id"],
                "request":{"operation":"workflow.step.confirm","arguments":arguments,"interaction":approval["interaction"]}}}));
    }
    Ok(context)
}

pub(crate) fn human_gate_summary(state: &EngineeringState, root: &str) -> Result<Value> {
    let gates: Vec<_> = step_approvals(state, root)?
        .into_iter()
        .filter(|a| a["human_required"] == true)
        .collect();
    Ok(
        json!({"total":gates.len(),"items":gates.iter().take(16).map(|a| json!({
        "instance_id":a["instance_id"],"instance_name":a["instance_name"],"owner_id":a["owner_id"],
        "step":a["arguments"]["step"],"step_name":a["step_name"],"execution_id":a["arguments"]["execution_id"],
        "plan_revision_id":a["arguments"]["plan_revision_id"],"title":a["interaction"]["title"],
        "confirmation_policy":"human","human_satisfied":false
    })).collect::<Vec<_>>()}),
    )
}

// Only explicit, unconditional human approval phrases are executable. Other prose
// remains a message for the manager; the model never acquires human signing rights.
pub(crate) fn explicit_approval(input: &str, interaction: &Value) -> bool {
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    let clauses: Vec<_> = input
        .split(['，', ',', '。', '！', '!', '\n', ';', '；'])
        .map(normalize)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(first) = clauses.first() else {
        return false;
    };
    if clauses.iter().skip(1).any(|s| {
        ![
            "继续",
            "请继续",
            "继续执行",
            "继续流程",
            "继续后续流程",
            "继续后续步骤",
            "继续执行后续步骤",
            "继续执行后续流程",
            "继续设计",
        ]
        .contains(&s.as_str())
    }) {
        return false;
    }
    let mut subjects = vec![
        "当前事项".to_owned(),
        "当前版本".to_owned(),
        "当前结果".to_owned(),
        "此版本结果".to_owned(),
    ];
    if let Some(title) = interaction["title"].as_str() {
        subjects.push(normalize(title.trim_start_matches("确认")));
    }
    if interaction.to_string().to_lowercase().contains("prd") {
        subjects.push("prd".into());
    }
    ["确认", "批准", "同意", "通过", "approve", "approved"].contains(&first.as_str())
        || ["确认", "批准", "同意", "通过"].iter().any(|verb| {
            subjects
                .iter()
                .any(|subject| !subject.is_empty() && *first == format!("{verb}{subject}"))
        })
}

pub(crate) fn validate(
    state: &EngineeringState,
    instance: &ConfigurationRecord,
    value: &Value,
    arguments: &Value,
) -> Result<Value> {
    require(
        value.is_object() && value.to_string().len() <= 32768,
        "需要有界的人类交互说明",
    )?;
    for key in [
        "title",
        "background",
        "requested_action",
        "completed_work",
        "review_summary",
    ] {
        require(
            !text(value, key)?.trim().is_empty(),
            format!("人类交互缺少 {key}"),
        )?;
    }
    let materials = value["materials"]
        .as_array()
        .ok_or_else(|| err("invalid_params", "需要相关材料清单"))?;
    let evidence = value["evidence"]
        .as_array()
        .ok_or_else(|| err("invalid_params", "需要执行依据清单"))?;
    require(
        materials.len() <= 32 && evidence.len() <= 32,
        "交互引用过多",
    )?;
    let root = &instance.data["root_instance_id"];
    let mut normalized = value.clone();
    for reference in materials.iter().chain(evidence) {
        text(reference, "label")?;
        text(reference, "purpose")?;
        let owner = state.configuration(&text(reference, "instance_id")?)?;
        require(
            owner.r#type == "workflow_instance" && &owner.data["root_instance_id"] == root,
            "交互依据不属于当前根实例",
        )?;
        let execution = owner.data["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["id"] == reference["execution_id"])
            .ok_or_else(|| err("invalid_params", "交互依据缺少确切执行"))?;
        if let Some(id) = reference["output_id"].as_str() {
            let output = state.configuration(id)?;
            require(
                output.r#type == "workflow_output"
                    && output.data["execution_id"] == execution["id"]
                    && output.data["input_digest"] == execution["input_digest"],
                "材料与执行版本不一致",
            )?;
        } else {
            require(
                evidence.iter().any(|r| r == reference),
                "材料引用需要 output_id",
            )?;
        }
    }
    if let Some(outputs) = arguments["outputs"].as_object() {
        for id in outputs.values() {
            require(
                materials.iter().any(|r| &r["output_id"] == id),
                "需要列出本次审批的全部产物",
            )?;
        }
    }
    if let Some(id) = arguments["execution_id"].as_str() {
        require(
            evidence.iter().any(|r| r["execution_id"] == id),
            "需要提供本次执行的上下文与记录依据",
        )?;
        if let Ok(manifest) = state.configuration(&format!("workflow-inputs:{id}")) {
            for source in manifest.data["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["origin_ref"].as_str())
            {
                if state
                    .configuration(source)
                    .is_ok_and(|r| r.r#type == "workflow_output")
                {
                    require(
                        materials.iter().any(|r| r["output_id"] == source),
                        "需要列出本次审批的上游输入材料",
                    )?;
                }
            }
        }
    }
    for key in ["materials", "evidence"] {
        for reference in normalized[key].as_array_mut().unwrap() {
            let owner = state.configuration(reference["instance_id"].as_str().unwrap())?;
            let execution = owner.data["steps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == reference["execution_id"])
                .unwrap();
            reference["owner_id"] = owner.data["owner_id"].clone();
            reference["session_id"] = execution["session_id"].clone();
            reference["turn_id"] = execution["turn_id"].clone();
        }
    }
    Ok(normalized)
}

/// One exact approval projection for the manager, chat and workflow readers.
pub(crate) fn step_approval(
    state: &EngineeringState,
    row: &ConfigurationRecord,
    instance: &ConfiguredWorkflowInstance,
    step: &StepDefinition,
    execution: &StepExecution,
    actions: &WorkflowStepActions,
) -> Result<Option<Value>> {
    if !["human_confirmation", "completion"].contains(&actions.accept.code)
        || state.governance.iter().any(|r| {
            r.r#type == "workflow_confirmation"
                && r.definition_id == execution.id
                && r.data["input_digest"] == execution.input_digest
                && r.data["outputs"] == json!(execution.outputs)
        })
    {
        return Ok(None);
    }
    let mut materials = vec![];
    let mut evidence = vec![];
    let mut references: std::collections::BTreeSet<String> =
        execution.outputs.values().cloned().collect();
    if let Ok(manifest) = state.configuration(&format!("workflow-inputs:{}", execution.id)) {
        references.extend(
            manifest.data["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["origin_ref"].as_str())
                .map(str::to_owned),
        );
    }
    for id in references {
        let Some(output) = state
            .configuration(&id)
            .ok()
            .filter(|r| r.r#type == "workflow_output")
        else {
            continue;
        };
        if let Some(producer) = state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance")
            .find_map(|owner| {
                owner.data["steps"]
                    .as_array()?
                    .iter()
                    .find(|e| e["id"] == output.data["execution_id"])
                    .map(|e| json!({"instance_id":owner.id,"owner_id":owner.data["owner_id"],"execution_id":e["id"],"session_id":e["session_id"],"turn_id":e["turn_id"],"output_id":output.id,"label":output.name,"purpose":"核对当前审批的输入或产物"}))
            })
        {
            materials.push(producer);
        }
    }
    evidence.push(json!({"instance_id":row.id,"owner_id":instance.owner_id,"execution_id":execution.id,"session_id":execution.session_id,"turn_id":execution.turn_id,"label":step.name,"purpose":"核对本轮执行与校验结果"}));
    let ancestor_instance_ids = instance_ancestors(state, &row.id);
    let owner_name = state
        .tasks
        .iter()
        .find(|task| task.id == instance.owner_id)
        .map(|task| task.title.as_str())
        .or_else(|| {
            state
                .features
                .iter()
                .find(|feature| feature.id == instance.owner_id)
                .map(|feature| feature.title.as_str())
        })
        .or_else(|| {
            state
                .projects
                .iter()
                .find(|project| project.id == instance.owner_id)
                .map(|project| project.title.as_str())
        })
        .unwrap_or(&row.name);
    Ok(Some(
        json!({"instance_id":row.id,"instance_name":row.name,"ancestor_instance_ids":ancestor_instance_ids,"step_name":step.name,"human_required":actions.accept.code == "human_confirmation","owner_id":instance.owner_id,"revision":row.revision,
                        "operation":if actions.accept.code == "completion" {"workflow.step.approve"} else {"workflow.step.confirm"},
                        "arguments":{"plan_revision_id":instance.active_plan_revision_id,"control_version":instance.control_version,"step":execution.step,"execution_id":execution.id,"input_digest":execution.input_digest,"outputs":execution.outputs,"accept_result":true},
                        "interaction":{"source_label":format!("{} / {} / {}", owner_name, row.name, step.name),"title":format!("确认{}", step.name),"background":format!("{}正在等待人工决定。",step.name),"requested_action":if actions.accept.code == "completion" {"完成条件尚未满足。可填写理由后手动批准当前版本，或退回经理处理。"} else {"核对当前材料后确认，流程将接纳本轮结果并继续。"},"completed_work":"已执行当前步骤，材料与执行依据见下方。","review_summary":if actions.accept.code == "completion" {actions.accept.reason.as_str()} else {"步骤完成条件已满足，等待人工确认。"},"materials":materials,"evidence":evidence}}),
    ))
}

pub(crate) fn step_approvals(state: &EngineeringState, root: &str) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for row in state.governance.iter().filter(|r| {
        r.r#type == "workflow_instance" && (r.id == root || r.data["root_instance_id"] == root)
    }) {
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        if !instance.steps.iter().any(|e| {
            matches!(
                e.status,
                StepExecutionStatus::WaitingConfirmation | StepExecutionStatus::AwaitingAcceptance
            )
        }) {
            continue;
        }
        let facts = crate::configured_runtime::facts(state, &instance)?;
        for execution in &instance.steps {
            if !matches!(
                execution.status,
                StepExecutionStatus::WaitingConfirmation | StepExecutionStatus::AwaitingAcceptance
            ) {
                continue;
            }
            let step = instance
                .plan
                .document
                .steps
                .iter()
                .find(|step| step.node_id == execution.node_id)
                .ok_or_else(|| err("storage", "执行合同不存在"))?;
            let actions = crate::workflow_eligibility::step_actions_with_facts(
                state, &row.id, &instance, step, execution, &facts,
            )?;
            if let Some(approval) = step_approval(state, row, &instance, step, execution, &actions)?
            {
                result.push(approval);
            }
        }
    }
    Ok(result)
}
