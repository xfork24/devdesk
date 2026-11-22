//! Session caches use an index while idle and private files while a worker runs.
use super::*;
use chrono::{DateTime, FixedOffset};
use std::{
    collections::{BTreeMap, HashMap},
    os::unix::fs::MetadataExt,
};

const CACHE_DIRS: &[&str] = &[
    "codex/plugins/cache",
    "codex/cache/codex_apps_tools",
    "codex/cache/codex_apps_server_info",
    "codex/skills/.system",
    "runtime/node/node_modules",
];
const REMOTE: &str = "cache/remote_plugin_catalog";
const MODEL_FILE: &str = "models_cache.json";
type CacheIndex = BTreeMap<String, String>;

pub(crate) struct CodexCache {
    data: PathBuf,
    gate: Mutex<()>,
}

impl CodexCache {
    pub(crate) fn new(data: &Path) -> Self {
        Self {
            data: data.to_owned(),
            gate: Mutex::new(()),
        }
    }

    fn objects(&self) -> Result<PathBuf> {
        cache_subdir(&self.data, "shared-cache/codex/objects")
    }

    fn remote(&self, account: &str) -> Result<PathBuf> {
        cache_subdir(
            &self.data,
            &format!(
                "shared-cache/codex/remote-catalog/{}",
                account_key(account)?
            ),
        )
    }

    fn model_account(&self, account: &str) -> Result<PathBuf> {
        cache_subdir(
            &self.data,
            &format!("shared-cache/codex/models/{}", account_key(account)?),
        )
    }

    fn models(&self, account: &str, version: &str) -> Result<PathBuf> {
        cache_subdir(
            &self.model_account(account)?,
            &configuration_hash(&json!(version)),
        )
    }

    pub(crate) fn prepare(&self, home: &Path, account: &str) -> Result<()> {
        let _guard = self.gate.lock().map_err(storage)?;
        let (state, index_file) = session_cache_paths(home)?;
        let objects = self.objects()?;
        for (relative, key) in read_index(&index_file)? {
            let target = cached_path(state, &relative)?;
            let object = objects.join(key);
            reject_symlink(&object)?;
            match target.symlink_metadata() {
                Ok(meta) if meta.is_file() && meta.nlink() == 1 => (),
                Ok(meta) if meta.is_file() => replace(&target, |temp| copy_private(&target, temp))?,
                Ok(_) => return Err(err("storage", "会话缓存目标不是普通文件")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(target.parent().unwrap()).map_err(storage)?;
                    replace(&target, |temp| copy_private(&object, temp))?;
                }
                Err(e) => return Err(storage(e)),
            }
        }
        // Compatibility with the existing idle folders which still contain
        // hard links. Worker-visible files always use separate inodes.
        for cache in CACHE_DIRS {
            visit_files(&cache_subdir(state, cache)?, &mut |path, meta| {
                if meta.nlink() > 1 {
                    replace(path, |temp| copy_private(path, temp))?;
                }
                Ok(())
            })?;
        }
        remove_cached_file(&index_file)?;
        self.prepare_remote(home, account)?;
        self.prepare_models(home, account)
    }

    fn prepare_remote(&self, home: &Path, account: &str) -> Result<()> {
        let local = cache_subdir(home, REMOTE)?;
        for entry in entries(&self.remote(account)?)? {
            if !catalog_file(&entry)? {
                continue;
            }
            let target = local.join(entry.file_name());
            reject_symlink(&target)?;
            if !target.exists() {
                private_directory(&local)?;
                let source = entry.path();
                let modified = entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .map_err(storage)?;
                publish_catalog(&source, &target, modified)?;
            }
        }
        Ok(())
    }

    pub(crate) fn retire(&self, home: &Path, account: &str) -> Result<()> {
        let _guard = self.gate.lock().map_err(storage)?;
        self.index_idle_files(home)?;
        self.retire_remote(home, account)?;
        self.retire_models(home, account)
    }

