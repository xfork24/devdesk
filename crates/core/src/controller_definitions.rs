//! Controller configuration is derived from the complete source catalog and
//! explicit actual work/root facts. It does not inspect runtime history.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    platform_facts::{one, restored},
    workflow_commits::WorkflowCommit,
    workflow_resources::version,
    *,
};

pub(crate) fn controller_definition(
    catalog: &EngineeringState,
    project: &str,
    owner: &str,
    root: Option<(&str, &ConfiguredWorkflowInstance)>,
    workspace_only: bool,
    p: &Value,
) -> Result<WorkflowController> {
    require(
        p.get("policy").is_none(),
        "控制授权必须选择不可变 Permission Profile",
    )?;
    let defaults = current_template_defaults(catalog).unwrap_or(Value::Null);
    let config = catalog
        .governance
        .iter()
        .filter(|r| r.r#type == "project_configuration" && r.definition_id == project)
        .max_by_key(|r| r.version);
    let pending_work = if !workspace_only && root.is_none() {
        crate::standards::work_configuration(catalog, owner)
    } else {
        None
    };
    let declared_role = if workspace_only {
        None
    } else if let Some((_, instance)) = root {
        instance.plan.document.orchestrator_role_ref.as_deref()
    } else {
        pending_work
            .or(config)
            .and_then(|selection| selection.data["workflow_ref"].as_str())
            .and_then(|reference| catalog.configuration(reference).ok())
            .and_then(|workflow| workflow.data["orchestrator_role_ref"].as_str())
    };
    if let Some(declared_role) = declared_role {
        require(
            p["role_ref"]
                .as_str()
                .is_none_or(|role| role == declared_role),
            "编排 Role 由工作流修订指定",
        )?;
    }
    let role = declared_role
        .or(p["role_ref"].as_str().or_else(|| {
            workspace_only
                .then(|| {
                    p["agent_ref"]
                        .as_str()
                        .and_then(|id| catalog.configuration(id).ok())
                        .and_then(|r| r.refs["role_ref"].as_str())
                })
                .flatten()
        }))
        .or_else(|| config.and_then(|r| r.data["orchestrator_role_ref"].as_str()))
        .or_else(|| defaults["orchestrator_role_ref"].as_str())
        .ok_or_else(|| err("configuration_required", "项目需要明确编排 Role"))?;
    let role = crate::configuration::project_manager_role(catalog, role)?;
    let agent = p["agent_ref"]
        .as_str()
        .or_else(|| {
            root.and_then(|(_, instance)| {
                instance
                    .role_agent_bindings
                    .get(&role.id)
                    .map(String::as_str)
            })
        })
        .or_else(|| pending_work.and_then(|r| r.data["role_agent_bindings"][&role.id].as_str()))
        .or_else(|| {
            if root.is_some() && declared_role.is_some() {
                None
            } else {
                config.and_then(|r| r.data["role_agent_bindings"][&role.id].as_str())
            }
        })
        .or_else(|| {
            workspace_only
                .then(|| defaults["role_agent_bindings"][&role.id].as_str())
                .flatten()
        })
        .ok_or_else(|| err("configuration_required", "编排 Role 尚未绑定 Agent"))?;
    let agent = catalog.configuration(agent)?;
    let composition = catalog.resolve_agent(&agent.id)?;
    require(
        composition.role == role.definition_id
            && !configuration_retired(catalog, agent)
            && template_record_current(catalog, agent),
        "编排 Agent 角色不匹配或已撤销",
    )?;
    require(
        driver_supports(
            catalog.configuration(&composition.harness_ref)?.data["provider"]
                .as_str()
                .unwrap_or(""),
            "orchestration",
        ),
        "此 Harness 尚不支持编排原生工具桥",
    )?;
    crate::governance::validate_composition(catalog, &composition, &Scope::project(project))?;
    let profile = p["harness_permission_ref"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            permission_selection(catalog)
                .and_then(|selection| {
                    resolve_permission_lock(catalog, &composition.harness_ref, &selection)
                })
                .ok()
                .map(|l| l.harness.profile.id)
        })
        .ok_or_else(|| err("permission_unconfigured", "请选择经理授权 Profile"))?;
    crate::governance::reference(
        catalog,
        &profile,
        "permission_profile",
        &Scope::project(project),
    )?;
    let supplied = if root.is_none() && p["budget"].is_object() {
        Some(
            serde_json::from_value::<AgentSchedulingBudget>(p["budget"].clone())
                .map_err(|e| err("invalid_params", e.to_string()))?,
        )
    } else {
        None
    };
    let budget = root
        .map(|(_, r)| &r.plan.document.scheduling.agent)
        .or(supplied.as_ref());
    let policy = profile_control_policy(catalog, &profile, budget)?;
    policy.validate()?;
    Ok(WorkflowController {
        workspace_only,
        project_id: project.into(),
        owner_id: owner.into(),
        root_instance_id: root.map(|(id, _)| id.into()),
        agent_revision_id: agent.id.clone(),
        role_ref: role.id.clone(),
        harness_permission_ref: profile,
        policy_digest: configuration_hash(&policy),
        policy,
        session_id: None,
        predecessor_session_id: None,
        snapshot_id: None,
        epoch: 1,
        status: "active".into(),
        event_cursor: 0,
        wakeup_turns: 0,
        source_cursor: 0,
        acknowledged_cursor: 0,
        delivery_id: String::new(),
        run_state: "runnable".into(),
        checkpoint: Value::Null,
        wait_deadline: None,
        wait_turn_id: None,
        bootstrap_epoch: 0,
        reason: String::new(),
    })
}
impl Engine {
    pub(crate) fn prepare_initial_native_driver(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &ConfigurationRecord,
        instance: &ConfiguredWorkflowInstance,
        value: &Value,
        catalog: &ConfigurationCatalogSnapshot,
    ) -> Result<()> {
        if value.is_null() || value["mode"] == "manual" {
            return Ok(());
        }
        let selection: OrchestrationSelection = serde_json::from_value(value.clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        selection.validate()?;
        let controller = if selection.mode == "agent" {
            Some(self.prepare_native_root_controller(commit, root, instance, catalog)?)
        } else {
            None
        };
        let policy = instance.plan.document.scheduling.clone();
        let mut row = record(
            "workflow_driver",
            root.id.clone(),
            root.scope.clone(),
            "工作流驱动".into(),
            1,
            None,
            json!({"mode":selection.mode,"policy":policy,"epoch":1,"controller_id":controller,"expires_at":now()+policy.lease_seconds*1000,"reason":"","origin":commit.context.actor()}),
            false,
        );
        row.id = format!("workflow-driver:{}", root.id);
        commit.insert_record(&row)
    }
    pub(crate) fn prepare_native_root_controller(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &ConfigurationRecord,
        instance: &ConfiguredWorkflowInstance,
        catalog: &ConfigurationCatalogSnapshot,
    ) -> Result<Id> {
        let selected = one(
            self.store.as_ref(),
            "controller_owner_source",
            &json!([instance.owner_id, false]).to_string(),
        )?;
        commit.witness([version(&selected)])?;
        if selected.value.is_some() {
            let row = restored(self, &selected)?;
            let mut controller: WorkflowController = row.decode()?;
            require(
                controller.project_id == instance.project_id
                    && controller.owner_id == instance.owner_id
                    && controller.status == "active"
                    && controller
                        .root_instance_id
                        .as_ref()
                        .is_none_or(|id| id == &root.id),
                "已有经理需要先通过原版本生命周期建立后继",
            )?;
            require(
                instance
                    .plan
                    .document
                    .orchestrator_role_ref
                    .as_ref()
                    .is_none_or(|role| role == &controller.role_ref),
                "已有控制器的 Role 与工作流指定的编排 Role 不一致",
            )?;
            if controller.root_instance_id.is_none() {
                controller.root_instance_id = Some(root.id.clone());
                let actual = ResourceSnapshot {
                    resource: ResourceKey::new("governance", &row.id),
                    revision: selected.revision,
                    value: selected.value.clone(),
                };
                commit.replace_record(&actual, row.clone(), json!(controller))?;
                let lease = one(self.store.as_ref(), "orchestration_lease", &row.id)?;
                let mut value = lease
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原经理租约缺失"))?;
                value["root_instance_id"] = json!(root.id);
                commit.witness([version(&lease)])?;
                commit.change(ResourceChange {
                    resource: lease.resource,
                    value,
                })?;
            }
            return Ok(row.id);
        }
        let controller = controller_definition(
            &catalog.state,
            &instance.project_id,
            &instance.owner_id,
            Some((&root.id, instance)),
            false,
            &json!({}),
        )?;
        let mut row = record(
            "workflow_controller",
            instance.owner_id.clone(),
            Scope::project(&instance.project_id),
            "流程经理".into(),
            1,
            None,
            json!(controller),
            false,
        );
        row.id = stable_object_id(
            self.store.as_ref(),
            &commit.command.command_id,
            "workflow-controller",
        )?;
        commit.insert_record(&row)?;
        let lease = OrchestrationLease {
            id: row.id.clone(),
            controller_id: row.id.clone(),
            owner_id: controller.owner_id,
            root_instance_id: controller.root_instance_id,
            epoch: controller.epoch,
            expires_at: now() + controller.policy.timeout_seconds * 1000,
        };
        let key = ResourceKey::new("orchestration_lease", &row.id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: json!(lease),
        })?;
        Ok(row.id)
    }
}
