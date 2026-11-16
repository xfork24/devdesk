//! Native preparation for one already reserved execution. Threads never write
//! engineering state or send a Turn; the core accepts the actual receipt first.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

enum Outcome {
    Pending(Receiver<Result<SessionReceipt>>),
    Complete(Result<SessionReceipt>),
}
pub(super) struct Start {
    operation: Id,
    fingerprint: String,
    cancelled: Arc<AtomicBool>,
    stopped: bool,
    outcome: Outcome,
}
impl Start {
    fn poll(&mut self) -> Result<Option<SessionReceipt>> {
        if self.stopped {
            return Err(err("cancelled", "原启动操作已停止，不能重新执行").technical_not_applied());
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Err(
                err("needs_reconciliation", "启动正在停止，必须先核对旧执行者").technical_unknown(),
            );
        }
        if let Outcome::Pending(rx) = &self.outcome {
            let result = match rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => Err(err(
                    "needs_reconciliation",
                    "启动线程中断，必须核对原执行身份",
                )
                .technical_unknown()),
            };
            self.outcome = Outcome::Complete(result);
        }
        match &self.outcome {
            Outcome::Complete(result) => result.clone().map(Some),
            Outcome::Pending(_) => unreachable!(),
        }
    }
}

impl RuntimeWorkers {
    pub(super) fn begin_start(
        self: &Arc<Self>,
        request: &SessionStart,
    ) -> Result<Option<SessionReceipt>> {
        let mut starts = self.starts.lock().map_err(storage)?;
        let fingerprint = configuration_digest(&json!(request))?;
        if let Some(start) = starts.get_mut(&request.session.id) {
            if start.operation == request.execution.operation_id {
                require(start.fingerprint == fingerprint, "启动身份被不同输入复用")?;
                return start.poll();
            }
            require(start.stopped, "前一启动操作尚未取得停止证明")?;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let worker = self.clone();
        let owned = request.clone();
        // Core reserved this Turn/Execution against the existing quota before
        // calling us. At most one startup job exists per reserved operation.
        std::thread::Builder::new().name("devdesk-native-start".into()).spawn(move||{
            let trace=std::env::var_os("DEVDESK_PERF_LOG").is_some_and(|v|v=="1").then(performance::Trace::start);
            let result=worker.open_guarded(&owned,Some(&flag));
            if let Some(trace)=trace {eprintln!("{}",json!({"type":"native_startup","operation_id":owned.execution.operation_id,"success":result.is_ok(),"measurement":trace.finish()}));}
            let _=tx.send(result);
        }).map_err(|_|err("provider_unavailable","无法创建启动线程，未发送输入").technical_not_applied())?;
        starts.insert(
            request.session.id.clone(),
            Start {
                operation: request.execution.operation_id.clone(),
                fingerprint,
                cancelled,
                stopped: false,
                outcome: Outcome::Pending(rx),
            },
        );
        Ok(None)
    }
    pub(super) fn poll_start(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
    ) -> Result<Option<SessionReceipt>> {
        let mut starts = self.starts.lock().map_err(storage)?;
        let start = starts.get_mut(&session.id).ok_or_else(|| {
            err(
                "needs_reconciliation",
                "本进程没有原启动操作，禁止重开或重发输入",
            )
        })?;
        require(start.operation == execution.operation_id, "启动代次不匹配")?;
        start.poll()
    }
    pub(super) fn cancel_start(&self, session: &str, operation: &str) -> Result<()> {
        if let Some(start) = self.starts.lock().map_err(storage)?.get(session) {
            require(start.operation == operation, "停止不能撤销其他启动代次")?;
            start.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(super) fn stopped_start(&self, session: &str, operation: &str) -> Result<()> {
        if let Some(start) = self.starts.lock().map_err(storage)?.get_mut(session) {
            require(start.operation == operation, "停止证明不属于当前启动代次")?;
            start.stopped = true;
        }
        Ok(())
    }
    // Called only after core durably released the execution and Docker cleanup
    // succeeded. Replay authority stays in that durable ledger, not this cache.
    pub(super) fn release_start(&self, session: &str, operation: &str) -> Result<()> {
        let mut starts = self.starts.lock().map_err(storage)?;
        if let Some(start) = starts.get(session)
            && start.operation == operation
        {
            require(start.stopped, "启动尚未取得停止证明")?;
            starts.remove(session);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_drops_only_the_stopped_operation_cache() {
        let root = tempfile::tempdir().unwrap();
        let workers = RuntimeWorkers::new(root.path()).unwrap();
        workers.starts.lock().unwrap().insert(
            "session".into(),
            Start {
                operation: "current".into(),
                fingerprint: "fixture".into(),
                cancelled: Arc::new(AtomicBool::new(false)),
                stopped: false,
                outcome: Outcome::Complete(Err(err("fixture", "fixture"))),
            },
        );
        assert!(workers.release_start("session", "current").is_err());
        workers.release_start("session", "old").unwrap();
        assert_eq!(workers.starts.lock().unwrap().len(), 1);
        workers.stopped_start("session", "current").unwrap();
        workers.release_start("session", "current").unwrap();
        assert!(workers.starts.lock().unwrap().is_empty());
    }
    #[test]
    fn receipt_errors_and_stopped_jobs_are_never_restarted_by_polling() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut start = Start {
            operation: "op".into(),
            fingerprint: "fixture".into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            stopped: false,
            outcome: Outcome::Pending(rx),
        };
        assert!(start.poll().unwrap().is_none());
        tx.send(Err(
            err("provider_failure", "fixture failure").technical_unknown()
        ))
        .unwrap();
        for _ in 0..3 {
            assert_eq!(start.poll().unwrap_err().code, "provider_failure");
        }
        start.cancelled.store(true, Ordering::Release);
        assert_eq!(start.poll().unwrap_err().code, "needs_reconciliation");
        start.stopped = true;
        assert_eq!(start.poll().unwrap_err().code, "cancelled");
    }
    #[test]
    fn lost_startup_thread_is_unknown_and_does_not_create_a_new_operation() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(tx);
        let mut start = Start {
            operation: "original".into(),
            fingerprint: "fixture".into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            stopped: false,
            outcome: Outcome::Pending(rx),
        };
        for _ in 0..3 {
            let error = start.poll().unwrap_err();
            assert_eq!(error.code, "needs_reconciliation");
            assert_eq!(start.operation, "original");
        }
    }
    #[test]
    fn cancellation_winning_the_cell_lock_prevents_late_preparation() {
        let root = tempfile::tempdir().unwrap();
        let workers = RuntimeWorkers::new(root.path()).unwrap();
        let session = id();
        let operation = id();
        let request:SessionStart=serde_json::from_value(json!({"session":{"id":session,"owner_id":"fixture","agent":"fixture","harness":"fixture","sandbox":"fixture","origin":"external"},"agent":{"id":"fixture","family":"agent","name":"fixture","version":1,"body":""},"skills":[],"execution":{"id":id(),"kind":"harness","session_id":session,"operation_id":operation,"image_digest":"fixture","status":"prepared","writer_revoked":false},"project_root":root.path(),"documents":[]})).unwrap();
        let cell = workers.cell(&session).unwrap();
        let held = cell.lock().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let token = cancelled.clone();
            let worker = workers.clone();
            let task = scope.spawn(move || worker.open_guarded(&request, Some(&token)));
            cancelled.store(true, Ordering::Release);
            drop(held);
            let error = task.join().unwrap().unwrap_err();
            assert_eq!(error.code, "cancelled");
            assert_eq!(
                error.execution_failure.unwrap().effect,
                ExecutionFailureEffect::NotApplied
            );
        });
        assert!(cell.lock().unwrap().is_none());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
