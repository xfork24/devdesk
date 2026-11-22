//! Invocation authority is checked from real driver/controller leases and
//! exact human decisions, independently of the workflow's business rules.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
impl WorkflowCommit<'_> {
    pub fn authorize(&mut self, root: &WorkflowHeaderFacts) -> Result<()> {
        let mut human_input = false;
        let mut manager_diagnosis = false;
        if self.method == "workflow.step.input" {
            let input = text(&self.command.payload, "input")?;
            if matches!(self.context.actor(), WorkflowActor::Controller { .. })
                && !crate::workflow_material_inputs::MaterialInput::is_reference(&input)
                && let Some(id) = self.command.payload["execution_id"].as_str()
            {
                let resources = root.step(self.engine, id)?;
                manager_diagnosis = resources.execution.status == StepExecutionStatus::Failed
                    && resources
                        .execution
                        .failure
                        .as_ref()
                        .is_some_and(|f| f.code == "needs_manager");
                if manager_diagnosis {
                    self.witness(resources.reads.versions().iter().cloned())?;
                }
            }
            // Preserve an actual Human reply in full, even when it quotes
            // platform evidence. Only explicit references opt into transfer.
            if !manager_diagnosis
                && !crate::workflow_material_inputs::MaterialInput::is_reference(&input)
            {
                human_input = self.relays_human_input(root)?;
            }
            if !human_input && !manager_diagnosis {
                self.material_input = self.resolve_material_input(root, &input)?;
            }
        }
        let actor = self.context.actor();
        let method = self.method;
        if matches!(actor, WorkflowActor::Human) {
            return Ok(());
        }
        require(
            ![
                "workflow.step.confirm",
                "workflow.step.approve",
                "workflow.decision.resolve",
                "orchestration.takeover",
                "workflow.driver.select",
            ]
            .contains(&method),
            "非人类调用不能签署人工决定或接管",
        )?;
        let governed = one(
            self.engine.store.as_ref(),
            "workflow_governed",
            &root.instance.root_instance_id,
        )?;
        self.witness([version(&governed)])?;
        if governed
            .value
            .as_ref()
            .is_some_and(|v| v["governed"] == true)
        {
            if method == "workflow.step.input"
                && self.material_input.is_none()
                && !human_input
                && !manager_diagnosis
            {
                return Err(err(
                    "human_input_required",
                    "需要人类判断的问题必须由人类回答；已有平台材料请先 workflow.outputs.read，再使用返回的 input_ref 原样转交，无需人工确认",
                ));
            }
            if method == "workflow.change.apply" {
                self.require_signed_request(root)?;
            }
        }
        let driver = one(
            self.engine.store.as_ref(),
            "workflow_driver_root",
            &root.instance.root_instance_id,
        )?;
        self.witness([version(&driver)])?;
        let selected = driver.value.as_ref();
        match actor {
            WorkflowActor::Human => {}
            WorkflowActor::Coordinator { .. } => {
                return Err(err("permission_denied", "系统事实投影不能授权业务控制"));
            }
            WorkflowActor::Driver { driver_id, epoch } => {
                require(
                    selected.is_some_and(|d| {
                        d["id"] == *driver_id
                            && d["data"]["mode"] == "rust"
                            && d["data"]["epoch"] == *epoch
                    }),
                    "Rust 驱动授权已撤销",
                )?;
                let lease = one(
                    self.engine.store.as_ref(),
                    "driver_lease_active",
                    &json!([driver_id, epoch]).to_string(),
                )?;
                require(lease.revision == 1, "Rust 驱动租约已失效")?;
                self.witness([version(&lease)])?;
                let catalog = self.engine.store.configuration_snapshot()?;
                let policy = profile_control_policy(
                    &catalog.state,
                    root.instance
                        .plan
                        .harness_permission_ref
                        .as_deref()
                        .ok_or_else(|| err("permission_unconfigured", "原流程缺少控制策略"))?,
                    Some(&root.instance.plan.document.scheduling.agent),
                )?;
                let operation = method.rsplit('.').next().unwrap_or(method);
                policy.permits(operation)?;
                require(
                    !policy.human_operations.contains(operation),
                    "该操作需要实际人类决定",
                )?;
            }
            WorkflowActor::Controller {
                controller_id,
                session_id,
                turn_id,
                epoch,
            } => {
                if let Some(driver) = selected {
                    require(
                        driver["data"]["mode"] == "agent"
                            && driver["data"]["controller_id"] == *controller_id,
                        "当前根流程控制权属于其他驱动",
                    )?;
                }
                let row = one(self.engine.store.as_ref(), "governance", controller_id)?;
                let actual = restored(self.engine, &row)?;
                let controller: WorkflowController = actual.decode()?;
                require(
                    actual.r#type == "workflow_controller"
                        && controller.project_id == root.instance.project_id
                        && controller.status == "active"
                        && controller.epoch == *epoch
                        && controller.session_id.as_deref() == Some(session_id)
                        && controller.root_instance_id.as_deref()
                            == Some(root.instance.root_instance_id.as_str()),
                    "控制器不拥有原流程、会话或代次",
                )?;
                let operation = match method {
                    "workflow.decision.request" => "request_decision",
                    "workflow.segment.dispatch" => "segment",
                    _ => method.rsplit('.').next().unwrap_or(method),
                };
                controller.policy.permits(operation)?;
                if controller.policy.human_operations.contains(operation)
                    && self.material_input.is_none()
                {
                    self.require_signed_request(root)?;
                }
                let lease = one(
                    self.engine.store.as_ref(),
                    "controller_lease_active",
                    &json!([controller_id, epoch]).to_string(),
                )?;
                require(lease.revision == 1, "原控制器租约已失效")?;
                let workers = one(
                    self.engine.store.as_ref(),
                    "session_live_workers",
                    session_id,
                )?;
                let rows = workers
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .ok_or_else(|| err("storage", "控制器原 worker 缺失"))?;
                let actual = rows
                    .iter()
                    .filter(|w| w["kind"] == "harness")
                    .collect::<Vec<_>>();
                require(actual.len() == 1, "控制器需要唯一实际原生 worker")?;
                let (current, instance) = self.engine.controller_instance_current(&controller)?;
                require(current, "原经理 Agent 实例已换绑或移除")?;
                self.witness(instance.versions().iter().cloned())?;
                let reads = self.engine.native_worker_guards(
                    session_id,
                    turn_id,
                    &text(actual[0], "operation_id")?,
                )?;
                self.witness([version(&row), version(&lease), version(&workers)])?;
                self.witness(reads.versions().iter().cloned())?;
            }
            WorkflowActor::Segment { authorization_id } => {
                let row = one(self.engine.store.as_ref(), "governance", authorization_id)?;
                let segment = row
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "委托授权缺失"))?;
                require(
                    segment["type"] == "workflow_segment"
                        && segment["data"]["status"] == "active"
                        && segment["definition_id"] == root.record.id
                        && segment["data"]["plan_revision_id"]
                            == root.instance.active_plan_revision_id
                        && crate::workflow_segments::segment_driver_matches(
                            selected,
                            &segment["data"],
                        ),
                    "委托已撤销或不属于原流程",
                )?;
                let node = if method == "workflow.decision.request" {
                    self.command.payload["arguments"]["execution_id"].as_str()
                } else {
                    self.command.payload["node_id"]
                        .as_str()
                        .or_else(|| self.command.payload["step"].as_str())
                };
                if let Some(node) = node {
                    let execution = root
                        .instance
                        .steps
                        .iter()
                        .find(|e| e.node_id == node || e.step == node || e.id == node)
                        .ok_or_else(|| err("not_found", "委托目标步骤缺失"))?;
                    require(
                        segment["data"]["node_ids"]
                            .as_array()
                            .is_some_and(|ids| ids.contains(&json!(execution.node_id))),
                        "步骤不属于原委托范围",
                    )?;
                }
                match method {
                    "workflow.step.dispatch" | "workflow.step.accept" => {}
                    "workflow.decision.request" => {
                        require(
                            segment["data"]["prepare_human_requests"] == true
                                && segment["data"]["include_human"] == true
                                && self.command.payload["operation"] == "workflow.step.confirm"
                                && node.is_some(),
                            "原委托没有普通审批材料准备权",
                        )?;
                        let e = root.step(self.engine, node.unwrap())?;
                        let ready = self.engine.native_step_readiness(root, &e)?;
                        require(
                            ready["completion_satisfied"] == true
                                && ready["human_satisfied"] != true
                                && ready["actions"]["accept"]["code"] == "human_confirmation",
                            "自动请求必须满足原完成条件且确实需要普通人工签核",
                        )?;
                        self.witness(e.reads.versions().iter().cloned())?;
                    }
                    "workflow.complete" => {
                        require(
                            segment["data"]["complete_instance"] == true
                                && root.instance.steps.iter().all(|e| {
                                    segment["data"]["node_ids"]
                                        .as_array()
                                        .is_some_and(|ids| ids.contains(&json!(e.node_id)))
                                }),
                            "自动完成必须持有完整实例委托",
                        )?;
                    }
                    _ => return Err(err("permission_denied", "该操作不属于原有界委托")),
                }
                let operations = segment["data"]["operations"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "原委托预算缺失"))?;
                require(
                    operations < segment["data"]["max_operations"].as_u64().unwrap_or(0),
                    "原委托预算已耗尽",
                )?;
                if segment["data"]["actor"]["kind"] == "controller" {
                    let id = text(&segment["data"]["actor"], "controller_id")?;
                    let epoch = segment["data"]["actor"]["epoch"]
                        .as_u64()
                        .ok_or_else(|| err("storage", "原委托代次缺失"))?;
                    let authority = one(self.engine.store.as_ref(), "controller_authority", &id)?;
                    require(
                        authority.value.as_ref().is_some_and(|r| {
                            r["data"]["epoch"] == epoch && r["data"]["status"] == "active"
                        }),
                        "原委托决定者已撤权",
                    )?;
                    self.witness([version(&authority)])?;
                    if matches!(method, "workflow.decision.request" | "workflow.complete") {
                        let current = one(self.engine.store.as_ref(), "governance", &id)?;
                        let owner = restored(self.engine, &current)?;
                        let controller: WorkflowController = owner.decode()?;
                        let operation = if method == "workflow.decision.request" {
                            "request_decision"
                        } else {
                            "complete"
                        };
                        controller.policy.permits(operation)?;
                        require(
                            !controller.policy.human_operations.contains(operation),
                            "委托操作仍需独立人类授权",
                        )?;
                        self.witness([version(&current)])?;
                    }
                }
                let original = restored(self.engine, &row)?;
                let mut data = original.data.clone();
                data["operations"] = json!(operations + 1);
                self.replace_record(&row, original, data)?;
            }
        }
        Ok(())
    }
    fn relays_human_input(&mut self, root: &WorkflowHeaderFacts) -> Result<bool> {
        let WorkflowActor::Controller {
            controller_id,
            session_id,
            turn_id,
            epoch,
        } = self.context.actor()
        else {
            return Ok(false);
        };
        if self.command.payload["decision_ref"].is_string() {
            // A Human may approve the exact proposed supplemental input.
            // This never treats manager-authored text as a Human message.
            self.require_signed_request(root)?;
            return Ok(true);
        }
        let Some(input) = self.command.payload["input"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
        else {
            return Ok(false);
        };
        let row = one(
            self.engine.store.as_ref(),
            "controller_turn_input",
            &json!([controller_id, epoch, turn_id]).to_string(),
        )?;
        self.witness([version(&row)])?;
        let Some(raw) = row.value.as_ref() else {
            return Ok(false);
        };
        let payload = &raw["data"]["command"]["payload"];
        if raw["definition_id"] != *controller_id
            || raw["data"]["command"]["target"] != *session_id
            || payload["controller_epoch"] != *epoch
        {
            return Ok(false);
        }
        let Some(refs) = payload["controller_input_refs"].as_array() else {
            return Ok(false);
        };
        require(refs.len() <= 8, "原经理唤醒输入超过预算")?;
        let mut matching = false;
        for id in refs {
            let row = one(
                self.engine.store.as_ref(),
                "governance",
                id.as_str()
                    .ok_or_else(|| err("storage", "原用户输入引用无效"))?,
            )?;
            if let Some(raw) = &row.value {
                matching |= raw["type"] == "controller_input"
                    && raw["definition_id"] == *controller_id
                    && raw["data"]["input"] == input;
            }
            self.witness([version(&row)])?;
        }
        let controller = one(self.engine.store.as_ref(), "governance", controller_id)?;
        self.witness([version(&controller)])?;
        Ok(matching
            && controller.value.as_ref().is_some_and(|r| {
                r["data"]["root_instance_id"] == root.instance.root_instance_id
                    && r["data"]["session_id"] == *session_id
                    && r["data"]["epoch"] == *epoch
            }))
    }
    pub(crate) fn require_signed_request(&mut self, root: &WorkflowHeaderFacts) -> Result<()> {
        let mut arguments = self.command.payload.clone();
        let id = arguments
            .as_object_mut()
            .and_then(|o| o.remove("decision_ref"))
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| err("human_decision_required", "该操作需要人类批准确切请求"))?;
        let row = one(self.engine.store.as_ref(), "governance", &id)?;
        let raw = row
            .value
            .as_ref()
            .ok_or_else(|| err("human_decision_required", "原人类签署缺失"))?;
        require(
            raw["type"] == "workflow_human_decision"
                && raw["immutable"] == true
                && raw["data"]["actor"]["kind"] == "human"
                && raw["data"]["approved"] == true
                && raw["data"]["plan_revision_id"] == root.instance.active_plan_revision_id
                && raw["data"]["request"]["operation"] == self.method
                && raw["data"]["request"]["arguments"] == arguments,
            "人类签署不属于该确切请求",
        )?;
        let pending = one(
            self.engine.store.as_ref(),
            "governance",
            &text(raw, "definition_id")?,
        )?;
        require(
            pending.value.as_ref().is_some_and(|r| {
                r["type"] == "workflow_pending_decision"
                    && r["definition_id"] == root.record.id
                    && r["data"]["status"] == "approved"
                    && r["data"]["decision_ref"] == id
            }),
            "原批准请求已失效",
        )?;
        self.witness([version(&row), version(&pending)])
    }
}
