//! Selected terminal ledger values and actual stopped-worker evidence. Normal
//! workflow observation does not load conversation or execution history.
use crate::*;
use rusqlite::OptionalExtension;

pub(super) fn turn(conn: &Connection, id: &str) -> Result<Option<Value>> {
    let raw=conn.query_row("SELECT json_object('id',id,'session_id',session_id,'project_id',project_id,'client_message_id',client_message_id,'input_digest',input_digest,'status',status,'consumed',json(CASE WHEN consumed THEN 'true' ELSE 'false' END),'cancel_requested',json(CASE WHEN cancel_requested THEN 'true' ELSE 'false' END),'started',started,'finished',finished,'revision',revision) FROM turns WHERE id=?1",[id],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let actual: Value = serde_json::from_str(&raw).map_err(storage)?;
    let source=load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type IN ('configured_execution_task','archived_configured_execution_task') AND definition_id=?1 ORDER BY rowid LIMIT 1)",&[&id])?.into_iter().next();
    let mut assistant = String::new();
    if actual["status"] == "completed" {
        let boundary:u64=conn.query_row("SELECT coalesce(max(sequence),0) FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND mode='full'",[id],|r|r.get(0)).map_err(storage)?;
        let (count,bytes):(u64,u64)=conn.query_row("SELECT count(*),coalesce(sum(length(CAST(content AS BLOB))),0) FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND sequence>=?2",rusqlite::params![id,boundary],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
        require(
            count <= 4096 && bytes <= SPEC_VALUE_LIMIT as u64,
            "实际 Agent 输出超过有界接纳预算",
        )?;
        let mut q=conn.prepare_cached("SELECT content FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND sequence>=?2 ORDER BY sequence").map_err(storage)?;
        for part in q
            .query_map(rusqlite::params![id, boundary], |r| r.get::<_, String>(0))
            .map_err(storage)?
        {
            assistant.push_str(&part.map_err(storage)?);
        }
    }
    let mut q=conn.prepare_cached("SELECT json_object('execution_id',r.id,'session_id',r.session_id,'turn_id',e.turn_id,'operation_id',r.operation_id,'image_digest',r.image_digest,'writer_revoked',json('true'),'protocol_artifact',r.protocol_artifact,'scope','Observed outer harness execution for this exact Turn; does not attest arbitrary nested or remote programs.') FROM conversation_events e JOIN executions r ON r.session_id=e.session_id AND r.kind='harness' AND r.writer_revoked=1 AND r.container_id IS NOT NULL AND e.deduplication_key=r.id||':stop-proof' WHERE e.turn_id=?1 AND e.kind='status' AND CASE WHEN json_valid(e.content) THEN json_extract(e.content,'$.operation_id')=r.operation_id AND json_extract(e.content,'$.writer_revoked')=1 ELSE 0 END ORDER BY e.sequence LIMIT 65").map_err(storage)?;
    let runtime = q
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .map(|r| serde_json::from_str::<Value>(&r.map_err(storage)?).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    require(runtime.len() <= 64, "原轮次停止证明超过预算")?;
    let failure = conn.query_row(
        "SELECT json_object('code',json_extract(content,'$.code'),'message',substr(json_extract(content,'$.message'),1,4096),'execution_failure',json_extract(content,'$.execution_failure')) FROM conversation_events WHERE turn_id=?1 AND kind='status' AND CASE WHEN json_valid(content) THEN json_type(content,'$.code')='text' ELSE 0 END ORDER BY sequence DESC LIMIT 1",
        [id], |r|r.get::<_,String>(0)).optional().map_err(storage)?
        .map(|s|serde_json::from_str::<Value>(&s).map_err(storage)).transpose()?;
    Ok(Some(
        json!({"turn":actual,"task_source":source,"assistant":assistant,"runtime":runtime,"failure":failure}),
    ))
}

/// Only the actual execution's child tree contributes to its waiting budget.
/// Values are timing facts, never expanded configuration or dialogue bodies.
pub(super) fn waits(conn: &Connection, root: &str, execution: &str, since: u64) -> Result<Value> {
    let mut q=conn.prepare_cached("WITH RECURSIVE children(id,depth) AS (SELECT id,1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.parent_execution_id')=?2 AND json_extract(data,'$.root_instance_id')=?1 UNION ALL SELECT c.id,p.depth+1 FROM children p JOIN governance_records i ON i.id=p.id JOIN json_each(i.data,'$.steps') e JOIN governance_records c ON c.type='workflow_instance' AND json_extract(c.data,'$.parent_execution_id')=json_extract(e.value,'$.id') AND json_extract(c.data,'$.root_instance_id')=?1 WHERE p.depth<8), executions(id) AS (SELECT ?2 UNION SELECT json_extract(e.value,'$.id') FROM children c JOIN governance_records i ON i.id=c.id JOIN json_each(i.data,'$.steps') e), intervals(id,started,finished,kind) AS (SELECT w.id,CAST(json_extract(w.data,'$.started') AS INTEGER),CAST(json_extract(w.data,'$.finished') AS INTEGER),'workflow_wait' FROM governance_records w WHERE w.type='workflow_wait_interval' AND w.definition_id IN (SELECT id FROM executions) UNION ALL SELECT w.id,CAST(json_extract(w.data,'$.started') AS INTEGER),CAST(json_extract(w.data,'$.finished') AS INTEGER),'child_pause' FROM governance_records w WHERE w.type='workflow_pause_interval' AND w.definition_id IN (SELECT id FROM children) UNION ALL SELECT a.id,a.created_at,a.answered_at,'interaction' FROM interactions a JOIN governance_records t ON t.type IN ('configured_execution_task','archived_configured_execution_task') AND t.definition_id=a.turn_id WHERE json_extract(t.data,'$.source.execution_id') IN (SELECT id FROM executions) AND a.created_at IS NOT NULL) SELECT json_object('id',id,'started',started,'finished',finished,'kind',kind) FROM intervals WHERE finished IS NULL OR finished>=?3 ORDER BY started,id LIMIT 1025").map_err(storage)?;
    let rows = q
        .query_map(rusqlite::params![root, execution, since], |r| {
            r.get::<_, String>(0)
        })
        .map_err(storage)?
        .map(|r| serde_json::from_str::<Value>(&r.map_err(storage)?).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    require(rows.len() <= 1024, "实际执行树等待区间超过预算")?;
    Ok(json!(rows))
}
