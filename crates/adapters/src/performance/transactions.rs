//! Opt-in individual transaction boundaries, with bounded identities and loss accounting.
use super::CURRENT;
use devdesk_core::concurrency::{CommitReceipt, PreparedChangeSet};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
const LIMIT: usize = 64;
#[derive(Default)]
struct Samples {
    rows: Vec<Value>,
    omitted: u64,
    reserved: usize,
}
thread_local! {static SAMPLES:RefCell<Samples>=RefCell::new(Samples::default());}
pub(super) fn reset() {
    SAMPLES.with(|s| *s.borrow_mut() = Samples::default());
}
pub(super) fn take() -> (Vec<Value>, u64) {
    SAMPLES.with(|s| {
        let s = s.take();
        (s.rows, s.omitted)
    })
}
fn wall() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) struct TransactionTiming {
    began: Instant,
    begin_attempt: Option<Instant>,
    transaction: Option<Instant>,
    commit: Option<Instant>,
    rollback: Option<Instant>,
    row: Value,
}
impl TransactionTiming {
    pub(crate) fn new(database: &str, prepared: &PreparedChangeSet, attempt: u64) -> Option<Self> {
        if !CURRENT.with(|c| c.borrow().is_some()) {
            return None;
        }
        if !SAMPLES.with(|s| {
            let mut s = s.borrow_mut();
            if s.rows.len() + s.reserved >= LIMIT {
                s.omitted += 1;
                false
            } else {
                s.reserved += 1;
                true
            }
        }) {
            return None;
        }
        let keys=prepared.changes.iter().take(16).map(|c| json!({"kind":c.resource.kind,"resource_key_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&c.resource).unwrap())),"expected_revision":prepared.read_set.require(&c.resource).ok()})).collect::<Vec<_>>();
        Some(Self {
            began: Instant::now(),
            begin_attempt: None,
            transaction: None,
            commit: None,
            rollback: None,
            row: json!({"database_path_sha256":database,"command_id_sha256":format!("{:x}",Sha256::digest(prepared.command_id.as_bytes())),"attempt":attempt,"observed_at_ms":wall(),"outcome":"interrupted","changed_resources":keys,"changed_resource_count":prepared.changes.len(),"resource_keys_truncated":prepared.changes.len()>16}),
        })
    }
    pub(crate) fn pool_acquired(&mut self) {
        self.row["submit_pool_wait_ms"] = json!(self.began.elapsed().as_secs_f64() * 1000.0);
    }
    pub(crate) fn beginning(&mut self) {
        self.begin_attempt = Some(Instant::now());
    }
    pub(crate) fn began(&mut self) {
        let clock = Instant::now();
        self.row["begin_wait_ms"] = json!(
            clock
                .duration_since(self.begin_attempt.unwrap())
                .as_secs_f64()
                * 1000.0
        );
        self.row["begin_returned_at_ms"] = json!(wall());
        self.transaction = Some(clock);
    }
    pub(crate) fn committing(&mut self) {
        self.commit = Some(Instant::now());
    }
    pub(crate) fn commit_returned(&mut self, success: bool) {
        let clock = Instant::now();
        self.row["commit_ms"] =
            json!(clock.duration_since(self.commit.unwrap()).as_secs_f64() * 1000.0);
        self.row["transaction_ms"] = json!(
            clock
                .duration_since(self.transaction.unwrap())
                .as_secs_f64()
                * 1000.0
        );
        self.row["commit_returned_at_ms"] = json!(wall());
        self.row["commit_succeeded"] = json!(success);
    }
    pub(crate) fn rolling_back(&mut self) {
        self.rollback = Some(Instant::now());
    }
    pub(crate) fn rollback_returned(&mut self, success: bool) {
        self.row["rollback_ms"] = json!(self.rollback.unwrap().elapsed().as_secs_f64() * 1000.0);
        self.row["rollback_succeeded"] = json!(success);
        self.row["transaction_ms"] =
            json!(self.transaction.unwrap().elapsed().as_secs_f64() * 1000.0);
    }
    pub(crate) fn finished(&mut self, result: &devdesk_domain::Result<(CommitReceipt, bool)>) {
        self.row["outcome"] = json!(match result {
            Ok((_, true)) => "committed",
            Ok((_, false)) => "replayed",
            Err(_) if self.commit.is_some() => "commit_failed",
            Err(_) if self.transaction.is_some() => "rejected",
            Err(_) if self.begin_attempt.is_some() => "begin_failed",
            Err(_) => "pool_or_receipt_failed",
        });
        match result {
            Ok((receipt, _)) => {
                self.row["event_cursor"] = json!(receipt.cursor);
                self.row["version_count"] = json!(receipt.versions.len());
                self.row["committed_versions"]=json!(receipt.versions.iter().take(16).map(|v|json!({"kind":v.resource.kind,"resource_key_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&v.resource).unwrap())),"revision":v.revision})).collect::<Vec<_>>());
            }
            Err(error) => {
                self.row["error_code"] = json!(error.code.chars().take(80).collect::<String>())
            }
        }
    }
}
impl Drop for TransactionTiming {
    fn drop(&mut self) {
        self.row["attempt_ms"] = json!(self.began.elapsed().as_secs_f64() * 1000.0);
        SAMPLES.with(|s| {
            let mut s = s.borrow_mut();
            s.reserved = s.reserved.saturating_sub(1);
            if s.rows.len() < LIMIT {
                s.rows.push(std::mem::take(&mut self.row));
            } else {
                s.omitted += 1;
            }
        });
    }
}