    fn index_idle_files(&self, home: &Path) -> Result<()> {
        let (state, index_file) = session_cache_paths(home)?;
        let mut index = read_index(&index_file)?;
        let objects = self.objects()?;
        let mut known = HashMap::new();
        for entry in entries(&objects)? {
            let name = entry.file_name();
            if let Some(key) = name.to_str().filter(|key| valid_object_key(key))
                && entry.file_type().map_err(storage)?.is_file()
            {
                let meta = entry.metadata().map_err(storage)?;
                known.insert((meta.dev(), meta.ino()), key.to_owned());
            }
        }
        let mut retired = Vec::new();
        let mut objects_ready = objects.is_dir();
        for cache in CACHE_DIRS {
            visit_files(&cache_subdir(state, cache)?, &mut |path, meta| {
                let Some(relative) = path.strip_prefix(state).ok().and_then(Path::to_str) else {
                    return Ok(());
                };
                if path.file_name().is_some_and(|name| name == ".DS_Store") {
                    remove_cached_file(path)?;
                    return Ok(());
                }
                let key = if let Some(key) = known.get(&(meta.dev(), meta.ino())) {
                    key.clone()
                } else {
                    let key = format!("{:o}-{}", meta.mode() & 0o777, digest(path)?);
                    if !objects_ready {
                        private_directory(&objects)?;
                        objects_ready = true;
                    }
                    let object = objects.join(&key);
                    reject_symlink(&object)?;
                    if !object.is_file() {
                        replace(&object, |temp| {
                            copy_private(path, temp)?;
                            File::open(temp)?.sync_all()
                        })?;
                        File::open(&objects)
                            .and_then(|dir| dir.sync_all())
                            .map_err(storage)?;
                    }
                    key
                };
                index.insert(relative.to_owned(), key);
                retired.push(path.to_owned());
                Ok(())
            })?;
        }
        if !retired.is_empty() {
            let bytes = serde_json::to_vec(&index).map_err(storage)?;
            write_index(&index_file, &bytes)?;
            for path in retired {
                remove_cached_file(&path)?;
            }
        }
        Ok(())
    }

    fn retire_remote(&self, home: &Path, account: &str) -> Result<()> {
        let local = cache_subdir(home, REMOTE)?;
        let remote = self.remote(account)?;
        for entry in entries(&local)? {
            if !catalog_file(&entry)? {
                continue;
            }
            let source = entry.path();
            let target = remote.join(entry.file_name());
            reject_symlink(&target)?;
            let current = target.metadata().and_then(|meta| meta.modified());
            let candidate = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map_err(storage)?;
            match current {
                Ok(timestamp) if timestamp >= candidate => (),
                Ok(_) => publish_catalog(&source, &target, candidate)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    private_directory(&remote)?;
                    publish_catalog(&source, &target, candidate)?;
                }
                Err(e) => return Err(storage(e)),
            }
            remove_cached_file(&source)?;
        }
        Ok(())
    }

    fn retire_models(&self, home: &Path, account: &str) -> Result<()> {
        let local = cache_subdir(home, MODEL_FILE)?;
        let Some(incoming) = read_model_cache(&local)? else {
            return Ok(());
        };
        let directory = self.models(account, &incoming.client_version)?;
        let file = directory.join(format!(
            "{}.json",
            configuration_hash(&json!(incoming.identity))
        ));
        if read_model_cache(&file)?.is_none_or(|saved| incoming.fetched_at > saved.fetched_at) {
            private_directory(&directory)?;
            publish_catalog(
                &local,
                &file,
                local
                    .metadata()
                    .and_then(|m| m.modified())
                    .map_err(storage)?,
            )?;
        }
        remove_cached_file(&local)
    }

    fn prepare_models(&self, home: &Path, account: &str) -> Result<()> {
        let local = cache_subdir(home, MODEL_FILE)?;
        if local.exists() {
            return Ok(());
        }
        let Some(version) = self.session_version(home, account)? else {
            return Ok(());
        };
        let directory = self.models(account, &version)?;
        let candidates = entries(&directory)?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|kind| kind.is_file())
                    .map(|_| entry.path())
            })
            .collect::<Vec<_>>();
        // The provider authenticates the opaque identity itself. With several
        // identities for the frozen account/version, let it fetch its own entry.
        if let [source] = candidates.as_slice()
            && read_model_cache(source)?.is_some_and(|entry| entry.client_version == version)
        {
            publish_catalog(
                source,
                &local,
                source
                    .metadata()
                    .and_then(|m| m.modified())
                    .map_err(storage)?,
            )?;
        }
        Ok(())
    }

    fn session_version(&self, home: &Path, account: &str) -> Result<Option<String>> {
        if !self.model_account(account)?.is_dir() {
            return Ok(None);
        }
        let relative = home.strip_prefix(&self.data).map_err(storage)?;
        let components = relative
            .iter()
            .filter_map(|part| part.to_str())
            .collect::<Vec<_>>();
        let (root, session) = match components.as_slice() {
            [
                "projects",
                project,
                "execution",
                "sessions",
                session,
                "state",
                "codex",
            ] => (SqliteStore::project_path(&self.data, project)?, *session),
            [
                "conversations",
                "global",
                "execution",
                "sessions",
                session,
                "state",
                "codex",
            ] => (SqliteStore::global_discussion_path(&self.data)?, *session),
            _ => return Ok(None),
        };
        let db = cache_subdir(&root, "engineering.sqlite3")?;
        if !db.is_file() {
            return Ok(None);
        }
        let connection = connection_pool::ManagedConnection::read_only(&db)?;
        use rusqlite::OptionalExtension;
        connection
            .query_row(
                "SELECT json_extract(runtime_binding,'$.installation.key.version') FROM sessions
             WHERE id=?1 AND json_extract(runtime_binding,'$.credential_ref')=?2",
                [session, account],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(storage)
    }
}

