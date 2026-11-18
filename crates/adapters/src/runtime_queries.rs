//! Workflow scheduling reads through the shared pool, without restoring history.
use super::*;

// Shared by admission hints and cursor pages. An active authorization alone
// must not restore a waiting controller on every scheduler pass.
pub(super) const CONTROLLER_READY: &str = "(
 coalesce(json_extract(c.data,'$.run_state'),'') IN ('','runnable','running')
 OR EXISTS(SELECT 1 FROM turns t WHERE t.session_id=json_extract(c.data,'$.session_id')
   AND t.status IN ('queued','dispatching','running','waiting_interaction','needs_reconciliation','failed','cancelled'))
 OR (coalesce(json_extract(c.data,'$.workspace_only'),0)=0
   AND json_extract(c.data,'$.bootstrap_epoch')!=json_extract(c.data,'$.epoch'))
 OR json_extract(c.data,'$.wait_deadline')<=CAST(strftime('%s','now') AS INTEGER)*1000
 OR EXISTS(SELECT 1 FROM governance_records u WHERE u.type='controller_input' AND u.definition_id=c.id AND json_extract(u.data,'$.consumed')=0)
 OR EXISTS(SELECT 1 FROM events e WHERE e.kind='workflow.runtime' AND e.id>coalesce(json_extract(c.data,'$.source_cursor'),0)
   AND json_extract(CASE WHEN e.kind='workflow.runtime' THEN e.detail ELSE '{}' END,'$.root_instance_id')=json_extract(c.data,'$.root_instance_id'))
)";

// The display rows are transactional hints, never dispatch authority. Include
// status repairs and cancellation as well as actual running receipts.
pub(super) const WORKFLOW_READY: &str = "(
 json_extract(c.data,'$.status') NOT IN ('completed','cancelled') AND (
 coalesce(json_extract(c.data,'$.cancel_requested'),0)=1
 OR EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=c.id AND (
   json_extract(s.data,'$.status')='running'
   OR (json_extract(s.data,'$.status')='needs_reconciliation' AND
      json_extract(c.data,'$.status')!='needs_reconciliation')
   OR (json_extract(s.data,'$.status')='failed' AND json_extract(c.data,'$.status') NOT IN ('failed','needs_reconciliation'))
   OR (json_extract(c.data,'$.status')='queued' AND json_extract(s.data,'$.started') IS NOT NULL)))
 OR EXISTS(SELECT 1 FROM governance_records stop WHERE stop.type='workflow_stop'
   AND stop.definition_id=json_extract(c.data,'$.parent_execution_id'))
 OR EXISTS(SELECT 1 FROM governance_records a WHERE a.type='workflow_instance'
   AND json_extract(a.data,'$.root_instance_id')=json_extract(c.data,'$.root_instance_id')
   AND json_extract(a.data,'$.status') NOT IN ('completed','cancelled')
   AND coalesce(json_extract(a.data,'$.cancel_requested'),0)=1)
))";

pub(super) fn controllers(conn: &Connection) -> Result<bool> {
    conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM governance_records c WHERE c.type='workflow_controller' AND json_extract(c.data,'$.status')='active' AND {CONTROLLER_READY})"), [], |r|r.get(0)).map_err(storage)
}
// Values are supplied by the application clock. This hint neither authorizes
// expiry nor compares monotonic counters from different process epochs.
pub(super) const WORKFLOW_TIME_READY:&str="EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=c.id
 AND json_extract(s.data,'$.time_budget.next_deadline_at') IS NOT NULL
 AND json_extract(s.data,'$.status') IN ('queued','waiting_input','waiting_confirmation','awaiting_acceptance')
 AND (CASE WHEN ?2 IS NOT NULL AND json_extract(s.data,'$.time_budget.clock.process_epoch')=?2 THEN
   (?3 IS NULL OR json_extract(s.data,'$.time_budget.clock.monotonic_ms') IS NULL
    OR ?3<json_extract(s.data,'$.time_budget.clock.monotonic_ms')
    OR json_extract(s.data,'$.time_budget.clock.wall_ms')+?3-json_extract(s.data,'$.time_budget.clock.monotonic_ms')>=json_extract(s.data,'$.time_budget.next_deadline_at'))
 ELSE (?1<json_extract(s.data,'$.time_budget.clock.wall_ms') OR ?1>=json_extract(s.data,'$.time_budget.next_deadline_at')) END))";
