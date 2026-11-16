//! Bounded, process-local read models over the authoritative source tables.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

const CACHE_BYTES: usize = 128 * 1024 * 1024;
const ENTRY_BYTES: usize = 32 * 1024 * 1024;
static RETAINED_BYTES: AtomicUsize = AtomicUsize::new(0);
struct Reservation(usize);
impl SnapshotAllocation for Reservation {}
impl Drop for Reservation {
    fn drop(&mut self) {
        RETAINED_BYTES.fetch_sub(self.0, Ordering::Relaxed);
    }
}
struct Entry {
    snapshot: Arc<ConfigurationCatalogSnapshot>,
}
#[derive(Default)]
pub(super) struct CatalogCache {
    entry: Mutex<Option<Entry>>,
}
impl CatalogCache {
    pub fn invalidate(&self) -> Result<()> {
        *self.entry.lock().map_err(storage)? = None;
        Ok(())
    }
    pub fn read(
        &self,
        conn: &Connection,
        project: bool,
    ) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        let _span = performance::Span::new("catalog.read");
        // Clock and rows must come from the same committed snapshot. PRAGMA
        // data_version is connection-local and cannot identify pooled snapshots.
        let transaction = conn.unchecked_transaction().map_err(storage)?;
        let revision = transaction
            .query_row("SELECT revision FROM catalog_clock WHERE id=1", [], |r| {
                r.get::<_, u64>(0)
            })
            .map_err(storage)?;
        let cached = self
            .entry
            .lock()
            .map_err(storage)?
            .as_ref()
            .filter(|entry| entry.snapshot.revision == revision)
            .map(|entry| entry.snapshot.clone());
        if let Some(snapshot) = cached {
            performance::record("catalog.cache_hit", 0.0);
            transaction.commit().map_err(storage)?;
            return Ok(snapshot);
        }
        let _build = performance::Span::new("catalog.build");
        let mut state = EngineeringState {
            governance: source_store::load_catalog_records(&transaction)?,
            projects: if project {
                load_table(&transaction, "projects")?
            } else {
                vec![]
            },
            ..Default::default()
        };
        state.refresh_configuration_views()?;
        source_store::restore_records(&mut state, &mut Default::default())?;
        let ready = project
            && transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM repository_worktrees WHERE kind='mainline')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(storage)?;
        transaction.commit().map_err(storage)?;
        let bytes = serde_json::to_vec(&state).map_err(storage)?.len();
        self.retain(
            ConfigurationCatalogSnapshot::new(state, revision, ready)?,
            bytes,
        )
    }
    fn retain(
        &self,
        snapshot: ConfigurationCatalogSnapshot,
        bytes: usize,
    ) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        let retained = bytes.saturating_mul(2);
        let reservation = (retained <= ENTRY_BYTES)
            .then(|| {
                RETAINED_BYTES
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        n.checked_add(retained).filter(|sum| *sum <= CACHE_BYTES)
                    })
                    .ok()
            })
            .flatten()
            .map(|_| Reservation(retained));
        let admitted = reservation.is_some();
        let snapshot = Arc::new(if let Some(reservation) = reservation {
            snapshot.with_allocation(reservation)
        } else {
            snapshot
        });
        if admitted {
            self.publish(snapshot.clone())?;
        }
        Ok(snapshot)
    }
    fn publish(&self, snapshot: Arc<ConfigurationCatalogSnapshot>) -> Result<()> {
        let mut entry = self.entry.lock().map_err(storage)?;
        // A slow old reader cannot evict a newer committed view.
        if entry
            .as_ref()
            .is_none_or(|old| old.snapshot.revision <= snapshot.revision)
        {
            *entry = Some(Entry { snapshot });
        }
        Ok(())
    }
}

