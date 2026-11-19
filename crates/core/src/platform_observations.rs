//! Platform requests are admitted from the literal provider journal against
//! actual worker/Turn/step/controller leases. Rejections are kernel facts.
use crate::{
    concurrency::*,
    conversation_observations::checkpoint,
    governance::record,
    platform_tools::PLATFORM_HANDLERS,
    provider_tools::ProviderPoll,
    workflow_resources::{WorkflowResources, live_workflow_chain, version},
    *,
};

impl Engine {
    pub(crate) fn admit_platform_provider_observation(
        &self,
        poll: &ProviderPoll,
        observations: &[ports::ConversationObservation],
        command: &str,
        fingerprint: &str,
    ) -> Result<Value> {
        poll.require_store(self.store.as_ref())?;
        require(observations.len() == 1, "平台原生调用按原观察逐项接纳")?;
        if let Some(receipt) = self.store.replay_commit(command, fingerprint)? {
            return Ok(receipt.result);
        }
        let observation = &observations[0];
        let ports::ConversationObservation::ToolCall {
            tool,
            call,
            request,
            arguments,
            ..
        } = observation
        else {
            return Err(err("invalid_params", "缺少平台工具观察"));
        };
        let binding = poll
            .session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("blocked", "原平台调用缺少运行绑定"))?;
        let configuration = binding
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("blocked", "原平台调用缺少 Agent 身份"))?;
        let definition = configuration
            .integration_lock
            .as_ref()
            .and_then(|l| {
                l.platform_tools.iter().find(|r| {
                    r.decode::<ToolDefinition>()
                        .is_ok_and(|d| d.call_name() == *tool)
                })
            })
            .ok_or_else(|| err("tool_not_authorized", "工具不属于原冻结 Capability"))?;
        let parsed: ToolDefinition = definition.decode()?;
        let journal = poll
            .journal
            .as_ref()
            .ok_or_else(|| err("storage", "原供应商观察尚未保存"))?;
        let mut keys = poll
            .guards()?
            .versions()
            .iter()
            .map(|v| v.resource.clone())
            .collect::<Vec<_>>();
        keys.push(ResourceKey::new("turn", &poll.turn.id));
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "原平台调用事实不完整",
        )?;
        for expected in poll.guards()?.versions() {
            require(
                rows.iter()
                    .any(|r| r.resource == expected.resource && r.revision == expected.revision),
                "原平台调用的会话、轮次或 worker 已变化",
            )?;
        }
        let turn = rows
            .last()
            .unwrap()
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原平台调用轮次缺失"))?;
        require(
            turn["revision"] == poll.turn.revision
                && turn["status"] == "running"
                && turn["cancel_requested"] == false,
            "原平台调用轮次不再运行",
        )?;
        self.validate_provider_call_scope(poll, turn, observation)?;
        let raw = rows
            .iter()
            .find(|r| r.resource == journal.resource)
            .and_then(|r| r.value.as_ref())
            .ok_or_else(|| err("not_found", "原供应商观察缺失"))?;
        require(
            raw["type"] == "provider_poll_observation"
                && raw["data"]["value"]["observations"]
                    .as_array()
                    .is_some_and(|items| items.contains(&json!(observation))),
            "平台调用不能替换供应商字面观察",
        )?;
        let mut reads = poll.guards()?.merge([version(rows.last().unwrap())])?;
        let live = poll
            .authority
            .live(self.store.as_ref(), &poll.session)?
            .ok_or_else(|| err("workflow_tool_suspended", "原执行暂停，调用观察保留原依据"))?;
        reads = reads.merge(live)?;
        let catalog = self.store.configuration_snapshot()?;
        let exact = catalog.state.configuration(&definition.id)?;
        require(
            exact.content_hash == definition.content_hash
                && json!(exact.decode::<ToolDefinition>()?) == json!(parsed),
            "平台调用不能改变原工具源",
        )?;
        let scope = Scope::project(&poll.turn.project_id);
        let mut changes = Vec::new();
        let permissions = binding
            .permissions
            .as_ref()
            .ok_or_else(|| err("permission_unconfigured", "原平台调用缺少权限"))?;
        let permission_result = (|| -> Result<()> {
            self.tool_executor
                .as_ref()
                .ok_or_else(|| err("unsupported", "缺少参数校验器"))?
                .validate_arguments(&parsed.input_schema, arguments)?;
            for policy in [&permissions.harness.policy, &permissions.sandbox.policy] {
                if let ToolPermission::Restricted { allow, deny } = &policy.tools {
                    require(
                        !deny.contains(&definition.id)
                            && (allow.is_empty() || allow.contains(&definition.id)),
                        "Permission 未允许该平台工具",
                    )?;
                }
            }
            Ok(())
        })();
        if let Some((_, controller_id, epoch)) = &poll.authority.controller {
            let keys = [
                ResourceKey::new("governance", controller_id),
                ResourceKey::new(
                    "controller_lease_active",
                    json!([controller_id, epoch]).to_string(),
                ),
            ];
            let facts = self.store.read_resources(&keys)?;
            require(
                facts.len() == 2 && facts.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "Controller 授权事实不完整",
            )?;
            let row = ConfigurationRecord::from_storage(
                facts[0]
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原 Controller 缺失"))?,
            )?;
            let row: ConfigurationRecord =
                serde_json::from_value(self.store.restore_runtime_record(&row.to_storage())?)
                    .map_err(|e| err("storage", e.to_string()))?;
            let controller: WorkflowController = row.decode()?;
            let rejection = permission_result
                .and_then(|_| {
                    require(
                        row.r#type == "workflow_controller"
                            && controller.status == "active"
                            && controller.epoch == *epoch
                            && controller.session_id.as_deref() == Some(poll.session.id.as_str())
                            && controller.project_id == poll.turn.project_id,
                        "Controller 原会话授权已撤销",
                    )?;
                    crate::configuration::project_manager_role(
                        &catalog.state,
                        &controller.role_ref,
                    )?;
                    let instance = catalog
                        .state
                        .configuration(&configuration.agent_instance_id)?;
                    require(
                        !configuration_retired(&catalog.state, instance)
                            && catalog.is_head(instance),
                        "经理实例已换绑或移除",
                    )?;
                    require(facts[1].revision == 1, "原 Controller 租约已过期")?;
                    require(
                        if controller.workspace_only {
                            MANAGER_TOOLS.contains(&parsed.qualified_name.as_str())
                        } else {
                            CONTROL_TOOLS.contains(&parsed.qualified_name.as_str())
                                || MANAGER_TOOLS.contains(&parsed.qualified_name.as_str())
                        },
                        "原 Controller 未声明此工具",
                    )?;
                    Ok(())
                })
                .err();
            reads = reads
                .merge(facts.iter().map(version))?
                .merge([ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog.revision,
                }])?;
            let id = format!(
                "controller-call-{}",
                configuration_hash(&(
                    poll.session.id.as_str(),
                    poll.turn.id.as_str(),
                    poll.worker.as_str(),
                    call.as_str()
                ))
            );
            let key = ResourceKey::new("governance", &id);
            let previous = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .remove(0);
            if let Some(raw) = &previous.value {
                let previous = self.store.restore_runtime_record(raw)?;
                require(
                    previous["type"] == "controller_tool_call"
                        && previous["data"]["arguments"] == *arguments
                        && previous["data"]["request_id"] == *request
                        && previous["data"]["tool_ref"] == definition.id,
                    "Controller 调用身份被不同请求复用",
                )?;
            } else {
                let mut row = record(
                    "controller_tool_call",
                    controller_id.clone(),
                    row.scope,
                    "原生 Controller 工具调用".into(),
                    1,
                    None,
                    json!({"status":if rejection.is_some(){"failed"}else{"prepared"},"method":parsed.qualified_name,"arguments":arguments,"session_id":poll.session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker,"request_id":request,"call_id":call,"tool_ref":definition.id,"epoch":epoch,"error":rejection,
                    "provider_basis":poll.basis(),"provider_poll_id":journal.resource.id,"origin":{"kind":"native_worker","session_id":poll.session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker}}),
                    false,
                );
                row.id = id;
                reads = reads.merge([version(&previous)])?;
                changes.push(ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&row)?,
                });
                let mut work = record(
                    "controller_reply_work",
                    row.id.clone(),
                    row.scope.clone(),
                    "经理调用回复任务".into(),
                    1,
                    None,
                    json!({"status":"pending","call_id":row.id}),
                    false,
                );
                work.id = format!("controller-reply-work:{}", row.id);
                let key = ResourceKey::new("governance", &work.id);
                reads = reads.merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
                changes.push(ResourceChange {
                    resource: key,
                    value: work.to_storage(),
                });
            }
        } else {
            let authority = poll
                .authority
                .workflow
                .as_ref()
                .ok_or_else(|| err("tool_not_authorized", "平台调用缺少实际 Workflow 归属"))?;
            let resources = WorkflowResources::read(
                self.store.as_ref(),
                authority
                    .instances
                    .first()
                    .ok_or_else(|| err("storage", "原工作流实例缺失"))?,
                &authority.execution_id,
            )?;
            let (actual, guards) = live_workflow_chain(
                self.store.as_ref(),
                &resources.record.id,
                &resources.execution.id,
                StepExecutionStatus::Running,
            )?;
            require(
                actual == *authority
                    && resources.execution.session_id.as_deref() == Some(poll.session.id.as_str()),
                "平台调用原步骤或 Session 已改变",
            )?;
            reads = reads
                .merge(resources.reads.versions().iter().cloned())?
                .merge(guards.versions().iter().cloned())?;
            let id = format!(
                "platform-call-{}",
                configuration_hash(&json!([poll.session.id, poll.turn.id, poll.worker, call]))
            );
            let source_id = format!("agent-platform-request:{id}");
            let source_key = ResourceKey::new("governance", &source_id);
            let previous = self
                .store
                .read_resources(std::slice::from_ref(&source_key))?
                .remove(0);
            let platform = PlatformToolRequest {
                definition_ref: Some(definition.id.clone()),
                execution_id: id.clone(),
                instance_id: resources.record.id.clone(),
                owner_id: resources.instance.owner_id.clone(),
                project_id: resources.instance.project_id.clone(),
                definition: parsed.clone(),
                arguments: arguments.clone(),
                runtime: None,
                permissions: resources
                    .instance
                    .plan
                    .permission_locks
                    .get(&definition.id)
                    .cloned(),
                caller: Some(crate::platform_tools::AgentPlatformCaller {
                    execution_id: resources.execution.id.clone(),
                    session_id: poll.session.id.clone(),
                    turn_id: poll.turn.id.clone(),
                    operation_id: poll.worker.clone(),
                    request_id: request.clone(),
                    call_id: call.clone(),
                    tool_ref: definition.id.clone(),
                }),
            };
            if let Some(raw) = &previous.value {
                let previous = self.store.restore_runtime_record(raw)?;
                require(
                    previous["type"] == "agent_platform_request"
                        && previous["data"]["request"] == json!(platform),
                    "原生平台调用身份被不同请求复用",
                )?;
            } else {
                let rejection = permission_result
                    .and_then(|_| {
                        require(
                            PLATFORM_HANDLERS.contains(&parsed.qualified_name.as_str()),
                            "平台处理器不可用",
                        )?;
                        require(
                            permissions.binding_lock.as_ref().is_some_and(|l| {
                                l.harness
                                    .profile
                                    .policy
                                    .tool_refs
                                    .as_ref()
                                    .is_none_or(|refs| refs.contains(&definition.id))
                            }),
                            "平台工具不属于原步骤权限",
                        )?;
                        for policy in [&permissions.harness.policy, &permissions.sandbox.policy] {
                            if ![
                                "spec.lint",
                                "workspace.current",
                                "materials.capture",
                                "tests.run",
                                "workflow.outputs.read",
                            ]
                            .contains(&parsed.qualified_name.as_str())
                            {
                                require(
                                    policy.filesystem != FilesystemPermission::ReadOnly
                                        && permissions.approval == ApprovalPolicy::Never,
                                    "此权限不能直接执行平台写操作",
                                )?;
                            }
                            if parsed.qualified_name == "git.publish" {
                                require(
                                    policy.network == NetworkPermission::Enabled,
                                    "Permission 禁止远端发布",
                                )?;
                            }
                        }
                        Ok(())
                    })
                    .err();
                let mut source = record(
                    "agent_platform_request",
                    id.clone(),
                    resources.record.scope.clone(),
                    definition.name.clone(),
                    1,
                    None,
                    json!({"request":platform,"provider_basis":poll.basis(),"conversation_authority":poll.authority,"provider_poll_id":journal.resource.id,"origin":{"kind":"native_worker","session_id":poll.session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker}}),
                    true,
                );
                source.id = source_id;
                reads = reads.merge([version(&previous)])?;
                changes.push(ResourceChange {
                    resource: source_key,
                    value: self.store.encode_runtime_record(&source)?,
                });
                let (kind, key, row) = if let Some(error) = rejection {
                    let mut row = record(
                        "platform_call",
                        id.clone(),
                        resources.record.scope.clone(),
                        parsed.qualified_name.clone(),
                        1,
                        None,
                        json!({"status":"failed","request":platform,"error":error,"remote_effect":"not_applied","issuer":"kernel_platform"}),
                        false,
                    );
                    row.id = id.clone();
                    ("call", ResourceKey::new("governance", id), row)
                } else {
                    let mut row = record(
                        "workflow_intent",
                        resources.execution.id.clone(),
                        resources.record.scope.clone(),
                        "workflow.tool.execute".into(),
                        1,
                        None,
                        json!({"status":"prepared","configured":true,"execution_authority":authority,"owner_id":resources.instance.owner_id,"method":"workflow.tool.execute","command":Command{command_id:id.clone(),target:Some(resources.instance.owner_id.clone()),expected_revision:None,payload:json!({"execution_id":resources.execution.id,"agent_platform_call_id":id})},"provider_request_id":source.id}),
                        false,
                    );
                    row.id = format!("agent-platform-intent:{id}");
                    ("intent", ResourceKey::new("governance", &row.id), row)
                };
                let _ = kind;
                reads = reads.merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
                changes.push(ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&row)?,
                });
                let mut reply = record(
                    "platform_reply_work",
                    platform.execution_id.clone(),
                    source.scope.clone(),
                    "等待平台回执交付".into(),
                    1,
                    None,
                    json!({"status":"pending","request":platform,"source_id":source.id}),
                    false,
                );
                reply.id = format!("platform-reply-work:{}", platform.execution_id);
                let key = ResourceKey::new("governance", &reply.id);
                reads = reads.merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
                changes.push(ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&reply)?,
                });
            }
        }
        let result = checkpoint(turn);
        let key = ResourceKey::new(
            "governance",
            format!(
                "provider-batch:{}",
                configuration_hash(&(command, fingerprint))
            ),
        );
        let mut ack = record(
            "provider_observation_batch",
            poll.turn.id.clone(),
            scope,
            "平台观察接纳".into(),
            1,
            None,
            json!({"poll_id":journal.resource.id,"basis":poll.basis(),"checkpoint":result,"fingerprint_digest":fingerprint}),
            true,
        );
        ack.id = key.id.clone();
        reads = reads.merge([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        changes.push(ResourceChange {
            resource: key,
            value: ack.to_storage(),
        });
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: command.into(),
                fingerprint: fingerprint.into(),
                read_set: reads,
                changes,
                result,
                time: now() as i64,
            })
            .map(|r| r.result)
    }
}