pub(super) fn utc_clock() -> ExecutionClockAnchor {
    ExecutionClockAnchor {
        wall_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        process_epoch: None,
        monotonic_ms: None,
    }
}
pub(super) fn workflow_observations(
    conn: &Connection,
    clock: &ExecutionClockAnchor,
) -> Result<bool> {
    conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM governance_records c WHERE c.type='workflow_instance' AND json_extract(c.data,'$.status') IN ('queued','running','failed','needs_reconciliation') AND ({WORKFLOW_READY} OR {WORKFLOW_TIME_READY}))"), rusqlite::params![clock.wall_ms,clock.process_epoch,clock.monotonic_ms], |r|r.get(0)).map_err(storage)
}

pub(super) fn running(conn: &Connection) -> Result<usize> {
    let _span = performance::Span::new("runtime.running_count");
    conn.query_row("SELECT count(*) FROM turns t
        WHERE t.status IN ('dispatching','running','waiting_interaction','needs_reconciliation')
        AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0)",
        [], |row| row.get(0)).map_err(storage)
}
pub(super) fn native_prepared(conn: &Connection, id: &str) -> Result<bool> {
    let _span = performance::Span::new("runtime.pending_read");
    conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE id=?1 AND type='platform_call' AND json_extract(data,'$.status')='prepared' AND json_type(data,'$.request.caller')='object')",[id],|row|row.get(0)).map_err(storage)
}

