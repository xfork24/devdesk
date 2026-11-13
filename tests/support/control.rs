//! Explicit test coordinator for execution-contract tests. Production never calls this helper.
#![allow(dead_code)]
use devdesk_core::{Command, Engine};
use devdesk_domain::*;
use serde_json::{Value, json};

pub fn evaluate(e: &Engine) -> Result<()> {
    e.evaluate_workflows()?;
    // Act through public commands, as a coordinator would. Human-only gates remain pending.
    for _ in 0..32 {
        let state = e.store.snapshot()?;
        let rows: Vec<_> = state
            .governance
            .iter()
            .filter(|r| {
                r.r#type == "workflow_instance"
                    && !state.workflow_instance_suspended(&r.id)
                    && !["completed", "cancelled"]
                        .contains(&r.data["status"].as_str().unwrap_or(""))
            })
            .cloned()
            .collect();
        let mut progress = false;
        for row in rows {
            let view = e.query("workflow.inspect", &json!({"id":row.id}), 4)?;
            for step in view["steps"].as_array().unwrap() {
                let exec = &step["execution"];
                let method = if step["dispatchable"] == true {
                    "workflow.step.dispatch"
                } else if ["awaiting_acceptance", "waiting_confirmation"]
                    .contains(&exec["status"].as_str().unwrap_or(""))
                    && (step["completion_satisfied"] == true
                        || step["completion_overridden"] == true)
                    && step["human_satisfied"] == true
                {
                    "workflow.step.accept"
                } else {
                    continue;
                };
                let fresh = e.store.snapshot()?;
                let record = fresh.configuration(&row.id)?;
                let payload = json!({"plan_revision_id":record.data["active_plan_revision_id"],"step":step["step"],"execution_id":exec["id"],"input_digest":exec["input_digest"],"outputs":exec["outputs"],"reason":"explicit test coordinator"});
                if e.execute(
                    method,
                    Command {
                        command_id: id(),
                        target: Some(row.id.clone()),
                        expected_revision: Some(record.revision),
                        payload,
                    },
                )
                .is_ok()
                {
                    progress = true;
                }
            }
            let fresh = e.store.snapshot()?;
            let record = fresh.configuration(&row.id)?;
            if record.data["steps"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| ["completed", "skipped"].contains(&e["status"].as_str().unwrap_or("")))
            {
                if e.execute("workflow.complete",Command{command_id:id(),target:Some(row.id.clone()),expected_revision:Some(record.revision),payload:json!({"plan_revision_id":record.data["active_plan_revision_id"],"reason":"explicit test coordinator completion"})}).is_ok(){progress=true;}
            }
        }
        if !progress {
            break;
        }
        e.evaluate_workflows()?;
    }
    Ok(())
}

pub fn tick(e: &Engine, limit: usize) -> Result<()> {
    evaluate(e)?;
    e.tick(limit)?;
    evaluate(e)
}

pub fn command(e: &Engine, method: &str, target: &str, mut payload: Value) -> Result<Value> {
    let state = e.store.snapshot()?;
    let row = state.configuration(target)?;
    payload["plan_revision_id"] = row.data["active_plan_revision_id"].clone();
    e.execute(
        method,
        Command {
            command_id: id(),
            target: Some(target.into()),
            expected_revision: Some(row.revision),
            payload,
        },
    )
}
