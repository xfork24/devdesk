//! The actual controller/native call grants one precise configuration action.
//! Cross-store completion consumes that immutable grant and original sources.
use crate::{concurrency::*, governance::record, *};

pub(crate) struct ManagerPublication<'a> {
    pub call: &'a ConfigurationRecord,
    pub method: &'a str,
    pub params: &'a Value,
    pub change: &'a ConfigurationChangeSet,
    pub actor: &'a WorkflowActor,
    pub action: &'a str,
    pub fingerprint: &'a str,
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}

impl Engine {
    pub(crate) fn manager_call_authority(
        &self,
        controller: &WorkflowController,
        call: &ConfigurationRecord,
        method: &str,
        params: &Value,
    ) -> Result<ReadSet> {
        let actor = WorkflowActor::Controller {
            controller_id: call.definition_id.clone(),
            session_id: text(&call.data, "session_id")?,
            turn_id: text(&call.data, "turn_id")?,
            epoch: controller.epoch,
        };
        let context = InvocationContext::workflow(actor, Some(controller.project_id.clone()));
        let authority = context.configuration_authority(self.store.as_ref(), method)?;
        let keys = [
            ResourceKey::new("governance", &call.id),
            ResourceKey::new("governance", &call.definition_id),
            ResourceKey::new("tool_worker", text(&call.data, "worker_operation_id")?),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "经理原调用事实读取不完整",
        )?;
        let original = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(
                rows[0]
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "经理原调用缺失"))?,
            )?,
        )?;
        require(
            original.r#type == "controller_tool_call"
                && original.definition_id == call.definition_id
                && original.revision == call.revision
                && original.data == call.data
                && original.data["method"] == method
                && original.data["arguments"] == *params
                && matches!(
                    original.data["status"].as_str(),
                    Some("prepared" | "running")
                ),
            "经理调用不能替换原方法、参数或版本",
        )?;
        require(
            rows[1].value.as_ref().is_some_and(|r| {
                r["type"] == "workflow_controller" && r["data"] == json!(controller)
            }) && rows[2].revision == 1
                && rows[2].value.as_ref().is_some_and(|w| {
                    w["session_id"] == call.data["session_id"] && w["kind"] == "harness"
                }),
            "经理原控制器或 worker 已变化",
        )?;
        authority.merge(rows.iter().map(version))
    }

    pub(crate) fn commit_manager_configuration(
        &self,
        target: &Engine,
        request: ManagerPublication<'_>,
        selected: &ConfigurationCatalogSnapshot,
        authority: ReadSet,
    ) -> Result<Value> {
        if let Some(receipt) = target
            .store
            .replay_commit(request.action, request.fingerprint)?
        {
            return Ok(receipt.result);
        }
        let global = matches!(target.store.namespace(), StoreNamespace::Configuration);
        let key = ResourceKey::new("governance", format!("{}:authorization", request.action));
        let rows = self.store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "经理配置授权读取不完整",
        )?;
        let grant = if let Some(raw) = &rows[0].value {
            let grant = ConfigurationRecord::from_storage(self.store.restore_runtime_record(raw)?)?;
            require(
                grant.r#type == "manager_write_authorization"
                    && grant.definition_id == request.call.id
                    && grant.data["actor"] == json!(request.actor)
                    && grant.data["method"] == request.method
                    && grant.data["arguments_digest"] == request.fingerprint
                    && grant.data["target_root"] == target.store.root()
                    && grant.data["target_namespace"] == json!(target.store.namespace()),
                "经理配置授权不能替换原调用或目标库",
            )?;
            grant
        } else {
            let work = target.configuration_work_facts(request.change)?;
            let mut grant = record(
                "manager_write_authorization",
                request.call.id.clone(),
                Scope::project(
                    self.store
                        .project_id()
                        .ok_or_else(|| err("project_required", "经理授权缺少项目"))?,
                ),
                "经理配置写入资格".into(),
                1,
                None,
                json!({"actor":request.actor,"method":request.method,"arguments_digest":request.fingerprint,"global":global,"target_root":target.store.root(),"target_namespace":target.store.namespace(),
                    "target_catalog_revision":selected.revision,"authority":authority,"work_facts":work,"request_digest":configuration_hash(request.params),
                    "origin":{"kind":"native_worker","call_id":request.call.id,"session_id":request.call.data["session_id"],"turn_id":request.call.data["turn_id"],"worker_operation_id":request.call.data["worker_operation_id"]}}),
                true,
            );
            grant.id = key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("{}:authorization", request.action),
                fingerprint: configuration_hash(&(
                    "manager-write-grant/v2",
                    request.action,
                    request.fingerprint,
                )),
                read_set: authority.merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: key.clone(),
                    value: self.store.encode_runtime_record(&grant)?,
                }],
                result: json!({"id":key.id}),
                time: now() as i64,
            })?;
            grant
        };
        if grant.data["target_catalog_revision"] != selected.revision {
            return Err(err(
                "revision_conflict",
                "经理原配置候选已经变化，请保留草稿并重新预演",
            ));
        }
        let work = serde_json::from_value(grant.data["work_facts"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let preview = target.configuration_preview_with(
            &selected.state,
            request.change,
            request.actor,
            &work,
        )?;
        require(
            request.params["preview_hash"] == preview["preview_hash"],
            "配置或授权已变化，请重新预演",
        )?;
        let (mut next, _, _) = target.configuration_candidate_with(
            &selected.state,
            request.change,
            request.actor,
            &work,
        )?;
        register_template_members(&mut next, template_catalog_generation(&selected.state));
        let candidate =
            ConfigurationCatalogSnapshot::new(next, selected.revision, selected.repository_ready)?;
        let publication =
            prepare_configuration_publication(target.store.as_ref(), selected, &candidate)?;
        let gate = configuration_transfer_witness(target.store.as_ref())?;
        let mut read_set = publication.read_set.merge([gate])?;
        let mut changes = publication.changes;
        if !global {
            let original: ReadSet = serde_json::from_value(grant.data["authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            read_set = read_set
                .merge(original.versions().iter().cloned())?
                .merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 1,
                }])?;
        }
        for fact in work.values() {
            read_set = read_set.merge(fact.read_set.versions().iter().cloned())?;
            if fact.owner.resource.kind != "project" {
                let mut raw = fact
                    .owner
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "经理原工作对象缺失"))?;
                raw["revision"] = json!(fact.owner.revision + 1);
                changes.push(ResourceChange {
                    resource: fact.owner.resource.clone(),
                    value: raw,
                });
            }
        }
        let command = Command {
            command_id: request.action.into(),
            target: None,
            expected_revision: None,
            payload: json!({"changeset":request.change,"preview_hash":request.params["preview_hash"]}),
        };
        let mut context =
            InvocationContext::workflow(request.actor.clone(), self.store.project_id())
                .prepare_record(request.method, &command);
        context.value["data"]["configuration_authorization"] = json!({"id":grant.id,"origin_project":self.store.project_id(),"request_digest":request.fingerprint});
        // Re-hash the immutable audit row after attaching its exact grant.
        context.value["content_hash"] =
            json!(content_digest(&References::new(), &context.value["data"]));
        read_set = read_set.merge([ResourceVersion {
            resource: context.resource.clone(),
            revision: 0,
        }])?;
        changes.push(context);
        target.store.commit_changes(&PreparedChangeSet{command_id:request.action.into(),fingerprint:request.fingerprint.into(),read_set,changes,
            result:json!({"id":request.action,"accepted":true,"published_refs":preview["published_refs"],"selections":preview["selections"]}),time:now() as i64}).map(|r|r.result)
    }
}
