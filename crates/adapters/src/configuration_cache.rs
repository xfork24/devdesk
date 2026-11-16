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
impl Drop for Reservation {
    fn drop(&mut self) {
        RETAINED_BYTES.fetch_sub(self.0, Ordering::Relaxed);
    }
}
struct Entry {
    data_version: u64,
    snapshot: Arc<ConfigurationCatalogSnapshot>,
    _reservation: Reservation,
}
pub(super) struct CatalogCache {
    entry: Mutex<Option<Entry>>,
    revision: AtomicU64,
    data_version: AtomicU64,
}
impl Default for CatalogCache {
    fn default() -> Self {
        Self {
            entry: Default::default(),
            revision: AtomicU64::new(1),
            data_version: AtomicU64::new(0),
        }
    }
}
impl CatalogCache {
    pub fn current(&self) -> Result<Option<Arc<ConfigurationCatalogSnapshot>>> {
        let entry = self.entry.lock().map_err(storage)?;
        if let Some(entry) = entry.as_ref() {
            performance::record("catalog.cache_hit", 0.0);
            return Ok(Some(entry.snapshot.clone()));
        }
        Ok(None)
    }
    pub fn invalidate(&self) -> Result<()> {
        self.revision.fetch_add(1, Ordering::Relaxed);
        *self.entry.lock().map_err(storage)? = None;
        Ok(())
    }
    pub fn read(
        &self,
        conn: &Connection,
        project: bool,
    ) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        let _span = performance::Span::new("catalog.read");
        let version = conn
            .query_row("PRAGMA data_version", [], |r| r.get::<_, u64>(0))
            .map_err(storage)?;
        let previous = self.data_version.swap(version, Ordering::Relaxed);
        if previous != 0 && previous != version {
            self.invalidate()?;
        }
        if let Some(entry) = self
            .entry
            .lock()
            .map_err(storage)?
            .as_ref()
            .filter(|e| e.data_version == version)
        {
            performance::record("catalog.cache_hit", 0.0);
            return Ok(entry.snapshot.clone());
        }
        *self.entry.lock().map_err(storage)? = None;
        let _build = performance::Span::new("catalog.build");
        let transaction = conn.unchecked_transaction().map_err(storage)?;
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
        let revision = self.revision.load(Ordering::Relaxed);
        let snapshot = Arc::new(ConfigurationCatalogSnapshot::new(state, revision, ready)?);
        // Account for the state and its auxiliary indexes conservatively. Readers may pin an old Arc.
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
        if let Some(reservation) = reservation {
            *self.entry.lock().map_err(storage)? = Some(Entry {
                data_version: version,
                snapshot: snapshot.clone(),
                _reservation: reservation,
            });
        }
        Ok(snapshot)
    }
}

struct ReadOnlyCatalog {
    connection: Mutex<Connection>,
    cache: CatalogCache,
}
/// Reuse a bounded set of read-only connections so PRAGMA data_version detects outside commits.
pub(super) fn read_only(path: &Path, project: bool) -> Result<Arc<ConfigurationCatalogSnapshot>> {
    type Readers = std::collections::BTreeMap<PathBuf, (u64, Arc<ReadOnlyCatalog>)>;
    static READERS: std::sync::OnceLock<Mutex<Readers>> = std::sync::OnceLock::new();
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    reject_symlink(path)?;
    let key = path.canonicalize().map_err(storage)?;
    let ticket = CLOCK.fetch_add(1, Ordering::Relaxed);
    let reader = {
        let mut readers = READERS
            .get_or_init(Default::default)
            .lock()
            .map_err(storage)?;
        if let Some((last, reader)) = readers.get_mut(&key) {
            *last = ticket;
            reader.clone()
        } else {
            if readers.len() >= 8
                && let Some(oldest) = readers
                    .iter()
                    .min_by_key(|(_, v)| v.0)
                    .map(|(k, _)| k.clone())
            {
                readers.remove(&oldest);
            }
            let connection =
                Connection::open_with_flags(&key, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(storage)?;
            connection
                .execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=5000;")
                .map_err(storage)?;
            let reader = Arc::new(ReadOnlyCatalog {
                connection: Mutex::new(connection),
                cache: Default::default(),
            });
            readers.insert(key, (ticket, reader.clone()));
            reader
        }
    };
    reader
        .cache
        .read(&*reader.connection.lock().map_err(storage)?, project)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(cache.read(&conn, false).unwrap().revision > first.revision);
    }
}
