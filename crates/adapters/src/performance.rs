//! Opt-in, bounded request measurements. No parameters or business content are recorded.
use serde::Serialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

#[derive(Default, Serialize)]
struct Measurement {
    calls: u64,
    total_ms: f64,
}

const MAX_PATHS: usize = 128;
const READ_POOL_BOUNDS_MS: [f64; 15] = [
    0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 1000.0, 2000.0,
];

#[derive(Default)]
struct Measurements {
    spans: BTreeMap<&'static str, Measurement>,
    paths: BTreeMap<Vec<&'static str>, Measurement>,
    databases: BTreeMap<(String, &'static str), Measurement>,
    active: Vec<(u64, &'static str)>,
    sequence: u64,
    paths_omitted: u64,
    read_checkout_histogram: [u64; READ_POOL_BOUNDS_MS.len() + 1],
}

impl Measurements {
    fn path(&self, name: &'static str) -> Vec<&'static str> {
        self.active
            .iter()
            .map(|(_, stage)| *stage)
            .chain([name])
            .collect()
    }

    fn record(&mut self, name: &'static str, path: Vec<&'static str>, milliseconds: f64) {
        if name == "sqlite.read_pool_wait" {
            let bucket = READ_POOL_BOUNDS_MS.partition_point(|upper| *upper < milliseconds);
            self.read_checkout_histogram[bucket] += 1;
        }
        let span = self.spans.entry(name).or_default();
        span.calls += 1;
        span.total_ms += milliseconds;
        if self.paths.len() < MAX_PATHS || self.paths.contains_key(&path) {
            let span = self.paths.entry(path).or_default();
            span.calls += 1;
            span.total_ms += milliseconds;
        } else {
            self.paths_omitted += 1;
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Measurements>> = const { RefCell::new(None) };
}
mod transactions;
pub(crate) use transactions::TransactionTiming;

pub struct Trace {
    started: Instant,
}
impl Trace {
    pub fn start() -> Self {
        transactions::reset();
        CURRENT.with(|current| *current.borrow_mut() = Some(Measurements::default()));
        Self {
            started: Instant::now(),
        }
    }
    pub fn finish(self) -> Value {
        let measurements = CURRENT.with(|current| current.borrow_mut().take().unwrap_or_default());
        let paths = measurements
            .paths
            .into_iter()
            .map(|(path, span)| json!({"path":path,"calls":span.calls,"total_ms":span.total_ms}))
            .collect::<Vec<_>>();
        let databases = measurements.databases.into_iter().map(|((database, stage), span)| {
            json!({"database_path_sha256":database,"stage":stage,"calls":span.calls,"total_ms":span.total_ms})
        }).collect::<Vec<_>>();
        let (transactions, omitted) = transactions::take();
        json!({"total_ms":self.started.elapsed().as_secs_f64()*1000.0,"spans":measurements.spans,"paths":paths,"paths_omitted":measurements.paths_omitted,"databases":databases,"transactions":transactions,"transactions_omitted":omitted,"read_pool_checkout_histogram":{"bounds_ms":READ_POOL_BOUNDS_MS,"counts":measurements.read_checkout_histogram}})
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = None);
        transactions::reset();
    }
}

pub struct Span {
    name: &'static str,
    active: Option<(Instant, u64, Vec<&'static str>)>,
    database: Option<String>,
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
        let active = CURRENT.with_borrow_mut(|current| {
            let current = current.as_mut()?;
            let id = current.sequence;
            current.sequence += 1;
            let path = current.path(name);
            current.active.push((id, name));
            Some((Instant::now(), id, path))
        });
        Self {
            name,
            active,
            database: None,
        }
    }
    pub(crate) fn for_database(name: &'static str, database: &str) -> Self {
        let mut span = Self::new(name);
        if span.active.is_some() {
            span.database = Some(database.into());
        }
        span
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((started, id, path)) = self.active.take() {
            CURRENT.with_borrow_mut(|current| {
                if let Some(current) = current
                    && let Some(index) =
                        current.active.iter().rposition(|(active, _)| *active == id)
                {
                    current.active.remove(index);
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    current.record(self.name, path, elapsed);
                    if let Some(database) = self.database.take() {
                        let key = (database, self.name);
                        if current.databases.len() < MAX_PATHS
                            || current.databases.contains_key(&key)
                        {
                            let measurement = current.databases.entry(key).or_default();
                            measurement.calls += 1;
                            measurement.total_ms += elapsed;
                        } else {
                            current.paths_omitted += 1;
                        }
                    }
                }
            });
        }
    }
}
pub fn record(name: &'static str, milliseconds: f64) {
    CURRENT.with_borrow_mut(|current| {
        if let Some(current) = current {
            current.record(name, current.path(name), milliseconds);
        }
    });
}

pub(crate) fn trace_active() -> bool {
    CURRENT.with_borrow(Option::is_some)
}

#[test]
fn paths_keep_inclusive_timings_separate_from_flat_counts() {
    let trace = Trace::start();
    {
        let _outer = Span::new("outer");
        let _inner = Span::new("inner");
        record("point", 0.0);
    }
    record("point", 0.0);
    let report = trace.finish();
    assert_eq!(report["spans"]["point"]["calls"], 2);
    let paths = report["paths"].as_array().unwrap();
    assert_eq!(paths[2]["path"], json!(["outer", "inner", "point"]));
    assert_eq!(paths[3]["path"], json!(["point"]));
    assert!(paths[0]["total_ms"].as_f64() >= paths[1]["total_ms"].as_f64());
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
