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

pub fn package_role_identity(record: &ConfigurationRecord) -> Option<&str> {
    // Domain bundles must namespace their Role identities. Built-in Roles and
    // identities allocated by role.create have no namespace.
    let identity = match record.r#type.as_str() {
        "role" | "role_retirement" => record.definition_id.as_str(),
        "template_catalog_member" if record.data["kind"] == "role" => {
            record.definition_id.rsplit_once(':')?.0
        }
        _ => return None,
    };
    identity.contains('.').then_some(identity)
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
    automation_sources: Arc<BTreeSet<Id>>,
    domain_catalog: bool,
    historical_sources: BTreeSet<Id>,
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
            automation_sources: Default::default(),
            domain_catalog: false,
            historical_sources: BTreeSet::new(),
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
    pub fn with_automation_sources(mut self, automation_sources: Arc<BTreeSet<Id>>) -> Self {
        if !automation_sources.is_empty() {
            self.fingerprint =
                configuration_hash(&(&self.fingerprint, automation_sources.as_ref()));
            self.automation_sources = automation_sources;
        }
        self
    }

    pub fn is_listed(&self, record: &ConfigurationRecord) -> bool {
        !self.automation_sources.contains(&record.id)
    }

    fn with_historical_sources(mut self, sources: BTreeSet<Id>) -> Self {
        if !sources.is_empty() {
            self.fingerprint = configuration_hash(&(&self.fingerprint, &sources));
            for id in &sources {
                if let Some(origin) = self.origins.get_mut(id) {
                    origin["origin"] = json!("archived");
                }
            }
            self.historical_sources = sources;
        }
        self
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
    pub(crate) fn needs_factory_reset_from(&self, global: &Self) -> bool {
        template_hard_reset_generation(&self.state) < template_hard_reset_generation(&global.state)
    }
    fn same_factory_reset_as(&self, other: &Self) -> bool {
        template_hard_reset_generation(&self.state) == template_hard_reset_generation(&other.state)
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
        self.is_historical_domain_source(record)
            || !self.is_current(record)
            || (record.r#type == "standard"
                && self.archived_standards.get(&record.id) == Some(&true))
    }
    pub fn is_historical_domain_source(&self, record: &ConfigurationRecord) -> bool {
        self.historical_sources.contains(&record.id)
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
            effect_in_flight: self.effect_in_flight.clone(),
            configuration_reset: std::sync::Mutex::new(()),
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
            domain_reader: self.domain_reader.clone(),
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
    let package_roles: BTreeSet<_> = global
        .records("role")
        .filter_map(package_role_identity)
        .collect();
    state.governance.retain(|record| {
        package_role_identity(record).is_none_or(|role| package_roles.contains(role))
    });
    // A hard reset may recreate IDs such as permission bindings at revision one.
    // The independent snapshots can straddle either side of the reset.
    if local.same_factory_reset_as(global)
        && state.projects.iter().all(|p| p.execution_mode == "live")
    {
        for record in global.state.governance.iter().filter(|r| {
            // Membership is local catalog metadata and is reconciled below.
            r.r#type != "template_catalog_member"
                && r.immutable
                && (r.r#type == "role"
                    || r.r#type == "role_retirement"
                    || configuration_visible(&global.state, r, Some(project)))
        }) {
            if let Some(old) = local.record(&record.id) {
                require(
                    old.content_hash == record.content_hash,
                    format!("共享配置摘要冲突 {}", record.id),
                )?;
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

    let historical_sources = state
        .governance
        .iter()
        .filter(|r| {
            global.historical_sources.contains(&r.id)
                || (global.domain_catalog
                    && r.is_definition()
                    && r.scope.is_global()
                    && global.record(&r.id).is_none())
        })
        .map(|r| r.id.clone())
        .collect();
    Ok(
        ConfigurationCatalogSnapshot::new(state, local.revision, local.repository_ready)?
            .with_automation_sources(global.automation_sources.clone())
            .with_historical_sources(historical_sources),
    )
}

/// Include active domain sources and discoverable project Roles in one read view.
pub fn compose_configuration_directory(
    global: &ConfigurationCatalogSnapshot,
    domain_sources: &[ConfigurationRecord],
    projects: &[Arc<ConfigurationCatalogSnapshot>],
    include_project: impl Fn(&str) -> bool,
) -> Result<ConfigurationCatalogSnapshot> {
    let mut state = global.state.as_ref().clone();
    let package_roles: BTreeSet<_> = domain_sources
        .iter()
        .filter_map(package_role_identity)
        .collect();
    let visible = |record: &ConfigurationRecord| {
        record.scope.project_id().is_none_or(&include_project)
            && package_role_identity(record).is_none_or(|role| package_roles.contains(role))
    };
    state.governance.retain(&visible);
    let mut known: BTreeMap<String, String> = state
        .governance
        .iter()
        .map(|r| (r.id.clone(), r.content_hash.clone()))
        .collect();
    for source in domain_sources {
        if let Some(hash) = known.get(&source.id) {
            require(
                hash == &source.content_hash,
                format!("领域源定义修订冲突 {}", source.id),
            )?;
        } else {
            known.insert(source.id.clone(), source.content_hash.clone());
            state.governance.push(source.clone());
        }
    }
    if !domain_sources.is_empty() {
        register_template_members(&mut state, template_catalog_generation(&global.state));
    }
    for project in projects
        .iter()
        .filter(|project| project.same_factory_reset_as(global))
    {
        for role in project
            .state
            .governance
            .iter()
            .filter(|r| matches!(r.r#type.as_str(), "role" | "role_retirement"))
            .filter(|r| package_role_identity(r).is_none())
            .filter(|r| visible(r))
        {
            if !domain_sources.is_empty() && role.scope.is_global() && !known.contains_key(&role.id)
            {
                continue;
            }
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
    let mut catalog = ConfigurationCatalogSnapshot::new(state, global.revision, false)?;
    catalog.domain_catalog = !domain_sources.is_empty();
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governance::record;

    #[test]
    fn imported_global_sources_remain_readable_as_history_in_a_domain_project() {
        let role = |id: &str, scope| {
            record(
                "role",
                id.into(),
                scope,
                "Reviewer".into(),
                1,
                None,
                json!({"instructions":"Review the result."}),
                true,
            )
        };
        let project_copy = role("old-project-global-reviewer", Scope::global());
        let project_role = role("project-reviewer", Scope::project("project"));
        let packaged = role("software-development.role-reviewer", Scope::global());
        let global =
            ConfigurationCatalogSnapshot::new(EngineeringState::default(), 1, false).unwrap();
        let local = Arc::new(
            ConfigurationCatalogSnapshot::new(
                EngineeringState {
                    governance: vec![project_copy.clone(), project_role.clone()],
                    ..Default::default()
                },
                1,
                true,
            )
            .unwrap(),
        );
        let directory = compose_configuration_directory(
            &global,
            std::slice::from_ref(&packaged),
            std::slice::from_ref(&local),
            |_| true,
        )
        .unwrap();
        assert!(directory.record(&project_copy.id).is_none());
        let view = compose_configuration_catalog(&directory, &local, "project").unwrap();
        let sources =
            crate::configuration_index(&view, &json!({"kind":"role"}), Some("project")).unwrap();
        let ids = sources["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            BTreeSet::from([packaged.id.as_str(), project_role.id.as_str()])
        );
        assert_eq!(view.record(&project_copy.id), Some(&project_copy));
        assert!(view.is_archived(&project_copy));
    }

    #[test]
    fn packaged_role_composition_preserves_project_membership() {
        let role = record(
            "role",
            "test-domain.reviewer".into(),
            Scope::global(),
            "Reviewer".into(),
            1,
            None,
            json!({"instructions":"Review the current result."}),
            true,
        );
        let mut project = EngineeringState::default();
        project.governance.push(role.clone());
        register_template_members(&mut project, TEMPLATE_INITIAL_GENERATION);
        let member = template_catalog_member(&project, &role).unwrap();
        let member_id = member.id.clone();

        // A shared directory can first discover the role without its membership.
        let mut directory = EngineeringState::default();
        directory.governance.push(role);
        copy_template_catalog(&EngineeringState::default(), &mut directory).unwrap();
        let global = ConfigurationCatalogSnapshot::new(directory, 1, false).unwrap();
        let local = ConfigurationCatalogSnapshot::new(project, 1, true).unwrap();
        assert_ne!(
            global.record(&member_id).unwrap().content_hash,
            local.record(&member_id).unwrap().content_hash
        );

        let composed = compose_configuration_catalog(&global, &local, "project").unwrap();
        assert_eq!(
            composed.record(&member_id).unwrap(),
            local.record(&member_id).unwrap()
        );

        // Older hosts could publish package Roles into the shared catalog.
        let core =
            compose_configuration_directory(&global, &[], &[Arc::new(local)], |_| true).unwrap();
        assert_eq!(core.records("role").count(), 0);
        assert!(core.record(&member_id).is_none());
        let core_project = compose_configuration_catalog(&core, &composed, "project").unwrap();
        assert_eq!(core_project.records("role").count(), 0);
        assert!(core_project.record(&member_id).is_none());
    }
}
