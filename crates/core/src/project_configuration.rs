//! Explicit project binding and immutable v2 feature configuration.
use crate::configuration::role_record;
use crate::governance::{record, reference, validate_composition};
use crate::workflow_definition::{Compilation, compile};
use crate::*;
use std::collections::BTreeMap;
fn bind(
    s: &EngineeringState,
    compilation: &mut Compilation,
    bindings: &BTreeMap<Id, Id>,
    scope: &Scope,
) -> Result<BTreeMap<Id, Id>> {
    let mut resolved = BTreeMap::new();
    if let Some(sandbox) = s.sandboxes.iter().find(|b| b.id == DEFAULT_SANDBOX_ID) {
        compilation
            .hashes
            .insert(sandbox.id.clone(), configuration_hash(sandbox));
    }
    for (role, execution_role) in &compilation.roles {
        let agent_id = bindings.get(role).ok_or_else(|| {
            err(
                "configuration_required",
                format!(
                    "Role {} 未绑定 Agent",
                    role_record(s, role)
                        .map(|r| r.name.as_str())
                        .unwrap_or(role)
                ),
            )
        })?;
        let agent = reference(s, agent_id, "agent", scope)?;
        let a: AgentComposition = agent.decode()?;
        let role_ref = a
            .role_ref
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "Agent 未声明 Role"))?;
        require(
            role_record(s, role_ref)?.definition_id == *role && a.role == *execution_role,
            "Agent 必须属于所选 Role 身份，不能仅匹配执行职责",
        )?;
        validate_composition(s, &a, scope)?;
        for id in std::iter::once(agent_id)
            .chain(a.role_ref.iter())
            .chain(a.capability_ref.iter())
            .chain(std::iter::once(&a.model_ref))
            .chain(std::iter::once(&a.harness_ref))
            .chain(a.tools.iter())
            .chain(a.skills.iter())
            .chain(a.permissions.iter())
        {
            if let Ok(r) = s.configuration(id) {
                compilation
                    .hashes
                    .insert(r.id.clone(), r.content_hash.clone());
            } else if let Some(d) = s.definitions.iter().find(|d| &d.id == id) {
                compilation.hashes.insert(id.clone(), configuration_hash(d));
            } else if let Some(permission) = s.permissions.iter().find(|p| &p.id == id) {
                compilation
                    .hashes
                    .insert(id.clone(), configuration_hash(permission));
            } else {
                return Err(err("not_found", format!("Agent 依赖不存在: {id}")));
            }
        }
        if a.capability_ref.is_none() {
            if let Some(cap) = s
                .governance
                .iter()
                .find(|r| r.kind == "agent_capability" && r.definition_id == *agent_id)
                .and_then(|r| r.data["capability_ref"].as_str())
                .and_then(|id| s.configuration(id).ok())
            {
                compilation
                    .hashes
                    .insert(cap.id.clone(), cap.content_hash.clone());
            }
        }
        resolved.insert(role.clone(), agent_id.clone());
    }
    require(
        compilation.baseline.as_ref().is_none_or(|b| b.valid),
        compilation
            .baseline
            .as_ref()
            .map(|b| b.errors.join("；"))
            .unwrap_or_default(),
    )?;
    for doc in [&mut compilation.plan, &mut compilation.task_plan] {
        for (key, slot) in &mut doc.binding_slots {
            slot.agent_ref = resolved.get(key).cloned();
        }
    }
    Ok(resolved)
}
impl Engine {
    pub(crate) fn update_project_configuration_v2(
        &self,
        s: &mut EngineeringState,
        c: &Command,
        wf: &ConfigurationRecord,
    ) -> Result<Value> {
        let target = c.target.as_deref().unwrap_or("");
        let project = s
            .projects
            .iter()
            .find(|p| p.id == target)
            .ok_or_else(|| err("not_found", "Project 不存在"))?
            .clone();
        let scope = Scope::Project {
            project_id: target.into(),
        };
        let standard = opt(&c.payload, "standard_ref");
        let mut result = compile(s, wf, standard.as_deref(), &scope)?;
        let bindings: BTreeMap<Id, Id> = serde_json::from_value(
            c.payload
                .get("role_agent_bindings")
                .cloned()
                .unwrap_or(json!({})),
        )
        .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            result.plan.process_kind == "feature" && result.plan.input_ports.is_empty(),
            "项目需要无需外部输入的 Feature Workflow",
        )?;
        let resolved = bind(s, &mut result, &bindings, &scope)?;
        let previous = s
            .governance
            .iter()
            .filter(|r| r.kind == "project_configuration" && r.definition_id == target)
            .max_by_key(|r| r.version)
            .cloned();
        if let Some(expected) = opt(&c.payload, "source_configuration_id") {
            require(
                previous.as_ref().is_some_and(|r| r.id == expected),
                "项目配置已变化",
            )?;
        }
        let r = record(
            "project_configuration",
            target.into(),
            scope,
            format!("{} 配置", project.title),
            previous.as_ref().map_or(1, |r| r.version + 1),
            previous.map(|r| r.id),
            json!({"schema_version":2,"workflow_ref":wf.id,"standard_ref":standard,"role_agent_bindings":resolved,"specification_snapshot":result.specifications,"baseline_check":result.baseline,"definition_hashes":result.hashes}),
            true,
        );
        let value = json!(r);
        s.governance.push(r);
        let project = s.projects.iter_mut().find(|p| p.id == target).unwrap();
        project.revision += 1;
        // Empty standard denotes explicit Step specifications, not absence of engineering gates.
        project.binding.standard = String::new();
        if let Some(id) = resolved.values().find(|id| {
            s.definitions.iter().any(|d| {
                &d.id == *id
                    && d.agent_profile
                        .as_ref()
                        .is_some_and(|p| p.role == AgentRole::Developer)
            })
        }) {
            project.binding.agent = id.clone();
            project.binding.skills = s
                .definitions
                .iter()
                .find(|d| &d.id == id)
                .and_then(|d| d.agent_profile.as_ref())
                .map(|p| p.skill_revisions.clone())
                .unwrap_or_default();
        }
        Ok(value)
    }
    pub(crate) fn freeze_configuration_v2(
        &self,
        s: &mut EngineeringState,
        feature: &str,
        current: &ConfigurationRecord,
    ) -> Result<()> {
        let f = s.feature(feature)?.clone();
        let scope = Scope::Feature {
            project_id: f.project_id.clone(),
            feature_id: feature.into(),
        };
        let wf = reference(s, &text(&current.data, "workflow_ref")?, "workflow", &scope)?.clone();
        let standard = opt(&current.data, "standard_ref");
        let mut result = compile(s, &wf, standard.as_deref(), &scope)?;
        let bindings: BTreeMap<Id, Id> =
            serde_json::from_value(current.data["role_agent_bindings"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
        let resolved = bind(s, &mut result, &bindings, &scope)?;
        let config = FeatureConfiguration {
            defaults_bundle_ref: None,
            project_configuration_id: current.id.clone(),
            workflow_ref: wf.id.clone(),
            standard_ref: String::new(),
            agent_refs: resolved.clone(),
            role_agent_bindings: resolved,
            specification_snapshot: result.specifications,
            baseline_check: result.baseline,
            definition_hashes: result.hashes,
            plan: result.plan,
            task_plan: result.task_plan,
            compiler_version: 2,
        };
        let lock = record(
            "feature_lock",
            feature.into(),
            scope.clone(),
            format!("{} 配置锁", f.title),
            1,
            None,
            json!(config),
            true,
        );
        let id = lock.id.clone();
        s.governance.push(lock);
        let runtime = WorkflowRuntime {
            owner_id: feature.into(),
            feature_id: feature.into(),
            task_id: None,
            parent_id: None,
            configuration_lock_id: id,
            workflow_ref: wf.id,
            status: "active".into(),
            paused: false,
            dirty: true,
            steps: config
                .plan
                .steps
                .iter()
                .map(|step| WorkflowStepRuntime {
                    id: crate::id(),
                    key: step.key.clone(),
                    round: 1,
                    status: "pending".into(),
                    input_hash: String::new(),
                    outputs: BTreeMap::new(),
                    reason: String::new(),
                    revision: 1,
                    confirmation: None,
                })
                .collect(),
            task_members: vec![],
            task_plan_revision: None,
            generation: 1,
        };
        s.governance.push(record(
            "workflow_instance",
            feature.into(),
            scope,
            f.title,
            1,
            None,
            json!(runtime),
            false,
        ));
        let f = s.features.iter_mut().find(|f| f.id == feature).unwrap();
        f.binding.standard = String::new();
        if let Some(a) = config.agent_refs.values().find(|id| {
            s.definitions.iter().any(|d| {
                &d.id == *id
                    && d.agent_profile
                        .as_ref()
                        .is_some_and(|p| p.role == AgentRole::Developer)
            })
        }) {
            f.binding.agent = a.clone();
            f.binding.skills = s
                .definitions
                .iter()
                .find(|d| &d.id == a)
                .and_then(|d| d.agent_profile.as_ref())
                .map(|p| p.skill_revisions.clone())
                .unwrap_or_default();
        }
        Ok(())
    }
    pub fn workflow_agent_for(
        &self,
        s: &EngineeringState,
        owner: &str,
        action: &str,
        phase: Option<&str>,
        node: Option<&str>,
        step_instance: Option<&str>,
    ) -> Result<Option<Id>> {
        let feature = s
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        let Some(lock) = s.feature_configuration(feature) else {
            return Ok(None);
        };
        if lock.compiler_version < 2 {
            return Ok(None);
        }
        let plan = if s.tasks.iter().any(|t| t.id == owner) {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let runtime = s
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_instance" && r.definition_id == owner)
            .max_by_key(|r| r.version)
            .map(|r| r.decode::<WorkflowRuntime>())
            .transpose()?;
        let runtime_step = step_instance
            .and_then(|id| {
                runtime
                    .as_ref()
                    .and_then(|r| r.steps.iter().find(|s| s.id == id))
            })
            .or_else(|| {
                runtime.as_ref().and_then(|r| {
                    r.steps.iter().find(|s| {
                        ["active", "waiting_input", "waiting_dependencies"]
                            .contains(&s.status.as_str())
                            && plan.steps.iter().any(|p| {
                                p.key == s.key
                                    && p.kind == action
                                    && phase.is_none_or(|phase| {
                                        p.parameters.get("phase").and_then(Value::as_str)
                                            == Some(phase)
                                    })
                            })
                    })
                })
            });
        let step = runtime_step
            .and_then(|state| plan.steps.iter().find(|s| s.key == state.key))
            .or_else(|| {
                let mut steps = plan.steps.iter().filter(|s| {
                    s.kind == action
                        && phase.is_none_or(|phase| {
                            s.parameters.get("phase").and_then(Value::as_str) == Some(phase)
                        })
                });
                let first = steps.next();
                if steps.next().is_none() { first } else { None }
            })
            .ok_or_else(|| err("configuration_required", "无法唯一确定冻结工作流步骤"))?;
        let role = node
            .and_then(|node| step.execution_recipe.iter().find(|n| n.key == node))
            .and_then(|n| n.role_ref.as_ref())
            .or(step.role_ref.as_ref())
            .ok_or_else(|| err("configuration_required", "执行节点未声明 Role"))?;
        lock.role_agent_bindings
            .get(role)
            .cloned()
            .map(Some)
            .ok_or_else(|| err("configuration_required", format!("未绑定 Role {role}")))
    }
    pub(crate) fn workflow_configuration_query(
        &self,
        method: &str,
        p: &Value,
    ) -> Option<Result<Value>> {
        if !["query.workflow_requirements", "query.specification_audit"].contains(&method) {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            let mut wf = if let Some(document) = p.get("document") {
                record(
                    "workflow",
                    "preview".into(),
                    p["project_id"]
                        .as_str()
                        .map(|id| Scope::Project {
                            project_id: id.into(),
                        })
                        .unwrap_or(Scope::Global),
                    "预览".into(),
                    1,
                    None,
                    document.clone(),
                    true,
                )
            } else {
                s.configuration(&text(p, "id")?)?.clone()
            };
            if wf.kind == "workflow_draft" {
                wf.data = wf.data["document"].clone();
            }
            let scope = p["project_id"]
                .as_str()
                .map(|id| Scope::Project {
                    project_id: id.into(),
                })
                .unwrap_or(Scope::Global);
            let result = compile(&s, &wf, opt(p, "standard_ref").as_deref(), &scope)?;
            let roles: Vec<_> = result
                .roles
                .iter()
                .map(|(id, execution_role)| {
                    let r = role_record(&s, id)?;
                    Ok(json!({"role_id":id,"role":r,"execution_role":execution_role}))
                })
                .collect::<Result<_>>()?;
            let changes = if let Some(before) = p.get("previous_snapshot") {
                let previous: Vec<SpecificationBinding> = serde_json::from_value(before.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                let mut changes = vec![];
                for binding in &result.specifications {
                    match previous.iter().find(|b|b.path==binding.path){None=>changes.push(json!({"kind":"added","path":binding.path,"after":binding})),Some(before)if before!=binding=>changes.push(json!({"kind":"changed","path":binding.path,"before":before,"after":binding})),_=>{}}
                }
                for binding in &previous {
                    if !result.specifications.iter().any(|b| b.path == binding.path) {
                        changes
                            .push(json!({"kind":"removed","path":binding.path,"before":binding}));
                    }
                }
                changes
            } else {
                vec![]
            };
            Ok(
                json!({"roles":roles,"specification_snapshot":result.specifications,"baseline_check":result.baseline,"definition_hashes":result.hashes,"plan":result.plan,"task_plan":result.task_plan,"changes":changes}),
            )
        })())
    }
}

impl Engine {
    pub(crate) fn freeze_action_specifications(
        &self,
        s: &mut EngineeringState,
        method: &str,
        c: &Command,
        result: &Value,
    ) -> Result<()> {
        if ![
            "attempt.start",
            "review.request",
            "merge.request",
            "session.open",
        ]
        .contains(&method)
        {
            return Ok(());
        }
        let Some(instance) = c.payload["workflow_step_instance_id"].as_str() else {
            return Ok(());
        };
        let Some(runtime) = s
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_instance")
            .filter_map(|r| r.decode::<WorkflowRuntime>().ok())
            .find(|r| r.steps.iter().any(|s| s.id == instance))
        else {
            return Err(err("invalid_workflow", "步骤实例不存在"));
        };
        let lock = s
            .feature_configuration(&runtime.feature_id)
            .ok_or_else(|| err("invalid_workflow", "配置锁不存在"))?;
        if lock.compiler_version < 2 {
            return Ok(());
        }
        let plan = if runtime.task_id.is_some() {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let key = &runtime.steps.iter().find(|s| s.id == instance).unwrap().key;
        let step = plan
            .steps
            .iter()
            .find(|s| &s.key == key)
            .ok_or_else(|| err("invalid_workflow", "冻结步骤不存在"))?;
        let target = text(result, "id")?;
        s.governance.push(record("execution_specification",target,Scope::Feature{project_id:s.feature(&runtime.feature_id)?.project_id.clone(),feature_id:runtime.feature_id},step.name.clone(),1,None,json!({"spec_refs":step.spec_refs,"workflow_step_instance_id":instance,"step":step.key}),true));
        Ok(())
    }
    pub(crate) fn execution_specifications(
        &self,
        s: &EngineeringState,
        run: &Run,
    ) -> Result<Option<Vec<Definition>>> {
        let target = run
            .attempt_id
            .as_ref()
            .or(run.merge_id.as_ref())
            .unwrap_or(&run.id);
        let mut lineage = vec![run.id.as_str()];
        let mut parent = run.retry_of.as_deref();
        while let Some(id) = parent {
            require(!lineage.contains(&id), "Run 重试链存在环")?;
            lineage.push(id);
            parent = s
                .runs
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| r.retry_of.as_deref());
        }
        let record = lineage
            .iter()
            .find_map(|id| {
                s.governance
                    .iter()
                    .find(|r| r.kind == "execution_specification" && r.definition_id == *id)
            })
            .or_else(|| {
                (!run.node.starts_with("review:"))
                    .then(|| {
                        s.governance.iter().find(|r| {
                            r.kind == "execution_specification" && &r.definition_id == target
                        })
                    })
                    .flatten()
            });
        let Some(record) = record else {
            let feature = run
                .attempt_id
                .as_ref()
                .and_then(|id| s.attempt(id).ok())
                .map(|a| a.feature_id.as_str())
                .or_else(|| {
                    run.merge_id
                        .as_ref()
                        .and_then(|id| s.merge(id).ok())
                        .map(|m| m.feature_id.as_str())
                });
            require(
                !feature
                    .and_then(|id| s.feature_configuration(id))
                    .is_some_and(|c| c.compiler_version >= 2),
                "v2 执行缺少冻结 Spec 引用",
            )?;
            return Ok(None);
        };
        let refs: Vec<Id> = serde_json::from_value(record.data["spec_refs"].clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        let specs = refs
            .iter()
            .map(|id| {
                s.definitions
                    .iter()
                    .find(|d| &d.id == id && d.family == "spec")
                    .cloned()
                    .ok_or_else(|| err("not_found", "冻结 Spec 不存在"))
            })
            .collect::<Result<_>>()?;
        Ok(Some(specs))
    }
}

impl Engine {
    pub fn migrate_project_configuration(&self) -> Result<()> {
        self.store.transact(None,&mut|s|{
        crate::configuration::migrate_capabilities(s);
        let configs:Vec<_>=s.projects.iter().filter_map(|p|s.governance.iter().filter(|r|r.kind=="project_configuration"&&r.definition_id==p.id).max_by_key(|r|r.version).cloned()).filter(|r|r.data["schema_version"]!=2).collect();
        for old in configs {
            let receipt_id=format!("project-configuration-conversion:{}",old.id);if s.governance.iter().any(|r|r.id==receipt_id){continue;}
            let checkpoint=s.clone();let upgraded=(||->Result<Value>{let mut role_overrides=BTreeMap::new();let mut roles=BTreeMap::new();
                if let Some(bindings)=old.data["agent_refs"].as_object(){for (slot,value) in bindings{let agent=value.as_str().ok_or_else(||err("configuration_required","旧 Agent 引用不完整"))?;let a:AgentComposition=s.configuration(agent)?.decode()?;let role=role_record(s,a.role_ref.as_deref().ok_or_else(||err("configuration_required","旧 Agent 缺少 Role 证明"))?)?;role_overrides.insert(slot.clone(),role.definition_id.clone());roles.insert(role.definition_id.clone(),agent.to_owned());}}
                let workflow=crate::workflow_definition::import_legacy_workflow_with_roles(s,&text(&old.data,"workflow_ref")?,&text(&old.data,"standard_ref")?,&role_overrides)?;
                let wf=s.configuration(&workflow)?.clone();let cmd=Command{command_id:receipt_id.clone(),target:Some(old.definition_id.clone()),expected_revision:None,payload:json!({"workflow_ref":workflow,"role_agent_bindings":roles})};self.update_project_configuration_v2(s,&cmd,&wf)
            })();
            let data=match upgraded{Ok(value)=>json!({"status":"converted","source":old.id,"configuration_ref":value["id"]}),Err(error)=>{*s=checkpoint;json!({"status":"needs_configuration","source":old.id,"reason":error.message})}};
            let mut receipt=record("configuration_conversion",old.definition_id,old.scope,"项目配置升级".into(),1,None,data,true);receipt.id=receipt_id;s.governance.push(receipt);
        }
        Ok(Value::Null)
    })?;
        Ok(())
    }
}
impl Engine {
    pub(crate) fn delivery_specifications(
        &self,
        s: &EngineeringState,
        d: &Delivery,
        review: Option<&str>,
    ) -> Result<Option<Vec<(Definition, String)>>> {
        if !s
            .feature_configuration(&d.feature_id)
            .is_some_and(|c| c.compiler_version >= 2)
        {
            return Ok(None);
        }
        let mut targets = vec![];
        if let Some(id) = &d.attempt_id {
            targets.push(id.as_str());
        }
        if let Some(id) = &d.merge_id {
            targets.push(id.as_str());
        }
        if let Some(id) = review {
            targets.push(id);
            let mut parent = s
                .runs
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| r.retry_of.as_deref());
            while let Some(id) = parent {
                require(!targets.contains(&id), "Run 重试链存在环")?;
                targets.push(id);
                parent = s
                    .runs
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| r.retry_of.as_deref());
            }
        }
        let mut result = vec![];
        for r in s.governance.iter().filter(|r| {
            r.kind == "execution_specification" && targets.contains(&r.definition_id.as_str())
        }) {
            let refs: Vec<Id> = serde_json::from_value(r.data["spec_refs"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
            for id in refs {
                let spec = s
                    .definitions
                    .iter()
                    .find(|d| d.id == id && d.family == "spec")
                    .cloned()
                    .ok_or_else(|| err("not_found", "冻结 Spec 不存在"))?;
                result.push((
                    spec,
                    format!(
                        "workflow_step={} step={}",
                        r.data["workflow_step_instance_id"], r.data["step"]
                    ),
                ));
            }
        }
        require(!result.is_empty(), "v2 交付缺少冻结规范来源")?;
        Ok(Some(result))
    }
}
impl Engine {
    pub fn workflow_step_id(
        &self,
        s: &EngineeringState,
        owner: &str,
        action: &str,
        phase: Option<&str>,
    ) -> Result<Option<Id>> {
        let feature = s
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        let Some(lock) = s.feature_configuration(feature) else {
            return Ok(None);
        };
        if lock.compiler_version < 2 {
            return Ok(None);
        }
        let plan = if s.tasks.iter().any(|t| t.id == owner) {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let runtime = s
            .governance
            .iter()
            .find(|r| r.kind == "workflow_instance" && r.definition_id == owner)
            .ok_or_else(|| err("invalid_workflow", "运行流程不存在"))?
            .decode::<WorkflowRuntime>()?;
        let mut matches = runtime.steps.iter().filter(|r| {
            !["completed", "skipped", "failed"].contains(&r.status.as_str())
                && plan.steps.iter().any(|p| {
                    p.key == r.key
                        && p.kind == action
                        && phase.is_none_or(|phase| {
                            p.parameters.get("phase").and_then(Value::as_str) == Some(phase)
                        })
                })
        });
        let id = matches
            .next()
            .ok_or_else(|| err("workflow_blocked", "没有适用的冻结步骤"))?
            .id
            .clone();
        require(matches.next().is_none(), "多个步骤匹配，请指定步骤实例")?;
        Ok(Some(id))
    }
}

impl Engine {
    pub fn project_configuration_status(&self) -> Result<Value> {
        let s = self.store.snapshot()?;
        let Some(current) = s
            .governance
            .iter()
            .filter(|r| r.kind == "project_configuration")
            .max_by_key(|r| r.version)
        else {
            return Ok(
                json!({"ready":false,"issues":["请选择 Workflow 并绑定所需 Role 的 Agent"]}),
            );
        };
        let checked = (|| -> Result<()> {
            require(
                current.data["schema_version"] == 2,
                "旧配置需要显式补全或转换",
            )?;
            let scope = current.scope.clone();
            let wf = reference(
                &s,
                &text(&current.data, "workflow_ref")?,
                "workflow",
                &scope,
            )?;
            let mut compilation = compile(
                &s,
                wf,
                opt(&current.data, "standard_ref").as_deref(),
                &scope,
            )?;
            let bindings = serde_json::from_value(current.data["role_agent_bindings"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
            bind(&s, &mut compilation, &bindings, &scope)?;
            Ok(())
        })();
        Ok(match checked {
            Ok(()) => json!({"ready":true,"configuration_id":current.id,"issues":[]}),
            Err(e) => json!({"ready":false,"configuration_id":current.id,"issues":[e.message]}),
        })
    }
}
