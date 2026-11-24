//! Installed domain bundles and the active generation, scoped to one dataset.
use super::*;
use devdesk_core::{DomainOutputBinding, DomainRelation, DomainWorkflowBinding};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    sync::{Arc, Mutex, RwLock},
};
use uuid::Uuid;

mod storage;
mod worker;
pub(crate) use storage::{externally_stored_projects, project_storage_root};

pub const MAX_ARCHIVE: usize = 2 * 1024 * 1024;
const MAX_EXPANDED: u64 = 8 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    id: String,
    version: String,
    files: BTreeMap<String, String>,
    runtime: Option<Runtime>,
    ui: Option<Ui>,
    entity_types: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    entry: String,
    handlers: BTreeMap<String, Handler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    test_discovery: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_connection: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    test_reporters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pull_request_creation: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Handler {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    core: Option<CoreInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_snapshot: Option<crate::git::GitSnapshotSelection>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CoreInput {
    GitHead,
    GitSnapshot,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ui {
    contributions: String,
    targets: Vec<Target>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Target {
    renderer: String,
    api: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<String>,
    bindings: Vec<Binding>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    contribution: String,
    entry: String,
    export: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Contribution {
    id: String,
    title: String,
    kind: String,
    context: String,
    query: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    projection: bool,
    fields: Vec<Field>,
    #[serde(default)]
    inputs: Vec<Field>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    key: String,
    label: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityType {
    name: String,
    schema: Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    workflows: BTreeMap<Id, EntityWorkflow>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    relations: BTreeMap<String, DomainRelation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityWorkflow {
    input: String,
    outputs: BTreeMap<String, Id>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    owner_relations: BTreeMap<String, Vec<String>>,
}

pub struct WorkflowPackage<'a> {
    pub digest: &'a str,
    pub sources: &'a [ConfigurationRecord],
    pub role_agent_bindings: &'a BTreeMap<Id, Id>,
    pub entity_bindings: BTreeMap<Id, DomainWorkflowBinding>,
}

pub(super) fn experience_domains(root: &Path) -> Result<BTreeMap<Id, String>> {
    let path = root.join("domain-packages.sqlite3");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    reject_symlink(&path)?;
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(storage)?;
    let mut query = db
        .prepare(
            "SELECT d.project, coalesce(p.id, ':' || d.digest) FROM domain_projects d
                  LEFT JOIN domain_package_identities p ON p.digest=d.digest",
        )
        .map_err(storage)?;
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}

pub fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 140
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

pub fn safe_path(value: &str) -> bool {
    value.len() <= 256 && value.split('/').all(is_identifier)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn invalid(message: impl Into<String>) -> Error {
    err("invalid_domain_package", message)
}

fn embedded_schema(text: &str) -> jsonschema::Validator {
    let schema = serde_json::from_str(text).expect("domain schema JSON");
    jsonschema::options()
        .offline()
        .build(&schema)
        .expect("valid embedded domain schema")
}

static MANIFEST_SCHEMA: std::sync::LazyLock<jsonschema::Validator> =
    std::sync::LazyLock::new(|| {
        embedded_schema(include_str!("../../../schemas/domain-package.schema.json"))
    });
static CONTRIBUTIONS_SCHEMA: std::sync::LazyLock<jsonschema::Validator> =
    std::sync::LazyLock::new(|| {
        embedded_schema(include_str!(
            "../../../schemas/domain-contributions.schema.json"
        ))
    });
static ENTITY_TYPES_SCHEMA: std::sync::LazyLock<jsonschema::Validator> =
    std::sync::LazyLock::new(|| {
        embedded_schema(include_str!(
            "../../../schemas/domain-entity-types.schema.json"
        ))
    });

fn decode_contract<T: serde::de::DeserializeOwned>(
    validator: &jsonschema::Validator,
    bytes: &[u8],
) -> Result<T> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| invalid("领域文件不是有效 JSON"))?;
    if !validator.is_valid(&value) {
        return Err(invalid("领域文件不符合发布的 Schema"));
    }
    serde_json::from_value(value).map_err(|_| invalid("领域文件无法解码"))
}

fn validate_exchange(schema: Option<&Value>, value: &Value) -> Result<()> {
    if let Some(schema) = schema {
        let validator = jsonschema::options()
            .offline()
            .build(schema)
            .map_err(|_| invalid("领域处理器 Schema 不可用"))?;
        if !validator.is_valid(value) {
            return Err(err("domain_contract", "调用输入或结果与领域处理器合同不符"));
        }
    }
    Ok(())
}

struct Package {
    digest: String,
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    contributions: Vec<Contribution>,
    sources: Arc<[ConfigurationRecord]>,
    automation_sources: Arc<BTreeSet<Id>>,
    role_agent_bindings: BTreeMap<Id, Id>,
    agent_workflows: BTreeSet<Id>,
    defaults: Value,
    entity_types: BTreeMap<String, EntityType>,
}

impl Package {
    fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_ARCHIVE {
            return Err(invalid("领域 ZIP 超出大小限制"));
        }
        let mut zip =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid("无效的领域 ZIP"))?;
        require(zip.len() <= 256, "领域 ZIP 文件数超出限制")?;
        let mut files = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut remaining = MAX_EXPANDED;
        for index in 0..zip.len() {
            let entry = zip
                .by_index(index)
                .map_err(|_| invalid("无法读取 ZIP 项"))?;
            let name = entry.name().to_owned();
            let normalized = name.trim_end_matches('/');
            if !safe_path(normalized)
                || !seen.insert(normalized.to_owned())
                || entry
                    .unix_mode()
                    .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
            {
                return Err(invalid("领域 ZIP 只允许普通文件和规范路径"));
            }
            if entry.is_dir() {
                continue;
            }
            let mut body = Vec::new();
            entry
                .take(remaining + 1)
                .read_to_end(&mut body)
                .map_err(|_| invalid("ZIP 解压失败"))?;
            let size = body.len() as u64;
            if size > remaining || files.insert(name, body).is_some() {
                return Err(invalid("领域 ZIP 解压超限或包含重复路径"));
            }
            remaining -= size;
        }
        if files.keys().any(|name| {
            name.match_indices('/')
                .any(|(end, _)| files.contains_key(&name[..end]))
        }) {
            return Err(invalid("ZIP 文件与目录路径冲突"));
        }
        let data = files
            .remove("manifest.json")
            .ok_or_else(|| invalid("缺少领域清单"))?;
        let manifest: Manifest = decode_contract(&MANIFEST_SCHEMA, &data)?;
        if manifest.format != 1 || !is_identifier(&manifest.id) || !is_identifier(&manifest.version)
        {
            return Err(invalid("领域包格式或标识不受支持"));
        }
        if manifest.files.len() != files.len()
            || manifest
                .files
                .iter()
                .any(|(path, hash)| files.get(path).is_none_or(|bytes| digest(bytes) != *hash))
        {
            return Err(invalid("领域文件与清单摘要不符"));
        }
        let prefix = format!("{}.", manifest.id);
        let mut entity_types: BTreeMap<String, EntityType> = BTreeMap::new();
        if let Some(path) = &manifest.entity_types {
            entity_types = decode_contract(
                &ENTITY_TYPES_SCHEMA,
                files
                    .get(path)
                    .ok_or_else(|| invalid("领域对象类型文件缺失"))?,
            )?;
            for (reference, entity) in &entity_types {
                require(
                    reference.starts_with(&prefix) && !entity.name.trim().is_empty(),
                    "领域对象类型需要本包命名空间和有效名称",
                )?;
                require(
                    entity.relations.iter().all(|(name, rule)| {
                        is_identifier(name)
                            && rule.min_items <= rule.max_items
                            && entity_types.contains_key(&rule.target)
                    }),
                    "领域对象关系引用了无效类型或基数",
                )?;
                jsonschema::options()
                    .offline()
                    .build(&entity.schema)
                    .map_err(|_| invalid("领域对象 Schema 不可用"))?;
            }
        }
        if let Some(runtime) = &manifest.runtime
            && (!runtime.entry.starts_with("runtime/")
                || !runtime.entry.ends_with(".mjs")
                || !files.contains_key(&runtime.entry)
                || runtime.handlers.is_empty()
                || runtime
                    .handlers
                    .keys()
                    .any(|name| !is_identifier(name) || !name.starts_with(&prefix)))
        {
            return Err(invalid("领域运行入口或处理器无效"));
        }
        if let Some(runtime) = &manifest.runtime {
            for (runner, path) in &runtime.test_reporters {
                let extension = if runner == "node_test" { ".mjs" } else { ".py" };
                require(
                    path.starts_with("runtime/")
                        && path.ends_with(extension)
                        && files.get(path).is_some_and(|body| {
                            !body.is_empty()
                                && body.len() <= 64 * 1024
                                && std::str::from_utf8(body).is_ok()
                        }),
                    "领域测试报告器需要对应运行器的有界文本资源",
                )?;
            }
            if let Some(handler) = &runtime.test_discovery {
                require(
                    runtime.handlers.get(handler).is_some_and(|handler| {
                        matches!(handler.core, Some(CoreInput::GitSnapshot))
                            && handler.input_schema.is_some()
                            && handler.output_schema.is_some()
                    }),
                    "项目测试识别需要声明输入输出的 Git 快照处理器",
                )?;
            }
            for name in [
                &runtime.worktree_branch,
                &runtime.project_connection,
                &runtime.pull_request_creation,
            ]
            .into_iter()
            .flatten()
            {
                require(
                    runtime.handlers.get(name).is_some_and(|handler| {
                        handler.core.is_none()
                            && handler.input_schema.is_some()
                            && handler.output_schema.is_some()
                    }),
                    "领域项目接入、分支和 PR 策略需要声明输入输出的纯处理器",
                )?;
            }
            for handler in runtime.handlers.values() {
                match (&handler.core, &handler.git_snapshot) {
                    (Some(CoreInput::GitSnapshot), Some(selection)) => selection.validate()?,
                    (Some(CoreInput::GitSnapshot), None) | (_, Some(_)) => {
                        return Err(invalid("Git 快照需要独立的跟踪文件和搜索声明"));
                    }
                    _ => {}
                }
            }
            for schema in runtime
                .handlers
                .values()
                .flat_map(|h| [&h.input_schema, &h.output_schema])
                .flatten()
            {
                jsonschema::options()
                    .offline()
                    .build(schema)
                    .map_err(|_| invalid("领域处理器 Schema 不可用"))?;
            }
        }
        let mut contributions: Vec<Contribution> = vec![];
        if let Some(ui) = &manifest.ui {
            contributions = decode_contract(
                &CONTRIBUTIONS_SCHEMA,
                files
                    .get(&ui.contributions)
                    .ok_or_else(|| invalid("领域界面声明缺失"))?,
            )?;
            let mut ids = BTreeSet::new();
            for contribution in &contributions {
                if !is_identifier(&contribution.id)
                    || !contribution.id.starts_with(&prefix)
                    || !ids.insert(contribution.id.as_str())
                    || contribution.title.trim().is_empty()
                    || if contribution.projection {
                        !contribution.query.starts_with("query.")
                            || !is_identifier(&contribution.query)
                            || contribution.context != "project"
                    } else {
                        manifest
                            .runtime
                            .as_ref()
                            .is_none_or(|r| !r.handlers.contains_key(&contribution.query))
                    }
                {
                    return Err(invalid("领域界面引用了无效的贡献或处理器"));
                }
            }
            let mut targets = BTreeSet::new();
            for target in &ui.targets {
                let mut bindings = BTreeSet::new();
                if !is_identifier(&target.renderer)
                    || !targets.insert(&target.renderer)
                    || target.api == 0
                    || target.styles.iter().any(|style| {
                        target.renderer != "web.react"
                            || !style.starts_with("ui/web/")
                            || !style.ends_with(".css")
                            || !files.contains_key(style)
                    })
                    || target.bindings.iter().any(|binding| {
                        !ids.contains(binding.contribution.as_str())
                            || !bindings.insert(&binding.contribution)
                            || !binding.entry.starts_with("ui/")
                            || !files.contains_key(&binding.entry)
                            || !is_identifier(&binding.export)
                    })
                {
                    return Err(invalid("领域呈现入口无效"));
                }
            }
        }
        let (sources, defaults, configuration) =
            if files.contains_key("configurations/manifest.json") {
                let bundle = factory_bundle::read_bundle(|path| {
                    files
                        .get(&format!("configurations/{path}"))
                        .and_then(|body| std::str::from_utf8(body).ok())
                        .ok_or_else(|| invalid("领域源文件缺失或编码无效"))
                })?;
                let configuration = devdesk_core::validate_domain_configuration(&prefix, &bundle)?;
                (bundle.entries, bundle.defaults, configuration)
            } else {
                (vec![], json!({}), Default::default())
            };
        for workflow in sources.iter().filter(|source| source.r#type == "workflow") {
            let document: WorkflowDefinition = workflow.decode()?;
            for contract in document.operation_evidence.values() {
                require(
                    contract.verifier.starts_with(&prefix)
                        && manifest
                            .runtime
                            .as_ref()
                            .and_then(|runtime| runtime.handlers.get(&contract.verifier))
                            .is_some_and(|handler| handler.core.is_none()),
                    "工作流证据验证器必须由同一领域包注册",
                )?;
            }
        }
        for (owner, entity) in &entity_types {
            for (reference, binding) in &entity.workflows {
                let workflow = sources
                    .iter()
                    .find(|row| row.r#type == "workflow" && row.id == *reference)
                    .ok_or_else(|| invalid("领域对象引用了未声明的工作流"))?;
                require(
                    workflow.data["inputs"].get(&binding.input).is_some()
                        && binding
                            .owner_relations
                            .keys()
                            .all(|port| binding.outputs.contains_key(port))
                        && binding.outputs.iter().all(|(name, target)| {
                            entity_types.get(target).is_some_and(|target| {
                                let links = binding
                                    .owner_relations
                                    .get(name)
                                    .map(Vec::as_slice)
                                    .unwrap_or_default();
                                links.iter().collect::<BTreeSet<_>>().len() == links.len()
                                    && links.iter().all(|link| target.relations.contains_key(link))
                                    && target.relations.iter().all(|(key, rule)| {
                                        if links.contains(key) {
                                            &rule.target == owner
                                                && rule.min_items <= 1
                                                && rule.max_items >= 1
                                        } else {
                                            rule.min_items == 0
                                        }
                                    })
                            }) && workflow.data["outputs"].get(name).is_some_and(|source| {
                                source["step"]
                                    .as_str()
                                    .is_some_and(|step| !step.starts_with('$'))
                                    && source["pointer"].as_str().is_none_or(str::is_empty)
                            })
                        }),
                    "领域对象工作流绑定引用了无效的输入或产物",
                )?;
            }
        }
        let automation_sources = automation_sources(&sources, &entity_types, &defaults)?;
        Ok(Self {
            digest: digest(bytes),
            manifest,
            files,
            contributions,
            sources: sources.into(),
            automation_sources: Arc::new(automation_sources),
            role_agent_bindings: configuration.role_agent_bindings,
            agent_workflows: configuration.agent_workflows,
            defaults,
            entity_types,
        })
    }

    fn summary(&self) -> Value {
        json!({"digest":self.digest,"id":self.manifest.id,"version":self.manifest.version})
    }

    fn runtime(&self) -> Option<&Runtime> {
        self.manifest.runtime.as_ref()
    }
}

// Entity workflows own their private closure. Anything also reached by an
// ordinary source or a package default stays in the editable directory.
fn automation_sources(
    sources: &[ConfigurationRecord],
    entity_types: &BTreeMap<String, EntityType>,
    defaults: &Value,
) -> Result<BTreeSet<Id>> {
    if entity_types
        .values()
        .all(|entity| entity.workflows.is_empty())
    {
        return Ok(BTreeSet::new());
    }
    let edges: BTreeMap<_, _> = sources
        .iter()
        .map(|source| configuration_edges(source).map(|edges| (source.id.as_str(), edges)))
        .collect::<Result<_>>()?;
    let descendants = |mut pending: Vec<Id>| {
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if let Some(children) = edges.get(id.as_str())
                && visited.insert(id)
            {
                pending.extend(children.iter().map(|edge| edge.to.clone()));
            }
        }
        visited
    };
    let private = descendants(
        entity_types
            .values()
            .flat_map(|entity| entity.workflows.keys().cloned())
            .collect(),
    );
    let mut visible: Vec<_> = edges
        .keys()
        .copied()
        .filter(|id| !private.contains(*id))
        .map(str::to_owned)
        .collect();
    visible.extend(
        defaults
            .as_object()
            .into_iter()
            .flat_map(|values| values.values().filter_map(Value::as_str))
            .map(str::to_owned),
    );
    let visible = descendants(visible);
    Ok(private
        .into_iter()
        .filter(|id| !visible.contains(id))
        .collect())
}

pub struct DomainConfigurationSources {
    pub records: Arc<[ConfigurationRecord]>,
    pub automation: Arc<BTreeSet<Id>>,
}

struct Active {
    epoch: u64,
    package: Option<Arc<Package>>,
}

pub struct ProjectVisibility {
    pub active_digest: Option<Id>,
    pub(super) projects: BTreeSet<Id>,
    pub(super) unidentified_projects: BTreeSet<Id>,
}

impl ProjectVisibility {
    pub fn includes(&self, project: &str) -> bool {
        self.projects.contains(project) == self.active_digest.is_some()
    }
}

/// Serializes only activation lifecycle. Invocations pin their read lease until
/// the bounded worker exits so that deactivation cannot publish mid-operation.
pub struct DomainPackages {
    root: PathBuf,
    db: Mutex<Connection>,
    active: RwLock<Active>,
}

impl DomainPackages {
    pub fn registry_exists(root: &Path) -> bool {
        root.join("domain-packages.sqlite3").exists()
    }

    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("domain-packages.sqlite3");
        reject_symlink(&path)?;
        let db = Connection::open(path).map_err(storage)?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS domain_packages(
                digest TEXT PRIMARY KEY, id TEXT NOT NULL, version TEXT NOT NULL,
                zip BLOB NOT NULL, UNIQUE(id, version));
             CREATE TABLE IF NOT EXISTS domain_package_identities(
                digest TEXT PRIMARY KEY, id TEXT NOT NULL);
             INSERT OR IGNORE INTO domain_package_identities SELECT digest,id FROM domain_packages;
             CREATE TABLE IF NOT EXISTS domain_projects(
                project TEXT PRIMARY KEY, digest TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS domain_projects_digest ON domain_projects(digest);
             CREATE TABLE IF NOT EXISTS active_domain(
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                epoch INTEGER NOT NULL, digest TEXT REFERENCES domain_packages(digest));
             INSERT OR IGNORE INTO active_domain VALUES(1,0,NULL);",
        )
        .map_err(storage)?;
        db.execute_batch(storage::SCHEMA).map_err(storage)?;
        storage::restore_package_identities(&db, root)?;
        let (epoch, installed_digest, archive): (u64, Option<String>, Option<Vec<u8>>) = db
            .query_row(
                "SELECT a.epoch, a.digest, p.zip FROM active_domain a
             LEFT JOIN domain_packages p ON p.digest=a.digest WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage)?;
        let package = archive
            .as_deref()
            .map(Package::read)
            .transpose()?
            .map(Arc::new);
        if package.as_ref().map(|p| &p.digest) != installed_digest.as_ref() {
            return Err(err("storage", "活动领域包摘要与存储内容不符"));
        }
        Ok(Self {
            root: root.into(),
            db: Mutex::new(db),
            active: RwLock::new(Active { epoch, package }),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.active.read().unwrap().epoch
    }

    pub fn status(&self) -> Value {
        let active = self.active.read().unwrap();
        let package = active.package.as_deref();
        json!({
            "epoch":active.epoch,
            "active":package.map(|p| p.summary()),
            "contributions":package.map(|p| &p.contributions),
            "targets":package.and_then(|p| p.manifest.ui.as_ref()).map(|ui| &ui.targets),
            "sources":package.map(|p| p.sources.as_ref()),
            "agent_workflows":package.map(|p| &p.agent_workflows),
            "defaults":package.map(|p| &p.defaults),
            "project_connection":package.and_then(|p| p.runtime()).and_then(|r| r.project_connection.as_deref()),
            "entity_types":package.map(|p| &p.entity_types),
        })
    }

    pub fn installed(&self) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT digest,id,version FROM domain_packages ORDER BY id,version")
            .map_err(storage)?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        Ok(json!(
            rows.into_iter()
                .map(|(digest, id, version)| json!({"digest":digest,"id":id,"version":version}))
                .collect::<Vec<_>>()
        ))
    }

    pub fn install(&self, bytes: &[u8]) -> Result<Value> {
        let package = Package::read(bytes)?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO domain_packages VALUES(?,?,?,?) ON CONFLICT(digest) DO NOTHING",
            params![
                package.digest,
                package.manifest.id,
                package.manifest.version,
                bytes
            ],
        )
        .map_err(|e| {
            if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                invalid("同一领域版本已登记了其他内容")
            } else {
                storage(e)
            }
        })?;
        // Project membership outlives the installed archive.
        tx.execute(
            "INSERT INTO domain_package_identities VALUES(?,?) ON CONFLICT(digest) DO UPDATE SET id=excluded.id",
            params![package.digest, package.manifest.id],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(package.summary())
    }

    pub fn activate(
        &self,
        digest: &str,
        expected: u64,
        quiescent: impl FnOnce(Vec<String>) -> Result<()>,
    ) -> Result<u64> {
        check_epoch(expected, self.epoch())?;
        let archive: Vec<u8> = self
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT zip FROM domain_packages WHERE digest=?",
                [digest],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| err("not_found", "领域包尚未安装"))?;
        let package = Arc::new(Package::read(&archive)?);
        if package.digest != digest {
            return Err(err("storage", "领域包内容与登记摘要不符"));
        }
        if let Some(runtime) = package.runtime() {
            worker::describe(&package, runtime)?;
        }
        let mut active = self.active.write().unwrap();
        check_epoch(expected, active.epoch)?;
        if active.package.as_ref().is_some_and(|p| p.digest == digest) {
            return Ok(active.epoch);
        }
        if let Some(previous) = &active.package {
            quiescent(self.projects(&previous.manifest.id)?)?;
        }
        let db = self.db.lock().unwrap();
        let epoch = next_epoch(expected)?;
        db.execute(
            "UPDATE active_domain SET epoch=?,digest=? WHERE singleton=1",
            params![epoch, digest],
        )
        .map_err(storage)?;
        *active = Active {
            epoch,
            package: Some(package),
        };
        Ok(epoch)
    }

    pub fn deactivate(
        &self,
        digest: &str,
        expected: u64,
        uninstall: bool,
        quiescent: impl FnOnce(Vec<String>) -> Result<()>,
    ) -> Result<u64> {
        let mut active = self.active.write().unwrap();
        check_epoch(expected, active.epoch)?;
        let stopping = active.package.as_ref().filter(|p| p.digest == digest);
        if let Some(package) = stopping {
            quiescent(self.projects(&package.manifest.id)?)?;
        }
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(storage)?;
        let mut epoch = active.epoch;
        if stopping.is_some() {
            epoch = next_epoch(expected)?;
            tx.execute(
                "UPDATE active_domain SET epoch=?,digest=NULL WHERE singleton=1",
                [epoch],
            )
            .map_err(storage)?;
        }
        if uninstall {
            tx.execute("DELETE FROM domain_packages WHERE digest=?", [digest])
                .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        if epoch != active.epoch {
            *active = Active {
                epoch,
                package: None,
            };
        }
        Ok(epoch)
    }

    pub fn check_epoch(&self, epoch: u64) -> Result<()> {
        check_epoch(epoch, self.epoch())
    }

    pub fn defaults_at(&self, epoch: u64) -> Result<Option<Value>> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        Ok(active
            .package
            .as_ref()
            .map(|package| package.defaults.clone()))
    }

    pub fn contribution_projection(&self, epoch: u64, id: &str) -> Result<String> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_deref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        package
            .contributions
            .iter()
            .find(|item| item.id == id && item.projection)
            .map(|item| item.query.clone())
            .ok_or_else(|| err("method_not_found", "领域查询贡献未注册"))
    }

    pub fn invoke(&self, epoch: u64, name: &str, input: Value) -> Result<Value> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_deref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        Self::run_handler(package, name, input)
    }

    fn run_handler(package: &Package, name: &str, input: Value) -> Result<Value> {
        let runtime = package
            .runtime()
            .ok_or_else(|| err("method_not_found", "领域没有运行端"))?;
        let handler = runtime
            .handlers
            .get(name)
            .ok_or_else(|| err("method_not_found", "领域操作未注册"))?;
        validate_exchange(handler.input_schema.as_ref(), &input)?;
        let core = match handler.core {
            Some(CoreInput::GitHead) => crate::git::local_git(&input, false)?,
            Some(CoreInput::GitSnapshot) => {
                let selection = handler
                    .git_snapshot
                    .as_ref()
                    .ok_or_else(|| invalid("Git 快照声明缺失"))?;
                json!(crate::git::local_git_snapshot(&input, selection)?)
            }
            None => Value::Null,
        };
        let result = worker::invoke(package, runtime, name, input, core)?;
        validate_exchange(handler.output_schema.as_ref(), &result)?;
        Ok(result)
    }

    pub fn read_for_project(&self, project: &str, name: &str, input: Value) -> Result<Value> {
        let active = self.active.read().unwrap();
        let package = self.require_project(&active, project)?;
        Self::run_handler(package, name, input)
    }

    pub fn worktree_branch(
        &self,
        project: &str,
        owner: &str,
        kind: WorktreeKind,
    ) -> Result<Option<String>> {
        let active = self.active.read().unwrap();
        if self.project_domain(project)?.is_none() {
            return Ok(None);
        }
        let package = self.require_project(&active, project)?;
        let Some(handler) = package
            .runtime()
            .and_then(|runtime| runtime.worktree_branch.as_deref())
        else {
            return Ok(None);
        };
        let value = Self::run_handler(package, handler, json!({"owner":owner,"kind":kind}))?;
        require(
            value["owner"] == owner && value["kind"] == json!(kind),
            "领域策略改变了工作副本归属",
        )?;
        value["branch"]
            .as_str()
            .filter(|branch| {
                !branch.is_empty()
                    && branch.len() <= 240
                    && !branch.starts_with('-')
                    && !branch.chars().any(char::is_control)
            })
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| invalid("领域策略返回了无效的分支"))
    }

    pub fn pull_request_creation(
        &self,
        request: &PullRequestRequest,
    ) -> Result<Option<PullRequestCreation>> {
        let active = self.active.read().unwrap();
        if self.project_domain(&request.project)?.is_none() {
            return Ok(None);
        }
        let package = self.require_project(&active, &request.project)?;
        let Some(handler) = package
            .runtime()
            .and_then(|runtime| runtime.pull_request_creation.as_deref())
        else {
            return Ok(None);
        };
        let result = Self::run_handler(
            package,
            handler,
            json!({"request":request,"marker":request.marker()}),
        )?;
        let creation: PullRequestCreation = serde_json::from_value(result)
            .map_err(|error| err("domain_contract", error.to_string()))?;
        creation.validate(request)?;
        Ok(Some(creation))
    }

    pub fn test_reporter(
        &self,
        project: &str,
        runner: &TestRunner,
    ) -> Result<Option<ports::TestReporter>> {
        let active = self.active.read().unwrap();
        if self.project_domain(project)?.is_none() {
            return Ok(None);
        }
        let package = self.require_project(&active, project)?;
        let key = json!(runner);
        let Some(path) = package
            .runtime()
            .and_then(|runtime| key.as_str().and_then(|key| runtime.test_reporters.get(key)))
        else {
            return Ok(None);
        };
        let content = std::str::from_utf8(&package.files[path]).map_err(storage)?;
        Ok(Some(ports::TestReporter {
            digest: package.manifest.files[path].clone(),
            content: content.to_owned(),
        }))
    }

    pub fn project_test_discovery(
        &self,
        project: &str,
        mainline: &str,
    ) -> Result<Option<ProjectTestDiscovery>> {
        if self.project_domain(project)?.is_none() {
            return Ok(None);
        }
        // Project RPCs can already own a read lease. A concurrent switch
        // must fail this read promptly instead of waiting on a nested lease.
        let active = self
            .active
            .try_read()
            .map_err(|_| err("domain_busy", "领域正在切换，请重新读取测试配置"))?;
        let package = self.require_project(&active, project)?;
        let Some(handler) = package
            .runtime()
            .and_then(|runtime| runtime.test_discovery.as_deref())
        else {
            return Ok(None);
        };
        let git = crate::git::CliGit::new(&self.root)?;
        let Some((repo, revision)) = git.registered_revision(project, mainline)? else {
            return Ok(Some(ProjectTestDiscovery::default()));
        };
        let reply = Self::run_handler(package, handler, json!({"path":repo,"revision":revision}))?;
        let result: ProjectTestDiscovery = serde_json::from_value(reply)
            .map_err(|error| err("domain_contract", error.to_string()))?;
        require(
            result.revision.as_deref() == Some(revision.as_str()),
            "测试入口改变了原 Git 修订",
        )?;
        result.configuration.validate()?;
        Ok(Some(result))
    }

    pub fn with_workflow<T>(
        &self,
        epoch: u64,
        (reference, mode, project): (&str, &str, Option<&str>),
        start: impl FnOnce(WorkflowPackage<'_>) -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_deref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        if let Some(project) = project {
            self.require_project(&active, project)?;
        }
        require(
            package
                .sources
                .iter()
                .any(|row| row.r#type == "workflow" && row.id == reference),
            "当前领域没有该 Workflow 修订",
        )?;
        if mode == "agent" && !package.agent_workflows.contains(reference) {
            return Err(err(
                "configuration_required",
                "此 Workflow 未配置编排 Agent",
            ));
        }
        let entity_bindings = package
            .entity_types
            .iter()
            .filter_map(|(owner_type, definition)| {
                let binding = definition.workflows.get(reference)?;
                let outputs = binding
                    .outputs
                    .iter()
                    .map(|(port, target)| {
                        (
                            port.clone(),
                            DomainOutputBinding {
                                type_ref: target.clone(),
                                schema: package.entity_types[target].schema.clone(),
                                owner_relations: binding
                                    .owner_relations
                                    .get(port)
                                    .cloned()
                                    .unwrap_or_default(),
                            },
                        )
                    })
                    .collect();
                Some((
                    owner_type.clone(),
                    DomainWorkflowBinding {
                        digest: package.digest.clone(),
                        owner_type: owner_type.clone(),
                        input: binding.input.clone(),
                        outputs,
                    },
                ))
            })
            .collect();
        start(WorkflowPackage {
            digest: &package.digest,
            sources: &package.sources,
            role_agent_bindings: &package.role_agent_bindings,
            entity_bindings,
        })
    }

    pub fn with_sources<T>(
        &self,
        epoch: u64,
        connection: Option<&Value>,
        action: impl FnOnce(&str, &[ConfigurationRecord], Option<Value>) -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_deref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        let plan = connection
            .map(|input| {
                let handler = package
                    .runtime()
                    .and_then(|runtime| runtime.project_connection.as_deref())
                    .ok_or_else(|| err("unsupported", "活动领域没有声明项目连接处理器"))?;
                Self::run_handler(package, handler, input.clone())
            })
            .transpose()?;
        action(&package.digest, &package.sources, plan)
    }

    fn projects(&self, domain: &str) -> Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT d.project FROM domain_projects d
                      JOIN domain_package_identities p ON p.digest=d.digest WHERE p.id=? ORDER BY d.project")
            .map_err(storage)?;
        query
            .query_map([domain], |row| row.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    /// A directory view follows the committed pointer, including while the
    /// caller already pins an active domain for a project RPC.
    pub fn project_visibility(&self, domain_epoch: Option<u64>) -> Result<ProjectVisibility> {
        let db = self.db.lock().unwrap();
        let (active_digest, domain) = if let Some(expected) = domain_epoch {
            let (epoch, digest, domain): (u64, Option<String>, Option<String>) = db
                .query_row(
                    "SELECT a.epoch,a.digest,p.id FROM active_domain a
                     LEFT JOIN domain_packages p ON p.digest=a.digest WHERE a.singleton=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(storage)?;
            check_epoch(expected, epoch)?;
            (
                digest,
                Some(domain.ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?),
            )
        } else {
            (None, None)
        };
        let mut query = db
            .prepare(
                "SELECT d.project,p.id FROM domain_projects d
                      LEFT JOIN domain_package_identities p ON p.digest=d.digest
                      WHERE ?1 IS NULL OR p.id=?1 OR p.id IS NULL",
            )
            .map_err(storage)?;
        let mut projects = BTreeSet::new();
        let mut unidentified_projects = BTreeSet::new();
        let rows = query
            .query_map([&domain], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(storage)?;
        for row in rows {
            let (project, owner) = row.map_err(storage)?;
            if domain.is_none() || owner.is_some() {
                projects.insert(project);
            } else {
                unidentified_projects.insert(project);
            }
        }
        Ok(ProjectVisibility {
            active_digest,
            projects,
            unidentified_projects,
        })
    }

    pub fn configuration_scope(
        &self,
        domain_epoch: Option<u64>,
    ) -> Result<(ProjectVisibility, Option<DomainConfigurationSources>)> {
        let Some(epoch) = domain_epoch else {
            return Ok((self.project_visibility(None)?, None));
        };
        // A project RPC may already pin the domain. Never wait for a second
        // lease while a switch is waiting to acquire the write lock.
        let active = self
            .active
            .try_read()
            .map_err(|_| err("domain_busy", "领域正在切换，请重新读取配置"))?;
        let visibility = self.project_visibility(Some(epoch))?;
        let sources = active
            .package
            .as_ref()
            .map(|package| DomainConfigurationSources {
                records: package.sources.clone(),
                automation: package.automation_sources.clone(),
            });
        Ok((visibility, sources))
    }

    pub fn permits_project(&self, project: &str) -> Result<bool> {
        let active = self.active.read().unwrap();
        let saved = self.project_domain(project)?;
        Ok(saved.is_none_or(|domain| {
            active
                .package
                .as_ref()
                .is_some_and(|package| package.manifest.id == domain)
        }))
    }

    fn project_domain(&self, project: &str) -> Result<Option<String>> {
        self.db
            .lock()
            .unwrap()
            .query_row(
                "SELECT coalesce(p.id, '') FROM domain_projects d
                 LEFT JOIN domain_package_identities p ON p.digest=d.digest WHERE d.project=?",
                [project],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)
    }

    pub fn project_package_namespace(&self, project: &str) -> Result<Option<String>> {
        Ok(self
            .project_domain(project)?
            .filter(|id| !id.is_empty())
            .map(|id| format!("{id}.")))
    }

    pub fn bind_project(&self, project: &str, digest: &str) -> Result<()> {
        self.bind_project_at(project, digest, None)
    }

    pub fn bind_copied_project(&self, project: &str, digest: &str, root: &Path) -> Result<()> {
        self.bind_project_at(project, digest, Some(root))
    }

    fn bind_project_at(&self, project: &str, digest: &str, root: Option<&Path>) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(storage)?;
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO domain_projects VALUES(?,?)",
                params![project, digest],
            )
            .map_err(storage)?;
        let same_domain: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM domain_projects d
                 JOIN domain_package_identities p ON p.digest=d.digest
                 JOIN domain_packages active ON active.id=p.id
                 WHERE d.project=?1 AND active.digest=?2)",
                params![project, digest],
                |row| row.get(0),
            )
            .map_err(storage)?;
        require(same_domain, "项目已登记到其他领域")?;
        if inserted == 1 {
            tx.execute(
                "INSERT INTO domain_project_storage(project,data_root)
                 SELECT ?1,s.data_root FROM domain_storage s JOIN domain_packages p ON s.package_id=p.id
                 WHERE p.digest=?2 AND s.data_root IS NOT NULL",
                params![project, digest],
            ).map_err(storage)?;
        }
        if let Some(expected) = root {
            let pinned: Option<String> = tx
                .query_row(
                    "SELECT data_root FROM domain_project_storage WHERE project=?",
                    [project],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            if pinned.as_deref().map(Path::new).unwrap_or(&self.root) != expected {
                return Err(err(
                    "revision_conflict",
                    "领域项目数据目录已改变，请重新打开项目",
                ));
            }
        }
        tx.commit().map_err(storage)
    }

    pub fn with_project<T>(
        &self,
        project: &str,
        epoch: u64,
        action: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = self.require_project(&active, project)?;
        action(&package.digest)
    }

    pub fn validate_entity(
        &self,
        project: &str,
        epoch: u64,
        (reference, data): (&str, &Value),
        write: impl FnOnce(&str, &BTreeMap<String, DomainRelation>) -> Result<Value>,
    ) -> Result<Value> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = self.require_project(&active, project)?;
        let entity = package
            .entity_types
            .get(reference)
            .ok_or_else(|| err("not_found", "活动包未声明此领域对象类型"))?;
        require(data.to_string().len() <= 16 * 1024, "领域对象数据超出预算")?;
        validate_exchange(Some(&entity.schema), data)?;
        write(&package.digest, &entity.relations)
    }

    /// Pins existing public project endpoints to the current domain.
    pub fn with_public_project<T>(
        &self,
        project: &str,
        epoch: Option<u64>,
        action: impl FnOnce(Option<u64>) -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        let Some(saved) = self.project_domain(project)? else {
            drop(active);
            if epoch.is_some() {
                return Err(err("domain_inactive", "项目未绑定领域包"));
            }
            return action(None);
        };
        if active
            .package
            .as_ref()
            .is_none_or(|p| p.manifest.id != saved)
        {
            return Err(err("domain_inactive", "项目所属领域当前不可用"));
        }
        let epoch = epoch.ok_or_else(|| err("invalid_params", "领域项目请求需要 domain_epoch"))?;
        check_epoch(epoch, active.epoch)?;
        action(Some(epoch))
    }

    fn require_project<'a>(&self, active: &'a Active, project: &str) -> Result<&'a Package> {
        let package = active
            .package
            .as_deref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        let saved = self.project_domain(project)?;
        require(
            saved.as_deref() == Some(&package.manifest.id),
            "项目不属于当前领域",
        )?;
        Ok(package)
    }

    pub fn asset(&self, epoch: u64, digest: &str, path: &str) -> Option<Vec<u8>> {
        let active = self.active.read().unwrap();
        if epoch != active.epoch
            || !path.starts_with("ui/web/")
            || !matches!(
                Path::new(path).extension().and_then(|e| e.to_str()),
                Some("js" | "mjs" | "css" | "png" | "jpg" | "webp")
            )
        {
            return None;
        }
        let package = active.package.as_ref().filter(|p| p.digest == digest)?;
        package.files.get(path).cloned()
    }
}

fn check_epoch(expected: u64, actual: u64) -> Result<()> {
    if expected != actual {
        return Err(err("domain_epoch_conflict", "活动的领域代次已变化"));
    }
    Ok(())
}

fn next_epoch(epoch: u64) -> Result<u64> {
    epoch
        .checked_add(1)
        .ok_or_else(|| err("storage", "领域代次溢出"))
}
