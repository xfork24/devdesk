//! A disposable, configuration-only read model. Execution facts remain in EngineeringStore.
use crate::*;
use std::collections::BTreeMap;

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

pub struct ConfigurationCatalogSnapshot {
    pub revision: u64,
    pub state: Arc<EngineeringState>,
    pub repository_ready: bool,
    references: BTreeMap<String, usize>,
    kinds: BTreeMap<String, Vec<usize>>,
    heads: BTreeMap<(String, String), u64>,
    origins: BTreeMap<String, Value>,
    health: std::sync::OnceLock<Result<Value>>,
}
impl ConfigurationCatalogSnapshot {
    pub fn new(mut state: EngineeringState, revision: u64, repository_ready: bool) -> Result<Self> {
        state = catalog_projection(&state);
        state.refresh_configuration_views()?;
        let mut snapshot = Self {
            revision,
            state: Arc::new(state),
            repository_ready,
            references: BTreeMap::new(),
            kinds: BTreeMap::new(),
            heads: BTreeMap::new(),
            origins: BTreeMap::new(),
            health: Default::default(),
        };
        for (index, record) in snapshot.state.governance.iter().enumerate() {
            snapshot.references.insert(record.id.clone(), index);
            snapshot
                .kinds
                .entry(record.r#type.clone())
                .or_default()
                .push(index);
            if template_record_current(&snapshot.state, record) {
                let head = snapshot
                    .heads
                    .entry((record.r#type.clone(), record.definition_id.clone()))
                    .or_default();
                *head = (*head).max(record.version);
            }
            let member = template_catalog_member(&snapshot.state, record);
            let origin = if !template_record_current(&snapshot.state, record) {
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
    pub fn origin(&self, reference: &str) -> Option<&Value> {
        self.origins.get(reference)
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
    fn transact(
        &self,
        _: Option<(&str, &str)>,
        _: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        Err(err("read_only", "配置读取视图不能执行命令"))
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
