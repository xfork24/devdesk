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

pub(super) const CONTROLLER_RECOVERABLE_BUDGET: &str = "(
 json_extract(c.data,'$.status')='budget_exhausted'
 AND json_extract(c.data,'$.reason')='编排唤醒预算已耗尽，请恢复或补充指令'
)";

// A technical suspension keeps the original manager epoch. Human takeover or
// replacement changes it, so their revoked delegations cannot match this read.
pub(super) const SEGMENT_RECOVERABLE: &str = "(
 json_extract(c.data,'$.status')='stopped'
 AND json_extract(c.data,'$.reason')='blocked: 原委托决定者已撤权'
 AND json_extract(c.data,'$.actor.kind')='controller'
 AND json_extract(c.data,'$.operations')<json_extract(c.data,'$.max_operations')
 AND EXISTS(SELECT 1 FROM governance_records a WHERE a.id=json_extract(c.data,'$.actor.controller_id')
   AND a.type='workflow_controller' AND json_extract(a.data,'$.status')='active'
   AND json_extract(a.data,'$.epoch')=json_extract(c.data,'$.actor.epoch'))
 AND EXISTS(SELECT 1 FROM governance_records w WHERE w.id=c.definition_id AND w.type='workflow_instance'
   AND json_extract(w.data,'$.active_plan_revision_id')=json_extract(c.data,'$.plan_revision_id')
   AND json_extract(w.data,'$.status') NOT IN ('completed','cancelled')
   AND coalesce(json_extract(w.data,'$.paused'),0)=0 AND coalesce(json_extract(w.data,'$.cancel_requested'),0)=0)
 AND EXISTS(SELECT 1 FROM governance_records d WHERE d.id=json_extract(c.data,'$.driver_id')
   AND d.type='workflow_driver' AND json_extract(d.data,'$.epoch')=json_extract(c.data,'$.driver_epoch')
   AND json_extract(d.data,'$.mode')=json_extract(c.data,'$.driver_mode'))
)";

pub(super) fn segments_pending_sql() -> String {
    format!(
        "EXISTS(SELECT 1 FROM governance_records c WHERE c.type='workflow_segment' AND (json_extract(c.data,'$.status')='active' OR {SEGMENT_RECOVERABLE}))"
    )
}

// The display rows are transactional hints, never dispatch authority. Include
// status repairs and cancellation as well as actual running receipts. Revisit
// older business/unknown projections of confirmed child cancellations once.
// The ancestor guard matches the partial idx_workflow_active_root predicate
// while retaining the original treatment of NULL status in legacy rows.
pub(super) const WORKFLOW_READY: &str = "(
 json_extract(c.data,'$.status') NOT IN ('completed','cancelled') AND (
 coalesce(json_extract(c.data,'$.cancel_requested'),0)=1
 OR (json_extract(c.data,'$.status') IN ('failed','needs_reconciliation')
   AND EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=c.id)
   AND NOT EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=c.id
     AND (json_extract(s.data,'$.status')='needs_reconciliation'
       OR (json_extract(s.data,'$.status')='failed' AND json_extract(c.data,'$.status')='failed'))))
 OR EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=c.id AND (
   json_extract(s.data,'$.status')='running'
   OR (json_extract(s.data,'$.status')='needs_reconciliation' AND
      json_extract(c.data,'$.status')!='needs_reconciliation')
   OR (json_extract(s.data,'$.status')='failed' AND json_extract(c.data,'$.status') NOT IN ('failed','needs_reconciliation'))
   OR (json_extract(c.data,'$.status')='queued' AND json_extract(s.data,'$.started') IS NOT NULL)))
 OR EXISTS(SELECT 1 FROM governance_records stop WHERE stop.type IN ('workflow_stop','workflow_stop_reassertion')
   AND stop.definition_id=json_extract(c.data,'$.parent_execution_id')
   AND NOT EXISTS(SELECT 1 FROM governance_records resolved WHERE resolved.type='workflow_stop_reconciliation'
     AND resolved.definition_id=stop.id AND resolved.immutable=1 AND json_extract(resolved.data,'$.issuer')='kernel_workspace_reconciliation'))
 OR EXISTS(SELECT 1 FROM json_each(c.data,'$.steps') s
   WHERE json_extract(s.value,'$.status') IN ('failed','needs_reconciliation')
   AND json_extract(s.value,'$.failure.code')='child_execution_failed'
   AND EXISTS(SELECT 1 FROM governance_records child WHERE child.type='workflow_instance'
     AND json_extract(child.data,'$.parent_execution_id')=json_extract(s.value,'$.id'))
   AND NOT EXISTS(SELECT 1 FROM governance_records child WHERE child.type='workflow_instance'
     AND json_extract(child.data,'$.parent_execution_id')=json_extract(s.value,'$.id')
     AND (json_extract(child.data,'$.status') IN ('failed','needs_reconciliation')
       OR (json_extract(child.data,'$.status')='cancelled'
         AND NOT (json_extract(s.value,'$.status')='needs_reconciliation'
           AND json_extract(s.value,'$.failure.kind')='business'
           AND json_extract(s.value,'$.failure.effect')='unknown')))))
 OR EXISTS(SELECT 1 FROM governance_records a WHERE a.type='workflow_instance'
   AND json_extract(a.data,'$.root_instance_id')=json_extract(c.data,'$.root_instance_id')
   AND coalesce(json_extract(a.data,'$.status'),'') NOT IN ('completed','cancelled')
   AND json_extract(a.data,'$.status') NOT IN ('completed','cancelled')
   AND coalesce(json_extract(a.data,'$.cancel_requested'),0)=1)
))";

