//! Global configuration for the current split-database layout.
use super::*;
use rusqlite::{OptionalExtension, params};

pub struct GlobalStore {
    pub(super) connection: Mutex<Connection>,
    pub(super) writer: Mutex<()>,
    root: PathBuf,
    _lock: File,
}

impl GlobalStore {
    pub fn model_credential_reference(&self, provider: DshModelProvider) -> Result<Option<String>> {
        Ok(setting(
            &*self.connection.lock().map_err(storage)?,
            &format!("credentials.dsh.{}", provider.as_str()),
        )?
        .and_then(|v| v.as_str().map(str::to_owned)))
    }
    pub fn set_model_credential_reference(
        &self,
        provider: DshModelProvider,
        reference: &str,
    ) -> Result<()> {
        require(provider.requires_key(), "Ollama 本地连接无需凭据")?;
        put_setting(
            &*self.connection.lock().map_err(storage)?,
            &format!("credentials.dsh.{}", provider.as_str()),
            &json!(reference),
        )
    }
    pub fn open(path: &Path) -> Result<Self> {
        let (root, lock) = lock_dataset(path)?;
        require(
            !root.join("engineering.sqlite3").exists(),
            "旧单库数据集已不再支持，请使用当前分库布局",
        )?;
        let connection = Connection::open(root.join("global.sqlite3")).map_err(storage)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;",
            )
            .map_err(storage)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        require(
            version == 0 || version == 10 || version == 11,
            "数据集不是当前全局结构，请先完成离线迁移",
        )?;
        if version == 0 {
            connection
                .execute_batch(include_str!("../../../sql/global/schema.sql"))
                .map_err(storage)?;
        }
        if version < 11 {
            connection
                .execute_batch(include_str!("../../../sql/global/011_template_catalog.sql"))
                .map_err(storage)?;
        }
        let store = Self {
            connection: Mutex::new(connection),
            writer: Mutex::new(()),
            root,
            _lock: lock,
        };
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
            "project IS NOT NULL AND json_extract(project,'$.execution_mode')='live' AND coalesce(json_extract(project,'$.workspace_definition.repository'),'') <> ''"
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
            governance: load_table(conn, "governance_records")?,
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
        save_table(
            conn,
            "governance_records",
            &s.governance,
            &before.governance,
        )?;
        save_table(conn, "definitions", &s.definitions, &before.definitions)?;
        save_table(conn, "harnesses", &s.harnesses, &before.harnesses)?;
        save_table(conn, "sandboxes", &s.sandboxes, &before.sandboxes)?;
        save_table(conn, "events", &s.events, &before.events)
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
        state.validate_governance(&before)?;
        state.validate_conversations(&before)?;
        let value = serde_json::to_value(&state).map_err(storage)?;
        require(
            value.as_object().unwrap().iter().all(|(key, items)| {
                [
                    "governance",
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
