//! Optional external project storage. A project's root is fixed when it is bound.
use super::*;

const REGISTRY: &str = "domain-packages.sqlite3";
const OWNER: &str = ".devdesk-domain-storage";
pub const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS domain_storage(
        package_id TEXT PRIMARY KEY, data_root TEXT, revision INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS domain_project_storage(
        project TEXT PRIMARY KEY REFERENCES domain_projects(project), data_root TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS domain_detached_projects(
        project TEXT PRIMARY KEY REFERENCES domain_projects(project));";

/// Older registries discarded the manifest on uninstall. An external project's
/// pinned root still carries the original domain ID in its ownership marker.
pub(super) fn restore_package_identities(db: &Connection, host: &Path) -> Result<()> {
    let tx = db.unchecked_transaction().map_err(storage)?;
    let candidates = {
        let mut query = tx
            .prepare(
                "SELECT DISTINCT d.digest,s.data_root FROM domain_projects d
             JOIN domain_project_storage s ON s.project=d.project
             LEFT JOIN domain_package_identities p ON p.digest=d.digest
             WHERE p.digest IS NULL",
            )
            .map_err(storage)?;
        query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    let mut recovered = BTreeMap::<String, Option<String>>::new();
    for (digest, path) in candidates {
        let Ok(owner) = read_owner(Path::new(&path)) else {
            continue;
        };
        let Some(id) = owner["package_id"].as_str().filter(|id| is_identifier(id)) else {
            continue;
        };
        if owner["host_root"].as_str() != host.to_str() {
            continue;
        }
        recovered
            .entry(digest)
            .and_modify(|previous| {
                if previous.as_deref() != Some(id) {
                    *previous = None;
                }
            })
            .or_insert_with(|| Some(id.to_owned()));
    }
    for (digest, id) in recovered {
        if let Some(id) = id {
            tx.execute(
                "INSERT INTO domain_package_identities VALUES(?,?)",
                params![digest, id],
            )
            .map_err(storage)?;
        }
    }
    tx.commit().map_err(storage)
}

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
        validate_root(&self.root, &root, Some(&package))?;
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

    #[expect(
        clippy::readonly_write_lock,
        reason = "exclude project opens while changing storage bindings"
    )]
    pub fn configure_storage(
        &self,
        global: &crate::GlobalStore,
        epoch: u64,
        expected_revision: u64,
        path: Option<&Path>,
    ) -> Result<(Value, Vec<Id>)> {
        let active = self.active.write().unwrap();
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
        let configured = path
            .map(|p| {
                p.to_str()
                    .ok_or_else(|| err("invalid_params", "目录路径需要使用 UTF-8"))
            })
            .transpose()?;
        let tx = db.unchecked_transaction().map_err(storage)?;
        recover_project_roots(
            &tx,
            &self.root,
            &package.manifest.id,
            path.unwrap_or(&self.root),
            Some(global),
        )?;
        if current.as_deref() != configured {
            tx.execute(
                "INSERT INTO domain_storage VALUES(?,?,?)
                 ON CONFLICT(package_id) DO UPDATE SET data_root=excluded.data_root, revision=excluded.revision",
                params![package.manifest.id, configured, next_epoch(revision)?],
            ).map_err(storage)?;
        }
        forget_missing_removed_roots(&tx, global)?;
        tx.commit().map_err(storage)?;
        let view = self.storage_view(&db, package)?;
        drop(db);
        Ok((view, self.projects(&package.manifest.id)?))
    }

    /// Run after startup recovery, before opening the registered projects.
    pub fn forget_missing_removed_roots(&self, global: &crate::GlobalStore) -> Result<()> {
        let db = self.db.lock().unwrap();
        let tx = db.unchecked_transaction().map_err(storage)?;
        forget_missing_removed_roots(&tx, global)?;
        tx.commit().map_err(storage)
    }

    /// Retry discovery when a project was copied into the configured root after startup.
    pub fn recover_project_from_storage(&self, digest: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        let package: String = db
            .query_row(
                "SELECT id FROM domain_packages WHERE digest=?",
                [digest],
                |row| row.get(0),
            )
            .map_err(storage)?;
        recover_configured_projects(&db, &self.root, &package)
    }

    fn storage_view(&self, db: &Connection, package: &Package) -> Result<Value> {
        let (configured_root, revision) = storage_setting(db, &package.manifest.id)?;
        let mut query = db
            .prepare(
                "SELECT d.project, coalesce(s.data_root, ?1)
             FROM domain_projects d
             JOIN domain_package_identities p ON p.digest=d.digest
             LEFT JOIN domain_project_storage s ON s.project=d.project
             WHERE p.id=?2 AND NOT EXISTS(
                 SELECT 1 FROM domain_detached_projects x WHERE x.project=d.project)",
            )
            .map_err(storage)?;
        let projects = query
            .query_map(params![self.root.to_str(), package.manifest.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        let mut roots = BTreeMap::<String, usize>::new();
        for project in projects {
            let (id, root) = project.map_err(storage)?;
            if uuid::Uuid::parse_str(&id).is_ok()
                && SqliteStore::project_path_in(Path::new(&root), &id)
                    .is_ok_and(|path| path.join("engineering.sqlite3").is_file())
            {
                *roots.entry(root).or_default() += 1;
            }
        }
        let rows: Vec<_> = roots
            .into_iter()
            .filter(|(root, _)| {
                let root = Path::new(root);
                root == self.root
                    || validate_root(&self.root, root, Some(&package.manifest.id)).is_ok()
            })
            .map(|(root, projects)| json!({"data_root":root, "projects":projects}))
            .collect();
        Ok(json!({
            "package_id":package.manifest.id, "revision":revision, "configured_root":configured_root,
            "available": configured_root.as_deref().is_none_or(|path|
                validate_root(&self.root, Path::new(path), Some(&package.manifest.id)).is_ok()),
            "data_root":configured_root.as_deref().or(self.root.to_str()), "project_roots":rows
        }))
    }
}

fn forget_missing_removed_roots(db: &Connection, global: &crate::GlobalStore) -> Result<()> {
    let mut query = db
        .prepare(
            "SELECT s.project,s.data_root FROM domain_project_storage s
             JOIN domain_projects d ON d.project=s.project
             JOIN domain_package_identities p ON p.digest=d.digest
             LEFT JOIN domain_storage c ON c.package_id=p.id
             WHERE s.data_root IS NOT c.data_root",
        )
        .map_err(storage)?;
    let candidates = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (id, root) in candidates {
        if fs::symlink_metadata(root)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            && global.project_removed(&id)?
        {
            db.execute(
                "INSERT OR IGNORE INTO domain_detached_projects VALUES(?)",
                [&id],
            )
            .map_err(storage)?;
            db.execute("DELETE FROM domain_project_storage WHERE project=?", [&id])
                .map_err(storage)?;
        }
    }
    Ok(())
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
    let metadata = fs::metadata(&marker).map_err(|error| root_error(root, error))?;
    require(metadata.is_file(), "领域数据目录登记标记无效")?;
    let file = fs::File::open(marker).map_err(|error| root_error(root, error))?;
    serde_json::from_reader(file).map_err(storage)
}

fn root_error(root: &Path, error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => err(
            DOMAIN_STORAGE_UNAVAILABLE,
            format!("找不到领域数据目录或登记标记：{}", root.display()),
        ),
        _ => storage(error),
    }
}

fn validate_root(host: &Path, root: &Path, package: Option<&str>) -> Result<()> {
    let mismatch = "领域项目数据目录归属不符";
    require(root.is_absolute(), mismatch)?;
    let canonical = root
        .canonicalize()
        .map_err(|error| root_error(root, error))?;
    require(canonical == root, mismatch)?;
    let owner = read_owner(root)?;
    require(
        match package {
            Some(id) => owner == json!({"host_root":host,"package_id":id}),
            None => owner["host_root"].as_str() == host.to_str(),
        },
        mismatch,
    )
}

pub(super) fn require_configured_storage(
    db: &Connection,
    host: &Path,
    package: &str,
) -> Result<()> {
    if let Some(path) = storage_setting(db, package)?.0 {
        validate_root(host, Path::new(&path), Some(package))?;
    }
    Ok(())
}

pub(super) fn recover_configured_projects(
    db: &Connection,
    host: &Path,
    package: &str,
) -> Result<()> {
    let configured = storage_setting(db, package)?.0;
    let target = configured.as_deref().map_or(host, Path::new);
    if target != host {
        match validate_root(host, target, Some(package)) {
            Err(error) if error.code == DOMAIN_STORAGE_UNAVAILABLE => return Ok(()),
            result => result?,
        }
    }
    let tx = db.unchecked_transaction().map_err(storage)?;
    recover_project_roots(&tx, host, package, target, None)?;
    tx.commit().map_err(storage)
}

fn recover_project_roots(
    db: &Connection,
    host: &Path,
    package: &str,
    target: &Path,
    global: Option<&crate::GlobalStore>,
) -> Result<()> {
    let mut query = db
        .prepare(
            "SELECT d.project,s.data_root FROM domain_projects d
         JOIN domain_package_identities p ON p.digest=d.digest
         LEFT JOIN domain_project_storage s ON s.project=d.project
         LEFT JOIN domain_detached_projects x ON x.project=d.project
         WHERE p.id=?1 AND (s.data_root<>?2 OR (?3 AND x.project IS NOT NULL)) ORDER BY d.project",
        )
        .map_err(storage)?;
    let projects = query
        .query_map(params![package, target.to_str(), global.is_some()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (id, old) in projects {
        require(uuid::Uuid::parse_str(&id).is_ok(), "无效的领域项目 ID")?;
        let new = SqliteStore::project_path_in(target, &id)?;
        if !new.join("engineering.sqlite3").is_file() {
            continue;
        }
        let old = if let Some(old) = old {
            PathBuf::from(old)
        } else {
            // An explicit save can reattach a backup using the published checkout
            // path to verify the move, even after forgetting the storage binding.
            let Some(project) = global
                .map(|store| store.project(&id))
                .transpose()?
                .flatten()
            else {
                continue;
            };
            let repository = Path::new(&project.workspace);
            let Some(root) = repository
                .ancestors()
                .nth(3)
                .filter(|root| root.is_absolute())
            else {
                continue;
            };
            if repository != root.join("projects").join(&id).join("repository") {
                continue;
            }
            root.to_path_buf()
        };
        let old_project = SqliteStore::project_path_in(&old, &id)?;
        if old != target
            && old_project
                .join("engineering.sqlite3")
                .try_exists()
                .map_err(storage)?
        {
            continue;
        }
        match validate_root(host, &old, Some(package)) {
            Err(error) if error.code == DOMAIN_STORAGE_UNAVAILABLE => {}
            result => result?,
        }
        crate::project_migration::relocate(host, &id, &old_project, &new)?;
        if target == host {
            db.execute("DELETE FROM domain_project_storage WHERE project=?", [&id])
                .map_err(storage)?;
        } else {
            db.execute(
                "INSERT INTO domain_project_storage VALUES(?,?)
                 ON CONFLICT(project) DO UPDATE SET data_root=excluded.data_root",
                params![id, target.to_str()],
            )
            .map_err(storage)?;
        }
        db.execute(
            "DELETE FROM domain_detached_projects WHERE project=?",
            [&id],
        )
        .map_err(storage)?;
    }
    Ok(())
}

fn open_project_registry(host: &Path) -> Result<Option<(Connection, bool)>> {
    let path = host.join(REGISTRY);
    reject_symlink(&path)?;
    if !path.try_exists().map_err(storage)? {
        return Ok(None);
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(storage)?;
    // Global dataset preflight also runs before an older registry is upgraded.
    let (has_storage, has_detached): (bool, bool) = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='domain_project_storage' AND type='table'),
                EXISTS(SELECT 1 FROM sqlite_schema WHERE name='domain_detached_projects' AND type='table')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(storage)?;
    Ok(has_storage.then_some((db, has_detached)))
}

pub(crate) fn project_storage_root(host: &Path, project: &str) -> Result<Option<PathBuf>> {
    let Some((db, has_detached)) = open_project_registry(host)? else {
        return Ok(None);
    };
    let sql = if has_detached {
        "SELECT data_root FROM domain_project_storage WHERE project=?1
         UNION ALL SELECT NULL FROM domain_detached_projects WHERE project=?1"
    } else {
        "SELECT data_root FROM domain_project_storage WHERE project=?1"
    };
    let location: Option<Option<String>> = db
        .query_row(sql, [project], |r| r.get(0))
        .optional()
        .map_err(storage)?;
    match location {
        Some(Some(path)) => {
            let root = PathBuf::from(path);
            validate_root(host, &root, None)?;
            Ok(Some(root))
        }
        Some(None) => Err(err(
            DOMAIN_STORAGE_UNAVAILABLE,
            "已移除项目的数据目录已取消登记",
        )),
        None => Ok(None),
    }
}

pub(crate) fn externally_stored_projects(host: &Path) -> Result<Vec<Id>> {
    let Some((db, _)) = open_project_registry(host)? else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obsolete_removed_root_is_forgotten_and_a_backup_can_be_reattached() {
        let temp = tempfile::tempdir().unwrap();
        let global = crate::GlobalStore::open(temp.path()).unwrap();
        let host = PathBuf::from(global.root());
        let domain = "software-development";
        let old = claim_root(&host, domain, &host.join("old")).unwrap();
        let current = claim_root(&host, domain, &host.join("current")).unwrap();
        let unavailable = claim_root(&host, domain, &host.join("unavailable")).unwrap();
        let removed = id();
        let checkout = SqliteStore::project_path_in(&old, &removed).unwrap();
        let repo = checkout.join("repository");
        let store = SqliteStore::open(&checkout).unwrap();
        let conn = Connection::open(checkout.join("engineering.sqlite3")).unwrap();
        conn.execute(
            "INSERT INTO projects(id,title,workspace,execution_mode,concurrency,revision,workspace_definition)
             VALUES(?1,'Archived',?2,'live',2,3,json_object('repository','owner/repo','mainline','main','path',?2))",
            params![removed, repo.to_str().unwrap()],
        )
        .unwrap();
        let project = crate::execution_read::project(&conn, &removed).unwrap();
        global.reserve_copied_project(&project).unwrap();
        global.publish_project(&project).unwrap();
        drop(conn);
        drop(store);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "--bare", repo.to_str().unwrap()]);
        git(&[
            "-C",
            repo.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            "https://github.com/owner/repo.git",
        ]);
        let global_db = Connection::open(host.join("global.sqlite3")).unwrap();
        global_db
            .execute(
                "INSERT INTO settings(key,value) VALUES(?1,'true')",
                [crate::GlobalStore::project_removal_key(&removed).id],
            )
            .unwrap();
        drop(global_db);
        let available = id();
        SqliteStore::open(&SqliteStore::project_path_in(&current, &available).unwrap()).unwrap();
        let active = id();
        {
            let packages = DomainPackages::open(&host).unwrap();
            let digest = packages
                .install(include_bytes!(
                    "../../../../tests/fixtures/domain-packages/software-development-0.3.0.zip"
                ))
                .unwrap()["digest"]
                .as_str()
                .unwrap()
                .to_owned();
            let db = packages.db.lock().unwrap();
            db.execute(
                "UPDATE active_domain SET epoch=1,digest=? WHERE singleton=1",
                [&digest],
            )
            .unwrap();
            db.execute(
                "INSERT INTO domain_storage VALUES(?,?,1)",
                params![domain, current.to_str()],
            )
            .unwrap();
            for (id, root) in [
                (&removed, &old),
                (&available, &current),
                (&active, &unavailable),
            ] {
                db.execute(
                    "INSERT INTO domain_projects VALUES(?,?)",
                    params![id, digest],
                )
                .unwrap();
                db.execute(
                    "INSERT INTO domain_project_storage VALUES(?,?)",
                    params![id, root.to_str()],
                )
                .unwrap();
            }
        }
        let backup = host.join("backup");
        fs::rename(&old, &backup).unwrap();
        fs::remove_file(unavailable.join(OWNER)).unwrap();
        fs::remove_dir(&unavailable).unwrap();
        let packages = DomainPackages::open(&host).unwrap();
        let settings = packages.storage(1).unwrap();
        assert_eq!(
            settings["project_roots"],
            json!([{"data_root":current,"projects":1}])
        );
        packages.forget_missing_removed_roots(&global).unwrap();
        packages.forget_missing_removed_roots(&global).unwrap();
        assert!(global.removed_project_missing(&removed).unwrap());
        assert!(global.project(&removed).unwrap().is_some());
        assert!(
            global
                .domain_github_project(
                    "owner/repo",
                    "main",
                    &packages.project_visibility(Some(1)).unwrap()
                )
                .unwrap()
                .is_none()
        );
        let db = packages.db.lock().unwrap();
        let registered_root = |id: &str| -> Option<String> {
            db.query_row(
                "SELECT data_root FROM domain_project_storage WHERE project=?",
                [id],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
        };
        assert_eq!(registered_root(&removed), None);
        assert_eq!(registered_root(&active).as_deref(), unavailable.to_str());
        drop(db);
        let (settings, _) = packages
            .configure_storage(&global, 1, 1, Some(&backup))
            .unwrap();
        assert_eq!(
            settings["project_roots"],
            json!([{"data_root":backup,"projects":1},{"data_root":current,"projects":1}])
        );
        assert!(!global.removed_project_missing(&removed).unwrap());
        let moved = SqliteStore::project_path(&host, &removed).unwrap();
        let conn = Connection::open(moved.join("engineering.sqlite3")).unwrap();
        let project = crate::execution_read::project(&conn, &removed).unwrap();
        assert_eq!(Path::new(&project.workspace), moved.join("repository"));
    }

    #[test]
    fn missing_configured_directory_deactivates_package_without_any_projects() {
        crate::factory_bundle::initialize().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let host = temp.path().canonicalize().unwrap();
        let external = claim_root(&host, "software-development", &host.join("external")).unwrap();
        let digest = {
            let packages = DomainPackages::open(&host).unwrap();
            let archive = include_bytes!(
                "../../../../tests/fixtures/domain-packages/software-development-0.3.0.zip"
            );
            let digest = packages.install(archive).unwrap()["digest"]
                .as_str()
                .unwrap()
                .to_owned();
            let db = packages.db.lock().unwrap();
            db.execute(
                "UPDATE active_domain SET epoch=1,digest=? WHERE singleton=1",
                [&digest],
            )
            .unwrap();
            db.execute(
                "INSERT INTO domain_storage VALUES('software-development',?,1)",
                [external.to_str().unwrap()],
            )
            .unwrap();
            digest
        };
        fs::rename(&external, host.join("unmounted")).unwrap();
        for _ in 0..2 {
            let packages = DomainPackages::open(&host).unwrap();
            assert_eq!(packages.status()["active"], Value::Null);
            assert_eq!(packages.epoch(), 2);
            assert_eq!(packages.installed().unwrap()[0]["digest"], digest);
            let db = packages.db.lock().unwrap();
            assert_eq!(
                storage_setting(&db, "software-development").unwrap(),
                (Some(external.to_str().unwrap().to_owned()), 1)
            );
        }
        assert!(!external.exists());
    }

    #[test]
    fn legacy_external_projects_recover_their_domain_after_the_zip_was_removed() {
        let host_dir = tempfile::tempdir().unwrap();
        let host = host_dir.path().canonicalize().unwrap();
        let project_dir = tempfile::tempdir().unwrap();
        let external = claim_root(&host, "software-development", project_dir.path()).unwrap();
        let digest = "ab".repeat(32);
        {
            let packages = DomainPackages::open(&host).unwrap();
            let db = packages.db.lock().unwrap();
            db.execute(
                "INSERT INTO domain_projects VALUES('old-project',?)",
                [&digest],
            )
            .unwrap();
            db.execute(
                "INSERT INTO domain_project_storage VALUES('old-project',?)",
                [external.to_str().unwrap()],
            )
            .unwrap();
        }
        let packages = DomainPackages::open(&host).unwrap();
        assert_eq!(packages.installed().unwrap(), json!([]));
        assert_eq!(
            packages.projects("software-development").unwrap(),
            ["old-project"]
        );
        assert!(packages.projects("manuscript").unwrap().is_empty());
        assert!(
            !packages
                .project_visibility(None)
                .unwrap()
                .includes("old-project")
        );
        assert_eq!(
            project_storage_root(&host, "old-project").unwrap(),
            Some(external)
        );
    }
}
