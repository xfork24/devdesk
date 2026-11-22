//! Output acceptance validates literal producer facts, prepares named files,
//! and seals them outside SQL under the original finite admission plan.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    platform_reads::input_evidence_from_manifest,
    workflow_commits::WorkflowCommit,
    workflow_resources::{WorkflowResources, version},
    *,
};

pub(crate) enum WorkflowOutputProof {
    Agent {
        runtime: Vec<Value>,
    },
    Tool {
        call: Box<ConfigurationRecord>,
        receipt: Value,
    },
    Children {
        ids: Vec<Id>,
        outputs: serde_json::Map<String, Value>,
    },
}

fn validate_checkpoint_inputs(
    plan: &ConfiguredWorkflowPlan,
    producer: &StepDefinition,
    outputs: &serde_json::Map<String, Value>,
) -> Result<()> {
    for consumer in &plan.document.steps {
        let StepExecutor::Tool {
            tool_ref,
            arguments,
            ..
        } = &consumer.executor
        else {
            continue;
        };
        let Some(InputBinding::Source { source }) = arguments.get("interfaces") else {
            continue;
        };
        if source.step != producer.key
            || !plan.definitions.get(tool_ref).is_some_and(|tool| {
                tool.data["implementation"] == "platform"
                    && tool.data["qualified_name"] == "workspace.checkpoint"
            })
        {
            continue;
        }
        let Some(output) = outputs.get(&source.port) else {
            continue;
        };
        let interfaces = output.pointer(&source.pointer).ok_or_else(|| {
            err(
                "output_invalid",
                format!(
                    "步骤「{}」需要产物 {}{} 中的接口清单",
                    consumer.name, source.port, source.pointer
                ),
            )
        })?;
        TestInterface::parse_list(interfaces).map_err(|error| {
            err(
                "output_invalid",
                format!(
                    "步骤「{}」无法使用这份接口清单。{}",
                    consumer.name, error.message
                ),
            )
        })?;
    }
    Ok(())
}

