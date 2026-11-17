//! Bounded display cache. A cache miss always computes outside the structural
//! mutex; simultaneous readers never queue behind another reader's SQL/decoding.
use super::*;
use std::time::{Duration, Instant};

struct Entry {
    value: Value,
    cursor: u64,
    bytes: usize,
    captured: Instant,
}
#[derive(Default)]
pub(super) struct RuntimeReadCache {
    entries: Mutex<Vec<(String, Entry)>>,
}
impl RuntimeReadCache {
    pub(super) fn read(&self, key: &str, compute: impl FnOnce() -> Result<Value>) -> Result<Value> {
        let ttl = Duration::from_millis(500);
        {
            let entries = self
                .entries
                .lock()
                .map_err(|e| err("storage", e.to_string()))?;
            if let Some((_, entry)) = entries
                .iter()
                .find(|(stored, entry)| stored == key && entry.captured.elapsed() < ttl)
            {
                return Ok(entry.value.clone());
            }
        }
        let captured = Instant::now();
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
