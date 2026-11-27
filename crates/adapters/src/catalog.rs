//! Global configuration for the current split-database layout.
use super::*;
use rusqlite::{OptionalExtension, params};

const DOMAIN_DATASET_LAYOUT: &str = "single-host-domain-packages-v1";
pub(super) const REMOVED_PROJECT_PREFIX: &str = "project.removed:";

pub struct GlobalStore {
    pub(super) connection: connection_pool::ConnectionPool,
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
            version == 0 || version == 24,
            "数据集协议不兼容；请使用新的空数据目录",
        )?;
        if version == 24 {
            let layout = connection.query_row(
                "SELECT contract FROM dataset_layout WHERE id=1",
                [],
                |row| row.get::<_, String>(0),
            );
            if !layout.is_ok_and(|value| value == DOMAIN_DATASET_LAYOUT) {
                return Err(err(
                    "incompatible_dataset",
                    "数据集资源协议不兼容。领域平台需要新的数据目录，原数据保持原样",
                ));
            }
            storage_maintenance::checkpoint(&connection)?;
            preflight_engineering_databases(&root, &connection)?;
        }
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
            .map(|limit| limit as usize)
            .unwrap_or(devdesk_core::settings::DEFAULT_GLOBAL_CONCURRENCY))
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
        self.project_rows(offset, limit, false, None, true)
    }
    pub fn github_projects(&self, offset: usize, limit: usize) -> Result<(Vec<Project>, usize)> {
        self.projects_visible(offset, limit, true, None)
    }
    pub fn projects_visible(
        &self,
        offset: usize,
        limit: usize,
        github_only: bool,
        visibility: Option<&domain_packages::ProjectVisibility>,
    ) -> Result<(Vec<Project>, usize)> {
        self.project_rows(offset, limit, github_only, visibility, false)
    }
    fn project_rows(
        &self,
        offset: usize,
        limit: usize,
        github_only: bool,
        visibility: Option<&domain_packages::ProjectVisibility>,
        include_removed: bool,
    ) -> Result<(Vec<Project>, usize)> {
        let conn = self.connection.read().map_err(storage)?;
        let filter = if github_only {
            "project IS NOT NULL AND json_extract(project,'$.execution_mode')='live' AND coalesce(json_extract(project,'$.workspace_definition.repository'),'') <> ''"
        } else {
            "project IS NOT NULL"
        };
        let filter = if include_removed {
            filter.to_owned()
        } else {
            format!(
                "{filter} AND NOT EXISTS(SELECT 1 FROM settings
                 WHERE key='{REMOVED_PROJECT_PREFIX}'||project_catalog.id AND value='true')"
            )
        };
        let (filter, projects) = match visibility {
            Some(visibility)
                if !visibility.projects.is_empty() || visibility.active_digest.is_some() =>
            {
                let operator = if visibility.active_digest.is_some() {
                    "IN"
                } else {
                    "NOT IN"
                };
                (
                    format!("{filter} AND id {operator} (SELECT value FROM json_each(?1))"),
                    Some(serde_json::to_string(&visibility.projects).map_err(storage)?),
                )
            }
            _ => (format!("{filter} AND ?1 IS NULL"), None),
        };
        let count = conn
            .query_row(
                &format!("SELECT count(*) FROM project_catalog WHERE {filter}"),
                [&projects],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let mut q = conn.prepare(&format!("SELECT project FROM project_catalog WHERE {filter} ORDER BY rowid LIMIT ?2 OFFSET ?3")).map_err(storage)?;
        let bodies = q
            .query_map(
                params![projects, limit.min(100) as i64, offset as i64],
                |r| r.get::<_, String>(0),
            )
            .map_err(storage)?;
        let mut projects = vec![];
        for body in bodies {
            projects.push(serde_json::from_str(&body.map_err(storage)?).map_err(storage)?);
        }
        Ok((projects, count))
    }

    pub fn project_removal_key(project: &str) -> devdesk_core::concurrency::ResourceKey {
        devdesk_core::concurrency::ResourceKey::new(
            "setting",
            format!("{REMOVED_PROJECT_PREFIX}{project}"),
        )
    }

    pub fn project_removed(&self, project: &str) -> Result<bool> {
        Ok(setting(
            &*self.connection.read()?,
            &Self::project_removal_key(project).id,
        )? == Some(Value::Bool(true)))
    }

    pub fn removed_project_missing(&self, project: &str) -> Result<bool> {
        self.removed_project_missing_on(&*self.connection.read()?, project)
    }

    fn removed_project_missing_on(&self, conn: &Connection, project: &str) -> Result<bool> {
        if setting(conn, &Self::project_removal_key(project).id)? != Some(json!(true)) {
            return Ok(false);
        }
        match SqliteStore::project_path(&self.root, project) {
            Ok(path) => path
                .join("engineering.sqlite3")
                .try_exists()
                .map(|exists| !exists)
                .map_err(storage),
            Err(error) if error.code == domain_packages::DOMAIN_STORAGE_UNAVAILABLE => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// Reopening can find a retained registration even when it is hidden.
    pub fn domain_github_project(
        &self,
        repository: &str,
        mainline: &str,
        visibility: &domain_packages::ProjectVisibility,
    ) -> Result<Option<Project>> {
        let conn = self.connection.read()?;
        let mut query = conn
            .prepare_cached(
                "SELECT id,project FROM project_catalog WHERE project IS NOT NULL
             AND json_extract(project,'$.workspace_definition.repository') = ?1 COLLATE NOCASE
             AND json_extract(project,'$.workspace_definition.mainline') = ?2
             ORDER BY EXISTS(SELECT 1 FROM settings
                WHERE key='project.removed:'||project_catalog.id AND value='true'), rowid",
            )
            .map_err(storage)?;
        let rows = query
            .query_map(params![repository, mainline], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        let mut unidentified = None;
        for row in rows {
            let (id, body) = row.map_err(storage)?;
            let visible = visibility.includes(&id);
            if !visible && !visibility.unidentified_projects.contains(&id) {
                continue;
            }
            if self.removed_project_missing_on(&conn, &id)? {
                continue;
            }
            if visible {
                return serde_json::from_str(&body).map(Some).map_err(storage);
            }
            unidentified = Some(id);
        }
        if let Some(id) = unidentified {
            return Err(err(
                "domain_package_required",
                format!("已有项目 {id} 的领域身份缺失，请导入该项目原来的领域 ZIP 以恢复登记"),
            ));
        }
        Ok(None)
    }

    pub fn unregistered_projects(&self, data_root: &Path) -> Result<Vec<Id>> {
        let projects = data_root.join("projects");
        reject_symlink(&projects)?;
        let entries = match fs::read_dir(&projects) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(storage(error)),
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(storage)?;
            let name = entry.file_name();
            let Some(id) = name.to_str().filter(|id| uuid::Uuid::parse_str(id).is_ok()) else {
                continue;
            };
            if entry.file_type().map_err(storage)?.is_dir() && self.project(id)?.is_none() {
                candidates.push(id.to_owned());
            }
        }
        candidates.sort_unstable();
        Ok(candidates)
    }

    pub fn copied_project(data_root: &Path, id: &str) -> Result<Option<Project>> {
        let Some((project, path)) = read_project_copy(data_root, id)? else {
            return Ok(None);
        };
        validate_project_copy(&project, &path)?;
        Ok(Some(project))
    }

    /// Find an unpublished project by the repository identity recorded in its database.
    pub fn copied_github_project(
        &self,
        data_root: &Path,
        repository: &str,
        mainline: &str,
    ) -> Result<Option<Project>> {
        let mut found: Option<Project> = None;
        for id in self.unregistered_projects(data_root)? {
            let Some((project, path)) = read_project_copy(data_root, &id)? else {
                continue;
            };
            let Some(workspace) = &project.workspace_definition else {
                continue;
            };
            if !workspace.repository.eq_ignore_ascii_case(repository)
                || workspace.mainline != mainline
            {
                continue;
            }
            validate_project_copy(&project, &path)?;
            if let Some(previous) = &found {
                return Err(err(
                    "project_ambiguous",
                    format!(
                        "同一仓库和主分支有多个未登记项目（{}、{id}），请只保留要接回的项目目录",
                        previous.id
                    ),
                ));
            }
            found = Some(project);
        }
        Ok(found)
    }

    /// Reserve the old ID before loading the database. Publication happens only
    /// after it opens successfully, and an interrupted import can retry.
    pub fn reserve_copied_project(&self, project: &Project) -> Result<()> {
        let command = format!("copied-project:{}", project.id);
        let conn = self.connection.lock().map_err(storage)?;
        conn.execute(
            "INSERT INTO project_catalog(id,command_id,fingerprint)
             VALUES(?1,?2,?1) ON CONFLICT(id) DO NOTHING",
            params![project.id, command],
        )
        .map_err(storage)?;
        let owned: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE id=?1 AND command_id=?2)",
                params![project.id, command],
                |row| row.get(0),
            )
            .map_err(storage)?;
        require(owned, "旧项目 ID 已被占用")
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

    #[cfg(any(test, feature = "test-fixtures"))]
    fn save_configuration(
        conn: &Connection,
        s: &EngineeringState,
        before: &EngineeringState,
    ) -> Result<()> {
        source_store::save_records(conn, s, before)?;
        append_events(conn, &s.events, &before.events)
    }
}

