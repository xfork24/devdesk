//! Disposable, thread-local advisory reads. No connection or transaction is held.
//! Commands do not enter this scope, and every commit invalidates its pool's rows.
use crate::{connection_pool::ConnectionPool, performance};
use devdesk_core::{concurrency::*, ports::QueryObservation};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

const ENTRIES: usize = 256;
const SCOPE_BYTES: usize = 1024 * 1024;
const PROCESS_BYTES: usize = 16 * 1024 * 1024;
static RETAINED: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static SCOPES: RefCell<Vec<Cache>> = const { RefCell::new(Vec::new()) };
}
struct Cache {
    pool: usize,
    token: Rc<()>,
    rows: BTreeMap<ResourceKey, ResourceSnapshot>,
    bytes: usize,
}
impl Cache {
    fn clear(&mut self) {
        self.rows.clear();
        RETAINED.fetch_sub(self.bytes, Ordering::AcqRel);
        self.bytes = 0;
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        self.clear();
    }
}
struct Scope {
    _pool: ConnectionPool,
    // The TLS scope must leave on its creating thread, including unwinding.
    token: Rc<()>,
}
impl QueryObservation for Scope {}
impl Drop for Scope {
    fn drop(&mut self) {
        SCOPES.with(|scopes| {
            scopes
                .borrow_mut()
                .retain(|s| !Rc::ptr_eq(&s.token, &self.token));
        });
    }
}
pub(crate) fn enter(pool: &ConnectionPool) -> Box<dyn QueryObservation> {
    let token = Rc::new(());
    let entered = SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        if scopes.len() >= 8 {
            return false;
        }
        scopes.push(Cache {
            pool: pool.identity(),
            token: token.clone(),
            rows: BTreeMap::new(),
            bytes: 0,
        });
        true
    });
    if entered {
        Box::new(Scope {
            _pool: pool.clone(),
            token,
        })
    } else {
        Box::new(())
    }
}

