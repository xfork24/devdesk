//! A single structural checker for authoring previews and accepted immutable values.
use crate::governance::record;
use crate::*;
pub(crate) fn step_content_checks(
    instance: &ConfiguredWorkflowInstance,
    step: &StepDefinition,
) -> Result<Vec<ContentCheckDefinition>> {
    step.spec_refs
        .iter()
        .map(|reference| {
            instance
                .plan
                .definitions
                .get(reference)
                .ok_or_else(|| err("configuration_required", "步骤 Spec 不在冻结闭包中"))?
                .decode::<ConfiguredSpecification>()
                .map(|s| s.checks)
        })
        .collect::<Result<Vec<_>>>()
        .map(|checks| checks.into_iter().flatten().collect())
}
impl Engine {
    pub(crate) fn validate_content_report(
        &self,
        state: &EngineeringState,
        instance: &ConfiguredWorkflowInstance,
        step: &StepDefinition,
        execution: &StepExecution,
        value: &Value,
    ) -> Result<()> {
        serde_json::from_value::<ContentCheckReport>(value.clone())
            .map_err(|e| err("output_invalid", e.to_string()))?
            .validate_evidence(
                &step_content_checks(instance, step)?,
                &self.input_evidence_targets(state, execution)?,
            )
    }

    pub(crate) fn lint_workflow_value(
        &self,
        state: &EngineeringState,
        request: &PlatformToolRequest,
    ) -> Result<Value> {
        let instance: ConfiguredWorkflowInstance =
            state.configuration(&request.instance_id)?.decode()?;
        require(
            instance.project_id == request.project_id,
            "检查请求不属于当前项目",
        )?;
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == request.step_execution_id())
            .ok_or_else(|| err("tool_not_authorized", "检查步骤已失效"))?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.node_id == execution.node_id)
            .ok_or_else(|| err("storage", "检查步骤合同不存在"))?;
        require(
            request.arguments.as_object().is_some_and(|p| {
                p.keys()
                    .all(|key| ["delivery_ref", "value"].contains(&key.as_str()))
            }),
            "业务步骤只能检查冻结交付类型与草稿，不能检查任意配置草稿",
        )?;
        let reference = text(&request.arguments, "delivery_ref")?;
        let mut authorized = step
            .outputs
            .values()
            .any(|port| port.delivery_ref == reference);
        let arguments = match &step.executor {
            StepExecutor::Tool { arguments, .. } => Some(arguments),
            _ => None,
        };
        for binding in step
            .inputs
            .values()
            .chain(arguments.into_iter().flat_map(|map| map.values()))
        {
            let delivery = match binding {
                InputBinding::Source { source } if source.step == "$input" => instance
                    .plan
                    .document
                    .inputs
                    .get(&source.port)
                    .map(|p| p.delivery_ref.as_str()),
                InputBinding::Source { source } => instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == source.step)
                    .and_then(|s| s.outputs.get(&source.port))
                    .map(|p| p.delivery_ref.as_str()),
                InputBinding::Material { material_ref } => {
                    let material = state.configuration(material_ref)?;
                    require(
                        material.r#type == "typed_material"
                            && material.data["project_id"] == instance.project_id
                            && material.data["root_instance_id"] == instance.root_instance_id,
                        "材料不属于本次冻结工作",
                    )?;
                    material.data["delivery_ref"].as_str()
                }
                _ => None,
            };
            authorized |= delivery == Some(reference.as_str());
        }
        if !authorized {
            return Err(err(
                "tool_not_authorized",
                "只能检查当前步骤声明的输入或输出 Delivery",
            ));
        }
        let delivery = instance
            .plan
            .definitions
            .get(&reference)
            .ok_or_else(|| err("tool_not_authorized", "交付类型不在冻结闭包中"))?;
        let value = request
            .arguments
            .get("value")
            .ok_or_else(|| err("invalid_params", "缺少待检查内容"))?;
        let mut report = self.lint_delivery_value(state, delivery, value)?;
        if step
            .outputs
            .values()
            .any(|port| port.delivery_ref == reference && port.content_checks)
        {
            let validation = self.validate_content_report(state, &instance, step, execution, value);
            report["content_report_valid"] = json!(validation.is_ok());
            report["content_report_error"] = json!(validation.err());
        }
        Ok(report)
    }
    pub fn lint_specification_value(
        &self,
        spec: &ConfiguredSpecification,
        reference: &str,
        value: &Value,
    ) -> Result<Value> {
        spec.validate()?;
        require(
            serde_json::to_vec(value)
                .map_err(|e| err("invalid_value", e.to_string()))?
                .len()
                <= SPEC_VALUE_LIMIT,
            "检查内容超过 8 MiB",
        )?;
        let mut checks = Vec::new();
        let mut valid = true;
        for check in &spec.checks {
            let passed = check
                .assertion
                .as_ref()
                .map(|assertion| assertion.evaluate(value))
                .transpose()?;
            valid &= passed != Some(false);
            checks.push(json!({"spec_ref":reference,"key":check.key,"name":check.name,"kind":if check.assertion.is_some(){"machine"}else{"semantic"},"passed":passed,"evidence_required":check.evidence_required,"status":if passed.is_none(){"requires_judgment"}else if passed==Some(true){"passed"}else{"failed"}}));
        }
        Ok(
            json!({"checker":SPEC_CHECKER_ID,"value_digest":configuration_hash(value),"structure_valid":valid,"semantic_quality":"requires_judgment","checks":checks}),
        )
    }
    pub fn lint_delivery_value(
        &self,
        s: &EngineeringState,
        delivery: &ConfigurationRecord,
        value: &Value,
    ) -> Result<Value> {
        require(delivery.r#type == "delivery_type", "需要精确交付类型")?;
        let definition: DeliveryTypeDefinition = delivery.decode()?;
        s.validate_delivery_specs(&delivery.refs, &definition, &delivery.scope)?;
        let structural = self.validate_configured_value(&definition, value);
        let mut checks = Vec::new();
        let mut valid = structural.is_ok();
        let references = reference_list(&delivery.refs, "spec_refs")?;
        require(references.len() <= 64, "一次最多关联 64 份规范")?;
        let mut machine_checks = 0usize;
        let value_bytes = serde_json::to_vec(value)
            .map_err(|e| err("invalid_value", e.to_string()))?
            .len();
        for reference in references {
            let source = s.configuration(&reference)?;
            let spec: ConfiguredSpecification = source.decode()?;
            machine_checks += spec.checks.iter().filter(|c| c.assertion.is_some()).count();
            require(
                machine_checks <= 128
                    && machine_checks.saturating_mul(value_bytes) <= 32 * 1024 * 1024,
                "结构检查工作量超出边界",
            )?;
            let result = self.lint_specification_value(&spec, &reference, value)?;
            valid &= result["structure_valid"] == true;
            checks.extend(result["checks"].as_array().into_iter().flatten().cloned());
        }
        Ok(
            json!({"checker":SPEC_CHECKER_ID,"delivery_ref":delivery.id,"spec_refs":reference_list(&delivery.refs,"spec_refs")?,"value_digest":configuration_hash(value),"structure_valid":valid,"semantic_quality":"requires_judgment","checks":checks,"format_schema_error":structural.err()}),
        )
    }
    pub(crate) fn enforce_delivery_value(
        &self,
        s: &EngineeringState,
        delivery: &ConfigurationRecord,
        value: &Value,
    ) -> Result<Value> {
        let report = self.lint_delivery_value(s, delivery, value)?;
        require(
            report["structure_valid"] == true,
            format!("规范结构检查未通过: {report}"),
        )?;
        Ok(report)
    }
    pub fn specification_lint(&self, p: &Value) -> Result<Value> {
        let value = p
            .get("value")
            .ok_or_else(|| err("invalid_params", "需要待检查内容"))?;
        if let Some(reference) = p["delivery_ref"].as_str() {
            let s = self.store.snapshot()?;
            return self.lint_delivery_value(&s, s.configuration(reference)?, value);
        }
        if let Some(delivery) = p.get("delivery") {
            let source: SourceDefinition = serde_json::from_value(delivery.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            let record = source.into_record()?;
            let state = self.store.snapshot()?;
            return self.lint_delivery_value(&state, &record, value);
        }
        let spec: ConfiguredSpecification = serde_json::from_value(p["specification"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        self.lint_specification_value(&spec, "unpublished-draft", value)
    }
    pub(crate) fn freeze_input_evidence(
        &self,
        s: &mut EngineeringState,
        row: &ConfigurationRecord,
        instance: &ConfiguredWorkflowInstance,
        step: &StepDefinition,
        execution: &StepExecution,
    ) -> Result<()> {
        let id = format!("workflow-inputs:{}", execution.id);
        if s.governance.iter().any(|r| r.id == id) {
            return Ok(());
        }
        let targets:Vec<_>=execution.inputs.iter().map(|(name,value)|{
            let origin=match step.inputs.get(name){Some(InputBinding::Source{source})=>instance.steps.iter().find(|e|e.step==source.step).and_then(|e|e.outputs.get(&source.port)).cloned(),Some(InputBinding::Material{material_ref})=>Some(material_ref.clone()),_=>None};
            let artifact=origin.as_deref().and_then(|r|s.configuration(r).ok()).and_then(|r|r.data["artifact_id"].as_str());
            json!({"input":name,"source_ref":format!("execution-input:{}:{}",execution.id,name),"origin_ref":origin,"artifact_ref":artifact,"value_digest":configuration_hash(value),"binding":step.inputs.get(name)})
        }).collect();
        let mut manifest = record(
            "workflow_input_manifest",
            execution.id.clone(),
            row.scope.clone(),
            "冻结输入证据".into(),
            1,
            None,
            json!({"execution_id":execution.id,"input_digest":execution.input_digest,"targets":targets}),
            true,
        );
        manifest.id = id;
        s.governance.push(manifest);
        Ok(())
    }
    pub(crate) fn input_evidence_targets(
        &self,
        s: &EngineeringState,
        execution: &StepExecution,
    ) -> Result<Vec<EvidenceTarget>> {
        let Some(manifest) = s
            .governance
            .iter()
            .find(|r| r.id == format!("workflow-inputs:{}", execution.id))
        else {
            return Ok(vec![]);
        };
        require(
            manifest.data["input_digest"] == execution.input_digest,
            "冻结证据与当前输入不匹配",
        )?;
        manifest.data["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|target| {
                let value = execution
                    .inputs
                    .get(target["input"].as_str().unwrap_or(""))
                    .ok_or_else(|| err("evidence_scope", "冻结输入不存在"))?;
                require(
                    target["value_digest"] == configuration_hash(value),
                    "冻结输入内容摘要变化",
                )?;
                Ok(EvidenceTarget {
                    source_ref: text(target, "source_ref")?,
                    value_digest: configuration_hash(value),
                    value: value.clone(),
                })
            })
            .collect()
    }
}
