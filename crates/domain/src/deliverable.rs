//! Reusable deliverable types and exact producer/consumer declarations.
use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryTypeDefinition {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default)]
    pub spec_refs: Vec<Id>,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<DeliveryProducer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeliveryProducer {
    Agent,
    Platform { tools: Vec<String> },
    Workflow,
}
impl DeliveryTypeDefinition {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.spec_refs.is_empty()
                && self.spec_refs.iter().all(|s| !s.trim().is_empty())
                && self
                    .spec_refs
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == self.spec_refs.len(),
            "Delivery 必须关联至少一个内容 Spec，引用不得为空或重复",
        )?;
        if let Some(DeliveryProducer::Platform { tools }) = &self.producer {
            require(
                !tools.is_empty() && tools.iter().all(|t| !t.trim().is_empty()),
                "平台产出必须声明来源工具",
            )?;
        }
        if let Some(schema) = &self.content_schema {
            require(
                schema.is_object() || schema.is_boolean(),
                "交付物结构需要 JSON Schema",
            )?;
        }
        require(
            (!self.body.trim().is_empty() || !self.spec_refs.is_empty())
                && self.body.len() <= TASK_TEXT_LIMIT,
            "Delivery 需要非空内容说明，最多 32 KiB",
        )?;
        require(
            ["markdown", "diff", "test_report", "json", "text"].contains(&self.format.as_str()),
            "Delivery 格式不受支持",
        )
    }
}

impl EngineeringState {
    pub fn validate_delivery_specs(
        &self,
        delivery: &DeliveryTypeDefinition,
        scope: &Scope,
    ) -> Result<()> {
        delivery.validate()?;
        for id in &delivery.spec_refs {
            let source = self.configuration(id)?;
            require(
                source.kind == "spec" && source.scope.visible_from(scope),
                "Delivery 内容 Spec 类型或作用域不匹配",
            )?;
            let spec: ConfiguredSpecification = source.decode()?;
            spec.validate()?;
            require(!spec.checks.is_empty(), "Delivery 内容 Spec 必须包含检查项")?;
        }
        Ok(())
    }
}
