//! Reusable deliverable types and exact producer/consumer declarations.
use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryTypeDefinition {
    pub body: String,
    pub format: String,
}
impl DeliveryTypeDefinition {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.body.trim().is_empty() && self.body.len() <= TASK_TEXT_LIMIT,
            "Delivery 需要非空内容说明，最多 32 KiB",
        )?;
        require(
            ["markdown", "diff", "test_report", "json", "text"].contains(&self.format.as_str()),
            "Delivery 格式不受支持",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDeliveryOutput {
    pub delivery_ref: Id,
    pub source_port: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDeliveryInput {
    pub delivery_ref: Id,
    pub source: WorkflowPortReference,
}
pub fn delivery_source_ports(step: &WorkflowStep) -> Vec<String> {
    let ports: &[&str] = match step.kind.as_str() {
        "agent.interact" => &["proposal"],
        "proposal.adopt" => &["document"],
        "attempt.execute" | "delivery.review" => &["delivery"],
        "merge.prepare" | "merge.review" => &["merge", "delivery"],
        "merge.apply" => &["merge"],
        "snapshot.capture" => &["snapshot"],
        _ => &[],
    };
    ports.iter().map(|p| (*p).into()).collect()
}
pub fn validate_workflow_deliveries(doc: &WorkflowDocument) -> Result<()> {
    fn valid_key(key: &str) -> bool {
        !key.is_empty()
            && key.len() <= 80
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
    }
    for step in &doc.steps {
        for (key, output) in &step.delivery_outputs {
            require(
                ![
                    "session", "proposal", "document", "attempt", "delivery", "merge", "snapshot",
                    "group",
                ]
                .contains(&key.as_str())
                    || key == &output.source_port,
                "Delivery 产出名称不能覆盖其他标准端口",
            )?;
            require(
                valid_key(key) && !output.delivery_ref.is_empty(),
                "交付物名称和 Delivery 引用不能为空",
            )?;
            require(
                if step.kind == "workflow.call" {
                    !output.source_port.is_empty()
                        && step
                            .parameters
                            .get("call_outputs")
                            .is_none_or(|ports| ports.get(&output.source_port).is_some())
                } else {
                    delivery_source_ports(step).contains(&output.source_port)
                },
                format!("{}: 交付物 {key} 的产出端口不存在", step.key),
            )?;
        }
        for (key, input) in &step.delivery_inputs {
            require(
                valid_key(key) && !input.delivery_ref.is_empty(),
                "引用交付物需要名称和 Delivery 类型",
            )?;
            require(
                step.inputs.get(key).is_none_or(|source| {
                    source.step == input.source.step && source.port == input.source.port
                }),
                format!("{}: 交付物 {key} 与普通输入重名", step.key),
            )?;
            if input.source.step == "$input" {
                require(
                    doc.input_ports.contains(&input.source.port),
                    "交付物引用了未声明的子流程输入",
                )?;
            } else {
                let source = doc
                    .steps
                    .iter()
                    .find(|source| source.key == input.source.step)
                    .and_then(|source| source.delivery_outputs.get(&input.source.port))
                    .ok_or_else(|| {
                        err(
                            "invalid_workflow",
                            format!("{}: 找不到交付物 {key} 的上游产出", step.key),
                        )
                    })?;
                require(
                    source.delivery_ref == input.delivery_ref,
                    format!("{}: 交付物 {key} 的 Delivery 类型修订不匹配", step.key),
                )?;
            }
        }
    }
    Ok(())
}
