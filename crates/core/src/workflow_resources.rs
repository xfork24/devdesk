//! Actual workflow resources, with independent native step/header epochs.
use crate::{concurrency::*, tool_calls::WorkflowToolAuthority, *};

pub(crate) fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: if row.resource.kind != "governance"
            && row
                .value
                .as_ref()
                .is_some_and(|v| v["type"].is_string() && v["id"].is_string())
        {
            ResourceKey::new(
                "selected_record",
                json!([
                    row.resource.kind,
                    row.resource.id,
                    row.value.as_ref().unwrap()["id"]
                ])
                .to_string(),
            )
        } else {
            row.resource.clone()
        },
        revision: row.revision,
    }
}
pub(crate) struct WorkflowResources {
    pub record: Arc<ConfigurationRecord>,
    pub instance: Arc<ConfiguredWorkflowInstance>,
    pub header: ResourceSnapshot,
    pub execution: StepExecution,
    pub step: ResourceSnapshot,
    pub reads: ReadSet,
}
impl WorkflowResources {
    pub fn read(store: &dyn EngineeringStore, instance: &str, execution: &str) -> Result<Self> {
        let keys = [
            ResourceKey::new("governance", instance),
            ResourceKey::new("workflow_header", instance),
            ResourceKey::new("workflow_step", json!([instance, execution]).to_string()),
        ];
        let rows = store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "步骤与流程头事实不完整",
        )?;
        let raw = ConfigurationRecord::from_storage(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原流程不存在"))?,
        )?;
        require(
            raw.r#type == "workflow_instance"
                && raw.scope.project_id() == store.project_id().as_deref(),
            "原流程不属于此项目库",
        )?;
        let selected = rows[2]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原步骤不存在或已替换"))?;
        let mut header = raw.data.clone();
        header
            .as_object_mut()
            .ok_or_else(|| err("storage", "流程头结构无效"))?
            .remove("steps");
        require(
            rows[1].value.as_ref() == Some(&header)
                && raw.data["steps"]
                    .as_array()
                    .is_some_and(|s| s.iter().any(|s| s == selected)),
            "流程头与步骤不属于同一已提交事实",
        )?;
        let record: ConfigurationRecord =
            serde_json::from_value(store.restore_runtime_record(&raw.to_storage())?)
                .map_err(|e| err("storage", e.to_string()))?;
        let instance: ConfiguredWorkflowInstance = record.decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == execution)
            .cloned()
            .ok_or_else(|| err("not_found", "原步骤已失效"))?;
        let mut reads = ReadSet::new(vec![version(&rows[1]), version(&rows[2])])?;
        let definition = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
        // The task renderer identifies upstream outputs. Witness precisely
        // those native siblings, rather than using the parent record epoch.
        for binding in definition.inputs.values() {
            if let InputBinding::Source { source } = binding
                && source.step != "$input"
            {
                let upstream = instance
                    .steps
                    .iter()
                    .find(|e| e.step == source.step)
                    .ok_or_else(|| err("storage", "任务输入步骤缺失"))?;
                if upstream.id == execution.id {
                    continue;
                }
                let key =
                    ResourceKey::new("workflow_step", json!([record.id, upstream.id]).to_string());
                let selected = store
                    .read_resources(std::slice::from_ref(&key))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| err("storage", "任务来源步骤事实缺失"))?;
                if !selected
                    .value
                    .as_ref()
                    .is_some_and(|s| s["outputs"] == json!(upstream.outputs))
                {
                    return Err(err("revision_conflict", "任务来源产物在准备期间变化")
                        .with_context("WorkflowResources::read", json!({"workflow_instance_id":record.id,"execution_id":execution.id,"upstream_execution_id":upstream.id,"expected_outputs":upstream.outputs,"actual_outputs":selected.value.as_ref().map(|v|&v["outputs"]),"actual_resource_revision":selected.revision})));
                }
                reads = reads.merge([version(&selected)])?;
            }
        }
        Ok(Self {
            record: Arc::new(record),
            instance: Arc::new(instance),
            header: rows[1].clone(),
            execution,
            step: rows[2].clone(),
            reads,
        })
    }
    /// Reuse the actual decoded source view for this selected root. Only the
    /// selected native step and declared upstream witnesses are read again.
    pub fn from_header(
        store: &dyn EngineeringStore,
        record: &Arc<ConfigurationRecord>,
        instance: &Arc<ConfiguredWorkflowInstance>,
        header: &ResourceSnapshot,
        id: &str,
    ) -> Result<Self> {
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| err("not_found", "原步骤不存在"))?
            .clone();
        let key = ResourceKey::new("workflow_step", json!([record.id, id]).to_string());
        let step = store.read_resources(std::slice::from_ref(&key))?.remove(0);
        let original = store.encode_runtime_value(&json!(execution))?;
        if step.value.as_ref() != Some(&original) {
            let mut changed = serde_json::Map::new();
            let keys: std::collections::BTreeSet<_> = original
                .as_object()
                .into_iter()
                .flat_map(|v| v.keys())
                .chain(
                    step.value
                        .as_ref()
                        .and_then(Value::as_object)
                        .into_iter()
                        .flat_map(|v| v.keys()),
                )
                .collect();
            for key in keys {
                let expected = original.get(key);
                let actual = step.value.as_ref().and_then(|v| v.get(key));
                if expected != actual {
                    changed.insert(key.clone(), json!({"expected":expected,"actual":actual}));
                }
            }
            return Err(err("revision_conflict", "原步骤在准备期间已变化")
                .with_context("WorkflowResources::from_header", json!({"project_id":store.project_id(),"workflow_instance_id":record.id,"execution_id":id,"step":execution.step,"node_id":execution.node_id,"plan_revision_id":execution.plan_revision_id,"snapshot_revision":record.revision,"header_revision":header.revision,"actual_step_revision":step.revision,"actual_step_exists":step.value.is_some(),"changed_fields":changed})));
        }
        let mut reads = ReadSet::new(vec![version(header), version(&step)])?;
        let definition = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
        for binding in definition.inputs.values() {
            if let InputBinding::Source { source } = binding
                && source.step != "$input"
            {
                let upstream = instance
                    .steps
                    .iter()
                    .find(|e| e.step == source.step)
                    .ok_or_else(|| err("storage", "原输入步骤缺失"))?;
                if upstream.id == execution.id {
                    continue;
                }
                let row = store
                    .read_resources(&[ResourceKey::new(
                        "workflow_step",
                        json!([record.id, upstream.id]).to_string(),
                    )])?
                    .remove(0);
                if !row
                    .value
                    .as_ref()
                    .is_some_and(|v| v["outputs"] == json!(upstream.outputs))
                {
                    return Err(err("revision_conflict", "原输入产物已变化")
                        .with_context("WorkflowResources::from_header", json!({"workflow_instance_id":record.id,"execution_id":id,"upstream_execution_id":upstream.id,"expected_outputs":upstream.outputs,"actual_outputs":row.value.as_ref().map(|v|&v["outputs"]),"actual_resource_revision":row.revision})));
                }
                reads = reads.merge([version(&row)])?;
            }
        }
        Ok(Self {
            record: record.clone(),
            instance: instance.clone(),
            header: header.clone(),
            execution,
            step,
            reads,
        })
    }
    pub fn changed_step(
        &self,
        store: &dyn EngineeringStore,
        next: &StepExecution,
    ) -> Result<ResourceChange> {
        require(
            next.id == self.execution.id && next.workflow_instance_id == self.record.id,
            "步骤变化不能替换原身份",
        )?;
        Ok(ResourceChange {
            resource: self.step.resource.clone(),
            value: store.encode_runtime_value(&json!(next))?,
        })
    }
}

