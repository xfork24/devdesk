//! Bounded connection ownership. Pool mutexes never cover SQL or application work.
use crate::{performance, storage};
use devdesk_domain::{Result, err};
use rusqlite::{Connection, OpenFlags};
use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const HOST_CONNECTIONS: usize = 64;
static OPEN: AtomicUsize = AtomicUsize::new(0);
static POOLS: OnceLock<Mutex<Vec<Weak<PoolInner>>>> = OnceLock::new();

struct Permit;
impl Permit {
    fn acquire() -> Option<Self> {
        OPEN.try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < HOST_CONNECTIONS).then_some(n + 1)
        })
        .ok()
        .map(|_| Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        OPEN.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ManagedConnection {
    connection: Connection,
    _permit: Permit,
}
#[derive(Default)]
struct Available {
    idle: Vec<ManagedConnection>,
    created: usize,
}
struct Lane {
    available: Mutex<Available>,
    wake: Condvar,
    limit: usize,
}
impl Lane {
    fn new(limit: usize) -> Self {
        Self {
            available: Default::default(),
            wake: Condvar::new(),
            limit,
        }
    }
}
struct PoolInner {
    path: PathBuf,
    readers: Lane,
    submitters: Lane,
}

pub(crate) struct ConnectionPool(Arc<PoolInner>);
impl ConnectionPool {
    /// Initialization and schema validation finish before the pool is published.
    pub fn new(path: &Path, initial: Connection) -> Result<Self> {
        let permit = Permit::acquire().or_else(|| {
            evict_idle();
            Permit::acquire()
        });
        let permit = permit.ok_or_else(|| err("storage_busy", "数据库连接预算已用尽"))?;
        initial
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(storage)?;
        initial
            .busy_timeout(Duration::from_millis(25))
            .map_err(storage)?;
        let inner = Arc::new(PoolInner {
            path: path.to_owned(),
            readers: Lane::new(4),
            submitters: Lane::new(2),
        });
        {
            let mut available = inner.submitters.available.lock().map_err(storage)?;
            available.created = 1;
            available.idle.push(ManagedConnection {
                connection: initial,
                _permit: permit,
            });
        }
        let mut pools = POOLS
            .get_or_init(Default::default)
            .lock()
            .map_err(storage)?;
        pools.retain(|pool| pool.strong_count() > 0);
        pools.push(Arc::downgrade(&inner));
        Ok(Self(inner))
    }

    // Existing callers borrow a private submit connection while their use cases
    // migrate to explicit resource commits. This does not lock other connections.
    pub fn lock(&self) -> Result<PooledConnection> {
        self.checkout(false)
    }

    pub fn read(&self) -> Result<PooledConnection> {
        self.checkout(true)
    }

    fn checkout(&self, read_only: bool) -> Result<PooledConnection> {
        let _span = performance::Span::new("sqlite.pool_wait");
        let lane = if read_only {
            &self.0.readers
        } else {
            &self.0.submitters
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut available = lane.available.lock().map_err(storage)?;
            if let Some(connection) = available.idle.pop() {
                return Ok(PooledConnection {
                    pool: self.0.clone(),
                    read_only,
                    connection: Some(connection),
                });
            }
            if available.created < lane.limit {
                available.created += 1;
                drop(available);
                if let Some(permit) = Permit::acquire() {
                    let flags = if read_only {
                        OpenFlags::SQLITE_OPEN_READ_ONLY
                    } else {
                        OpenFlags::SQLITE_OPEN_READ_WRITE
                    };
                    let opened = (|| {
                        let conn =
                            Connection::open_with_flags(&self.0.path, flags).map_err(storage)?;
                        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=25;")
                            .map_err(storage)?;
                        if read_only {
                            conn.execute_batch("PRAGMA query_only=ON;")
                                .map_err(storage)?;
                        }
                        Ok(conn)
                    })();
                    match opened {
                        Ok(connection) => {
                            return Ok(PooledConnection {
                                pool: self.0.clone(),
                                read_only,
                                connection: Some(ManagedConnection {
                                    connection,
                                    _permit: permit,
                                }),
                            });
                        }
                        Err(error) => {
                            lane.available.lock().map_err(storage)?.created -= 1;
                            lane.wake.notify_one();
                            return Err(error);
                        }
                    }
                }
                lane.available.lock().map_err(storage)?.created -= 1;
                evict_idle();
                available = lane.available.lock().map_err(storage)?;
            }
            if Instant::now() >= deadline {
                return Err(err("storage_busy", "等待数据库连接超时"));
            }
            drop(
                lane.wake
                    .wait_timeout(available, Duration::from_millis(5))
                    .map_err(storage)?,
            );
        }
    }
}

fn evict_idle() {
    let pools: Vec<_> = POOLS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for pool in pools {
        for lane in [&pool.readers, &pool.submitters] {
            let evicted = if let Ok(mut available) = lane.available.try_lock() {
                let item = available.idle.pop();
                if item.is_some() {
                    available.created -= 1;
                }
                item
            } else {
                None
            };
            if evicted.is_some() {
                drop(evicted);
                lane.wake.notify_one();
                return;
            }
        }
    }
}

pub(crate) struct PooledConnection {
    pool: Arc<PoolInner>,
    read_only: bool,
    connection: Option<ManagedConnection>,
}
impl Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self
            .connection
            .as_ref()
            .expect("checked out connection")
            .connection
    }
}
impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self
            .connection
            .as_mut()
            .expect("checked out connection")
            .connection
    }
}
impl Drop for PooledConnection {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        let lane = if self.read_only {
            &self.pool.readers
        } else {
            &self.pool.submitters
        };
        let reusable = connection.connection.is_autocommit()
            || connection.connection.execute_batch("ROLLBACK;").is_ok();
        let mut available = lane.available.lock().unwrap();
        if reusable {
            available.idle.push(connection);
        } else {
            available.created -= 1;
        }
        drop(available);
        lane.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(path: &Path) -> ConnectionPool {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS values_at(id INTEGER PRIMARY KEY, value INTEGER); INSERT OR IGNORE INTO values_at VALUES(1,0);").unwrap();
        ConnectionPool::new(path, conn).unwrap()
    }

