//! Explicit display facts for one instance. No aggregate runtime restoration.
use crate::read_models::{WorkflowDetailQuery, WorkflowDetailView, WorkflowSessionActor};
use crate::*;
use std::collections::BTreeMap;

pub(crate) fn session_actor_brief(
    catalog: &EngineeringState,
    actor: &WorkflowSessionActor,
) -> Value {
    let agent = catalog.configuration(&actor.agent_ref).ok();
    json!({"kind":"agent","session_id":actor.session_id,"agent_revision_id":actor.agent_ref,
        "agent_instance_id":actor.agent_instance_id,"name":agent.map(|a|a.name.as_str()).unwrap_or(&actor.agent_ref),
        "role_ref":agent.and_then(|a|a.refs.get("role_ref"))})
}

impl Engine {
    pub(crate) fn workflow_instance_detail(&self, p: &Value) -> Result<Value> {
        if p["view"] == "qualification" {
            crate::read_models::fields(p, &["project_id", "id", "execution_id", "view"])?;
            let project = crate::read_models::identifier(p, "project_id")?;
            require(
                self.store.project_id().as_deref() == Some(&project),
                "工作流不属于当前项目",
            )?;
            let root = crate::workflow_commits::WorkflowHeaderFacts::read(
                self,
                &crate::read_models::identifier(p, "id")?,
            )?;
            let execution = crate::read_models::identifier(p, "execution_id")?;
            let resources = root.step(self, &execution)?;
            let ready = self.native_step_readiness(&root, &resources)?;
            let driver = crate::platform_facts::one(
                self.store.as_ref(),
                "workflow_driver_root",
                &root.instance.root_instance_id,
            )?;
            let human_can_accept = driver
                .value
                .as_ref()
                .is_none_or(|r| r["data"]["mode"] != "agent");
            let confirmation = if let Some(id) = ready["confirmation_ref"].as_str() {
                crate::platform_facts::one(self.store.as_ref(), "governance", id)?
                    .value
                    .map(|r| r["data"].clone())
            } else {
                None
            };
            return Ok(
                json!({"id":root.record.id,"execution_id":execution,"step_revision":resources.step.revision,"completion_status":{execution.clone():{"satisfied":ready["completion_satisfied"],"pending":false,"human_satisfied":ready["human_satisfied"],"human_can_accept":human_can_accept,"confirmation":confirmation}},"advisory":true}),
            );
        }
        let query = WorkflowDetailQuery::parse(p)?;
        if let Some(project) = p["project_id"].as_str() {
            require(
                self.store.project_id().as_deref() == Some(project),
                "工作流不属于当前项目",
            )?;
        }
        let Some(selected) = self.store.workflow_detail(&query)? else {
            return Ok(json!({"record":null}));
        };
        let row = &selected.record;
        let runtime = &row.data;
        if query.view == WorkflowDetailView::History {
            return Ok(
                json!({"cursor":selected.cursor,"record":{"id":row.id,"revision":row.revision},"history":selected.history,
                "next_history_before":selected.next_history_before,"history_more":selected.next_history_before.is_some()}),
            );
        }
        let catalog = self.store.configuration_snapshot()?;
        let plan: WorkflowDefinition = serde_json::from_value(selected.plan_document.clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let steps = runtime["steps"]
            .as_array()
            .ok_or_else(|| err("storage", "流程缺少执行步骤"))?;
        let mut facts = WorkflowFacts::default();
        for (name, value) in runtime["inputs"].as_object().into_iter().flatten() {
            facts
                .values
                .insert(("$input".into(), name.clone()), value.clone());
        }
        let mut confirmations = BTreeMap::new();
        for execution in steps {
            let key = text(execution, "step")?;
            let id = text(execution, "id")?;
            for (port, output_id) in execution["outputs"].as_object().into_iter().flatten() {
                let Some(output) = selected
                    .outputs
                    .iter()
                    .find(|o| Some(o.id.as_str()) == output_id.as_str())
                else {
                    if query.view == WorkflowDetailView::Status {
                        continue;
                    }
                    return Err(err("not_found", "当前执行产物缺失"));
                };
                require(
                    output.r#type == "workflow_output"
                        && output.scope == row.scope
                        && output.data["execution_id"] == id,
                    "产物来源不属于当前执行",
                )?;
                facts
                    .values
                    .insert((key.clone(), port.clone()), output.data["value"].clone());
            }
            if matches!(
                execution["status"].as_str(),
                Some("completed" | "waiting_confirmation" | "awaiting_acceptance")
            ) {
                facts.executions.insert(
                    key.clone(),
                    ExecutionFact {
                        operation_id: id.clone(),
                        input_digest: execution["input_digest"].as_str().unwrap_or("").into(),
                        identity_root: execution["agent_revision_id"]
                            .as_str()
                            .map(|id| catalog.state.agent_identity(id))
                            .transpose()?,
                        started: execution["started"].as_u64().unwrap_or(0),
                        finished: execution["finished"].as_u64().unwrap_or(0),
                        outcome: "succeeded".into(),
                    },
                );
            }
            if let Some(confirmation) = selected.confirmations.iter().rev().find(|r| {
                r.definition_id == id
                    && r.data["input_digest"] == execution["input_digest"]
                    && r.data["outputs"] == execution["outputs"]
            }) {
                facts.approvals.insert(key);
                confirmations.insert(id, confirmation);
            }
        }
        for material in &selected.materials {
            facts.values.insert(
                ("$material".into(), material.id.clone()),
                material.data["value"].clone(),
            );
        }
        let mut completion_status = BTreeMap::new();
        let mut actors = BTreeMap::new();
        for execution in steps {
            let id = text(execution, "id")?;
            let step = plan
                .steps
                .iter()
                .find(|s| Some(s.key.as_str()) == execution["step"].as_str())
                .ok_or_else(|| err("storage", "当前执行步骤不在冻结计划中"))?;
            let confirmation = confirmations.get(&id);
            let waived = selected.waivers.iter().any(|w| {
                w.data["plan_revision_id"] == runtime["active_plan_revision_id"]
                    && w.data["node_id"] == step.node_id
                    && w.data["requirement"] == "confirmation"
            });
            let satisfied = if query.view == WorkflowDetailView::Status {
                selected.rule_hints.get(&id).copied().flatten()
            } else {
                Some(step.completion.evaluate(&facts))
            };
            completion_status.insert(id.clone(),json!({"satisfied":satisfied,"pending":satisfied.is_none(),"human_satisfied":step.confirmation!=ConfirmationPolicy::Human || confirmation.is_some() || waived,"confirmation":confirmation.map(|r|&r.data)}));
            let actor = execution["session_id"]
                .as_str()
                .and_then(|s| selected.actors.get(s))
                .map(|s| session_actor_brief(&catalog.state, s))
                .unwrap_or_else(|| {
                    let definition = json!(step);
                    crate::workflow_reading::unstarted_execution_actor(
                        &catalog.state,
                        execution,
                        &definition,
                        &runtime["plan"]["definitions"],
                    )
                });
            actors.insert(id, actor);
        }
        let outputs = if query.view == WorkflowDetailView::Detail {
            selected
                .outputs
                .iter()
                .map(|o| (o.id.clone(), o.data.clone()))
                .collect::<BTreeMap<_, _>>()
        } else {
            BTreeMap::new()
        };
        let mut display = row.clone();
        if query.view == WorkflowDetailView::Status {
            display.data.as_object_mut().unwrap().remove("inputs");
            display.data["plan"] = json!({"document":status_plan(&selected.plan_document)});
        }
        let visible_record = if query.view == WorkflowDetailView::Status {
            json!({"id":display.id,"name":display.name,"revision":display.revision,"data":display.data})
        } else {
            json!(display)
        };
        Ok(
            json!({"cursor":selected.cursor,"record":visible_record,"runtime":display.data,"plan":if query.view==WorkflowDetailView::Status {status_plan(&selected.plan_document)} else {selected.plan_document},
            "outputs":outputs,"children":selected.children,"children_more":selected.children_more,"next_child_offset":selected.next_child_offset,
            "step_total":selected.step_total,"next_step_offset":selected.next_step_offset,"step_offset":query.step_offset,"step_revisions":selected.step_revisions,"completion_status":completion_status,"actors":actors,"history":selected.history,
            "history_more":selected.next_history_before.is_some(),"next_history_before":selected.next_history_before,
            "representation":if query.view==WorkflowDetailView::Status {"workflow_status"} else {"workflow_detail"}}),
        )
    }
}

fn status_plan(document: &Value) -> Value {
    let steps=document["steps"].as_array().into_iter().flatten().map(|step| {
        let inputs=step["inputs"].as_object().into_iter().flatten().map(|(name,binding)|(name.clone(),json!({"source":binding["source"]}))).collect::<BTreeMap<_,_>>();
        json!({"node_id":step["node_id"],"key":step["key"],"name":step["name"],"stage":step["stage"],
            "executor":{"type":step["executor"]["type"],"role_ref":step["executor"]["role_ref"],"tool_ref":step["executor"]["tool_ref"]},"inputs":inputs,"confirmation":step["confirmation"]})
    }).collect::<Vec<_>>();
    let stages=document["stages"].as_array().into_iter().flatten().map(|stage|json!({"key":stage["key"],"name":stage["name"],"description":stage["description"].as_str().map(|s|s.chars().take(180).collect::<String>())})).collect::<Vec<_>>();
    json!({"stages":stages,"steps":steps})
}
