//! Host-owned cache storage. Cached files linked to an idle session are
//! materialized privately before Docker can write to them.
use super::*;
use std::io::{BufReader, Read};
use std::os::unix::fs::PermissionsExt;

#[path = "session_cache.rs"]
mod session_cache;
pub(crate) use session_cache::CodexCache;

pub(crate) fn checkpoint(connection: &Connection) -> Result<()> {
    let busy: i64 = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(storage)?;
    if busy != 0 {
        eprintln!("DevDesk SQLite checkpoint: database busy, will retry on next startup");
    }
    Ok(())
}

fn entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    reject_symlink(dir)?;
    match fs::read_dir(dir) {
        Ok(entries) => entries.collect::<std::io::Result<_>>().map_err(storage),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(storage(e)),
    }
}

fn cache_subdir(home: &Path, suffix: &str) -> Result<PathBuf> {
    reject_symlink(home)?;
    let mut path = home.to_owned();
    for part in suffix.split('/') {
        path.push(part);
        reject_symlink(&path)?;
    }
    Ok(path)
}

fn private_directory(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    fs::create_dir_all(path).map_err(storage)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(storage)
}

fn digest(path: &Path) -> Result<String> {
    let mut source = BufReader::new(File::open(path).map_err(storage)?);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let bytes = source.read(&mut buffer).map_err(storage)?;
        if bytes == 0 {
            return Ok(format!("{:x}", hash.finalize()));
        }
        hash.update(&buffer[..bytes]);
    }
}

fn replace(target: &Path, write: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<()> {
    let temp = target.with_file_name(format!(".devdesk-cache-{}.pending", id()));
    let result = write(&temp).and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(storage)
}

fn copy_private(source: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(source.as_os_str().as_bytes())?;
        let to = std::ffi::CString::new(dest.as_os_str().as_bytes())?;
        // Both paths belong to the locked dataset. clonefile creates a new
        // inode and falls back to a normal copy on other filesystems.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    fs::copy(source, dest).map(|_| ())
}

fn visit_files(
    dir: &Path,
    visit: &mut impl FnMut(&Path, &fs::Metadata) -> Result<()>,
) -> Result<()> {
    for entry in entries(dir)? {
        let path = entry.path();
        let kind = entry.file_type().map_err(storage)?;
        if kind.is_dir() {
            visit_files(&path, visit)?;
        } else if kind.is_file() {
            visit(&path, &entry.metadata().map_err(storage)?)?;
        }
    }
    Ok(())
}

pub(crate) fn shared_runtime(data: &Path) -> Result<PathBuf> {
    cache_subdir(data, "shared-cache/runtime")
}

fn move_dependencies(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in entries(source)? {
        let path = entry.path();
        let target = target.join(entry.file_name());
        let kind = entry.file_type().map_err(storage)?;
        match fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::rename(&path, &target).map_err(storage)?;
            }
            Ok(meta) if kind.is_dir() && meta.is_dir() => move_dependencies(&path, &target)?,
            Ok(meta)
                if kind.is_file()
                    && meta.is_file()
                    && meta.len() == entry.metadata().map_err(storage)?.len()
                    && digest(&path)? == digest(&target)? =>
            {
                fs::remove_file(path).map_err(storage)?
            }
            Ok(_) => eprintln!(
                "DevDesk shared cache: preserved conflicting cache at {}",
                path.display()
            ),
            Err(e) => return Err(storage(e)),
        }
    }
    if entries(source)?.is_empty() {
        fs::remove_dir(source).map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn remove_scratch(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(storage(e)),
    }
}

pub(crate) fn retire_test(job: &Path) -> Result<()> {
    reject_symlink(job)?;
    remove_scratch(&job.join("work"))?;
    remove_scratch(&job.join("state"))?;
    let reporter = job.join("reporter.mjs");
    reject_symlink(&reporter)?;
    match fs::remove_file(reporter) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(storage(e)),
    }
}