pub(super) fn controllers(conn: &Connection) -> Result<bool> {
    conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM governance_records c WHERE c.type='workflow_controller' AND ((json_extract(c.data,'$.status')='active' AND {CONTROLLER_READY}) OR {CONTROLLER_RECOVERABLE_BUDGET}))"), [], |r|r.get(0)).map_err(storage)
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

pub(super) fn domain_workflow_running(conn: &Connection, project: &str) -> Result<bool> {
    // A workflow header stays running between steps and while a human is
    // deciding. A running subflow wrapper represents its child, not a worker.
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM governance_records i
            WHERE i.type='workflow_instance' AND json_extract(i.data,'$.project_id')=?1
              AND json_extract(i.data,'$.status') NOT IN ('completed','cancelled','skipped')
              AND (
                (json_extract(i.data,'$.status')='running'
                    AND coalesce(json_extract(i.data,'$.paused'),0)=0
                    AND coalesce(json_array_length(i.data,'$.steps'),0)=0)
                OR EXISTS(SELECT 1 FROM json_each(i.data,'$.steps') s
                    WHERE json_extract(s.value,'$.status')='running'
                      AND NOT EXISTS(SELECT 1 FROM governance_records child
                          WHERE child.type='workflow_instance'
                            AND json_extract(child.data,'$.project_id')=?1
                            AND json_extract(child.data,'$.parent_execution_id')=json_extract(s.value,'$.id')))
              )
        ) OR EXISTS(
            SELECT 1 FROM governance_records c
            JOIN governance_records i ON i.id=json_extract(c.data,'$.root_instance_id')
            JOIN turns t ON t.session_id=json_extract(c.data,'$.session_id')
            WHERE c.type='workflow_controller' AND json_extract(c.data,'$.project_id')=?1
              AND i.type='workflow_instance' AND json_extract(i.data,'$.project_id')=?1
              AND json_extract(i.data,'$.status') NOT IN ('completed','cancelled','skipped')
              AND t.status IN ('dispatching','running')
        )",
        [project],
        |row| row.get(0),
    )
    .map_err(storage)
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
        ports::WorkflowRuntimeWork::Segments => return conn.query_row(&format!("SELECT {}", segments_pending_sql()), [], |r|r.get(0)).map_err(storage),
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
    fn only_a_legacy_wakeup_count_suspension_is_automatically_recoverable() {
        let conn = database();
        let mut data = json!({"status":"budget_exhausted","run_state":"waiting","epoch":1,"bootstrap_epoch":1,"reason":"编排唤醒预算已耗尽，请恢复或补充指令"});
        record(
            &conn,
            "controller",
            "workflow_controller",
            "root",
            data.clone(),
        );
        assert!(pending(&conn, Controllers).unwrap());
        for (status, reason) in [
            ("budget_exhausted", "经理轮次超时，已请求停止"),
            ("paused", "编排唤醒预算已耗尽，请恢复或补充指令"),
            ("failed", "原经理轮次失败"),
            ("needs_reconciliation", "原经理轮次结果未知"),
            ("closed", ""),
        ] {
            data["status"] = json!(status);
            data["reason"] = json!(reason);
            conn.execute(
                "UPDATE governance_records SET data=?1 WHERE id='controller'",
                [data.to_string()],
            )
            .unwrap();
            assert!(!pending(&conn, Controllers).unwrap(), "{status}: {reason}");
        }
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

#[cfg(test)]
mod domain_uninstall_tests {
    use super::*;

    #[test]
    fn uninstall_activity_tracks_running_steps_and_controller_turns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE governance_records (id TEXT, type TEXT, data TEXT);
             CREATE TABLE turns (session_id TEXT, status TEXT);",
        )
        .unwrap();
        let workflow = |status: &str, steps: Value| json!({"project_id":"project","status":status,"steps":steps});
        let set_workflows = |rows: &[Value]| {
            conn.execute("DELETE FROM governance_records", []).unwrap();
            for row in rows {
                conn.execute(
                    "INSERT INTO governance_records VALUES (json_extract(?1,'$.id'), 'workflow_instance', ?1)",
                    [row.to_string()],
                )
                .unwrap();
            }
        };
        for status in [
            "queued",
            "blocked",
            "waiting_input",
            "waiting_confirmation",
            "failed",
            "needs_reconciliation",
            "completed",
            "cancelled",
        ] {
            set_workflows(&[workflow(status, json!([]))]);
            assert!(
                !domain_workflow_running(&conn, "project").unwrap(),
                "{status}"
            );
        }
        let wrapper = workflow("running", json!([{"id":"parent","status":"running"}]));
        set_workflows(std::slice::from_ref(&wrapper));
        assert!(domain_workflow_running(&conn, "project").unwrap());
        let mut child = workflow(
            "running",
            json!([{"id":"worker","status":"waiting_confirmation"}]),
        );
        child["parent_execution_id"] = json!("parent");
        set_workflows(&[wrapper.clone(), child.clone()]);
        assert!(!domain_workflow_running(&conn, "project").unwrap());

        child["steps"][0]["status"] = json!("running");
        child["paused"] = json!(true);
        set_workflows(&[wrapper, child]);
        assert!(domain_workflow_running(&conn, "project").unwrap());
        assert!(!domain_workflow_running(&conn, "another-project").unwrap());

        let mut root = workflow("running", json!([{"id":"step","status":"blocked"}]));
        root["id"] = json!("root");
        root["paused"] = json!(true);
        set_workflows(&[root]);
        assert!(!domain_workflow_running(&conn, "project").unwrap());
        conn.execute(
            "INSERT INTO governance_records VALUES ('controller', 'workflow_controller', ?1)",
            [
                json!({"project_id":"project","root_instance_id":"root","session_id":"manager"})
                    .to_string(),
            ],
        )
        .unwrap();
        conn.execute("INSERT INTO turns VALUES ('manager','running')", [])
            .unwrap();
        assert!(domain_workflow_running(&conn, "project").unwrap());
        conn.execute("UPDATE governance_records SET data=json_set(data,'$.status','completed') WHERE id='root'", [])
            .unwrap();
        assert!(!domain_workflow_running(&conn, "project").unwrap());
        conn.execute(
            "UPDATE governance_records SET data=json_set(data,'$.status','queued') WHERE id='root'",
            [],
        )
        .unwrap();
        conn.execute("UPDATE turns SET status='waiting_interaction'", [])
            .unwrap();
        assert!(!domain_workflow_running(&conn, "project").unwrap());
    }
}
