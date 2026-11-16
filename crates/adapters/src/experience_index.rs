//! Read experience metadata and immutable snapshot text without rebuilding execution history.
use super::*;
use rusqlite::OptionalExtension;

pub(super) fn entries(
    root: &Path,
    project: Option<&str>,
    agent: &str,
    enrich: bool,
) -> Result<Vec<Value>> {
    let path = match project {
        None => root.join("global.sqlite3"),
        Some(GLOBAL_DISCUSSION_NAMESPACE) => {
            SqliteStore::global_discussion_path(root)?.join("engineering.sqlite3")
        }
        Some(id) => SqliteStore::project_path(root, id)?.join("engineering.sqlite3"),
    };
    let conn = connection_pool::ManagedConnection::read_only(&path)?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let mut query=conn.prepare("SELECT data FROM governance_records WHERE type='experience' AND json_extract(data,'$.agent_instance_id')=?1 ORDER BY rowid").map_err(storage)?;
    let mut rows = query
        .query_map([agent], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .map(|v| serde_json::from_str::<Value>(&v.map_err(storage)?).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    if rows.is_empty() || project.is_none() {
        return Ok(rows);
    }
    // Restore only the selected experience records against source revisions from this same read transaction.
    let mut state = EngineeringState {
        governance: source_store::load_catalog_records(&conn)?,
        ..Default::default()
    };
    state.refresh_configuration_views()?;
    let mut memo = Default::default();
    for row in &mut rows {
        *row = source_views::restore_with(&state, row, &mut memo)?;
        row["availability"] = json!("available");
        if !enrich {
            continue;
        }
        let session = row["session_id"].as_str().unwrap_or("");
        let source:Option<String>=conn.query_row("SELECT data FROM governance_records WHERE type='configured_session' AND definition_id=?1 ORDER BY rowid LIMIT 1",[session],|r|r.get(0)).optional().map_err(storage)?;
        if let Some(source) = source {
            let source: Value = serde_json::from_str(&source).map_err(storage)?;
            row["workflow_instance_id"] = source["instance_id"].clone();
            row["step_execution_id"] = source["execution_id"].clone();
            row["plan_revision_id"] = source["plan_revision_id"].clone();
            let instance = source["instance_id"].as_str().unwrap_or("");
            let workflow: Option<(String, String)> = conn
                .query_row(
                    "SELECT name,data FROM governance_records WHERE id=?1",
                    [instance],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(storage)?;
            let mut plans = std::collections::BTreeSet::new();
            if let Some((name, data)) = workflow {
                row["workflow_name"] = json!(name);
                let data: Value = serde_json::from_str(&data).map_err(storage)?;
                for step in data["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|step| step["id"] == source["execution_id"])
                {
                    if let Some(plan) = step["plan_revision_id"].as_str() {
                        plans.insert(plan.to_owned());
                    }
                }
            }
            if row["plan_revision_id"].is_null() {
                let mut query=conn.prepare("SELECT json_extract(data,'$.plan_revision_id') FROM governance_records WHERE type='workflow_execution_history' AND json_extract(data,'$.workflow_instance_id')=?1 AND json_extract(data,'$.id')=?2").map_err(storage)?;
                for plan in query
                    .query_map(
                        rusqlite::params![instance, source["execution_id"].as_str().unwrap_or("")],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .map_err(storage)?
                {
                    if let Some(plan) = plan.map_err(storage)? {
                        plans.insert(plan);
                    }
                }
                if plans.len() == 1 {
                    row["plan_revision_id"] = json!(plans.first());
                }
            }
        }
        let prefix: Option<String> = conn
            .query_row(
                "SELECT substr(content,1,320) FROM snapshots WHERE id=?1",
                [row["snapshot_id"].as_str().unwrap_or("")],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(prefix) = prefix {
            row["summary"] = json!(prefix);
        }
        // content_digest is already sealed in the immutable experience fact. Detail reads still verify the manifest.
    }
    Ok(rows)
}
