//! Metadata pages and exact records in one project. No aggregate state restore.
use crate::{connection_pool::ConnectionPool, *};
use rusqlite::params;
const KINDS: &[&str] = &[
    "projects",
    "features",
    "tasks",
    "documents",
    "sessions",
    "contexts",
    "snapshots",
    "heads",
    "artifacts",
    "events",
    "turns",
    "conversation_events",
    "interactions",
    "proposals",
    "executions",
    "repository_operations",
    "repository_worktrees",
    "governance",
    "orchestration_leases",
    "orchestration_wakeups",
];
fn table(kind: &str) -> Result<&str> {
    require(KINDS.contains(&kind), "未知项目记录类型")?;
    Ok(if kind == "governance" {
        "governance_records"
    } else {
        kind
    })
}
fn metadata(conn: &Connection, kind: &str) -> Result<String> {
    let table = table(kind)?;
    let columns = columns(conn, table)?;
    let has = |key: &str| columns.iter().any(|(name, _)| name == key);
    let col = |key: &str| {
        if has(key) {
            format!("\"{key}\"")
        } else {
            "NULL".into()
        }
    };
    Ok(format!(
        "SELECT id,{} AS type,coalesce({},{}) AS name,coalesce({},{}) AS revision,{} AS status,{} AS content_hash FROM {table}",
        col("type"),
        col("name"),
        col("title"),
        col("revision"),
        col("version"),
        if has("status") {
            col("status")
        } else if has("data") {
            "json_extract(data,'$.status')".into()
        } else {
            "NULL".into()
        },
        col("content_hash")
    ))
}
pub(super) fn read(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    method: &str,
    p: &Value,
) -> Result<Value> {
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "项目读取需要当前源目录"));
    }
    let result = if method == "project.overview" {
        let projects = load_table::<Project>(&conn, "projects")?;
        require(projects.len() == 1, "项目资料需要唯一实际 Project")?;
        let mut counts = serde_json::Map::new();
        for kind in KINDS {
            let count: u64 = conn
                .query_row(&format!("SELECT count(*) FROM {}", table(kind)?), [], |r| {
                    r.get(0)
                })
                .map_err(storage)?;
            counts.insert(
                (*kind).into(),
                json!(
                    count
                        + if *kind == "governance" {
                            catalog
                                .state
                                .governance
                                .iter()
                                .filter(|r| r.is_definition())
                                .count() as u64
                        } else {
                            0
                        }
                ),
            );
        }
        json!({"project":projects[0],"counts":counts,"record_kinds":KINDS,"overview_only":true,"continuation":{"list":"project.records.list","read":"project.record.read","artifact":"project.artifact.read"},"snapshot_digest":configuration_hash(&counts)})
    } else {
        let kind = p["kind"]
            .as_str()
            .ok_or_else(|| err("invalid_params", "需要记录类型"))?;
        let table = table(kind)?;
        if method == "project.record.read" {
            let id = p["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| err("invalid_params", "需要原记录 ID"))?;
            let raw = if kind == "governance" {
                catalog.record(id).map(|r| json!(r))
            } else {
                None
            };
            let raw = match raw {
                Some(raw) => raw,
                None => load_table_filtered_params::<Value>(&conn, table, "id=?1", &[&id])?
                    .pop()
                    .ok_or_else(|| err("not_found", "原项目记录不存在"))?,
            };
            // Formal task bodies use the authenticated original user ledger.
            let task = if matches!(
                raw["type"].as_str(),
                Some("configured_execution_task" | "archived_configured_execution_task")
            ) {
                Some(
                    raw["definition_id"]
                        .as_str()
                        .ok_or_else(|| err("storage", "原任务轮次缺失"))?
                        .to_owned(),
                )
            } else {
                None
            };
            let cursor = conn
                .query_row("SELECT coalesce(max(id),0) FROM events", [], |r| {
                    r.get::<_, u64>(0)
                })
                .map_err(storage)?;
            conn.execute_batch("COMMIT;").map_err(storage)?;
            drop(conn);
            let value = if let Some(turn) = task {
                let mut row = raw;
                row["data"] = workflow_details::task(
                    pool,
                    catalog,
                    catalog
                        .state
                        .projects
                        .first()
                        .ok_or_else(|| err("storage", "任务缺少项目"))?
                        .id
                        .as_str(),
                    &turn,
                )?
                .ok_or_else(|| err("not_found", "原任务缺失"))?["task"]
                    .clone();
                row
            } else {
                source_views::restore(&catalog.state, &raw)?
            };
            return Ok(json!({"record":value,"cursor":cursor}));
        }
        require(method == "project.records.list", "未知原生项目资料读取")?;
        let offset = p["offset"].as_u64().unwrap_or(0).min(1_000_000);
        let limit = p["limit"].as_u64().unwrap_or(32).clamp(1, 100);
        let family = p["type"].as_str();
        let search = p["search"].as_str();
        require(search.is_none_or(|s| s.len() <= 1024), "搜索超出预算")?;
        let mut sql = metadata(&conn, kind)?;
        if kind == "governance" {
            sql.push_str(" UNION ALL SELECT reference AS id,type,name,revision,NULL AS status,NULL AS content_hash FROM source_definitions");
        }
        let filter = "(?1 IS NULL OR type=?1) AND (?2 IS NULL OR instr(coalesce(name,'')||' '||CAST(id AS TEXT),?2)>0)";
        let total: u64 = conn
            .query_row(
                &format!("SELECT count(*) FROM ({sql}) WHERE {filter}"),
                params![family, search],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let clock = resource_commits::read_on(
            &conn,
            &[devdesk_core::concurrency::ResourceKey::new(
                "collection",
                format!("record-summary:{kind}"),
            )],
        )?
        .remove(0)
        .revision;
        let digest = configuration_hash(&(
            kind,
            clock,
            if kind == "governance" {
                catalog.revision
            } else {
                0
            },
            family,
            search,
        ));
        require(
            p["snapshot_digest"].as_str().is_none_or(|v| v == digest),
            "列表摘要已变化，请重新分页",
        )?;
        let mut q=conn.prepare_cached(&format!("SELECT json_object('id',id,'type',type,'name',name,'revision',revision,'status',status,'content_hash',content_hash) FROM ({sql}) WHERE {filter} ORDER BY CAST(id AS TEXT) LIMIT ?3 OFFSET ?4")).map_err(storage)?;
        let mut rows = q
            .query_map(params![family, search, limit, offset], |r| {
                r.get::<_, String>(0)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
            .into_iter()
            .map(|s| serde_json::from_str::<Value>(&s).map_err(storage))
            .collect::<Result<Vec<_>>>()?;
        if kind == "governance" {
            for row in &mut rows {
                if let Some(source) = row["id"].as_str().and_then(|id| catalog.record(id)) {
                    row["content_hash"] = json!(source.content_hash);
                }
            }
        }
        json!({"kind":kind,"records":rows,"total":total,"offset":offset,"next_offset":if offset+limit<total{Some(offset+limit)}else{None},"snapshot_digest":digest,"complete":offset+limit>=total})
    };
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}
