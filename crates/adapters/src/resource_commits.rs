//! Finite resource reads and atomic optimistic commits over existing fact tables.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::concurrency::*;
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::time::{Duration, Instant};

fn table(kind: &str) -> Result<&'static str> {
    match kind {
        "project" => Ok("projects"),
        "feature" => Ok("features"),
        "task" => Ok("tasks"),
        "context" => Ok("contexts"),
        "document" => Ok("documents"),
        "session" => Ok("sessions"),
        "turn" => Ok("turns"),
        "interaction" => Ok("interactions"),
        "proposal" => Ok("proposals"),
        "execution" => Ok("executions"),
        "repository_operation" => Ok("repository_operations"),
        "repository_worktree" => Ok("repository_worktrees"),
        "snapshot" => Ok("snapshots"),
        "artifact" => Ok("artifacts"),
        "head" => Ok("heads"),
        "governance" => Ok("governance_records"),
        "runtime_installation" => Ok("runtime_installations"),
        "project_directory" => Ok("project_catalog"),
        _ => Err(err("invalid_params", "不支持的资源类型")),
    }
}

fn value(conn: &Connection, key: &ResourceKey) -> Result<Option<Value>> {
    key.validate()?;
    match key.kind.as_str() {
        // Collection tokens have no entity payload. Their version is maintained
        // by fact-table triggers and is read atomically with the selected facts.
        "collection" => Ok(None),
        "runtime_presence" => Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM runtime_installations WHERE id=?1)",[&key.id],|row|row.get::<_,bool>(0)).map_err(storage)?.then_some(json!(true))),
        "setting" => catalog::setting(conn, &key.id),
        "source_head" => conn.query_row(
            "SELECT reference,type,revision FROM source_definitions WHERE identity=?1 ORDER BY revision DESC LIMIT 1",
            [&key.id],
            |r| Ok(json!({"reference":r.get::<_,String>(0)?,"type":r.get::<_,String>(1)?,"revision":r.get::<_,u64>(2)?})),
        ).optional().map_err(storage),
        "source" => {
            let (identity, revision) = parse_source_reference(&key.id)?;
            conn.query_row(
                "SELECT type,name,scope,refs,data FROM source_definitions WHERE identity=?1 AND revision=?2",
                rusqlite::params![identity,revision],
                |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)),
            ).optional().map_err(storage)?.map(|(kind,name,scope,refs,data)| {
                serde_json::to_value(SourceDefinition {
                    r#type:kind,identity:identity.into(),revision,name,
                    scope:serde_json::from_str(&scope).map_err(storage)?,
                    refs:serde_json::from_str(&refs).map_err(storage)?,
                    data:serde_json::from_str(&data).map_err(storage)?,
                }).map_err(storage)
            }).transpose()
        }
        kind => Ok(load_table_filtered_params::<Value>(conn, table(kind)?, "id=?1", &[&key.id])?
            .into_iter().next()),
    }
}

fn snapshot(conn: &Connection, key: &ResourceKey) -> Result<ResourceSnapshot> {
    let value = value(conn, key)?;
    Ok(ResourceSnapshot {
        resource: key.clone(),
        revision: current_revision(conn, key)?,
        value,
    })
}

fn current_revision(conn: &Connection, key: &ResourceKey) -> Result<u64> {
    key.validate()?;
    if key.kind == "runtime_presence" {
        return conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_installations WHERE id=?1)",
                [&key.id],
                |row| row.get(0),
            )
            .map_err(storage);
    }
    if let Some(revision) = conn
        .query_row(
            "SELECT revision FROM resource_versions WHERE kind=?1 AND resource_id=?2",
            rusqlite::params![key.kind, key.id],
            |r| r.get::<_, u64>(0),
        )
        .optional()
        .map_err(storage)?
    {
        return Ok(revision);
    }
    let revision = match key.kind.as_str() {
        "collection" => return Ok(0),
        "setting" => conn.query_row("SELECT 1 FROM settings WHERE key=?1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        "source_head" => conn.query_row("SELECT revision FROM source_definitions WHERE identity=?1 ORDER BY revision DESC LIMIT 1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        "source" => conn.query_row("SELECT 1 FROM source_definitions WHERE reference=?1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        kind => {
            let expression = if matches!(kind,"project"|"feature"|"task"|"context"|"turn"|"interaction"|"proposal"|"governance"|"repository_operation") { "revision" } else { "1" };
            conn.query_row(&format!("SELECT {expression} FROM {} WHERE id=?1",table(kind)?),[&key.id],|r|r.get::<_,u64>(0)).optional()
        }
    }.map_err(sqlite_error)?;
    Ok(revision.unwrap_or(0))
}

