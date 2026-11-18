//! Select a finite set of native tool rows and restore their exact frozen views.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{concurrency::*, tool_calls::*};

pub(super) fn encode(
    catalog: &ConfigurationCatalogSnapshot,
    record: &ConfigurationRecord,
) -> Result<Value> {
    require(!source_store::is_source(record), "运行编码不能发布配置源")?;
    source_store::pack_record(&catalog.state, record).map(|row| row.to_storage())
}
pub(super) fn restore(catalog: &ConfigurationCatalogSnapshot, value: &Value) -> Result<Value> {
    let mut row = ConfigurationRecord::from_storage(value.clone())?;
    require(!source_store::is_source(&row), "运行恢复不能替换配置源")?;
    // A frozen plan is a pure projection of its exact descriptor and this
    // immutable catalog snapshot. Do not compile it again for every status
    // read. Runtime facts and permission-backend enforcement are never cached.
    let plan = if matches!(
        row.r#type.as_str(),
        "workflow_instance" | "workflow_plan_revision"
    ) && row.data["plan"]["$source"] == "workflow_plan"
    {
        let descriptor = row.data.as_object_mut().unwrap().remove("plan").unwrap();
        Some(
            catalog.cached_query(format!("frozen-plan:{descriptor}"), || {
                let _span = performance::Span::new("source.restore_plan.compile");
                devdesk_core::frozen_views::restore_plan(
                    &catalog.state,
                    &descriptor,
                    &mut Default::default(),
                )
            })?,
        )
    } else {
        None
    };
    row.data = source_views::restore_with(&catalog.state, &row.data, &mut Default::default())?;
    if let Some(plan) = plan {
        row.data["plan"] = plan;
    }
    row.content_hash = content_digest(&row.refs, &row.data);
    Ok(json!(row))
}

