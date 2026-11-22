//! Read experience metadata and immutable snapshot text without rebuilding execution history.
use super::*;
use rusqlite::OptionalExtension;
use std::collections::BTreeSet;

pub(super) fn source_path(root: &Path, project: &str) -> Result<PathBuf> {
    if project == GLOBAL_DISCUSSION_NAMESPACE {
        SqliteStore::global_discussion_path(root)
    } else {
        SqliteStore::project_path(root, project)
    }
}

pub(super) fn deleted(root: &Path, agent: &str) -> Result<BTreeSet<(String, String)>> {
    let conn = connection_pool::ManagedConnection::read_only(&root.join("global.sqlite3"))?;
    let mut query = conn.prepare(
        "SELECT json_extract(data,'$.source_project_id'),json_extract(data,'$.snapshot_id') FROM governance_records WHERE type='experience_deleted' AND definition_id=?1",
    ).map_err(storage)?;
    query
        .query_map([agent], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}

pub(super) fn entries(
    root: &Path,
    project: Option<&str>,
    agent: &str,
    enrich: bool,
    snapshot: Option<&str>,
) -> Result<Vec<Value>> {
    let path = match project {
        None => root.join("global.sqlite3"),
        Some(id) => source_path(root, id)?.join("engineering.sqlite3"),
    };
    let conn = connection_pool::ManagedConnection::read_only(&path)?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let sql = if project.is_none() {
        "SELECT data FROM governance_records WHERE type='experience' AND json_extract(data,'$.agent_instance_id')=?1 UNION ALL SELECT json_extract(data,'$.value') FROM governance_records WHERE type='experience_directory' AND definition_id=?1 AND json_extract(data,'$.value.agent_instance_id')=?1"
    } else {
        "SELECT data FROM governance_records WHERE type='experience' AND json_extract(data,'$.agent_instance_id')=?1 ORDER BY rowid"
    };
    let sql = format!(
        "WITH entries(data) AS ({sql})
         SELECT json_remove(json_set(data,'$.history_count',coalesce(json_extract(data,'$.history_count'),json_array_length(data,'$.history'),0)),'$.history') FROM entries
         WHERE ?2 IS NULL OR json_extract(data,'$.snapshot_id')=?2
         UNION ALL
         SELECT h.value FROM entries,json_each(entries.data,'$.history') h
         WHERE ?2 IS NOT NULL AND json_extract(h.value,'$.snapshot_id')=?2"
    );
    let mut query = conn.prepare(&sql).map_err(storage)?;
    let mut rows = query
        .query_map([Some(agent), snapshot], |r| r.get::<_, String>(0))
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
        if let Some(feature_id) = row["feature_id"].as_str() {
            let title: Option<String> = conn
                .query_row(
                    "SELECT title FROM features WHERE id=?1 AND project_id=?2",
                    rusqlite::params![feature_id, project],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(title) = title {
                row["feature_name"] = json!(title);
            }
        }
        let session = row["session_id"].as_str().unwrap_or("");
        let source:Option<String>=conn.query_row("SELECT data FROM governance_records WHERE type='configured_session' AND definition_id=?1 ORDER BY rowid LIMIT 1",[session],|r|r.get(0)).optional().map_err(storage)?;
        if let Some(source) = source {
            let source: Value = serde_json::from_str(&source).map_err(storage)?;
            for (target, field) in [
                ("workflow_instance_id", "instance_id"),
                ("step_execution_id", "execution_id"),
                ("plan_revision_id", "plan_revision_id"),
            ] {
                if row[target].is_null() {
                    row[target] = source[field].clone();
                }
            }
            let instance = row["workflow_instance_id"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            let workflow: Option<(String, String)> = conn
                .query_row(
                    "SELECT name,data FROM governance_records WHERE id=?1",
                    [&instance],
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
                    .filter(|step| step["id"] == row["step_execution_id"])
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
                        rusqlite::params![
                            instance,
                            row["step_execution_id"].as_str().unwrap_or("")
                        ],
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
