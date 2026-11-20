//! Indexed Session pages and exact Turn/event reads. Runtime source expansion
//! uses only the immutable configuration catalog after the read snapshot ends.
use crate::{connection_pool::ConnectionPool, *};
use rusqlite::{OptionalExtension, params};
fn context(conn: &Connection, sid: &str) -> Result<Option<Value>> {
    conn.query_row("SELECT json_object('step_instance_id',json_extract(data,'$.execution_id'),'step',json_extract(data,'$.step'),'instance_id',json_extract(data,'$.instance_id')) FROM governance_records WHERE type='configured_session' AND definition_id=?1",[sid],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose()
}
fn instance(raw: &Value) -> Option<&str> {
    raw["runtime_binding"]["agent_configuration"]["instance_ref"]
        .as_str()
        .or_else(|| raw["runtime_binding"]["agent_configuration"]["agent_instance_id"].as_str())
}
fn selected(
    conn: &Connection,
    table: &str,
    predicate: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Value>> {
    load_table_filtered_params(conn, table, predicate, args)
}
pub(super) fn query(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    method: &str,
    p: &Value,
) -> Result<Value> {
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "会话读取需要当前源目录"));
    }
    let limit = p["limit"].as_u64().unwrap_or(50).clamp(1, 100);
    let offset = p["offset"].as_u64().unwrap_or(0).min(1_000_000);
    let mut result = if method == "query.sessions" {
        let anchor = p["workflow_session_id"]
            .as_str()
            .map(|id| {
                selected(&conn, "sessions", "id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-session-deleted:' || sessions.id)", &[&id]).map(|mut rows| rows.pop())
            })
            .transpose()?
            .flatten();
        let task = anchor.as_ref().filter(|v| v["scope"] == "task");
        let owner = p["owner_id"]
            .as_str()
            .or_else(|| task.and_then(|v| v["owner_id"].as_str()));
        let agent = p["agent"]
            .as_str()
            .or_else(|| task.and_then(|v| v["agent"].as_str()));
        let agent_instance = p["agent_instance_id"]
            .as_str()
            .or_else(|| anchor.as_ref().and_then(instance));
        let workflow = anchor
            .as_ref()
            .filter(|v| v["scope"] != "task")
            .map(|v| context(&conn, v["id"].as_str().unwrap_or("")))
            .transpose()?
            .flatten();
        let step = p["step_execution_id"].as_str().or_else(|| {
            workflow
                .as_ref()
                .and_then(|v| v["step_instance_id"].as_str())
        });
        let anchor_id = anchor.as_ref().and_then(|row| row["id"].as_str());
        let predicate = "scope IS NOT NULL AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-session-deleted:' || sessions.id) AND (id=?5 OR ((?1 IS NULL OR owner_id=?1) AND (?2 IS NULL OR agent=?2) AND (?3 IS NULL OR coalesce(json_extract(runtime_binding,'$.agent_configuration.instance_ref'),json_extract(runtime_binding,'$.agent_configuration.agent_instance_id'))=?3 OR EXISTS(SELECT 1 FROM governance_records a WHERE a.type='agent_session' AND a.definition_id=sessions.id AND json_extract(a.data,'$.agent_instance_id')=?3)) AND (?4 IS NULL OR EXISTS(SELECT 1 FROM governance_records g WHERE g.type='configured_session' AND g.definition_id=sessions.id AND json_extract(g.data,'$.execution_id')=?4))))";
        let total: u64 = conn
            .query_row(
                &format!("SELECT count(*) FROM sessions WHERE {predicate}"),
                params![owner, agent, agent_instance, step, anchor_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let rows = selected(
            &conn,
            "sessions",
            &format!(
                "id IN (SELECT id FROM sessions WHERE {predicate} ORDER BY (id IS ?5) DESC,rowid DESC LIMIT ?6 OFFSET ?7)"
            ),
            &[
                &owner,
                &agent,
                &agent_instance,
                &step,
                &anchor_id,
                &limit,
                &offset,
            ],
        )?;
        let mut sessions = vec![];
        for mut row in rows.into_iter().rev() {
            row["agent_instance_id"] = json!(instance(&row));
            row["workflow_context"] = json!(context(&conn, row["id"].as_str().unwrap_or(""))?);
            sessions.push(row);
        }
        json!({"sessions":sessions,"total":total})
    } else {
        let sid = p["id"]
            .as_str()
            .or_else(|| p["session_id"].as_str())
            .ok_or_else(|| err("invalid_params", "需要 Session ID"))?;
        let session = selected(&conn, "sessions", "id=?1 AND scope IS NOT NULL", &[&sid])?
            .pop()
            .ok_or_else(|| err("not_found", "原会话不存在"))?;
        if let Some(project) = p["project_id"].as_str() {
            require(session["project_id"] == project, "会话跨项目")?;
        }
        if let Some(event) = p["event_id"].as_str() {
            let event = selected(
                &conn,
                "conversation_events",
                "id=?1 AND session_id=?2",
                &[&event, &sid],
            )?
            .pop()
            .ok_or_else(|| err("not_found", "原事件不属于会话"))?;
            json!({"event":event})
        } else if let Some(tid) = p["diagnostic_turn_id"].as_str() {
            let turn = selected(&conn, "turns", "id=?1 AND session_id=?2", &[&tid, &sid])?
                .pop()
                .ok_or_else(|| err("not_found", "原轮次不属于会话"))?;
            let records = selected(
                &conn,
                "conversation_events",
                "id IN (SELECT id FROM conversation_events WHERE session_id=?1 AND turn_id=?2 AND mode!='delta' ORDER BY sequence LIMIT ?3 OFFSET ?4)",
                &[&sid, &tid, &limit, &offset],
            )?;
            let stops = records
                .iter()
                .filter_map(|r| serde_json::from_str::<Value>(r["content"].as_str()?).ok())
                .filter(|r| r.get("writer_revoked").is_some())
                .collect::<Vec<_>>();
            let operations = json!(
                stops
                    .iter()
                    .filter_map(|r| r["operation_id"].as_str())
                    .collect::<Vec<_>>()
            )
            .to_string();
            let executions = selected(
                &conn,
                "executions",
                "session_id=?1 AND operation_id IN (SELECT value FROM json_each(?2))",
                &[&sid, &operations],
            )?;
            json!({"session_id":sid,"turn":turn,"executions":executions,"stop_receipts":stops,"records":records,"offset":offset,"more":records.len() as u64==limit,"source":"persisted conversation records; streaming fragments omitted"})
        } else {
            let hidden: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-session-deleted:' || ?1)",
                [sid], |r| r.get(0),
            ).map_err(storage)?;
            if hidden {
                return Err(err("not_found", "会话已删除"));
            }
            let cursor = p["cursor"].as_u64().unwrap_or(0);
            let events = selected(
                &conn,
                "conversation_events",
                "id IN (SELECT id FROM conversation_events WHERE session_id=?1 AND sequence>?2 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-turn-deleted:' || conversation_events.turn_id) ORDER BY sequence LIMIT ?3)",
                &[&sid, &cursor, &limit],
            )?;
            let next = events
                .last()
                .and_then(|r| r["sequence"].as_u64())
                .unwrap_or(cursor);
            let turn_filter = "session_id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-turn-deleted:' || turns.id)";
            let through = p["anchor_turn_id"].as_str();
            let turns = selected(&conn, "turns",
                &format!("id IN (SELECT id FROM turns WHERE {turn_filter} ORDER BY (id IS ?4) DESC,rowid DESC LIMIT ?2 OFFSET ?3)"),
                &[&sid, &limit, &offset, &through],
            )?.into_iter().rev().collect::<Vec<_>>();
            let total: u64 = conn
                .query_row(
                    &format!("SELECT count(*) FROM turns WHERE {turn_filter}"),
                    [sid],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            let interactions=selected(&conn,"interactions","id IN (SELECT id FROM interactions WHERE session_id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-turn-deleted:' || interactions.turn_id) ORDER BY rowid DESC LIMIT ?2)",&[&sid,&limit])?.into_iter().rev().collect::<Vec<_>>();
            let proposals = selected(
                &conn,
                "proposals",
                "id IN (SELECT id FROM proposals WHERE session_id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-turn-deleted:' || proposals.turn_id) ORDER BY rowid DESC LIMIT ?2)",
                &[&sid, &limit],
            )?
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
            let executions=selected(&conn,"executions","id IN (SELECT id FROM executions WHERE session_id=?1 ORDER BY rowid DESC LIMIT ?2)",&[&sid,&limit])?.into_iter().rev().collect::<Vec<_>>();
            let binding = selected(
                &conn,
                "governance_records",
                "type='controller_session' AND definition_id=?1",
                &[&sid],
            )?
            .pop();
            let controller=binding.as_ref().map(|b|selected(&conn,"governance_records","id=?1 AND type='workflow_controller' AND json_extract(data,'$.session_id')=?2",&[&b["data"]["controller_id"].as_str().unwrap_or(""),&sid])).transpose()?.and_then(|mut rows|rows.pop());
            let predecessor_available = session["predecessor"].as_str().map(|id| conn.query_row(
                "SELECT NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-session-deleted:' || ?1)",
                [id], |r| r.get::<_, bool>(0),
            ).map_err(storage)).transpose()?;
            let workflow = context(&conn, sid)?;
            let experience=selected(&conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='agent_context_call' AND json_extract(data,'$.invocation.session_id')=?1 AND name LIKE 'agent.experience.%' AND NOT EXISTS(SELECT 1 FROM governance_records d WHERE d.id='discussion-turn-deleted:' || json_extract(governance_records.data,'$.invocation.turn_id')) ORDER BY rowid DESC LIMIT 50)",&[&sid])?.into_iter().rev().map(|r|json!({"id":r["id"],"method":r["name"],"status":r["data"]["status"],"source":r["data"]["result"]["entry"],"content_digest":r["data"]["result"]["content_digest"],"turn_id":r["data"]["invocation"]["turn_id"],"error":r["data"]["error"]})).collect::<Vec<_>>();
            json!({"session":session,"predecessor_available":predecessor_available,"controller":controller,"current_runtime_binding":session["runtime_binding"],"workflow_context":workflow,"formal_execution":binding.is_some()||workflow.is_some(),"output_contract":null,"agent_instance_id":instance(&session),"experience_reads":experience,"turns":turns,"total_turns":total,"events":events,"next_cursor":next,"interactions":interactions,"proposals":proposals,"executions":executions})
        }
    };
    // Use only the original admitted user ledger to display compact task inputs.
    let mut task_inputs = std::collections::BTreeMap::<String, Value>::new();
    if let Some(turn) = result.get("turn")
        && turn["input"]
            .as_str()
            .map(|s| source_views::task_descriptor(&json!(s)))
            .transpose()?
            .flatten()
            .is_some()
    {
        task_inputs.insert(turn["id"].as_str().unwrap_or("").into(), turn.clone());
    }
    for turn in result["turns"].as_array().into_iter().flatten() {
        if turn["input"]
            .as_str()
            .map(|s| source_views::task_descriptor(&json!(s)))
            .transpose()?
            .flatten()
            .is_some()
        {
            task_inputs.insert(turn["id"].as_str().unwrap_or("").into(), turn.clone());
        }
    }
    let mut literal = std::collections::BTreeMap::new();
    for (id, turn) in task_inputs {
        let body=conn.query_row("SELECT e.content,json_extract(g.data,'$.digest') FROM conversation_events e JOIN governance_records g ON g.definition_id=e.turn_id AND g.type IN ('configured_execution_task','archived_configured_execution_task') WHERE e.turn_id=?1 AND e.session_id=?2 AND e.kind='user' ORDER BY e.sequence LIMIT 1",params![id,turn["session_id"].as_str()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(storage)?.ok_or_else(||err("storage","原任务用户账本缺失"))?;
        let value: Value = serde_json::from_str(&body.0).map_err(storage)?;
        require(configuration_hash(&value) == body.1, "原任务账本摘要不符")?;
        literal.insert(id, body.0);
    }
    if let Some(turn) = result.get_mut("turn")
        && let Some(body) = turn["id"].as_str().and_then(|id| literal.get(id))
    {
        turn["input"] = json!(body);
    }
    for turn in result["turns"].as_array_mut().into_iter().flatten() {
        if let Some(body) = turn["id"].as_str().and_then(|id| literal.get(id)) {
            turn["input"] = json!(body);
        }
    }
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let mut memo = Default::default();
    result = source_views::restore_with(&catalog.state, &result, &mut memo)?;
    Ok(result)
}
