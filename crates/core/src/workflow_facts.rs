//! Explicit rule inputs, selected native executions and immutable observations.
//! A WorkflowFacts is the rule's declared interface, never a partial store state.
use crate::{
    concurrency::*,
    workflow_resources::{WorkflowResources, version},
    *,
};
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct FactSelection {
    values: BTreeSet<(String, String)>,
    executions: BTreeSet<String>,
    approvals: BTreeSet<String>,
    materials: BTreeSet<Id>,
}
impl FactSelection {
    pub fn source(&mut self, source: &PortSource) {
        if source.step == "$material" {
            self.materials.insert(source.port.clone());
        } else {
            self.values
                .insert((source.step.clone(), source.port.clone()));
        }
    }
    pub fn binding(&mut self, binding: &InputBinding) {
        match binding {
            InputBinding::Source { source } => self.source(source),
            InputBinding::Material { material_ref } => {
                self.materials.insert(material_ref.clone());
            }
            InputBinding::Constant { .. } => {}
        }
    }
    pub fn rule(&mut self, rule: &CompletionRule) {
        match rule {
            CompletionRule::Always => {}
            CompletionRule::All { rules } | CompletionRule::Any { rules } => {
                for rule in rules {
                    self.rule(rule);
                }
            }
            CompletionRule::Exists { source } | CompletionRule::Equals { source, .. } => {
                self.source(source)
            }
            CompletionRule::Same { left, right } => {
                self.source(left);
                self.source(right);
            }
            CompletionRule::Covers {
                available,
                required,
            } => {
                self.source(available);
                self.source(required);
            }
            CompletionRule::Before { first, second } => {
                self.executions.insert(first.clone());
                self.executions.insert(second.clone());
            }
            CompletionRule::Independent { author, reviewer } => {
                self.executions.insert(author.clone());
                self.executions.insert(reviewer.clone());
            }
            CompletionRule::Outcome { step, .. } => {
                self.executions.insert(step.clone());
            }
            CompletionRule::Approved { step } => {
                self.approvals.insert(step.clone());
            }
        }
    }
    pub fn approval(&mut self, step: &str) {
        self.approvals.insert(step.into());
    }
}
pub(crate) struct SelectedWorkflowFacts {
    pub facts: WorkflowFacts,
    pub reads: ReadSet,
}
impl WorkflowResources {
    pub fn selected_facts(
        &self,
        store: &dyn EngineeringStore,
        selected: &FactSelection,
    ) -> Result<SelectedWorkflowFacts> {
        let catalog = store.configuration_snapshot()?;
        let mut facts = WorkflowFacts::default();
        let mut reads = self.reads.clone();
        let names = selected
            .values
            .iter()
            .filter(|(s, _)| s != "$input")
            .map(|(s, _)| s.clone())
            .chain(selected.executions.iter().cloned())
            .chain(selected.approvals.iter().cloned())
            .collect::<BTreeSet<_>>();
        require(
            names.len() <= WORKFLOW_STEP_LIMIT && selected.materials.len() <= WORKFLOW_STEP_LIMIT,
            "规则事实选择超过有界预算",
        )?;
        for name in names {
            let execution = self
                .instance
                .steps
                .iter()
                .find(|s| s.step == name)
                .ok_or_else(|| err("storage", "规则引用的实际步骤不存在"))?;
            let key = ResourceKey::new(
                "workflow_step",
                json!([self.record.id, execution.id]).to_string(),
            );
            let actual = store.read_resources(std::slice::from_ref(&key))?.remove(0);
            // Other scheduling stages may advance or replace this execution
            // after the instance snapshot. No action has been admitted here:
            // discard these facts and let the scheduler reread on retry.
            let conflict = |changed_fields: Value| {
                err(
                    "revision_conflict",
                    "规则事实在准备期间已变化，请保留原请求",
                )
                .with_context(
                    "WorkflowResources::selected_facts",
                    json!({
                        "project_id":self.instance.project_id,
                        "workflow_instance_id":self.record.id,
                        "execution_id":self.execution.id,
                        "referenced_execution_id":execution.id,
                        "step":name,
                        "snapshot_revision":self.record.revision,
                        "actual_resource_revision":actual.revision,
                        "actual_execution_exists":actual.value.is_some(),
                        "changed_fields":changed_fields,
                    }),
                )
            };
            let raw = actual.value.as_ref().ok_or_else(|| conflict(Value::Null))?;
            let mut changed = serde_json::Map::new();
            for (field, value) in [
                ("id", json!(execution.id)),
                ("status", json!(execution.status)),
                ("input_digest", json!(execution.input_digest)),
                ("agent_revision_id", json!(execution.agent_revision_id)),
                ("started", json!(execution.started)),
                ("finished", json!(execution.finished)),
                ("outputs", json!(execution.outputs)),
            ] {
                if raw[field] != value {
                    changed.insert(field.into(), json!({"expected":value,"actual":raw[field]}));
                }
            }
            if !changed.is_empty() {
                return Err(conflict(Value::Object(changed)));
            }
            reads = reads.merge([version(&actual)])?;
            if selected.executions.contains(&name)
                && matches!(
                    execution.status,
                    StepExecutionStatus::Completed
                        | StepExecutionStatus::WaitingConfirmation
                        | StepExecutionStatus::AwaitingAcceptance
                )
            {
                facts.executions.insert(
                    name.clone(),
                    ExecutionFact {
                        operation_id: execution.id.clone(),
                        input_digest: execution.input_digest.clone(),
                        identity_root: execution
                            .agent_revision_id
                            .as_ref()
                            .map(|id| catalog.state.agent_identity(id))
                            .transpose()?,
                        started: execution.started.unwrap_or(0),
                        finished: execution.finished.unwrap_or(0),
                        outcome: "succeeded".into(),
                    },
                );
            }
            if selected.approvals.contains(&name) {
                let row = store
                    .read_resources(&[ResourceKey::new("workflow_confirmations", &execution.id)])?
                    .remove(0);
                reads = reads.merge([version(&row)])?;
                if row
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .is_some_and(|rows| {
                        rows.iter().any(|r| {
                            r["data"]["input_digest"] == execution.input_digest
                                && r["data"]["outputs"] == json!(execution.outputs)
                        })
                    })
                {
                    facts.approvals.insert(name.clone());
                }
            }
            for (_, port) in selected.values.iter().filter(|(s, _)| s == &name) {
                if let Some(id) = execution.outputs.get(port) {
                    let row = store
                        .read_resources(&[ResourceKey::new("governance", id)])?
                        .remove(0);
                    let output = ConfigurationRecord::from_storage(
                        row.value
                            .clone()
                            .ok_or_else(|| err("not_found", "原步骤产物缺失"))?,
                    )?;
                    require(
                        output.r#type == "workflow_output"
                            && output.immutable
                            && output.scope == self.record.scope
                            && output.data["execution_id"] == execution.id,
                        "规则产物与原执行来源不符",
                    )?;
                    reads = reads.merge([version(&row)])?;
                    facts
                        .values
                        .insert((name.clone(), port.clone()), output.data["value"].clone());
                }
            }
        }
        for (_, port) in selected.values.iter().filter(|(s, _)| s == "$input") {
            if let Some(value) = self.instance.inputs.get(port) {
                facts
                    .values
                    .insert(("$input".into(), port.clone()), value.clone());
            }
        }
        for id in &selected.materials {
            let row = store
                .read_resources(&[ResourceKey::new("governance", id)])?
                .remove(0);
            reads = reads.merge([version(&row)])?;
            if let Some(raw) = row.value {
                let material = ConfigurationRecord::from_storage(raw)?;
                require(
                    material.r#type == "typed_material"
                        && material.immutable
                        && material.data["project_id"] == self.instance.project_id
                        && material.data["root_instance_id"] == self.instance.root_instance_id,
                    "规则材料不属于本次流程根",
                )?;
                facts.values.insert(
                    ("$material".into(), id.clone()),
                    material.data["value"].clone(),
                );
            }
        }
        Ok(SelectedWorkflowFacts { facts, reads })
    }
}