pub(super) fn read(pool: &ConnectionPool, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
    require(keys.len() <= 4096, "一次读取的资源过多")?;
    let _span = performance::Span::new("resource.read");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let values = read_on(&conn, keys)?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(values)
}

/// Compose several indexed facts and predicate tokens in the caller's one read snapshot.
pub(super) fn read_on(conn: &Connection, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
    require(keys.len() <= 4096, "一次读取的资源过多")?;
    keys.iter().map(|key| snapshot(conn, key)).collect()
}

fn sqlite_error(error: rusqlite::Error) -> Error {
    if matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ) {
        err("storage_busy", "数据库短事务正在竞争")
    } else {
        storage(error)
    }
}

fn replay_on(
    conn: &Connection,
    command_id: &str,
    expected_fingerprint: &str,
) -> Result<Option<CommitReceipt>> {
    let previous = conn
        .query_row(
            "SELECT fingerprint,CAST(result AS TEXT),versions,cursor FROM operations WHERE id=?1",
            [command_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error)?;
    previous
        .map(|(fingerprint, result, versions, cursor)| {
            if fingerprint != expected_fingerprint {
                return Err(err("invalid_params", "command_id 被不同参数或身份复用"));
            }
            Ok(CommitReceipt {
                result: serde_json::from_str(&result).map_err(storage)?,
                versions: serde_json::from_str(&versions).map_err(storage)?,
                cursor,
            })
        })
        .transpose()
}

pub(super) fn replay(
    pool: &ConnectionPool,
    command_id: &str,
    fingerprint: &str,
) -> Result<Option<CommitReceipt>> {
    let conn = pool.read()?;
    replay_on(&conn, command_id, fingerprint)
}

struct SqlWrite {
    sql: String,
    values: Vec<SqlValue>,
}

type ColumnSpecs = std::collections::BTreeMap<&'static str, Vec<(String, String)>>;

fn encode_column(value: &Value, kind: &str) -> Result<SqlValue> {
    if value.is_null() {
        return Ok(SqlValue::Null);
    }
    if kind == "JSON" {
        return Ok(SqlValue::Text(value.to_string()));
    }
    match value {
        Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        Value::Number(value) => value
            .as_i64()
            .map(SqlValue::Integer)
            .ok_or_else(|| err("invalid_params", "整数值超出 SQLite 存储范围")),
        Value::String(value) => Ok(SqlValue::Text(value.clone())),
        _ => Err(err("invalid_params", "结构化值需要 JSON 列")),
    }
}

fn prepare_write(change: &ResourceChange, specs: &ColumnSpecs) -> Result<Option<SqlWrite>> {
    if change.resource.kind == "source_head" {
        return Ok(None);
    }
    if change.resource.kind == "setting" {
        return Ok(Some(SqlWrite {
            sql: "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value".into(),
            values: vec![SqlValue::Text(change.resource.id.clone()), SqlValue::Text(change.value.to_string())],
        }));
    }
    if change.resource.kind == "source" {
        let source: SourceDefinition =
            serde_json::from_value(change.value.clone()).map_err(storage)?;
        require(
            source.revision <= i64::MAX as u64,
            "源修订超出 SQLite 存储范围",
        )?;
        require(
            source.clone().into_record()?.id == change.resource.id,
            "源身份与提交目标不一致",
        )?;
        return Ok(Some(SqlWrite {
            sql: "INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES(?1,?2,?3,?4,?5,?6,?7)".into(),
            values: vec![
                SqlValue::Text(source.identity), SqlValue::Integer(source.revision as i64),
                SqlValue::Text(source.r#type), SqlValue::Text(source.name),
                SqlValue::Text(serde_json::to_string(&source.scope).map_err(storage)?),
                SqlValue::Text(serde_json::to_string(&source.refs).map_err(storage)?),
                SqlValue::Text(source.data.to_string()),
            ],
        }));
    }
    require(
        change.value["id"].as_str() == Some(&change.resource.id),
        "对象身份与提交目标不一致",
    )?;
    let table = table(&change.resource.kind)?;
    let cols = &specs[table];
    let names: Vec<_> = cols.iter().map(|(name, _)| format!("\"{name}\"")).collect();
    let placeholders: Vec<_> = (1..=cols.len()).map(|n| format!("?{n}")).collect();
    let updates: Vec<_> = names
        .iter()
        .filter(|name| name.as_str() != "\"id\"")
        .map(|name| format!("{name}=excluded.{name}"))
        .collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        names.join(","),
        placeholders.join(","),
        updates.join(",")
    );
    let values = cols
        .iter()
        .map(|(name, kind)| encode_column(&change.value[name], kind))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(SqlWrite { sql, values }))
}