pub(super) fn pending(conn: &Connection, kind: ports::WorkflowRuntimeWork) -> Result<bool> {
    let _span = performance::Span::new("runtime.pending_read");
    let query = match kind {
        ports::WorkflowRuntimeWork::Conversations => "SELECT EXISTS(SELECT 1 FROM turns t
            WHERE t.status IN ('queued','dispatching','running','waiting_interaction')
            OR (t.status='needs_reconciliation' AND (t.cancel_requested=1
              OR EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0))))",
        ports::WorkflowRuntimeWork::QueuedConversations => "SELECT EXISTS(SELECT 1 FROM turns WHERE status='queued')",
        ports::WorkflowRuntimeWork::WorkflowIntents => "SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared')",
        ports::WorkflowRuntimeWork::Controllers => return controllers(conn),
        ports::WorkflowRuntimeWork::ControllerCalls => "SELECT EXISTS(SELECT 1 FROM governance_records c
            WHERE c.type='controller_tool_call' AND (json_extract(c.data,'$.status')='prepared'
              OR (json_extract(c.data,'$.status') IN ('succeeded','failed')
                AND NOT EXISTS(SELECT 1 FROM governance_records r WHERE r.type='controller_tool_reply' AND r.definition_id=c.id))))",
        ports::WorkflowRuntimeWork::Changes => "SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_change' AND json_extract(data,'$.status') IN ('prepared','waiting_quiescence'))",
        ports::WorkflowRuntimeWork::Segments => "SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_segment' AND json_extract(data,'$.status')='active')",
        ports::WorkflowRuntimeWork::RustDrivers => "SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_driver' AND json_extract(data,'$.mode')='rust')",
        ports::WorkflowRuntimeWork::PlatformTools => {
            let active: bool = conn.query_row("SELECT
            EXISTS(SELECT 1 FROM governance_records WHERE type='platform_call' AND json_extract(data,'$.status') IN ('prepared','running')) OR
            EXISTS(SELECT 1 FROM governance_records WHERE type='agent_context_call' AND coalesce(json_extract(data,'$.reply_sent'),0)!=1)", [], |row| row.get(0)).map_err(storage)?;
            return Ok(active || pending(conn, ports::WorkflowRuntimeWork::AgentReplies)?);
        }
        ports::WorkflowRuntimeWork::AgentReplies => "SELECT EXISTS(SELECT 1 FROM governance_records a WHERE a.type='agent_platform_request'
              AND NOT EXISTS(SELECT 1 FROM governance_records r WHERE r.type='platform_tool_reply' AND r.definition_id=json_extract(a.data,'$.request.execution_id'))
              AND (EXISTS(SELECT 1 FROM governance_records c WHERE c.type='platform_call'
                  AND c.id=json_extract(a.data,'$.request.execution_id') AND json_extract(c.data,'$.status') IN ('succeeded','failed','cancelled','needs_reconciliation'))
                OR EXISTS(SELECT 1 FROM governance_records i WHERE i.type='workflow_intent' AND json_extract(i.data,'$.status')='failed'
                  AND json_extract(i.data,'$.command.payload.agent_platform_call_id')=json_extract(a.data,'$.request.execution_id'))))",
    };
    conn.query_row(query, [], |row| row.get(0)).map_err(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::WorkflowRuntimeWork::*;
    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE turns(id TEXT,session_id TEXT,status TEXT,cancel_requested BOOLEAN);
            CREATE TABLE executions(session_id TEXT,writer_revoked BOOLEAN);
            CREATE TABLE governance_records(id TEXT,type TEXT,definition_id TEXT,data JSON);
             CREATE TABLE events(id INTEGER,kind TEXT,detail TEXT);",
        )
        .unwrap();
        conn
    }
    fn record(conn: &Connection, id: &str, kind: &str, parent: &str, data: Value) {
        conn.execute(
            "INSERT INTO governance_records VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, kind, parent, data.to_string()],
        )
        .unwrap();
    }
    #[test]
    fn native_ready_hint_distinguishes_prepared_callers_from_workflow_tools_and_replays() {
        let conn = database();
        record(
            &conn,
            "native",
            "platform_call",
            "step",
            json!({"status":"prepared","request":{"caller":{"request_id":7}}}),
        );
        record(
            &conn,
            "workflow",
            "platform_call",
            "step",
            json!({"status":"prepared","request":{"caller":null}}),
        );
        record(
            &conn,
            "done",
            "platform_call",
            "step",
            json!({"status":"succeeded","request":{"caller":{"request_id":8}}}),
        );
        assert!(native_prepared(&conn, "native").unwrap());
        for id in ["workflow", "done", "missing"] {
            assert!(!native_prepared(&conn, id).unwrap());
        }
        conn.execute(
            "UPDATE governance_records SET data=?1 WHERE id='native'",
            [json!({"status":"running","request":{"caller":{"request_id":7}}}).to_string()],
        )
        .unwrap();
        assert!(!native_prepared(&conn, "native").unwrap());
    }
    #[test]
    fn count_preserves_unknown_writer_slots_without_multiplying_execution_joins() {
        let conn = database();
        for status in [
            "queued",
            "dispatching",
            "running",
            "waiting_interaction",
            "needs_reconciliation",
            "completed",
            "failed",
            "cancelled",
        ] {
            conn.execute("INSERT INTO turns VALUES(?1,'session',?1,0)", [status])
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO executions VALUES('session',0),('session',0),('session',1);",
        )
        .unwrap();
        assert_eq!(running(&conn).unwrap(), 4);
        assert!(pending(&conn, Conversations).unwrap());
        conn.execute_batch("UPDATE executions SET writer_revoked=1; DELETE FROM turns WHERE status!='needs_reconciliation';").unwrap();
        assert_eq!(running(&conn).unwrap(), 0);
        assert!(!pending(&conn, Conversations).unwrap());
        conn.execute_batch("UPDATE turns SET cancel_requested=1;")
            .unwrap();
        assert!(pending(&conn, Conversations).unwrap());
    }
    #[test]
    fn undelivered_and_failed_dispatch_receipts_remain_schedulable_until_delivery() {
        let conn = database();
        record(
            &conn,
            "call",
            "controller_tool_call",
            "manager",
            json!({"status":"succeeded"}),
        );
        assert!(pending(&conn, ControllerCalls).unwrap());
        record(
            &conn,
            "reply",
            "controller_tool_reply",
            "call",
            json!({"status":"worker_unavailable"}),
        );
        assert!(!pending(&conn, ControllerCalls).unwrap());
        record(
            &conn,
            "agent",
            "agent_platform_request",
            "step",
            json!({"request":{"execution_id":"platform"}}),
        );
        assert!(!pending(&conn, PlatformTools).unwrap());
        assert!(!pending(&conn, AgentReplies).unwrap());
        record(
            &conn,
            "intent",
            "workflow_intent",
            "step",
            json!({"status":"failed","command":{"payload":{"agent_platform_call_id":"platform"}}}),
        );
        assert!(pending(&conn, PlatformTools).unwrap());
        assert!(pending(&conn, AgentReplies).unwrap());
        record(
            &conn,
            "platform",
            "platform_call",
            "step",
            json!({"status":"needs_reconciliation"}),
        );
        assert!(pending(&conn, PlatformTools).unwrap());
        record(
            &conn,
            "platform-reply",
            "platform_tool_reply",
            "platform",
            json!({}),
        );
        assert!(!pending(&conn, PlatformTools).unwrap());
    }
    #[test]
    fn idle_hints_do_not_restore_unrelated_history_or_drop_context_replies() {
        let conn = database();
        record(
            &conn,
            "bad-history",
            "snapshot",
            "old",
            json!({"source_view":{"missing":"historical-source"}}),
        );
        record(
            &conn,
            "controller",
            "workflow_controller",
            "root",
            json!({"status":"paused"}),
        );
        assert!(!pending(&conn, Controllers).unwrap());
        record(&conn, "context", "agent_context_call", "worker", json!({}));
        assert!(pending(&conn, PlatformTools).unwrap());
        conn.execute(
            "UPDATE governance_records SET data=?1 WHERE id='context'",
            [json!({"reply_sent":true}).to_string()],
        )
        .unwrap();
        assert!(!pending(&conn, PlatformTools).unwrap());
        assert_eq!(running(&conn).unwrap(), 0);
    }

    #[test]
    fn waiting_controller_still_observes_its_live_or_failed_native_turn() {
        let conn = database();
        record(
            &conn,
            "controller",
            "workflow_controller",
            "root",
            json!({"status":"active","run_state":"waiting","session_id":"manager","epoch":1,"bootstrap_epoch":1,"source_cursor":0}),
        );
        assert!(!pending(&conn, Controllers).unwrap());
        conn.execute("INSERT INTO turns VALUES('turn','manager','running',0)", [])
            .unwrap();
        assert!(pending(&conn, Controllers).unwrap());
        conn.execute("UPDATE turns SET status='completed'", [])
            .unwrap();
        assert!(!pending(&conn, Controllers).unwrap());
        conn.execute("UPDATE turns SET status='failed'", [])
            .unwrap();
        assert!(pending(&conn, Controllers).unwrap());
        conn.execute("UPDATE turns SET status='cancelled'", [])
            .unwrap();
        assert!(pending(&conn, Controllers).unwrap());
    }

    #[test]
    fn an_unstarted_workbench_waits_without_bootstrap_polling_until_a_real_message() {
        let conn = database();
        record(
            &conn,
            "workbench",
            "workflow_controller",
            "project",
            json!({"status":"active","workspace_only":true,"run_state":"waiting","epoch":1,"bootstrap_epoch":0}),
        );
        assert!(!pending(&conn, Controllers).unwrap());
        record(
            &conn,
            "message",
            "controller_input",
            "workbench",
            json!({"consumed":false}),
        );
        assert!(pending(&conn, Controllers).unwrap());
        conn.execute(
            "UPDATE governance_records SET data=?1 WHERE id='message'",
            [json!({"consumed":true}).to_string()],
        )
        .unwrap();
        assert!(!pending(&conn, Controllers).unwrap());
    }
}