fn eligible(key: &ResourceKey) -> bool {
    matches!(
        key.kind.as_str(),
        "workflow_step"
            | "workflow_change_pending"
            | "workflow_retry_count"
            | "workflow_logical_retries"
            | "workflow_confirmations"
            | "workflow_waiver"
            | "workflow_children_source"
            | "governance"
    )
}
pub(crate) fn active(pool: &ConnectionPool) -> bool {
    SCOPES.with(|scopes| {
        scopes
            .borrow()
            .last()
            .is_some_and(|s| s.pool == pool.identity())
    })
}
pub(crate) fn get(pool: &ConnectionPool, key: &ResourceKey) -> Option<ResourceSnapshot> {
    SCOPES
        .with(|scopes| {
            scopes
                .borrow()
                .last()
                .filter(|s| s.pool == pool.identity())
                .and_then(|s| s.rows.get(key).cloned())
        })
        .inspect(|_| performance::record("resource.advisory_hit", 0.0))
}
// Conservative estimate with bounded traversal; avoid serializing a large value
// just to decide not to cache it. This is a retention budget, not allocator RSS.
fn charge(value: &Value, remaining: &mut usize) -> bool {
    let overhead = 64
        + match value {
            Value::String(s) => s.capacity(),
            Value::Object(m) => m.len().saturating_mul(128),
            Value::Array(a) => a.capacity().saturating_mul(32),
            _ => 0,
        };
    let Some(rest) = remaining.checked_sub(overhead) else {
        return false;
    };
    *remaining = rest;
    match value {
        Value::Array(values) => values.iter().all(|v| charge(v, remaining)),
        Value::Object(values) => values.iter().all(|(key, v)| {
            let Some(rest) = remaining.checked_sub(key.capacity()) else {
                return false;
            };
            *remaining = rest;
            charge(v, remaining)
        }),
        _ => true,
    }
}
pub(crate) fn insert(pool: &ConnectionPool, rows: &[ResourceSnapshot]) {
    SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        let Some(cache) = scopes.last_mut().filter(|s| s.pool == pool.identity()) else {
            return;
        };
        for row in rows {
            if !eligible(&row.resource)
                || cache.rows.len() >= ENTRIES
                || cache.rows.contains_key(&row.resource)
            {
                continue;
            }
            let mut remaining = SCOPE_BYTES.saturating_sub(cache.bytes);
            let available = remaining;
            let overhead = 256 + row.resource.kind.capacity() + row.resource.id.capacity();
            let Some(rest) = remaining.checked_sub(overhead) else {
                continue;
            };
            remaining = rest;
            if row
                .value
                .as_ref()
                .is_some_and(|v| !charge(v, &mut remaining))
            {
                continue;
            }
            let bytes = available - remaining;
            if RETAINED
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    n.checked_add(bytes).filter(|n| *n <= PROCESS_BYTES)
                })
                .is_err()
            {
                continue;
            }
            cache.bytes += bytes;
            cache.rows.insert(row.resource.clone(), row.clone());
        }
    });
}
pub(crate) fn invalidate(pool: &ConnectionPool) {
    SCOPES.with(|scopes| {
        for cache in scopes
            .borrow_mut()
            .iter_mut()
            .filter(|s| s.pool == pool.identity())
        {
            cache.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_pool::ManagedConnection;
    use serde_json::json;

    fn pool(path: &std::path::Path) -> ConnectionPool {
        ConnectionPool::new(path, ManagedConnection::open(path).unwrap()).unwrap()
    }
    fn row(id: &str, value: Value) -> ResourceSnapshot {
        ResourceSnapshot {
            resource: ResourceKey::new("workflow_step", id),
            revision: 1,
            value: Some(value),
        }
    }
    #[test]
    fn advisory_rows_are_scoped_to_pool_and_thread_and_invalidate_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let a = pool(&dir.path().join("a"));
        let b = pool(&dir.path().join("b"));
        let value = row("same", json!("old"));
        insert(&a, std::slice::from_ref(&value));
        assert!(get(&a, &value.resource).is_none());
        let first = enter(&a);
        insert(&a, std::slice::from_ref(&value));
        assert_eq!(get(&a, &value.resource).unwrap().value, value.value);
        assert!(get(&b, &value.resource).is_none());
        std::thread::scope(|scope| {
            scope
                .spawn(|| assert!(get(&a, &value.resource).is_none()))
                .join()
                .unwrap();
        });
        let second = enter(&b);
        insert(&b, &[row("same", json!("other project"))]);
        drop(first); // A non-LIFO guard must not remove another scope.
        assert_eq!(
            get(&b, &value.resource).unwrap().value,
            Some(json!("other project"))
        );
        invalidate(&b);
        assert!(get(&b, &value.resource).is_none());
        drop(second);
        assert!(!active(&a));
        assert!(!active(&b));
    }
    #[test]
    fn advisory_retention_is_bounded_and_unwinding_releases_the_scope() {
        let dir = tempfile::tempdir().unwrap();
        let p = pool(&dir.path().join("db"));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _scope = enter(&p);
            for n in 0..300 {
                insert(&p, &[row(&n.to_string(), json!(n))]);
            }
            insert(&p, &[row("large", json!("x".repeat(SCOPE_BYTES)))]);
            insert(
                &p,
                &[ResourceSnapshot {
                    resource: ResourceKey::new("workflow_run_authority", "authority"),
                    revision: 1,
                    value: Some(json!({})),
                }],
            );
            SCOPES.with(|s| {
                let scopes = s.borrow();
                let cache = scopes.last().unwrap();
                assert!(cache.rows.len() <= ENTRIES && cache.bytes <= SCOPE_BYTES);
                assert!(
                    !cache
                        .rows
                        .contains_key(&ResourceKey::new("workflow_step", "large"))
                );
                assert!(
                    !cache
                        .rows
                        .contains_key(&ResourceKey::new("workflow_run_authority", "authority"))
                );
            });
            panic!("query interrupted");
        }));
        assert!(!active(&p));
    }
}
