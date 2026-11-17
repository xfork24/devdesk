//! Human manager lifecycle commands carry explicit owner/controller versions.
//! Successor snapshots are sealed through the same durable artifact protocol.
use crate::{
    concurrency::*,
    controller_definitions::controller_definition,
    governance::record,
    material_commands::material_parts,
    platform_facts::{one, restored},
    work_owners::WorkOwnerFacts,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
impl Engine {
    pub(crate) fn prepare_controller_turn_cancellation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        controller: &WorkflowController,
    ) -> Result<()> {
        if let Some(sid) = &controller.session_id {
            let active = one(self.store.as_ref(), "session_active_turns", sid)?;
            commit.witness([version(&active)])?;
            for raw in active
                .value
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                require(raw["session_id"] == *sid, "原经理轮次归属不符")?;
                let revision = raw["revision"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "原经理轮次版本缺失"))?;
                let key = ResourceKey::new("turn", text(raw, "id")?);
                commit.witness([ResourceVersion {
                    resource: key.clone(),
                    revision,
                }])?;
                if raw["cancel_requested"] != true {
                    let mut next = raw.clone();
                    next["cancel_requested"] = json!(true);
                    next["revision"] = json!(revision + 1);
                    if raw["status"] == "queued" {
                        next["status"] = json!("cancelled");
                        next["finished"] = json!(now());
                    }
                    commit.change(ResourceChange {
                        resource: key,
                        value: next,
                    })?;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn prepare_controller_lease(
        &self,
        commit: &mut WorkflowCommit<'_>,
        id: &str,
        controller: &WorkflowController,
        active: bool,
    ) -> Result<()> {
        let selected = one(self.store.as_ref(), "orchestration_lease", id)?;
        commit.witness([version(&selected)])?;
        let lease = OrchestrationLease {
            id: id.into(),
            controller_id: id.into(),
            owner_id: controller.owner_id.clone(),
            root_instance_id: if active {
                controller.root_instance_id.clone()
            } else {
                None
            },
            epoch: controller.epoch,
            expires_at: if active {
                now() + controller.policy.timeout_seconds * 1000
            } else {
                now()
            },
        };
        commit.change(ResourceChange {
            resource: selected.resource,
            value: json!(lease),
        })
    }
    pub(crate) fn controller_instance_current(
        &self,
        controller: &WorkflowController,
    ) -> Result<(bool, ReadSet)> {
        let catalog = self.store.configuration_snapshot()?;
        let mut reads = ReadSet::new(vec![ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: catalog.revision,
        }])?;
        let Some(sid) = &controller.session_id else {
            return Ok((true, reads));
        };
        let selected = one(self.store.as_ref(), "session", sid)?;
        reads = reads.merge([version(&selected)])?;
        let session: Session = serde_json::from_value(
            self.store.restore_runtime_value(
                selected
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原经理会话缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let Some(instance) = session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.agent_configuration.as_ref())
            .and_then(|a| catalog.record(&a.agent_instance_id))
        else {
            return Ok((false, reads));
        };
        Ok((
            !configuration_retired(&catalog.state, instance) && catalog.is_head(instance),
            reads,
        ))
    }
    fn prepare_controller_handoff(
        &self,
        commit: &mut WorkflowCommit<'_>,
        controller: &mut WorkflowController,
    ) -> Result<Option<(ArtifactSeal, String)>> {
        let Some(sid) = &controller.session_id else {
            return Ok(None);
        };
        let selected = one(self.store.as_ref(), "session", sid)?;
        let session: Session = serde_json::from_value(
            self.store.restore_runtime_value(
                selected
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原经理会话缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            session.project_id.as_deref() == Some(controller.project_id.as_str())
                && session.owner_id == controller.owner_id,
            "原经理会话归属不符",
        )?;
        let workers = one(self.store.as_ref(), "session_live_workers", sid)?;
        let inputs = one(self.store.as_ref(), "session_input_state", sid)?;
        require(
            workers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
                && inputs.value.as_ref().is_some_and(|v| v["active"] == false),
            "请先停止并核对原经理 worker 和轮次",
        )?;
        commit.witness([version(&selected), version(&workers), version(&inputs)])?;
        let facts = self.store.conversation_input_facts(&session)?;
        commit.witness(facts.read_set.versions().iter().cloned())?;
        let owner = WorkOwnerFacts::read(self.store.as_ref(), &controller.owner_id)?;
        commit.witness(owner.read_set.versions().iter().cloned())?;
        let id = commit.object_id("controller-handoff-snapshot")?;
        let (project_id, feature_id, task_id) = material_parts(&owner.material);
        let snapshot = Snapshot {
            id: id.clone(),
            project_id,
            feature_id,
            task_id,
            kind: "orchestration_handoff".into(),
            content: serde_json::to_string(&facts.content)
                .map_err(|e| err("serialization", e.to_string()))?,
            documents: facts.documents.clone(),
            inherited_from: session.snapshot_id.clone(),
            capture: SnapshotCapture {
                step_execution_ids: vec![],
                sessions: vec![session.id.clone()],
                captured_at: Some(now()),
                reason: "Actual human authorized manager successor".into(),
            },
            runtime_bindings: session.runtime_binding.iter().cloned().collect(),
            execution_instances: facts.execution_ids.clone(),
        };
        let body = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| err("serialization", e.to_string()))?;
        let seal = ArtifactSeal::prepare(
            commit.object_id("controller-handoff-artifact")?,
            &owner.material,
            "snapshots",
            &body,
            Origin::External,
        )?;
        let key = ResourceKey::new("snapshot", &id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: self.store.encode_runtime_value(&json!(snapshot))?,
        })?;
        if let Some(change) =
            self.captured_snapshot_experience(&facts, &session, &snapshot, &seal, None)?
        {
            commit.witness([ResourceVersion {
                resource: change.resource.clone(),
                revision: 0,
            }])?;
            commit.change(change)?;
        }
        controller.snapshot_id = Some(id);
        controller.predecessor_session_id = Some(session.id);
        controller.session_id = None;
        Ok(Some((seal, body)))
    }
    pub(crate) fn native_controller_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if !matches!(
            method,
            "manager.open"
                | "orchestration.open"
                | "orchestration.input"
                | "orchestration.takeover"
                | "orchestration.resume"
        ) {
            return Ok(None);
        }
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "经理生命周期和用户输入需要实际 Human 来源",
        )?;
        if let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)? {
            return Ok(Some(result));
        }
        if let Some(r) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(r.result));
        }
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少经理归属"))?;
        let mut commit = WorkflowCommit::new(self, method, c, context)?;
        let catalog = self.store.configuration_snapshot()?;
        commit.witness([ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: catalog.revision,
        }])?;
        let mut handoff = None;
        let mut approval = Value::Null;
        let (id, controller) = if matches!(method, "manager.open" | "orchestration.open") {
            let workspace_only = method == "manager.open";
            let root = if workspace_only {
                None
            } else {
                let selected = one(self.store.as_ref(), "governance", target)?;
                if selected
                    .value
                    .as_ref()
                    .is_some_and(|v| v["type"] == "workflow_instance")
                {
                    Some(WorkflowHeaderFacts::read(self, target)?)
                } else {
                    None
                }
            };
            let owner_id = root
                .as_ref()
                .map(|r| r.instance.owner_id.as_str())
                .unwrap_or(target);
            let owner = WorkOwnerFacts::read(self.store.as_ref(), owner_id)?;
            context.require_project(&owner.project)?;
            require(
                !workspace_only || owner.owner.resource.kind == "project",
                "经理工作台需要项目归属",
            )?;
            if let Some(root) = &root {
                root.guard_request(c, None)?;
                commit.witness([version(&root.header)])?;
            } else {
                owner.require_revision(c.expected_revision)?;
            }
            commit.witness(owner.read_set.versions().iter().cloned())?;
            let chosen = one(
                self.store.as_ref(),
                "controller_owner_source",
                &json!([owner_id, workspace_only]).to_string(),
            )?;
            commit.witness([version(&chosen)])?;
            let mut controller = controller_definition(
                &catalog.state,
                &owner.project,
                owner_id,
                root.as_ref()
                    .map(|r| (r.record.id.as_str(), r.instance.as_ref())),
                workspace_only,
                &c.payload,
            )?;
            let id = commit.object_id("workflow-controller")?;
            if chosen.value.is_some() {
                let previous = restored(self, &chosen)?;
                let mut actual: WorkflowController = previous.decode()?;
                let same = actual.agent_revision_id == controller.agent_revision_id
                    && actual.harness_permission_ref == controller.harness_permission_ref
                    && actual.role_ref == controller.role_ref
                    && actual.root_instance_id == controller.root_instance_id;
                let selected = ResourceSnapshot {
                    resource: ResourceKey::new("governance", &previous.id),
                    revision: chosen.revision,
                    value: chosen.value.clone(),
                };
                if same {
                    let result = json!({"id":previous.id,"reused":true,"controller":actual});
                    commit.witness([version(&selected)])?;
                    return commit.finish(fingerprint, result).map(Some);
                }
                if c.payload["controller_id"] != previous.id
                    || c.payload["expected_controller_revision"].as_u64() != Some(previous.revision)
                    || c.payload["epoch"].as_u64() != Some(actual.epoch)
                {
                    return Err(err("revision_conflict", "采用后继需要原经理版本和代次"));
                }
                require(actual.status != "active", "请先接管原经理再采用后继配置")?;
                handoff = self.prepare_controller_handoff(&mut commit, &mut actual)?;
                controller.snapshot_id = actual.snapshot_id.clone();
                controller.predecessor_session_id = actual.predecessor_session_id.clone();
                actual.epoch += 1;
                actual.status = "closed".into();
                commit.replace_record(&selected, previous.clone(), json!(actual))?;
                self.prepare_controller_lease(&mut commit, &previous.id, &actual, false)?;
                commit.new_record(record("controller_selection",id.clone(),Scope::project(&owner.project),"人类采用后继经理".into(),1,None,json!({"predecessor_controller_id":previous.id,"successor_controller_id":id,"actor":context.actor(),"agent_ref":controller.agent_revision_id,"harness_permission_ref":controller.harness_permission_ref}),true),"controller-selection")?;
                if let Some(root) = &root {
                    let selected = one(
                        self.store.as_ref(),
                        "workflow_driver_root",
                        &root.instance.root_instance_id,
                    )?;
                    if selected.value.is_some() {
                        let driver = restored(self, &selected)?;
                        if driver.data["mode"] == "agent" {
                            require(
                                driver.data["controller_id"] == previous.id,
                                "根控制权已变化",
                            )?;
                            let actual = ResourceSnapshot {
                                resource: ResourceKey::new("governance", &driver.id),
                                revision: selected.revision,
                                value: selected.value,
                            };
                            let mut data = driver.data.clone();
                            data["controller_id"] = json!(id);
                            data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
                            commit.replace_record(&actual, driver, data)?;
                        }
                    }
                }
            }
            let mut row = record(
                "workflow_controller",
                owner_id.into(),
                Scope::project(&owner.project),
                "工作经理".into(),
                1,
                None,
                json!(controller),
                false,
            );
            row.id = id.clone();
            commit.insert_record(&row)?;
            self.prepare_controller_lease(&mut commit, &id, &controller, true)?;
            (id, controller)
        } else {
            let selected = one(self.store.as_ref(), "governance", target)?;
            let row = restored(self, &selected)?;
            require(row.r#type == "workflow_controller", "不是经理控制器")?;
            let mut controller: WorkflowController = row.decode()?;
            context.require_project(&controller.project_id)?;
            if c.expected_revision != Some(selected.revision)
                || c.payload["epoch"].as_u64() != Some(controller.epoch)
            {
                return Err(err(
                    "revision_conflict",
                    "原经理版本或代次已变化，请保留输入并刷新",
                ));
            }
            require(controller.status != "closed", "已交接的经理不能恢复")?;
            if method == "orchestration.input" {
                require(
                    matches!(controller.status.as_str(), "active" | "budget_exhausted"),
                    "请先恢复经理再输入指令",
                )?;
                let input = text(&c.payload, "input")?;
                require(input.len() <= 65536, "用户输入超过预算")?;
                if matches!(context.actor(), WorkflowActor::Human)
                    && let Some(root) = controller.root_instance_id.as_deref()
                {
                    let pending = self.controller_approval_context(root)?;
                    let observed = c.payload["approval_context"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if pending.len() == 1 && observed.len() == 1 {
                        let decision = &pending[0];
                        let operation = decision["data"]["request"]["operation"]
                            .as_str()
                            .unwrap_or("");
                        if ["workflow.step.confirm", "workflow.step.accept"].contains(&operation)
                            && crate::controller_interactions::explicit_approval(
                                &input,
                                &decision["data"]["request"]["interaction"],
                            )
                        {
                            require(
                                observed[0]["id"] == decision["id"]
                                    && observed[0]["revision"] == decision["revision"],
                                "待审批事项已变化，请保留原消息并重新确认",
                            )?;
                            if decision["approval_source"] == "workflow_step" {
                                let owner = WorkflowHeaderFacts::read(
                                    self,
                                    &text(decision, "definition_id")?,
                                )?;
                                let mut payload = decision["data"]["request"]["arguments"].clone();
                                payload["reason"] = json!(input);
                                let resources =
                                    owner.step(self, &text(&payload, "execution_id")?)?;
                                require(
                                    payload["expected_step_revision"] == resources.step.revision,
                                    "原审批执行已变化",
                                )?;
                                let definition = resources
                                    .instance
                                    .plan
                                    .document
                                    .steps
                                    .iter()
                                    .find(|s| s.node_id == resources.execution.node_id)
                                    .ok_or_else(|| err("storage", "审批步骤合同缺失"))?;
                                let mut selection = crate::workflow_facts::FactSelection::default();
                                selection.rule(&definition.completion);
                                let facts =
                                    resources.selected_facts(self.store.as_ref(), &selection)?;
                                commit.witness(facts.reads.versions().iter().cloned())?;
                                self.prepare_native_confirmation(
                                    &mut commit,
                                    &resources,
                                    &payload,
                                    false,
                                    &facts.facts,
                                )?;
                                self.settle_matching_step_decisions(
                                    &mut commit,
                                    &resources,
                                    &input,
                                    None,
                                )?;
                                commit.decision(
                                    &owner,
                                    Some(&resources.execution),
                                    "confirm",
                                    &input,
                                )?;
                                approval = json!({"id":decision["id"],"status":"approved"});
                            } else {
                                let selected =
                                    one(self.store.as_ref(), "governance", &text(decision, "id")?)?;
                                require(decision["revision"] == selected.revision, "原决定已变化")?;
                                let row = restored(self, &selected)?;
                                approval = self.prepare_controller_decision(
                                    &mut commit,
                                    &selected,
                                    row,
                                    true,
                                    &input,
                                )?;
                            }
                        }
                    }
                }

                commit.new_record(
                    record(
                        "controller_input",
                        row.id.clone(),
                        row.scope.clone(),
                        "实际用户指令".into(),
                        1,
                        None,
                        json!({"input":input,"consumed":false,"actor":context.actor()}),
                        false,
                    ),
                    "controller-input",
                )?;
                controller.status = "active".into();
                controller.run_state = "runnable".into();
                controller.reason.clear();
                controller.wakeup_turns = 0;
            } else if method == "orchestration.takeover" {
                controller.epoch += 1;
                controller.status = "paused".into();
                controller.wakeup_turns = 0;
                controller.reason.clear();
                self.prepare_controller_turn_cancellation(&mut commit, &controller)?;
            } else {
                require(
                    c.payload.get("policy").is_none(),
                    "控制策略必须选择确切 Profile 源",
                )?;
                let root = controller
                    .root_instance_id
                    .as_ref()
                    .map(|id| WorkflowHeaderFacts::read(self, id))
                    .transpose()?;
                let mut payload = c.payload.clone();
                for (key, value) in [
                    ("role_ref", controller.role_ref.as_str()),
                    ("agent_ref", controller.agent_revision_id.as_str()),
                    (
                        "harness_permission_ref",
                        controller.harness_permission_ref.as_str(),
                    ),
                ] {
                    if payload.get(key).is_none() {
                        payload[key] = json!(value);
                    }
                }
                let mut next = controller_definition(
                    &catalog.state,
                    &controller.project_id,
                    &controller.owner_id,
                    root.as_ref()
                        .map(|r| (r.record.id.as_str(), r.instance.as_ref())),
                    controller.workspace_only,
                    &payload,
                )?;
                next.epoch = controller.epoch + 1;
                next.checkpoint = controller.checkpoint.clone();
                // Wakeup event cursors are monotonic for this Controller ID,
                // including failed-start recovery across epochs.
                next.event_cursor = controller.event_cursor;
                handoff = self.prepare_controller_handoff(&mut commit, &mut controller)?;
                next.predecessor_session_id = controller.predecessor_session_id.clone();
                next.snapshot_id = controller.snapshot_id.clone();
                if next.agent_revision_id != controller.agent_revision_id
                    || next.role_ref != controller.role_ref
                    || next.harness_permission_ref != controller.harness_permission_ref
                {
                    let next_id = commit.object_id("workflow-controller")?;
                    next.epoch = 1;
                    next.event_cursor = 0;
                    controller.epoch += 1;
                    controller.status = "closed".into();
                    commit.replace_record(&selected, row.clone(), json!(controller))?;
                    self.prepare_controller_lease(&mut commit, &row.id, &controller, false)?;
                    let mut next_row = record(
                        "workflow_controller",
                        next.owner_id.clone(),
                        row.scope.clone(),
                        row.name.clone(),
                        1,
                        None,
                        json!(next),
                        false,
                    );
                    next_row.id = next_id.clone();
                    commit.insert_record(&next_row)?;
                    self.prepare_controller_lease(&mut commit, &next_id, &next, true)?;
                    commit.new_record(record("controller_selection",next_id.clone(),row.scope.clone(),"人类采用后继经理".into(),1,None,json!({"predecessor_controller_id":row.id,"successor_controller_id":next_id,"actor":context.actor(),"agent_ref":next.agent_revision_id,"harness_permission_ref":next.harness_permission_ref}),true),"controller-selection")?;
                    if let Some(root) = &root {
                        let driver = one(
                            self.store.as_ref(),
                            "workflow_driver_root",
                            &root.instance.root_instance_id,
                        )?;
                        if driver
                            .value
                            .as_ref()
                            .is_some_and(|r| r["data"]["mode"] == "agent")
                        {
                            let actual = restored(self, &driver)?;
                            require(actual.data["controller_id"] == row.id, "根控制权已变化")?;
                            let selected = ResourceSnapshot {
                                resource: ResourceKey::new("governance", &actual.id),
                                revision: driver.revision,
                                value: driver.value,
                            };
                            let mut data = actual.data.clone();
                            data["controller_id"] = json!(next_id);
                            data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
                            commit.replace_record(&selected, actual, data)?;
                        } else {
                            commit.witness([version(&driver)])?;
                        }
                    }
                    let prepared =
                        commit.prepare(fingerprint, json!({"id":next_id,"controller":next}))?;
                    return if let Some((seal, body)) = handoff {
                        self.seal_command_changes(
                            prepared,
                            seal,
                            &body,
                            Scope::project(&next.project_id),
                        )
                        .map(Some)
                    } else {
                        self.store.commit_changes(&prepared).map(|r| Some(r.result))
                    };
                }
                controller = next;
            }
            commit.replace_record(&selected, row.clone(), json!(controller))?;
            self.prepare_controller_lease(
                &mut commit,
                &row.id,
                &controller,
                controller.status == "active",
            )?;
            (row.id, controller)
        };
        let scope = Scope::project(&controller.project_id);
        let prepared = commit.prepare(
            fingerprint,
            json!({"id":id,"controller":controller,"approval":approval}),
        )?;
        if let Some((seal, body)) = handoff {
            self.seal_command_changes(prepared, seal, &body, scope)
                .map(Some)
        } else {
            self.store.commit_changes(&prepared).map(|r| Some(r.result))
        }
    }
}
