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
