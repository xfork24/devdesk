mod catalog;
pub mod simulation;
pub use catalog::GlobalStore;
use devdesk_core::*;
use devdesk_domain::*;
use rusqlite::{
    Connection,
    types::{Value as SqlValue, ValueRef},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub use simulation::{SimGit, SimHarness, SimSandbox, SimTests};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Mutex,
};
fn storage(e: impl std::fmt::Display) -> Error {
    err("storage", e.to_string())
}
pub struct SqliteStore {
    connection: Mutex<Connection>,
    writer: Mutex<()>,
    root: PathBuf,
    _lock: File,
    project_id: Option<String>,
}
impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        let (root, lock) = lock_dataset(path)?;
        require(
            !root.join("global.sqlite3").exists(),
            "此目录是全局配置库；请打开其项目数据库",
        )?;
        let connection = open_engineering_connection(&root.join("engineering.sqlite3"))?;
        fs::create_dir_all(root.join("artifacts")).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
            writer: Mutex::new(()),
            root,
            _lock: lock,
            project_id: None,
        })
    }
    pub fn open_project(data_root: &Path, project_id: &str) -> Result<Self> {
        let root = Self::project_path(data_root, project_id)?;
        let parent = data_root.join("projects");
        fs::create_dir_all(&parent).map_err(storage)?;
        let mut store = Self::open(&root)?;
        store.project_id = Some(project_id.into());
        let state = store.snapshot()?;
        require(
            state.projects.iter().all(|p| p.id == project_id) && state.projects.len() <= 1,
            "项目数据库身份不匹配",
        )?;
        Ok(store)
    }
    pub fn project_path(data_root: &Path, project_id: &str) -> Result<PathBuf> {
        require(uuid::Uuid::parse_str(project_id).is_ok(), "无效项目 ID")?;
        let parent = data_root.join("projects");
        let root = parent.join(project_id);
        reject_symlink(&parent)?;
        reject_symlink(&root)?;
        for name in [
            "engineering.sqlite3",
            "engineering.sqlite3-wal",
            "engineering.sqlite3-shm",
            ".devdesk-dataset",
            ".devdesk.lock",
            "artifacts",
        ] {
            reject_symlink(&root.join(name))?;
        }
        Ok(root)
    }
    fn load(conn: &Connection) -> Result<EngineeringState> {
        let mut s = EngineeringState::default();
        s.advances = load_table::<MergeAdvance>(conn, "advances")?;
        s.effects = load_table::<EffectRecord>(conn, "effects")?;
        s.harnesses = load_table::<Harness>(conn, "harnesses")?;
        s.sandboxes = load_table::<Sandbox>(conn, "sandboxes")?;
        s.projects = load_table::<Project>(conn, "projects")?;
        s.features = load_table::<Feature>(conn, "features")?;
        s.tasks = load_table::<Task>(conn, "tasks")?;
        s.documents = load_table::<Document>(conn, "documents")?;
        s.definitions = load_table::<Definition>(conn, "definitions")?;
        s.attempts = load_table::<Attempt>(conn, "attempts")?;
        s.runs = load_table::<Run>(conn, "runs")?;
        s.sessions = load_table::<Session>(conn, "sessions")?;
        s.contexts = load_table::<Context>(conn, "contexts")?;
        s.snapshots = load_table::<Snapshot>(conn, "snapshots")?;
        s.deliveries = load_table::<Delivery>(conn, "deliveries")?;
        s.reviews = load_table::<Review>(conn, "reviews")?;
        s.overrides = load_table::<Override>(conn, "overrides")?;
        s.merges = load_table::<Merge>(conn, "merges")?;
        s.heads = load_table::<GitHead>(conn, "heads")?;
        s.resources = load_table::<Resource>(conn, "resources")?;
        s.artifacts = load_table::<Artifact>(conn, "artifacts")?;
        s.events = load_table::<Event>(conn, "events")?;
        Ok(s)
    }
    fn save(conn: &Connection, s: &EngineeringState, before: &EngineeringState) -> Result<()> {
        save_table(conn, "advances", &s.advances, &before.advances)?;
        save_table(conn, "effects", &s.effects, &before.effects)?;
        save_table(conn, "harnesses", &s.harnesses, &before.harnesses)?;
        save_table(conn, "sandboxes", &s.sandboxes, &before.sandboxes)?;
        save_table(conn, "projects", &s.projects, &before.projects)?;
        save_table(conn, "features", &s.features, &before.features)?;
        save_table(conn, "tasks", &s.tasks, &before.tasks)?;
        save_table(conn, "documents", &s.documents, &before.documents)?;
        save_table(conn, "definitions", &s.definitions, &before.definitions)?;
        save_table(conn, "attempts", &s.attempts, &before.attempts)?;
        save_table(conn, "runs", &s.runs, &before.runs)?;
        save_table(conn, "sessions", &s.sessions, &before.sessions)?;
        save_table(conn, "contexts", &s.contexts, &before.contexts)?;
        save_table(conn, "snapshots", &s.snapshots, &before.snapshots)?;
        save_table(conn, "deliveries", &s.deliveries, &before.deliveries)?;
        save_table(conn, "reviews", &s.reviews, &before.reviews)?;
        save_table(conn, "overrides", &s.overrides, &before.overrides)?;
        save_table(conn, "merges", &s.merges, &before.merges)?;
        save_table(conn, "heads", &s.heads, &before.heads)?;
        save_table(conn, "resources", &s.resources, &before.resources)?;
        save_table(conn, "artifacts", &s.artifacts, &before.artifacts)?;
        save_table(conn, "events", &s.events, &before.events)?;
        Ok(())
    }
}
fn lock_dataset(path: &Path) -> Result<(PathBuf, File)> {
    fs::create_dir_all(path).map_err(storage)?;
    let root = path.canonicalize().map_err(storage)?;
    for name in [
        ".devdesk-dataset",
        ".devdesk.lock",
        "engineering.sqlite3",
        "engineering.sqlite3-wal",
        "engineering.sqlite3-shm",
        "artifacts",
        "global.sqlite3",
        "global.sqlite3-wal",
        "global.sqlite3-shm",
        "projects",
        "runtime.json",
    ] {
        reject_symlink(&root.join(name))?;
    }
    let marker = root.join(".devdesk-dataset");
    if !marker.exists() {
        let entries = fs::read_dir(&root)
            .map_err(storage)?
            .filter_map(|x| x.ok())
            .collect::<Vec<_>>();
        require(
            entries.is_empty(),
            "目录非空且不是 DevDesk 数据集；请选择空目录",
        )?;
        fs::write(&marker, "devdesk-schema-v1\n").map_err(storage)?;
    }
    require(
        fs::read_to_string(&marker).map_err(storage)? == "devdesk-schema-v1\n",
        "数据集标记不受支持",
    )?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".devdesk.lock"))
        .map_err(storage)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| err("dataset_locked", "此 data 已由另一个 host 打开"))?;
    Ok((root, lock))
}

