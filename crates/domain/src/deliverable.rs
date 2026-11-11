//! Reusable deliverable types and exact producer/consumer declarations.
use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryTypeDefinition {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub spec_refs: Vec<Id>,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_schema: Option<serde_json::Value>,
}
impl DeliveryTypeDefinition {
    pub fn validate(&self) -> Result<()> {
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