pub(crate) fn checkpoint_provider(home: &Path) -> Result<()> {
    for entry in entries(home)? {
        if let Some(name) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".sqlite-wal"))
        {
            if !entry.file_type().map_err(storage)?.is_file()
                || entry.metadata().map_err(storage)?.len() == 0
            {
                continue;
            }
            let db = home.join(format!("{name}.sqlite"));
            reject_symlink(&db)?;
            reject_symlink(&home.join(format!("{name}.sqlite-shm")))?;
            let connection =
                Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(storage)?;
            connection
                .busy_timeout(std::time::Duration::from_millis(100))
                .map_err(storage)?;
            checkpoint(&connection)?;
        }
    }
    Ok(())
}

pub(crate) fn retire_protocol(
    root: &Path,
    session: &str,
    operation: &str,
    relative_path: &str,
    expected: &str,
) -> Result<()> {
    require(
        uuid::Uuid::parse_str(session).is_ok() && uuid::Uuid::parse_str(operation).is_ok(),
        "无效 worker 日志身份",
    )?;
    let dir = cache_subdir(root, &format!("execution/sessions/{session}/coordinator"))?;
    let raw = dir.join(format!("{operation}.jsonl"));
    reject_symlink(&raw)?;
    if !raw.is_file() {
        return Ok(());
    }
    let sealed = FileArtifacts::new(root.to_owned()).checked_path(relative_path)?;
    require(
        sealed
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|part| part == "protocol"),
        "只清理具有协议产物的日志",
    )?;
    require(
        digest(&sealed)? == expected && digest(&raw)? == expected,
        "原始协议日志和已封存产物不一致",
    )?;
    fs::remove_file(raw).map_err(storage)
}

fn retire_sealed_protocols(root: &Path, connection: &Connection) -> Result<()> {
    let mut query = connection
        .prepare(
            "SELECT e.session_id,e.operation_id,a.relative_path,a.sha256
         FROM executions e JOIN artifacts a ON a.id=e.protocol_artifact
         WHERE e.kind='harness' AND e.writer_revoked=1 AND e.status='released'
           AND a.origin='external'",
        )
        .map_err(storage)?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(storage)?;
    for row in rows {
        let (session, operation, path, sha256) = row.map_err(storage)?;
        if let Err(error) = retire_protocol(root, &session, &operation, &path, &sha256) {
            eprintln!(
                "DevDesk protocol compaction kept original journal: {}",
                error.message
            );
        }
    }
    Ok(())
}

