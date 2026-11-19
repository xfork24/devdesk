//! Read-only platform tools use exact source contracts and selected records.
use crate::{
    concurrency::*,
    platform_facts::{one, restored},
    workflow_resources::{WorkflowResources, version},
    *,
};

pub(crate) fn input_evidence_from_manifest(
    execution: &StepExecution,
    manifest: Option<&ConfigurationRecord>,
) -> Result<Vec<EvidenceTarget>> {
    let Some(manifest) = manifest else {
        return Ok(vec![]);
    };
    require(
        manifest.r#type == "workflow_input_manifest"
            && manifest.immutable
            && manifest.definition_id == execution.id
            && manifest.data["input_digest"] == execution.input_digest,
        "冻结证据与原输入不符",
    )?;
    let targets = manifest.data["targets"]
        .as_array()
        .ok_or_else(|| err("storage", "冻结证据缺少目标集合"))?;
    require(targets.len() <= WORKFLOW_STEP_LIMIT, "冻结证据目标超出预算")?;
    targets
        .iter()
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
impl Engine {
    pub(crate) fn native_platform_lint(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
    ) -> Result<(Value, ReadSet)> {
        let instance = &resources.instance;
        let execution = &resources.execution;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.node_id == execution.node_id)
            .ok_or_else(|| err("storage", "检查步骤合同缺失"))?;
        require(
            request.arguments.as_object().is_some_and(|p| {
                p.keys()
                    .all(|k| ["delivery_ref", "value"].contains(&k.as_str()))
            }),
            "业务步骤只能检查冻结交付类型与草稿",
        )?;
        let reference = text(&request.arguments, "delivery_ref")?;
        let mut reads = resources.reads.clone();
        let mut authorized = step.outputs.values().any(|p| p.delivery_ref == reference);
        let arguments = match &step.executor {
            StepExecutor::Tool { arguments, .. } => Some(arguments),
            _ => None,
        };
        for binding in step
            .inputs
            .values()
            .chain(arguments.into_iter().flat_map(|a| a.values()))
        {
            let delivery = match binding {
                InputBinding::Source { source } if source.step == "$input" => instance
                    .plan
                    .document
                    .inputs
                    .get(&source.port)
                    .map(|p| p.delivery_ref.clone()),
                InputBinding::Source { source } => instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == source.step)
                    .and_then(|s| s.outputs.get(&source.port))
                    .map(|p| p.delivery_ref.clone()),
                InputBinding::Material { material_ref } => {
                    let selected = one(self.store.as_ref(), "governance", material_ref)?;
                    let row = restored(self, &selected)?;
                    require(
                        row.r#type == "typed_material"
                            && row.immutable
                            && row.data["project_id"] == instance.project_id
                            && row.data["root_instance_id"] == instance.root_instance_id,
                        "材料不属于原冻结工作",
                    )?;
                    reads = reads.merge([version(&selected)])?;
                    opt(&row.data, "delivery_ref")
                }
                _ => None,
            };
            authorized |= delivery.as_deref() == Some(reference.as_str());
        }
        require(authorized, "只能检查当前步骤声明的输入或输出 Delivery")?;
        let delivery = instance
            .plan
            .definitions
            .get(&reference)
            .ok_or_else(|| err("tool_not_authorized", "交付类型不在原冻结闭包"))?;
        let value = request
            .arguments
            .get("value")
            .ok_or_else(|| err("invalid_params", "缺少待检查内容"))?;
        let catalog = self.store.configuration_snapshot()?;
        let mut report = self.lint_delivery_value(&catalog.state, delivery, value)?;
        if step
            .outputs
            .values()
            .any(|p| p.delivery_ref == reference && p.content_checks)
        {
            let selected = one(
                self.store.as_ref(),
                "governance",
                &format!("workflow-inputs:{}", execution.id),
            )?;
            let manifest = selected
                .value
                .as_ref()
                .map(|_| restored(self, &selected))
                .transpose()?;
            reads = reads.merge([version(&selected)])?;
            let targets = input_evidence_from_manifest(execution, manifest.as_ref())?;
            report["content_report_required_checks"] =
                json!(crate::spec_lint::step_content_checks(instance, step)?);
            report["content_report_instructions"] = json!(
                "Call spec.lint with exactly {delivery_ref, value}; delivery_ref is this exact Delivery revision and value is the complete report. Report checks must match content_report_required_checks, not the Delivery's generic checks above. Correct the draft in this same Turn; do not substitute a port name or add spec_ref/output_port arguments."
            );
            report["content_report_sources"] = json!(
                targets
                    .iter()
                    .map(EvidenceTarget::location_guidance)
                    .collect::<Vec<_>>()
            );
            let validation = serde_json::from_value::<ContentCheckReport>(value.clone())
                .map_err(|e| err("output_invalid", e.to_string()))
                .and_then(|r| {
                    r.validate_evidence(
                        &crate::spec_lint::step_content_checks(instance, step)?,
                        &targets,
                    )
                });
            report["content_report_valid"] = json!(validation.is_ok());
            report["content_report_error"] = json!(validation.err());
        }
        Ok((report, reads))
    }
    pub(crate) fn native_platform_outputs(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
    ) -> Result<(Value, ReadSet)> {
        let p = &request.arguments;
        let pointer = p["pointer"].as_str().unwrap_or("");
        let ids: Vec<Id> = serde_json::from_value(p["ids"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            !ids.is_empty() && ids.len() <= 100,
            "一次选择 1 至 100 个原始产物",
        )?;
        let execution = &resources.execution;
        let instance = &resources.instance;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原读取步骤合同缺失"))?;
        let mut reads = resources.reads.clone();
        let mut outputs = vec![];
        let budget =
            p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) as usize / ids.len();
        for id in ids {
            let authorized = (pointer.is_empty()
                && execution
                    .rework
                    .as_ref()
                    .is_some_and(|r| r.outputs.values().any(|o| o == &id)))
                || (pointer.is_empty()
                    && execution.rework_workspace.as_ref().is_some_and(|c| {
                        c.output_id == id
                            && step
                                .access
                                .workspace_input
                                .as_ref()
                                .is_some_and(|n| execution.inputs.get(n) == Some(&c.workspace))
                    }))
                || step.inputs.values().any(|b| match b {
                    InputBinding::Source { source } if source.step != "$input" => {
                        source.pointer == pointer
                            && instance
                                .steps
                                .iter()
                                .find(|e| e.step == source.step)
                                .and_then(|e| e.outputs.get(&source.port))
                                == Some(&id)
                    }
                    InputBinding::Material { material_ref } => {
                        pointer.is_empty() && material_ref == &id
                    }
                    _ => false,
                });
            let supplemental = if authorized {
                None
            } else {
                self.supplemental_material_read(instance, execution, &id, pointer)?
            };
            require(
                authorized || supplemental.is_some(),
                "只能读取当前步骤显式输入、已转交的平台材料及原返工产物的声明字段",
            )?;
            if let Some(supplemental) = supplemental {
                reads = reads.merge(supplemental.versions().iter().cloned())?;
            }
            let selected = one(self.store.as_ref(), "governance", &id)?;
            let row = restored(self, &selected)?;
            require(
                row.immutable
                    && row.scope.project_id() == Some(request.project_id.as_str())
                    && matches!(row.r#type.as_str(), "workflow_output" | "typed_material")
                    && (row.r#type != "typed_material"
                        || row.data["root_instance_id"] == instance.root_instance_id),
                "产物不属于原项目与流程根",
            )?;
            reads = reads.merge([version(&selected)])?;
            let value = row.data["value"]
                .pointer(pointer)
                .ok_or_else(|| err("invalid_params", "产物字段不存在"))?;
            let body = value.to_string();
            let mut offset = (p["offset"].as_u64().unwrap_or(0) as usize).min(body.len());
            while !body.is_char_boundary(offset) {
                offset -= 1;
            }
            if body.len() > budget || offset > 0 {
                let mut end = (offset + budget).min(body.len());
                while !body.is_char_boundary(end) {
                    end -= 1;
                }
                let mut data = row.data.clone();
                data.as_object_mut().unwrap().remove("value");
                outputs.push(json!({"id":row.id,"value_digest":configuration_hash(value),"data":data,"value_chunk":&body[offset..end],"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"total_bytes":body.len()}));
            } else {
                let mut out = json!(row);
                out["data"]["value"] = value.clone();
                out["value_digest"] = json!(configuration_hash(value));
                outputs.push(out);
            }
        }
        Ok((json!({"outputs":outputs}), reads))
    }
}