struct ReadOnlyCatalog {
    connection: connection_pool::ConnectionPool,
    cache: CatalogCache,
}
/// Reuse bounded read-only pools; the committed catalog clock detects outside changes.
pub(super) fn read_only(path: &Path, project: bool) -> Result<Arc<ConfigurationCatalogSnapshot>> {
    type Readers = std::collections::BTreeMap<PathBuf, (u64, Arc<ReadOnlyCatalog>)>;
    static READERS: std::sync::OnceLock<Mutex<Readers>> = std::sync::OnceLock::new();
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    reject_symlink(path)?;
    let key = path.canonicalize().map_err(storage)?;
    let ticket = CLOCK.fetch_add(1, Ordering::Relaxed);
    let existing = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(storage)?
        .get_mut(&key)
        .map(|(last, reader)| {
            *last = ticket;
            reader.clone()
        });
    let reader = if let Some(existing) = existing {
        existing
    } else {
        // Opening and reclaiming connection budgets must not hold the map lock.
        let candidate = Arc::new(ReadOnlyCatalog {
            connection: connection_pool::ConnectionPool::read_only(&key)?,
            cache: Default::default(),
        });
        let mut readers = READERS
            .get_or_init(Default::default)
            .lock()
            .map_err(storage)?;
        if readers.len() >= 8
            && !readers.contains_key(&key)
            && let Some(oldest) = readers
                .iter()
                .min_by_key(|(_, v)| v.0)
                .map(|(key, _)| key.clone())
        {
            readers.remove(&oldest);
        }
        readers.entry(key).or_insert((ticket, candidate)).1.clone()
    };

    reader.cache.read(&*reader.connection.read()?, project)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_source(conn: &Connection) {
        conn.execute("INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES('sample-spec',1,'spec','Sample','\"global\"','{}',?1)",
            [json!({"schema_version":3,"body":"Sample contract","checks":[]}).to_string()]).unwrap();
    }

    #[test]
    fn new_connections_and_late_publishers_cannot_hide_a_committed_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("global.sqlite3");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        writer
            .execute_batch(include_str!("../../../sql/global/schema.sql"))
            .unwrap();
        let cache = CatalogCache::default();
        let old_reader = Connection::open(&path).unwrap();
        let old = cache.read(&old_reader, false).unwrap();
        insert_source(&writer);
        let new_reader = Connection::open(&path).unwrap();
        let new = cache.read(&new_reader, false).unwrap();
        assert!(new.revision > old.revision);
        assert!(old.record("sample-spec:1").is_none());
        assert!(new.record("sample-spec:1").is_some());
        cache.publish(old.clone()).unwrap();
        assert!(Arc::ptr_eq(&cache.read(&old_reader, false).unwrap(), &new));
        // An invalidation can race with an old build before any newer cache entry exists.
        cache.invalidate().unwrap();
        cache.publish(old).unwrap();
        let rebuilt = cache.read(&new_reader, false).unwrap();
        assert_eq!(rebuilt.revision, new.revision);
        assert!(rebuilt.record("sample-spec:1").is_some());
    }

    #[test]
    fn catalog_clock_changes_atomically_and_ignores_runtime_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("global.sqlite3");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        writer
            .execute_batch(include_str!("../../../sql/global/schema.sql"))
            .unwrap();
        let reader = Connection::open(&path).unwrap();
        let clock = |conn: &Connection| {
            conn.query_row("SELECT revision FROM catalog_clock", [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap()
        };
        let initial = clock(&reader);
        writer.execute_batch("BEGIN IMMEDIATE;").unwrap();
        insert_source(&writer);
        assert!(clock(&writer) > initial);
        assert_eq!(clock(&reader), initial);
        writer.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(clock(&reader), initial);
        writer
            .execute(
                "INSERT INTO events(target,kind,detail,time) VALUES('runtime','progress','',1)",
                [],
            )
            .unwrap();
        assert_eq!(clock(&reader), initial);
        insert_source(&writer);
        assert!(clock(&reader) > initial);
    }

    #[test]
    fn every_catalog_governance_kind_advances_the_persisted_clock() {
        for schema in [
            include_str!("../../../sql/global/schema.sql"),
            include_str!("../../../sql/project/schema.sql"),
        ] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(schema).unwrap();
            for kind in CATALOG_RECORD_KINDS {
                let before: u64 = conn
                    .query_row("SELECT revision FROM catalog_clock", [], |r| r.get(0))
                    .unwrap();
                conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,supersedes,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,?1,?1,'\"global\"',?1,1,NULL,'fixture','{}','{}',0,1,1)",[kind]).unwrap();
                let after: u64 = conn
                    .query_row("SELECT revision FROM catalog_clock", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(after, before + 1, "missing clock trigger: {kind}");
            }
        }
    }

    #[test]
    fn cache_admission_and_eviction_do_not_change_the_committed_catalog_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../sql/global/schema.sql"))
            .unwrap();
        let cache = CatalogCache::default();
        let first = cache.read(&conn, false).unwrap();
        *cache.entry.lock().unwrap() = None;
        let rebuilt = cache.read(&conn, false).unwrap();
        assert_eq!(first.revision, rebuilt.revision);
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        cache.invalidate().unwrap();
        assert_eq!(cache.read(&conn, false).unwrap().revision, first.revision);
    }
}
