//! Human-facing projections from exact workflow resources, never an incomplete
//! EngineeringState. Display witnesses do not replace command authorization.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};

impl Engine {
    pub(crate) fn controller_workspace_view(&self, p: &Value) -> Result<Value> {
        let mut value = json!(
            self.store
                .orchestration_summary(&crate::read_models::ControllerQuery::parse(p)?)?
        );
        let root = p["root_instance_id"]
            .as_str()
            .or_else(|| value["record"]["data"]["root_instance_id"].as_str());
        let decisions = if let Some(root) = root {
            one(self.store.as_ref(), "workflow_root_decisions", root)?
                .value
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        } else {
            value["decisions"].as_array().cloned().unwrap_or_default()
        };
        let mut full = Vec::new();
        for decision in decisions.into_iter().take(20) {
            let selected = one(self.store.as_ref(), "governance", &text(&decision, "id")?)?;
            let row = restored(self, &selected)?;
            full.push(self.controller_decision_view(&row)?);
        }
        value["decisions"] = json!(full);
        if let Some(id) = value["record"]["id"].as_str() {
            let interactions = one(self.store.as_ref(), "controller_interaction_page", id)?;
            value["interactions"] = interactions.value.unwrap_or(json!([]));
        }
        require(
            value.to_string().len() <= 512 * 1024,
            "经理视图超过预算，请缩小分页",
        )?;
        Ok(value)
    }
    pub(crate) fn controller_approval_context(&self, root: &str) -> Result<Vec<Value>> {
        let selected = one(self.store.as_ref(), "workflow_root_decisions", root)?;
        let mut result = Vec::new();
        for raw in selected
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|r| r["status"] == "pending")
        {
            let selected = one(self.store.as_ref(), "governance", &text(raw, "id")?)?;
            let row = restored(self, &selected)?;
            let value = self.controller_decision_view(&row)?;
            if value["current"] == true {
                result.push(value);
            }
        }
        for approval in self
            .controller_step_approvals(root)?
            .into_iter()
            .filter(|a| a["human_required"] == true)
        {
            let args = &approval["arguments"];
            if result
                .iter()
                .any(|d| d["data"]["request"]["arguments"]["execution_id"] == args["execution_id"])
            {
                continue;
            }
            result.push(json!({"id":args["execution_id"],"revision":args["expected_step_revision"],"approval_source":"workflow_step","definition_id":approval["instance_id"],"current":true,"data":{"status":"pending","plan_revision_id":args["plan_revision_id"],"request":{"operation":"workflow.step.confirm","arguments":args,"interaction":approval["interaction"]}}}));
        }
        Ok(result)
    }
    pub(crate) fn controller_ancestors(&self, id: &str) -> Result<Vec<Id>> {
        let mut current = WorkflowHeaderFacts::read(self, id)?;
        let mut result = Vec::new();
        for _ in 0..WORKFLOW_DEPTH_LIMIT {
            let Some(parent) = &current.instance.parent_execution_id else {
                return Ok(result);
            };
            let owner = one(self.store.as_ref(), "workflow_execution_owner", parent)?;
            let id = text(
                owner
                    .value
                    .as_ref()
                    .ok_or_else(|| err("storage", "祖先执行缺失"))?,
                "instance_id",
            )?;
            require(!result.contains(&id), "流程祖先循环")?;
            result.push(id.clone());
            current = WorkflowHeaderFacts::read(self, &id)?;
        }
        Err(err("storage", "流程祖先超过预算"))
    }
    pub(crate) fn controller_decision_view(&self, row: &ConfigurationRecord) -> Result<Value> {
        let instance = WorkflowHeaderFacts::read(self, &row.definition_id)?;
        let request = &row.data["request"];
        let current = instance.instance.active_plan_revision_id == row.data["plan_revision_id"]
            && request["arguments"]["execution_id"]
                .as_str()
                .is_none_or(|id| {
                    instance.instance.steps.iter().any(|e| {
                        e.id == id
                            && json!(e.input_digest) == request["arguments"]["input_digest"]
                            && json!(e.outputs) == request["arguments"]["outputs"]
                            && matches!(
                                e.status,
                                StepExecutionStatus::WaitingConfirmation
                                    | StepExecutionStatus::AwaitingAcceptance
                            )
                    })
                });
        let mut value = json!(row);
        value["current"] = json!(current);
        value["ancestor_instance_ids"] = json!(self.controller_ancestors(&row.definition_id)?);
        Ok(value)
    }
    pub(crate) fn controller_step_approvals(&self, root: &str) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        let mut after = String::new();
        for _ in 0..16 {
            let ids = self.native_workflow_members(root, &after)?;
            for id in &ids {
                let member = WorkflowHeaderFacts::read(self, id)?;
                let mut ancestors = None;
                for e in &member.instance.steps {
                    if !matches!(
                        e.status,
                        StepExecutionStatus::WaitingConfirmation
                            | StepExecutionStatus::AwaitingAcceptance
                    ) {
                        continue;
                    }
                    let resources = member.step(self, &e.id)?;
                    let readiness = self.native_step_readiness(&member, &resources)?;
                    let code = readiness["actions"]["accept"]["code"]
                        .as_str()
                        .unwrap_or("");
                    if !["human_confirmation", "completion"].contains(&code) {
                        continue;
                    }
                    let confirmations = one(self.store.as_ref(), "workflow_confirmations", &e.id)?;
                    if confirmations
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(|rows| {
                            rows.iter().any(|r| {
                                r["data"]["input_digest"] == e.input_digest
                                    && r["data"]["outputs"] == json!(e.outputs)
                            })
                        })
                    {
                        continue;
                    }
                    let step = member
                        .instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.node_id == e.node_id)
                        .ok_or_else(|| err("storage", "审批步骤合同缺失"))?;
                    let mut references: std::collections::BTreeSet<_> =
                        e.outputs.values().cloned().collect();
                    let manifest = one(
                        self.store.as_ref(),
                        "governance",
                        &format!("workflow-inputs:{}", e.id),
                    )?;
                    if let Some(raw) = &manifest.value {
                        references.extend(
                            raw["data"]["targets"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|t| t["origin_ref"].as_str())
                                .map(str::to_owned),
                        );
                    }
                    let mut materials = vec![];
                    for id in references {
                        let raw = one(self.store.as_ref(), "governance", &id)?;
                        let Some(raw) = &raw.value else { continue };
                        if raw["type"] != "workflow_output" {
                            continue;
                        }
                        let Some(execution) = raw["data"]["execution_id"].as_str() else {
                            continue;
                        };
                        let owner =
                            one(self.store.as_ref(), "workflow_execution_owner", execution)?;
                        let Some(owner) = owner.value else { continue };
                        let actual =
                            WorkflowHeaderFacts::read(self, &text(&owner, "instance_id")?)?;
                        if actual.instance.root_instance_id != root {
                            continue;
                        }
                        if let Some(source) =
                            actual.instance.steps.iter().find(|s| s.id == execution)
                        {
                            materials.push(json!({"instance_id":actual.record.id,"owner_id":actual.instance.owner_id,"execution_id":source.id,"session_id":source.session_id,"turn_id":source.turn_id,"output_id":id,"label":raw["name"],"purpose":"核对当前审批的输入或产物"}));
                        }
                    }
                    let evidence = json!([{"instance_id":id,"owner_id":member.instance.owner_id,"execution_id":e.id,"session_id":e.session_id,"turn_id":e.turn_id,"label":step.name,"purpose":"核对本轮执行与校验结果"}]);
                    if ancestors.is_none() {
                        ancestors = Some(self.controller_ancestors(id)?);
                    }
                    let owner = crate::work_owners::WorkOwnerFacts::read(
                        self.store.as_ref(),
                        &member.instance.owner_id,
                    )?;
                    result.push(json!({"instance_id":id,"instance_name":member.record.name,"ancestor_instance_ids":ancestors,"step_name":step.name,"human_required":code=="human_confirmation","owner_id":member.instance.owner_id,"revision":member.record.revision,"operation":if code=="completion" {"workflow.step.approve"}else{"workflow.step.confirm"},"arguments":{"plan_revision_id":member.instance.active_plan_revision_id,"control_version":member.instance.control_version,"step":e.step,"execution_id":e.id,"expected_step_revision":resources.step.revision,"input_digest":e.input_digest,"outputs":e.outputs,"accept_result":true},"interaction":{"source_label":format!("{} / {} / {}",owner.owner.value.as_ref().and_then(|r|r["title"].as_str()).unwrap_or(&member.record.name),member.record.name,step.name),"title":format!("确认{}",step.name),"background":format!("{}正在等待人工决定。",step.name),"requested_action":if code=="completion"{"完成条件尚未满足。可填写理由后手动批准当前版本，或退回经理处理。"}else{"核对当前材料后确认，流程将接纳本轮结果并继续。"},"completed_work":"已执行当前步骤，材料与执行依据见下方。","review_summary":if code=="completion"{readiness["actions"]["accept"]["reason"].as_str().unwrap_or("")}else{"步骤完成条件已满足，等待人工确认。"},"materials":materials,"evidence":evidence}}));
                    if result.len() >= 128 {
                        return Ok(result);
                    }
                }
            }
            if ids.len() < 16 {
                return Ok(result);
            }
            after = ids.last().unwrap().clone();
        }
        Ok(result)
    }
    pub(crate) fn controller_human_gates(&self, root: &str) -> Result<Value> {
        let gates = self
            .controller_step_approvals(root)?
            .into_iter()
            .filter(|a| a["human_required"] == true)
            .collect::<Vec<_>>();
        Ok(
            json!({"total":gates.len(),"items":gates.iter().take(16).map(|a|json!({"instance_id":a["instance_id"],"instance_name":a["instance_name"],"owner_id":a["owner_id"],"step":a["arguments"]["step"],"step_name":a["step_name"],"execution_id":a["arguments"]["execution_id"],"plan_revision_id":a["arguments"]["plan_revision_id"],"title":a["interaction"]["title"],"confirmation_policy":"human","human_satisfied":false})).collect::<Vec<_>>()}),
        )
    }
    pub(crate) fn validate_controller_interaction(
        &self,
        commit: &mut WorkflowCommit<'_>,
        instance: &WorkflowHeaderFacts,
        value: &Value,
        arguments: &Value,
    ) -> Result<Value> {
        require(
            value.is_object() && value.to_string().len() <= 32768,
            "需要有界的人类交互说明",
        )?;
        for key in [
            "title",
            "background",
            "requested_action",
            "completed_work",
            "review_summary",
        ] {
            require(
                !text(value, key)?.trim().is_empty(),
                format!("人类交互缺少 {key}"),
            )?;
        }
        let materials = value["materials"]
            .as_array()
            .ok_or_else(|| err("invalid_params", "需要相关材料清单"))?;
        let evidence = value["evidence"]
            .as_array()
            .ok_or_else(|| err("invalid_params", "需要执行依据清单"))?;
        require(
            materials.len() <= 32 && evidence.len() <= 32,
            "交互引用过多",
        )?;
        let mut normalized = value.clone();
        for field in ["materials", "evidence"] {
            for reference in normalized[field].as_array_mut().unwrap() {
                text(reference, "label")?;
                text(reference, "purpose")?;
                let owner = WorkflowHeaderFacts::read(self, &text(reference, "instance_id")?)?;
                require(
                    owner.instance.root_instance_id == instance.instance.root_instance_id,
                    "交互依据不属于当前根实例",
                )?;
                let resource = owner.step(self, &text(reference, "execution_id")?)?;
                let e = &resource.execution;
                commit.witness(resource.reads.versions().iter().cloned())?;
                if let Some(id) = reference["output_id"].as_str() {
                    let selected = one(self.store.as_ref(), "governance", id)?;
                    let output = restored(self, &selected)?;
                    require(
                        output.r#type == "workflow_output"
                            && output.data["execution_id"] == e.id
                            && output.data["input_digest"] == e.input_digest,
                        "材料与执行版本不一致",
                    )?;
                    commit.witness([version(&selected)])?;
                } else {
                    require(field == "evidence", "材料缺少确切产物引用")?;
                }
                for (key, actual) in [
                    ("session_id", json!(e.session_id)),
                    ("turn_id", json!(e.turn_id)),
                    ("owner_id", json!(owner.instance.owner_id)),
                ] {
                    if !reference[key].is_null() {
                        require(reference[key] == actual, "交互依据的会话或归属不一致")?;
                    }
                    reference[key] = actual;
                }
            }
        }
        if let Some(id) = arguments["execution_id"].as_str() {
            require(
                materials
                    .iter()
                    .chain(evidence)
                    .any(|r| r["instance_id"] == instance.record.id && r["execution_id"] == id),
                "交互依据未包含当前审批执行",
            )?;
        }
        Ok(normalized)
    }
}
