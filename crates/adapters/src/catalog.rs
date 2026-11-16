//! Global configuration for the current split-database layout.
use super::*;
use rusqlite::{OptionalExtension, params};

pub struct GlobalStore {
    pub(super) connection: connection_pool::ConnectionPool,
    pub(super) writer: Mutex<()>,
    catalog_cache: configuration_cache::CatalogCache,
    changes: store_changes::StoreChanges,
    root: PathBuf,
    _lock: File,
}

impl GlobalStore {
    pub fn read_configuration(
        data_root: &Path,
    ) -> Result<std::sync::Arc<ConfigurationCatalogSnapshot>> {
        configuration_cache::read_only(&data_root.join("global.sqlite3"), false)
    }
    pub fn model_credential_reference(&self, provider: &ProviderFacts) -> Result<Option<String>> {
        Ok(setting(
            &*self.connection.read()?,
            &format!("credentials.dsh.{}", provider.id),
        )?
        .and_then(|v| v.as_str().map(str::to_owned)))
    }
    pub fn open(path: &Path) -> Result<Self> {
        factory_bundle::initialize()?;
        let (root, lock) = lock_dataset(path)?;
        require(
            !root.join("engineering.sqlite3").exists(),
            "旧单库数据集已不再支持，请使用当前分库布局",
        )?;
        let connection = connection_pool::ManagedConnection::open(&root.join("global.sqlite3"))?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        require(
            version == 0 || version == 21,
            "数据集协议不兼容；请使用新的空数据目录",
        )?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;",
            )
            .map_err(storage)?;
        if version == 0 {
            connection
                .execute_batch(include_str!("../../../sql/global/schema.sql"))
                .map_err(storage)?;
        }

        let store = Self {
            connection: connection_pool::ConnectionPool::new(
                &root.join("global.sqlite3"),
                connection,
            )?,
            writer: Mutex::new(()),
            catalog_cache: Default::default(),
            changes: Default::default(),
            root,
            _lock: lock,
        };
        Ok(store)
    }

    pub fn concurrency(&self) -> Result<usize> {
        let conn = self.connection.read().map_err(storage)?;
        Ok(setting(&conn, "global_concurrency")?
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize)
    }

    pub fn set_concurrency(&self, value: usize) -> Result<()> {
        self.update_preferences(Some(value), None)
    }

    pub fn default_harness(&self) -> Result<HarnessProvider> {
        let value = setting(
            &*self.connection.read().map_err(storage)?,
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
        // Trusted bootstrap/test convenience. RPC saves use the client's original
        // versions directly and must never refresh their baseline through here.
        let settings = devdesk_core::settings::read(self)?;
        let mut payload = serde_json::Map::new();
        if let Some(n) = concurrency {
            payload.insert("global_concurrency".into(), json!(n));
        }
        if let Some(p) = provider {
            payload.insert("default_harness".into(), json!(p));
        }
        let versions = settings
            .versions
            .into_iter()
            .filter(|version| payload.contains_key(&version.resource.id))
            .collect();
        devdesk_core::settings::save(
            self,
            &Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: Value::Object(payload),
            },
            &concurrency::ReadSet::new(versions)?,
            &InvocationContext::human(None),
        )?;
        Ok(())
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>> {
        let conn = self.connection.read().map_err(storage)?;
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
        let conn = self.connection.read().map_err(storage)?;
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
    pub fn reserve_project(
        &self,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<String> {
        devdesk_core::project_reservation::reserve(self, command, context)
    }

    /// This is a directory summary; the project database remains authoritative.
    pub fn publish_project(&self, project: &Project) -> Result<()> {
        let _span = performance::Span::new("directory.publish");
        if let Some(known) = self.project(&project.id)? {
            if known.revision > project.revision {
                return Ok(());
            }
            if known.revision == project.revision {
                return require(
                    json!(known) == json!(project),
                    "同一项目修订不能发布不同目录摘要",
                );
            }
        }
        let conn = self.connection.lock().map_err(storage)?;
        let updated = conn
            .execute(
                "UPDATE project_catalog SET project=?1 WHERE id=?2 AND (project IS NULL OR json_extract(project,'$.revision')<?3)",
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
        )?;
        drop(conn);
        if updated == 1 {
            self.changes.notify();
        } else if let Some(current) = self.project(&project.id)? {
            require(
                current.revision > project.revision || json!(current) == json!(project),
                "同一项目修订不能发布不同目录摘要",
            )?;
        }
        Ok(())
    }

    fn configuration(conn: &Connection) -> Result<EngineeringState> {
        let _span = performance::Span::new("global.full_snapshot");
        let mut state = EngineeringState {
            governance: source_store::load_records(conn)?,
            events: load_table(conn, "events")?,
            ..Default::default()
        };
        state.refresh_configuration_views()?;
        source_store::restore_records(&mut state, &mut Default::default())?;
        Ok(state)
    }

    fn save_configuration(
        conn: &Connection,
        s: &EngineeringState,
        before: &EngineeringState,
    ) -> Result<()> {
        source_store::save_records(conn, s, before)?;
        append_events(conn, &s.events, &before.events)
    }
}

impl EngineeringStore for GlobalStore {
    fn replay_commit(
        &self,
        command_id: &str,
        fingerprint: &str,
    ) -> Result<Option<concurrency::CommitReceipt>> {
        resource_commits::replay(&self.connection, command_id, fingerprint)
    }
    fn namespace(&self) -> concurrency::StoreNamespace {
        concurrency::StoreNamespace::Configuration
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
    fn observe_query(&self, stage: &'static str) -> Box<dyn ports::QueryObservation> {
        performance::observe(stage)
    }
    fn active_conversation_count(&self) -> Result<usize> {
        Ok(0)
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
    fn catalog_revision(&self) -> Result<u64> {
        configuration_cache::revision(&*self.connection.read()?)
    }
    fn configuration_snapshot(&self) -> Result<std::sync::Arc<ConfigurationCatalogSnapshot>> {
        self.catalog_cache.read(&*self.connection.read()?, false)
    }
    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    fn snapshot(&self) -> Result<EngineeringState> {
        let waiting = performance::Span::new("global.connection_wait");
        let conn = self.connection.read()?;
        drop(waiting);
        conn.execute_batch("BEGIN;").map_err(storage)?;
        let state = Self::configuration(&conn)?;
        conn.execute_batch("COMMIT;").map_err(storage)?;
        Ok(state)
    }
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        let _span = performance::Span::new("global.transaction");
        let _writer = self.writer.lock().map_err(storage)?;
        let conn = self.connection.lock().map_err(storage)?;
        if let Some((id, fp)) = key {
            let previous: Option<(String, String)> = conn
                .query_row(
                    "SELECT fingerprint,CAST(result AS TEXT) FROM operations WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(storage)?;
            if let Some((saved, result)) = previous {
                require(saved == fp, "command_id 被不同参数复用")?;
                return source_views::restore(
                    &Self::configuration(&conn)?,
                    &serde_json::from_str(&result).map_err(storage)?,
                );
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
        let changed = serde_json::to_value(&before).map_err(storage)? != value;
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
                params![id, fp, source_views::pack(&state, &result)?.to_string()],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        if catalog_changed(&before, &state) {
            self.catalog_cache.invalidate()?;
        }
        drop(conn);
        drop(_writer);
        if changed {
            self.changes.notify();
        }
        Ok(result)
    }
}

pub(crate) fn setting(conn: &Connection, key: &str) -> Result<Option<Value>> {
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
pub(crate) fn put_setting(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()]).map_err(storage)?;
    Ok(())
}