fn prepare_writes(
    pool: &ConnectionPool,
    prepared: &PreparedChangeSet,
) -> Result<Vec<Option<SqlWrite>>> {
    let changed: std::collections::BTreeSet<_> = prepared
        .changes
        .iter()
        .map(|change| &change.resource)
        .collect();
    for change in &prepared.changes {
        if change.resource.kind == "runtime_installation" {
            if prepared.read_set.require(&change.resource)? > 0 {
                require(
                    prepared
                        .read_set
                        .require(&ResourceKey::new("runtime_presence", &change.resource.id))?
                        == 0,
                    "恢复安装必须验证记录确实缺失",
                )?;
                require(
                    prepared.read_set.require(&ResourceKey::new(
                        "setting",
                        format!("runtime.seal.{}", change.resource.id),
                    ))? > 0,
                    "恢复安装需要原始内容封存",
                )?;
            }
            let key: RuntimeKey = serde_json::from_value(json!({"provider":change.value["provider"],"version":change.value["version"],"os":change.value["os"],"arch":change.value["arch"]})).map_err(storage)?;
            require(
                change.resource.id
                    == devdesk_core::runtime_registry::installation_id(
                        &key,
                        change.value["recipe_digest"].as_str(),
                    ),
                "安装记录与版本/平台身份不一致",
            )?;
        }
        if change.resource.kind == "source" {
            let identity = change.value["identity"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "缺少源身份"))?;
            let head = ResourceKey::new("source_head", identity);
            prepared.read_set.require(&head)?;
            if !changed.contains(&head) {
                return Err(err(
                    "precondition_required",
                    "源发布必须同时提交 head 的版本变化",
                ));
            }
        }
    }
    let mut specs = ColumnSpecs::new();
    {
        let conn = pool.read()?;
        for change in &prepared.changes {
            if !matches!(
                change.resource.kind.as_str(),
                "setting" | "source" | "source_head"
            ) {
                let table = table(&change.resource.kind)?;
                if !specs.contains_key(table) {
                    specs.insert(table, columns(&conn, table)?);
                }
            }
        }
    }
    prepared
        .changes
        .iter()
        .map(|change| prepare_write(change, &specs))
        .collect()
}

