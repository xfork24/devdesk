use devdesk_core::ports::Harness;
use devdesk_domain::{Result, Run, err};
use serde_json::{Value, json};

/// Simulated implementation of the `Harness` execution contract.
pub struct SimHarness;
impl Harness for SimHarness {
    // This identity is persisted in existing effect/advance records; keep it stable.
    fn identity(&self) -> String {
        "devdesk_adapters::SimHarness".into()
    }
    fn reconcile(&self, run: &Run) -> Result<Option<Value>> {
        Ok(Some(
            json!({"status":"cancelled","cancellation_confirmed":true,"origin":"simulated","run_id":run.id,"session_id":run.session_id}),
        ))
    }
    fn configure(&self, node: &str, scenario: &str) -> String {
        match scenario {
            "technical_failure" if node == "implement" || node == "merge_verify" => {
                "technical_failure"
            }
            "assertion_failure" if node == "green" || node == "merge_verify" => "assertion_failure",
            "missing_fields" if node == "research" => "missing_fields",
            "hold" => "hold",
            "cleanup_failure" => "cleanup_failure",
            "review_reject" if node.starts_with("review:") => "review_reject",
            _ => "success",
        }
        .into()
    }

    fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        if !run.cancel_requested
            && (run.outcome == "hold" || now < run.started.unwrap_or(now) + 180)
        {
            return Ok(None);
        }
        if run.cancel_requested {
            return Ok(Some(
                json!({"status":"cancelled","origin":"simulated","cancellation_confirmed":true,"run_id":run.id,"session_id":run.session_id}),
            ));
        }
        if run.node.starts_with("review:")
            && (input["delivery"]["id"] != run.node.trim_start_matches("review:")
                || !input["documents"].is_array()
                || !input["evidence"].is_array()
                || !input["artifact_body"].is_string())
        {
            return Err(err(
                "adapter_contract",
                "Reviewer 未收到冻结 Delivery/文档/证据/报告",
            ));
        }
        let mut result = match run.outcome.as_str() {
            "technical_failure" => {
                json!({"status":"failed","failure_type":"environment","message":"固定注入：模拟执行环境不可用","origin":"simulated"})
            }
            "missing_fields" => json!({"message":"固定注入：供应商缺少结果字段"}),
            _ => {
                json!({"status":"succeeded","origin":"simulated","session_id":run.session_id,"message":format!("{} 模拟执行完成",run.node)})
            }
        };
        result["run_id"] = json!(run.id);
        result["session_id"] = json!(run.session_id);
        result["external_session"] = json!(format!("sim-session-{}", run.session_id));
        result["external_environment"] = json!(format!("sim-environment-{}", run.session_id));
        result["cleanup_failed"] = json!(run.outcome == "cleanup_failure");
        if run.node.starts_with("review:") {
            result["review_delivery"] = input["delivery"]["id"].clone();
            result["review_decision"] = json!(if run.outcome == "review_reject" {
                "changes_requested"
            } else {
                "pass"
            });
        }
        let kind = match run.node.as_str() {
            "requirement" => Some("requirement"),
            "design" => Some("design"),
            "test_plan" => Some("test"),
            "docs" => Some("doc"),
            _ => None,
        };
        if let Some(kind) = kind {
            let requirement = input["documents"]
                .as_array()
                .and_then(|ds| ds.iter().rev().find(|d| d["kind"] == "requirement"))
                .and_then(|d| d["body"].as_str())
                .unwrap_or("建立可验证的软件变更");
            let body=match kind {"requirement"=>"## 需求\n建立可验证的软件变更。\n\n## 验收条件\n- 主流程可完成\n- 错误路径可观察\n- 历史可追踪".to_string(),"design"=>format!("## 设计\n针对需求：{requirement}\n\n## 方案与任务计划\n1. 实现主要行为\n2. 完善错误处理与文档\n\n## 验证\n各 Task 执行 Red → 实现 → Green，正式交付需 Review。"),"test"=>"## Test 计划\n实现前断言失败 Red，候选实现后 Green；验证错误路径，合入前重验候选。".into(),_=>"## 交付说明\n记录实现、验证、错误处理与已知限制。外部执行为模拟。".into()};
            result["document"] =
                json!({"kind":kind,"title":format!("模拟建议 · {kind}"),"body":body});
        }
        Ok(Some(result))
    }
}