pub(crate) fn live_workflow_chain(
    store: &dyn EngineeringStore,
    instance: &str,
    execution: &str,
    leaf_status: StepExecutionStatus,
) -> Result<(WorkflowToolAuthority, ReadSet)> {
    let mut instance = instance.to_owned();
    let mut execution = execution.to_owned();
    let mut instances = Vec::new();
    let mut executions = Vec::new();
    let mut reads = ReadSet::default();
    let mut leaf = None;
    for depth in 0..=WORKFLOW_DEPTH_LIMIT {
        require(!instances.contains(&instance), "流程祖先存在环")?;
        let keys = [
            ResourceKey::new("workflow_run_authority", &instance),
            ResourceKey::new(
                "workflow_step_authority",
                json!([instance, execution]).to_string(),
            ),
        ];
        let rows = store.read_resources(&keys)?;
        require(
            rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "流程资格读取不完整",
        )?;
        let run = rows[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原流程祖先缺失"))?;
        let step = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原祖先执行缺失"))?;
        require(
            run["project_id"].as_str() == store.project_id().as_deref()
                && run["paused"] != true
                && run["cancel_requested"] != true
                && !matches!(run["status"].as_str(), Some("completed" | "cancelled")),
            "原流程或祖先已暂停、终止或撤销",
        )?;
        require(
            step["stop_requested"] != true
                && step["status"]
                    == json!(if depth == 0 {
                        leaf_status.clone()
                    } else {
                        StepExecutionStatus::Running
                    }),
            "原步骤或祖先执行资格已经变化",
        )?;
        reads = reads.merge(rows.iter().map(version))?;
        if leaf.is_none() {
            leaf = Some(step.clone());
        }
        instances.push(instance.clone());
        executions.push(execution.clone());
        let Some(parent) = run["parent_execution_id"].as_str() else {
            let leaf = leaf.unwrap();
            return Ok((
                WorkflowToolAuthority {
                    instances,
                    executions,
                    execution_id: leaf["id"].as_str().unwrap_or(&execution).into(),
                    plan_revision_id: text(&leaf, "plan_revision_id")?,
                    input_digest: if leaf_status == StepExecutionStatus::Queued {
                        leaf["input_digest"].as_str().unwrap_or("").into()
                    } else {
                        text(&leaf, "input_digest")?
                    },
                    contract_digest: text(&leaf, "contract_digest")?,
                    node_id: text(&leaf, "node_id")?,
                    session_id: opt(&leaf, "session_id"),
                },
                reads,
            ));
        };
        let key = ResourceKey::new("workflow_execution_owner", parent);
        let owner = store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "流程祖先归属读取不完整"))?;
        instance = text(
            owner
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "流程祖先归属缺失"))?,
            "instance_id",
        )?;
        execution = parent.into();
        reads = reads.merge([version(&owner)])?;
    }
    Err(err("invalid_workflow", "流程祖先超过深度预算"))
}
