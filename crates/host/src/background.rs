//! Bounded scheduler workers. This registry deduplicates coordinator stages;
//! requests and resource commits never acquire it to read or write business data.
use devdesk_domain::{Error, Result, configuration_hash, err};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
};
const WORKERS: usize = 8;
struct ConflictRetry {
    signature: String,
    attempts: u32,
    first: std::time::Instant,
    next: std::time::Instant,
    reported: Option<std::time::Instant>,
}
pub(super) fn failures(operation: &str, values: Vec<Value>) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    Err(err(
        "scheduler_stage_failed",
        format!("{} 个调度操作失败", values.len()),
    )
    .with_context(operation, json!({"failures":values})))
}
fn stage_name(key: &str) -> &str {
    let suffix = key.rsplit(':').next().unwrap_or(key);
    match suffix.parse::<usize>() {
        Ok(index) => devdesk_core::SchedulingStage::ALL
            .get(index)
            .map(|s| s.name())
            .unwrap_or(if index == devdesk_core::SchedulingStage::ALL.len() {
                "catalog"
            } else {
                "workflow_intents"
            }),
        Err(_) => suffix,
    }
}
pub(super) struct Wake {
    pub notify: Arc<tokio::sync::Notify>,
    listener: Arc<dyn Fn() + Send + Sync>,
}
impl Default for Wake {
    fn default() -> Self {
        let notify = Arc::new(tokio::sync::Notify::new());
        let signal = notify.clone();
        Self {
            notify,
            listener: Arc::new(move || signal.notify_one()),
        }
    }
}
impl Wake {
    pub fn listener(&self) -> std::sync::Weak<dyn Fn() + Send + Sync> {
        Arc::downgrade(&self.listener)
    }
}
struct Job {
    project: String,
    reserved_conversations: Option<usize>,
    handle: JoinHandle<Result<()>>,
}
#[derive(Default)]
pub(super) struct Background {
    jobs: Mutex<BTreeMap<String, Job>>,
    pub stopped: Arc<AtomicBool>,
    pub stage_next: AtomicUsize,
    pub catalog_next: AtomicUsize,
    pub wake: Wake,
    pub runnable: AtomicBool,
    maintenance: Mutex<Option<std::time::Instant>>,
    idle_stages: Mutex<BTreeMap<String, (u64, std::time::Instant)>>,
    conflicts: Mutex<BTreeMap<String, ConflictRetry>>,
}
impl Background {
    pub(super) fn clear_conflict(&self, key: &str) -> Result<()> {
        self.conflicts
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?
            .remove(key);
        Ok(())
    }
    pub(super) fn retry_conflict(&self, key: &str, error: &Error) -> Result<Option<Error>> {
        // Re-read on the next scheduling attempt. Never label this as an idle
        // successful stage or reuse its old task/view/command with a new identity.
        self.idle_stages
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?
            .remove(key);
        let mut conflicts = self
            .conflicts
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        let signature = configuration_hash(&json!(error));
        let now = std::time::Instant::now();
        if conflicts.len() >= 128
            && !conflicts.contains_key(key)
            && let Some(oldest) = conflicts
                .iter()
                .min_by_key(|(_, state)| state.first)
                .map(|(key, _)| key.clone())
        {
            conflicts.remove(&oldest);
        }
        let state = conflicts
            .entry(key.into())
            .or_insert_with(|| ConflictRetry {
                signature: signature.clone(),
                attempts: 0,
                first: now,
                next: now,
                reported: None,
            });
        if state.signature != signature {
            *state = ConflictRetry {
                signature,
                attempts: 0,
                first: now,
                next: now,
                reported: None,
            };
        }
        state.attempts = state.attempts.saturating_add(1);
        state.next =
            now + std::time::Duration::from_millis((25u64 << state.attempts.min(6)).min(1000));
        if state.attempts >= 8
            && state.first.elapsed() >= std::time::Duration::from_secs(5)
            && state
                .reported
                .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(30))
        {
            state.reported = Some(now);
            return Ok(Some(err("scheduler_retry_stalled", "调度阶段持续遇到相同冲突，自动重试未取得进展")
                .with_context("Background::retry_conflict", json!({"attempts":state.attempts,"elapsed_ms":state.first.elapsed().as_millis(),"retry_scheduled":true,"last_error":error}))));
        }
        Ok(None)
    }
    fn finish_jobs(&self, jobs: Vec<(String, Job)>, stopping: bool) -> Result<()> {
        let mut errors = vec![];
        for (key, job) in jobs {
            let result = job.handle.join().unwrap_or_else(|payload| {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic payload");
                Err(
                    err("scheduler_panic", "后台调度阶段发生 panic，原持久意图保留")
                        .with_context("Background::finish_jobs", json!({"panic_message":message})),
                )
            });
            let error = match result {
                Err(error) if error.retryable_conflict() => {
                    if stopping {
                        None
                    } else {
                        self.retry_conflict(&key, &error)?
                    }
                }
                other => {
                    self.conflicts
                        .lock()
                        .map_err(|e| err("scheduler", e.to_string()))?
                        .remove(&key);
                    other.err()
                }
            };
            if let Some(error) = error {
                errors.push(json!({"project_id":job.project,"stage":stage_name(&key),"stage_key":key,"stopping":stopping,"error":error}));
            }
        }
        failures("Background::finish_jobs", errors)
    }
    pub fn stage_due(&self, key: &str, cursor: u64) -> Result<bool> {
        let idle = self
            .idle_stages
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        Ok(idle.get(key).is_none_or(|(observed, since)| {
            *observed != cursor || since.elapsed() >= std::time::Duration::from_secs(1)
        }))
    }
    pub fn stage_finished(&self, key: String, before: u64, after: u64) -> Result<()> {
        let mut idle = self
            .idle_stages
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        if before == after {
            if idle.len() >= 128 && !idle.contains_key(&key) {
                let oldest = idle
                    .iter()
                    .min_by_key(|(_, (_, since))| *since)
                    .map(|(key, _)| key.clone())
                    .unwrap();
                idle.remove(&oldest);
            }
            idle.insert(key, (after, std::time::Instant::now()));
        } else {
            idle.remove(&key);
        }
        Ok(())
    }
    pub fn maintenance_due(&self) -> Result<bool> {
        let mut last = self
            .maintenance
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        if last.is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(1)) {
            return Ok(false);
        }
        *last = Some(std::time::Instant::now());
        Ok(true)
    }
    pub fn poll(&self) -> Result<()> {
        let finished = {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|e| err("scheduler", e.to_string()))?;
            let keys = jobs
                .iter()
                .filter(|(_, job)| job.handle.is_finished())
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| jobs.remove(&key).map(|job| (key, job)))
                .collect::<Vec<_>>()
        };
        self.finish_jobs(finished, false)
    }
    #[cfg(test)]
    pub fn drain(&self, stop: impl Fn() -> bool) -> Result<()> {
        let began = std::time::Instant::now();
        loop {
            if stop() {
                self.stopped.store(true, Ordering::Release);
            }
            self.poll()?;
            if self
                .jobs
                .lock()
                .map_err(|e| err("scheduler", e.to_string()))?
                .is_empty()
            {
                return Ok(());
            }
            if began.elapsed() > std::time::Duration::from_secs(10) {
                return Err(err("fixture_timeout", "后台阶段未在夹具期限内完成"));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    pub fn reserve_count(&self, running: &BTreeMap<String, usize>) -> Result<usize> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        Ok(jobs
            .values()
            .filter_map(|j| {
                j.reserved_conversations.map(|allowance| {
                    allowance.saturating_sub(*running.get(&j.project).unwrap_or(&0))
                })
            })
            .sum())
    }
    pub fn spawn(
        &self,
        key: String,
        project: String,
        reservation: Option<usize>,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<bool> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?;
        if self
            .conflicts
            .lock()
            .map_err(|e| err("scheduler", e.to_string()))?
            .get(&key)
            .is_some_and(|retry| std::time::Instant::now() < retry.next)
        {
            return Ok(false);
        }
        if self.stopped.load(Ordering::Acquire) || jobs.len() >= WORKERS || jobs.contains_key(&key)
        {
            return Ok(false);
        }
        let stage = key.clone();
        let work_project = project.clone();
        let handle = std::thread::Builder::new()
            .name(format!("devdesk-stage-{}", jobs.len()))
            .spawn(move || {
                let trace = crate::performance_logging()
                    .then(devdesk_adapters::performance::Trace::start);
                let result = work();
                if let Some(trace) = trace {
                    eprintln!("{}", serde_json::json!({"type":"performance","context":"scheduler_stage","stage":stage,"success":result.is_ok(),"measurement":trace.finish()}));
                }
                result.map_err(|error| {
                    if error.retryable_conflict() { error } else {
                        error.with_context("scheduler worker", json!({"project_id":work_project,"stage":stage_name(&stage),"stage_key":stage,"boundary_backtrace":std::backtrace::Backtrace::force_capture().to_string()}))
                    }
                })
            })
            .map_err(|e| err("scheduler", e.to_string()))?;
        jobs.insert(
            key,
            Job {
                project,
                reserved_conversations: reservation,
                handle,
            },
        );
        Ok(true)
    }
    pub fn stop_and_drain(&self) -> Result<()> {
        self.stopped.store(true, Ordering::Release);
        let jobs = std::mem::take(
            &mut *self
                .jobs
                .lock()
                .map_err(|e| err("scheduler", e.to_string()))?,
        );
        self.finish_jobs(jobs.into_iter().collect(), true)
    }
}

#[cfg(test)]
mod idle_tests {
    use super::*;
    #[test]
    fn unchanged_stage_waits_for_a_commit_or_bounded_fallback_without_hiding_new_work() {
        let background = Background::default();
        assert!(background.stage_due("project:stage:driver", 3).unwrap());
        background
            .stage_finished("project:stage:driver".into(), 3, 3)
            .unwrap();
        assert!(!background.stage_due("project:stage:driver", 3).unwrap());
        assert!(background.stage_due("project:stage:driver", 4).unwrap());
        assert!(background.stage_due("other:stage:driver", 3).unwrap());
        background
            .idle_stages
            .lock()
            .unwrap()
            .get_mut("project:stage:driver")
            .unwrap()
            .1 -= std::time::Duration::from_secs(2);
        assert!(background.stage_due("project:stage:driver", 3).unwrap());
        background
            .stage_finished("project:stage:driver".into(), 3, 4)
            .unwrap();
        assert!(background.stage_due("project:stage:driver", 4).unwrap());
        for n in 0..130 {
            background
                .stage_finished(format!("other-{n}"), 4, 4)
                .unwrap();
        }
        assert_eq!(background.idle_stages.lock().unwrap().len(), 128);
        assert!(background.stage_due("project:stage:driver", 4).unwrap());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    fn wait_finished(background: &Background) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while background
            .jobs
            .lock()
            .unwrap()
            .values()
            .any(|job| !job.handle.is_finished())
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn conflict_releases_the_stage_and_idle_hint_then_recovers_without_failure() {
        let background = Background::default();
        let key = "project:stage:5";
        background.stage_finished(key.into(), 1, 1).unwrap();
        background
            .spawn(key.into(), "project".into(), None, || {
                Err(err("revision_conflict", "原步骤在准备期间已变化"))
            })
            .unwrap();
        wait_finished(&background);
        background.poll().unwrap();
        assert!(background.jobs.lock().unwrap().is_empty());
        assert!(background.stage_due(key, 1).unwrap());
        assert!(
            !background
                .spawn(key.into(), "project".into(), None, || panic!(
                    "cooldown must defer this attempt"
                ))
                .unwrap()
        );
        background
            .conflicts
            .lock()
            .unwrap()
            .get_mut(key)
            .unwrap()
            .next = std::time::Instant::now();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        background
            .spawn(key.into(), "project".into(), None, move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        wait_finished(&background);
        background.poll().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(!background.conflicts.lock().unwrap().contains_key(key));
    }
    #[test]
    fn real_errors_and_panic_payloads_are_all_retained_alongside_normal_conflicts() {
        let background = Background::default();
        for (key, code) in [
            ("one:stage:1", "storage"),
            ("two:stage:3", "adapter_contract"),
            ("retry:stage:4", "revision_conflict"),
        ] {
            background
                .spawn(
                    key.into(),
                    key.split(':').next().unwrap().into(),
                    None,
                    move || Err(err(code, format!("failure in {key}"))),
                )
                .unwrap();
        }
        background
            .spawn("panic:stage:0".into(), "panic".into(), None, || {
                panic!("original stage panic details")
            })
            .unwrap();
        wait_finished(&background);
        let error = background.poll().unwrap_err();
        let failures = error.context[0].details["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 3);
        assert!(failures.iter().all(|f| f["project_id"] != "retry"));
        assert!(failures.iter().any(|f| f["project_id"] == "one"
            && f["stage"] == "workflows"
            && f["error"]["code"] == "storage"));
        assert!(
            failures
                .iter()
                .any(|f| f["project_id"] == "two" && f["stage"] == "tools")
        );
        assert!(
            failures
                .iter()
                .any(|f| f["error"]["context"][0]["details"]["panic_message"]
                    == "original stage panic details")
        );
        assert!(failures.iter().find(|f|f["project_id"] == "one").unwrap()["error"]["context"][0]["details"]["boundary_backtrace"].as_str().is_some_and(|s|!s.is_empty()));
        assert!(background.jobs.lock().unwrap().is_empty());
    }
    #[test]
    fn repeated_unchanged_conflicts_are_reported_but_recovery_remains_enabled() {
        let background = Background::default();
        let key = "stalled:stage:7";
        let original = err("revision_conflict", "unchanged stale selection");
        for _ in 0..7 {
            assert!(background.retry_conflict(key, &original).unwrap().is_none());
        }
        background
            .conflicts
            .lock()
            .unwrap()
            .get_mut(key)
            .unwrap()
            .first -= std::time::Duration::from_secs(6);
        let failure = background.retry_conflict(key, &original).unwrap().unwrap();
        assert_eq!(failure.code, "scheduler_retry_stalled");
        assert_eq!(failure.context[0].details["retry_scheduled"], true);
        assert_eq!(
            failure.context[0].details["last_error"]["code"],
            "revision_conflict"
        );
        assert!(
            background.retry_conflict(key, &original).unwrap().is_none(),
            "repeated diagnostics must be rate-limited"
        );
        let fresh = original.with_context("changed step", json!({"actual_revision":2}));
        assert!(background.retry_conflict(key, &fresh).unwrap().is_none());
        assert_eq!(background.conflicts.lock().unwrap()[key].attempts, 1);
    }
    #[test]
    fn an_unknown_effect_is_not_suppressed_as_contention() {
        let background = Background::default();
        background
            .spawn("unknown:stage:2".into(), "unknown".into(), None, || {
                Err(err("revision_conflict", "effect requires reconciliation").technical_unknown())
            })
            .unwrap();
        wait_finished(&background);
        let error = background.poll().unwrap_err();
        assert_eq!(
            error.context[0].details["failures"][0]["error"]["execution_failure"]["effect"],
            "unknown"
        );
        assert!(background.conflicts.lock().unwrap().is_empty());
    }
}
