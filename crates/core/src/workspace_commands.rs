//! Workspace metadata and new work are admitted from explicit native facts.
//! A Feature and its frozen initial plan are created in one atomic commit.
use crate::{
    concurrency::*, governance::record, material_commands::stable_object_id, platform_facts::one,
    work_owners::WorkOwnerFacts, workflow_commits::WorkflowCommit,
    workflow_start::queued_execution, *,
};
use std::collections::BTreeMap;
impl Engine {
    pub(crate) fn native_workspace_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if !matches!(method, "project.update" | "feature.create") {
            return Ok(None);
        }
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "项目和 Feature 编辑需要实际 Human 来源",
        )?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        require(c.payload.get("scenario").is_none(), "不再支持模拟场景参数")?;
        let project = if method == "feature.create" {
            text(&c.payload, "project_id")?
        } else {
            c.target
                .clone()
                .ok_or_else(|| err("invalid_params", "缺少 Project"))?
        };
        let owner = WorkOwnerFacts::read(self.store.as_ref(), &project)?;
        require(owner.owner.resource.kind == "project", "需要 Project 归属")?;
        context.require_project(&project)?;
        let mut commit = WorkflowCommit::new(self, method, c, context)?;
        commit.witness(owner.read_set.versions().iter().cloned())?;
        if method == "project.update" {
            owner.require_revision(c.expected_revision)?;
            require(
                c.payload.as_object().is_some_and(|fields| {
                    fields.keys().all(|key| {
                        ["title", "workspace", "concurrency", "workspace_definition"]
                            .contains(&key.as_str())
                    })
                }),
                "Project 更新包含未知字段",
            )?;
            let mut next: Project = serde_json::from_value(owner.owner.value.clone().unwrap())
                .map_err(|e| err("storage", e.to_string()))?;
            if c.payload.get("title").is_some() {
                next.title = text(&c.payload, "title")?;
            }
            if let Some(value) = c.payload.get("concurrency") {
                let value = value
                    .as_u64()
                    .filter(|v| (1..=32).contains(v))
                    .ok_or_else(|| err("invalid_params", "并发额度需为 1 至 32"))?;
                next.concurrency = value as u32;
            }
            if let Some(value) = c.payload.get("workspace_definition") {
                let workspace: Workspace = serde_json::from_value(value.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                require(!workspace.mainline.trim().is_empty(), "mainline 不可为空")?;
                next.workspace = workspace.path.clone();
                next.workspace_definition = Some(workspace);
            } else if c.payload.get("workspace").is_some() {
                next.workspace = text(&c.payload, "workspace")?;
                if let Some(workspace) = &mut next.workspace_definition {
                    workspace.path = next.workspace.clone();
                }
            }
            let binding = one(self.store.as_ref(), "repository_project_state", &project)?;
            let features = one(self.store.as_ref(), "project_features_empty", &project)?;
            let actual: Project = serde_json::from_value(owner.owner.value.clone().unwrap())
                .map_err(|e| err("storage", e.to_string()))?;
            if binding
                .value
                .as_ref()
                .is_some_and(|v| v["worktree_exists"] == true)
                || features.value.as_ref().is_some_and(|v| v["empty"] == false)
            {
                require(
                    next.workspace == actual.workspace
                        && json!(next.workspace_definition) == json!(actual.workspace_definition),
                    "已有仓库或 Feature 的项目不能原地切换绑定",
                )?;
            }
            commit.witness([
                crate::workflow_resources::version(&binding),
                crate::workflow_resources::version(&features),
            ])?;
            next.revision += 1;
            commit.change(ResourceChange {
                resource: owner.owner.resource.clone(),
                value: json!(next),
            })?;
            return commit
                .finish(fingerprint, json!({"id":project,"revision":next.revision}))
                .map(Some);
        }
        let repository = one(self.store.as_ref(), "repository_project_state", &project)?;
        require(
            repository
                .value
                .as_ref()
                .is_some_and(|r| r["connect_pending"] == false),
            "仓库登记尚未完成或需要核对",
        )?;
        commit.witness([crate::workflow_resources::version(&repository)])?;
        let id = stable_object_id(self.store.as_ref(), &c.command_id, "feature")?;
        let predecessor = opt(&c.payload, "predecessor");
        if let Some(previous) = &predecessor {
            let selected = one(self.store.as_ref(), "feature", previous)?;
            require(
                selected
                    .value
                    .as_ref()
                    .is_some_and(|r| r["project_id"] == project),
                "前序 Feature 跨项目",
            )?;
            commit.witness([crate::workflow_resources::version(&selected)])?;
        }
        let materials: Vec<Id> = serde_json::from_value(
            c.payload
                .get("source_materials")
                .cloned()
                .unwrap_or(json!([])),
        )
        .map_err(|e| err("invalid_params", e.to_string()))?;
        require(materials.len() <= 256, "Feature 输入材料超出预算")?;
        for material in &materials {
            let selected = one(self.store.as_ref(), "document", material)?;
            require(
                selected
                    .value
                    .as_ref()
                    .is_some_and(|d| d["project_id"] == project),
                "Feature 输入材料必须属于该项目",
            )?;
            commit.witness([crate::workflow_resources::version(&selected)])?;
        }
        let before = self.store.configuration_snapshot()?;
        let mut catalog = before.state.as_ref().clone();
        let current = catalog
            .governance
            .iter()
            .filter(|r| r.r#type == "project_configuration" && r.definition_id == project)
            .max_by_key(|r| r.version)
            .cloned()
            .ok_or_else(|| {
                err(
                    "configuration_required",
                    "请先选择 Project 的 Workflow、Standard 和 Agent 组合",
                )
            })?;
        if let Some(expected) = c.payload["configuration_revision_id"].as_str() {
            require(
                expected == current.id,
                "Project 默认配置已变化，请保留创建表单并刷新",
            )?;
        }
        let mut selection = current.data.clone();
        for key in [
            "standard_ref",
            "workflow_ref",
            "role_agent_bindings",
            "harness_permission_ref",
            "orchestration",
        ] {
            if let Some(value) = c.payload.get(key) {
                selection[key] = value.clone();
            }
        }
        selection["source_configuration_ref"] = json!(current.id);
        selection["inputs"] = c
            .payload
            .get("workflow_inputs")
            .cloned()
            .unwrap_or(json!({}));
        let scope = Scope::feature(&project, &id);
        let configuration = crate::standards::select_work_configuration_for(
            &mut catalog,
            &WorkReference::Feature { id: id.clone() },
            &scope,
            &selection,
            None,
        )?;
        let selected = crate::standards::selection_state(&catalog, &configuration.data);
        let workflow = selected
            .configuration(&text(&configuration.data, "workflow_ref")?)?
            .clone();
        let bindings = serde_json::from_value(configuration.data["role_agent_bindings"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let mut plan =
            self.compile_configured_workflow(&selected, &workflow, &scope, Some(&bindings))?;
        plan.standard_ref = Some(text(&configuration.data, "standard_ref")?);
        plan.work_configuration_ref = Some(configuration.id.clone());
        let mut inputs: BTreeMap<String, Value> =
            serde_json::from_value(selection["inputs"].clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
        if plan.document.inputs.contains_key("test_configuration") {
            let tests = configuration
                .data
                .get("test_configuration")
                .filter(|v| !v.is_null())
                .ok_or_else(|| err("configuration_required", "该流程需要项目测试执行配置"))?;
            require(
                inputs.get("test_configuration").is_none_or(|v| v == tests),
                "不能覆盖冻结项目测试配置",
            )?;
            inputs.insert("test_configuration".into(), tests.clone());
        }
        self.validate_workflow_inputs(&catalog, &plan, &inputs)?;
        let start = c.payload["start_workflow"] != false;
        let feature = Feature {
            id: id.clone(),
            project_id: project.clone(),
            title: text(&c.payload, "title")?,
            status: if start {
                Status::Running
            } else {
                Status::Draft
            },
            revision: 1,
            input_version: 1,
            predecessor,
            source_materials: materials,
        };
        let user_request = json!({"feature_id":id,"title":feature.title,"initial_requirement":c.payload["initial_requirement"].as_str().unwrap_or("")});
        if c.payload.get("initial_requirement").is_some()
            || c.payload.get("workflow_inputs").is_some()
        {
            commit.new_record(record("feature_request",id.clone(),scope.clone(),"用户初始需求".into(),1,None,json!({"actor":context.actor(),"workflow_inputs":inputs,"user_request":user_request}),true),"feature-request")?;
        }
        let mut root = None;
        if start {
            let instance_id =
                stable_object_id(self.store.as_ref(), &c.command_id, "feature-workflow")?;
            let steps = plan
                .document
                .steps
                .iter()
                .map(|step| {
                    Ok(queued_execution(
                        &instance_id,
                        &workflow.id,
                        step,
                        stable_object_id(
                            self.store.as_ref(),
                            &c.command_id,
                            &format!("feature-step:{}", step.node_id),
                        )?,
                        format!("/{}", step.key),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut instance = ConfiguredWorkflowInstance {
                active_plan_revision_id: String::new(),
                control_version: 1,
                schema_version: WORKFLOW_INSTANCE_SCHEMA,
                owner_id: id.clone(),
                workflow_ref: workflow.id.clone(),
                project_id: project.clone(),
                plan,
                role_agent_bindings: bindings,
                inputs,
                user_request,
                human_governance: false,
                steps,
                status: StepExecutionStatus::Queued,
                paused: false,
                parent_execution_id: None,
                root_instance_id: instance_id.clone(),
                cancel_requested: false,
            };
            let mut frozen = crate::orchestration::freeze_plan_record(
                &mut catalog,
                &mut instance,
                &scope,
                "initial",
            )?;
            let old = frozen.id.clone();
            frozen.id = stable_object_id(self.store.as_ref(), &c.command_id, "feature-plan")?;
            instance.active_plan_revision_id = frozen.id.clone();
            for e in &mut instance.steps {
                if e.plan_revision_id == old {
                    e.plan_revision_id = frozen.id.clone();
                }
            }
            let mut row = record(
                "workflow_instance",
                id.clone(),
                scope.clone(),
                workflow.name,
                1,
                None,
                json!(instance),
                false,
            );
            row.id = instance_id;
            root = Some((row, instance, frozen));
        }
        let candidate =
            ConfigurationCatalogSnapshot::new(catalog, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        commit.witness(publication.read_set.versions().iter().cloned())?;
        for change in publication.changes {
            commit.change(change)?;
        }
        if let Some((row, instance, frozen)) = &root {
            for row in [frozen, row] {
                let key = ResourceKey::new("governance", &row.id);
                commit.witness([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
                commit.change(ResourceChange {
                    resource: key,
                    value: self.store.encode_configuration_record(&candidate, row)?,
                })?;
            }
            self.prepare_initial_native_driver(
                &mut commit,
                row,
                instance,
                &configuration.data["orchestration"],
                &candidate,
            )?;
        }
        let head = one(self.store.as_ref(), "head", &format!("main:{project}"))?;
        commit.witness([crate::workflow_resources::version(&head)])?;
        let base = head
            .value
            .as_ref()
            .and_then(|h| h["revision"].as_str())
            .unwrap_or("")
            .to_owned();
        let context = Context {
            id: stable_object_id(self.store.as_ref(), &c.command_id, "feature-context")?,
            owner_id: id.clone(),
            content: "Feature 初始上下文".into(),
            revision: 1,
        };
        for (kind, key, value) in [
            ("feature", id.clone(), json!(feature)),
            ("context", context.id.clone(), json!(context)),
            (
                "head",
                id.clone(),
                json!(GitHead {
                    id: id.clone(),
                    project_id: project.clone(),
                    revision: base.clone()
                }),
            ),
        ] {
            let key = ResourceKey::new(kind, key);
            commit.witness([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            commit.change(ResourceChange {
                resource: key,
                value,
            })?;
        }
        if !base.is_empty() {
            self.prepare_native_worktree_intent(
                &mut commit,
                &project,
                &id,
                WorktreeKind::Feature,
                &base,
            )?;
        }
        commit.finish(fingerprint, json!({"id":id})).map(Some)
    }
}