fn open_engineering_connection(path: &Path) -> Result<Connection> {
    let mut connection = Connection::open(path).map_err(storage)?;
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(storage)?;
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    require(version <= 13, "数据集版本比程序新")?;
    if version == 0 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/001_initial.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 2 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!(
            "../../../migrations/002_execution_binding.sql"
        ))
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 3 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/003_traceability.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 4 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/004_instances.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 5 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/005_code_baseline.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 6 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!(
            "../../../migrations/006_binding_provenance.sql"
        ))
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 7 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/007_effects.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 8 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/008_attempt_outputs.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 9 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/009_attempt_scope.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 10 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/010_merge_advances.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 11 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/011_advance_provider.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 12 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!(
            "../../../migrations/012_advance_observation.sql"
        ))
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    if version < 13 {
        let tx = connection.transaction().map_err(storage)?;
        tx.execute_batch(include_str!("../../../migrations/013_git_context.sql"))
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    Ok(connection)
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<(String, String)>> {
    let mut q = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(storage)?;
    q.query_map([], |r| Ok((r.get(1)?, r.get(2)?)))
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)
}
fn load_table<T: DeserializeOwned>(conn: &Connection, table: &str) -> Result<Vec<T>> {
    let columns = columns(conn, table)?;
    let mut q = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .map_err(storage)?;
    let rows = q
        .query_map([], |r| {
            let mut map = serde_json::Map::new();
            for (i, (name, typ)) in columns.iter().enumerate() {
                let v = match r.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => {
                        if typ == "BOOLEAN" {
                            json!(n != 0)
                        } else {
                            json!(n)
                        }
                    }
                    ValueRef::Real(n) => json!(n),
                    ValueRef::Text(bytes) => {
                        let text = String::from_utf8_lossy(bytes);
                        if typ == "JSON" {
                            serde_json::from_str(&text).map_err(|e| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    i,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                )
                            })?
                        } else {
                            json!(text)
                        }
                    }
                    ValueRef::Blob(_) => Value::Null,
                };
                map.insert(name.clone(), v);
            }
            Ok(Value::Object(map))
        })
        .map_err(storage)?;
    rows.map(|v| serde_json::from_value(v.map_err(storage)?).map_err(storage))
        .collect()
}
fn save_table<T: Serialize>(
    conn: &Connection,
    table: &str,
    items: &[T],
    before: &[T],
) -> Result<()> {
    let previous: std::collections::HashMap<String, Value> = before
        .iter()
        .map(|item| {
            let v = serde_json::to_value(item).map_err(storage)?;
            Ok((v["id"].to_string(), v))
        })
        .collect::<Result<_>>()?;
    if items.is_empty() {
        return Ok(());
    }
    let cols = columns(conn, table)?;
    let names = cols
        .iter()
        .map(|(n, _)| format!("\"{n}\""))
        .collect::<Vec<_>>();
    let placeholders = (1..=cols.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>();
    let updates = names
        .iter()
        .filter(|n| n.as_str() != "\"id\"")
        .map(|n| format!("{n}=excluded.{n}"))
        .collect::<Vec<_>>();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        names.join(","),
        placeholders.join(","),
        updates.join(",")
    );
    let mut statement = conn.prepare_cached(&sql).map_err(storage)?;
    for item in items {
        let v = serde_json::to_value(item).map_err(storage)?;
        if previous.get(&v["id"].to_string()) == Some(&v) {
            continue;
        }
        let values = cols
            .iter()
            .map(|(n, typ)| {
                let v = &v[n];
                if v.is_null() {
                    SqlValue::Null
                } else if typ == "JSON" {
                    SqlValue::Text(v.to_string())
                } else {
                    match v {
                        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
                        Value::Number(n) => SqlValue::Integer(n.as_i64().unwrap_or_default()),
                        Value::String(s) => SqlValue::Text(s.clone()),
                        _ => SqlValue::Text(v.to_string()),
                    }
                }
            })
            .collect::<Vec<_>>();
        statement
            .execute(rusqlite::params_from_iter(values))
            .map_err(storage)?;
    }
    Ok(())
}
impl EngineeringStore for SqliteStore {
    fn project_id(&self) -> Option<String> {
        self.project_id.clone()
    }
    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    fn snapshot(&self) -> Result<EngineeringState> {
        Self::load(&*self.connection.lock().map_err(storage)?)
    }
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        // Serialize writers, but release the DB connection and transaction during
        // adapter calls. Read-only queries remain available while an adapter polls.
        let _writer = self.writer.lock().map_err(storage)?;
        let conn = self.connection.lock().map_err(storage)?;
        if let Some((id, fp)) = key {
            let mut q = conn
                .prepare("SELECT fingerprint,result FROM operations WHERE id=?1")
                .map_err(storage)?;
            let mut rows = q.query([id]).map_err(storage)?;
            if let Some(row) = rows.next().map_err(storage)? {
                let saved: String = row.get(0).map_err(storage)?;
                require(saved == fp, "command_id 被不同参数复用")?;
                let result: String = row.get(1).map_err(storage)?;
                return serde_json::from_str(&result).map_err(storage);
            }
        }
        let mut state = Self::load(&conn)?;
        let before = state.clone();
        drop(conn);
        let result = action(&mut state)?;
        if let Some(id) = &self.project_id {
            require(
                state.projects.len() <= 1 && state.projects.iter().all(|p| &p.id == id),
                "项目数据库不能写入其他项目",
            )?;
        }
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        if serde_json::to_vec(&before).map_err(storage)?
            != serde_json::to_vec(&state).map_err(storage)?
        {
            Self::save(&tx, &state, &before)?;
        }
        if let Some((id, fp)) = key {
            tx.execute(
                "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
                rusqlite::params![id, fp, result.to_string()],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
fn reject_symlink(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        require(
            !metadata.file_type().is_symlink(),
            "数据集关键路径不能为符号链接",
        )?;
    }
    Ok(())
}
fn artifact_directory(root: &Path, parts: &[&str]) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(storage)?;
    let mut current = root.clone();
    for part in std::iter::once(&"artifacts").chain(parts.iter()) {
        current.push(part);
        reject_symlink(&current)?;
        fs::create_dir_all(&current).map_err(storage)?;
        require(
            current.canonicalize().map_err(storage)?.starts_with(&root),
            "Artifact 目录越界",
        )?;
    }
    Ok(current)
}
pub struct FileArtifacts {
    root: PathBuf,
}
impl FileArtifacts {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
impl ArtifactStore for FileArtifacts {
    fn seal(
        &self,
        feature: &str,
        task: Option<&str>,
        category: &str,
        body: &str,
    ) -> Result<Artifact> {
        for part in std::iter::once(feature)
            .chain(task)
            .chain(std::iter::once(category))
        {
            require(
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "非法 Artifact 归属路径",
            )?;
        }
        let aid = id();
        let extension = if ["manifests", "snapshots"].contains(&category) {
            "json"
        } else {
            "md"
        };
        let path = format!(
            "artifacts/{feature}/{}/{category}/{aid}.{extension}",
            task.unwrap_or("feature")
        );
        let dest = self.root.join(&path);
        artifact_directory(&self.root, &[feature, task.unwrap_or("feature"), category])?;
        let temporary = dest.with_extension("pending");
        {
            use std::io::Write;
            let mut f = File::create(&temporary).map_err(storage)?;
            f.write_all(body.as_bytes()).map_err(storage)?;
            f.sync_all().map_err(storage)?;
        }
        fs::rename(temporary, &dest).map_err(storage)?;
        let sha256 = format!("{:x}", Sha256::digest(body.as_bytes()));
        Ok(Artifact {
            id: aid,
            feature_id: feature.into(),
            task_id: task.map(str::to_owned),
            relative_path: path,
            sha256,
            media_type: if extension == "json" {
                "application/json"
            } else {
                "text/markdown; charset=utf-8"
            }
            .into(),
            origin: Origin::Simulated,
        })
    }
    fn read(&self, artifact: &Artifact) -> Result<Vec<u8>> {
        let root = self.root.canonicalize().map_err(storage)?;
        reject_symlink(&root.join("artifacts"))?;
        let base = root.join("artifacts");
        require(
            base.canonicalize().map_err(storage)? == base,
            "Artifact 根目录越界",
        )?;
        let relative = Path::new(&artifact.relative_path);
        require(
            relative
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
            "Artifact 相对路径越界",
        )?;
        let mut current = root.clone();
        for part in relative.components() {
            current.push(part);
            reject_symlink(&current)?;
        }
        let path = current.canonicalize().map_err(storage)?;
        require(path.starts_with(&base), "Artifact 越界")?;
        let bytes = fs::read(path).map_err(storage)?;
        require(
            format!("{:x}", Sha256::digest(&bytes)) == artifact.sha256,
            "Artifact 哈希不匹配",
        )?;
        Ok(bytes)
    }
}
