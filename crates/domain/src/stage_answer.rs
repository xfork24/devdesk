//! The same formal output contract is published to agents and enforced on their answers.
use crate::*;
use serde_json::json;

pub const STAGE_ANSWER_CONTRACT: &str = "evidence-checks/v2";
pub const STAGE_CHECK_KEYS: [&str; 4] = ["traceability", "scope", "tests", "failure_paths"];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageCheck {
    pub key: String,
    pub passed: bool,
    pub evidence: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approve: Option<bool>,
    pub summary: String,
    pub checks: Vec<StageCheck>,
}
pub fn stage_answer_schema() -> Value {
    serde_json::from_str(include_str!("../../../schemas/stage-answer.schema.json"))
        .expect("embedded stage answer schema")
}
pub fn stage_answer_example() -> Value {
    json!({"approve":false,"summary":"示例：应替换为当前阶段的实际内容判断。","checks":STAGE_CHECK_KEYS.map(|key|json!({"key":key,"passed":false,"evidence":"示例占位：请提供实际材料位置、证据或具体缺口。"}))})
}
pub fn stage_answer_output_contract() -> Value {
    let schema = stage_answer_schema();
    json!({"id":STAGE_ANSWER_CONTRACT,"format":"one JSON object, no wrapper",
    "required_fields":["approve","summary","checks"],"required_checks":STAGE_CHECK_KEYS,
    "schema":schema,"example":stage_answer_example(),
    "instructions":[
        "Return exactly one JSON object matching schema: approve (boolean), summary and checks only; no Markdown or proposal/schema wrapper.",
        "Each check has key, passed (boolean), evidence (nonempty string). Include each required key exactly once. Do not use name, status, or evidence arrays.",
        "approve must equal whether all checks passed. The example is illustrative, not evidence."
    ]})
}
fn invalid(path: &str, reason: impl std::fmt::Display) -> Error {
    err("adapter_contract", format!("正式交付输出 {path}：{reason}"))
}
fn fields(value: &Value, expected: &[&str], path: &str) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "必须是 JSON 对象"))?;
    for key in object.keys() {
        if !expected.contains(&key.as_str()) {
            return Err(invalid(
                &format!("{path}.{key}"),
                "不允许此字段，请按正式交付 Schema 输出，勿包装 proposal、schema 或 status",
            ));
        }
    }
    Ok(())
}
pub fn parse_stage_answer(text: &str) -> Result<StageAnswer> {
    let value: Value = serde_json::from_str(text).map_err(|e| invalid("$", e))?;
    fields(&value, &["approve", "summary", "checks"], "$")?;
    if !value["summary"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(invalid("$.summary", "必须是非空字符串"));
    }
    let checks = value["checks"]
        .as_array()
        .ok_or_else(|| invalid("$.checks", "必须是数组"))?;
    if checks.len() != STAGE_CHECK_KEYS.len() {
        return Err(invalid(
            "$.checks",
            "必须恰好包含 traceability、scope、tests、failure_paths 四项",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for (index, check) in checks.iter().enumerate() {
        let path = format!("$.checks[{index}]");
        let key = check["key"]
            .as_str()
            .filter(|key| STAGE_CHECK_KEYS.contains(key))
            .ok_or_else(|| {
                invalid(
                    &format!("{path}.key"),
                    "必须为四种检查项之一，使用 key 而非 name",
                )
            })?;
        if !seen.insert(key) {
            return Err(invalid(&format!("{path}.key"), format!("重复检查项 {key}")));
        }
        if !check["passed"].is_boolean() {
            return Err(invalid(
                &format!("{path}.passed"),
                "必须为 true 或 false，不能使用 status: pass 或字符串",
            ));
        }
        if !check["evidence"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(invalid(
                &format!("{path}.evidence"),
                "必须为非空字符串，多条证据请用换行分隔，不能使用数组",
            ));
        }
        fields(check, &["key", "passed", "evidence"], &path)?;
    }
    if let Some(approve) = value.get("approve") {
        if approve.as_bool() != Some(checks.iter().all(|check| check["passed"] == true)) {
            return Err(invalid(
                "$.approve",
                "必须为布尔值，并与四项检查是否全部通过一致",
            ));
        }
    }
    // Deserialize the original text as well, rejecting duplicate JSON fields.
    serde_json::from_str(text).map_err(|e| invalid("$", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_check_field_types_keys_and_consistent_model_decisions() {
        let good = stage_answer_example();
        assert!(
            !parse_stage_answer(&good.to_string())
                .unwrap()
                .approve
                .unwrap()
        );
        for (field, value, expected) in [
            ("passed", json!("pass"), ".passed"),
            ("evidence", json!(["a finding"]), ".evidence"),
            ("key", json!("scope"), ".key"),
        ] {
            let mut bad = good.clone();
            bad["checks"][0][field] = value;
            assert!(
                parse_stage_answer(&bad.to_string())
                    .unwrap_err()
                    .message
                    .contains(expected)
            );
        }
        let mut bad = good.clone();
        bad["approve"] = json!(true);
        assert!(
            parse_stage_answer(&bad.to_string())
                .unwrap_err()
                .message
                .contains("$.approve")
        );
        assert!(
            parse_stage_answer(&json!({"proposal":good}).to_string())
                .unwrap_err()
                .message
                .contains("$.proposal")
        );
    }
}
