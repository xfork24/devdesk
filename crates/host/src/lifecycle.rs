//! Instance-scoped optimistic lifecycle admission; locks cover bookkeeping only.
use devdesk_domain::{Result, err};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const EMPTY: u64 = 0;
const OPENING: u64 = 1;
const READY: u64 = 2;
const STOPPING: u64 = 3;
const MAX_OPERATIONS: usize = 128;

struct Operation {
    fingerprint: String,
    result: Option<Value>,
}
pub struct Lifecycle {
    instance: String,
    state: AtomicU64,
    operations: Mutex<BTreeMap<String, Operation>>,
}
pub struct Ticket {
    instance: String,
    command: String,
    state: u64,
}
pub enum Admission {
    New(Ticket),
    Replay(Value),
}
impl Lifecycle {
    pub fn new(initialized: bool) -> Self {
        Self {
            instance: devdesk_core::id(),
            state: AtomicU64::new(if initialized { 4 | READY } else { EMPTY }),
            operations: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn snapshot(&self) -> Value {
        let state = self.state.load(Ordering::Acquire);
        json!({"instance_id":self.instance,"revision":state >> 2,"status":match state & 3 {
            EMPTY=>"uninitialized", OPENING=>"initializing", READY=>"ready", _=>"stopping"
        }})
    }
    pub fn begin(&self, setup: bool, params: &Value) -> Result<Admission> {
        let fields = params
            .as_object()
            .ok_or_else(|| err("invalid_params", "生命周期请求必须是对象"))?;
        let allowed: &[&str] = if setup {
            &["command_id", "instance_id", "expected_revision", "path"]
        } else {
            &["command_id", "instance_id", "expected_revision"]
        };
        if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(err("invalid_params", "生命周期请求包含未知字段"));
        }
        let command = params["command_id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .ok_or_else(|| err("invalid_params", "需要明确的 command_id"))?;
        let instance = params["instance_id"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 1024)
            .ok_or_else(|| err("precondition_required", "缺少 Host 实例前置条件"))?;
        let revision = params["expected_revision"]
            .as_u64()
            .ok_or_else(|| err("precondition_required", "缺少 Host 版本前置条件"))?;
        let fingerprint = devdesk_domain::configuration_hash(&(setup, params));
        let mut operations = self
            .operations
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        if let Some(previous) = operations.get(command) {
            if previous.fingerprint != fingerprint {
                return Err(err("invalid_params", "command_id 被不同参数复用"));
            }
            return previous
                .result
                .clone()
                .map(Admission::Replay)
                .ok_or_else(|| {
                    err(
                        "operation_in_progress",
                        "相同生命周期操作仍在执行，请用原命令核对",
                    )
                });
        }
        if instance != self.instance {
            return Err(err("revision_conflict", "Host 实例已改变，请重新连接"));
        }
        let state = self.state.load(Ordering::Acquire);
        if state >> 2 != revision {
            return Err(err("revision_conflict", "Host 状态版本已改变"));
        }
        if setup && state & 3 == OPENING {
            return Err(err(
                "operation_in_progress",
                "Host 正在打开数据集，请等待完成",
            ));
        }
        if (setup && state & 3 != EMPTY) || state & 3 == STOPPING {
            return Err(err("invalid_state", "Host 当前状态不接纳此操作"));
        }
        if operations.len() >= MAX_OPERATIONS {
            return Err(err(
                "resource_exhausted",
                "本次 Host 生命周期回执预算已用尽",
            ));
        }
        let next = next_state(state, if setup { OPENING } else { STOPPING })?;
        self.state
            .compare_exchange(state, next, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| err("revision_conflict", "Host 状态在提交时已改变"))?;
        operations.insert(
            command.into(),
            Operation {
                fingerprint,
                result: None,
            },
        );
        Ok(Admission::New(Ticket {
            instance: self.instance.clone(),
            command: command.into(),
            state: next,
        }))
    }
    /// Called after initialization I/O. A concurrent stop fences late attachment.
    pub fn finish_setup(&self, ticket: &Ticket, succeeded: bool) -> Result<()> {
        if ticket.instance != self.instance || ticket.state & 3 != OPENING {
            return Err(err("invalid_scope", "初始化意图不属于当前 Host"));
        }
        let next = next_state(ticket.state, if succeeded { READY } else { EMPTY })?;
        self.state
            .compare_exchange(ticket.state, next, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                err(
                    "revision_conflict",
                    "初始化期间 Host 已停止，结果未接入当前实例",
                )
            })?;
        Ok(())
    }
    pub fn record(&self, ticket: Ticket, mut result: Value) -> Result<Value> {
        if ticket.instance != self.instance {
            return Err(err("invalid_scope", "回执不属于当前 Host"));
        }
        result["operation_id"] = json!(ticket.command);
        result["lifecycle"] = self.snapshot();
        let mut operations = self
            .operations
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        let operation = operations
            .get_mut(&ticket.command)
            .ok_or_else(|| err("storage", "缺少生命周期意图"))?;
        operation.result = Some(result.clone());
        Ok(result)
    }
    pub fn stop_from_signal(&self) {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & 3 == STOPPING {
                return;
            }
            let Ok(next) = next_state(state, STOPPING) else {
                return;
            };
            match self
                .state
                .compare_exchange(state, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(current) => state = current,
            }
        }
    }
}
fn next_state(state: u64, phase: u64) -> Result<u64> {
    (state >> 2)
        .checked_add(1)
        .filter(|n| *n <= u64::MAX >> 2)
        .map(|revision| (revision << 2) | phase)
        .ok_or_else(|| err("resource_exhausted", "Host 版本超出范围"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    fn params(lifecycle: &Lifecycle, command: &str) -> Value {
        let snapshot = lifecycle.snapshot();
        json!({"command_id":command,"instance_id":snapshot["instance_id"],"expected_revision":snapshot["revision"]})
    }
    #[test]
    fn one_hundred_same_version_initializations_have_one_claim() {
        let lifecycle = Arc::new(Lifecycle::new(false));
        let baseline = params(&lifecycle, "initial");
        let barrier = Arc::new(Barrier::new(100));
        let threads: Vec<_> = (0..100)
            .map(|n| {
                let lifecycle = lifecycle.clone();
                let barrier = barrier.clone();
                let mut request = baseline.clone();
                request["command_id"] = json!(format!("setup-{n}"));
                std::thread::spawn(move || {
                    barrier.wait();
                    lifecycle.begin(true, &request).is_ok()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        assert_eq!(lifecycle.snapshot()["status"], "initializing");
    }
    #[test]
    fn original_replay_survives_newer_state_and_late_setup_cannot_revive_a_stop() {
        let lifecycle = Lifecycle::new(false);
        let request = params(&lifecycle, "setup");
        let Admission::New(ticket) = lifecycle.begin(true, &request).unwrap() else {
            panic!()
        };
        assert!(matches!(lifecycle.begin(true,&request),Err(e) if e.code=="operation_in_progress"));
        assert!(matches!(
            lifecycle.begin(true, &params(&lifecycle, "different-setup")),
            Err(e) if e.code == "operation_in_progress"
        ));
        lifecycle.finish_setup(&ticket, true).unwrap();
        let original = lifecycle.record(ticket, json!({"root":"fixture"})).unwrap();
        lifecycle.stop_from_signal();
        let Admission::Replay(replay) = lifecycle.begin(true, &request).unwrap() else {
            panic!()
        };
        assert_eq!(original, replay);
        let newer = Lifecycle::new(false);
        assert!(matches!(newer.begin(true,&request),Err(e) if e.code=="revision_conflict"));
        let Admission::New(ticket) = newer.begin(true, &params(&newer, "pending")).unwrap() else {
            panic!()
        };
        newer.stop_from_signal();
        assert!(newer.finish_setup(&ticket, true).is_err());
        assert_eq!(newer.snapshot()["status"], "stopping");
    }
}
