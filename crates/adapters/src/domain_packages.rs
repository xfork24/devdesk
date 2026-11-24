//! Installed domain bundles and the active generation, scoped to one dataset.
use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    sync::{Arc, Mutex, RwLock},
};
use uuid::Uuid;

mod worker;

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
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    entry: String,
    handlers: BTreeMap<String, Handler>,
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
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CoreInput {
    GitHead,
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
    sources: Vec<ConfigurationRecord>,
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
                    || manifest
                        .runtime
                        .as_ref()
                        .is_none_or(|r| !r.handlers.contains_key(&contribution.query))
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
        let sources = if files.contains_key("configurations/manifest.json") {
            let bundle = factory_bundle::read_bundle(|path| {
                files
                    .get(&format!("configurations/{path}"))
                    .and_then(|body| std::str::from_utf8(body).ok())
                    .ok_or_else(|| invalid("领域源文件缺失或编码无效"))
            })?;
            if configuration_usage_index(&bundle.entries)? != bundle.usage_index
                || bundle.entries.iter().any(|record| {
                    !record.scope.is_global() || !record.definition_id.starts_with(&prefix)
                })
            {
                return Err(invalid("领域源定义命名空间或引用索引不符"));
            }
            let mut catalog = EngineeringState::default();
            devdesk_core::initialize_template_catalog(&mut catalog)?;
            catalog.governance.extend(bundle.entries.iter().cloned());
            validate_source_closure(&catalog, &bundle.entries)?;
            bundle.entries
        } else {
            vec![]
        };
        Ok(Self {
            digest: digest(bytes),
            manifest,
            files,
            contributions,
            sources,
        })
    }

    fn summary(&self) -> Value {
        json!({"digest":self.digest,"id":self.manifest.id,"version":self.manifest.version})
    }

    fn runtime(&self) -> Option<&Runtime> {
        self.manifest.runtime.as_ref()
    }
}

fn validate_source_closure(
    catalog: &EngineeringState,
    sources: &[ConfigurationRecord],
) -> Result<()> {
    for source in sources {
        for edge in configuration_edges(source)? {
            let target = catalog.configuration(&edge.to)?;
            require(target.r#type == edge.expected_type, "领域源引用类型不匹配")?;
        }
        if source.r#type == "workflow" {
            devdesk_core::compile_workflow_sources(None, catalog, source, &Scope::global(), None)?;
        }
    }
    Ok(())
}

struct Active {
    epoch: u64,
    package: Option<Arc<Package>>,
}

/// Serializes only activation lifecycle. Invocations pin their read lease until
/// the bounded worker exits so that deactivation cannot publish mid-operation.
pub struct DomainPackages {
    db: Mutex<Connection>,
    active: RwLock<Active>,
}

