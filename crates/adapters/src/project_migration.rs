//! Explicit offline project database upgrades. The original remains usable
//! until the candidate and every new payload reference have been checked.
use super::*;
use rusqlite::{OptionalExtension, params};

fn database_len(path: &Path) -> Result<u64> {
    Ok(path.metadata().map_err(storage)?.len())
}

fn validate(conn: &Connection, project: &str) -> Result<()> {
    check_dataset_layout(conn)?;
    let actual: String = conn
        .query_row("SELECT id FROM projects", [], |r| r.get(0))
        .map_err(storage)?;
    require(actual == project, "待迁移库的项目身份不符")?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(storage)?;
    require(integrity == "ok", "工程库完整性检查失败")?;
    let foreign_keys = conn
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()
        .map_err(storage)?;
    require(foreign_keys.is_none(), "工程库外键检查失败")
}

fn upgrade(conn: &mut Connection, root: &Path) -> Result<Value> {
    let tx = conn.transaction().map_err(storage)?;
    let triggers = {
        let mut query = tx.prepare(
            "SELECT name,sql FROM sqlite_schema WHERE type='trigger'
             AND tbl_name IN ('governance_records','conversation_events','operations') ORDER BY name",
        ).map_err(storage)?;
        query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    for (name, _) in &triggers {
        tx.execute_batch(&format!("DROP TRIGGER \"{}\"", name.replace('"', "\"\"")))
            .map_err(storage)?;
    }
    tx.pragma_update(None, "user_version", PROJECT_SCHEMA_VERSION)
        .map_err(storage)?;
    tx.execute_batch(
        "CREATE TABLE payload_pack_objects (
         sha256 TEXT PRIMARY KEY, pack TEXT NOT NULL,
         offset INTEGER NOT NULL CHECK(offset >= 0), bytes INTEGER NOT NULL CHECK(bytes >= 0)
         ) WITHOUT ROWID;
         CREATE INDEX idx_payload_pack_file ON payload_pack_objects(pack);",
    )
    .map_err(storage)?;
    let mut counts = serde_json::Map::new();
    for (table, kind, column, filter) in [
        ("governance_records", "type", "data", "1"),
        ("operations", "''", "result", "1"),
        (
            "conversation_events",
            "kind",
            "content",
            "kind IN ('user','tool')",
        ),
    ] {
        let mut cursor = 0;
        let mut changed = 0u64;
        let mut select = tx
            .prepare(&format!(
                "SELECT rowid,{kind},CAST({column} AS TEXT) FROM {table}
             WHERE rowid>?1 AND {filter} ORDER BY rowid LIMIT 128"
            ))
            .map_err(storage)?;
        let mut update = tx
            .prepare(&format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2"))
            .map_err(storage)?;
        loop {
            let rows = select
                .query_map([cursor], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?;
            if rows.is_empty() {
                break;
            }
            for (rowid, kind, raw) in rows {
                cursor = rowid;
                let stored = if table == "conversation_events" {
                    let stored = payload_store::pack_text(root, &raw)?;
                    require(
                        payload_store::unpack_text(&tx, stored.clone())? == raw,
                        "消息迁移正文不符",
                    )?;
                    stored
                } else {
                    let original: Value = serde_json::from_str(&raw).map_err(storage)?;
                    let mut value = original.clone();
                    if table == "operations" {
                        payload_store::result(root, &mut value)?;
                    } else {
                        let manifest = if kind == "workflow_output" {
                            load_table_filtered_params::<Artifact>(
                                &tx,
                                "artifacts",
                                "id=?1",
                                &[&original["artifact_id"].as_str()],
                            )?
                            .pop()
                        } else {
                            None
                        };
                        payload_store::governance(root, &kind, &mut value, manifest.as_ref())?;
                    }
                    if value == original {
                        continue;
                    }
                    let encoded = value.to_string();
                    if table == "operations" {
                        value = payload_store::decode_result(&tx, &encoded)?;
                    } else {
                        payload_store::restore_governance(&tx, &kind, &mut value)?;
                    }
                    require(value == original, "结构化正文迁移不符")?;
                    encoded
                };
                if stored != raw {
                    changed += update.execute(params![stored, rowid]).map_err(storage)? as u64;
                }
            }
        }
        counts.insert(table.into(), json!(changed));
    }
    for (_, sql) in triggers {
        tx.execute_batch(&sql).map_err(storage)?;
    }
    // Keep the published shape of each event. The physical list shares field
    // names by position instead of repeating them for every changed resource.
    let events = tx
        .execute(
            "UPDATE events SET detail=(SELECT json_group_array(json_array(
          json_extract(value,'$.resource.kind'),json_extract(value,'$.resource.id'),
          json_extract(value,'$.revision'))) FROM json_each(events.detail))
         WHERE kind='resource.changed' AND json_valid(detail)
         AND json_type(detail,'$[0]')='object'
         AND id IN (SELECT cursor FROM operations WHERE cursor>0 AND versions=?1)",
            [project_storage::EVENT_VERSIONS],
        )
        .map_err(storage)?;
    counts.insert("events".into(), json!(events));
    tx.execute_batch(
        "PRAGMA legacy_alter_table=ON;
         CREATE TABLE compact_operations(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,
         result JSON NOT NULL CHECK(json_valid(result)),versions JSON NOT NULL DEFAULT '[]' CHECK(json_valid(versions)),
         cursor INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID;
         INSERT INTO compact_operations SELECT * FROM operations;
         DROP TABLE operations;
         ALTER TABLE compact_operations RENAME TO operations;
         CREATE TABLE compact_versions(kind TEXT NOT NULL,resource_id TEXT NOT NULL,
         revision INTEGER NOT NULL CHECK(revision>=0),PRIMARY KEY(kind,resource_id)) WITHOUT ROWID;
         INSERT INTO compact_versions SELECT * FROM resource_versions;
         DROP TABLE resource_versions;
         ALTER TABLE compact_versions RENAME TO resource_versions;
         CREATE INDEX idx_step_authority_execution ON resource_versions(json_extract(resource_id,'$[1]'))
         WHERE kind='workflow_step_authority';
         DROP INDEX idx_runtime_work_seek;
         DROP INDEX idx_governance_scope;
         CREATE INDEX idx_runtime_work_seek ON governance_records(type,json_extract(data,'$.status'),id)
         WHERE json_extract(data,'$.status') IS NOT NULL;
         PRAGMA legacy_alter_table=OFF;",
    ).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(Value::Object(counts))
}

