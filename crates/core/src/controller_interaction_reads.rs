//! Human-facing projections from exact workflow resources, never an incomplete
//! EngineeringState. Display witnesses do not replace command authorization.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
use std::collections::BTreeMap;

impl Engine {
    pub(crate) fn controller_workspace_view(&self, p: &Value) -> Result<Value> {
        use crate::read_models::{ControllerQuery, fields, identifier};
        let current = match p.get("view").and_then(Value::as_str) {
            None if p.get("view").is_none() => false,
            Some("current") => true,
            Some("history") => {
                fields(
                    p,
                    &["project_id", "view", "root_instance_id", "decision_before"],
                )?;
                let root = identifier(p, "root_instance_id")?;
                let before = p
                    .get("decision_before")
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                            .ok_or_else(|| err("invalid_params", "无效的历史决定游标"))
                    })
                    .transpose()?;
                return self.store.orchestration_history(&root, before);
            }
            Some("decision") => {
                fields(p, &["project_id", "view", "root_instance_id", "id"])?;
                let root = identifier(p, "root_instance_id")?;
                let id = identifier(p, "id")?;
                require(
                    self.store.orchestration_history_member(&root, &id)?,
                    "历史决定不属于当前流程",
                )?;
                let _body = self.store.observe_query("orchestration.history_body");
                let selected = one(self.store.as_ref(), "governance", &id)?;
                let row = restored(self, &selected)?;
                require(
                    row.r#type == "workflow_pending_decision" && row.data["status"] != "pending",
                    "历史决定已变化",
                )?;
                return self.controller_decision_view(&row);
            }
            _ => return Err(err("invalid_params", "无效的经理协作视图")),
        };
        let mut selector = p.clone();
        if current && let Some(fields) = selector.as_object_mut() {
            fields.remove("view");
        }
        let mut value = json!(
            self.store
                .orchestration_summary(&ControllerQuery::parse(&selector)?)?
        );
        // The indexed summary selects pending decisions across the whole root
        // before pagination. Do not replace it with a tail of resolved history.
        let mut decisions = value["decisions"].as_array().cloned().unwrap_or_default();
        if !current && p.get("decision_before").is_none() {
            let root = p["root_instance_id"]
                .as_str()
                .or_else(|| value["record"]["data"]["root_instance_id"].as_str());
            if let Some(root) = root {
                let recent = one(self.store.as_ref(), "workflow_root_decisions", root)?;
                decisions.extend(
                    recent
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|r| r["status"] != "pending")
                        .take(10)
                        .cloned(),
                );
            }
        }
        let mut full = Vec::new();
        for decision in decisions {
            let _body = (decision["status"] != "pending"
                && decision["data"]["status"] != "pending")
                .then(|| self.store.observe_query("orchestration.history_body"));
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
        let driver = one(self.store.as_ref(), "workflow_driver_root", root)?;
        if driver
            .value
            .as_ref()
            .is_some_and(|v| v["data"]["mode"] == "agent")
        {
            return Ok(result);
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
        let request = &row.data["request"];
        let execution = request["arguments"]["execution_id"].as_str();
        let context = self
            .store
            .workflow_decision_context(&row.definition_id, execution)?;
        let run = one(
            self.store.as_ref(),
            "workflow_run_authority",
            &row.definition_id,
        )?;
        let current = run
            .value
            .as_ref()
            .is_some_and(|r| r["cancel_requested"] != true && r["status"] != "cancelled")
            && context.active_plan_revision_id == row.data["plan_revision_id"]
            && (execution.is_none()
                || context.execution.as_ref().is_some_and(|e| {
                    if !matches!(
                        request["operation"].as_str(),
                        Some(
                            "workflow.step.confirm"
                                | "workflow.step.accept"
                                | "workflow.step.approve"
                        )
                    ) {
                        return (request["operation"] != "workflow.step.input"
                            || e.status == StepExecutionStatus::WaitingInput)
                            && request["arguments"]["expected_step_revision"] == e.revision
                            && request["arguments"]["control_version"] == context.control_version;
                    }
                    json!(e.input_digest) == request["arguments"]["input_digest"]
                        && json!(e.outputs) == request["arguments"]["outputs"]
                        && matches!(
                            e.status,
                            StepExecutionStatus::WaitingConfirmation
                                | StepExecutionStatus::AwaitingAcceptance
                        )
                }));
        let mut value = json!(row);
        value["current"] = json!(current);
        value["ancestor_instance_ids"] = json!(context.ancestor_instance_ids);
        if let Some(id) = row.data["decision_ref"].as_str() {
            let selected = one(self.store.as_ref(), "governance", id)?;
            if let Some(signed) = selected
                .value
                .as_ref()
                .filter(|r| r["type"] == "workflow_human_decision" && r["immutable"] == true)
            {
                value["signed_reason"] = signed["data"]["reason"].clone();
            }
        }
        if request["operation"] == "workflow.step.approve" {
            let root = WorkflowHeaderFacts::read(self, &row.definition_id)?;
            value["rework_budgets"] = json!(
                root.instance
                    .plan
                    .document
                    .rework_policies
                    .keys()
                    .map(|key| self.rework_budget_view(&root, key).map(|(_, v)| v))
                    .collect::<Result<Vec<_>>>()?
            );
            value["data"]["request"]["interaction"]["source_label"] = json!(format!(
                "{} / {}",
                root.record.name,
                request["arguments"]["step"].as_str().unwrap_or("评审")
            ));
        }
        if let Some(root) = run
            .value
            .as_ref()
            .and_then(|r| r["root_instance_id"].as_str())
            && let Some(materials) = value
                .pointer_mut("/data/request/interaction/materials")
                .and_then(Value::as_array_mut)
        {
            for material in materials {
                let generic_label = material["label"]
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.trim().eq_ignore_ascii_case("result"));
                let generic_purpose = material["purpose"]
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty() || s == "核对当前审批的输入或产物");
                if !(generic_label || generic_purpose) {
                    continue;
                }
                let (Some(id), Some(instance)) = (
                    material["output_id"].as_str(),
                    material["instance_id"].as_str(),
                ) else {
                    continue;
                };
                let output = one(self.store.as_ref(), "governance", id)?;
                let Some(raw) = output.value.as_ref().filter(|r| {
                    r["type"] == "workflow_output"
                        && r["data"]["execution_id"] == material["execution_id"]
                }) else {
                    continue;
                };
                let producer = WorkflowHeaderFacts::read(self, instance)?;
                if producer.instance.root_instance_id != root {
                    continue;
                }
                let is_output = request["arguments"]["outputs"]
                    .as_object()
                    .is_some_and(|outputs| outputs.values().any(|output| output == id));
                let purpose = if is_output {
                    "用作本次审批的待确认产物"
                } else {
                    "供本次审批核对"
                };
                if let Some(description) =
                    self.describe_approval_material(&producer, id, raw, purpose)?
                {
                    if generic_label {
                        material["label"] = description["label"].clone();
                    }
                    if generic_purpose {
                        material["purpose"] = description["purpose"].clone();
                    }
                }
            }
        }
        Ok(value)
    }

    fn describe_approval_material(
        &self,
        producer: &WorkflowHeaderFacts,
        id: &str,
        output: &Value,
        purpose: &str,
    ) -> Result<Option<Value>> {
        let Some(source) = producer
            .instance
            .steps
            .iter()
            .find(|e| output["data"]["execution_id"] == e.id)
        else {
            return Ok(None);
        };
        let plan = &producer.instance.plan;
        let delivery_ref = output["data"]["delivery_ref"].as_str().unwrap_or_default();
        let frozen_delivery = plan.definitions.get(delivery_ref);
        let same_workflow = source.workflow_revision_id == plan.workflow_revision_id;
        let catalog = if frozen_delivery.is_none() || !same_workflow {
            Some(self.store.configuration_snapshot()?)
        } else {
            None
        };
        let delivery =
            frozen_delivery.or_else(|| catalog.as_ref()?.state.configuration(delivery_ref).ok());
        let step_name = if same_workflow {
            plan.document
                .steps
                .iter()
                .find(|step| step.key == source.step)
                .map(|step| step.name.as_str())
        } else {
            catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog
                        .state
                        .configuration(&source.workflow_revision_id)
                        .ok()
                })
                .and_then(|workflow| workflow.data["steps"].as_array())
                .and_then(|steps| steps.iter().find(|step| step["key"] == source.step))
                .and_then(|step| step["name"].as_str())
        }
        .unwrap_or(&source.step);
        let label = delivery
            .filter(|delivery| {
                delivery.r#type == "delivery_type" && !delivery.name.trim().is_empty()
            })
            .map(|delivery| delivery.name.clone())
            .unwrap_or_else(|| format!("{step_name}产物"));
        let purpose = format!("由「{step_name}」第 {} 轮产出，{purpose}。", source.round);
        Ok(Some(
            json!({"instance_id":producer.record.id,"owner_id":producer.instance.owner_id,"execution_id":source.id,"session_id":source.session_id,"turn_id":source.turn_id,"output_id":id,"label":label,"purpose":purpose}),
        ))
    }

    pub(crate) fn controller_approval_materials(
        &self,
        member: &WorkflowHeaderFacts,
        execution: &StepExecution,
        include_inputs: bool,
    ) -> Result<Vec<Value>> {
        let mut references = execution
            .outputs
            .values()
            .map(|id| (id.clone(), true))
            .collect::<BTreeMap<_, _>>();
        if include_inputs {
            let manifest = one(
                self.store.as_ref(),
                "governance",
                &format!("workflow-inputs:{}", execution.id),
            )?;
            if let Some(manifest) = manifest.value {
                for input in manifest["data"]["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t["origin_ref"].as_str())
                {
                    references.entry(input.to_owned()).or_insert(false);
                }
            }
        }
        let mut materials = vec![];
        for (id, is_output) in references {
            let output = one(self.store.as_ref(), "governance", &id)?;
            let Some(raw) = output
                .value
                .as_ref()
                .filter(|r| r["type"] == "workflow_output")
            else {
                continue;
            };
            let Some(source) = raw["data"]["execution_id"].as_str() else {
                continue;
            };
            let other;
            let producer = if member.instance.steps.iter().any(|e| e.id == source) {
                member
            } else {
                let owner = one(self.store.as_ref(), "workflow_execution_owner", source)?;
                let Some(owner) = owner.value else { continue };
                other = WorkflowHeaderFacts::read(self, &text(&owner, "instance_id")?)?;
                if other.instance.root_instance_id != member.instance.root_instance_id {
                    continue;
                }
                &other
            };
            let purpose = if is_output {
                "用作本次审批的待确认产物"
            } else {
                "用作本次审批的输入材料"
            };
            if let Some(material) = self.describe_approval_material(producer, &id, raw, purpose)? {
                materials.push(material);
            }
        }
        Ok(materials)
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
                    let recovery = &readiness["recovery"];
                    if !recovery.is_null()
                        && !(recovery["kind"] == "review_rework"
                            && recovery["status"] == "exhausted"
                            && recovery["allow_human_approval"] == true)
                    {
                        continue;
                    }
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
                    let materials = self.controller_approval_materials(&member, e, true)?;
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
            json!({"total":gates.len(),"items":gates.iter().take(16).map(|a| {
                let mut arguments = a["arguments"].clone();
                arguments["instance_id"] = a["instance_id"].clone();
                arguments["expected_revision"] = a["revision"].clone();
                json!({"instance_id":a["instance_id"],"instance_name":a["instance_name"],"owner_id":a["owner_id"],"step":arguments["step"],"step_name":a["step_name"],"execution_id":arguments["execution_id"],"plan_revision_id":arguments["plan_revision_id"],"title":a["interaction"]["title"],"confirmation_policy":"human","human_satisfied":false,"operation":a["operation"],"arguments":arguments,"interaction":a["interaction"]})
            }).collect::<Vec<_>>()}),
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
