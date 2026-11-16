//! Bounded display metadata from authoritative experience rows. No source view
//! reconstruction, execution history or snapshot body is read here.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::read_models::*;
use rusqlite::{OptionalExtension, params};
const PAGE: usize = 16;
const OWNER: &str = "owner_project IS ?1";

pub(super) fn headers(
    pool: &ConnectionPool,
    project: Option<&str>,
    after: Option<&str>,
) -> Result<ExperienceHeaderPage> {
    let _span = performance::Span::new("read_model.experience_headers");
    let conn = pool.read()?;
    let mut query=conn.prepare_cached(&format!("SELECT id,revision FROM governance_records WHERE type='experience' AND {OWNER} AND id>?2 ORDER BY id LIMIT 17")).map_err(storage)?;
    let mut records = query
        .query_map(params![project, after.unwrap_or("")], |row| {
            Ok(ExperienceHeader {
                id: row.get(0)?,
                revision: row.get(1)?,
            })
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = records.len() > PAGE;
    records.truncate(PAGE);
    let next_after = more.then(|| records.last().unwrap().id.clone());
    Ok(ExperienceHeaderPage {
        records,
        next_after,
    })
}
fn metadata(expr: &str) -> String {
    let mut fields = vec![];
    for key in [
        "agent_instance_id",
        "agent_revision_id",
        "snapshot_id",
        "session_id",
        "project_id",
        "feature_id",
        "task_id",
        "workflow_instance_id",
        "step_execution_id",
        "plan_revision_id",
        "content_digest",
        "captured_at",
        "work_time",
        "kind",
    ] {
        fields.push(format!("'{key}',json_extract({expr},'$.{key}')"));
    }
    for key in ["project_name", "feature_name"] {
        fields.push(format!(
            "'{key}',substr(json_extract({expr},'$.{key}'),1,128)"
        ));
    }
    // Model/provider selection is useful in the unavailable-origin directory.
    // Paths, credentials, compiled permissions and source bodies stay at origin.
    fields.push(format!("'binding',json_object('model',json_extract({expr},'$.binding.model'),'work_type',json_extract({expr},'$.binding.work_type'),'installation',json_object('key',json_extract({expr},'$.binding.installation.key'))),'metadata_excerpt',json('true')"));
    format!("json_object({})", fields.join(","))
}
pub(super) fn source(
    pool: &ConnectionPool,
    project: Option<&str>,
    id: &str,
) -> Result<Option<ExperienceDirectorySource>> {
    let _span = performance::Span::new("read_model.experience_directory_source");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let sql = format!(
        "SELECT id,revision,scope,content_hash,substr(name,1,128),{},(SELECT json_group_array(json(item)) FROM (SELECT {} AS item FROM json_each(r.data,'$.history') h ORDER BY json_extract(h.value,'$.captured_at') DESC,CAST(h.key AS INTEGER) DESC LIMIT 20)),coalesce(json_array_length(data,'$.history'),0),coalesce(json_type(data,'$.history'),'null') NOT IN ('array','null') FROM governance_records r WHERE type='experience' AND {OWNER} AND id=?2",
        metadata("r.data"),
        metadata("h.value")
    );
    let row = conn
        .query_row(&sql, params![project, id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, u64>(7)?,
                row.get::<_, bool>(8)?,
            ))
        })
        .optional()
        .map_err(storage)?;
    let result = row
        .map(
            |(
                id,
                revision,
                scope,
                content_hash,
                name,
                metadata,
                history,
                history_count,
                invalid_history,
            )| {
                require(!invalid_history, "经验历史必须为数组")?;
                let mut metadata: Value = serde_json::from_str(&metadata).map_err(storage)?;
                metadata["history"] = serde_json::from_str(&history).map_err(storage)?;
                metadata["history_more"] = json!(history_count > 20);
                metadata["history_count"] = json!(history_count);
                require(
                    serde_json::to_vec(&metadata).map_err(storage)?.len() <= 64 * 1024,
                    "经验目录节选超过 64 KiB",
                )?;
                Ok(ExperienceDirectorySource {
                    id,
                    revision,
                    scope: serde_json::from_str(&scope).map_err(storage)?,
                    content_hash,
                    name,
                    metadata,
                })
            },
        )
        .transpose()?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}
