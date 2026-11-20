//! Actual ancestors, input-runtime evidence and immutable continuation inputs.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_outputs::WorkflowOutputProof,
    workflow_resources::{WorkflowResources, version},
    *,
};
use std::collections::{BTreeMap, BTreeSet};
type ChildOutputObservation = (serde_json::Map<String, Value>, WorkflowOutputProof, u64);
impl Engine {
    pub(crate) fn native_workflow_cancelled(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        _resources: &WorkflowResources,
    ) -> Result<bool> {
        let mut current = root.record.id.clone();
        let mut seen = BTreeSet::new();
        for _ in 0..=WORKFLOW_DEPTH_LIMIT {
            require(seen.insert(current.clone()), "实际流程祖先存在环")?;
            let run = one(self.store.as_ref(), "workflow_run_authority", &current)?;
            commit.witness([version(&run)])?;
            let value = run
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原祖先流程缺失"))?;
            require(
                value["project_id"] == root.instance.project_id,
                "实际祖先流程跨项目",
            )?;
            if value["cancel_requested"] == true
                || (current != root.record.id
                    && matches!(value["status"].as_str(), Some("completed" | "cancelled")))
            {
                return Ok(true);
            }
            let Some(parent) = value["parent_execution_id"].as_str() else {
                return Ok(false);
            };
            let owner = one(self.store.as_ref(), "workflow_execution_owner", parent)?;
            commit.witness([version(&owner)])?;
            current = text(
                owner
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原父执行归属缺失"))?,
                "instance_id",
            )?;
            let step = one(
                self.store.as_ref(),
                "workflow_step_authority",
                &json!([current, parent]).to_string(),
            )?;
            commit.witness([version(&step)])?;
            if step.value.as_ref().is_none_or(|s| {
                s["stop_requested"] == true
                    || matches!(
                        s["status"].as_str(),
                        Some("completed" | "skipped" | "cancelled")
                    )
            }) {
                return Ok(true);
            }
        }
        Err(err("invalid_workflow", "实际祖先链超过预算"))
    }
    pub(crate) fn native_workflow_runtime_context(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        session: &Session,
    ) -> Result<Value> {
        let binding = session
            .runtime_binding
            .as_ref()
            .map(|b| crate::workflow_runtime_facts::public_binding(&session.id, b));
        let manifest = one(
            self.store.as_ref(),
            "governance",
            &format!("workflow-inputs:{}", resources.execution.id),
        )?;
        commit.witness([version(&manifest)])?;
        let mut inputs = BTreeMap::new();
        if let Some(raw) = &manifest.value {
            require(
                raw["type"] == "workflow_input_manifest"
                    && raw["immutable"] == true
                    && raw["data"]["input_digest"] == resources.execution.input_digest,
                "原运行上下文缺少冻结输入证据",
            )?;
            let targets = raw["data"]["targets"]
                .as_array()
                .ok_or_else(|| err("storage", "输入证据目标缺失"))?;
            require(targets.len() <= WORKFLOW_STEP_LIMIT, "输入证据超过预算")?;
            for target in targets {
                let Some(id) = target["origin_ref"].as_str() else {
                    continue;
                };
                let output = one(self.store.as_ref(), "governance", id)?;
                commit.witness([version(&output)])?;
                if let Some(raw) = &output.value
                    && raw["type"] == "workflow_output"
                    && raw["immutable"] == true
                    && raw["data"]["provenance"]["kind"] == "agent"
                {
                    let runs = raw["data"]["provenance"]["runtime_executions"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if !runs.is_empty() {
                        inputs.insert(
                            text(target, "input")?,
                            json!({"source_output_id":id,"runtime_executions":runs}),
                        );
                    }
                }
            }
        }
        let mut exceptions = Vec::new();
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == resources.execution.step)
            .ok_or_else(|| err("storage", "步骤合同缺失"))?;
        let ancestors = resources.instance.plan.document.ancestors(step);
        for e in resources
            .instance
            .steps
            .iter()
            .filter(|e| ancestors.contains(e.step.as_str()))
        {
            let selected = one(self.store.as_ref(), "workflow_confirmations", &e.id)?;
            commit.witness([version(&selected)])?;
            for row in selected
                .value
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if row["data"]["manual_approval"] == true
                    && row["data"]["input_digest"] == e.input_digest
                    && row["data"]["outputs"] == json!(e.outputs)
                {
                    exceptions.push(json!({"confirmation_ref":row["id"],"execution_id":e.id,"outputs":e.outputs,"reason":row["data"]["reason"],"scope":row["data"]["exception_scope"],"basis":row["data"]["approval_basis"]}));
                }
            }
        }
        let mut context = json!({"human_exceptions":exceptions});
        context["workspace_policy"] = json!({
            "source_directory":"/work",
            "runtime_directory":WORKSPACE_RUNTIME_DIRECTORY,
            "dependency_cache_directory":format!("{WORKSPACE_RUNTIME_DIRECTORY}/shared"),
            "cache_scope":"project by default; cross-project sharing requires explicit Host opt-in",
            "runtime_environment_variable":"DEVDESK_RUNTIME_DIR",
            "instructions":"Keep downloaded browsers, package installations, virtual environments, caches, model weights and build output outside /work, including evidence directories. Use DEVDESK_RUNTIME_DIR and the configured package-manager paths; for npm use npm install --prefix \"$DEVDESK_RUNTIME_DIR/node\" instead of installing into the source directory. Keep only project source, dependency manifests/lockfiles, tests and final acceptance reports/screenshots in /work. Do not copy installed dependencies into Git or evidence. Large intentional project assets must be explicitly declared in snapshot allowed_paths."
        });
        context["test_execution_policy"] = json!({
            "instructions":"Read the repository's existing test instructions and reuse its maintained acceptance entry points before writing a new browser script. Keep formal project regression tests in the repository's normal test locations; they are different from the frozen independent workflow assertions, which the implementation must not modify. Unless the frozen plan assigns another authorized writer, the implementation role maintains formal project regression tests and reusable acceptance entry points; the independent test author stays within its declared validation directory. Browser and server checks must have bounded timeouts and close pages, browsers and child processes in finally blocks, including after assertion or launch failures. Finish and close one browser run before starting another; a failed run is not evidence of success. Use actual project files or a correctly awaited local server. Verify required libraries and fonts in the external runtime directories before launching. Preserve the actual successful command observation and source identity for review."
        });
        let runtime = json!({"binding":binding,"inputs":inputs,"instructions":"binding is the kernel-frozen runtime selection for this Session, not a claim about arbitrary nested or remote commands. inputs contains kernel-observed outer harness execution facts bound to exact producer Turns and stop proofs. Use these image identities alongside the actual command logs and code snapshots; model-reported Node versions alone do not identify an image. Do not invent missing facts."});
        context
            .as_object_mut()
            .unwrap()
            .extend(runtime.as_object().unwrap().clone());
        Ok(context)
    }
    pub(crate) fn native_retry_supplement(
        &self,
        commit: &mut WorkflowCommit<'_>,
        execution: &StepExecution,
    ) -> Result<Option<String>> {
        let mut previous = execution.retry_of.clone();
        let mut seen = BTreeSet::new();
        while let Some(id) = previous {
            require(
                seen.insert(id.clone()) && seen.len() <= 32,
                "技术重试来源存在环或超过边界",
            )?;
            let history = one(self.store.as_ref(), "workflow_history_source", &id)?;
            let record = restored(self, &history)?;
            let old: StepExecution =
                serde_json::from_value(self.store.restore_runtime_value(&record.data)?)
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                record.r#type == "workflow_execution_history"
                    && record.immutable
                    && old.workflow_instance_id == execution.workflow_instance_id
                    && old.node_id == execution.node_id
                    && old.round == execution.round,
                "技术重试来源与原步骤不符",
            )?;
            commit.witness([version(&history)])?;
            let command = format!(
                "workflow:{}",
                configuration_hash(&(old.id.as_str(), old.sequence, "conversation.send"))
            );
            let input = one(self.store.as_ref(), "workflow_command_intent", &command)?;
            commit.witness([version(&input)])?;
            if let Some(raw) = &input.value {
                let facts = self
                    .store
                    .workflow_intent_facts(&text(raw, "id")?, false)?
                    .ok_or_else(|| err("not_found", "原输入意图缺失"))?;
                require(
                    facts.row.revision == input.revision && facts.record.definition_id == old.id,
                    "原输入意图在准备期间已变化",
                )?;
                let task: Value =
                    serde_json::from_str(&text(&facts.record.data["command"]["payload"], "input")?)
                        .map_err(|e| err("storage", e.to_string()))?;
                return Ok(task["supplemental_input"].as_str().map(str::to_owned));
            }
            previous = old.retry_of;
        }
        Ok(None)
    }
    pub(crate) fn native_child_outputs(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        step: &StepDefinition,
    ) -> Result<Option<ChildOutputObservation>> {
        let group = one(
            self.store.as_ref(),
            "workflow_children_source",
            &resources.execution.id,
        )?;
        commit.witness([version(&group)])?;
        let raw = group
            .value
            .as_ref()
            .ok_or_else(|| err("invalid_workflow", "实际子流程关联缺失"))?;
        require(
            raw["type"] == "workflow_children" && raw["immutable"] == true,
            "子流程关联不是原不可变来源",
        )?;
        let ids: Vec<Id> = serde_json::from_value(raw["data"]["children"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(ids.len() <= WORKFLOW_STEP_LIMIT, "子流程集合超过预算")?;
        let StepExecutor::Workflow { foreach, .. } = &step.executor else {
            return Err(err("storage", "非子流程步骤不能汇总"));
        };
        let mut values: BTreeMap<String, Vec<Value>> =
            step.outputs.keys().map(|k| (k.clone(), vec![])).collect();
        let mut finished = resources.execution.started.unwrap_or(0);
        for (index, id) in ids.iter().enumerate() {
            let child = WorkflowHeaderFacts::read(self, id)?;
            commit.witness([version(&child.header)])?;
            require(
                child.instance.parent_execution_id.as_deref()
                    == Some(resources.execution.id.as_str())
                    && child.instance.root_instance_id == resources.instance.root_instance_id
                    && child.instance.project_id == resources.instance.project_id,
                "实际子流程不属于原调用",
            )?;
            if let Some(failed) = child.instance.steps.iter().find(|e| {
                matches!(
                    e.status,
                    StepExecutionStatus::Failed
                        | StepExecutionStatus::Cancelled
                        | StepExecutionStatus::NeedsReconciliation
                )
            }) {
                let selected = child.step(self, &failed.id)?;
                commit.witness(selected.reads.versions().iter().cloned())?;
                let mut error = err(
                    "child_execution_failed",
                    format!("实际子流程执行未成功：{}", selected.execution.reason),
                );
                if let Some(failure) = selected.execution.failure {
                    error.execution_failure = Some(ExecutionFailureDisposition {
                        kind: failure.kind,
                        effect: failure.effect,
                    });
                }
                return Err(error);
            }
            if child.instance.status != StepExecutionStatus::Completed {
                return Ok(None);
            }
            if let Some(collection) = foreach {
                let members = raw["data"]["collection"]["value"]
                    .as_array()
                    .ok_or_else(|| err("storage", "原集合成员未冻结"))?;
                require(
                    members.len() == ids.len()
                        && child.instance.inputs.get(&collection.item_input) == members.get(index),
                    "汇总集合成员遗漏、重复或顺序变化",
                )?;
            }
            let first = child
                .instance
                .steps
                .first()
                .ok_or_else(|| err("storage", "实际子流程没有执行步骤"))?;
            let selected = child.step(self, &first.id)?;
            let mut selection = FactSelection::default();
            for name in step.outputs.keys() {
                selection.source(
                    child
                        .instance
                        .plan
                        .document
                        .outputs
                        .get(name)
                        .ok_or_else(|| err("invalid_workflow", "子流程未声明汇总端口"))?,
                );
            }
            let facts = selected.selected_facts(self.store.as_ref(), &selection)?;
            commit.witness(facts.reads.versions().iter().cloned())?;
            for name in step.outputs.keys() {
                let source = &child.instance.plan.document.outputs[name];
                values.get_mut(name).unwrap().push(
                    facts
                        .facts
                        .value(source)
                        .cloned()
                        .ok_or_else(|| err("output_invalid", "实际完成子流程缺少声明产物"))?,
                );
            }
            finished = finished.max(
                child
                    .instance
                    .steps
                    .iter()
                    .filter_map(|e| e.finished)
                    .max()
                    .unwrap_or(child.record.created_at),
            );
        }
        let outputs = values
            .into_iter()
            .map(|(name, mut values)| {
                Ok((
                    name,
                    if foreach.is_some() {
                        json!(values)
                    } else {
                        require(values.len() == 1, "非集合子流程必须有唯一真实成员")?;
                        values.remove(0)
                    },
                ))
            })
            .collect::<Result<serde_json::Map<_, _>>>()?;
        Ok(Some((
            outputs.clone(),
            WorkflowOutputProof::Children { ids, outputs },
            finished,
        )))
    }
}