pub fn migrate(data: &Path, project: &str) -> Result<Value> {
    require(
        data.join("global.sqlite3").is_file(),
        "需要已初始化的 DevDesk 数据集",
    )?;
    let (data, _dataset_lock) = lock_dataset(data)?;
    let root = SqliteStore::project_path(&data, project)?;
    let live = root.join("engineering.sqlite3");
    reject_symlink(&live)?;
    require(live.is_file(), "工程库不存在")?;
    let (root, _project_lock) = lock_dataset(&root)?;
    let source = Connection::open(&live).map_err(storage)?;
    let version: u32 = source
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage)?;
    if version == PROJECT_SCHEMA_VERSION {
        return Ok(
            json!({"project_id":project,"version":version,"changed":false,"bytes":database_len(&live)?}),
        );
    }
    require(
        version == LEGACY_PROJECT_SCHEMA_VERSION,
        "工程库版本不支持此迁移",
    )?;
    let busy: u64 = source
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .map_err(storage)?;
    require(busy == 0, "工程库仍有读写连接")?;
    validate(&source, project)?;
    let before = database_len(&live)?;
    let staged = root.join(format!(".engineering-{}.pending", id()));
    let outcome = (|| -> Result<Value> {
        source
            .execute("VACUUM INTO ?1", [staged.to_str()])
            .map_err(storage)?;
        source.close().map_err(|(_, e)| storage(e))?;
        let mut candidate = Connection::open(&staged).map_err(storage)?;
        candidate
            .execute_batch(
                "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
            )
            .map_err(storage)?;
        let counts = payload_store::stage(|| upgrade(&mut candidate, &root))?;
        validate(&candidate, project)?;
        candidate
            .pragma_update(None, "page_size", 8192)
            .map_err(storage)?;
        candidate.execute_batch("VACUUM").map_err(storage)?;
        validate(&candidate, project)?;
        candidate.close().map_err(|(_, e)| storage(e))?;
        File::open(&staged)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        let after = database_len(&staged)?;
        let backup_dir = data
            .parent()
            .ok_or_else(|| err("invalid_params", "数据集没有备份父目录"))?
            .join(".devdesk-storage-backups");
        reject_symlink(&backup_dir)?;
        fs::create_dir_all(&backup_dir).map_err(storage)?;
        let backup = backup_dir.join(format!("{project}-engineering-v{version}-{}.sqlite3", id()));
        fs::hard_link(&live, &backup).map_err(storage)?;
        File::open(&backup_dir)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        File::open(backup_dir.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        fs::rename(&staged, &live).map_err(storage)?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        Ok(
            json!({"project_id":project,"version":PROJECT_SCHEMA_VERSION,"changed":true,
            "before_bytes":before,"after_bytes":after,"backup":backup,"records":counts}),
        )
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(staged);
    }
    outcome
}

pub fn compact(data: &Path, project: &str) -> Result<Value> {
    require(
        data.join("global.sqlite3").is_file(),
        "需要已初始化的 DevDesk 数据集",
    )?;
    let (data, _dataset_lock) = lock_dataset(data)?;
    let root = SqliteStore::project_path(&data, project)?;
    let live = root.join("engineering.sqlite3");
    reject_symlink(&live)?;
    require(live.is_file(), "工程库不存在")?;
    let (root, _project_lock) = lock_dataset(&root)?;
    let mut connection =
        Connection::open_with_flags(&live, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(storage)?;
    require(payload_store::enabled(&connection)?, "请先执行项目正文迁移")?;
    validate(&connection, project)?;
    connection
        .execute_batch("PRAGMA synchronous=FULL;")
        .map_err(storage)?;
    let report = payload_store::compact(&mut connection, &root)?;
    if report["packed_objects"].as_u64().is_some_and(|n| n > 0) {
        connection.execute_batch("VACUUM").map_err(storage)?;
    }
    storage_maintenance::checkpoint(&connection)?;
    Ok(report)
}
