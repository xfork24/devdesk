//! Workflow scheduling reads through the shared pool, without restoring history.
use super::*;

pub(super) fn running(conn: &Connection) -> Result<usize> {
    let _span = performance::Span::new("runtime.running_count");
    conn.query_row("SELECT count(*) FROM turns t
        WHERE t.status IN ('dispatching','running','waiting_interaction','needs_reconciliation')
        AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0)",
        [], |row| row.get(0)).map_err(storage)
}

pub(super) fn pending(conn: &Connection, kind: ports::WorkflowRuntimeWork) -> Result<bool> {
    let _span = performance::Span::new("runtime.pending_read");
    let query = match kind {
        ports::WorkflowRuntimeWork::Conversations => "SELECT EXISTS(SELECT 1 FROM turns t
            WHERE t.status IN ('queued','dispatching','running','waiting_interaction')
            OR (t.status='needs_reconciliation' AND (t.cancel_requested=1
              OR EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0))))",
        ports::WorkflowRuntimeWork::WorkflowIntents => "SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared')",
        ports::WorkflowRuntimeWork::Controllers => "SELECT EXISTS(SELECT 1 FROM governance_records
            WHERE type='workflow_controller' AND json_extract(data,'$.status')='active')",
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
            CREATE TABLE governance_records(id TEXT,type TEXT,definition_id TEXT,data JSON);",
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
}