fn commit_once(
    conn: &mut Connection,
    prepared: &PreparedChangeSet,
    writes: &[Option<SqlWrite>],
) -> Result<(CommitReceipt, bool)> {
    if let Some(receipt) = replay_on(conn, &prepared.command_id, &prepared.fingerprint)? {
        return Ok((receipt, false));
    }
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let _span = performance::Span::new("resource.sql_transaction");
    if let Some(receipt) = replay_on(&transaction, &prepared.command_id, &prepared.fingerprint)? {
        return Ok((receipt, false));
    }
    for expected in prepared.read_set.versions() {
        let actual = current_revision(&transaction, &expected.resource)?;
        if expected.revision != actual {
            return Err(err(
                "revision_conflict",
                format!(
                    "{} {} 的版本已变化：预期 {}，当前 {}",
                    expected.resource.kind, expected.resource.id, expected.revision, actual
                ),
            ));
        }
    }
    let mut versions = Vec::with_capacity(prepared.changes.len());
    for (change, write) in prepared.changes.iter().zip(writes) {
        let expected = prepared.read_set.require(&change.resource)?;
        transaction.execute("INSERT OR IGNORE INTO resource_versions(kind,resource_id,revision) VALUES(?1,?2,?3)",rusqlite::params![change.resource.kind,change.resource.id,expected]).map_err(sqlite_error)?;
        let updated = transaction.execute("UPDATE resource_versions SET revision=revision+1 WHERE kind=?1 AND resource_id=?2 AND revision=?3",rusqlite::params![change.resource.kind,change.resource.id,expected]).map_err(sqlite_error)?;
        if updated != 1 {
            return Err(err("revision_conflict", "提交时资源版本已变化"));
        }
        if let Some(write) = write {
            transaction
                .prepare_cached(&write.sql)
                .map_err(sqlite_error)?
                .execute(rusqlite::params_from_iter(&write.values))
                .map_err(sqlite_error)?;
        }
        versions.push(ResourceVersion {
            resource: change.resource.clone(),
            revision: expected + 1,
        });
    }
    let encoded_versions = serde_json::to_string(&versions).map_err(storage)?;
    for change in &prepared.changes {
        if change.resource.kind == "source_head" {
            let actual = value(&transaction, &change.resource)?;
            require(
                actual
                    .as_ref()
                    .is_some_and(|actual| actual["reference"] == change.value["reference"]),
                "源 head 与实际修订不一致",
            )?;
        }
    }
    transaction
        .execute(
            "INSERT INTO events(target,kind,detail,time) VALUES(?1,'resource.changed',?2,?3)",
            rusqlite::params![
                prepared.changes[0].resource.id,
                encoded_versions,
                prepared.time
            ],
        )
        .map_err(sqlite_error)?;
    let cursor = transaction.last_insert_rowid() as u64;
    transaction
        .execute(
            "INSERT INTO operations(id,fingerprint,result,versions,cursor) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                prepared.command_id,
                prepared.fingerprint,
                prepared.result.to_string(),
                encoded_versions,
                cursor
            ],
        )
        .map_err(sqlite_error)?;
    transaction.commit().map_err(sqlite_error)?;
    Ok((
        CommitReceipt {
            result: prepared.result.clone(),
            versions,
            cursor,
        },
        true,
    ))
}

pub(super) fn commit(
    pool: &ConnectionPool,
    prepared: &PreparedChangeSet,
) -> Result<(CommitReceipt, bool)> {
    prepared.validate()?;
    require(prepared.changes.len() <= 4096, "一次提交的资源过多")?;
    let _span = performance::Span::new("resource.commit");
    let writes = prepare_writes(pool, prepared)?;
    let mut conn = pool.lock()?;
    let started = Instant::now();
    for attempt in 0..3 {
        match commit_once(&mut conn, prepared, &writes) {
            Err(error)
                if error.code == "storage_busy"
                    && attempt < 2
                    && started.elapsed() < Duration::from_millis(100) =>
            {
                performance::record("resource.busy_retry", 0.0);
                std::thread::sleep(Duration::from_millis(2 + attempt));
            }
            result => return result,
        }
    }
    unreachable!("bounded transaction retry")
}
