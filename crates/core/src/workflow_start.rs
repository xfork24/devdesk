//! Work start compiles only the complete configuration catalog. Native owner
//! and active-root predicates protect creation, plan publication and source heads.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    platform_facts::{one, restored},
    work_owners::WorkOwnerFacts,
    workflow_commits::WorkflowCommit,
    workflow_resources::version,
    *,
};
use std::collections::BTreeMap;

pub(crate) fn queued_execution(
    instance: &str,
    workflow: &str,
    step: &StepDefinition,
    id: Id,
    path: String,
) -> StepExecution {
    StepExecution {
        time_budget: None,
        plan_revision_id: String::new(),
        node_id: step.node_id.clone(),
        contract_digest: configuration_hash(step),
        id,
        workflow_instance_id: instance.into(),
        workflow_revision_id: workflow.into(),
        path,
        step: step.key.clone(),
        round: 1,
        sequence: 1,
        failure: None,
        retry_of: None,
        status: StepExecutionStatus::Queued,
        input_digest: String::new(),
        budget_wait_started: None,
        budget_waited_ms: 0,
        rework_workspace: None,
        rework: None,
        inputs: BTreeMap::new(),
        runtime_context: Value::Null,
        outputs: BTreeMap::new(),
        agent_revision_id: None,
        session_id: None,
        turn_id: None,
        tool_call_id: None,
        started: None,
        finished: None,
        reason: String::new(),
    }
}
impl Engine {
    pub(crate) fn native_workflow_start(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return self.store.restore_runtime_record(&receipt.result);
        }
        let owner = WorkOwnerFacts::read(
            self.store.as_ref(),
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少工作归属"))?,
        )?;
        owner.require_revision(c.expected_revision)?;
        owner.require_editable()?;
        context.require_project(&owner.project)?;
        let mut commit = WorkflowCommit::new(self, "workflow.start", c, context)?;
        commit.witness(owner.read_set.versions().iter().cloned())?;
        let active = one(
            self.store.as_ref(),
            "workflow_active_owner",
            &owner.owner.resource.id,
        )?;
        require(
            active.value.as_ref().is_some_and(|v| v["active"] == false),
            "同一工作已有未结束根流程",
        )?;
        commit.witness([version(&active)])?;
        if !matches!(context.actor(), WorkflowActor::Human) {
            let WorkflowActor::Controller {
                controller_id,
                session_id,
                turn_id,
                epoch,
            } = context.actor()
            else {
                return Err(err("permission_denied", "此来源不能启动新的工作根"));
            };
            let source = one(self.store.as_ref(), "governance", controller_id)?;
            let row = restored(self, &source)?;
            let controller: WorkflowController = row.decode()?;
            require(
                row.r#type == "workflow_controller"
                    && controller.project_id == owner.project
                    && controller.owner_id == owner.owner.resource.id
                    && controller.session_id.as_deref() == Some(session_id)
                    && controller.epoch == *epoch
                    && controller.status == "active",
                "原经理不拥有此工作",
            )?;
            controller.policy.permits("start")?;
            require(
                !controller.policy.human_operations.contains("start"),
                "启动需要实际人类决定",
            )?;
            let governed = one(
                self.store.as_ref(),
                "workflow_governed_owner",
                &owner.owner.resource.id,
            )?;
            require(
                governed.revision == 0,
                "受人工决策保护的工作需要人类启动后继流程",
            )?;
            let lease = one(
                self.store.as_ref(),
                "controller_lease_active",
                &json!([controller_id, epoch]).to_string(),
            )?;
            require(lease.revision == 1, "经理租约已失效")?;
            let workers = one(self.store.as_ref(), "session_live_workers", session_id)?;
            let actual = workers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| err("storage", "经理原生 worker 缺失"))?;
            let native = actual
                .iter()
                .filter(|w| w["kind"] == "harness")
                .collect::<Vec<_>>();
            require(native.len() == 1, "经理需要唯一原生 worker")?;
            let guards =
                self.native_worker_guards(session_id, turn_id, &text(native[0], "operation_id")?)?;
            commit.witness(guards.versions().iter().cloned())?;
            commit.witness([
                version(&source),
                version(&lease),
                version(&workers),
                version(&governed),
            ])?;
        }
        let before = self.store.configuration_snapshot()?;
        let mut catalog = before.state.as_ref().clone();
        let inherited = if let Some(id) = c.payload["work_configuration_ref"].as_str() {
            Some(before.state.configuration(id)?.clone())
        } else {
            crate::standards::work_configuration(&before.state, &owner.owner.resource.id).cloned()
        };
        if let Some(row) = &inherited {
            require(
                row.r#type == "work_configuration" && row.definition_id == owner.owner.resource.id,
                "工作配置不属于此工作",
            )?;
        }
        let work = match owner.owner.resource.kind.as_str() {
            "project" => WorkReference::Project {
                id: owner.owner.resource.id.clone(),
            },
            "feature" => WorkReference::Feature {
                id: owner.owner.resource.id.clone(),
            },
            _ => WorkReference::Task {
                id: owner.owner.resource.id.clone(),
            },
        };
        let configuration = crate::standards::select_work_configuration_for(
            &mut catalog,
            &work,
            &owner.scope,
            &c.payload,
            inherited.as_ref(),
        )?;
        let workflow = crate::governance::reference(
            &catalog,
            &text(&configuration.data, "workflow_ref")?,
            "workflow",
            &owner.scope,
        )?
        .clone();
        let bindings = serde_json::from_value(configuration.data["role_agent_bindings"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let selected = crate::standards::selection_state(&catalog, &configuration.data);
        let mut plan =
            self.compile_configured_workflow(&selected, &workflow, &owner.scope, Some(&bindings))?;
        plan.standard_ref = Some(text(&configuration.data, "standard_ref")?);
        plan.work_configuration_ref = Some(configuration.id.clone());
        let request = one(
            self.store.as_ref(),
            "feature_request_source",
            &owner.owner.resource.id,
        )?;
        commit.witness([version(&request)])?;
        let mut inputs: BTreeMap<String, Value> = serde_json::from_value(
            c.payload
                .get("inputs")
                .cloned()
                .or_else(|| {
                    request
                        .value
                        .as_ref()
                        .and_then(|r| r["data"].get("workflow_inputs").cloned())
                })
                .unwrap_or(json!({})),
        )
        .map_err(|e| err("invalid_params", e.to_string()))?;
        if plan.document.inputs.contains_key("test_configuration") {
            let tests = configuration
                .data
                .get("test_configuration")
                .filter(|v| !v.is_null())
                .cloned();
            let tests = match tests {
                Some(tests) => tests,
                None => {
                    json!(self.discover_project_tests()?.configuration)
                }
            };
            serde_json::from_value::<ProjectTestConfiguration>(tests.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?
                .validate()?;
            require(
                inputs.get("test_configuration").is_none_or(|v| v == &tests),
                "不能覆盖冻结项目测试配置",
            )?;
            inputs.insert("test_configuration".into(), tests.clone());
        }
        self.validate_workflow_inputs(&catalog, &plan, &inputs)?;
        let instance_id =
            stable_object_id(self.store.as_ref(), &c.command_id, "workflow-instance")?;
        let steps = plan
            .document
            .steps
            .iter()
            .map(|step| {
                Ok(queued_execution(
                    &instance_id,
                    &workflow.id,
                    step,
                    stable_object_id(
                        self.store.as_ref(),
                        &c.command_id,
                        &format!("workflow-step:{}", step.node_id),
                    )?,
                    format!("/{}", step.key),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut instance=ConfiguredWorkflowInstance{active_plan_revision_id:String::new(),control_version:1,schema_version:WORKFLOW_INSTANCE_SCHEMA,owner_id:owner.owner.resource.id.clone(),workflow_ref:workflow.id.clone(),project_id:owner.project.clone(),plan,role_agent_bindings:bindings,inputs,user_request:request.value.as_ref().map(|r|r["data"]["user_request"].clone()).unwrap_or_else(||if owner.owner.resource.kind=="feature"{json!({"feature_id":owner.owner.resource.id,"title":owner.owner.value.as_ref().unwrap()["title"],"initial_requirement":""})}else{Value::Null}),human_governance:false,steps,status:StepExecutionStatus::Queued,paused:false,parent_execution_id:None,root_instance_id:instance_id.clone(),cancel_requested:false};
        let frozen = crate::orchestration::freeze_plan_record(
            &mut catalog,
            &mut instance,
            &owner.scope,
            "initial",
        )?;
        register_template_members(&mut catalog, template_catalog_generation(&before.state));
        let candidate =
            ConfigurationCatalogSnapshot::new(catalog, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        commit.witness(publication.read_set.versions().iter().cloned())?;
        for change in publication.changes {
            commit.change(change)?;
        }
        let mut row = record(
            "workflow_instance",
            owner.owner.resource.id.clone(),
            owner.scope.clone(),
            workflow.name,
            1,
            None,
            json!(instance),
            false,
        );
        row.id = instance_id;
        for fact in [&frozen, &row] {
            let key = ResourceKey::new("governance", &fact.id);
            commit.witness([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            commit.change(ResourceChange {
                resource: key,
                value: self.store.encode_configuration_record(&candidate, fact)?,
            })?;
        }
        if owner.owner.resource.kind == "feature"
            && owner
                .owner
                .value
                .as_ref()
                .is_some_and(|v| v["status"] != "running")
        {
            let mut next = owner.owner.value.clone().unwrap();
            next["status"] = json!(Status::Running);
            next["revision"] = json!(owner.owner.revision + 1);
            commit.change(ResourceChange {
                resource: owner.owner.resource.clone(),
                value: next,
            })?;
        }
        commit.initialize_rework_budgets(&instance, &row.scope)?;
        self.prepare_initial_native_driver(
            &mut commit,
            &row,
            &instance,
            &configuration.data["orchestration"],
            &candidate,
        )?;
        let result = self.store.encode_configuration_record(&candidate, &row)?;
        let receipt = commit.finish(fingerprint, result)?;
        self.store.restore_runtime_record(&receipt)
    }
}