pub(crate) fn maintain(data: &Path, root: &Path, connection: &Connection) -> Result<()> {
    let execution = root.join("execution");
    let sessions = cache_subdir(&execution, "sessions")?;
    let legacy = cache_subdir(&execution, "runtime-cache")?;
    if !sessions.is_dir() && !legacy.is_dir() {
        return Ok(());
    }
    // A worker left behind by a previous Host may still own or later reopen
    // its mounts. An unavailable Docker daemon cannot prove the root is idle.
    if !crate::docker::DockerSandbox::new(&execution)?.dataset_containers_absent()? {
        return Ok(());
    }
    retire_sealed_protocols(root, connection)?;
    let target = shared_runtime(data)?;
    for entry in entries(&legacy)? {
        if std::env::var_os("DEVDESK_SHARED_DEPENDENCY_CACHE").is_none()
            && entry.file_type().map_err(storage)?.is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
        {
            private_directory(&target)?;
            move_dependencies(&entry.path(), &target)?;
        }
    }
    let tests = cache_subdir(&execution, "tests")?;
    let mut completed = connection
        .prepare(
            "SELECT json_extract(data,'$.request.operation_id') FROM governance_records
         WHERE type='platform_test_cleanup' AND json_extract(data,'$.status')='completed'",
        )
        .map_err(storage)?;
    for operation in completed
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
    {
        let operation = operation.map_err(storage)?;
        require(
            uuid::Uuid::parse_str(&operation).is_ok(),
            "无效测试清理操作 ID",
        )?;
        retire_test(&tests.join(operation))?;
    }
    let cache = CodexCache::new(data);
    let mut query = connection.prepare(
        "SELECT s.id, json_extract(s.runtime_binding,'$.credential_ref') FROM sessions s
         WHERE NOT EXISTS (SELECT 1 FROM executions e WHERE e.session_id=s.id AND NOT e.writer_revoked)",
    ).map_err(storage)?;
    let rows = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(storage)?;
    for row in rows {
        let (session, credential) = row.map_err(storage)?;
        require(uuid::Uuid::parse_str(&session).is_ok(), "无效会话目录 ID")?;
        let base = sessions.join(session);
        for entry in entries(&cache_subdir(&base, "coordinator")?)? {
            let name = entry.file_name();
            if entry.file_type().map_err(storage)?.is_dir()
                && name
                    .to_str()
                    .and_then(|name| name.strip_prefix("view-"))
                    .is_some_and(|operation| uuid::Uuid::parse_str(operation).is_ok())
            {
                remove_scratch(&entry.path())?;
            }
        }
        remove_scratch(&cache_subdir(&base, "state/runtime/tmp")?)?;
        let home = cache_subdir(&base, "state/codex")?;
        checkpoint_provider(&home)?;
        if let Some(account) = credential.filter(|s| s.starts_with("codex:")) {
            cache.retire(&home, &account)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path, body: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn protocol_compaction_keeps_unsealed_or_different_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let session = id();
        let operation = id();
        let raw = dir.path().join(format!(
            "execution/sessions/{session}/coordinator/{operation}.jsonl"
        ));
        let relative = "artifacts/global/feature/protocol/id.md";
        let sealed = dir.path().join(relative);
        file(&raw, b"worker journal");
        let hash = format!("{:x}", Sha256::digest(b"worker journal"));

        assert!(retire_protocol(dir.path(), &session, &operation, relative, &hash).is_err());
        assert!(raw.is_file());
        file(&sealed, b"wrong file");
        assert!(retire_protocol(dir.path(), &session, &operation, relative, &hash).is_err());
        assert!(raw.is_file());

        file(&sealed, b"worker journal");
        retire_protocol(dir.path(), &session, &operation, relative, &hash).unwrap();
        retire_protocol(dir.path(), &session, &operation, relative, &hash).unwrap();
        assert!(!raw.exists());
        assert_eq!(fs::read(sealed).unwrap(), b"worker journal");
    }

    #[test]
    fn cache_migration_is_repeatable_and_retains_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old");
        let to = dir.path().join("global");
        file(&from.join("downloads/new"), b"unique");
        file(&from.join("downloads/duplicate"), b"same");
        file(&from.join("downloads/conflict"), b"old value");
        file(&to.join("downloads/duplicate"), b"same");
        file(&to.join("downloads/conflict"), b"other value");
        move_dependencies(&from, &to).unwrap();
        move_dependencies(&from, &to).unwrap();
        assert!(!from.join("downloads/duplicate").exists());
        assert!(!from.join("downloads/new").exists());
        assert_eq!(
            fs::read(from.join("downloads/conflict")).unwrap(),
            b"old value"
        );
        assert_eq!(fs::read(to.join("downloads/new")).unwrap(), b"unique");
    }

    #[test]
    fn wal_checkpoint_preserves_committed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("state.sqlite")).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL; CREATE TABLE t (name TEXT); INSERT INTO t VALUES ('kept');",
        )
        .unwrap();
        let wal = dir.path().join("state.sqlite-wal");
        assert!(wal.metadata().unwrap().len() > 0);
        checkpoint(&db).unwrap();
        assert_eq!(wal.metadata().unwrap().len(), 0);
        let row: String = db
            .query_row("SELECT name FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row, "kept");
    }

    #[test]
    fn test_cleanup_preserves_operation_and_result() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "job.json",
            "result.json",
            "reporter.mjs",
            "work/source",
            "state/runtime/tmp/build",
        ] {
            file(&dir.path().join(name), b"data");
        }
        retire_test(dir.path()).unwrap();
        retire_test(dir.path()).unwrap();
        assert!(dir.path().join("job.json").is_file());
        assert!(dir.path().join("result.json").is_file());
        assert!(!dir.path().join("reporter.mjs").exists());
        assert!(!dir.path().join("work").exists());
        assert!(!dir.path().join("state").exists());
    }
}