fn account_key(account: &str) -> Result<&str> {
    account
        .strip_prefix("codex:")
        .filter(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| err("storage", "无效的 Codex 缓存账户引用"))
}

fn session_cache_paths(home: &Path) -> Result<(&Path, PathBuf)> {
    let state = home
        .parent()
        .ok_or_else(|| err("storage", "会话状态目录缺失"))?;
    let session = state
        .parent()
        .ok_or_else(|| err("storage", "会话目录缺失"))?;
    reject_symlink(state)?;
    Ok((
        state,
        cache_subdir(session, "coordinator/cache-index.json")?,
    ))
}

fn valid_object_key(key: &str) -> bool {
    key.split_once('-').is_some_and(|(mode, digest)| {
        !mode.is_empty()
            && mode.len() <= 3
            && mode.bytes().all(|b| (b'0'..=b'7').contains(&b))
            && digest.len() == 64
            && digest.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

fn cached_path(state: &Path, relative: &str) -> Result<PathBuf> {
    require(
        relative
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
            && CACHE_DIRS.iter().any(|dir| {
                relative
                    .strip_prefix(dir)
                    .is_some_and(|tail| tail.starts_with('/'))
            }),
        "会话缓存索引路径越界",
    )?;
    cache_subdir(state, relative)
}

fn read_index(path: &Path) -> Result<CacheIndex> {
    let index: CacheIndex = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(storage)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CacheIndex::new()),
        Err(e) => return Err(storage(e)),
    };
    require(
        index.values().all(|key| valid_object_key(key)),
        "会话缓存索引对象身份无效",
    )?;
    Ok(index)
}

fn write_index(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    private_directory(path.parent().unwrap())?;
    replace(path, |temp| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })?;
    File::open(path.parent().unwrap())
        .and_then(|dir| dir.sync_all())
        .map_err(storage)
}

fn remove_cached_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(storage(e)),
    }
}

#[derive(serde::Deserialize)]
struct ModelCache {
    client_version: String,
    identity: String,
    #[serde(deserialize_with = "model_timestamp")]
    fetched_at: DateTime<FixedOffset>,
    models: Vec<serde::de::IgnoredAny>,
}

fn model_timestamp<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<DateTime<FixedOffset>, D::Error> {
    DateTime::parse_from_rfc3339(&<String as serde::Deserialize>::deserialize(input)?)
        .map_err(serde::de::Error::custom)
}

fn read_model_cache(path: &Path) -> Result<Option<ModelCache>> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice::<ModelCache>(&bytes)
            .ok()
            .filter(|m| {
                !m.client_version.is_empty() && !m.identity.is_empty() && !m.models.is_empty()
            })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(storage(e)),
    }
}

fn catalog_file(entry: &fs::DirEntry) -> Result<bool> {
    Ok(entry.file_type().map_err(storage)?.is_file()
        && entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .is_some_and(|key| key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit())))
}