// Check compatibility before bootstrap, recovery or journal mode changes can
// write global facts. Incomplete reservations may have no database yet; a
// published project may not silently become a newly initialized empty project.
fn preflight_engineering_databases(root: &Path, global: &Connection) -> Result<()> {
    use devdesk_core::template_reset_jobs::{ACTIVE_SETTING, TemplateResetJob};
    let reset = setting(global, ACTIVE_SETTING)?
        .as_ref()
        .and_then(Value::as_str)
        .map(|key| setting(global, key))
        .transpose()?
        .flatten()
        .map(serde_json::from_value::<TemplateResetJob>)
        .transpose()
        .map_err(storage)?;
    let pending = |target: &str| {
        reset.as_ref().is_some_and(|job| {
            job.targets
                .iter()
                .any(|item| item.id == target && item.applied_at.is_none())
        })
    };
    let mut query = global
        .prepare("SELECT id,project IS NOT NULL FROM project_catalog")
        .map_err(storage)?;
    let projects = query
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))
        .map_err(storage)?;
    for row in projects {
        let (id, published) = row.map_err(storage)?;
        let path = match SqliteStore::project_path(root, &id) {
            Ok(path) => path.join("engineering.sqlite3"),
            Err(error) if error.code == domain_packages::DOMAIN_STORAGE_UNAVAILABLE => continue,
            Err(error) => return Err(error),
        };
        let status = if path.exists() {
            check_engineering_baseline(&path)
        } else {
            require(
                !published
                    || setting(global, &GlobalStore::project_removal_key(&id).id)?
                        == Some(json!(true)),
                "已发布项目的数据库缺失；拒绝重建",
            )
        };
        if !pending(&id) {
            status?;
        }
    }
    let path = SqliteStore::global_discussion_path(root)?.join("engineering.sqlite3");
    if path.exists() {
        reject_symlink(&path)?;
        let status = check_engineering_baseline(&path);
        if !pending(GLOBAL_DISCUSSION_NAMESPACE) {
            status?;
        }
    }
    Ok(())
}
fn read_project_copy(data_root: &Path, id: &str) -> Result<Option<(Project, PathBuf)>> {
    require(uuid::Uuid::parse_str(id).is_ok(), "无效项目 ID")?;
    let path = SqliteStore::project_path_in(data_root, id)?;
    let db = path.join("engineering.sqlite3");
    if !db.is_file() {
        return Ok(None);
    }
    let connection = connection_pool::ManagedConnection::read_only(&db)?;
    match execution_read::project(&connection, id) {
        Ok(project) => Ok(Some((project, path))),
        Err(error) if error.code == "not_found" => Ok(None),
        Err(error) => Err(error),
    }
}

