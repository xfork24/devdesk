//! Bounded facts for a completed conversation's derived snapshot.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{ConversationCaptureFacts, concurrency::*, tool_calls::ToolCallPage};
use rusqlite::{OptionalExtension, params};

pub(super) fn page(pool: &ConnectionPool, after: Option<&str>) -> Result<ToolCallPage> {
    let conn = pool.read()?;
    let mut q = conn.prepare_cached("SELECT id FROM governance_records INDEXED BY idx_conversation_capture_pending WHERE type IN ('conversation_capture_intent','captured_experience_intent') AND json_extract(data,'$.status') IN ('prepared','capturing','captured') AND id>?1 ORDER BY id LIMIT 17").map_err(storage)?;
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
fn ids(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Id>> {
    let mut q = conn.prepare_cached(sql).map_err(storage)?;
    let rows = q
        .query_map(args, |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    require(rows.len() <= 2048, "会话快照引用超出有界捕获预算")?;
    Ok(rows)
}
fn json_rows(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
    let mut q = conn.prepare_cached(sql).map_err(storage)?;
    q.query_map(args, |r| r.get::<_, String>(0))
        .map_err(storage)?
        .map(|v| serde_json::from_str(&v.map_err(storage)?).map_err(storage))
        .collect()
}

pub(super) fn facts(
    pool: &ConnectionPool,
    session: &Session,
    tid: &str,
    terminal: &str,
    worker_boundary: u64,
) -> Result<ConversationCaptureFacts> {
    let _span = performance::Span::new("conversation.capture_facts");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let mut keys = vec![
        ResourceKey::new("session", &session.id),
        ResourceKey::new("turn", tid),
        ResourceKey::new("conversation_event", terminal),
    ];
    if session.scope != Some(SessionScope::Global) {
        keys.push(ResourceKey::new(
            "project",
            session
                .project_id
                .as_ref()
                .ok_or_else(|| err("invalid_scope", "快照缺少项目"))?,
        ));
    }
    if let Some(feature) = &session.feature_id {
        keys.push(ResourceKey::new("feature", feature));
    }
    for kind in ["agent_session", "configured_session"] {
        let id = conn.query_row("SELECT id FROM governance_records WHERE type=?1 AND definition_id=?2 ORDER BY rowid LIMIT 1", params![kind,session.id], |r| r.get::<_,String>(0)).optional().map_err(storage)?;
        if let Some(id) = id {
            keys.push(ResourceKey::new("governance", id));
        }
    }
    let rows = resource_commits::read_on(&conn, &keys)?;
    let raw_session = rows[0]
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "捕获会话缺失"))?;
    let turn = rows[1]
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "捕获轮次缺失"))?;
    let event = rows[2]
        .value
        .as_ref()
        .ok_or_else(|| err("not_found", "完成边界事件缺失"))?;
    require(
        raw_session["id"] == session.id
            && raw_session["project_id"] == json!(session.project_id)
            && turn["session_id"] == session.id
            && turn["status"] == "completed"
            && event["session_id"] == session.id
            && event["turn_id"] == tid,
        "捕获必须属于原完成边界",
    )?;
    let sequence = event["sequence"]
        .as_u64()
        .ok_or_else(|| err("storage", "完成边界缺少序号"))?;
    let mut messages: Vec<ConversationEvent> = load_table_filtered_params(
        &conn,
        "conversation_events",
        "id IN (SELECT id FROM (SELECT id,sum(bytes) OVER(ORDER BY sequence DESC) AS total FROM (SELECT id,sequence,length(CAST(content AS BLOB)) AS bytes FROM conversation_events WHERE session_id=?1 AND sequence<=?2 ORDER BY sequence DESC LIMIT 100)) WHERE total<=65536)",
        &[&session.id, &sequence],
    )?;
    messages.sort_by_key(|e| e.sequence);
    let inherited = session.snapshot_id.as_ref().map(|id| conn.query_row("SELECT kind,substr(content,1,16384) FROM snapshots WHERE id=?1", [id], |r| Ok(json!({"snapshot_id":id,"kind":r.get::<_,String>(0)?,"content_excerpt":r.get::<_,String>(1)?}))).optional().map_err(storage)).transpose()?.flatten();
    let contexts = json_rows(
        &conn,
        "SELECT json_object('turn_id',g.definition_id,'experience_refs',json_extract(g.data,'$.experience_refs')) FROM governance_records g WHERE g.type='conversation_context' AND json_extract(g.data,'$.session_id')=?1 AND EXISTS(SELECT 1 FROM turns t WHERE t.id=g.definition_id AND t.session_id=?1 AND t.rowid<=(SELECT rowid FROM turns WHERE id=?2)) ORDER BY g.rowid DESC LIMIT 50",
        &[&session.id, &tid],
    )?;
    let reads = json_rows(
        &conn,
        "SELECT json_object('call_id',g.id,'turn_id',json_extract(g.data,'$.invocation.turn_id'),'source',json_extract(g.data,'$.result.entry'),'content_digest',json_extract(g.data,'$.result.content_digest')) FROM governance_records g WHERE g.type='agent_context_call' AND json_extract(g.data,'$.invocation.session_id')=?1 AND json_extract(g.data,'$.status')='succeeded' AND EXISTS(SELECT 1 FROM turns t WHERE t.id=json_extract(g.data,'$.invocation.turn_id') AND t.session_id=?1 AND t.rowid<=(SELECT rowid FROM turns WHERE id=?2)) ORDER BY g.rowid DESC LIMIT 100",
        &[&session.id, &tid],
    )?;
    let turns = json_rows(
        &conn,
        "SELECT json_object('id',id,'status',status,'consumed',json(CASE WHEN consumed=1 THEN 'true' ELSE 'false' END),'cancel_requested',json(CASE WHEN cancel_requested=1 THEN 'true' ELSE 'false' END)) FROM turns WHERE session_id=?1 AND rowid<=(SELECT rowid FROM turns WHERE id=?2) ORDER BY rowid DESC LIMIT 50",
        &[&session.id, &tid],
    )?;
    let documents = match session
        .scope
        .ok_or_else(|| err("invalid_scope", "快照缺少归属"))?
    {
        SessionScope::Global => ids(
            &conn,
            "SELECT id FROM documents WHERE project_id IS NULL AND feature_id IS NULL AND task_id IS NULL ORDER BY rowid LIMIT 2049",
            &[],
        )?,
        SessionScope::Project => ids(
            &conn,
            "SELECT id FROM documents WHERE project_id=?1 ORDER BY rowid LIMIT 2049",
            &[&session.project_id],
        )?,
        SessionScope::Feature | SessionScope::Task => ids(
            &conn,
            "SELECT d.id FROM documents d WHERE (d.feature_id=?1 OR (d.project_id=?2 AND EXISTS(SELECT 1 FROM features f,json_each(f.source_materials) x WHERE f.id=?1 AND x.value=d.id))) AND (d.task_id IS NULL OR d.task_id=?3) AND NOT EXISTS(SELECT 1 FROM documents n WHERE n.supersedes=d.id AND n.kind=d.kind) ORDER BY d.rowid LIMIT 2049",
            &[&session.feature_id, &session.project_id, &session.task_id],
        )?,
    };
    let execution_ids = ids(
        &conn,
        "SELECT id FROM executions WHERE session_id=?1 AND rowid<=?2 ORDER BY rowid LIMIT 2049",
        &[&session.id, &worker_boundary],
    )?;
    let project = rows
        .iter()
        .find(|r| r.resource.kind == "project")
        .and_then(|r| r.value.as_ref());
    let feature = rows
        .iter()
        .find(|r| r.resource.kind == "feature")
        .and_then(|r| r.value.as_ref());
    let link = rows
        .iter()
        .filter_map(|r| r.value.as_ref())
        .find(|v| v["type"] == "agent_session")
        .cloned()
        .map(ConfigurationRecord::from_storage)
        .transpose()?;
    let workflow = rows
        .iter()
        .filter_map(|r| r.value.as_ref())
        .find(|v| v["type"] == "configured_session")
        .cloned()
        .map(ConfigurationRecord::from_storage)
        .transpose()?;
    let result = ConversationCaptureFacts {
        read_set: ReadSet::new(
            rows.iter()
                .map(|r| ResourceVersion {
                    resource: r.resource.clone(),
                    revision: r.revision,
                })
                .collect(),
        )?,
        content: json!({"source_session":session.id,"inherited_context_excerpt":inherited,"messages":messages,"context_references":contexts,"context_reads":reads,"turns":turns}),
        documents,
        execution_ids,
        link,
        workflow,
        project_title: project
            .map(|p| text_value(p, "title"))
            .unwrap_or_else(|| "全局 / 跨项目".into()),
        feature_title: feature.map(|f| text_value(f, "title")),
        work_time: turn["finished"]
            .as_u64()
            .or_else(|| turn["started"].as_u64()),
    };
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}
fn text_value(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

/// Input captures a real committed window before admission. No fabricated Turn
/// or terminal event is passed to the completed-capture rules.
pub(super) fn input(
    pool: &ConnectionPool,
    session: &Session,
    through_turn: Option<&str>,
) -> Result<ConversationCaptureFacts> {
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let mut keys = vec![ResourceKey::new("session", &session.id)];
    if session.scope != Some(SessionScope::Global) {
        keys.push(ResourceKey::new(
            "project",
            session
                .project_id
                .as_ref()
                .ok_or_else(|| err("invalid_scope", "输入快照缺少项目"))?,
        ));
    }
    if let Some(feature) = &session.feature_id {
        keys.push(ResourceKey::new("feature", feature));
    }
    for kind in ["agent_session", "configured_session"] {
        let id=conn.query_row("SELECT id FROM governance_records WHERE type=?1 AND definition_id=?2 ORDER BY rowid LIMIT 1",params![kind,session.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
        if let Some(id) = id {
            keys.push(ResourceKey::new("governance", id));
        }
    }
    let rows = resource_commits::read_on(&conn, &keys)?;
    require(
        rows[0].revision == session.revision
            && rows[0]
                .value
                .as_ref()
                .is_some_and(|s| s["project_id"] == json!(session.project_id)),
        "输入快照不能替换原会话版本",
    )?;
    let turn_boundary: i64 = conn.query_row(
        "SELECT coalesce(max(rowid),0) FROM turns WHERE session_id=?1 AND (?2 IS NULL OR id=?2)",
        params![session.id, through_turn],
        |r| r.get(0),
    ).map_err(storage)?;
    // Hide deleted Turns from new context. Immutable captures and the original
    // provider ledger stay available to audit readers.
    let visible_turns = "SELECT id FROM turns WHERE session_id=?1 AND rowid<=?2 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-turn-deleted:' || turns.id)";
    let mut messages: Vec<ConversationEvent> = load_table_filtered_params(
        &conn,
        "conversation_events", &format!(
            "id IN (SELECT id FROM (SELECT id,sum(bytes) OVER(ORDER BY sequence DESC) AS total FROM
             (SELECT id,sequence,length(CAST(content AS BLOB)) AS bytes FROM conversation_events
              WHERE session_id=?1 AND turn_id IN ({visible_turns}) ORDER BY sequence DESC LIMIT 100))
             WHERE total<=65536)"
        ),
        &[&session.id, &turn_boundary],
    )?;
    messages.sort_by_key(|e| e.sequence);
    let inherited = session.snapshot_id.as_ref().map(|id|conn.query_row(
        "SELECT kind,substr(content,1,16384) FROM snapshots WHERE id=?1",[id],
        |r|Ok(json!({"snapshot_id":id,"kind":r.get::<_,String>(0)?,"content_excerpt":r.get::<_,String>(1)?}))
    ).optional().map_err(storage)).transpose()?.flatten();
    let contexts = json_rows(
        &conn,
        &format!(
            "SELECT json_object('turn_id',definition_id,'experience_refs',json_extract(data,'$.experience_refs')) FROM governance_records WHERE type='conversation_context' AND definition_id IN ({visible_turns}) ORDER BY rowid DESC LIMIT 50"
        ),
        &[&session.id, &turn_boundary],
    )?;
    let reads = json_rows(
        &conn,
        &format!(
            "SELECT json_object('call_id',id,'turn_id',json_extract(data,'$.invocation.turn_id'),'source',json_extract(data,'$.result.entry'),'content_digest',json_extract(data,'$.result.content_digest')) FROM governance_records WHERE type='agent_context_call' AND json_extract(data,'$.status')='succeeded' AND json_extract(data,'$.invocation.turn_id') IN ({visible_turns}) ORDER BY rowid DESC LIMIT 100"
        ),
        &[&session.id, &turn_boundary],
    )?;
    let turns = json_rows(
        &conn,
        &format!(
            "SELECT json_object('id',id,'status',status,'consumed',json(CASE WHEN consumed=1 THEN 'true' ELSE 'false' END),'cancel_requested',json(CASE WHEN cancel_requested=1 THEN 'true' ELSE 'false' END)) FROM turns WHERE id IN ({visible_turns}) ORDER BY rowid DESC LIMIT 50"
        ),
        &[&session.id, &turn_boundary],
    )?;
    let documents = if let Some(turn) = through_turn {
        ids(
            &conn,
            "SELECT d.value FROM turns t JOIN snapshots s ON s.id=t.input_snapshot, json_each(s.documents) d WHERE t.id=?1 AND t.session_id=?2 ORDER BY d.key LIMIT 2049",
            &[&turn, &session.id],
        )?
    } else {
        match session
            .scope
            .ok_or_else(|| err("invalid_scope", "输入快照缺少归属"))?
        {
            SessionScope::Global => ids(
                &conn,
                "SELECT d.id FROM documents d WHERE project_id IS NULL AND feature_id IS NULL AND task_id IS NULL AND NOT EXISTS(SELECT 1 FROM documents n WHERE n.supersedes=d.id AND n.kind=d.kind) ORDER BY d.rowid LIMIT 2049",
                &[],
            )?,
            SessionScope::Project => ids(
                &conn,
                "SELECT d.id FROM documents d WHERE project_id=?1 AND NOT EXISTS(SELECT 1 FROM documents n WHERE n.supersedes=d.id AND n.kind=d.kind) ORDER BY d.rowid LIMIT 2049",
                &[&session.project_id],
            )?,
            SessionScope::Feature | SessionScope::Task => ids(
                &conn,
                "SELECT d.id FROM documents d WHERE (d.feature_id=?1 OR (d.project_id=?2 AND EXISTS(SELECT 1 FROM features f,json_each(f.source_materials) x WHERE f.id=?1 AND x.value=d.id))) AND (d.task_id IS NULL OR d.task_id=?3) AND NOT EXISTS(SELECT 1 FROM documents n WHERE n.supersedes=d.id AND n.kind=d.kind) ORDER BY d.rowid LIMIT 2049",
                &[&session.feature_id, &session.project_id, &session.task_id],
            )?,
        }
    };
    let execution_ids = ids(
        &conn,
        "SELECT id FROM (SELECT id,rowid AS boundary FROM executions WHERE session_id=?1 AND (?2 IS NULL OR started<=(SELECT coalesce(finished,started) FROM turns WHERE id=?2)) ORDER BY rowid DESC LIMIT 128) ORDER BY boundary",
        &[&session.id, &through_turn],
    )?;
    let link = rows
        .iter()
        .filter_map(|r| r.value.as_ref())
        .find(|r| r["type"] == "agent_session")
        .cloned()
        .map(ConfigurationRecord::from_storage)
        .transpose()?;
    let workflow = rows
        .iter()
        .filter_map(|r| r.value.as_ref())
        .find(|r| r["type"] == "configured_session")
        .cloned()
        .map(ConfigurationRecord::from_storage)
        .transpose()?;
    let project = rows
        .iter()
        .find(|r| r.resource.kind == "project")
        .and_then(|r| r.value.as_ref());
    let feature = rows
        .iter()
        .find(|r| r.resource.kind == "feature")
        .and_then(|r| r.value.as_ref());
    let mut result = ConversationCaptureFacts {
        read_set: ReadSet::new(
            rows.iter()
                .map(|r| ResourceVersion {
                    resource: r.resource.clone(),
                    revision: r.revision,
                })
                .collect(),
        )?,
        content: json!({"source_session":session.id,"inherited_context_excerpt":inherited,"messages":messages,"context_references":contexts,"context_reads":reads,"turns":turns}),
        documents,
        execution_ids,
        link,
        workflow,
        project_title: project
            .map(|p| text_value(p, "title"))
            .unwrap_or_else(|| "全局 / 跨项目".into()),
        feature_title: feature.map(|f| text_value(f, "title")),
        work_time: None,
    };
    if let Some(through) = through_turn {
        result.content["source_turn"] = json!(through);
    }
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}
