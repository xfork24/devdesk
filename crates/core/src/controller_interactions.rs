//! Human statements and immutable interaction evidence.
use crate::*;
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

#[cfg(test)]
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