    #[test]
    fn a_writer_and_two_read_snapshots_progress_independently() {
        let directory = tempfile::tempdir().unwrap();
        let pool = pool(&directory.path().join("db.sqlite3"));
        let first = pool.read().unwrap();
        let second = pool.read().unwrap();
        first.execute_batch("BEGIN;").unwrap();
        second.execute_batch("BEGIN;").unwrap();
        let read = |conn: &Connection| {
            conn.query_row("SELECT value FROM values_at", [], |r| r.get::<_, u64>(0))
                .unwrap()
        };
        assert_eq!(read(&first), 0);
        assert_eq!(read(&second), 0);
        pool.lock()
            .unwrap()
            .execute("UPDATE values_at SET value=1", [])
            .unwrap();
        assert_eq!(read(&first), 0);
        assert_eq!(read(&second), 0);
        assert_eq!(read(&pool.read().unwrap()), 1);
        assert!(first.execute("UPDATE values_at SET value=2", []).is_err());
    }

    #[test]
    fn returning_a_connection_rolls_back_unfinished_work() {
        let directory = tempfile::tempdir().unwrap();
        let pool = pool(&directory.path().join("db.sqlite3"));
        {
            let conn = pool.lock().unwrap();
            conn.execute_batch("BEGIN; UPDATE values_at SET value=9;")
                .unwrap();
        }
        let conn = pool.lock().unwrap();
        assert!(conn.is_autocommit());
        assert_eq!(
            conn.query_row("SELECT value FROM values_at", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert!(
            conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, bool>(0))
                .unwrap()
        );
    }

    #[test]
    fn cold_pools_can_reclaim_idle_connections_within_the_host_budget() {
        let directory = tempfile::tempdir().unwrap();
        let pools: Vec<_> = (0..HOST_CONNECTIONS + 8)
            .map(|n| pool(&directory.path().join(format!("{n}.sqlite3"))))
            .collect();
        for pool in &pools {
            assert!(pool.read().unwrap().is_autocommit());
            assert!(OPEN.load(Ordering::Acquire) <= HOST_CONNECTIONS);
        }
    }
}
