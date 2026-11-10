//! Resolve reusable workflow definitions into immutable executable plans.
use crate::configuration::{role_record, visible};
use crate::governance::reference;
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Compilation {
    pub plan: WorkflowDocument,
    pub task_plan: WorkflowDocument,
    pub roles: BTreeMap<Id, AgentRole>,
    pub specifications: Vec<SpecificationBinding>,
    pub hashes: BTreeMap<Id, String>,
    pub baseline: Option<BaselineCheck>,
}
fn phase(step: &WorkflowStep) -> Option<String> {
    if step.kind == "agent.interact" {
        return Some(step.interaction.output_kind.clone());
    }
    step.parameters
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn delivery_kind(
    doc: &WorkflowDocument,
    step: &WorkflowStep,
    seen: &mut BTreeSet<String>,
) -> Option<String> {
    if !seen.insert(step.key.clone()) {
        return None;
    }
    if let Some(p) = phase(step) {
        return Some(match p.as_str() {
            "requirement_definition" => "requirement".into(),
            "solution_planning" => "plan".into(),
            _ => p,
        });
    }
    if step.kind.starts_with("merge.") {
        return Some("merge_candidate".into());
    }
    for dep in &step.dependencies {
        if let Some(source) = doc.steps.iter().find(|s| &s.key == dep) {
            if let Some(kind) = delivery_kind(doc, source, seen) {
                return Some(kind);
            }
        }
    }
    None
}
fn map_condition(condition: &mut WorkflowCondition, keys: &BTreeMap<String, String>) {
    match condition {
        WorkflowCondition::All { conditions } | WorkflowCondition::Any { conditions } => {
            for c in conditions {
                map_condition(c, keys)
            }
        }
        WorkflowCondition::OutputPresent { step } => {
            if let Some(k) = keys.get(step) {
                *step = k.clone()
            }
        }
        _ => {}
    }
}
pub(crate) fn namespace(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.into()
    } else {
        let hash = configuration_hash(&(prefix, key));
        format!(
            "call_{}_{}",
            &hash[..12],
            key.chars().take(50).collect::<String>()
        )
    }
}
struct Builder<'a> {
    s: &'a EngineeringState,
    scope: &'a Scope,
    roles: BTreeMap<Id, AgentRole>,
    hashes: BTreeMap<Id, String>,
    specs: Vec<SpecificationBinding>,
    stages: Vec<WorkflowStage>,
    steps: Vec<WorkflowStep>,
    stack: Vec<Id>,
}
impl Builder<'_> {
    fn expand(
        &mut self,
        doc: &WorkflowDocument,
        prefix: &str,
        incoming: &[String],
        inputs: &BTreeMap<String, WorkflowPortReference>,
    ) -> Result<BTreeMap<String, WorkflowPortReference>> {
        require(self.stack.len() <= 8, "子流程深度超过 8")?;
        validate_workflow(doc)?;
        let keys: BTreeMap<String, String> = doc
            .steps
            .iter()
            .map(|s| (s.key.clone(), namespace(prefix, &s.key)))
            .collect();
        for stage in &doc.stages {
            let key = namespace(prefix, &stage.key);
            if !self.stages.iter().any(|s| s.key == key) {
                self.stages.push(WorkflowStage {
                    description: stage.description.clone(),
                    key,
                    name: if prefix.is_empty() {
                        stage.name.clone()
                    } else {
                        format!("{prefix} / {}", stage.name)
                    },
                });
            }
        }
        for original in &doc.steps {
            require(self.steps.len() < 1000, "展开子流程超过 1000 个步骤")?;
            let mut step = original.clone();
            for (name, input) in &original.delivery_inputs {
                step.inputs
                    .entry(name.clone())
                    .or_insert_with(|| input.source.clone());
            }
            step.key = keys[&original.key].clone();
            step.stage = namespace(prefix, &original.stage);
            step.dependencies = original
                .dependencies
                .iter()
                .map(|d| keys[d].clone())
                .collect();
            if original.dependencies.is_empty() {
                step.dependencies.extend_from_slice(incoming);
            }
            step.rework_scope = original
                .rework_scope
                .iter()
                .map(|d| keys[d].clone())
                .collect();
            map_condition(&mut step.condition, &keys);
            for input in step.inputs.values_mut() {
                if input.step == "$input" {
                    *input = inputs.get(&input.port).cloned().ok_or_else(|| {
                        err(
                            "invalid_workflow",
                            format!("子流程输入未提供: {}", input.port),
                        )
                    })?;
                } else {
                    input.step = keys[&input.step].clone();
                }
            }
            for (name, input) in &mut step.delivery_inputs {
                input.source = step.inputs[name].clone();
            }
            let path = if prefix.is_empty() {
                original.key.clone()
            } else {
                format!("{prefix}/{}", original.key)
            };
            if let Some(role) = &original.role_ref {
                let r = role_record(self.s, role)?;
                let r = role_record(self.s, &r.definition_id)?;
                let def: RoleDefinition = r.decode()?;
                require(
                    visible(self.s, r, self.scope.project()),
                    format!("Role {} 对项目不可见", r.name),
                )?;
                require(
                    !crate::builtin_defaults::role_deleted(self.s, r),
                    "Role 已删除",
                )?;
                let required = match step.kind.as_str() {
                    "delivery.review" | "merge.review" => Some(AgentRole::Reviewer),
                    "merge.prepare" => Some(AgentRole::Tester),
                    "agent.interact" => match step.interaction.output_kind.as_str() {
                        "requirement" | "feature" => Some(AgentRole::Analyst),
                        "design" | "task_plan" => Some(AgentRole::Architect),
                        "test_plan" => Some(AgentRole::Tester),
                        _ => None,
                    },
                    "attempt.execute" => match phase(&step).as_deref() {
                        Some("requirement_definition") => Some(AgentRole::Analyst),
                        Some("solution_planning") => Some(AgentRole::Architect),
                        Some("implementation") => Some(AgentRole::Developer),
                        Some("closure") => Some(AgentRole::Tester),
                        _ => return Err(err("invalid_workflow", "不支持的正式执行阶段")),
                    },
                    _ => None,
                };
                require(
                    required.is_none_or(|required| required == def.execution_role),
                    format!("{}: Role 执行职责不匹配", step.name),
                )?;
                step.role_ref = Some(r.definition_id.clone());
                step.participant_slot = Some(r.definition_id.clone());
                self.roles
                    .insert(r.definition_id.clone(), def.execution_role);
                self.hashes.insert(r.id.clone(), r.content_hash.clone());
            }
            for node in &mut step.execution_recipe {
                let r = role_record(
                    self.s,
                    node.role_ref
                        .as_deref()
                        .ok_or_else(|| err("invalid_workflow", "执行节点缺少 Role"))?,
                )?;
                let r = role_record(self.s, &r.definition_id)?;
                let def: RoleDefinition = r.decode()?;
                require(
                    def.execution_role == node.role && visible(self.s, r, self.scope.project()),
                    "执行节点 Role 职责或范围不符",
                )?;
                node.role_ref = Some(r.definition_id.clone());
                self.roles
                    .insert(r.definition_id.clone(), def.execution_role);
                self.hashes.insert(r.id.clone(), r.content_hash.clone());
            }
            let phase = phase(original);
            let delivery = delivery_kind(doc, original, &mut BTreeSet::new());
            let mut checks = BTreeMap::new();
            for spec in &step.spec_refs {
                let definition = self
                    .s
                    .definitions
                    .iter()
                    .find(|d| &d.id == spec && d.family == "spec")
                    .ok_or_else(|| err("not_found", format!("Spec 不存在: {spec}")))?;
                if let Ok(r) = self.s.configuration(spec) {
                    reference(self.s, &r.id, "spec", self.scope)?;
                    let applicable: Vec<ActionSelector> = serde_json::from_value(
                        r.data
                            .get("applicable_actions")
                            .cloned()
                            .unwrap_or(json!([])),
                    )
                    .map_err(|e| err("invalid_configuration", e.to_string()))?;
                    require(
                        applicable.is_empty()
                            || applicable.iter().any(|a| {
                                a.matches(&step.kind, phase.as_deref(), delivery.as_deref())
                            }),
                        format!("{}: Spec {} 不适用", step.name, r.name),
                    )?;
                    self.hashes.insert(r.id.clone(), r.content_hash.clone());
                } else {
                    self.hashes
                        .insert(spec.clone(), configuration_hash(definition));
                }
                checks.insert(spec.clone(), definition.rules.clone());
            }
            let delivery_types = crate::deliverables::definitions(self.s, &step, self.scope)?;
            for delivery in &delivery_types {
                self.hashes
                    .insert(delivery.id.clone(), delivery.content_hash.clone());
            }
            let task = if action_requires_task(&step.kind) {
                Some(WorkflowTaskDefinition {
                    delivery_types,
                    renderer_version: TASK_RENDERER.into(),
                    workflow_revision_id: self.stack.last().cloned().unwrap(),
                    source_step: original.key.clone(),
                    path: path.clone(),
                    step: json!(step),
                    stage: json!(doc.stages.iter().find(|s| s.key == original.stage)),
                })
            } else {
                None
            };
            self.specs.push(SpecificationBinding{task,path:path.clone(),step:step.key.clone(),action:step.kind.clone(),phase,delivery_kind:delivery,role_ref:step.role_ref.clone(),spec_refs:step.spec_refs.clone(),checks,output_contract:json!({"interaction":step.interaction,"inputs":step.inputs,"parameters":step.parameters,"dependencies":step.dependencies,"condition":step.condition,"advance_mode":step.advance_mode,"execution_recipe":step.execution_recipe,"subworkflow_ref":step.subworkflow_ref,"stage":doc.stages.iter().find(|s|s.key==original.stage),"workflow_outputs":doc.output_ports})});
            if step.kind == "workflow.call" {
                let child = reference(
                    self.s,
                    step.subworkflow_ref
                        .as_deref()
                        .ok_or_else(|| err("invalid_workflow", "缺少子流程"))?,
                    "workflow",
                    self.scope,
                )?;
                require(!self.stack.contains(&child.id), "子流程引用存在环")?;
                let child_doc: WorkflowDocument = child.decode()?;
                require(
                    child_doc.schema_version == 3,
                    "子流程需要 Workflow Schema v3",
                )?;
                for required in &child_doc.input_ports {
                    require(
                        step.inputs.contains_key(required),
                        format!("子流程缺少输入 {required}"),
                    )?;
                }
                self.stack.push(child.id.clone());
                self.hashes
                    .insert(child.id.clone(), child.content_hash.clone());
                let mut gate = step.clone();
                gate.key = namespace(&path, "entry");
                gate.kind = "branch".into();
                gate.name = format!("{} · 进入", step.name);
                gate.subworkflow_ref = None;
                gate.inputs.clear();
                gate.delivery_inputs.clear();
                gate.delivery_outputs.clear();
                gate.parameters.clear();
                gate.spec_refs.clear();
                gate.role_ref = None;
                gate.participant_slot = None;
                let entry = gate.key.clone();
                self.steps.push(gate);
                step.condition = WorkflowCondition::Always;
                step.advance_mode = AdvanceMode::Automatic;
                let start = self.steps.len();
                let outputs = self.expand(
                    &child_doc,
                    &path,
                    std::slice::from_ref(&entry),
                    &step.inputs,
                )?;
                self.stack.pop();
                for child in &mut self.steps[start..] {
                    let mut guards: Vec<String> = serde_json::from_value(
                        child
                            .parameters
                            .get("call_guards")
                            .cloned()
                            .unwrap_or(json!([])),
                    )
                    .unwrap_or_default();
                    guards.push(entry.clone());
                    child.parameters.insert("call_guards".into(), json!(guards));
                }
                step.parameters.insert("call_entry".into(), json!(entry));
                step.dependencies = std::iter::once(entry)
                    .chain(self.steps[start..].iter().map(|s| s.key.clone()))
                    .collect();
                step.parameters
                    .insert("call_outputs".into(), json!(outputs));
                step.parameters.insert("call_path".into(), json!(path));
            }
            self.steps.push(step);
        }
        let mut outputs = doc.output_ports.clone();
        for port in outputs.values_mut() {
            port.step = keys
                .get(&port.step)
                .cloned()
                .ok_or_else(|| err("invalid_workflow", "子流程输出不存在"))?;
        }
        Ok(outputs)
    }
}
pub(crate) fn compile(
    s: &EngineeringState,
    wf: &ConfigurationRecord,
    standard: Option<&str>,
    scope: &Scope,
) -> Result<Compilation> {
    let source: WorkflowDocument = wf.decode()?;
    require(source.schema_version == 3, "请选择 Workflow Schema v3")?;
    let mut b = Builder {
        s,
        scope,
        roles: BTreeMap::new(),
        hashes: [(wf.id.clone(), wf.content_hash.clone())].into(),
        specs: vec![],
        stages: vec![],
        steps: vec![],
        stack: vec![wf.id.clone()],
    };
    let input_bindings = source
        .input_ports
        .iter()
        .map(|name| {
            (
                name.clone(),
                WorkflowPortReference {
                    step: "$input".into(),
                    port: name.clone(),
                },
            )
        })
        .collect();
    b.expand(&source, "", &[], &input_bindings)?;
    let mut plan = source.clone();
    plan.steps = std::mem::take(&mut b.steps);
    plan.stages = std::mem::take(&mut b.stages);
    let mut task_plan = WorkflowDocument {
        schema_version: source.schema_version,
        process_kind: "task".into(),
        stages: vec![],
        steps: vec![],
        binding_slots: BTreeMap::new(),
        subworkflow_refs: BTreeMap::new(),
        input_ports: vec![],
        output_ports: BTreeMap::new(),
    };
    if let Some(task) = source.subworkflow_refs.get("task") {
        let r = reference(s, task, "workflow", scope)?;
        let doc: WorkflowDocument = r.decode()?;
        require(
            doc.schema_version == 3 && doc.process_kind == "task",
            "Task 子流程用途或版本不匹配",
        )?;
        b.hashes.insert(r.id.clone(), r.content_hash.clone());
        b.stack.push(r.id.clone());
        let before = b.specs.len();
        require(
            doc.input_ports.is_empty(),
            "Task Workflow 不接受未绑定的外部端口",
        )?;
        let outputs = b.expand(&doc, "task", &[], &BTreeMap::new())?;
        b.stack.pop();
        task_plan = doc;
        task_plan.output_ports = outputs;
        task_plan.steps = std::mem::take(&mut b.steps);
        task_plan.stages = std::mem::take(&mut b.stages);
        for binding in &mut b.specs[before..] {
            binding.path = format!("tasks/{}", binding.path);
            if let Some(task) = &mut binding.task {
                task.path = binding.path.clone();
            }
        }
    } else if source.process_kind == "feature"
        && source.steps.iter().any(|s| s.kind == "tasks.foreach")
    {
        return Err(err("invalid_workflow", "缺少 Task 子流程"));
    }
    validate_workflow_tasks(&plan)?;
    validate_workflow_tasks(&task_plan)?;
    let task_bytes: usize = plan
        .steps
        .iter()
        .chain(&task_plan.steps)
        .map(|step| {
            step.task_instructions.as_ref().map_or(0, String::len)
                + step
                    .execution_recipe
                    .iter()
                    .map(|n| n.task_instructions.as_ref().map_or(0, String::len))
                    .sum::<usize>()
        })
        .sum();
    require(
        task_bytes <= TASK_WORKFLOW_LIMIT,
        "Feature 与 Task 展开任务正文合计超过 1 MiB",
    )?;
    for document in [&mut plan, &mut task_plan] {
        document.binding_slots = b
            .roles
            .iter()
            .map(|(key, role)| {
                (
                    key.clone(),
                    WorkflowSlot {
                        role: *role,
                        agent_ref: None,
                        capabilities: vec![],
                    },
                )
            })
            .collect();
    }
    require(
        plan.steps.len() + task_plan.steps.len() <= 1000,
        "展开流程超过 1000 个步骤",
    )?;
    for doc in [&plan, &task_plan] {
        if !doc.steps.is_empty() {
            validate_workflow_ports(doc)?;
        }
        if ["feature", "task"].contains(&doc.process_kind.as_str()) && !doc.steps.is_empty() {
            validate_workflow_semantics(doc)?;
        }
    }
    let baseline = standard
        .map(|id| -> Result<_> {
            let r = reference(s, id, "standard", scope)?;
            let standard: StandardDefinition = r.decode()?;
            for rule in &standard.baseline_rules {
                for spec in &rule.required_spec_refs {
                    reference(s, spec, "spec", scope)?;
                }
            }
            b.hashes.insert(r.id.clone(), r.content_hash.clone());
            check_baseline(id, &standard, &b.specs)
        })
        .transpose()?;
    Ok(Compilation {
        plan,
        task_plan,
        roles: b.roles,
        specifications: b.specs,
        hashes: b.hashes,
        baseline,
    })
}
