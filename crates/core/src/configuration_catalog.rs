//! A disposable, configuration-only read model. Execution facts remain in EngineeringStore.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub const CATALOG_RECORD_KINDS: &[&str] = &[
    "project_configuration",
    "work_configuration",
    "integration_project_configuration",
    "workflow_draft",
    "agent_instance",
    "agent_instance_retirement",
    "skill_package",
    "role_retirement",
    "configuration_retirement",
    "template_catalog_state",
    "template_catalog_member",
    "standard_archive",
    "standard_derivation",
    "integration_discovery",
    "integration_disabled",
    "integration_operation",
    "harness_materialization",
    "permission_origin",
    "permission_file_origin",
    "permission_source_selection",
];
pub fn catalog_kind(kind: &str) -> bool {
    template_definition_kind(kind)
        || matches!(kind, "permission_binding" | "permission_assignment")
        || CATALOG_RECORD_KINDS.contains(&kind)
}
pub fn catalog_record(record: &ConfigurationRecord) -> bool {
    record.is_definition() || CATALOG_RECORD_KINDS.contains(&record.r#type.as_str())
}

/// An adapter may attach its memory reservation to the view's complete lifetime.
/// Old views can remain pinned by readers after eviction from the current cache.
pub trait SnapshotAllocation: Send + Sync {}

pub struct ConfigurationCatalogSnapshot {
    pub revision: u64,
    pub fingerprint: String,
    pub state: Arc<EngineeringState>,
    pub repository_ready: bool,
    references: BTreeMap<String, usize>,
    kinds: BTreeMap<String, Vec<usize>>,
    heads: BTreeMap<(String, String), u64>,
    origins: BTreeMap<String, Value>,
    current: BTreeSet<String>,
    all_heads: BTreeMap<(String, String), u64>,
    retired_identities: BTreeSet<String>,
    retired_sources: BTreeSet<(String, String)>,
    archived_standards: BTreeMap<String, bool>,
    health: std::sync::OnceLock<Result<Value>>,
    memo: std::sync::Mutex<crate::configuration_queries::QueryMemo>,
    _allocation: Option<Box<dyn SnapshotAllocation>>,
}
impl ConfigurationCatalogSnapshot {
    pub fn new(mut state: EngineeringState, revision: u64, repository_ready: bool) -> Result<Self> {
        state = catalog_projection(&state);
        state.refresh_configuration_views()?;
        let mut snapshot = Self {
            revision,
            fingerprint: configuration_hash(&(
                &state.governance,
                &state.projects,
                repository_ready,
            )),
            state: Arc::new(state),
            repository_ready,
            references: BTreeMap::new(),
            kinds: BTreeMap::new(),
            heads: BTreeMap::new(),
            origins: BTreeMap::new(),
            current: Default::default(),
            all_heads: Default::default(),
            retired_identities: Default::default(),
            retired_sources: Default::default(),
            archived_standards: Default::default(),
            health: Default::default(),
            memo: Default::default(),
            _allocation: None,
        };
        let mut members: BTreeMap<(&str, &str), &ConfigurationRecord> = BTreeMap::new();
        for (index, record) in snapshot.state.governance.iter().enumerate() {
            snapshot.references.insert(record.id.clone(), index);
            snapshot
                .kinds
                .entry(record.r#type.clone())
                .or_default()
                .push(index);
            let head = snapshot
                .all_heads
                .entry((record.r#type.clone(), record.definition_id.clone()))
                .or_default();
            *head = (*head).max(record.version);
            if record.r#type == "template_catalog_member" {
                let key = (
                    record.definition_id.as_str(),
                    record.data["kind"].as_str().unwrap_or(""),
                );
                if members.get(&key).is_none_or(|old| {
                    old.data["generation"].as_u64().unwrap_or(0)
                        <= record.data["generation"].as_u64().unwrap_or(0)
                }) {
                    members.insert(key, record);
                }
            }
            if matches!(
                record.r#type.as_str(),
                "role_retirement" | "agent_instance_retirement"
            ) {
                snapshot
                    .retired_identities
                    .insert(record.definition_id.clone());
            }
            if record.r#type == "standard_archive" {
                snapshot.archived_standards.insert(
                    record.definition_id.clone(),
                    record.data["archived"] == true,
                );
            }
        }
        snapshot.retired_sources = snapshot
            .records("configuration_retirement")
            .filter_map(|record| snapshot.record(&record.definition_id))
            .map(|source| (source.r#type.clone(), source.definition_id.clone()))
            .collect();
        let generation = template_catalog_generation(&snapshot.state);
        for record in &snapshot.state.governance {
            let member = members
                .get(&(record.id.as_str(), template_member_kind(&record.r#type)))
                .copied();
            let current = if template_definition_kind(&record.r#type) {
                member
                    .and_then(|m| m.data["generation"].as_u64())
                    .unwrap_or(TEMPLATE_INITIAL_GENERATION)
                    == generation
            } else {
                template_record_current(&snapshot.state, record)
            };
            if current {
                snapshot.current.insert(record.id.clone());
                let head = snapshot
                    .heads
                    .entry((record.r#type.clone(), record.definition_id.clone()))
                    .or_default();
                *head = (*head).max(record.version);
            }
            let origin = if !current {
                "archived"
            } else if let Some(member) = member.filter(|m| m.data["template_key"].is_string()) {
                if member.data["source_hash"] == template_source_hash(record) {
                    "default"
                } else {
                    "modified"
                }
            } else if member.is_some_and(|m| m.data["origin"] == "existing") {
                "existing"
            } else {
                "custom"
            };
            snapshot.origins.insert(
                record.id.clone(),
                json!({"origin":origin,"template_key":member.map(|m|&m.data["template_key"])}),
            );
        }
        Ok(snapshot)
    }
    pub fn with_allocation(mut self, allocation: impl SnapshotAllocation + 'static) -> Self {
        self._allocation = Some(Box::new(allocation));
        self
    }
    pub fn record(&self, reference: &str) -> Option<&ConfigurationRecord> {
        self.references
            .get(reference)
            .map(|index| &self.state.governance[*index])
    }
    pub fn records(&self, kind: &str) -> impl Iterator<Item = &ConfigurationRecord> {
        self.kinds
            .get(kind)
            .into_iter()
            .flatten()
            .map(|index| &self.state.governance[*index])
    }
    pub fn is_current_head(&self, record: &ConfigurationRecord) -> bool {
        self.heads
            .get(&(record.r#type.clone(), record.definition_id.clone()))
            .is_none_or(|version| record.version >= *version)
    }
    pub fn is_current(&self, record: &ConfigurationRecord) -> bool {
        self.current.contains(&record.id)
    }
    pub fn is_head(&self, record: &ConfigurationRecord) -> bool {
        self.all_heads
            .get(&(record.r#type.clone(), record.definition_id.clone()))
            .is_none_or(|version| record.version >= *version)
    }
    pub fn is_retired(&self, record: &ConfigurationRecord) -> bool {
        self.retired_identities.contains(&record.definition_id)
            || self
                .retired_sources
                .contains(&(record.r#type.clone(), record.definition_id.clone()))
    }
    pub fn is_archived(&self, record: &ConfigurationRecord) -> bool {
        !self.is_current(record)
            || (record.r#type == "standard"
                && self.archived_standards.get(&record.id) == Some(&true))
    }
    pub fn origin(&self, reference: &str) -> Option<&Value> {
        self.origins.get(reference)
    }
    pub fn cached_query(
        &self,
        key: String,
        build: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        if let Some(value) = self.memo.lock().unwrap().get(&key) {
            return Ok(value);
        }
        // Expensive work never holds the catalog or memo lock. Concurrent readers can use other keys.
        let value = build()?;
        self.memo.lock().unwrap().insert(key, &value);
        Ok(value)
    }
}

/// Configuration commits are compared without serializing or restoring execution history.
pub fn catalog_changed(before: &EngineeringState, after: &EngineeringState) -> bool {
    json!(before.projects) != json!(after.projects)
        || !before
            .governance
            .iter()
            .filter(|r| catalog_record(r))
            .eq(after.governance.iter().filter(|r| catalog_record(r)))
        || before
            .repository_worktrees
            .iter()
            .any(|w| w.kind == WorktreeKind::Mainline)
            != after
                .repository_worktrees
                .iter()
                .any(|w| w.kind == WorktreeKind::Mainline)
}

pub fn catalog_projection(state: &EngineeringState) -> EngineeringState {
    EngineeringState {
        governance: state
            .governance
            .iter()
            .filter(|r| catalog_record(r))
            .cloned()
            .collect(),
        projects: state.projects.clone(),
        ..Default::default()
    }
}

struct CatalogView {
    catalog: Arc<ConfigurationCatalogSnapshot>,
    root: String,
    project: Option<String>,
    source: Arc<dyn EngineeringStore>,
}
impl EngineeringStore for CatalogView {
    fn observe_query(&self, stage: &'static str) -> Box<dyn ports::QueryObservation> {
        self.source.observe_query(stage)
    }
    fn snapshot(&self) -> Result<EngineeringState> {
        Ok(self.catalog.state.as_ref().clone())
    }
    fn configuration_snapshot(&self) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        Ok(self.catalog.clone())
    }
    fn root(&self) -> String {
        self.root.clone()
    }
    fn project_id(&self) -> Option<String> {
        self.project.clone()
    }
}
impl Engine {
    pub fn configuration_status_at(
        &self,
        catalog: Arc<ConfigurationCatalogSnapshot>,
    ) -> Result<Value> {
        catalog
            .health
            .get_or_init(|| {
                self.at_configuration(catalog.clone())
                    .project_configuration_status()
            })
            .clone()
    }
    /// Reuse one pinned configuration view across existing pure query implementations.
    pub fn at_configuration(&self, catalog: Arc<ConfigurationCatalogSnapshot>) -> Self {
        Self {
            runtime_callback_cursors: std::sync::Mutex::new(Default::default()),
            experience_cursor: std::sync::Mutex::new(None),
            workflow_intent_cursor: std::sync::Mutex::new(None),
            provider_tool_cursor: std::sync::Mutex::new(None),
            provider_poll_cursor: std::sync::Mutex::new(None),
            conversation_capture_cursor: std::sync::Mutex::new(None),
            conversation_active_cursor: std::sync::Mutex::new(None),
            conversation_queued_cursor: std::sync::Mutex::new(None),
            context_call_cursor: std::sync::Mutex::new(None),
            tool_cursor: std::sync::Mutex::new(None),
            store: Arc::new(CatalogView {
                catalog,
                root: self.store.root(),
                project: self.store.project_id(),
                source: self.store.clone(),
            }),
            context_reader: self.context_reader.clone(),
            manager_services: self.manager_services.clone(),
            configuration_backend: self.configuration_backend.clone(),
            tool_executor: self.tool_executor.clone(),
            artifacts: self.artifacts.clone(),
            harness: self.harness.clone(),
            git: self.git.clone(),
            tests: self.tests.clone(),
            pull_requests: self.pull_requests.clone(),
        }
    }
}

pub fn compose_configuration_catalog(
    global: &ConfigurationCatalogSnapshot,
    local: &ConfigurationCatalogSnapshot,
    project: &str,
) -> Result<ConfigurationCatalogSnapshot> {
    let mut state = local.state.as_ref().clone();
    if state.projects.iter().all(|p| p.execution_mode == "live") {
        for record in global.state.governance.iter().filter(|r| {
            r.immutable
                && (r.r#type == "role"
                    || r.r#type == "role_retirement"
                    || configuration_visible(&global.state, r, Some(project)))
        }) {
            if let Some(old) = local.record(&record.id) {
                require(old.content_hash == record.content_hash, "共享配置摘要冲突")?;
            } else {
                state.governance.push(record.clone());
            }
        }
        copy_template_catalog(&global.state, &mut state)?;
        // Shared mutable drafts are read from their owning store, never published by a reader.
        for draft in global
            .records("workflow_draft")
            .filter(|r| r.scope == Scope::global())
        {
            state.governance.retain(|r| r.id != draft.id);
            state.governance.push(draft.clone());
        }
    }

    ConfigurationCatalogSnapshot::new(state, local.revision, local.repository_ready)
}

/// Project-owned Role identities remain globally discoverable without publishing from a query.
pub fn compose_role_directory(
    global: &ConfigurationCatalogSnapshot,
    projects: &[Arc<ConfigurationCatalogSnapshot>],
) -> Result<ConfigurationCatalogSnapshot> {
    let mut state = global.state.as_ref().clone();
    let mut known: BTreeMap<String, String> = state
        .governance
        .iter()
        .map(|r| (r.id.clone(), r.content_hash.clone()))
        .collect();
    for project in projects {
        for role in project
            .state
            .governance
            .iter()
            .filter(|r| matches!(r.r#type.as_str(), "role" | "role_retirement"))
        {
            if let Some(hash) = known.get(&role.id) {
                require(hash == &role.content_hash, "Role 目录摘要冲突")?;
            } else {
                known.insert(role.id.clone(), role.content_hash.clone());
                state.governance.push(role.clone());
            }
            if let Some(member) = template_catalog_member(&project.state, role)
                && !known.contains_key(&member.id)
            {
                known.insert(member.id.clone(), member.content_hash.clone());
                state.governance.push(member.clone());
            }
        }
    }
    ConfigurationCatalogSnapshot::new(state, global.revision, false)
}
