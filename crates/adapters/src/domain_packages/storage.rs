//! Optional external project storage. A project's root is fixed when it is bound.
use super::*;

const REGISTRY: &str = "domain-packages.sqlite3";
const OWNER: &str = ".devdesk-domain-storage";

pub const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS domain_storage(
        package_id TEXT PRIMARY KEY, data_root TEXT, revision INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS domain_project_storage(
        project TEXT PRIMARY KEY REFERENCES domain_projects(project), data_root TEXT NOT NULL);";

impl DomainPackages {
    /// Called under the active package lease held by the project opener.
    pub fn project_import_root(&self, digest: &str) -> Result<PathBuf> {
        let db = self.db.lock().unwrap();
        let package: String = db
            .query_row(
                "SELECT id FROM domain_packages WHERE digest=?",
                [digest],
                |row| row.get(0),
            )
            .map_err(storage)?;
        let (configured, _) = storage_setting(&db, &package)?;
        let Some(configured) = configured else {
            return Ok(self.root.clone());
        };
        let root = PathBuf::from(configured);
        require(
            root.is_absolute()
                && root.canonicalize().map_err(storage)? == root
                && read_owner(&root)? == json!({"host_root":self.root, "package_id":package}),
            "领域项目数据目录归属不符",
        )?;
        Ok(root)
    }

    pub fn storage(&self, epoch: u64) -> Result<Value> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_ref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        self.storage_view(&self.db.lock().unwrap(), package)
    }

    pub fn configure_storage(
        &self,
        epoch: u64,
        expected_revision: u64,
        path: Option<&Path>,
    ) -> Result<Value> {
        let active = self.active.read().unwrap();
        check_epoch(epoch, active.epoch)?;
        let package = active
            .package
            .as_ref()
            .ok_or_else(|| err("domain_inactive", "当前没有活动领域"))?;
        let db = self.db.lock().unwrap();
        let (current, revision) = storage_setting(&db, &package.manifest.id)?;
        if revision != expected_revision {
            return Err(err(
                "revision_conflict",
                "领域数据目录已修改，请重新读取设置",
            ));
        }
        let path = path
            .map(|path| claim_root(&self.root, &package.manifest.id, path))
            .transpose()?;
        let path = path.as_ref().and_then(|p| (p != &self.root).then_some(p));
        let path = path
            .map(|p| {
                p.to_str()
                    .ok_or_else(|| err("invalid_params", "目录路径需要使用 UTF-8"))
            })
            .transpose()?;
        if current.as_deref() != path {
            db.execute(
                "INSERT INTO domain_storage VALUES(?,?,?)
                 ON CONFLICT(package_id) DO UPDATE SET data_root=excluded.data_root, revision=excluded.revision",
                params![package.manifest.id, path, next_epoch(revision)?],
            ).map_err(storage)?;
        }
        self.storage_view(&db, package)
    }

    fn storage_view(&self, db: &Connection, package: &Package) -> Result<Value> {
        let (configured_root, revision) = storage_setting(db, &package.manifest.id)?;
        let mut query = db
            .prepare(
                "SELECT coalesce(s.data_root, ?1) AS root, count(*)
             FROM domain_projects d
             JOIN domain_packages p ON p.digest=d.digest
             LEFT JOIN domain_project_storage s ON s.project=d.project
             WHERE p.id=?2 GROUP BY root ORDER BY root",
            )
            .map_err(storage)?;
        let rows = query
            .query_map(params![self.root.to_str(), package.manifest.id], |r| {
                Ok(json!({"data_root":r.get::<_, String>(0)?, "projects":r.get::<_, u64>(1)?}))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        Ok(json!({
            "package_id":package.manifest.id, "revision":revision, "configured_root":configured_root,
            "data_root":configured_root.as_deref().or(self.root.to_str()), "project_roots":rows
        }))
    }
}

fn storage_setting(db: &Connection, package: &str) -> Result<(Option<String>, u64)> {
    Ok(db
        .query_row(
            "SELECT data_root,revision FROM domain_storage WHERE package_id=?",
            [package],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .unwrap_or_default())
}

fn claim_root(host: &Path, package: &str, path: &Path) -> Result<PathBuf> {
    require(path.is_absolute(), "请输入 Host 所在机器上的绝对路径")?;
    fs::create_dir_all(path).map_err(storage)?;
    let root = path.canonicalize().map_err(storage)?;
    if root == host {
        return Ok(root);
    }
    let marker = root.join(OWNER);
    reject_symlink(&marker)?;
    let owner = json!({"host_root":host, "package_id":package});
    if !marker.exists() {
        for entry in fs::read_dir(&root).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            require(
                entry.file_name() == ".DS_Store" && entry.file_type().map_err(storage)?.is_file(),
                "请使用空目录或已经由此 Host 和领域登记的目录",
            )?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&root).map_err(storage)?;
        serde_json::to_writer(&mut temporary, &owner).map_err(storage)?;
        temporary.as_file().sync_all().map_err(storage)?;
        if let Err(error) = temporary.persist_noclobber(&marker)
            && error.error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(storage(error.error));
        }
    }
    require(
        read_owner(&root)? == owner,
        "领域数据目录已由其他 Host 或领域登记",
    )?;
    reject_symlink(&root.join("projects"))?;
    Ok(root)
}

fn read_owner(root: &Path) -> Result<Value> {
    let marker = root.join(OWNER);
    reject_symlink(&marker)?;
    require(marker.is_file(), "领域数据目录不可用或缺少登记标记")?;
    serde_json::from_reader(fs::File::open(marker).map_err(storage)?).map_err(storage)
}

fn open_project_registry(host: &Path) -> Result<Option<Connection>> {
    let path = host.join(REGISTRY);
    reject_symlink(&path)?;
    if !path.try_exists().map_err(storage)? {
        return Ok(None);
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(storage)?;
    // Global dataset preflight also runs before an older registry is upgraded.
    let has_storage: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='domain_project_storage' AND type='table')",
        [],
        |r| r.get(0),
    ).map_err(storage)?;
    Ok(has_storage.then_some(db))
}

pub(crate) fn project_storage_root(host: &Path, project: &str) -> Result<Option<PathBuf>> {
    let Some(db) = open_project_registry(host)? else {
        return Ok(None);
    };
    let path: Option<String> = db
        .query_row(
            "SELECT data_root FROM domain_project_storage WHERE project=?",
            [project],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    path.map(|path| {
        let root = PathBuf::from(path);
        require(
            root.is_absolute()
                && root.canonicalize().map_err(storage)? == root
                && read_owner(&root)?["host_root"].as_str() == host.to_str(),
            "领域项目数据目录归属不符",
        )?;
        Ok(root)
    })
    .transpose()
}

pub(crate) fn externally_stored_projects(host: &Path) -> Result<Vec<Id>> {
    let Some(db) = open_project_registry(host)? else {
        return Ok(Vec::new());
    };
    let mut query = db
        .prepare("SELECT project FROM domain_project_storage")
        .map_err(storage)?;
    query
        .query_map([], |row| row.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage)
}
