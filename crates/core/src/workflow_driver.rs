//! Rust and Harness drivers use the same explicit control kernel and persisted worker intents.
use crate::governance::{record, reference, update_record};
use crate::*;

pub(crate) fn validate_selection(
    s: &EngineeringState,
    value: &Value,
    scope: &Scope,
) -> Result<OrchestrationSelection> {
    let selection: OrchestrationSelection = serde_json::from_value(value.clone())
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    selection.validate()?;
    if let Some(id) = &selection.policy_ref {
        reference(s, id, "orchestration_policy", scope)?
            .decode::<OrchestrationPolicy>()?
            .validate()?;
    }
    Ok(selection)
}
fn driver<'a>(s: &'a EngineeringState, root: &str) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .find(|r| r.r#type == "workflow_driver" && r.definition_id == root)
}
fn operation(method: &str) -> &str {
    method.rsplit('.').next().unwrap_or(method)
}
impl Engine {
    pub(crate) fn initialize_workflow_driver(
        &self,
        s: &mut EngineeringState,
        root: &ConfigurationRecord,
        value: &Value,
    ) -> Result<()> {
        if value.is_null() || value["mode"] == "manual" {
            return Ok(());
        }
        let c = Command {
            command_id: format!("{}:driver", root.id),
            target: Some(root.id.clone()),
            expected_revision: Some(root.revision),
            payload: value.clone(),
        };
        self.workflow_driver_command("workflow.driver.select", &c, s, &WorkflowActor::Human)?;
        Ok(())
    }
    pub(crate) fn workflow_driver_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Option<Value>> {
        if method != "workflow.driver.select" {
            return Ok(None);
        }
        require(
            matches!(actor, WorkflowActor::Human),
            "调度模式切换需要真实人类入口",
        )?;
        let root = s.configuration(c.target.as_deref().unwrap_or(""))?.clone();
        require(
            root.r#type == "workflow_instance" && root.data["root_instance_id"] == root.id,
            "只能为根实例选择驱动",
        )?;
        require(c.expected_revision == Some(root.revision), "流程修订冲突")?;
        require(
            !["completed", "cancelled"].contains(&root.data["status"].as_str().unwrap_or("")),
            "流程已结束",
        )?;
        let selection = validate_selection(
            s,
            &json!({"mode":c.payload["mode"],"policy_ref":c.payload["policy_ref"]}),
            &root.scope,
        )?;
        let previous = driver(s, &root.id).cloned();
        let epoch = previous
            .as_ref()
            .and_then(|r| r.data["epoch"].as_u64())
            .unwrap_or(0)
            + 1;
        // Revoke old controller calls and segments, while preserving every business worker.
        let controllers: Vec<_> = s
            .governance
            .iter()
            .filter(|r| {
                r.r#type == "workflow_controller"
                    && r.data["workspace_only"] != true
                    && (r.data["root_instance_id"] == root.id
                        || r.data["owner_id"] == root.data["owner_id"])
            })
            .cloned()
            .collect();
        for controller in &controllers {
            if selection.mode == "agent" {
                continue;
            }
            let mut data = controller.data.clone();
            data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
            data["status"] = json!("paused");
            data["reason"] = json!("调度控制已切换；业务 worker 保留");
            for turn in s.turns.iter_mut().filter(|t| {
                Some(t.session_id.as_str()) == controller.data["session_id"].as_str()
                    && t.status.active()
            }) {
                turn.cancel_requested = true;
                turn.revision += 1;
            }
            update_record(
                s.governance
                    .iter_mut()
                    .find(|r| r.id == controller.id)
                    .unwrap(),
                data,
            );
            for intent in s.governance.iter_mut().filter(|r| {
                r.r#type == "workflow_intent"
                    && r.definition_id == controller.id
                    && r.data["status"] == "prepared"
            }) {
                let mut data = intent.data.clone();
                data["status"] = json!("revoked");
                update_record(intent, data);
            }
        }
        let members: std::collections::BTreeSet<_> = s
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance" && r.data["root_instance_id"] == root.id)
            .map(|r| r.id.clone())
            .collect();
        for segment in s.governance.iter_mut().filter(|r| {
            r.r#type == "workflow_segment"
                && r.data["status"] == "active"
                && members.contains(&r.definition_id)
        }) {
            let mut data = segment.data.clone();
            data["status"] = json!("stopped");
            data["reason"] = json!("驱动切换");
            update_record(segment, data);
        }
        let controller_id = if selection.mode == "agent" {
            let opened = self
                .controller_command(
                    "orchestration.open",
                    &Command {
                        command_id: format!("{}:controller", c.command_id),
                        target: Some(root.id.clone()),
                        expected_revision: None,
                        payload: json!({}),
                    },
                    s,
                )?
                .ok_or_else(|| err("configuration_required", "无法创建 Agent 驱动"))?;
            let id = text(&opened, "id")?;
            let row = s.configuration(&id)?.clone();
            if row.data["status"] != "active" {
                self.controller_command(
                    "orchestration.resume",
                    &Command {
                        command_id: format!("{}:resume", c.command_id),
                        target: Some(id.clone()),
                        expected_revision: Some(row.revision),
                        payload: json!({"epoch":row.data["epoch"]}),
                    },
                    s,
                )?;
            }
            Some(id)
        } else {
            None
        };
        let policy = selection
            .policy_ref
            .as_deref()
            .map(|id| s.configuration(id)?.decode::<OrchestrationPolicy>())
            .transpose()?;
        let data = json!({"mode":selection.mode,"policy_ref":selection.policy_ref,"policy":policy,"epoch":epoch,
            "controller_id":controller_id,"expires_at":now()+policy.as_ref().map_or(60,|p|p.lease_seconds)*1000,"reason":""});
        let id = if let Some(previous) = previous {
            update_record(
                s.governance
                    .iter_mut()
                    .find(|r| r.id == previous.id)
                    .unwrap(),
                data.clone(),
            );
            previous.id
        } else {
            let mut row = record(
                "workflow_driver",
                root.id.clone(),
                root.scope.clone(),
                "工作流驱动".into(),
                1,
                None,
                data.clone(),
                false,
            );
            row.id = format!("workflow-driver:{}", root.id);
            let id = row.id.clone();
            s.governance.push(row);
            id
        };
        event(
            s,
            &root.id,
            "workflow.driver.selected",
            &json!({"driver_id":id,"epoch":epoch,"mode":selection.mode}).to_string(),
        );
        Ok(Some(json!({"id":id,"driver":data})))
    }
    pub(crate) fn authorize_driver_action(
        &self,
        s: &EngineeringState,
        method: &str,
        c: &Command,
        actor: &WorkflowActor,
    ) -> Result<()> {
        if !method.starts_with("workflow.") {
            return Ok(());
        }
        let Some(instance) = c
            .target
            .as_deref()
            .and_then(|id| s.configuration(id).ok())
            .filter(|r| r.r#type == "workflow_instance")
        else {
            return Ok(());
        };
        let Some(driver) = driver(
            s,
            instance.data["root_instance_id"]
                .as_str()
                .unwrap_or(&instance.id),
        ) else {
            return require(
                !matches!(actor, WorkflowActor::Driver { .. }),
                "Rust 驱动授权已撤销",
            );
        };
        match actor {
            WorkflowActor::Human => Ok(()),
            WorkflowActor::Driver { driver_id, epoch } => {
                require(
                    driver.id == *driver_id
                        && driver.data["mode"] == "rust"
                        && driver.data["epoch"] == *epoch
                        && driver.data["expires_at"].as_u64().unwrap_or(0) >= now(),
                    "驱动租约已失效",
                )?;
                let instance: ConfiguredWorkflowInstance = instance.decode()?;
                let context = s.configuration(
                    instance
                        .plan
                        .permission_context_ref
                        .as_deref()
                        .ok_or_else(|| err("permission_unconfigured", "缺少控制策略"))?,
                )?;
                let policy: WorkflowControlPolicy =
                    serde_json::from_value(context.data["control_policy"].clone())
                        .map_err(|e| err("invalid_configuration", e.to_string()))?;
                policy.permits(operation(method))?;
                require(
                    !policy.human_operations.contains(operation(method)),
                    "该操作要求人类决定",
                )
            }
            WorkflowActor::Controller { controller_id, .. } => require(
                driver.data["mode"] == "agent" && driver.data["controller_id"] == *controller_id,
                "当前根实例控制权属于其他驱动",
            ),
            WorkflowActor::Segment { authorization_id } => {
                let segment = s.configuration(authorization_id)?;
                require(
                    segment.data["status"] == "active" && driver.data["mode"] == "agent",
                    "委托已撤销或当前模式不允许委托",
                )
            }
        }
    }
    pub fn workflow_driver_query(&self, p: &Value) -> Result<Value> {
        let s = self.store.snapshot()?;
        let instance = s.configuration(&text(p, "id")?)?;
        require(instance.r#type == "workflow_instance", "需要流程实例")?;
        let root = instance.data["root_instance_id"]
            .as_str()
            .unwrap_or(&instance.id);
        let waiting:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_instance" && r.data["root_instance_id"]==root)
            .flat_map(|r|r.data["steps"].as_array().into_iter().flatten().filter(|e|["waiting_input","waiting_confirmation","failed","needs_reconciliation"].contains(&e["status"].as_str().unwrap_or("")))
                .map(|e|json!({"instance_id":r.id,"execution_id":e["id"],"step":e["step"],"status":e["status"],"reason":e["reason"]}))).collect();
        Ok(
            json!({"waiting":waiting,"driver":driver(&s,root),"root_instance_id":root,"children_inherit":true}),
        )
    }
    pub fn drive_rust_workflows(&self) -> Result<()> {
        let initial = self.store.snapshot()?;
        let drivers: Vec<_> = initial
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_driver" && r.data["mode"] == "rust")
            .map(|r| r.id.clone())
            .collect();
        for id in drivers {
            let row = initial.configuration(&id)?;
            if initial.configuration(&row.definition_id).is_ok_and(|r| {
                ["completed", "cancelled"].contains(&r.data["status"].as_str().unwrap_or(""))
            }) {
                continue;
            }
            let policy: OrchestrationPolicy = serde_json::from_value(row.data["policy"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            if row.data["expires_at"].as_u64().unwrap_or(0) < now() + policy.lease_seconds * 500 {
                self.store.transact(None, &mut |s| {
                    let old = s.configuration(&id)?.clone();
                    if old.data["mode"] != "rust" {
                        return Ok(Value::Null);
                    }
                    let mut d = old.data.clone();
                    if d["expires_at"].as_u64().unwrap_or(0) < now() {
                        d["epoch"] = json!(d["epoch"].as_u64().unwrap_or(0) + 1);
                    }
                    d["expires_at"] = json!(now() + policy.lease_seconds * 1000);
                    update_record(s.governance.iter_mut().find(|r| r.id == id).unwrap(), d);
                    Ok(Value::Null)
                })?;
            }
            for _ in 0..policy.max_actions_per_tick {
                let s = self.store.snapshot()?;
                let driver = s.configuration(&id)?;
                if driver.data["mode"] != "rust" || driver.data["policy"] != json!(policy) {
                    break;
                }
                let root = &driver.definition_id;
                let active = s
                    .governance
                    .iter()
                    .filter(|r| {
                        r.r#type == "workflow_instance" && r.data["root_instance_id"] == *root
                    })
                    .map(|r| {
                        r.data["steps"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|e| {
                                e["status"] == "running"
                                    && r.data["plan"]["document"]["steps"].as_array().is_some_and(
                                        |steps| {
                                            steps.iter().any(|step| {
                                                step["key"] == e["step"]
                                                    && ["agent", "tool"].contains(
                                                        &step["executor"]["type"]
                                                            .as_str()
                                                            .unwrap_or(""),
                                                    )
                                            })
                                        },
                                    )
                            })
                            .count()
                    })
                    .sum::<usize>();
                let mut rows: Vec<_> = s
                    .governance
                    .iter()
                    .filter(|r| {
                        r.r#type == "workflow_instance"
                            && r.data["root_instance_id"] == *root
                            && !["completed", "cancelled"]
                                .contains(&r.data["status"].as_str().unwrap_or(""))
                    })
                    .collect();
                // Children complete before parents; dependencies determine the remaining order.
                rows.sort_by_key(|r| (r.id == *root, r.id.clone()));
                let mut action = None;
                for instance in rows {
                    if s.workflow_instance_suspended(&instance.id) {
                        continue;
                    }
                    let mut ready = self
                        .workflow_control_query(
                            "workflow.inspect",
                            &json!({"id":instance.id,"limit":64}),
                        )
                        .unwrap()?;
                    let mut steps = ready["steps"].as_array().cloned().unwrap_or_default();
                    let mut next = ready["page"]["next_offset"].as_u64();
                    while let Some(offset) = next {
                        let page = self
                            .workflow_control_query(
                                "workflow.inspect",
                                &json!({"id":instance.id,"offset":offset,"limit":64}),
                            )
                            .unwrap()?;
                        require(
                            page["runtime"]["active_plan_revision_id"]
                                == instance.data["active_plan_revision_id"],
                            "就绪集合读取期间计划已变化",
                        )?;
                        steps.extend(page["steps"].as_array().into_iter().flatten().cloned());
                        next = page["page"]["next_offset"].as_u64();
                    }
                    ready["steps"] = json!(steps);
                    let context = instance.data["plan"]["permission_context_ref"]
                        .as_str()
                        .and_then(|id| s.configuration(id).ok());
                    let control = context
                        .map(|r| {
                            serde_json::from_value::<WorkflowControlPolicy>(
                                r.data["control_policy"].clone(),
                            )
                        })
                        .transpose()
                        .map_err(|e| err("storage", e.to_string()))?;
                    let permits = |op: &str| {
                        control.as_ref().is_some_and(|p| {
                            p.allowed_operations.contains(op) && !p.human_operations.contains(op)
                        })
                    };
                    let mut selected = None;
                    for step in ready["steps"].as_array().into_iter().flatten() {
                        let status = step["execution"]["status"].as_str().unwrap_or("");
                        let execution = &step["execution"];
                        let retry_code = execution["tool_call_id"]
                            .as_str()
                            .and_then(|id| s.configuration(id).ok())
                            .and_then(|r| r.data["error"]["code"].as_str());
                        if status == "failed"
                            && permits("retry")
                            && execution["sequence"].as_u64().unwrap_or(1) <= policy.max_retries
                            && retry_code.is_some_and(|code| {
                                code != "needs_reconciliation"
                                    && policy.retry_error_codes.contains(code)
                            })
                        {
                            selected = Some(("workflow.step.retry", step.clone()));
                            break;
                        }
                        if permits("accept")
                            && ["awaiting_acceptance", "waiting_confirmation"].contains(&status)
                            && step["human_satisfied"] == true
                            && (step["completion_satisfied"] == true
                                || step["completion_overridden"] == true)
                        {
                            selected = Some(("workflow.step.accept", step.clone()));
                            break;
                        }
                        if permits("dispatch") && step["dispatchable"] == true {
                            let contract = instance.data["plan"]["document"]["steps"]
                                .as_array()
                                .and_then(|a| a.iter().find(|d| d["key"] == step["step"]));
                            let uses_worker = contract.is_some_and(|d| {
                                ["agent", "tool"]
                                    .contains(&d["executor"]["type"].as_str().unwrap_or(""))
                            });
                            if !uses_worker || active < policy.max_concurrency {
                                selected = Some(("workflow.step.dispatch", step.clone()));
                                break;
                            }
                        }
                    }
                    if let Some((method, step)) = selected {
                        let execution = &step["execution"];
                        action = Some((
                            method,
                            Command {
                                command_id: format!(
                                    "rust:{}:{}:{}:{}:{}",
                                    id,
                                    driver.data["epoch"],
                                    method,
                                    execution["id"].as_str().unwrap_or(""),
                                    instance.data["control_version"]
                                ),
                                target: Some(instance.id.clone()),
                                expected_revision: Some(instance.revision),
                                payload: json!({"plan_revision_id":instance.data["active_plan_revision_id"],"control_version":instance.data["control_version"],"step":step["step"],"execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"],"reason":"Rust 驱动从共同就绪集合选择"}),
                            },
                        ));
                        break;
                    }
                    if permits("complete")
                        && ready["allowed_actions"]
                            .as_array()
                            .is_some_and(|v| v.contains(&json!("complete")))
                    {
                        action = Some((
                            "workflow.complete",
                            Command {
                                command_id: format!(
                                    "rust:{}:{}:complete:{}",
                                    id, driver.data["epoch"], instance.id
                                ),
                                target: Some(instance.id.clone()),
                                expected_revision: Some(instance.revision),
                                payload: json!({"plan_revision_id":instance.data["active_plan_revision_id"],"reason":"Rust 驱动完成"}),
                            },
                        ));
                        break;
                    }
                }
                let Some((method, c)) = action else {
                    break;
                };
                let context = InvocationContext::workflow(
                    WorkflowActor::Driver {
                        driver_id: id.clone(),
                        epoch: driver.data["epoch"].as_u64().unwrap_or(0),
                    },
                    s.projects.first().map(|p| p.id.clone()),
                );
                // This transaction persists the decision and any worker intent before external I/O.
                match self.execute_in_context(method, c, &context) {
                    Ok(_) => {}
                    Err(error) => {
                        self.store.transact(None, &mut |state| {
                            let old = state.configuration(&id)?.clone();
                            let mut data = old.data.clone();
                            let reason = json!({"code":error.code,"message":error.message});
                            if data["reason"] != reason {
                                data["reason"] = reason;
                                update_record(
                                    state.governance.iter_mut().find(|r| r.id == id).unwrap(),
                                    data,
                                );
                            }
                            Ok(Value::Null)
                        })?;
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}