pub(super) fn provider_intents(pool: &ConnectionPool, after: Option<&str>) -> Result<ToolCallPage> {
    provider_page(
        pool,
        after,
        "provider_tool_intent",
        "idx_provider_tool_prepared",
    )
}
pub(super) fn provider_polls(pool: &ConnectionPool, after: Option<&str>) -> Result<ToolCallPage> {
    provider_page(
        pool,
        after,
        "provider_poll_pending",
        "idx_provider_poll_prepared",
    )
}
fn provider_page(
    pool: &ConnectionPool,
    after: Option<&str>,
    kind: &str,
    index: &str,
) -> Result<ToolCallPage> {
    let conn = pool.read()?;
    let sql = format!(
        "SELECT id FROM governance_records INDEXED BY {index} WHERE type='{kind}' AND json_extract(data,'$.status')='prepared' AND id>?1 ORDER BY id LIMIT 17"
    );
    let mut query = conn.prepare_cached(&sql).map_err(storage)?;
    let mut ids = query
        .query_map([after.unwrap_or("")], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = ids.len() > 16;
    ids.truncate(16);
    Ok(ToolCallPage {
        next_after: more.then(|| ids.last().unwrap().clone()),
        ids,
    })
}

pub(super) fn page(pool: &ConnectionPool, after: Option<&str>) -> Result<ToolCallPage> {
    let _span = performance::Span::new("tool_calls.page");
    let conn = pool.read()?;
    let mut query=conn.prepare_cached("SELECT id FROM governance_records INDEXED BY idx_tool_call_drivable WHERE type='tool_call' AND id>?1 AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR (json_extract(data,'$.status') IN ('succeeded','failed','cancelled') AND coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL)) ORDER BY id LIMIT 17").map_err(storage)?;
    let mut ids = query
        .query_map([after.unwrap_or("")], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = ids.len() > 16;
    ids.truncate(16);
    Ok(ToolCallPage {
        next_after: more.then(|| ids.last().unwrap().clone()),
        ids,
    })
}
pub(super) fn facts(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    id: &str,
) -> Result<ToolCallFacts> {
    let _span = performance::Span::new("tool_calls.facts");
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let mut rows = resource_commits::read_on(&tx, &[ResourceKey::new("governance", id)])?;
    let row = rows.remove(0);
    let raw = ConfigurationRecord::from_storage(
        row.value
            .clone()
            .ok_or_else(|| err("not_found", "工具调用缺失"))?,
    )?;
    require(raw.r#type == "tool_call", "资源不是工具调用")?;
    let mut memo = Default::default();
    let invocation: ToolInvocation = serde_json::from_value(source_views::restore_with(
        &catalog.state,
        &raw.data["invocation"],
        &mut memo,
    )?)
    .map_err(storage)?;
    let mut result = ToolCallFacts {
        row,
        invocation,
        session: None,
        worker_guards: vec![],
        worker_active: false,
        dispatch_allowed: true,
        dispatch_suspended: false,
    };
    if let (Some(session), Some(turn), Some(operation)) = (
        &result.invocation.session_id,
        &result.invocation.turn_id,
        &result.invocation.operation_id,
    ) {
        let keys = [
            ResourceKey::new("session", session),
            ResourceKey::new("tool_worker", operation),
            ResourceKey::new("tool_turn", turn),
            ResourceKey::new("tool_session", session),
        ];
        let mut rows = resource_commits::read_on(&tx, &keys)?.into_iter();
        let session_row = rows.next().unwrap();
        let raw = session_row
            .value
            .ok_or_else(|| err("not_found", "工具来源会话缺失"))?;
        let session: Session =
            serde_json::from_value(source_views::restore_with(&catalog.state, &raw, &mut memo)?)
                .map_err(storage)?;
        require(
            Some(&session.id) == result.invocation.session_id.as_ref()
                && session.project_id == result.invocation.project_id,
            "工具来源会话范围不符",
        )?;
        let worker = rows.next().unwrap();
        let turn = rows.next().unwrap();
        let session_guard = rows.next().unwrap();
        result.worker_active = worker
            .value
            .as_ref()
            .is_some_and(|v| v["active"] == true && v["session_id"] == session.id)
            && turn.value.as_ref().is_some_and(|v| {
                v["session_id"] == session.id
                    && v["cancel_requested"] != true
                    && matches!(
                        v["status"].as_str(),
                        Some("dispatching" | "running" | "waiting_interaction")
                    )
            });
        result.worker_guards = vec![worker, turn, session_guard]
            .into_iter()
            .map(|r| ResourceVersion {
                resource: r.resource,
                revision: r.revision,
            })
            .collect();
        result.dispatch_allowed = result.worker_active;
        if raw["data"]["conversation_authority"].is_object() {
            let authority: devdesk_core::ConversationAuthority =
                serde_json::from_value(raw["data"]["conversation_authority"].clone())
                    .map_err(storage)?;
            let facts = conversation_authority_on(
                &tx,
                &authority,
                &session.id,
                session
                    .project_id
                    .as_deref()
                    .ok_or_else(|| err("invalid_scope", "原会话缺少项目命名空间"))?,
            )?;
            result.dispatch_allowed &= facts.allowed;
            result.dispatch_suspended = facts.suspended;
            result.worker_guards.extend(facts.versions);
        }
        result.session = Some(session);
    }
    if result.invocation.session_id.is_none()
        && let Some(operation) = &result.invocation.operation_id
    {
        let authority: devdesk_core::tool_calls::WorkflowToolAuthority =
            serde_json::from_value(raw.data["workflow_authority"].clone()).map_err(storage)?;
        require(
            authority.execution_id == *operation,
            "工具步骤缺少原工作流资格",
        )?;
        let project = result
            .invocation
            .project_id
            .as_deref()
            .ok_or_else(|| err("invalid_scope", "工具步骤缺少项目"))?;
        let facts = workflow_on(&tx, &authority, project)?;
        result.worker_guards.extend(facts.versions);
        result.dispatch_suspended = facts.suspended;
        result.dispatch_allowed = facts.allowed;
    }
    tx.commit().map_err(storage)?;
    Ok(result)
}

pub(super) fn conversation_authority_on(
    conn: &Connection,
    authority: &devdesk_core::ConversationAuthority,
    session_id: &str,
    project: &str,
) -> Result<WorkflowToolFacts> {
    let mut result = if let Some(workflow) = &authority.workflow {
        workflow_on(conn, workflow, project)?
    } else {
        WorkflowToolFacts {
            allowed: true,
            suspended: false,
            versions: vec![],
        }
    };
    if let Some((binding, controller, epoch)) = &authority.controller {
        let rows = resource_commits::read_on(
            conn,
            &[
                ResourceKey::new("governance", binding),
                ResourceKey::new("controller_authority", controller),
            ],
        )?;
        let binding = rows[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原控制器绑定缺失"))?;
        let controller = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原控制器缺失"))?;
        result.allowed &= binding["type"] == "controller_session"
            && binding["definition_id"] == session_id
            && binding["data"]["epoch"] == *epoch
            && controller["type"] == "workflow_controller"
            && controller["data"]["session_id"] == session_id
            && controller["data"]["epoch"] == *epoch
            && controller["data"]["status"] == "active";
        result.versions.extend(rows.iter().map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        }));
    }
    Ok(result)
}

pub(super) fn workflow(
    pool: &ConnectionPool,
    authority: &WorkflowToolAuthority,
    project: &str,
) -> Result<WorkflowToolFacts> {
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let facts = workflow_on(&tx, authority, project)?;
    tx.commit().map_err(storage)?;
    Ok(facts)
}
pub(super) fn workflow_on(
    conn: &Connection,
    authority: &WorkflowToolAuthority,
    project: &str,
) -> Result<WorkflowToolFacts> {
    let mut versions = vec![];
    let ids = &authority.instances;
    require(
        !ids.is_empty() && ids.len() <= 9 && ids.len() == authority.executions.len(),
        "工具步骤祖先范围无效",
    )?;
    let keys = ids
        .iter()
        .map(|id| ResourceKey::new("workflow_run_authority", id))
        .collect::<Vec<_>>();
    let rows = resource_commits::read_on(conn, &keys)?;
    let mut valid = true;
    let mut paused = false;
    for (index, row) in rows.into_iter().enumerate() {
        let value = row
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "工具步骤工作流缺失"))?;
        require(
            value["project_id"].as_str() == Some(project)
                && serde_json::from_value::<Scope>(value["scope"].clone())
                    .map_err(storage)?
                    .project_id()
                    == Some(project),
            "工具步骤跨项目",
        )?;
        valid &= value["cancel_requested"] != true
            && !matches!(value["status"].as_str(), Some("completed" | "cancelled"));
        paused |= value["paused"] == true;
        valid &= if index + 1 < ids.len() {
            value["parent_execution_id"] == authority.executions[index + 1]
        } else {
            value["parent_execution_id"].is_null()
        };
        let key = ResourceKey::new(
            "workflow_step_authority",
            json!([ids[index], authority.executions[index]]).to_string(),
        );
        let mut selected = resource_commits::read_on(conn, &[key])?;
        let step_row = selected.remove(0);
        let step = step_row.value.unwrap_or(Value::Null);
        valid &= step["status"] == "running" && step["stop_requested"] != true;
        if index == 0 {
            valid &= step["plan_revision_id"] == authority.plan_revision_id
                && step["input_digest"] == authority.input_digest
                && step["contract_digest"] == authority.contract_digest
                && step["node_id"] == authority.node_id
                && step["session_id"] == json!(authority.session_id);
        }
        versions.push(ResourceVersion {
            resource: row.resource,
            revision: row.revision,
        });
        versions.push(ResourceVersion {
            resource: step_row.resource,
            revision: step_row.revision,
        });
    }

    Ok(WorkflowToolFacts {
        versions,
        allowed: valid && !paused,
        suspended: valid && paused,
    })
}
