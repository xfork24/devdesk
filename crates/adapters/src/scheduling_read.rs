//! Finite scheduling hints. Dispatch still checks authoritative domain facts.
use crate::*;
use devdesk_core::read_models::SchedulingHints;
const PENDING: &str = r#"EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.status')='active') OR
          EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.status') NOT IN ('cancelled','completed') AND (json_extract(data,'$.cancel_requested')=1 OR (json_extract(data,'$.paused')=0 AND json_extract(data,'$.status') NOT IN ('failed','needs_reconciliation')))) OR
          EXISTS(SELECT 1 FROM governance_records WHERE type='platform_call' AND json_extract(data,'$.status') IN ('prepared','running')) OR
          EXISTS(SELECT 1 FROM governance_records WHERE type='tool_call' AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR ((json_extract(data,'$.status') IN ('succeeded','failed','cancelled') OR (json_extract(data,'$.status')='needs_reconciliation' AND json_type(data,'$.receipt')='object')) AND json_extract(data,'$.reply_sent')=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL))) OR
          EXISTS(SELECT 1 FROM repository_operations WHERE status IN ('prepared','dispatched','observed') OR (status='needs_reconciliation' AND reason='reconcile_requested')) OR
          EXISTS(SELECT 1 FROM turns WHERE status IN ('queued','dispatching','running','waiting_interaction') OR (status='needs_reconciliation' AND cancel_requested=1)) OR
          EXISTS(SELECT 1 FROM executions WHERE writer_revoked=0 OR status='succeeded')"#;
const RUNNING: &str = "SELECT count(*) FROM turns t WHERE t.status NOT IN ('queued','completed','failed','cancelled') AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0)";
pub(super) fn running(conn: &Connection) -> Result<usize> {
    conn.query_row(RUNNING, [], |row| row.get(0))
        .map_err(storage)
}
pub(super) fn pending(conn: &Connection) -> Result<bool> {
    conn.query_row(&format!("SELECT {PENDING}"), [], |row| row.get(0))
        .map_err(storage)
}
pub(super) fn hints(conn: &Connection) -> Result<SchedulingHints> {
    let _span = performance::Span::new("read_model.scheduling_hints");
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let (pending,live)=tx.query_row(&format!("SELECT ({PENDING}), NOT EXISTS(SELECT 1 FROM projects WHERE execution_mode!='live')"),[],|row|Ok((row.get(0)?,row.get(1)?))).map_err(storage)?;
    let running_conversations = running(&tx)?;
    tx.commit().map_err(storage)?;
    Ok(SchedulingHints {
        pending,
        live,
        running_conversations,
    })
}
