//! Native conversation polling reads one Turn's metadata and original worker
//! authority. It never restores input bodies, plans or unrelated history.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{
    ConversationAuthority, ConversationRuntimeFacts, ProviderTurn, concurrency::*,
    tool_calls::ToolCallPage,
};
use rusqlite::OptionalExtension;

pub(super) fn page(
    pool: &ConnectionPool,
    queued: bool,
    after: Option<&str>,
) -> Result<ToolCallPage> {
    let conn = pool.read()?;
    let (index, predicate) = if queued {
        ("idx_conversation_queued_seek", "status='queued'")
    } else {
        (
            "idx_conversation_active_seek",
            "status NOT IN ('queued','completed','failed','cancelled')",
        )
    };
    let sql = format!(
        "SELECT id FROM turns INDEXED BY {index} WHERE {predicate} AND id>?1 ORDER BY id LIMIT 17"
    );
    let mut q = conn.prepare_cached(&sql).map_err(storage)?;
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
pub(super) fn answers(pool: &ConnectionPool, turn: &str) -> Result<Vec<PendingInteraction>> {
    let conn = pool.read()?;
    load_table_filtered_params(
        &conn,
        "interactions",
        "id IN (SELECT i.id FROM interactions i INDEXED BY idx_conversation_answer_pending WHERE i.status='answered' AND i.turn_id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records g WHERE g.type='interaction_reply_intent' AND g.definition_id=i.id AND json_extract(g.data,'$.interaction_revision')=i.revision) ORDER BY i.id LIMIT 16)",
        &[&turn],
    )
}
pub(super) fn runtime(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    tid: &str,
) -> Result<Option<ConversationRuntimeFacts>> {
    let _span = performance::Span::new("conversation.runtime_facts");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err(
            "catalog_refresh_required",
            "会话引用需要较新的不可变源目录",
        ));
    }
    let turn = conn.query_row("SELECT json_object('id',id,'session_id',session_id,'project_id',project_id,'revision',revision,'status',status,'external_message',external_message,'external_turn',external_turn,'consumed',json(CASE WHEN consumed=1 THEN 'true' ELSE 'false' END),'cancel_requested',json(CASE WHEN cancel_requested=1 THEN 'true' ELSE 'false' END),'input_digest',input_digest) FROM turns WHERE id=?1 AND status NOT IN ('queued','completed','failed','cancelled')",[tid],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    let Some(turn) = turn else {
        conn.execute_batch("COMMIT;").map_err(storage)?;
        return Ok(None);
    };
    let turn: ProviderTurn = serde_json::from_str(&turn).map_err(storage)?;
    let keys = [
        ResourceKey::new("session", &turn.session_id),
        ResourceKey::new("conversation_open_seed", tid),
    ];
    let rows = resource_commits::read_on(&conn, &keys)?;
    let raw_session = rows[0]
        .value
        .clone()
        .ok_or_else(|| err("not_found", "运行会话缺失"))?;
    let workers = load_table_filtered_params::<Value>(
        &conn,
        "executions",
        "id IN (SELECT id FROM executions WHERE session_id=?1 AND writer_revoked=0 ORDER BY id LIMIT 65)",
        &[&turn.session_id],
    )?;
    require(workers.len() <= 64, "单会话 worker 数量超出预算")?;
    let worker_keys = workers
        .iter()
        .filter_map(|r| r["operation_id"].as_str())
        .map(|id| ResourceKey::new("tool_worker", id))
        .collect::<Vec<_>>();
    let worker_leases = resource_commits::read_on(&conn, &worker_keys)?;
    let binding_count: u64=conn.query_row("SELECT count(*) FROM governance_records WHERE type IN ('configured_session','controller_session') AND definition_id=?1",[&turn.session_id],|r|r.get(0)).map_err(storage)?;
    let seed = rows[1].value.clone();
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let session: Session =
        serde_json::from_value(source_views::restore(&catalog.state, &raw_session)?)
            .map_err(storage)?;
    require(
        session.id == turn.session_id
            && session.project_id.as_ref() == Some(&turn.project_id)
            && session.scope.is_some(),
        "原会话运行事实跨范围",
    )?;
    let mut memo = Default::default();
    let workers = workers
        .iter()
        .map(|raw| {
            serde_json::from_value(source_views::restore_with(&catalog.state, raw, &mut memo)?)
                .map_err(storage)
        })
        .collect::<Result<Vec<RuntimeExecution>>>()?;
    let authority = if let Some(seed) = seed {
        require(
            seed["data"]["session_id"] == session.id
                && seed["data"]["turn_id"] == turn.id
                && seed["data"]["input_digest"] == turn.input_digest
                && workers
                    .iter()
                    .filter(|w| w.kind == RuntimeExecutionKind::Harness)
                    .all(|w| seed["data"]["worker_operation_id"] == w.operation_id),
            "原 worker 与启动种子不符",
        )?;
        Some(serde_json::from_value(seed["data"]["authority"].clone()).map_err(storage)?)
    } else if binding_count == 0
        && session
            .runtime_binding
            .as_ref()
            .is_some_and(|b| b.work_type == WorkType::Discussion)
    {
        // An actual detached discussion has no workflow/controller authority.
        // Associations cannot be attached after a worker exists.
        Some(ConversationAuthority {
            workflow: None,
            controller: None,
        })
    } else {
        None
    };
    let worker_active = worker_leases
        .iter()
        .filter(|r| r.value.as_ref().is_some_and(|v| v["kind"] == "harness"))
        .any(|r| r.revision == 1);
    Ok(Some(ConversationRuntimeFacts {
        session,
        turn,
        workers,
        authority,
        worker_active,
    }))
}