fn validate_project_copy(project: &Project, path: &Path) -> Result<()> {
    check_engineering_baseline(&path.join("engineering.sqlite3"))?;
    if let Some(workspace) = &project.workspace_definition
        && !(workspace.repository.is_empty()
            && workspace.path.is_empty()
            && project.workspace.is_empty())
    {
        let checkout = path.join("repository");
        reject_symlink(&checkout)?;
        if project.execution_mode != "live"
            || Path::new(&workspace.path) != checkout
            || Path::new(&project.workspace) != checkout
            || !checkout.is_dir()
        {
            return Err(err(
                "incompatible_dataset",
                format!(
                    "旧项目 {} 的本地仓库路径不匹配或不存在，请将完整项目目录复制到原路径",
                    project.id
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn check_engineering_baseline(path: &Path) -> Result<()> {
    let connection = connection_pool::ManagedConnection::read_only(path)?;
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    require(
        matches!(
            version,
            LEGACY_PROJECT_SCHEMA_VERSION
                | PAYLOAD_PROJECT_SCHEMA_VERSION
                | SOURCE_PROJECT_SCHEMA_VERSION
                | PROJECT_SCHEMA_VERSION
        ),
        "数据集协议不兼容；请使用新的空数据目录",
    )?;
    check_dataset_layout(&connection)
}

impl EngineeringStore for GlobalStore {
    fn operational_read(&self, method: &str, params: &Value) -> Result<Value> {
        operational_reads::read(
            &self.connection,
            self.configuration_snapshot()?.as_ref(),
            None,
            method,
            params,
        )
    }
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
    fn platform_calls_pending(&self) -> Result<bool> {
        Ok(false)
    }
    fn runtime_record_page(
        &self,
        kind: &str,
        status: &str,
        after: Option<&str>,
    ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
        let conn = self.connection.read()?;
        let mut q=conn.prepare_cached("SELECT id FROM governance_records INDEXED BY idx_runtime_work_seek WHERE type=?1 AND json_extract(data,'$.status')=?2 AND id>?3 ORDER BY id LIMIT 17").map_err(storage)?;
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
    fn restore_runtime_record(&self, value: &Value) -> Result<Value> {
        tool_calls::restore(self.configuration_snapshot()?.as_ref(), value)
    }
    fn workflow_tool_facts(
        &self,
        _: &devdesk_core::tool_calls::WorkflowToolAuthority,
        _: &str,
    ) -> Result<devdesk_core::tool_calls::WorkflowToolFacts> {
        Err(err("invalid_scope", "工作流工具不能写入全局配置库"))
    }
    fn catalog_revision(&self) -> Result<u64> {
        configuration_cache::revision(&*self.connection.read()?)
    }
    fn prepare_configuration_reset(
        &self,
        records: &[ConfigurationRecord],
        revision: u64,
    ) -> Result<ConfigurationPublication> {
        configuration_reset::prepare(&self.connection, false, records, revision)
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
    #[cfg(feature = "test-fixtures")]
    fn seed_fixture_state(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        let _span = performance::Span::new("global.transaction");
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
                require(
                    project_storage::fingerprint_matches(&saved, fp),
                    "command_id 被不同参数复用",
                )?;
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
