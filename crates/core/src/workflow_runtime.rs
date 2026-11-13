use crate::governance::update_record;
use crate::*;
impl Engine {
    pub fn evaluate_workflows(&self) -> Result<()> {
        self.evaluate_configured_workflows()
    }
    pub fn pending_workflow_intents(&self) -> Result<Vec<ConfigurationRecord>> {
        let s = self.store.snapshot()?;
        let active: Vec<String> = s
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance" && r.data["status"] != "cancelled")
            .filter(|r| {
                !s.workflow_instance_suspended(&r.id)
                    && r.data["owner_id"]
                        .as_str()
                        .is_some_and(|owner| !s.workflow_paused(owner))
            })
            .flat_map(|r| {
                r.data["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|step| step["status"] == "running")
                    .filter_map(|step| step["id"].as_str().map(str::to_owned))
            })
            .collect();
        let mut active = active;
        active.extend(
            s.governance
                .iter()
                .filter(|r| r.r#type == "workflow_controller" && r.data["status"] == "active")
                .map(|r| r.id.clone()),
        );
        Ok(s.governance
            .iter()
            .filter(|r| {
                r.r#type == "workflow_intent"
                    && r.data["status"] == "prepared"
                    && active.contains(&r.definition_id)
            })
            .take(16)
            .cloned()
            .collect())
    }
    pub fn observe_workflow_intent(&self, intent: &str, result: &Result<Value>) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let old = s.configuration(intent)?.clone();
            require(old.r#type == "workflow_intent", "不是工作流动作")?;
            let mut data = old.data.clone();
            match result {
                Ok(value) => {
                    data["status"] = json!("completed");
                    data["result"] = value.clone();
                }
                Err(e) => {
                    data["status"] = json!("failed");
                    data["error"] = json!(format!("{}: {}", e.code, e.message));
                }
            }
            update_record(
                s.governance.iter_mut().find(|r| r.id == intent).unwrap(),
                data,
            );
            Ok(json!({"recorded":true}))
        })?;
        Ok(())
    }
}
