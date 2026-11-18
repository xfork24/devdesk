//! Opt-in, bounded request measurements. No parameters or business content are recorded.
use serde::Serialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

#[derive(Default, Serialize)]
struct Measurement {
    calls: u64,
    total_ms: f64,
}
thread_local! {
    static CURRENT: RefCell<Option<BTreeMap<&'static str, Measurement>>> = const { RefCell::new(None) };
}

pub struct Trace {
    started: Instant,
}
impl Trace {
    pub fn start() -> Self {
        CURRENT.with(|current| *current.borrow_mut() = Some(BTreeMap::new()));
        Self {
            started: Instant::now(),
        }
    }
    pub fn finish(self) -> Value {
        let spans = CURRENT.with(|current| current.borrow_mut().take().unwrap_or_default());
        json!({"total_ms":self.started.elapsed().as_secs_f64()*1000.0,"spans":spans})
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = None);
    }
}

pub struct Span {
    name: &'static str,
    started: Option<Instant>,
}
impl devdesk_core::ports::QueryObservation for Span {}
pub fn observe(stage: &'static str) -> Box<dyn devdesk_core::ports::QueryObservation> {
    if CURRENT.with(|current| current.borrow().is_some()) {
        Box::new(Span::new(stage))
    } else {
        Box::new(())
    }
}
struct NativePhase {
    call_id: String,
    stage: &'static str,
    started: Instant,
    aggregate: Span,
    started_at_ms: u64,
}
impl devdesk_core::ports::QueryObservation for NativePhase {}
pub(crate) fn observe_native(
    call_id: &str,
    stage: &'static str,
) -> Box<dyn devdesk_core::ports::QueryObservation> {
    if CURRENT.with(|current| current.borrow().is_some()) {
        Box::new(NativePhase {
            call_id: call_id.into(),
            stage,
            started: Instant::now(),
            aggregate: Span::new(stage),
            started_at_ms: NativeProtocolTiming::observed().1,
        })
    } else {
        Box::new(())
    }
}
impl Drop for NativePhase {
    fn drop(&mut self) {
        let _ = &self.aggregate;
        eprintln!(
            "{}",
            json!({"type":"native_phase","call_id":self.call_id,"phase":self.stage,"elapsed_ms":self.started.elapsed().as_secs_f64()*1000.0,"started_at_ms":self.started_at_ms,"finished_at_ms":NativeProtocolTiming::observed().1})
        );
    }
}
impl Span {
    pub fn new(name: &'static str) -> Self {
        let active = CURRENT.with(|current| current.borrow().is_some());
        Self {
            name,
            started: active.then(Instant::now),
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            record(self.name, started.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
pub fn record(name: &'static str, milliseconds: f64) {
    CURRENT.with(|current| {
        if let Some(spans) = current.borrow_mut().as_mut() {
            let span = spans.entry(name).or_default();
            span.calls += 1;
            span.total_ms += milliseconds;
        }
    });
}

/// Transport boundaries, independent of the scheduler thread's scoped spans.
/// Only request identities are retained; tool arguments and results never enter metrics.
pub(crate) struct NativeProtocolTiming {
    operation: String,
    sequence: std::sync::atomic::AtomicU64,
    pending: std::sync::Mutex<BTreeMap<String, (u64, Instant, u64)>>,
}
impl NativeProtocolTiming {
    pub(crate) fn new(path: &std::path::Path) -> Option<Self> {
        let operation = path.file_stem()?.to_str()?;
        (std::env::var_os("DEVDESK_PERF_LOG").is_some_and(|v| v == "1")
            && uuid::Uuid::parse_str(operation).is_ok())
        .then(|| Self {
            operation: operation.into(),
            sequence: Default::default(),
            pending: Default::default(),
        })
    }
    fn key(frame: &Value) -> Option<String> {
        use sha2::{Digest, Sha256};
        let id = frame.get("id")?;
        if !id.is_string() && !id.is_u64() {
            return None;
        }
        Some(format!("{:x}", Sha256::digest(id.to_string().as_bytes())))
    }
    pub(crate) fn observed() -> (Instant, u64) {
        (
            Instant::now(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        )
    }
    pub(crate) fn request(&self, frame: &Value, (started, wall): (Instant, u64)) {
        if !["item/tool/call", "devdesk/tool/call"]
            .contains(&frame["method"].as_str().unwrap_or(""))
        {
            return;
        }
        let Some(key) = Self::key(frame) else { return };
        let mut pending = self.pending.lock().unwrap();
        if pending.len() >= 64 || pending.contains_key(&key) {
            return;
        }
        let sequence = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        pending.insert(key.clone(), (sequence, started, wall));
        eprintln!(
            "{}",
            json!({"type":"native_protocol","phase":"request_observed","operation_id":self.operation,"sequence":sequence,"request_key":key,"timestamp_ms":wall})
        );
    }
    pub(crate) fn reply(&self, frame: &Value) {
        if frame.get("method").is_some() || frame.get("result").is_none() {
            return;
        }
        let Some(key) = Self::key(frame) else { return };
        let Some((sequence, started, wall)) = self.pending.lock().unwrap().remove(&key) else {
            return;
        };
        eprintln!(
            "{}",
            json!({"type":"native_protocol","phase":"reply_written","operation_id":self.operation,"sequence":sequence,"request_key":key,"observed_at_ms":wall,"roundtrip_ms":started.elapsed().as_secs_f64()*1000.0})
        );
    }
}

#[cfg(test)]
mod protocol_timing_tests {
    use super::*;
    #[test]
    fn native_timing_is_bounded_and_matches_only_actual_tool_requests() {
        let timing = NativeProtocolTiming {
            operation: uuid::Uuid::nil().to_string(),
            sequence: Default::default(),
            pending: Default::default(),
        };
        timing.request(
            &json!({"id":0,"method":"account/read","params":{"token":"canary"}}),
            NativeProtocolTiming::observed(),
        );
        assert!(timing.pending.lock().unwrap().is_empty());
        for id in 0..100 {
            timing.request(
                &json!({"id":id,"method":"item/tool/call","params":{"arguments":"canary"}}),
                NativeProtocolTiming::observed(),
            );
        }
        assert_eq!(timing.pending.lock().unwrap().len(), 64);
        timing.reply(&json!({"id":0,"method":"other","result":{}}));
        assert_eq!(timing.pending.lock().unwrap().len(), 64);
        for id in 0..100 {
            timing.reply(&json!({"id":id,"result":{"body":"canary"}}));
        }
        assert!(timing.pending.lock().unwrap().is_empty());
    }
}