fn publish_catalog(source: &Path, target: &Path, fetched: std::time::SystemTime) -> Result<()> {
    replace(target, |temp| {
        copy_private(source, temp)?;
        let file = File::options().write(true).open(temp)?;
        file.set_modified(fetched)?;
        file.sync_all()
    })?;
    File::open(target.parent().unwrap())
        .and_then(|dir| dir.sync_all())
        .map_err(storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(file: &Path, content: impl AsRef<[u8]>) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    #[test]
    fn idle_index_reuses_node_and_skill_files_and_restores_private_copies() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CodexCache::new(dir.path());
        let account = format!("codex:{}", "a".repeat(64));
        let first = dir.path().join("first/state/codex");
        let second = dir.path().join("second/state/codex");
        for home in [&first, &second] {
            put(&home.join("skills/.system/skill/SKILL.md"), b"system skill");
            put(
                &home
                    .parent()
                    .unwrap()
                    .join("runtime/node/node_modules/pkg/index.js"),
                b"package",
            );
        }
        let private = first.parent().unwrap().join("runtime/node/.npmrc");
        put(&private, b"private configuration");
        for home in [&first, &second] {
            cache.retire(home, &account).unwrap();
            cache.retire(home, &account).unwrap();
            assert!(!home.join("skills/.system/skill/SKILL.md").exists());
            assert!(
                !home
                    .parent()
                    .unwrap()
                    .join("runtime/node/node_modules/pkg/index.js")
                    .exists()
            );
            assert!(session_cache_paths(home).unwrap().1.is_file());
        }
        assert_eq!(entries(&cache.objects().unwrap()).unwrap().len(), 2);
        assert!(private.exists());
        cache.prepare(&first, &account).unwrap();
        let package = first
            .parent()
            .unwrap()
            .join("runtime/node/node_modules/pkg/index.js");
        assert_eq!(fs::read(&package).unwrap(), b"package");
        fs::write(&package, b"edited privately").unwrap();
        cache.prepare(&second, &account).unwrap();
        assert_eq!(
            fs::read(
                second
                    .parent()
                    .unwrap()
                    .join("runtime/node/node_modules/pkg/index.js")
            )
            .unwrap(),
            b"package"
        );
        assert_eq!(
            fs::read(first.join("skills/.system/skill/SKILL.md")).unwrap(),
            b"system skill"
        );
        assert!(!session_cache_paths(&first).unwrap().1.exists());
    }

    #[test]
    fn legacy_hard_link_is_removed_from_idle_session_and_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CodexCache::new(dir.path());
        let account = format!("codex:{}", "a".repeat(64));
        let home = dir.path().join("session/state/codex");
        let file = home.join("plugins/cache/plugin/package");
        let content = b"installed plugin";
        let object = cache
            .objects()
            .unwrap()
            .join(format!("644-{:x}", Sha256::digest(content)));
        put(&object, content);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::hard_link(&object, &file).unwrap();
        cache.retire(&home, &account).unwrap();
        assert_eq!(object.metadata().unwrap().nlink(), 1);
        assert!(!file.exists());
        cache.prepare(&home, &account).unwrap();
        assert_eq!(fs::read(&file).unwrap(), content);
        assert_ne!(
            file.metadata().unwrap().ino(),
            object.metadata().unwrap().ino()
        );
    }

    #[test]
    fn models_keep_latest_bytes_and_load_only_for_the_frozen_account_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().canonicalize().unwrap();
        let cache = CodexCache::new(&data);
        let account = format!("codex:{}", "a".repeat(64));
        let other = format!("codex:{}", "b".repeat(64));
        let project = data.join("projects").join(id());
        fs::create_dir_all(&project).unwrap();
        let db = Connection::open(project.join("engineering.sqlite3")).unwrap();
        db.execute_batch(
            "CREATE TABLE sessions(id TEXT PRIMARY KEY, runtime_binding TEXT NOT NULL)",
        )
        .unwrap();
        let session = |account: &str, version: &str| {
            let id = id();
            let binding =
                json!({"installation":{"key":{"version":version}},"credential_ref":account});
            db.execute(
                "INSERT INTO sessions VALUES (?1,?2)",
                [&id, &binding.to_string()],
            )
            .unwrap();
            let home = project
                .join("execution/sessions")
                .join(id)
                .join("state/codex");
            fs::create_dir_all(&home).unwrap();
            home
        };
        let body = |version: &str, fetched: &str, etag: &str| {
            json!({"client_version":version,"identity":"opaque","fetched_at":fetched,"etag":etag,"models":[{}]}).to_string()
        };
        let a = session(&account, "0.167.0");
        let b = session(&account, "0.167.0");
        let newest = body("0.167.0", "2026-11-22T10:00:00Z", "latest");
        put(&a.join(MODEL_FILE), &newest);
        cache.retire(&a, &account).unwrap();
        put(
            &b.join(MODEL_FILE),
            body("0.167.0", "2026-11-21T10:00:00Z", "older"),
        );
        cache.retire(&b, &account).unwrap();
        assert!(!a.join(MODEL_FILE).exists() && !b.join(MODEL_FILE).exists());

        let different_account = session(&other, "0.167.0");
        let different_version = session(&account, "0.168.0");
        cache.prepare(&different_account, &other).unwrap();
        cache.prepare(&different_version, &account).unwrap();
        assert!(!different_account.join(MODEL_FILE).exists());
        assert!(!different_version.join(MODEL_FILE).exists());

        cache.prepare(&a, &account).unwrap();
        assert_eq!(fs::read_to_string(a.join(MODEL_FILE)).unwrap(), newest);
    }

    #[test]
    fn remote_catalog_keeps_latest_snapshot_for_each_account() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CodexCache::new(dir.path());
        let account = format!("codex:{}", "a".repeat(64));
        let other = format!("codex:{}", "b".repeat(64));
        let relative = "cache/remote_plugin_catalog/0123456789abcdef.json";
        let a = dir.path().join("a/state/codex");
        let b = dir.path().join("b/state/codex");
        put(&a.join(relative), b"newest");
        put(&b.join(relative), b"older");
        File::options()
            .write(true)
            .open(b.join(relative))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        cache.retire(&a, &account).unwrap();
        cache.retire(&b, &account).unwrap();
        cache.prepare(&b, &other).unwrap();
        assert!(!b.join(relative).exists());
        cache.prepare(&a, &account).unwrap();
        assert_eq!(fs::read(a.join(relative)).unwrap(), b"newest");
    }
}