impl DomainPackages {
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("domain-packages.sqlite3");
        reject_symlink(&path)?;
        let db = Connection::open(path).map_err(storage)?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS domain_packages(
                digest TEXT PRIMARY KEY, id TEXT NOT NULL, version TEXT NOT NULL,
                zip BLOB NOT NULL, UNIQUE(id, version));
             CREATE TABLE IF NOT EXISTS domain_projects(
                project TEXT PRIMARY KEY, digest TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS active_domain(
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                epoch INTEGER NOT NULL, digest TEXT REFERENCES domain_packages(digest));
             INSERT OR IGNORE INTO active_domain VALUES(1,0,NULL);",
        )
        .map_err(storage)?;
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
            db: Mutex::new(db),
            active: RwLock::new(Active { epoch, package }),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.active.read().unwrap().epoch
    }

    pub fn status(&self, trusted: &BTreeSet<String>) -> Value {
        let active = self.active.read().unwrap();
        let package = active
            .package
            .as_ref()
            .filter(|p| trusted.contains(&p.digest));
        json!({
            "epoch":active.epoch,
            "active":package.map(|p| p.summary()),
            "contributions":package.map(|p| &p.contributions),
            "targets":package.and_then(|p| p.manifest.ui.as_ref()).map(|ui| &ui.targets),
            "sources":package.map(|p| &p.sources),
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
        let db = self.db.lock().unwrap();
        db.execute(
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
        Ok(package.summary())
    }

    pub fn activate(
        &self,
        digest: &str,
        expected: u64,
        trusted: &BTreeSet<String>,
        quiescent: impl FnOnce(Vec<String>) -> Result<()>,
    ) -> Result<u64> {
        if !trusted.contains(digest) {
            return Err(err("domain_untrusted", "该领域 ZIP 摘要未获信任"));
        }
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
            quiescent(self.projects(&previous.digest)?)?;
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
        let stopping = active.package.as_ref().is_some_and(|p| p.digest == digest);
        if stopping {
            quiescent(self.projects(digest)?)?;
        }
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(storage)?;
        let mut epoch = active.epoch;
        if stopping {
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

    pub fn invoke(
        &self,
        epoch: u64,
        name: &str,
        input: Value,
        trusted: &BTreeSet<String>,
    ) -> Result<Value> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_ref()
            .filter(|p| trusted.contains(&p.digest))
            .ok_or_else(|| err("domain_inactive", "没有获准的活动领域"))?;
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
            None => Value::Null,
        };
        let result = worker::invoke(package, runtime, name, input, core)?;
        validate_exchange(handler.output_schema.as_ref(), &result)?;
        Ok(result)
    }

    pub fn read_for_project(
        &self,
        project: &str,
        name: &str,
        input: Value,
        trusted: &BTreeSet<String>,
    ) -> Result<Value> {
        let active = self.active.read().unwrap();
        let package = self.require_project(&active, project, trusted)?;
        Self::run_handler(package, name, input)
    }

    pub fn with_workflow<T>(
        &self,
        epoch: u64,
        reference: &str,
        trusted: &BTreeSet<String>,
        start: impl FnOnce(&str, &[ConfigurationRecord]) -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_ref()
            .filter(|p| trusted.contains(&p.digest))
            .ok_or_else(|| err("domain_inactive", "没有获准的活动领域"))?;
        require(
            package
                .sources
                .iter()
                .any(|row| row.r#type == "workflow" && row.id == reference),
            "当前领域没有该 Workflow 修订",
        )?;
        start(&package.digest, &package.sources)
    }

    fn projects(&self, digest: &str) -> Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT project FROM domain_projects WHERE digest=? ORDER BY project")
            .map_err(storage)?;
        query
            .query_map([digest], |row| row.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    pub fn permits_project(&self, project: &str, trusted: &BTreeSet<String>) -> Result<bool> {
        let active = self.active.read().unwrap();
        let saved = self.project_package(project)?;
        Ok(saved.is_none_or(|digest| {
            trusted.contains(&digest)
                && active
                    .package
                    .as_ref()
                    .is_some_and(|package| package.digest == digest)
        }))
    }

    fn project_package(&self, project: &str) -> Result<Option<String>> {
        self.db
            .lock()
            .unwrap()
            .query_row(
                "SELECT digest FROM domain_projects WHERE project=?",
                [project],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)
    }

    pub fn bind_project(&self, project: &str, digest: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO domain_projects VALUES(?,?)",
            params![project, digest],
        )
        .map_err(storage)?;
        let saved: String = db
            .query_row(
                "SELECT digest FROM domain_projects WHERE project=?",
                [project],
                |row| row.get(0),
            )
            .map_err(storage)?;
        require(saved == digest, "项目已冻结到其他领域包版本")
    }

    pub fn with_project<T>(
        &self,
        project: &str,
        epoch: u64,
        trusted: &BTreeSet<String>,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        self.require_project(&active, project, trusted)?;
        action()
    }

    fn require_project<'a>(
        &self,
        active: &'a Active,
        project: &str,
        trusted: &BTreeSet<String>,
    ) -> Result<&'a Package> {
        let package = active
            .package
            .as_deref()
            .filter(|p| trusted.contains(&p.digest))
            .ok_or_else(|| err("domain_inactive", "没有获准的活动领域"))?;
        let saved = self.project_package(project)?;
        require(
            saved.as_deref() == Some(&package.digest),
            "项目不属于当前领域版本",
        )?;
        Ok(package)
    }

    pub fn asset(
        &self,
        epoch: u64,
        digest: &str,
        path: &str,
        trusted: &BTreeSet<String>,
    ) -> Option<Vec<u8>> {
        let active = self.active.read().unwrap();
        if epoch != active.epoch
            || !path.starts_with("ui/web/")
            || !trusted.contains(digest)
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
