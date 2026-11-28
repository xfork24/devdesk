//! Explicit offline upgrades for the global database. Keep the dataset lock
//! across the backup and the transaction that replaces the installation table.
use super::*;
use rusqlite::{OpenFlags, TransactionBehavior};
use std::{
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const GLOBAL_SCHEMA_VERSION: u32 = 25;

const GLOBAL_MIGRATIONS: &[(u32, u32, &str)] = &[(
    24,
    25,
    include_str!("../../../sql/global/migrate-24-to-25.sql"),
)];

pub fn upgrade(data: &Path) -> Result<Value> {
    let (root, _lock) = lock_dataset(data)?;
    let database = root.join("global.sqlite3");
    if !database.exists() {
        return Ok(unchanged(0));
    }
    let mut connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(storage)?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage)?;
    if version == 0 || version == GLOBAL_SCHEMA_VERSION {
        return Ok(unchanged(version));
    }
    let Some(start) = GLOBAL_MIGRATIONS.iter().position(|step| step.0 == version) else {
        return Err(err("incompatible_dataset", "全局库版本不支持此迁移"));
    };
    catalog::validate_global_layout(&connection)?;
    let busy: i64 = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(storage)?;
    require(busy == 0, "全局库仍有读写连接，无法备份")?;
    let backup = backup_database(&root, &database, version)?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
        .map_err(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    let rows = installation_count(&transaction)?;
    let mut current = version;
    for &(from, to, sql) in &GLOBAL_MIGRATIONS[start..] {
        require(from == current, "全局库迁移步骤不连续")?;
        transaction.execute_batch(sql).map_err(storage)?;
        transaction
            .pragma_update(None, "user_version", to)
            .map_err(storage)?;
        current = to;
    }
    require(current == GLOBAL_SCHEMA_VERSION, "全局库未迁移到当前版本")?;
    require(
        installation_count(&transaction)? == rows,
        "迁移前后运行安装行数不一致",
    )?;
    let integrity: String = transaction
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .map_err(storage)?;
    require(integrity == "ok", "全局库完整性检查失败")?;
    transaction.commit().map_err(storage)?;
    Ok(json!({
        "source_version": version,
        "target_version": current,
        "changed": true,
        "runtime_installations": rows,
        "backup_path": backup,
    }))
}

fn unchanged(version: u32) -> Value {
    json!({"source_version": version, "target_version": version, "changed": false})
}

fn installation_count(connection: &Connection) -> Result<u64> {
    connection
        .query_row("SELECT count(*) FROM runtime_installations", [], |row| {
            row.get(0)
        })
        .map_err(storage)
}

fn backup_database(root: &Path, database: &Path, version: u32) -> Result<PathBuf> {
    let parent = root
        .parent()
        .ok_or_else(|| err("invalid_params", "数据集必须有可用于备份的父目录"))?;
    let directory = parent.join(".devdesk-storage-backups");
    reject_symlink(&directory)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&directory)
        .map_err(storage)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(storage)?
        .as_secs();
    let backup = directory.join(format!("global-v{version}-{timestamp}-{}.sqlite3", id()));
    let mut source = File::open(database).map_err(storage)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&backup)
        .map_err(storage)?;
    let copied = std::io::copy(&mut source, &mut output)
        .and_then(|_| output.sync_all())
        .and_then(|_| File::open(&directory)?.sync_all());
    if let Err(error) = copied {
        drop(output);
        let _ = fs::remove_file(&backup);
        return Err(storage(error));
    }
    Ok(backup)
}
