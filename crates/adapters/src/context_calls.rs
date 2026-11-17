//! Exact context invocation and live caller witnesses, without aggregate restore.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{
    ContextCallFacts, ConversationAuthority, concurrency::*, tool_calls::ToolCallPage,
};

pub(super) fn page(pool: &ConnectionPool, after: Option<&str>) -> Result<ToolCallPage> {
    let conn = pool.read()?;
    let mut q=conn.prepare_cached("SELECT id FROM governance_records INDEXED BY idx_context_call_pending WHERE type='agent_context_call' AND (json_extract(data,'$.status') IN ('prepared','running') OR coalesce(json_extract(data,'$.reply_sent'),0)=0) AND id>?1 ORDER BY id LIMIT 17").map_err(storage)?;
    let mut ids = q
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
) -> Result<ContextCallFacts> {
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "上下文调用需要新的源目录"));
    }
    let row = resource_commits::read_on(&conn, &[ResourceKey::new("governance", id)])?.remove(0);
    let raw = row
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "上下文调用缺失"))?;
    require(raw["type"] == "agent_context_call", "不是上下文调用")?;
    let invocation = &raw["data"]["invocation"];
    let sid = invocation["session_id"]
        .as_str()
        .ok_or_else(|| err("storage", "上下文调用缺少 Session"))?;
    let tid = invocation["turn_id"]
        .as_str()
        .ok_or_else(|| err("storage", "上下文调用缺少 Turn"))?;
    let operation = invocation["operation_id"]
        .as_str()
        .ok_or_else(|| err("storage", "上下文调用缺少 worker"))?;
    let keys = [
        ResourceKey::new("session", sid),
        ResourceKey::new("tool_turn", tid),
        ResourceKey::new("tool_worker", operation),
    ];
    let rows = resource_commits::read_on(&conn, &keys)?;
    let raw_session = rows[0]
        .value
        .clone()
        .ok_or_else(|| err("not_found", "原上下文会话缺失"))?;
    let pid = raw_session["project_id"]
        .as_str()
        .ok_or_else(|| err("invalid_scope", "原上下文会话缺少项目命名空间"))?;
    require(invocation["project_id"] == pid, "上下文调用跨原项目")?;
    let authority: ConversationAuthority =
        serde_json::from_value(raw["data"]["conversation_authority"].clone()).map_err(storage)?;
    let authority = tool_calls::conversation_authority_on(&conn, &authority, sid, pid)?;
    let active =
        rows[1].value.as_ref().is_some_and(|t| {
            t["session_id"] == sid
                && t["cancel_requested"] != true
                && matches!(
                    t["status"].as_str(),
                    Some("dispatching" | "running" | "waiting_interaction")
                )
        }) && rows[2].value.as_ref().is_some_and(|w| {
            w["session_id"] == sid && w["active"] == true && w["kind"] == "harness"
        }) && raw_session["lifecycle"] == "active";
    let mut guards = rows
        .iter()
        .map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        })
        .collect::<Vec<_>>();
    guards.extend(authority.versions);
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let session: Session =
        serde_json::from_value(source_views::restore(&catalog.state, &raw_session)?)
            .map_err(storage)?;
    let expected_scope = if session.scope == Some(SessionScope::Global) {
        Scope::global()
    } else {
        Scope::project(session.project_id.as_ref().unwrap())
    };
    require(
        raw["scope"] == json!(expected_scope),
        "上下文运行事实不属于原执行范围",
    )?;
    Ok(ContextCallFacts {
        row,
        session,
        caller_guards: guards,
        active: active && authority.allowed,
        suspended: active && authority.suspended,
    })
}
