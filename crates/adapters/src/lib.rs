mod catalog;
mod configuration_backend;
mod configuration_cache;
mod configuration_reset;
mod connection_pool;
mod execution_read;
mod experience_directory;
mod named_artifacts;
mod operational_reads;
pub mod performance;
mod read_models;
mod repository_contexts;
mod resource_commits;
mod resource_read_scope;
mod runtime_queries;
mod scheduling_read;
mod standard_reads;
mod storage_maintenance;
mod store_changes;
mod tool_calls;
mod workflow_details;
mod workflow_events;
mod workflow_intents;
mod workflow_observations;
mod workflow_projection;
mod workflow_readers;
mod workflow_stop_events;
pub use configuration_backend::ConfigurationBackend;
mod context_calls;
mod conversation_captures;
mod conversation_queries;
mod conversation_reads;
mod conversation_sources;
mod factory_bundle;
mod payload_store;
pub mod project_migration;
mod project_records;
mod project_storage;
pub mod session_resources;
mod source_store;
mod source_views;
mod workspace_read;
pub use source_views::runtime_digest;
pub mod codex_auth;
pub mod credentials;
#[cfg(target_os = "macos")]
mod credentials_macos;
pub mod docker;
pub mod experience;
mod experience_index;
pub mod git;
pub mod github;
mod github_delivery;
pub mod integration_oauth;
pub mod integrations;
pub mod permission_profiles;
pub mod permissions;
pub mod tests;
pub use tests::CommandTests;
pub mod harness;
pub mod model_providers;
pub use git::CliGit;
mod jsonl;
mod native_command_output;
pub use harness::{CodexHarness, DshHarness, RoutingHarness, RuntimeWorkers};
mod process;
mod runtime_images;
pub mod runtime_preparation;
mod runtime_registry;
pub mod runtime_verification;
pub use catalog::GlobalStore;
use devdesk_core::*;
use devdesk_domain::*;
pub use runtime_registry::configuration_digest;
const PROJECT_SCHEMA_VERSION: u32 = 61;
const LEGACY_PROJECT_SCHEMA_VERSION: u32 = 60;
use rusqlite::{
    Connection,
    types::{Value as SqlValue, ValueRef},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Mutex,
};
#[track_caller]
fn storage(e: impl std::fmt::Display + std::fmt::Debug) -> Error {
    err("storage", e.to_string()).with_context("adapter storage", json!({"cause":format!("{e:?}")}))
}
pub struct SqliteStore {
    connection: connection_pool::ConnectionPool,
    source_views: Mutex<std::collections::BTreeMap<String, Value>>,
    catalog_cache: configuration_cache::CatalogCache,
    changes: store_changes::StoreChanges,
    root: PathBuf,
    _lock: File,
    project_id: Option<String>,
}
impl Drop for SqliteStore {
    fn drop(&mut self) {
        // Explicit release also fences transient descriptors inherited by a concurrently
        // spawned child before exec closes them; reopening must not wait on that child.
        let _ = fs2::FileExt::unlock(&self._lock);
    }
}
impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        factory_bundle::initialize()?;
        let (root, lock) = lock_dataset(path)?;
        require(
            !root.join("global.sqlite3").exists(),
            "此目录是全局配置库；请打开其项目数据库",
        )?;
        let mut connection = open_engineering_connection(&root.join("engineering.sqlite3"))?;
        fs::create_dir_all(root.join("artifacts")).map_err(storage)?;
        if !payload_store::enabled(&connection)? {
            project_storage::maintain(&mut connection, &root)?;
        }
        Ok(Self {
            connection: connection_pool::ConnectionPool::new(
                &root.join("engineering.sqlite3"),
                connection,
            )?,
            source_views: Mutex::new(Default::default()),
            catalog_cache: Default::default(),
            changes: Default::default(),
            root,
            _lock: lock,
            project_id: None,
        })
    }
    /// Flat, isolated project root for offline legacy fixtures. Production must
    /// use the explicit per-project path and reserved project identity.
    #[cfg(feature = "test-fixtures")]
    pub fn open_fixture(path: &Path) -> Result<Self> {
        let mut store = Self::open(path)?;
        let projects = load_table::<Project>(&*store.connection.read()?, "projects")?;
        require(projects.len() <= 1, "离线测试根只能包含一个项目")?;
        store.project_id = Some(
            projects
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_else(devdesk_core::id),
        );
        Ok(store)
    }
    pub fn maintain_runtime_storage(&self, data_root: &Path) {
        let result = self
            .connection
            .read()
            .and_then(|conn| storage_maintenance::maintain(data_root, &self.root, &conn));
        if let Err(error) = result {
            eprintln!("DevDesk runtime storage maintenance: {}", error.message);
        }
    }
    pub fn open_project(data_root: &Path, project_id: &str) -> Result<Self> {
        let root = Self::project_path(data_root, project_id)?;
        let parent = data_root.join("projects");
        fs::create_dir_all(&parent).map_err(storage)?;
        let mut store = Self::open(&root)?;
        store.project_id = Some(project_id.into());
        let projects = load_table::<Project>(&*store.connection.read()?, "projects")?;
        require(
            projects.len() <= 1 && projects.iter().all(|p| p.id == project_id),
            "项目数据库身份不匹配",
        )?;
        Ok(store)
    }
    pub fn read_project_snapshot(data_root: &Path, project_id: &str) -> Result<EngineeringState> {
        let root = Self::project_path(data_root, project_id)?;
        let conn =
            connection_pool::ManagedConnection::read_only(&root.join("engineering.sqlite3"))?;
        conn.execute_batch("BEGIN;").map_err(storage)?;
        let state = Self::load(&conn)?;
        require(
            state.projects.iter().all(|p| p.id == project_id),
            "历史项目身份不符",
        )?;
        Ok(state)
    }
    pub fn read_project_catalog(
        data_root: &Path,
        project_id: &str,
    ) -> Result<std::sync::Arc<ConfigurationCatalogSnapshot>> {
        let root = Self::project_path(data_root, project_id)?;
        let catalog = configuration_cache::read_only(&root.join("engineering.sqlite3"), true)?;
        require(
            catalog.state.projects.iter().all(|p| p.id == project_id),
            "历史项目身份不符",
        )?;
        Ok(catalog)
    }
    pub fn read_global_discussion_catalog(
        data_root: &Path,
    ) -> Result<std::sync::Arc<ConfigurationCatalogSnapshot>> {
        configuration_cache::read_only(
            &Self::global_discussion_path(data_root)?.join("engineering.sqlite3"),
            true,
        )
    }
    pub fn global_discussion_path(data_root: &Path) -> Result<PathBuf> {
        let parent = data_root.join("conversations");
        let root = parent.join("global");
        reject_symlink(&parent)?;
        reject_symlink(&root)?;
        Ok(root)
    }
    pub fn read_global_discussion_snapshot(data_root: &Path) -> Result<EngineeringState> {
        let path = Self::global_discussion_path(data_root)?.join("engineering.sqlite3");
        if !path.is_file() {
            return Ok(EngineeringState::default());
        }
        let conn = connection_pool::ManagedConnection::read_only(&path)?;
        conn.execute_batch("BEGIN;").map_err(storage)?;
        Self::load(&conn)
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
        Self::load_with(conn, &mut Default::default())
    }
    fn load_cached(&self, conn: &Connection) -> Result<EngineeringState> {
        let mut memo = self.source_views.lock().map_err(storage)?.clone();
        // A plan descriptor contains only immutable source revisions and fixed choices.
        // Keep a bounded request-independent memory cache; it never crosses the storage boundary.
        if memo.len() >= 128 {
            memo.clear();
        }
        let state = Self::load_with(conn, &mut memo)?;
        if memo.len() <= 128 {
            *self.source_views.lock().map_err(storage)? = memo;
        }
        Ok(state)
    }
    // Table loads interleave with view refreshes: later tables restore against
    // views rebuilt from earlier fields, so a struct literal is not equivalent.
    #[allow(clippy::field_reassign_with_default)]
    fn load_with(
        conn: &Connection,
        memo: &mut std::collections::BTreeMap<String, Value>,
    ) -> Result<EngineeringState> {
        let _span = performance::Span::new("project.full_snapshot");
        source_views::begin_snapshot(memo);
        let mut s = EngineeringState::default();
        s.governance = source_store::load_records(conn)?;
        s.orchestration_wakeups = load_table(conn, "orchestration_wakeups")?;
        s.orchestration_leases = load_table(conn, "orchestration_leases")?;
        let turns: Vec<Value> = load_table(conn, "turns")?;
        s.conversation_events = load_table(conn, "conversation_events")?;
        s.interactions = load_table(conn, "interactions")?;
        s.proposals = load_table(conn, "proposals")?;
        let executions: Vec<Value> = load_table(conn, "executions")?;
        s.repository_operations = load_table(conn, "repository_operations")?;
        s.repository_worktrees = load_table(conn, "repository_worktrees")?;
        s.projects = load_table::<Project>(conn, "projects")?;
        s.features = load_table::<Feature>(conn, "features")?;
        s.tasks = load_table::<Task>(conn, "tasks")?;
        s.documents = load_table::<Document>(conn, "documents")?;
        s.refresh_configuration_views()?;
        s.turns = turns
            .into_iter()
            .map(|v| {
                source_views::restore_with(&s, &v, memo)
                    .and_then(|v| serde_json::from_value(v).map_err(storage))
            })
            .collect::<Result<_>>()?;
        s.executions = executions
            .into_iter()
            .map(|v| {
                source_views::restore_with(&s, &v, memo)
                    .and_then(|v| serde_json::from_value(v).map_err(storage))
            })
            .collect::<Result<_>>()?;
        let sessions: Vec<Value> = load_table(conn, "sessions")?;
        s.sessions = sessions
            .into_iter()
            .map(|v| {
                source_views::restore_with(&s, &v, memo)
                    .and_then(|v| serde_json::from_value(v).map_err(storage))
            })
            .collect::<Result<_>>()?;
        s.contexts = load_table::<Context>(conn, "contexts")?;
        let snapshots: Vec<Value> = load_table(conn, "snapshots")?;
        s.snapshots = snapshots
            .into_iter()
            .map(|v| {
                source_views::restore_with(&s, &v, memo)
                    .and_then(|v| serde_json::from_value(v).map_err(storage))
            })
            .collect::<Result<_>>()?;
        s.heads = load_table::<GitHead>(conn, "heads")?;
        s.artifacts = load_table::<Artifact>(conn, "artifacts")?;
        s.events = load_table::<Event>(conn, "events")?;
        s.refresh_configuration_views()?;
        source_store::restore_records(&mut s, memo)?;
        Ok(s)
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    fn save(conn: &Connection, s: &EngineeringState, before: &EngineeringState) -> Result<()> {
        // Even offline fixture seeding must not overwrite a concurrent native
        // resource commit made after its original snapshot.
        guard_fixture_revisions(conn, "turn", &s.turns, &before.turns)?;
        guard_fixture_revisions(conn, "session", &s.sessions, &before.sessions)?;
        guard_fixture_revisions(conn, "interaction", &s.interactions, &before.interactions)?;
        guard_fixture_revisions(conn, "project", &s.projects, &before.projects)?;
        guard_fixture_revisions(conn, "feature", &s.features, &before.features)?;
        guard_fixture_revisions(conn, "task", &s.tasks, &before.tasks)?;
        guard_fixture_revisions(conn, "context", &s.contexts, &before.contexts)?;
        guard_fixture_revisions(conn, "proposal", &s.proposals, &before.proposals)?;
        guard_fixture_revisions(
            conn,
            "repository_operation",
            &s.repository_operations,
            &before.repository_operations,
        )?;
        save_table(
            conn,
            "orchestration_wakeups",
            &s.orchestration_wakeups,
            &before.orchestration_wakeups,
        )?;
        save_table(
            conn,
            "orchestration_leases",
            &s.orchestration_leases,
            &before.orchestration_leases,
        )?;
        source_store::save_records(conn, s, before)?;
        let turns = s
            .turns
            .iter()
            .filter(|v| {
                before
                    .turns
                    .iter()
                    .find(|old| old.id == v.id)
                    .is_none_or(|old| json!(old) != json!(v))
            })
            .map(|v| source_views::pack(s, &json!(v)))
            .collect::<Result<Vec<_>>>()?;
        let old = Vec::<Value>::new();
        save_table(conn, "turns", &turns, &old)?;
        save_table(
            conn,
            "conversation_events",
            &s.conversation_events,
            &before.conversation_events,
        )?;
        save_table(conn, "interactions", &s.interactions, &before.interactions)?;
        save_table(conn, "proposals", &s.proposals, &before.proposals)?;
        let executions = s
            .executions
            .iter()
            .filter(|v| {
                before
                    .executions
                    .iter()
                    .find(|old| old.id == v.id)
                    .is_none_or(|old| json!(old) != json!(v))
            })
            .map(|v| source_views::pack(s, &json!(v)))
            .collect::<Result<Vec<_>>>()?;
        save_table(conn, "executions", &executions, &[])?;
        save_table(
            conn,
            "repository_operations",
            &s.repository_operations,
            &before.repository_operations,
        )?;
        save_table(
            conn,
            "repository_worktrees",
            &s.repository_worktrees,
            &before.repository_worktrees,
        )?;
        save_table(conn, "projects", &s.projects, &before.projects)?;
        save_table(conn, "features", &s.features, &before.features)?;
        save_table(conn, "tasks", &s.tasks, &before.tasks)?;
        save_table(conn, "documents", &s.documents, &before.documents)?;
        let sessions = s
            .sessions
            .iter()
            .filter(|v| {
                before
                    .sessions
                    .iter()
                    .find(|old| old.id == v.id)
                    .is_none_or(|old| json!(old) != json!(v))
            })
            .map(|v| source_views::pack(s, &json!(v)))
            .collect::<Result<Vec<_>>>()?;
        let old = Vec::<Value>::new();
        save_table(conn, "sessions", &sessions, &old)?;
        save_table(conn, "contexts", &s.contexts, &before.contexts)?;
        let snapshots = s
            .snapshots
            .iter()
            .filter(|v| before.snapshots.iter().all(|old| old.id != v.id))
            .map(|v| source_views::pack(s, &json!(v)))
            .collect::<Result<Vec<_>>>()?;
        save_table(conn, "snapshots", &snapshots, &[])?;
        save_table(conn, "heads", &s.heads, &before.heads)?;
        save_table(conn, "artifacts", &s.artifacts, &before.artifacts)?;
        append_events(conn, &s.events, &before.events)?;
        for row in s
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance")
        {
            workflow_projection::refresh_root(conn, &row.id)?;
        }
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
    // Clearing a dataset in Finder can leave its hidden marker, lock and folder metadata.
    // These files contain no dataset records; every other entry makes the directory nonempty.
    let metadata_only = || -> Result<bool> {
        for entry in fs::read_dir(&root).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if !matches!(
                entry.file_name().to_str(),
                Some(".devdesk-dataset" | ".devdesk.lock" | ".DS_Store")
            ) || !entry.file_type().map_err(storage)?.is_file()
            {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let validate_marker = || -> Result<()> {
        require(
            marker.exists(),
            "目录非空且不是 DevDesk 数据集；请选择空目录",
        )?;
        require(
            fs::read_to_string(&marker).map_err(storage)? == "devdesk-profile-runtime-v2\n",
            "数据集标记不受支持",
        )
    };
    // Reject foreign or incompatible data before creating any metadata in that directory.
    if !metadata_only()? {
        validate_marker()?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".devdesk.lock"))
        .map_err(storage)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| err("dataset_locked", "此 data 已由另一个 host 打开"))?;
    // Recheck after obtaining ownership. Never replace a lock file that another Host can hold.
    if metadata_only()? {
        fs::write(&marker, "devdesk-profile-runtime-v2\n").map_err(storage)?;
    } else {
        validate_marker()?;
    }
    Ok((root, lock))
}

fn open_engineering_connection(path: &Path) -> Result<connection_pool::ManagedConnection> {
    let connection = connection_pool::ManagedConnection::open(path)?;
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    require(
        matches!(
            version,
            0 | LEGACY_PROJECT_SCHEMA_VERSION | PROJECT_SCHEMA_VERSION
        ),
        "数据集协议不兼容；请使用新的空数据目录",
    )?;
    if version != 0 {
        check_dataset_layout(&connection)?;
        storage_maintenance::checkpoint(&connection)?;
    } else {
        connection
            .pragma_update(None, "page_size", 8192)
            .map_err(storage)?;
    }
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(storage)?;
    if version == 0 {
        connection
            .execute_batch(include_str!("../../../sql/project/schema.sql"))
            .map_err(storage)?;
    }

    Ok(connection)
}

fn check_dataset_layout(conn: &Connection) -> Result<()> {
    let layout = conn.query_row("SELECT contract FROM dataset_layout WHERE id=1", [], |r| {
        r.get::<_, String>(0)
    });
    if !layout.is_ok_and(|v| v == "single-host-resource-v2") {
        return Err(err(
            "incompatible_dataset",
            "数据集资源协议不兼容；保留原数据并使用新的空数据目录",
        ));
    }
    Ok(())
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
    load_table_filtered(conn, table, "1")
}
fn load_table_filtered<T: DeserializeOwned>(
    conn: &Connection,
    table: &str,
    predicate: &str,
) -> Result<Vec<T>> {
    load_table_filtered_params(conn, table, predicate, &[])
}

fn load_table_filtered_params<T: DeserializeOwned>(
    conn: &Connection,
    table: &str,
    predicate: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<Vec<T>> {
    let _span = performance::Span::new("sqlite.table_read_decode");
    let _packs = payload_store::read_scope();
    let columns = columns(conn, table)?;
    let selection = columns
        .iter()
        .map(|(name, _)| format!("\"{}\"", name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    let mut q = conn
        .prepare(&format!(
            "SELECT {selection} FROM {table} WHERE {predicate} ORDER BY rowid"
        ))
        .map_err(storage)?;
    let rows = q
        .query_map(parameters, |r| {
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
    rows.map(|row| {
        let mut value = row.map_err(storage)?;
        if table == "snapshots" {
            project_storage::restore_snapshot(conn, &mut value)?;
        }
        if table == "events"
            && value["kind"] == "resource.changed"
            && let Some(detail) = value["detail"]
                .as_str()
                .filter(|detail| detail.starts_with("[["))
        {
            value["detail"] = json!(store_changes::encode_versions(
                &store_changes::decode_versions(detail)?,
                false
            )?);
        }
        if table == "governance_records" {
            let kind = value["type"].as_str().unwrap_or("").to_owned();
            payload_store::restore_governance(conn, &kind, &mut value["data"])?;
        }
        if table == "conversation_events" && matches!(value["kind"].as_str(), Some("user" | "tool"))
        {
            let text = value["content"]
                .take()
                .as_str()
                .ok_or_else(|| err("storage", "消息正文不是文本"))?
                .to_owned();
            value["content"] = json!(payload_store::unpack_text(conn, text)?);
        }
        serde_json::from_value(value).map_err(storage)
    })
    .collect()
}
#[cfg(any(test, feature = "test-fixtures"))]
fn normalize_fixture_revisions<T: Serialize + DeserializeOwned>(
    items: &mut [T],
    before: &[T],
) -> Result<()> {
    let previous = before
        .iter()
        .map(|r| serde_json::to_value(r).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    for item in items {
        let mut value = serde_json::to_value(&*item).map_err(storage)?;
        let old = previous.iter().find(|r| r["id"] == value["id"]);
        if old == Some(&value) {
            continue;
        }
        let revision = old.and_then(|r| r["revision"].as_u64()).unwrap_or(0);
        if value["revision"].as_u64().unwrap_or(0) <= revision {
            value["revision"] = json!(revision + 1);
            *item = serde_json::from_value(value).map_err(storage)?;
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-fixtures"))]
fn guard_fixture_revisions<T: Serialize>(
    conn: &Connection,
    kind: &str,
    items: &[T],
    before: &[T],
) -> Result<()> {
    let previous = before
        .iter()
        .map(|row| {
            let value = serde_json::to_value(row).map_err(storage)?;
            Ok((value["id"].as_str().unwrap_or_default().to_owned(), value))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    for row in items {
        let value = serde_json::to_value(row).map_err(storage)?;
        let id = value["id"]
            .as_str()
            .ok_or_else(|| err("storage", "会话事实缺少身份"))?;
        let old = previous.get(id);
        if old == Some(&value) {
            continue;
        }
        if kind == "session" && value["scope"].is_null() {
            continue;
        }
        let revision = old.and_then(|r| r["revision"].as_u64()).unwrap_or(0);
        require(
            value["revision"]
                .as_u64()
                .is_some_and(|next| next > revision),
            "会话事实变化必须推进原版本",
        )?;
        resource_commits::verify_read_set(
            conn,
            &concurrency::ReadSet::new(vec![concurrency::ResourceVersion {
                resource: concurrency::ResourceKey::new(kind, id),
                revision,
            }])?,
        )?;
    }
    Ok(())
}

#[cfg(any(test, feature = "test-fixtures"))]
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
            .map_err(|e| err("storage", format!("{table} {}: {e}", v["id"])))?;
    }
    Ok(())
}

#[cfg(any(test, feature = "test-fixtures"))]
fn append_events(conn: &Connection, events: &[Event], before: &[Event]) -> Result<()> {
    require(events.starts_with(before), "事件历史只能追加")?;
    let mut statement = conn
        .prepare_cached("INSERT INTO events(target,kind,detail,time) VALUES(?1,?2,?3,?4)")
        .map_err(storage)?;
    for event in &events[before.len()..] {
        // Temporary in-memory IDs are display hints. SQLite assigns the committed
        // stream cursor, including when an independent resource committed meanwhile.
        statement
            .execute(rusqlite::params![
                event.target,
                event.kind,
                event.detail,
                event.time
            ])
            .map_err(storage)?;
    }
    Ok(())
}
impl SqliteStore {
    // Offline fixture setup is feature gated and never called by Host commands.
    #[cfg(feature = "test-fixtures")]
    fn fixture_transaction(
        &self,
        key: Option<(&str, &str)>,
        guards: Option<&concurrency::ReadSet>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        if let Some(guards) = guards {
            guards.validate()?;
        }
        let _span = performance::Span::new("project.transaction");
        let conn = self.connection.lock().map_err(storage)?;
        if let Some((id, fp)) = key {
            let mut q = conn
                .prepare("SELECT fingerprint,CAST(result AS TEXT) FROM operations WHERE id=?1")
                .map_err(storage)?;
            let mut rows = q.query([id]).map_err(storage)?;
            if let Some(row) = rows.next().map_err(storage)? {
                let saved: String = row.get(0).map_err(storage)?;
                require(
                    project_storage::fingerprint_matches(&saved, fp),
                    "command_id 被不同参数复用",
                )?;
                let result: String = row.get(1).map_err(storage)?;
                return source_views::restore(
                    &self.load_cached(&conn)?,
                    &payload_store::decode_result(&conn, &result)?,
                );
            }
        }
        conn.execute_batch("BEGIN;").map_err(storage)?;
        let mut state = self.load_cached(&conn)?;
        let execution_versions = {
            let mut query = conn
                .prepare_cached(
                    "SELECT resource_id,revision FROM resource_versions WHERE kind='execution'",
                )
                .map_err(storage)?;
            query
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                .map_err(storage)?
                .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
                .map_err(storage)?
        };
        conn.execute_batch("COMMIT;").map_err(storage)?;
        let before = state.clone();
        drop(conn);
        let result = action(&mut state)?;
        normalize_fixture_revisions(&mut state.projects, &before.projects)?;
        normalize_fixture_revisions(&mut state.features, &before.features)?;
        normalize_fixture_revisions(&mut state.tasks, &before.tasks)?;
        normalize_fixture_revisions(&mut state.contexts, &before.contexts)?;
        normalize_fixture_revisions(&mut state.sessions, &before.sessions)?;
        normalize_fixture_revisions(&mut state.turns, &before.turns)?;
        normalize_fixture_revisions(&mut state.interactions, &before.interactions)?;
        normalize_fixture_revisions(&mut state.proposals, &before.proposals)?;
        normalize_fixture_revisions(
            &mut state.repository_operations,
            &before.repository_operations,
        )?;

        let changed = serde_json::to_vec(&before).map_err(storage)?
            != serde_json::to_vec(&state).map_err(storage)?;
        // Idle observations cannot introduce a new invalid state. Avoid parsing
        // and hashing every historical task/plan again on each empty poll.
        if changed {
            state.validate_governance(&before)?;
            state.validate_conversations(&before)?;
        }
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
        if let Some(guards) = guards {
            resource_commits::verify_read_set(&tx, guards)?;
        }
        if changed {
            let old = before
                .executions
                .iter()
                .map(|e| (&e.id, e))
                .collect::<std::collections::HashMap<_, _>>();
            let mut versions = Vec::new();
            for next in &state.executions {
                if old.get(&next.id).is_some_and(|prior| *prior == next) {
                    continue;
                }
                let revision = if old.contains_key(&next.id) {
                    *execution_versions
                        .get(&next.id)
                        .ok_or_else(|| err("storage", "原 worker 缺少技术版本"))?
                } else {
                    0
                };
                versions.push(concurrency::ResourceVersion {
                    resource: concurrency::ResourceKey::new("execution", &next.id),
                    revision,
                });
            }
            resource_commits::verify_read_set(&tx, &concurrency::ReadSet::new(versions)?)?;
            Self::save(&tx, &state, &before)?;
        }
        if let Some((id, fp)) = key {
            tx.execute(
                "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
                rusqlite::params![id, fp, source_views::pack(&state, &result)?.to_string()],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        if catalog_changed(&before, &state) {
            self.catalog_cache.invalidate()?;
        }
        drop(conn);
        if changed {
            self.changes.notify();
        }
        Ok(result)
    }
}
impl EngineeringStore for SqliteStore {
    fn operational_read(&self, method: &str, params: &Value) -> Result<Value> {
        operational_reads::read(
            &self.connection,
            self.configuration_snapshot()?.as_ref(),
            self.project_id.as_deref(),
            method,
            params,
        )
    }
    fn workspace_reading(&self, p: &Value) -> Result<Value> {
        workspace_read::read(self, p)
    }
    fn replay_commit(
        &self,
        command_id: &str,
        fingerprint: &str,
    ) -> Result<Option<concurrency::CommitReceipt>> {
        resource_commits::replay(&self.connection, command_id, fingerprint)
    }
    fn read_resources(
        &self,
        keys: &[concurrency::ResourceKey],
    ) -> Result<Vec<concurrency::ResourceSnapshot>> {
        resource_commits::read(&self.connection, keys)
    }
    fn submit_changes(
        &self,
        changes: &concurrency::PreparedChangeSet,
    ) -> Result<concurrency::CommitOutcome> {
        if let Some(project) = &self.project_id {
            for change in &changes.changes {
                if change.resource.kind == "governance" {
                    let scope: Scope = serde_json::from_value(change.value["scope"].clone())
                        .map_err(|error| err("invalid_params", error.to_string()))?;
                    let catalog = change.value["type"]
                        .as_str()
                        .is_some_and(devdesk_core::catalog_kind);
                    require(
                        scope.project_id() == Some(project.as_str())
                            || (scope.is_global() && catalog),
                        format!(
                            "项目数据库不能提交其他项目或全局的运行事实：{} {}（{:?}）",
                            change.value["type"], change.resource.id, scope
                        ),
                    )?;
                    if !catalog {
                        require(
                            change.value["data"]
                                .get("project_id")
                                .is_none_or(|value| value.as_str() == Some(project.as_str())),
                            "运行事实正文与项目归属不一致",
                        )?;
                    }
                }
                require(
                    (change.resource.kind != "project" || &change.resource.id == project)
                        && change.value.get("project_id").is_none_or(|value| {
                            value.is_null() || value.as_str() == Some(project.as_str())
                        }),
                    "项目数据库不能提交其他项目的事实",
                )?;
            }
        }
        let (receipt, changed) = resource_commits::commit(&self.connection, changes)?;
        if changed {
            // The persisted catalog clock already identifies invalidation.
            // Runtime/pref commits must keep the immutable source view alive.
            self.changes.notify();
        }
        Ok(concurrency::CommitOutcome {
            receipt,
            applied: changed,
        })
    }
    fn orchestration_summary(
        &self,
        query: &devdesk_core::read_models::ControllerQuery,
    ) -> Result<devdesk_core::read_models::OrchestrationSummary> {
        read_models::orchestration(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "经理摘要需要明确项目"))?,
            query,
        )
    }
    fn workflow_driver_summary(
        &self,
        id: &str,
    ) -> Result<devdesk_core::read_models::WorkflowDriverSummary> {
        read_models::driver(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "驱动摘要需要明确项目"))?,
            id,
        )
    }
    fn workflow_decision(&self, id: &str) -> Result<devdesk_core::read_models::DecisionDetail> {
        read_models::decision(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "决定请求需要明确项目"))?,
            id,
        )
    }
    fn workflow_decision_context(
        &self,
        instance: &str,
        execution: Option<&str>,
    ) -> Result<devdesk_core::read_models::DecisionContext> {
        read_models::decision_context(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "审批展示依据需要明确项目"))?,
            instance,
            execution,
        )
    }
    fn workflow_management_summary(
        &self,
        id: &str,
        child_offset: u64,
    ) -> Result<devdesk_core::read_models::WorkflowManagementSummary> {
        read_models::management(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "流程摘要需要明确项目"))?,
            id,
            child_offset,
        )
    }
    fn work_configuration_view(
        &self,
        owner: &str,
        project: Option<&str>,
        include_history: bool,
    ) -> Result<Value> {
        standard_reads::work_configuration(self, owner, project, include_history)
    }
    fn standard_work_snapshot(&self, usage_ref: Option<&str>) -> Result<EngineeringState> {
        standard_reads::snapshot(self, usage_ref)
    }
    fn workflow_progress(
        &self,
        id: &str,
        stage_offset: u64,
    ) -> Result<devdesk_core::read_models::WorkflowProgress> {
        read_models::progress(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "工作流进度需要明确项目"))?,
            id,
            stage_offset,
        )
    }
    fn workflow_detail(
        &self,
        query: &devdesk_core::read_models::WorkflowDetailQuery,
    ) -> Result<Option<devdesk_core::read_models::WorkflowDetailFacts>> {
        workflow_details::with_catalog(self, |catalog| {
            workflow_details::detail(
                &self.connection,
                catalog,
                self.project_id
                    .as_deref()
                    .ok_or_else(|| err("project_required", "流程详情需要项目"))?,
                query,
            )
        })
    }
    fn execution_task_detail(&self, turn: &str) -> Result<Option<Value>> {
        workflow_details::with_catalog(self, |catalog| {
            workflow_details::task(
                &self.connection,
                catalog,
                self.project_id
                    .as_deref()
                    .ok_or_else(|| err("project_required", "冻结任务需要项目"))?,
                turn,
            )
        })
    }
    fn project_records(&self, method: &str, params: &Value) -> Result<Value> {
        workflow_details::with_catalog(self, |catalog| {
            project_records::read(&self.connection, catalog, method, params)
        })
    }
    fn conversation_query(&self, method: &str, params: &Value) -> Result<Value> {
        workflow_details::with_catalog(self, |catalog| {
            conversation_queries::query(&self.connection, catalog, method, params)
        })
    }
    fn workflow_reading(
        &self,
        view: devdesk_core::read_models::WorkflowReadingView,
        params: &Value,
    ) -> Result<Value> {
        workflow_details::with_catalog(self, |catalog| {
            workflow_readers::read(
                &self.connection,
                catalog,
                self.project_id
                    .as_deref()
                    .ok_or_else(|| err("project_required", "流程读取需要项目"))?,
                view,
                params,
            )
        })
    }
    fn observe_query(&self, stage: &'static str) -> Box<dyn ports::QueryObservation> {
        performance::observe(stage)
    }
    fn advisory_read_scope(&self) -> Box<dyn ports::QueryObservation> {
        resource_read_scope::enter(&self.connection)
    }
    fn observe_native_call(
        &self,
        call_id: &str,
        stage: &'static str,
    ) -> Box<dyn ports::QueryObservation> {
        performance::observe_native(call_id, stage)
    }
    fn repository_registration_facts(
        &self,
    ) -> Result<devdesk_core::repository_updates::RepositoryRegistrationFacts> {
        repository_contexts::registration(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "仓库上下文需要明确项目"))?,
        )
    }
    fn repository_refresh_facts(
        &self,
    ) -> Result<devdesk_core::repository_updates::RepositoryRefreshFacts> {
        repository_contexts::refresh(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "仓库上下文需要明确项目"))?,
        )
    }
    fn repository_connect_projection_facts(
        &self,
        operation: &str,
    ) -> Result<devdesk_core::repository_projection::RepositoryConnectProjectionFacts> {
        repository_contexts::connect_projection(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "仓库接纳需要明确项目"))?,
            operation,
        )
    }
    fn project_metadata(&self) -> Result<Project> {
        execution_read::project(
            &*self.connection.read()?,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "项目元数据需要明确项目"))?,
        )
    }
    fn execution_workspace(
        &self,
        query: &devdesk_core::read_models::ExecutionWorkspaceQuery,
    ) -> Result<devdesk_core::read_models::ExecutionWorkspace> {
        execution_read::workspace(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "执行工作台需要明确项目"))?,
            query,
        )
    }
    fn feature_branches(&self, feature: &str, task_offset: u64) -> Result<Value> {
        execution_read::feature_branches(
            &self.connection,
            self.project_id
                .as_deref()
                .ok_or_else(|| err("project_required", "分支查询需要明确项目"))?,
            feature,
            task_offset,
        )
    }
    fn experience_headers(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::read_models::ExperienceHeaderPage> {
        experience_directory::headers(&self.connection, self.project_id.as_deref(), after)
    }
    fn experience_directory_source(
        &self,
        id: &str,
    ) -> Result<Option<devdesk_core::read_models::ExperienceDirectorySource>> {
        experience_directory::source(&self.connection, self.project_id.as_deref(), id)
    }
    fn event_cursor(&self) -> Result<u64> {
        store_changes::cursor(&*self.connection.read()?)
    }
    fn event_hints_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<Vec<devdesk_core::read_models::TailEvent>> {
        store_changes::tail(&*self.connection.read()?, cursor, limit)
    }
    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<Event>> {
        store_changes::events_after(&*self.connection.read()?, cursor, limit)
    }
    fn subscribe_changes(&self, listener: std::sync::Weak<dyn Fn() + Send + Sync>) {
        self.changes.subscribe(listener);
    }
    fn has_pending_tool_calls(&self) -> Result<bool> {
        store_changes::pending_tools(&*self.connection.read()?)
    }
    fn provider_tool_intent_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        tool_calls::provider_intents(&self.connection, after)
    }
    fn provider_poll_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        tool_calls::provider_polls(&self.connection, after)
    }
    fn conversation_capture_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        conversation_captures::page(&self.connection, after)
    }
    fn conversation_capture_facts(
        &self,
        session: &Session,
        turn: &str,
        terminal: &str,
        worker_boundary: u64,
    ) -> Result<devdesk_core::ConversationCaptureFacts> {
        conversation_captures::facts(&self.connection, session, turn, terminal, worker_boundary)
    }
    fn conversation_input_facts(
        &self,
        session: &Session,
        through_turn: Option<&str>,
    ) -> Result<devdesk_core::ConversationCaptureFacts> {
        conversation_captures::input(&self.connection, session, through_turn)
    }
    fn conversation_page(
        &self,
        queued: bool,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        conversation_reads::page(&self.connection, queued, after)
    }
    fn conversation_answers_pending(&self, turn: &str) -> Result<Vec<PendingInteraction>> {
        conversation_reads::answers(&self.connection, turn)
    }
    fn conversation_runtime_facts(
        &self,
        turn: &str,
    ) -> Result<Option<devdesk_core::ConversationRuntimeFacts>> {
        for _ in 0..2 {
            let catalog = self.configuration_snapshot()?;
            match conversation_reads::runtime(&self.connection, &catalog, turn) {
                Err(error) if error.code == "catalog_refresh_required" => continue,
                result => return result,
            }
        }
        Err(err("revision_conflict", "会话引用的源目录持续变化，请重读"))
    }
    fn conversation_start_source(
        &self,
        turn: &str,
    ) -> Result<devdesk_core::ConversationStartSource> {
        for _ in 0..2 {
            let catalog = self.configuration_snapshot()?;
            match conversation_sources::source(&self.connection, &catalog, turn) {
                Err(error) if error.code == "catalog_refresh_required" => continue,
                result => return result,
            }
        }
        Err(err(
            "revision_conflict",
            "冻结派发引用的源目录持续变化，请重读",
        ))
    }
    fn context_call_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        context_calls::page(&self.connection, after)
    }
    fn context_call_facts(&self, id: &str) -> Result<devdesk_core::ContextCallFacts> {
        for _ in 0..2 {
            let catalog = self.configuration_snapshot()?;
            match context_calls::facts(&self.connection, &catalog, id) {
                Err(error) if error.code == "catalog_refresh_required" => continue,
                result => return result,
            }
        }
        Err(err("revision_conflict", "上下文源目录持续变化，请重读"))
    }
    fn platform_calls_pending(&self) -> Result<bool> {
        let conn = self.connection.read()?;
        conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records INDEXED BY idx_platform_call_active WHERE type='platform_call' AND json_extract(data,'$.status') IN ('prepared','running')) OR EXISTS(SELECT 1 FROM governance_records r WHERE r.type='agent_platform_request' AND NOT EXISTS(SELECT 1 FROM governance_records p WHERE p.type='platform_tool_reply' AND p.definition_id=json_extract(r.data,'$.request.execution_id')) AND (EXISTS(SELECT 1 FROM governance_records c WHERE c.id=json_extract(r.data,'$.request.execution_id') AND c.type='platform_call' AND json_extract(c.data,'$.status') IN ('succeeded','failed','cancelled','needs_reconciliation')) OR EXISTS(SELECT 1 FROM governance_records i WHERE i.type='workflow_intent' AND json_extract(i.data,'$.status')='failed' AND json_extract(i.data,'$.command.payload.agent_platform_call_id')=json_extract(r.data,'$.request.execution_id'))))",[],|r|r.get(0)).map_err(storage)
    }
    fn runtime_record_page(
        &self,
        kind: &str,
        status: &str,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        if kind == "workflow_instance" {
            return self.runtime_record_page_at(kind, status, after, &runtime_queries::utc_clock());
        }
        let conn = self.connection.read()?;
        let sql = if kind == "workflow_controller" {
            format!(
                "SELECT c.id FROM governance_records c INDEXED BY idx_runtime_work_seek WHERE c.type=?1 AND json_extract(c.data,'$.status')=?2 AND c.id>?3 AND {} ORDER BY c.id LIMIT 17",
                if status == "budget_exhausted" {
                    runtime_queries::CONTROLLER_RECOVERABLE_BUDGET
                } else {
                    runtime_queries::CONTROLLER_READY
                }
            )
        } else if kind == "workflow_segment" && status == "stopped" {
            format!(
                "SELECT c.id FROM governance_records c INDEXED BY idx_runtime_work_seek WHERE c.type=?1 AND json_extract(c.data,'$.status')=?2 AND c.id>?3 AND {} ORDER BY c.id LIMIT 17",
                runtime_queries::SEGMENT_RECOVERABLE
            )
        } else if kind == "workflow_driver" {
            "SELECT id FROM governance_records WHERE type=?1 AND json_extract(data,'$.mode')=?2 AND id>?3 ORDER BY id LIMIT 17".into()
        } else {
            "SELECT id FROM governance_records INDEXED BY idx_runtime_work_seek WHERE type=?1 AND json_extract(data,'$.status')=?2 AND id>?3 ORDER BY id LIMIT 17".into()
        };
        let mut q = conn.prepare_cached(&sql).map_err(storage)?;
        let mut ids = q
            .query_map(rusqlite::params![kind, status, after.unwrap_or("")], |r| {
                r.get::<_, String>(0)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let more = ids.len() > 16;
        ids.truncate(16);
        Ok(devdesk_core::tool_calls::ToolCallPage {
            next_after: more.then(|| ids.last().unwrap().clone()),
            ids,
        })
    }
    fn runtime_record_page_at(
        &self,
        kind: &str,
        status: &str,
        after: Option<&str>,
        clock: &ExecutionClockAnchor,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        if kind != "workflow_instance" {
            return self.runtime_record_page(kind, status, after);
        }
        let conn = self.connection.read()?;
        let sql = format!(
            "SELECT c.id FROM governance_records c INDEXED BY idx_runtime_work_seek WHERE c.type=?4 AND json_extract(c.data,'$.status')=?5 AND c.id>?6 AND ({} OR {}) ORDER BY c.id LIMIT 17",
            runtime_queries::WORKFLOW_READY,
            runtime_queries::WORKFLOW_TIME_READY
        );
        let mut query = conn.prepare_cached(&sql).map_err(storage)?;
        let mut ids = query
            .query_map(
                rusqlite::params![
                    clock.wall_ms,
                    clock.process_epoch,
                    clock.monotonic_ms,
                    kind,
                    status,
                    after.unwrap_or("")
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let more = ids.len() > 16;
        ids.truncate(16);
        Ok(devdesk_core::tool_calls::ToolCallPage {
            next_after: more.then(|| ids.last().unwrap().clone()),
            ids,
        })
    }
    fn tool_call_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        tool_calls::page(&self.connection, after)
    }
    fn tool_call_facts(&self, id: &str) -> Result<devdesk_core::tool_calls::ToolCallFacts> {
        tool_calls::facts(
            &self.connection,
            self.configuration_snapshot()?.as_ref(),
            id,
        )
    }
    fn workflow_intent_page(
        &self,
        after: Option<&str>,
    ) -> Result<devdesk_core::WorkflowIntentPage> {
        workflow_intents::page(&self.connection, after)
    }
    fn workflow_intent_facts(
        &self,
        id: &str,
        runnable_only: bool,
    ) -> Result<Option<devdesk_core::WorkflowIntentFacts>> {
        workflow_intents::facts(
            &self.connection,
            self.configuration_snapshot()?.as_ref(),
            id,
            runnable_only,
            self.project_id.as_deref(),
        )
    }
    fn workflow_intent_active(&self, id: &str, revision: Option<u64>) -> Result<bool> {
        workflow_intents::active(&self.connection, id, revision, self.project_id.as_deref())
    }
    fn encode_runtime_record(&self, record: &ConfigurationRecord) -> Result<Value> {
        tool_calls::encode(self.configuration_snapshot()?.as_ref(), record)
    }
    fn encode_configuration_record(
        &self,
        catalog: &ConfigurationCatalogSnapshot,
        record: &ConfigurationRecord,
    ) -> Result<Value> {
        Ok(source_store::pack_record(&catalog.state, record)?.to_storage())
    }
    fn encode_configuration_value(
        &self,
        catalog: &ConfigurationCatalogSnapshot,
        value: &Value,
    ) -> Result<Value> {
        source_views::pack(&catalog.state, value)
    }
    fn restore_runtime_value(&self, value: &Value) -> Result<Value> {
        source_views::restore(&self.configuration_snapshot()?.state, value)
    }
    fn encode_runtime_value(&self, value: &Value) -> Result<Value> {
        source_views::pack(&self.configuration_snapshot()?.state, value)
    }
    fn encode_execution_task(&self, task: &ConfiguredAgentTask) -> Result<(Value, String)> {
        source_views::encode_task(self.configuration_snapshot()?.as_ref(), task)
    }
    fn encode_workflow_input_intent(&self, record: &ConfigurationRecord) -> Result<Value> {
        source_views::encode_workflow_input_intent(self.configuration_snapshot()?.as_ref(), record)
    }
    fn restore_runtime_record(&self, value: &Value) -> Result<Value> {
        tool_calls::restore(self.configuration_snapshot()?.as_ref(), value)
    }
    fn workflow_tool_facts(
        &self,
        authority: &devdesk_core::tool_calls::WorkflowToolAuthority,
        project: &str,
    ) -> Result<devdesk_core::tool_calls::WorkflowToolFacts> {
        require(
            self.project_id.as_deref().is_none_or(|id| id == project),
            "工具步骤跨项目",
        )?;
        tool_calls::workflow(&self.connection, authority, project)
    }
    fn catalog_revision(&self) -> Result<u64> {
        configuration_cache::revision(&*self.connection.read()?)
    }
    fn prepare_configuration_reset(
        &self,
        records: &[ConfigurationRecord],
        revision: u64,
    ) -> Result<ConfigurationPublication> {
        configuration_reset::prepare(self, records, revision)
    }
    fn configuration_snapshot(&self) -> Result<std::sync::Arc<ConfigurationCatalogSnapshot>> {
        self.catalog_cache.read(&*self.connection.read()?, true)
    }
    fn workflow_runtime_pending(&self, kind: ports::WorkflowRuntimeWork) -> Result<bool> {
        runtime_queries::pending(&*self.connection.read()?, kind)
    }
    fn native_platform_tool_prepared(&self, id: &str) -> Result<bool> {
        runtime_queries::native_prepared(&*self.connection.read()?, id)
    }
    fn conversation_running_count(&self) -> Result<usize> {
        runtime_queries::running(&*self.connection.read()?)
    }
    fn has_pending_work(&self) -> Result<bool> {
        scheduling_read::pending(&*self.connection.read()?)
    }
    fn active_conversation_count(&self) -> Result<usize> {
        scheduling_read::running(&*self.connection.read()?)
    }
    fn scheduling_hints(&self) -> Result<devdesk_core::read_models::SchedulingHints> {
        scheduling_read::hints(&*self.connection.read()?)
    }
    fn scheduling_hints_at(
        &self,
        clock: &ExecutionClockAnchor,
    ) -> Result<devdesk_core::read_models::SchedulingHints> {
        scheduling_read::hints_at(&*self.connection.read()?, clock)
    }
    fn project_id(&self) -> Option<String> {
        self.project_id.clone()
    }
    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    fn snapshot(&self) -> Result<EngineeringState> {
        let waiting = performance::Span::new("project.connection_wait");
        let conn = self.connection.read()?;
        drop(waiting);
        conn.execute_batch("BEGIN;").map_err(storage)?;
        let state = self.load_cached(&conn)?;
        conn.execute_batch("COMMIT;").map_err(storage)?;
        Ok(state)
    }
    #[cfg(feature = "test-fixtures")]
    fn seed_fixture_state(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        self.fixture_transaction(key, None, action)
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
    fn checked_path(&self, relative_path: &str) -> Result<PathBuf> {
        let root = self.root.canonicalize().map_err(storage)?;
        reject_symlink(&root.join("artifacts"))?;
        let base = root.join("artifacts");
        require(
            base.canonicalize().map_err(storage)? == base,
            "Artifact 根目录越界",
        )?;
        let relative = Path::new(relative_path);
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
        Ok(path)
    }
}
impl ArtifactStore for FileArtifacts {
    fn seal_prepared(&self, seal: &ArtifactSeal, body: &str) -> Result<Artifact> {
        named_artifacts::seal(&self.root, seal, body)
    }
    fn inspect_prepared(&self, seal: &ArtifactSeal) -> Result<Option<Artifact>> {
        named_artifacts::inspect(&self.root, seal)
    }
    fn seal(
        &self,
        feature: &str,
        task: Option<&str>,
        category: &str,
        body: &str,
    ) -> Result<Artifact> {
        self.seal_scoped(
            &if let Some(id) = task {
                MaterialOwner::Task {
                    feature: feature.into(),
                    id: id.into(),
                }
            } else {
                MaterialOwner::Feature { id: feature.into() }
            },
            category,
            body,
        )
    }
    fn seal_scoped(&self, owner: &MaterialOwner, category: &str, body: &str) -> Result<Artifact> {
        let (project_id, feature_id, task, folder) = match owner {
            MaterialOwner::Global => (None, None, None, "global".into()),
            MaterialOwner::Project { id } => {
                (Some(id.clone()), None, None, format!("project-{id}"))
            }
            MaterialOwner::Feature { id } => (None, Some(id.clone()), None, id.clone()),
            MaterialOwner::Task { feature, id } => (
                None,
                Some(feature.clone()),
                Some(id.as_str()),
                feature.clone(),
            ),
        };
        let feature = folder.as_str();
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
        let extension = if ["snapshots", "workflow-outputs", "proposals"].contains(&category) {
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
            feature_id,
            project_id,
            task_id: task.map(str::to_owned),
            relative_path: path,
            sha256,
            media_type: if extension == "json" {
                "application/json"
            } else {
                "text/markdown; charset=utf-8"
            }
            .into(),
            origin: Origin::Human,
        })
    }
    fn read(&self, artifact: &Artifact) -> Result<Vec<u8>> {
        let path = self.checked_path(&artifact.relative_path)?;
        let bytes = fs::read(path).map_err(storage)?;
        require(
            format!("{:x}", Sha256::digest(&bytes)) == artifact.sha256,
            "Artifact 哈希不匹配",
        )?;
        Ok(bytes)
    }
    fn retire_protocol_copy(
        &self,
        execution: &RuntimeExecution,
        artifact: &Artifact,
    ) -> Result<()> {
        require(
            artifact.origin == Origin::External,
            "只清理已封存的外部 worker 日志",
        )?;
        storage_maintenance::retire_protocol(
            &self.root,
            &execution.session_id,
            &execution.operation_id,
            &artifact.relative_path,
            &artifact.sha256,
        )
    }
}

/// Native execution providers. Construction performs no Docker, GitHub or model calls.
pub fn execution_adapters(root: &Path) -> Result<ports::ExecutionAdapters> {
    fs::create_dir_all(root).map_err(storage)?;
    Ok(ports::ExecutionAdapters {
        harness: std::sync::Arc::new(RoutingHarness::new(root)?),
        git: std::sync::Arc::new(CliGit::new(root)?),
        tests: std::sync::Arc::new(CommandTests::new(root)?),
        pull_requests: Some(std::sync::Arc::new(github::GitHubPullRequests::new(root)?)),
    })
}

/// Product/test composition boundary for callers constructing a state without a Store.
pub fn seed_factory_configuration(state: &mut EngineeringState) {
    factory_bundle::initialize().expect("validated packaged factory resources");
    devdesk_core::seed_configuration(state);
}
