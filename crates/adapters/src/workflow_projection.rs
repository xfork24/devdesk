//! Transactional display metadata. Canonical runtime facts remain resource
//! rows; these small projections never authorize control or replace sources.
use crate::*;
use devdesk_core::concurrency::*;
use std::collections::BTreeSet;
const STEP_FIELDS: &[&str] = &[
    "time_budget",
    "id",
    "node_id",
    "plan_revision_id",
    "step",
    "path",
    "round",
    "status",
    "input_digest",
    "outputs",
    "agent_revision_id",
    "session_id",
    "turn_id",
    "started",
    "finished",
];
fn step_metadata(value: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in STEP_FIELDS {
        out.insert((*key).into(), value[*key].clone());
    }
    let reason = value["reason"].as_str().unwrap_or("");
    out.insert(
        "reason".into(),
        json!(reason.chars().take(512).collect::<String>()),
    );
    out.insert(
        "reason_truncated".into(),
        json!(reason.chars().count() > 512),
    );
    if let Some(code) = value["failure"]["code"].as_str() {
        out.insert(
            "failure_code".into(),
            json!(code.chars().take(128).collect::<String>()),
        );
    }
    Value::Object(out)
}
fn header(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("INSERT INTO workflow_display_headers(instance_id,record) SELECT id,json_object('id',id,'type',type,'definition_id',definition_id,'scope',json(scope),'name',name,'revision',revision,'version',version,'created_at',created_at,'supersedes',supersedes,'immutable',json(CASE immutable WHEN 1 THEN 'true' ELSE 'false' END),'refs',json(refs),'content_hash',content_hash,'data',json_remove(data,'$.steps','$.inputs')) FROM governance_records WHERE id=?1 AND type='workflow_instance' ON CONFLICT(instance_id) DO UPDATE SET record=excluded.record",[id]).map_err(storage)?;
    Ok(())
}
fn put_step(conn: &Connection, instance: &str, execution: &str, value: &Value) -> Result<()> {
    let key = serde_json::to_string(&(instance, execution)).map_err(storage)?;
    let revision: u64 = conn
        .query_row(
            "SELECT revision FROM resource_versions WHERE kind='workflow_step' AND resource_id=?1",
            [key],
            |r| r.get(0),
        )
        .map_err(storage)?;
    conn.execute("INSERT INTO workflow_display_steps(instance_id,execution_id,data,step_revision) VALUES(?1,?2,?3,?4) ON CONFLICT(instance_id,execution_id) DO UPDATE SET data=excluded.data,step_revision=excluded.step_revision",rusqlite::params![instance,execution,step_metadata(value).to_string(),revision]).map_err(storage)?;
    Ok(())
}
pub(super) fn refresh_root(conn: &Connection, id: &str) -> Result<()> {
    header(conn, id)?;
    let data: String = conn
        .query_row(
            "SELECT data FROM governance_records WHERE id=?1 AND type='workflow_instance'",
            [id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let data: Value = serde_json::from_str(&data).map_err(storage)?;
    conn.execute(
        "DELETE FROM workflow_display_steps WHERE instance_id=?1",
        [id],
    )
    .map_err(storage)?;
    for step in data["steps"]
        .as_array()
        .ok_or_else(|| err("storage", "实际流程缺少步骤"))?
    {
        put_step(
            conn,
            id,
            step["id"]
                .as_str()
                .ok_or_else(|| err("storage", "实际步骤缺少身份"))?,
            step,
        )?;
    }
    Ok(())
}
/// A catalog rewrite may include legacy rows without a versioned preview.
pub(super) fn refresh_after_reset(conn: &Connection, id: &str) -> Result<()> {
    let rebuildable: bool = conn
        .query_row(
            "SELECT NOT EXISTS(
                SELECT 1 FROM governance_records root JOIN json_each(root.data,'$.steps') step
                WHERE root.id=?1 AND NOT EXISTS(
                    SELECT 1 FROM resource_versions rev WHERE rev.kind='workflow_step'
                      AND rev.resource_id=json_array(root.id,json_extract(step.value,'$.id'))))
             AND EXISTS(SELECT 1 FROM workflow_display_headers WHERE instance_id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(storage)?;
    if rebuildable {
        refresh_root(conn, id)?;
    } else {
        conn.execute(
            "DELETE FROM workflow_display_steps WHERE instance_id=?1",
            [id],
        )
        .map_err(storage)?;
        conn.execute(
            "DELETE FROM workflow_display_headers WHERE instance_id=?1",
            [id],
        )
        .map_err(storage)?;
    }
    Ok(())
}

pub(super) fn commit(conn: &Connection, prepared: &PreparedChangeSet) -> Result<()> {
    let mut roots = BTreeSet::new();
    for change in &prepared.changes {
        if change.resource.kind == "governance" && change.value["type"] == "workflow_instance" {
            refresh_root(conn, &change.resource.id)?;
            roots.insert(change.resource.id.clone());
        } else if change.resource.kind == "workflow_header" {
            header(conn, &change.resource.id)?;
            roots.insert(change.resource.id.clone());
        } else if matches!(
            change.resource.kind.as_str(),
            "workflow_step"
                | "workflow_step_replace"
                | "workflow_step_restore"
                | "workflow_step_delete"
                | "workflow_step_create"
        ) {
            let (instance, execution): (String, String) =
                serde_json::from_str(&change.resource.id).map_err(storage)?;
            if change.resource.kind != "workflow_step" {
                conn.execute(
                    "DELETE FROM workflow_display_steps WHERE instance_id=?1 AND execution_id=?2",
                    rusqlite::params![instance, execution],
                )
                .map_err(storage)?;
            }
            if change.resource.kind != "workflow_step_delete" {
                put_step(
                    conn,
                    &instance,
                    change.value["id"]
                        .as_str()
                        .ok_or_else(|| err("storage", "步骤变化缺少实际身份"))?,
                    &change.value,
                )?;
            }
            roots.insert(instance);
        }
    }
    for id in roots {
        conn.execute("UPDATE workflow_display_headers SET record=json_set(record,'$.revision',(SELECT revision FROM governance_records WHERE id=?1),'$.content_hash',(SELECT content_hash FROM governance_records WHERE id=?1)) WHERE instance_id=?1",[id]).map_err(storage)?;
    }
    Ok(())
}

/// Rebuild the display tables on an offline project with a fixed event cursor.
/// A failed repair rolls back the derived tables together.
pub(super) fn rebuild(conn: &mut Connection) -> Result<Value> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(storage)?;
    let event_cursor = |conn: &Connection| -> Result<u64> {
        conn.query_row("SELECT COALESCE(max(id),0) FROM events", [], |row| {
            row.get(0)
        })
        .map_err(storage)
    };
    let initial = event_cursor(&tx)?;
    tx.execute_batch("DELETE FROM workflow_display_steps; DELETE FROM workflow_display_headers;")
        .map_err(storage)?;
    let mut cursor = 0_i64;
    let mut batches = 0;
    let mut rebuilt = 0_u64;
    loop {
        let rows = tx
            .prepare_cached(
                "SELECT rowid,id FROM governance_records
                 WHERE type='workflow_instance' AND rowid>?1 ORDER BY rowid LIMIT 128",
            )
            .map_err(storage)?
            .query_map([cursor], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        if rows.is_empty() {
            break;
        }
        for (position, instance) in rows {
            refresh_root(&tx, &instance)?;
            cursor = position;
            rebuilt += 1;
        }
        batches += 1;
        eprintln!(
            "{}",
            json!({"type":"workflow_display_rebuild","event_cursor":initial,"row_cursor":cursor,"instances":rebuilt})
        );
    }
    let (headers, steps, source_steps, invalid_keys): (u64, u64, u64, u64) = tx
        .query_row(
            "SELECT (SELECT count(*) FROM workflow_display_headers),
                (SELECT count(*) FROM workflow_display_steps),
                (SELECT coalesce(sum(json_array_length(data,'$.steps')),0)
                 FROM governance_records WHERE type='workflow_instance'),
                (SELECT count(*) FROM workflow_display_steps step
                 WHERE json_extract(step.data,'$.id') IS NOT step.execution_id
                    OR NOT EXISTS (
                        SELECT 1 FROM resource_versions rev
                        WHERE rev.kind='workflow_step'
                          AND rev.resource_id=json_array(step.instance_id,step.execution_id)
                          AND rev.revision=step.step_revision)
                    OR NOT EXISTS (
                        SELECT 1 FROM governance_records plan
                        WHERE plan.id=json_extract(step.data,'$.plan_revision_id')
                          AND plan.type='workflow_plan_revision'
                          AND json_extract(plan.data,'$.instance_id')=step.instance_id))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(storage)?;
    require(
        headers == rebuilt
            && steps == source_steps
            && invalid_keys == 0
            && event_cursor(&tx)? == initial,
        "流程展示回填的数量、引用或事件位置不一致",
    )?;
    tx.commit().map_err(storage)?;
    Ok(
        json!({"instances":headers,"steps":steps,"event_cursor":initial,"row_cursor":cursor,"batches":batches}),
    )
}
