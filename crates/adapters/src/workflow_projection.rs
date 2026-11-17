//! Transactional display metadata. Canonical runtime facts remain resource
//! rows; these small projections never authorize control or replace sources.
use crate::*;
use devdesk_core::concurrency::*;
use std::collections::BTreeSet;
const STEP_FIELDS: &[&str] = &[
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
