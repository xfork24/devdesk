//! Global configuration and resumable migration from the former multi-project database.
use super::*;
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};

pub struct GlobalStore {
    pub(super) connection: Mutex<Connection>,
    pub(super) writer: Mutex<()>,
    root: PathBuf,
    _lock: File,
}

impl GlobalStore {
    pub fn dsh_credential_reference(&self) -> Result<Option<String>> {
        Ok(setting(
            &*self.connection.lock().map_err(storage)?,
            "credentials.dsh",
        )?
        .and_then(|v| v.as_str().map(str::to_owned)))
    }
    pub fn set_dsh_credential_reference(&self, reference: &str) -> Result<()> {
        put_setting(
            &*self.connection.lock().map_err(storage)?,
            "credentials.dsh",
            &json!(reference),
        )
    }
    pub fn open(path: &Path) -> Result<Self> {
        let (root, lock) = lock_dataset(path)?;
        let mut connection = Connection::open(root.join("global.sqlite3")).map_err(storage)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;",
            )
            .map_err(storage)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        require(version <= 4, "全局配置库版本比程序新")?;
        if version == 0 {
            let tx = connection.transaction().map_err(storage)?;
            tx.execute_batch(include_str!("../../../sql/global/001_initial.sql"))
                .map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if version < 2 {
            let tx = connection.transaction().map_err(storage)?;
            tx.execute_batch(include_str!(
                "../../../sql/global/002_runtime_installations.sql"
            ))
            .map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if version < 3 {
            let tx = connection.transaction().map_err(storage)?;
            tx.execute_batch(include_str!("../../../sql/global/003_agent_profiles.sql"))
                .map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if version < 4 {
            let tx = connection.transaction().map_err(storage)?;
            tx.execute_batch(include_str!("../../../sql/global/004_permissions.sql"))
                .map_err(storage)?;
            let legacy = version != 0 || root.join("engineering.sqlite3").exists();
            tx.execute(
                "INSERT INTO settings(key,value) VALUES('permission_configuration',?1)",
                [
                    serde_json::to_string(&PermissionConfiguration::defaults(legacy))
                        .map_err(storage)?,
                ],
            )
            .map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        let store = Self {
            connection: Mutex::new(connection),
            writer: Mutex::new(()),
            root,
            _lock: lock,
        };
        store.migrate_legacy()?;
        Ok(store)
    }

    pub fn concurrency(&self) -> Result<usize> {
        let conn = self.connection.lock().map_err(storage)?;
        Ok(setting(&conn, "global_concurrency")?
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize)
    }

    pub fn set_concurrency(&self, value: usize) -> Result<()> {
        require((1..=32).contains(&value), "全局并发额度需为 1—32")?;
        put_setting(
            &*self.connection.lock().map_err(storage)?,
            "global_concurrency",
            &json!(value),
        )
    }

    pub fn default_harness(&self) -> Result<HarnessProvider> {
        let value = setting(
            &*self.connection.lock().map_err(storage)?,
            "default_harness",
        )?
        .unwrap_or(json!("codex"));
        serde_json::from_value(value).map_err(storage)
    }

    /// Change preferences together; existing immutable execution bindings are unaffected.
    pub fn update_preferences(
        &self,
        concurrency: Option<usize>,
        provider: Option<HarnessProvider>,
    ) -> Result<()> {
        require(
            concurrency.is_some() || provider.is_some(),
            "设置内容不能为空",
        )?;
        if let Some(n) = concurrency {
            require((1..=32).contains(&n), "全局并发额度需为 1—32")?;
        }
        let _writer = self.writer.lock().map_err(storage)?;
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        if let Some(n) = concurrency {
            put_setting(&tx, "global_concurrency", &json!(n))?;
        }
        if let Some(p) = provider {
            put_setting(&tx, "default_harness", &json!(p))?;
        }
        tx.commit().map_err(storage)
    }

    pub fn active_project(&self) -> Result<Option<String>> {
        Ok(
            setting(&*self.connection.lock().map_err(storage)?, "active_project")?
                .and_then(|v| v.as_str().map(str::to_owned)),
        )
    }

    pub fn activate(&self, id: &str) -> Result<()> {
        require(self.project(id)?.is_some(), "项目尚未创建完成")?;
        put_setting(
            &*self.connection.lock().map_err(storage)?,
            "active_project",
            &json!(id),
        )
    }

    pub fn activate_command(&self, command: &Command) -> Result<()> {
        let id = command
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "需要 Project target"))?;
        require(self.project(id)?.is_some(), "项目不存在")?;
        let fp = serde_json::to_string(&(
            "project.open",
            &command.target,
            &command.expected_revision,
            &command.payload,
        ))
        .map_err(storage)?;
        let _writer = self.writer.lock().map_err(storage)?;
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT fingerprint FROM operations WHERE id=?1",
                [&command.command_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(previous) = previous {
            require(previous == fp, "command_id 被不同参数复用")?;
        } else {
            require(
                !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE command_id=?1)",
                    [&command.command_id],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage)?,
                "command_id 被不同参数复用",
            )?;
            tx.execute(
                "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
                params![command.command_id, fp, json!({"id":id}).to_string()],
            )
            .map_err(storage)?;
        }
        put_setting(&tx, "active_project", &json!(id))?;
        tx.commit().map_err(storage)?;
        Ok(())
    }

