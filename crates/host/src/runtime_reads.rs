//! Bounded display cache. A cache miss always computes outside the structural
//! mutex; simultaneous readers never queue behind another reader's SQL/decoding.
use super::*;
use std::time::{Duration, Instant};

pub(super) fn eligible(method: &str, params: &Value) -> bool {
    if method == "query.orchestration" && params["view"] == "decision" {
        return false;
    }
    params["project_id"].as_str().is_some_and(|p| !p.is_empty())
        && (matches!(
            method,
            "query.workflow_progress"
                | "query.workflow_management_summary"
                | "query.orchestration_summary"
                | "query.workflow_driver_summary"
                | "query.orchestration"
                | "query.execution_workspace"
        ) || (method == "query.workflow_driver" && params["view"] == "summary"))
}

struct Entry {
    value: Value,
    cursor: u64,
    bytes: usize,
    captured: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_committed_command_invalidates_its_project_without_reusing_an_old_in_flight_key() {
        let cache = RuntimeReadCache::default();
        let a = cache.scoped_key("a", "same-read").unwrap();
        let b = cache.scoped_key("b", "same-read").unwrap();
        cache.invalidate(Some("a")).unwrap();
        let next = cache.scoped_key("a", "same-read").unwrap();
        assert_ne!(a, next);
        assert_eq!(b, cache.scoped_key("b", "same-read").unwrap());
        cache
            .read_for(&a, Duration::from_secs(10), || Ok(json!({"cursor":1})))
            .unwrap();
        assert_eq!(
            cache
                .read_for(&next, Duration::from_secs(10), || Ok(json!({"cursor":2})))
                .unwrap()["cursor"],
            2
        );
        for n in 0..130 {
            cache.scoped_key(&format!("other-{n}"), "read").unwrap();
        }
        assert_ne!(next, cache.scoped_key("a", "same-read").unwrap());
        cache.invalidate(None).unwrap();
        assert_ne!(b, cache.scoped_key("b", "same-read").unwrap());
    }
    #[test]
    fn exact_requests_and_action_qualification_never_enter_the_display_cache() {
        let p = json!({"project_id":"project","id":"root"});
        for method in [
            "workflow.inspect",
            "query.workflow_driver",
            "query.orchestration_decision",
            "query.workflow_materials",
            "workflow.step.accept",
            "workflow.decision.resolve",
        ] {
            assert!(!eligible(method, &p), "{method}");
        }
        assert!(eligible(
            "query.workflow_driver",
            &json!({"project_id":"project","id":"root","view":"summary"})
        ));
        assert!(!eligible("query.workflow_progress", &json!({"id":"root"})));
    }
    #[test]
    fn an_older_in_flight_snapshot_never_blocks_or_replaces_a_newer_snapshot() {
        let cache = Arc::new(RuntimeReadCache::default());
        let (started, observed) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let old = cache.clone();
        let thread = std::thread::spawn(move || {
            old.read_for("project-a/root", Duration::from_secs(10), || {
                started.send(()).unwrap();
                released.recv().unwrap();
                Ok(json!({"cursor":1,"record":{"revision":3}}))
            })
            .unwrap()
        });
        observed.recv().unwrap();
        let current = cache
            .read_for("project-a/root", Duration::from_secs(10), || {
                Ok(json!({"cursor":2,"record":{"revision":4}}))
            })
            .unwrap();
        release.send(()).unwrap();
        assert_eq!(thread.join().unwrap()["record"]["revision"], 3);
        assert_eq!(
            cache
                .read_for("project-a/root", Duration::from_secs(10), || panic!(
                    "must reuse the newer committed snapshot"
                ))
                .unwrap(),
            current
        );
        assert_eq!(
            cache
                .read_for("project-b/root", Duration::from_secs(10), || Ok(
                    json!({"cursor":1,"record":{"revision":99}})
                ))
                .unwrap()["record"]["revision"],
            99
        );
    }
}
#[derive(Default)]
pub(super) struct RuntimeReadCache {
    entries: Mutex<Vec<(String, Entry)>>,
    epochs: Mutex<std::collections::BTreeMap<String, u64>>,
    serial: std::sync::atomic::AtomicU64,
}
impl RuntimeReadCache {
    pub(super) fn scoped_key(&self, project: &str, key: &str) -> Result<String> {
        let mut epochs = self
            .epochs
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        let epoch = if let Some(epoch) = epochs.get(project) {
            *epoch
        } else {
            if epochs.len() >= 128 {
                let first = epochs.keys().next().cloned().unwrap();
                epochs.remove(&first);
            }
            let epoch = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
            epochs.insert(project.into(), epoch);
            epoch
        };
        Ok(format!("{project}:{epoch}:{key}"))
    }
    pub(super) fn invalidate(&self, project: Option<&str>) -> Result<()> {
        let mut epochs = self
            .epochs
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        let epoch = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(project) = project {
            if epochs.len() >= 128 && !epochs.contains_key(project) {
                let first = epochs.keys().next().cloned().unwrap();
                epochs.remove(&first);
            }
            epochs.insert(project.into(), epoch);
        } else {
            for value in epochs.values_mut() {
                *value = epoch;
            }
        }
        Ok(())
    }
    pub(super) fn read(&self, key: &str, compute: impl FnOnce() -> Result<Value>) -> Result<Value> {
        self.read_for(key, Duration::from_millis(100), compute)
    }
    pub(super) fn read_for(
        &self,
        key: &str,
        ttl: Duration,
        compute: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        {
            let entries = self
                .entries
                .lock()
                .map_err(|e| err("storage", e.to_string()))?;
            if let Some((_, entry)) = entries
                .iter()
                .find(|(stored, entry)| stored == key && entry.captured.elapsed() < ttl)
            {
                performance::record("runtime.display_cache_hit", 0.0);
                return Ok(entry.value.clone());
            }
        }
        let captured = Instant::now();
        performance::record("runtime.display_cache_miss", 0.0);
        let value = compute()?;
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| err("storage", e.to_string()))?
            .len();
        if bytes > 512 * 1024 {
            return Ok(value);
        }
        let cursor = value["cursor"].as_u64().unwrap_or(0);
        let mut entries = self
            .entries
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        // An earlier snapshot may finish after a newer one. It must not replace
        // or extend the lifetime of that newer result.
        if let Some((_, entry)) = entries.iter().find(|(stored, _)| stored == key)
            && (entry.cursor > cursor || (entry.cursor == cursor && entry.captured > captured))
        {
            return Ok(value);
        }
        entries.retain(|(stored, entry)| {
            stored != key && entry.captured.elapsed() < Duration::from_secs(5)
        });
        while entries.len() >= 128
            || entries.iter().map(|(_, e)| e.bytes).sum::<usize>() + bytes > 12 * 1024 * 1024
        {
            if entries.is_empty() {
                break;
            }
            entries.remove(0);
        }
        entries.push((
            key.into(),
            Entry {
                value: value.clone(),
                cursor,
                bytes,
                captured,
            },
        ));
        Ok(value)
    }
}
