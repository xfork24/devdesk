//! Test authoring helpers publish real Profiles and Workflow revisions through core commands.
#![allow(dead_code)]
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
fn execute(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
    let s = e.store.snapshot()?;
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: target
                .and_then(|id| s.configuration(id).ok())
                .map(|r| r.revision),
            payload,
        },
    )
}
pub fn create_profile(e: &Engine, mut payload: Value) -> Result<Value> {
    let mut policy = payload["data"]["policy"].clone();
    if policy.is_null() {
        policy = payload["policy"].clone();
    }
    // Only authority fields are part of a Profile; budgets are authored in Workflow.
    if let Some(platform) = policy.get_mut("platform").and_then(Value::as_object_mut) {
        for field in [
            "max_turns_per_wakeup",
            "max_tool_calls_per_turn",
            "timeout_seconds",
        ] {
            platform.remove(field);
        }
    }
    payload = json!({"identity":id(),"name":payload["name"].as_str().unwrap_or("Fixture Profile"),"target_kind":"harness","policy":policy});
    let mut result = execute(e, "permission.profile.create", None, payload)?;
    result["id"] = json!(source_reference(
        result["identity"].as_str().unwrap(),
        result["revision"].as_u64().unwrap()
    )?);
    Ok(result)
}
pub fn scheduling(e: &Engine, target: &str, patch: &Value) -> Result<()> {
    let s = e.store.snapshot()?;
    let row = s.configuration(target)?;
    let instance: ConfiguredWorkflowInstance = row.decode()?;
    let mut document = s.configuration(&instance.workflow_ref)?.data.clone();
    let mut value = serde_json::to_value(instance.plan.document.scheduling).unwrap();
    for (key, item) in patch
        .as_object()
        .ok_or_else(|| err("fixture", "scheduling patch must be an object"))?
    {
        value[key] = item.clone();
    }
    document["scheduling"] = value;
    let changes = json!({"plan_revision_id":instance.active_plan_revision_id,"reason":"Fixture authors Workflow scheduling","operations":[{"type":"replace_plan","document":document}]});
    let preview = e.query(
        "workflow.change.preview",
        &json!({"id":target,"changeset":changes}),
        100,
    )?;
    let change = execute(
        e,
        "workflow.change.apply",
        Some(target),
        json!({"plan_revision_id":instance.active_plan_revision_id,"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )?;
    for _ in 0..32 {
        e.drive_workflow_changes()?;
        let state = e.store.snapshot()?;
        let row = state.configuration(change["id"].as_str().unwrap())?;
        if row.data["status"] == "applied" {
            return Ok(());
        }
        if row.data["status"] == "conflicted" {
            return Err(err("fixture", row.data["error"].to_string()));
        }
        e.tick_conversations(8)?;
        e.evaluate_workflows()?;
    }
    Err(err("fixture", "计划变更未在有界夹具轮次内完成"))
}
pub fn workflow_with_scheduling(e: &Engine, reference: &str, scheduling: &Value) -> Result<String> {
    let s = e.store.snapshot()?;
    let source = s.configuration(reference)?;
    let mut document = source.data.clone();
    document["scheduling"] = scheduling.clone();
    let draft = execute(
        e,
        "workflow.create",
        None,
        json!({"name":"Fixture scheduled workflow","scope":e.store.project_id().as_deref().map(Scope::project).unwrap_or_else(||source.scope.clone()),"document":document}),
    )?;
    Ok(
        execute(e, "workflow.publish", draft["id"].as_str(), json!({}))?["id"]
            .as_str()
            .unwrap()
            .into(),
    )
}