    /// Reject reused global command keys before opening a project can run recovery hooks.
    pub fn validate_open_command(&self, command: &Command) -> Result<()> {
        let fp = serde_json::to_string(&(
            "project.open",
            &command.target,
            &command.expected_revision,
            &command.payload,
        ))
        .map_err(storage)?;
        let conn = self.connection.lock().map_err(storage)?;
        let old: Option<String> = conn
            .query_row(
                "SELECT fingerprint FROM operations WHERE id=?1",
                [&command.command_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(old) = old {
            require(old == fp, "command_id 被不同参数复用")?;
        }
        require(
            !conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE command_id=?1)",
                    [&command.command_id],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage)?,
            "command_id 被不同参数复用",
        )
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>> {
        let conn = self.connection.lock().map_err(storage)?;
        let body: Option<String> = conn
            .query_row(
                "SELECT project FROM project_catalog WHERE id=?1 AND project IS NOT NULL",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        body.map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
    }

    pub fn projects(&self, offset: usize, limit: usize) -> Result<(Vec<Project>, usize)> {
        self.projects_filtered(offset, limit, false)
    }
    pub fn github_projects(&self, offset: usize, limit: usize) -> Result<(Vec<Project>, usize)> {
        self.projects_filtered(offset, limit, true)
    }
    fn projects_filtered(
        &self,
        offset: usize,
        limit: usize,
        github_only: bool,
    ) -> Result<(Vec<Project>, usize)> {
        let conn = self.connection.lock().map_err(storage)?;
        let filter = if github_only {
            "project IS NOT NULL AND coalesce(json_extract(project,'$.workspace_definition.repository'),'') <> ''"
        } else {
            "project IS NOT NULL"
        };
        let count = conn
            .query_row(
                &format!("SELECT count(*) FROM project_catalog WHERE {filter}"),
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let mut q = conn.prepare(&format!("SELECT project FROM project_catalog WHERE {filter} ORDER BY rowid LIMIT ?1 OFFSET ?2")).map_err(storage)?;
        let bodies = q
            .query_map(params![limit.min(100) as i64, offset as i64], |r| {
                r.get::<_, String>(0)
            })
            .map_err(storage)?;
        let mut projects = vec![];
        for body in bodies {
            projects.push(serde_json::from_str(&body.map_err(storage)?).map_err(storage)?);
        }
        Ok((projects, count))
    }

    /// Reserve an identity before creating its database, so retries after a crash use the same file.
    pub fn reserve_project(&self, command: &Command) -> Result<String> {
        require(!command.command_id.is_empty(), "command_id 不能为空")?;
        require(
            command.target.is_none() && command.expected_revision.is_none(),
            "新建项目不能指定 target 或 expected_revision",
        )?;
        require(
            command.payload["title"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
            "缺少 title",
        )?;
        let fp = serde_json::to_string(&(
            "project.create",
            &command.target,
            &command.expected_revision,
            &command.payload,
        ))
        .map_err(storage)?;
        let _writer = self.writer.lock().map_err(storage)?;
        let conn = self.connection.lock().map_err(storage)?;
        require(
            !conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM operations WHERE id=?1)",
                    [&command.command_id],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage)?,
            "command_id 被不同参数复用",
        )?;
        let old: Option<(String, String)> = conn
            .query_row(
                "SELECT id,fingerprint FROM project_catalog WHERE command_id=?1",
                [&command.command_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((id, previous)) = old {
            require(fp == previous, "command_id 被不同参数复用")?;
            return Ok(id);
        }
        let id = devdesk_domain::id();
        conn.execute(
            "INSERT INTO project_catalog(id,command_id,fingerprint) VALUES(?1,?2,?3)",
            params![id, command.command_id, fp],
        )
        .map_err(storage)?;
        Ok(id)
    }

    /// This is a directory summary; the project database remains authoritative.
    pub fn publish_project(&self, project: &Project) -> Result<()> {
        let conn = self.connection.lock().map_err(storage)?;
        let updated = conn
            .execute(
                "UPDATE project_catalog SET project=?1 WHERE id=?2 AND (project IS NULL OR json_extract(project,'$.revision')<=?3)",
                params![serde_json::to_string(project).map_err(storage)?, project.id, project.revision],
            )
            .map_err(storage)?;
        require(
            updated == 1
                || conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE id=?1)",
                        [&project.id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(storage)?,
            "项目未登记",
        )
    }

    fn configuration(conn: &Connection) -> Result<EngineeringState> {
        Ok(EngineeringState {
            permissions: load_table(conn, "permissions")?,
            definitions: load_table(conn, "definitions")?,
            harnesses: load_table(conn, "harnesses")?,
            sandboxes: load_table(conn, "sandboxes")?,
            events: load_table(conn, "events")?,
            ..Default::default()
        })
    }

    fn save_configuration(
        conn: &Connection,
        s: &EngineeringState,
        before: &EngineeringState,
    ) -> Result<()> {
        save_table(conn, "permissions", &s.permissions, &before.permissions)?;
        save_table(conn, "definitions", &s.definitions, &before.definitions)?;
        save_table(conn, "harnesses", &s.harnesses, &before.harnesses)?;
        save_table(conn, "sandboxes", &s.sandboxes, &before.sandboxes)?;
        save_table(conn, "events", &s.events, &before.events)
    }

    fn migrate_legacy(&self) -> Result<()> {
        let conn = self.connection.lock().map_err(storage)?;
        if setting(&conn, "layout_version")? == Some(json!(2)) {
            return Ok(());
        }
        drop(conn);
        let legacy_path = self.root.join("engineering.sqlite3");
        let mut source = EngineeringState::default();
        let mut operations = vec![];
        let copy_path = self
            .root
            .join(format!("migration-{}.sqlite3", devdesk_domain::id()));
        let _copy_cleanup = MigrationCopy(copy_path.clone());
        if legacy_path.exists() {
            // SQLite makes a consistent copy including WAL. Schema upgrades affect the copy only.
            let original = Connection::open_with_flags(
                &legacy_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(storage)?;
            original
                .execute("VACUUM INTO ?1", [copy_path.to_string_lossy().as_ref()])
                .map_err(storage)?;
            let copy = open_engineering_connection(&copy_path)?;
            source = SqliteStore::load(&copy)?;
            let mut q = copy
                .prepare("SELECT id,fingerprint,result FROM operations ORDER BY rowid")
                .map_err(storage)?;
            for op in q
                .query_map([], |r| {
                    Ok(Operation {
                        id: r.get(0)?,
                        fingerprint: r.get(1)?,
                        result: r.get(2)?,
                    })
                })
                .map_err(storage)?
            {
                operations.push(op.map_err(storage)?);
            }
        }
        seed_configuration(&mut source);
        let mut catalog = vec![];
        let global_ops: Vec<_> = operations
            .iter()
            .filter(|op| {
                ["definition.revise", "agent.create", "agent.revise"]
                    .contains(&op.method().as_str())
            })
            .collect();
        let global_ids: HashSet<_> = global_ops.iter().map(|op| op.id.as_str()).collect();
        let global_events: Vec<_> = source
            .events
            .iter()
            .filter(|e| global_ids.contains(e.detail.strip_prefix("command_id=").unwrap_or("")))
            .cloned()
            .collect();
        let mut assigned_ops: HashSet<String> = global_ops.iter().map(|op| op.id.clone()).collect();
        let mut assigned_events: HashSet<u64> =
            global_events.iter().map(|event| event.id).collect();
        let mut assigned_rows: HashMap<String, usize> = HashMap::new();
        for project in &source.projects {
            let mut state = project_state(&source, &project.id);
            let ids = state_ids(&state)?;
            let owned_ops: Vec<_> = operations.iter().filter(|op| op.belongs(&ids)).collect();
            for op in &owned_ops {
                require(assigned_ops.insert(op.id.clone()), "迁移发现跨项目命令记录")?;
            }
            let command_ids: HashSet<_> = owned_ops.iter().map(|op| op.id.as_str()).collect();
            state.events = source
                .events
                .iter()
                .filter(|e| {
                    ids.contains(&e.target)
                        || command_ids.contains(e.detail.strip_prefix("command_id=").unwrap_or(""))
                })
                .cloned()
                .collect();
            for event in &state.events {
                require(assigned_events.insert(event.id), "迁移发现跨项目事件记录")?;
            }
            for (table, rows) in serde_json::to_value(&state)
                .map_err(storage)?
                .as_object()
                .unwrap()
            {
                *assigned_rows.entry(table.clone()).or_default() += rows.as_array().unwrap().len();
            }
            let store = SqliteStore::open_project(&self.root, &project.id)?;
            let before = store.snapshot()?;
            require(
                before.projects.is_empty()
                    || serde_json::to_value(&before.projects).map_err(storage)?
                        == serde_json::to_value(&state.projects).map_err(storage)?,
                "迁移目标已包含其他工程状态",
            )?;
            for artifact in &state.artifacts {
                let body = FileArtifacts::new(self.root.clone()).read(artifact)?;
                let relative = Path::new(&artifact.relative_path);
                let parts: Vec<_> = relative
                    .parent()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .map(|p| {
                        p.to_str()
                            .ok_or_else(|| err("storage", "产物路径不是 UTF-8"))
                    })
                    .collect::<Result<_>>()?;
                artifact_directory(&PathBuf::from(store.root()), &parts)?;
                let dest = PathBuf::from(store.root()).join(relative);
                reject_symlink(&dest)?;
                fs::write(dest, body).map_err(storage)?;
            }
            let mut conn = store.connection.lock().map_err(storage)?;
            let tx = conn.transaction().map_err(storage)?;
            SqliteStore::save(&tx, &state, &before)?;
            for op in &owned_ops {
                op.save(&tx)?;
            }
            tx.commit().map_err(storage)?;
            let creator = owned_ops.iter().find(|op| op.method() == "project.create");
            let (command, fingerprint) = creator
                .map(|op| (op.id.clone(), op.fingerprint.clone()))
                .unwrap_or_else(|| (format!("legacy:{}", project.id), String::new()));
            catalog.push((project, command, fingerprint));
        }
        require(
            assigned_ops.len() == operations.len(),
            "迁移存在无法确定归属的命令记录；旧库保持不变",
        )?;
        require(
            assigned_events.len() == source.events.len(),
            "迁移存在无法确定归属的事件记录；旧库保持不变",
        )?;
        for (table, rows) in serde_json::to_value(&source)
            .map_err(storage)?
            .as_object()
            .unwrap()
        {
            if ![
                "permissions",
                "definitions",
                "harnesses",
                "sandboxes",
                "events",
            ]
            .contains(&table.as_str())
            {
                require(
                    assigned_rows.get(table).copied().unwrap_or(0)
                        == rows.as_array().unwrap().len(),
                    &format!("迁移存在无法确定归属的 {table} 记录；旧库保持不变"),
                )?;
            }
        }
        let runtime = self.root.join("runtime.json");
        let concurrency = if runtime.exists() {
            let v: Value = serde_json::from_str(&fs::read_to_string(runtime).map_err(storage)?)
                .map_err(storage)?;
            let n = v["global_concurrency"]
                .as_u64()
                .ok_or_else(|| err("storage", "旧并发配置无效"))?;
            require((1..=32).contains(&n), "旧并发配置无效")?;
            n
        } else {
            4
        };
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        source.events = global_events;
        Self::save_configuration(&tx, &source, &EngineeringState::default())?;
        for op in global_ops {
            op.save(&tx)?;
        }
        for (project, command, fingerprint) in catalog {
            tx.execute("INSERT INTO project_catalog(id,command_id,fingerprint,project) VALUES(?1,?2,?3,?4)", params![project.id,command,fingerprint,serde_json::to_string(project).map_err(storage)?]).map_err(storage)?;
        }
        put_setting(&tx, "global_concurrency", &json!(concurrency))?;
        if let Some(project) = source.projects.first() {
            put_setting(&tx, "active_project", &json!(project.id))?;
        }
        // Publish only once every project, its ledger and its artifacts have been committed.
        put_setting(&tx, "layout_version", &json!(2))?;
        tx.commit().map_err(storage)?;
        drop(conn);
        Ok(())
    }
}

struct MigrationCopy(PathBuf);
impl Drop for MigrationCopy {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

impl EngineeringStore for GlobalStore {
    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    fn snapshot(&self) -> Result<EngineeringState> {
        Self::configuration(&*self.connection.lock().map_err(storage)?)
    }
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        let _writer = self.writer.lock().map_err(storage)?;
        let conn = self.connection.lock().map_err(storage)?;
        if let Some((id, fp)) = key {
            let previous: Option<(String, String)> = conn
                .query_row(
                    "SELECT fingerprint,result FROM operations WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(storage)?;
            if let Some((saved, result)) = previous {
                require(saved == fp, "command_id 被不同参数复用")?;
                return serde_json::from_str(&result).map_err(storage);
            }
            require(
                !conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE command_id=?1)",
                        [id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(storage)?,
                "command_id 被不同参数复用",
            )?;
        }
        let mut state = Self::configuration(&conn)?;
        let before = state.clone();
        drop(conn);
        let result = action(&mut state)?;
        state.validate_conversations(&before)?;
        validate_permission_revisions(&state.permissions, &before.permissions)?;
        let value = serde_json::to_value(&state).map_err(storage)?;
        require(
            value.as_object().unwrap().iter().all(|(key, items)| {
                [
                    "permissions",
                    "definitions",
                    "harnesses",
                    "sandboxes",
                    "events",
                ]
                .contains(&key.as_str())
                    || items.as_array().is_some_and(Vec::is_empty)
            }),
            "全局配置库不能写入工程数据",
        )?;
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        Self::save_configuration(&tx, &state, &before)?;
        if let Some((id, fp)) = key {
            tx.execute(
                "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
                params![id, fp, result.to_string()],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}

fn setting(conn: &Connection, key: &str) -> Result<Option<Value>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT CAST(value AS TEXT) FROM settings WHERE key=?1",
            [key],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    value
        .map(|v| serde_json::from_str(&v).map_err(storage))
        .transpose()
}
fn put_setting(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()]).map_err(storage)?;
    Ok(())
}
struct Operation {
    id: String,
    fingerprint: String,
    result: String,
}
impl Operation {
    fn parsed(&self) -> Value {
        serde_json::from_str(&self.fingerprint).unwrap_or(Value::Null)
    }
    fn method(&self) -> String {
        self.parsed()[0].as_str().unwrap_or("").to_owned()
    }
    fn belongs(&self, ids: &HashSet<String>) -> bool {
        if self.method() == "definition.revise" {
            return false;
        }
        let fp = self.parsed();
        let result: Value = serde_json::from_str(&self.result).unwrap_or(Value::Null);
        [
            &fp[1],
            &fp[3]["project_id"],
            &result["id"],
            &result["operation_id"],
        ]
        .iter()
        .any(|v| v.as_str().is_some_and(|id| ids.contains(id)))
    }
    fn save(&self, conn: &Connection) -> Result<()> {
        conn.execute("INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3) ON CONFLICT(id) DO NOTHING",params![self.id,self.fingerprint,self.result]).map_err(storage)?;
        Ok(())
    }
}
fn state_ids(state: &EngineeringState) -> Result<HashSet<String>> {
    let value = serde_json::to_value(state).map_err(storage)?;
    Ok(value
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| {
            ![
                "permissions",
                "definitions",
                "harnesses",
                "sandboxes",
                "events",
            ]
            .contains(&k.as_str())
        })
        .flat_map(|(_, v)| v.as_array().unwrap())
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect())
}
fn project_state(source: &EngineeringState, pid: &str) -> EngineeringState {
    let mut s = source.clone();
    s.events.clear();
    s.projects.retain(|p| p.id == pid);
    s.repository_operations.retain(|op| op.project_id == pid);
    s.repository_worktrees.retain(|w| w.project_id == pid);
    s.features.retain(|f| f.project_id == pid);
    let features: HashSet<_> = s.features.iter().map(|f| f.id.clone()).collect();
    s.tasks.retain(|t| features.contains(&t.feature_id));
    s.documents.retain(|d| {
        d.project_id.as_deref() == Some(pid)
            || d.feature_id.as_ref().is_some_and(|f| features.contains(f))
    });
    s.attempts.retain(|a| features.contains(&a.feature_id));
    s.merges.retain(|m| features.contains(&m.feature_id));
    let owners: HashSet<_> = s
        .attempts
        .iter()
        .map(|a| a.id.clone())
        .chain(s.merges.iter().map(|m| m.id.clone()))
        .chain(s.tasks.iter().map(|t| t.id.clone()))
        .chain(features.iter().cloned())
        .chain(std::iter::once(pid.to_string()))
        .collect();
    s.runs.retain(|r| {
        r.attempt_id
            .as_ref()
            .or(r.merge_id.as_ref())
            .is_some_and(|id| owners.contains(id))
    });
    let runs: HashSet<_> = s.runs.iter().map(|r| r.id.clone()).collect();
    s.sessions.retain(|s| {
        s.project_id.as_deref() == Some(pid)
            || (s.project_id.is_none() && owners.contains(&s.owner_id))
    });
    let sessions: HashSet<_> = s.sessions.iter().map(|s| s.id.clone()).collect();
    s.turns.retain(|t| t.project_id == pid);
    let turns: HashSet<_> = s.turns.iter().map(|t| t.id.clone()).collect();
    s.conversation_events.retain(|e| turns.contains(&e.turn_id));
    s.interactions.retain(|i| i.project_id == pid);
    s.proposals.retain(|p| p.project_id == pid);
    s.executions.retain(|e| sessions.contains(&e.session_id));
    s.contexts.retain(|c| owners.contains(&c.owner_id));
    s.snapshots.retain(|s| {
        s.project_id.as_deref() == Some(pid)
            || s.feature_id.as_ref().is_some_and(|f| features.contains(f))
    });
    s.deliveries.retain(|d| features.contains(&d.feature_id));
    let deliveries: HashSet<_> = s.deliveries.iter().map(|d| d.id.clone()).collect();
    s.reviews.retain(|r| deliveries.contains(&r.delivery_id));
    s.overrides.retain(|o| deliveries.contains(&o.delivery_id));
    s.heads.retain(|h| h.project_id == pid);
    s.resources.retain(|r| runs.contains(&r.run_id));
    s.artifacts.retain(|a| {
        a.project_id.as_deref() == Some(pid)
            || a.feature_id.as_ref().is_some_and(|f| features.contains(f))
    });
    s.effects.retain(|e| runs.contains(&e.run_id));
    s.advances.retain(|a| s.merges.iter().any(|m| m.id == a.id));
    s
}
