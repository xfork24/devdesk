//! Human conversation control and provider admissions share native Turn CAS.
use crate::{concurrency::*, *};

fn fact(store: &dyn ports::EngineeringStore, key: ResourceKey) -> Result<ResourceSnapshot> {
    let mut rows = store.read_resources(std::slice::from_ref(&key))?;
    require(
        rows.len() == 1 && rows[0].resource == key,
        "会话控制读取不完整",
    )?;
    Ok(rows.remove(0))
}

pub(crate) fn control(
    store: &dyn ports::EngineeringStore,
    method: &str,
    c: &Command,
    context: &InvocationContext,
) -> Result<Option<Value>> {
    if !matches!(method, "conversation.cancel" | "interaction.answer") {
        return Ok(None);
    }
    require(!c.command_id.is_empty(), "command_id 不能为空")?;
    require(
        matches!(context.actor(), WorkflowActor::Human),
        "会话人工控制需要实际 Human 来源",
    )?;
    let fingerprint = configuration_hash(&("conversation-control/v1", method, c, context));
    if let Some(receipt) = store.replay_commit(&c.command_id, &fingerprint)? {
        return Ok(Some(receipt.result));
    }
    let target = c
        .target
        .as_deref()
        .ok_or_else(|| err("invalid_params", "会话控制缺少目标"))?;
    let root = fact(
        store,
        ResourceKey::new(
            if method == "interaction.answer" {
                "interaction"
            } else {
                "turn"
            },
            target,
        ),
    )?;
    let value = root
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "会话控制目标不存在"))?;
    if c.expected_revision != Some(root.revision) {
        return Err(err(
            "revision_conflict",
            "原编辑版本已变化，请保留草稿并刷新",
        ));
    }
    let tid = if method == "interaction.answer" {
        text(value, "turn_id")?
    } else {
        target.into()
    };
    let sid = text(value, "session_id")?;
    let mut keys = vec![
        ResourceKey::new("session", &sid),
        ResourceKey::new("turn", &tid),
        ResourceKey::new("turn_pending_interactions", &tid),
    ];
    if method == "interaction.answer" {
        keys.push(root.resource.clone());
    }
    let rows = store.read_resources(&keys)?;
    require(
        rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
        "会话控制关联事实不完整",
    )?;
    let session = rows[0]
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "会话不存在"))?;
    let mut turn = rows[1]
        .value
        .clone()
        .ok_or_else(|| err("not_found", "轮次不存在"))?;
    let project = text(&turn, "project_id")?;
    let global = session["scope"] == "global" && project == GLOBAL_DISCUSSION_NAMESPACE;
    context.require_scope(if global { None } else { Some(project.as_str()) })?;
    require(
        session["project_id"] == project
            && turn["session_id"] == sid
            && store.project_id().is_none_or(|id| id == project),
        "会话控制跨项目",
    )?;
    let pending = rows[2]
        .value
        .as_ref()
        .and_then(Value::as_array)
        .ok_or_else(|| err("storage", "缺少待处理交互事实"))?;
    let status: TurnStatus = serde_json::from_value(turn["status"].clone())
        .map_err(|e| err("storage", e.to_string()))?;
    require(status.active(), "轮次已经结束")?;
    let mut guards = rows
        .iter()
        .map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        })
        .collect::<Vec<_>>();
    let mut changes = Vec::new();
    let result = if method == "conversation.cancel" {
        if rows[1].revision != root.revision {
            return Err(err("revision_conflict", "原轮次已变化"));
        }
        turn["cancel_requested"] = json!(true);
        let binding = fact(store, ResourceKey::new("controller_session_source", &sid))?;
        guards.push(ResourceVersion {
            resource: binding.resource.clone(),
            revision: binding.revision,
        });
        if let Some(raw) = &binding.value {
            let id = text(&raw["data"], "controller_id")?;
            let selected = fact(store, ResourceKey::new("governance", &id))?;
            guards.push(ResourceVersion {
                resource: selected.resource.clone(),
                revision: selected.revision,
            });
            if let Some(raw_controller) = &selected.value {
                let mut row = ConfigurationRecord::from_storage(raw_controller.clone())?;
                if row.r#type == "workflow_controller"
                    && row.data["session_id"] == sid
                    && row.data["epoch"] == raw["data"]["epoch"]
                {
                    let mut data = row.data.clone();
                    data["status"] = json!("paused");
                    data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
                    data["wakeup_turns"] = json!(0);
                    data["run_state"] = json!("waiting");
                    data["reason"] = json!("用户停止经理轮次，已撤销本轮控制资格");
                    crate::governance::update_record(&mut row, data);
                    changes.push(ResourceChange {
                        resource: selected.resource,
                        value: store.encode_runtime_record(&row)?,
                    });
                }
            }
        }

        if status == TurnStatus::Queued {
            turn["status"] = json!(TurnStatus::Cancelled);
            turn["finished"] = json!(now());
        }
        for raw in pending {
            require(
                raw["session_id"] == sid && raw["turn_id"] == tid && raw["project_id"] == project,
                "待处理交互跨会话",
            )?;
            let key = ResourceKey::new("interaction", text(raw, "id")?);
            let revision = raw["revision"]
                .as_u64()
                .ok_or_else(|| err("storage", "交互缺少版本"))?;
            let mut next = raw.clone();
            next["status"] = json!(InteractionStatus::Cancelled);
            next["answered_at"] = json!(now());
            next["revision"] = json!(revision + 1);
            guards.push(ResourceVersion {
                resource: key.clone(),
                revision,
            });
            changes.push(ResourceChange {
                resource: key,
                value: next,
            });
        }
        json!({"id":target,"cancel_requested":true})
    } else {
        if rows[3].revision != root.revision {
            return Err(err("revision_conflict", "原交互已变化"));
        }
        let mut interaction: PendingInteraction = serde_json::from_value(
            rows[3]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原交互缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            interaction.session_id == sid
                && interaction.turn_id == tid
                && interaction.project_id == project
                && interaction.status == InteractionStatus::Pending
                && status == TurnStatus::WaitingInteraction
                && turn["cancel_requested"] != true,
            "交互已结束、已取消或不再等待回答",
        )?;
        require(
            c.payload["session_id"] == sid && c.payload["turn_id"] == tid,
            "回答 Session/Turn 不匹配",
        )?;
        let answer: InteractionAnswer = serde_json::from_value(c.payload["answer"].clone())
            .map_err(|_| err("invalid_params", "无效交互回答"))?;
        interaction.validate_answer(&answer)?;
        interaction.answer = Some(answer);
        interaction.status = InteractionStatus::Answered;
        interaction.answered_at = Some(now());
        interaction.revision += 1;
        turn["status"] = json!(if pending.iter().any(|i| i["id"] != target) {
            TurnStatus::WaitingInteraction
        } else {
            TurnStatus::Running
        });
        changes.push(ResourceChange {
            resource: root.resource,
            value: json!(interaction),
        });
        json!({"id":target,"accepted":true})
    };
    turn["revision"] = json!(rows[1].revision + 1);
    changes.push(ResourceChange {
        resource: keys[1].clone(),
        value: turn,
    });
    let origin = context.prepare_record(method, c);
    guards.push(ResourceVersion {
        resource: origin.resource.clone(),
        revision: 0,
    });
    changes.push(origin);
    store
        .commit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint,
            read_set: ReadSet::new(guards)?,
            changes,
            result,
            time: now() as i64,
        })
        .map(|r| Some(r.result))
}