impl Engine {
    pub(crate) fn prepare_native_outputs(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        next: &mut StepExecution,
        result: &Value,
        proof: &WorkflowOutputProof,
        finished: u64,
    ) -> Result<Vec<(ArtifactSeal, String)>> {
        let summary = result["summary"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| err("output_invalid", "执行结论缺少实际摘要"))?;
        match result["status"].as_str() {
            Some("needs_manager") => {
                require(
                    matches!(proof, WorkflowOutputProof::Agent { .. })
                        && next.runtime_context["technical_recovery"] == true,
                    "经理恢复需要原 Agent 的冻结恢复协议",
                )?;
                let diagnosis = text(&result["recovery"], "diagnosis")?;
                let attempts = result["recovery"]["attempted_actions"]
                    .as_array()
                    .ok_or_else(|| err("output_invalid", "缺少已尝试的诊断动作"))?;
                require(
                    summary.chars().count() <= 4096
                        && diagnosis.chars().count() <= 2048
                        && (1..=8).contains(&attempts.len())
                        && attempts.iter().all(|action| {
                            action
                                .as_str()
                                .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 512)
                        }),
                    "Agent 恢复诊断超出预算",
                )?;
                let attempted = attempts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("；");
                next.status = StepExecutionStatus::Failed;
                next.reason = format!("{summary}\n诊断：{diagnosis}\n已尝试：{attempted}");
                next.finished = Some(finished);
                next.failure = Some(WorkflowFailure {
                    code: "needs_manager".into(),
                    message: next.reason.clone(),
                    source_ref: next.turn_id.clone().unwrap_or_else(|| next.id.clone()),
                    kind: ExecutionFailureKind::Business,
                    effect: ExecutionFailureEffect::Applied,
                });
                return Ok(vec![]);
            }
            Some("needs_input") => {
                require(
                    matches!(proof, WorkflowOutputProof::Agent { .. }),
                    "平台或子流程不能制造 Agent 澄清结论",
                )?;
                next.status = StepExecutionStatus::WaitingInput;
                next.reason = summary.into();
                next.budget_wait_started = Some(finished);
                let active = one(self.store.as_ref(), "workflow_active_wait", &next.id)?;
                require(active.value.is_none(), "原执行已有未结束等待区间")?;
                commit.witness([version(&active)])?;
                commit.new_record(record("workflow_wait_interval",next.id.clone(),resources.record.scope.clone(),"等待实际人类输入".into(),1,None,json!({"started":finished,"finished":null,"turn_id":next.turn_id,"reason":"waiting_input"}),false),"workflow-wait-interval")?;
                return Ok(vec![]);
            }
            Some("blocked") => {
                next.status = StepExecutionStatus::Failed;
                next.reason = summary.into();
                next.finished = Some(finished);
                next.failure = Some(WorkflowFailure {
                    code: "agent_blocked".into(),
                    message: summary.into(),
                    source_ref: next.turn_id.clone().unwrap_or_else(|| next.id.clone()),
                    kind: ExecutionFailureKind::Business,
                    effect: ExecutionFailureEffect::Applied,
                });
                return Ok(vec![]);
            }
            Some("completed") => {}
            _ => return Err(err("output_invalid", "无效的实际执行结论")),
        }
        commit.identity = format!(
            "{}:{}",
            commit.command.command_id,
            &configuration_hash(&commit.reads)[..16]
        );
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == next.step)
            .ok_or_else(|| err("storage", "原输出步骤定义缺失"))?;
        let outputs = result["outputs"]
            .as_object()
            .ok_or_else(|| err("output_invalid", "outputs 必须是对象"))?;
        require(
            outputs.len() <= 64 && outputs.keys().all(|n| step.outputs.contains_key(n)),
            "输出端口超过预算或未在原步骤声明",
        )?;
        require(
            result.to_string().len() <= SPEC_VALUE_LIMIT,
            "一次执行的原始输出超过内容预算",
        )?;
        if let Some(rework) = &next.rework
            && rework.assessment["kind"] == "test_support"
        {
            for (port, id) in &rework.outputs {
                let original = one(self.store.as_ref(), "governance", id)?;
                let row = restored(self, &original)?;
                let previous = &row.data["value"];
                if row.r#type != "workflow_output" || previous["recipe"].is_null() {
                    continue;
                }
                require(
                    row.immutable && row.definition_id == rework.previous_execution_id,
                    "测试恢复需要原执行冻结的验收声明",
                )?;
                commit.witness([version(&original)])?;
                require(
                    outputs.get(port).is_some_and(|next| {
                        ["cases", "interfaces", "recipe"]
                            .iter()
                            .all(|key| next.get(*key) == previous.get(*key))
                    }),
                    "测试环境修复必须保留原用例、接口和测试配方",
                )?;
            }
        }
        validate_checkpoint_inputs(&resources.instance.plan, step, outputs)?;
        let catalog = self.store.configuration_snapshot()?;
        let manifest = one(
            self.store.as_ref(),
            "governance",
            &format!("workflow-inputs:{}", next.id),
        )?;
        commit.witness([version(&manifest)])?;
        let evidence = manifest
            .value
            .as_ref()
            .map(|_| restored(self, &manifest))
            .transpose()?;
        let mut validated = vec![];
        for (name, port) in &step.outputs {
            if port.required {
                require(outputs.contains_key(name), format!("缺少实际输出 {name}"))?;
            }
            if let Some(value) = outputs.get(name) {
                let delivery = resources
                    .instance
                    .plan
                    .definitions
                    .get(&port.delivery_ref)
                    .ok_or_else(|| err("configuration_required", "原输出类型未冻结"))?;
                let report = self.enforce_delivery_value(&catalog.state, delivery, value)?;
                if port.content_checks {
                    serde_json::from_value::<ContentCheckReport>(value.clone())
                        .map_err(|e| err("output_invalid", e.to_string()))?
                        .validate_evidence(
                            &crate::spec_lint::step_content_checks(&resources.instance, step)?,
                            &input_evidence_from_manifest(next, evidence.as_ref())?,
                        )?;
                }
                let provenance = self.native_output_provenance(
                    resources,
                    step,
                    next,
                    (name, delivery),
                    value,
                    proof,
                )?;
                validated.push((name, port, value, report, provenance));
            }
        }
        let mut files = vec![];
        for (name, port, value, report, provenance) in validated {
            let body = value.to_string();
            let seal = ArtifactSeal::prepare(
                commit.object_id(&format!("workflow-output-artifact:{name}"))?,
                &MaterialOwner::Project {
                    id: resources.instance.project_id.clone(),
                },
                "workflow-outputs",
                &body,
                Origin::External,
            )?;
            let output = record(
                "workflow_output",
                next.id.clone(),
                resources.record.scope.clone(),
                name.clone(),
                1,
                None,
                json!({"execution_id":next.id,"port":name,"delivery_ref":port.delivery_ref,"value":value,"artifact_id":seal.artifact.id,"input_digest":next.input_digest,"provenance":provenance,"spec_validation":report,"input_manifest_ref":format!("workflow-inputs:{}",next.id)}),
                true,
            );
            let id = commit.new_record(output, &format!("workflow-output:{name}"))?;
            next.outputs.insert(name.clone(), id);
            files.push((seal, body));
        }
        self.start_native_workflow_wait(commit, resources, finished, "awaiting_acceptance")?;
        next.finished = Some(finished);
        next.status = if step.confirmation == ConfirmationPolicy::Human {
            StepExecutionStatus::WaitingConfirmation
        } else {
            StepExecutionStatus::AwaitingAcceptance
        };
        next.reason = summary.into();
        Ok(files)
    }
    fn native_output_provenance(
        &self,
        resources: &WorkflowResources,
        step: &StepDefinition,
        execution: &StepExecution,
        (name, delivery): (&str, &ConfigurationRecord),
        value: &Value,
        proof: &WorkflowOutputProof,
    ) -> Result<Value> {
        let definition: DeliveryTypeDefinition = delivery.decode()?;
        let mut out = json!({"value_digest":configuration_hash(value),"spec_refs":reference_list(&delivery.refs,"spec_refs")?});
        match proof {
            WorkflowOutputProof::Agent { runtime } => {
                require(
                    matches!(step.executor, StepExecutor::Agent { .. })
                        && execution.turn_id.is_some()
                        && !matches!(
                            definition.producer,
                            Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
                        ),
                    "Agent 不能冒充平台或子流程产出",
                )?;
                out["kind"] = json!("agent");
                out["turn_id"] = json!(execution.turn_id);
                out["runtime_executions"] = json!(runtime);
            }
            WorkflowOutputProof::Tool { call, receipt } => {
                let StepExecutor::Tool { output_mapping, .. } = &step.executor else {
                    return Err(err("output_invalid", "非工具步骤不能接纳工具回执"));
                };
                let pointer = output_mapping.get(name).map(String::as_str).unwrap_or("");
                require(
                    receipt.pointer(pointer) == Some(value),
                    "输出与原工具回执的确切映射不符",
                )?;
                if call.r#type == "platform_call" {
                    let request: PlatformToolRequest =
                        serde_json::from_value(call.data["request"].clone())
                            .map_err(|e| err("storage", e.to_string()))?;
                    require(
                        call.data["status"] == "succeeded"
                            && request.execution_id == execution.id
                            && request.instance_id == resources.record.id
                            && request.project_id == resources.instance.project_id
                            && call.data["receipt"] == *receipt,
                        "平台产物缺少当前实际步骤回执",
                    )?;
                    if let Some(DeliveryProducer::Platform { tools }) = &definition.producer {
                        require(
                            tools.contains(&request.definition.qualified_name),
                            "交付声明与实际平台工具不符",
                        )?;
                    }
                    out["kind"] = json!("platform");
                    out["call_id"] = json!(call.id);
                    out["tool"] = json!(request.definition.qualified_name);
                    out["pointer"] = json!(pointer);
                    if definition.format == "diff" {
                        let comparison = pointer
                            .strip_suffix("/diff")
                            .and_then(|p| receipt.pointer(p));
                        let base = comparison
                            .and_then(|v| v.get("base_revision"))
                            .unwrap_or(&receipt["target_revision"]);
                        let head = comparison
                            .and_then(|v| v.get("revision"))
                            .unwrap_or(&receipt["revision"]);
                        require(
                            base.is_string() && head.is_string(),
                            "实际 Diff 缺少确切端点",
                        )?;
                        out["base_revision"] = base.clone();
                        out["revision"] = head.clone();
                        out["comparison"] = json!(pointer);
                    }
                } else {
                    require(
                        call.r#type == "tool_call"
                            && call.data["status"] == "succeeded"
                            && call.data["invocation"]["operation_id"] == execution.id
                            && !matches!(
                                definition.producer,
                                Some(
                                    DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow
                                )
                            ),
                        "普通工具不能替代平台或子流程执行",
                    )?;
                    out["kind"] = json!("tool");
                    out["call_id"] = json!(call.id);
                    out["pointer"] = json!(pointer);
                }
            }
            WorkflowOutputProof::Children { ids, outputs } => {
                require(
                    matches!(step.executor, StepExecutor::Workflow { .. })
                        && outputs.get(name) == Some(value),
                    "汇总不是已完成子流程的实际产物",
                )?;
                out["kind"] = json!("workflow");
                out["children"] = json!(ids);
            }
        }
        Ok(out)
    }
    pub(crate) fn start_native_workflow_wait(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        started: u64,
        reason: &str,
    ) -> Result<()> {
        let active = one(
            self.store.as_ref(),
            "workflow_active_wait",
            &resources.execution.id,
        )?;
        commit.witness([version(&active)])?;
        if active.value.is_none() {
            commit.new_record(
                record(
                    "workflow_wait_interval",
                    resources.execution.id.clone(),
                    resources.record.scope.clone(),
                    "实际执行等待区间".into(),
                    1,
                    None,
                    json!({"started":started,"finished":null,"reason":reason}),
                    false,
                ),
                "workflow-wait-interval",
            )?;
        }
        Ok(())
    }
    pub(crate) fn resume_native_workflow_wait(
        &self,
        execution: &StepExecution,
    ) -> Result<(ReadSet, Vec<ResourceChange>)> {
        let active = one(self.store.as_ref(), "workflow_active_wait", &execution.id)?;
        let mut reads = ReadSet::new(vec![version(&active)])?;
        let mut changes = vec![];
        if active.value.is_some() {
            let mut row = restored(self, &active)?;
            require(
                row.r#type == "workflow_wait_interval"
                    && row.definition_id == execution.id
                    && row.data["finished"].is_null(),
                "原等待区间身份不符",
            )?;
            let key = ResourceKey::new("governance", &row.id);
            reads = reads.merge([ResourceVersion {
                resource: key.clone(),
                revision: active.revision,
            }])?;
            let mut data = row.data.clone();
            data["finished"] = json!(now());
            crate::governance::update_record(&mut row, data);
            changes.push(ResourceChange {
                resource: key,
                value: row.to_storage(),
            });
        }
        Ok((reads, changes))
    }
}
