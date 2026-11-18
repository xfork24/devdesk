//! Bounded scheduler workers. This registry deduplicates coordinator stages;
//! requests and resource commits never acquire it to read or write business data.
use devdesk_domain::{Result, err};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
};
const WORKERS: usize = 8;
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
}
impl Background {
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
                .filter_map(|key| jobs.remove(&key))
                .collect::<Vec<_>>()
        };
        let mut failure = None;
        for job in finished {
            match job.handle.join() {
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(_) => {
                    failure.get_or_insert_with(|| {
                        err("scheduler_panic", "后台阶段发生 panic，原持久意图保留")
                    });
                }
                _ => {}
            }
        }
        failure.map_or(Ok(()), Err)
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
        if self.stopped.load(Ordering::Acquire) || jobs.len() >= WORKERS || jobs.contains_key(&key)
        {
            return Ok(false);
        }
        let stage = key.clone();
        let handle = std::thread::Builder::new()
            .name(format!("devdesk-stage-{}", jobs.len()))
            .spawn(move || {
                let trace = crate::performance_logging()
                    .then(devdesk_adapters::performance::Trace::start);
                let result = work();
                if let Some(trace) = trace {
                    eprintln!("{}", serde_json::json!({"type":"performance","context":"scheduler_stage","stage":stage,"success":result.is_ok(),"measurement":trace.finish()}));
                }
                result
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
        let mut failure = None;
        for (_, job) in jobs {
            match job.handle.join() {
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(_) => {
                    failure.get_or_insert_with(|| {
                        err("scheduler_panic", "停止时后台阶段 panic，原持久事实保留")
                    });
                }
                _ => {}
            }
        }
        failure.map_or(Ok(()), Err)
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
