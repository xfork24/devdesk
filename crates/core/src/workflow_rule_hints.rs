//! Eventually consistent display hints retain their exact native witnesses.
//! They never authorize a command; control admissions repeat the actual rules.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::version,
    *,
};
impl Engine {
    pub(crate) fn publish_workflow_rule_hints(&self, root: &WorkflowHeaderFacts) -> Result<()> {
        let key = format!("rule-hints:{}", root.record.id);
        let after = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let mut ids = root
            .instance
            .steps
            .iter()
            .filter(|e| {
                matches!(
                    e.status,
                    StepExecutionStatus::WaitingConfirmation
                        | StepExecutionStatus::AwaitingAcceptance
                        | StepExecutionStatus::Completed
                )
            })
            .map(|e| e.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        let order = ids
            .iter()
            .filter(|id| *id > &after)
            .chain(ids.iter().filter(|id| *id <= &after))
            .take(16)
            .cloned()
            .collect::<Vec<_>>();
        let began = std::time::Instant::now();
        for (n, id) in order.iter().enumerate() {
            if n > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            self.runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .insert(key.clone(), id.clone());
            let selected = one(
                self.store.as_ref(),
                "governance",
                &format!("workflow-rule-hint:{id}"),
            )?;
            let resources = root.step(self, id)?;
            if selected.value.is_some() {
                let hint = restored(self, &selected)?;
                let reads: ReadSet = serde_json::from_value(hint.data["basis"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
                let keys = reads
                    .versions()
                    .iter()
                    .map(|v| v.resource.clone())
                    .collect::<Vec<_>>();
                if self
                    .store
                    .read_resources(&keys)?
                    .iter()
                    .map(version)
                    .eq(reads.versions().iter().cloned())
                {
                    continue;
                }
            }
            let c = Command {
                command_id: format!("workflow-rule-hint:{id}:{}", resources.step.revision),
                target: Some(root.record.id.clone()),
                expected_revision: Some(root.record.revision),
                payload: json!({}),
            };
            let context = InvocationContext::workflow(
                WorkflowActor::Coordinator {
                    source_id: id.clone(),
                },
                Some(root.instance.project_id.clone()),
            );
            let mut selection =
                WorkflowCommit::new(self, "runtime.workflow.rule_hint", &c, &context)?;
            let step = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.node_id == resources.execution.node_id)
                .ok_or_else(|| err("storage", "原步骤合同缺失"))?;
            let mut requested = FactSelection::default();
            requested.rule(&step.completion);
            let actual = resources.selected_facts(self.store.as_ref(), &requested)?;
            selection.witness(actual.reads.versions().iter().cloned())?;
            let satisfied = selection.rule(
                &root.instance,
                &step.node_id,
                &step.completion,
                &actual.facts,
            )?;
            let data = json!({"execution_id":id,"plan_revision_id":root.instance.active_plan_revision_id,"basis":selection.reads,"satisfied":satisfied,"issuer":"kernel_read_projection"});
            let mut row = if selected.value.is_some() {
                let mut row = restored(self, &selected)?;
                update_record(&mut row, data);
                row
            } else {
                record(
                    "workflow_rule_hint",
                    id.clone(),
                    root.record.scope.clone(),
                    "完成条件显示提示".into(),
                    1,
                    None,
                    data,
                    false,
                )
            };
            row.id = selected.resource.id.clone();
            let fingerprint = configuration_hash(&row.data);
            let result = self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("workflow-rule-hint-admit:{id}:{fingerprint}"),
                fingerprint,
                read_set: selection.reads.merge([version(&selected)])?,
                changes: vec![ResourceChange {
                    resource: selected.resource,
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":id}),
                time: now() as i64,
            });
            if let Err(error) = result
                && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
            {
                return Err(error);
            }
        }
        Ok(())
    }
}
