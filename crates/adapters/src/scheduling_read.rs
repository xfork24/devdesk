//! Finite scheduling hints. Dispatch still checks authoritative domain facts.
use crate::*;
use devdesk_core::read_models::SchedulingHints;
const RUNNING: &str = "SELECT count(*) FROM turns t WHERE t.status NOT IN ('queued','completed','failed','cancelled') AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0)";
pub(super) fn running(conn: &Connection) -> Result<usize> {
    conn.query_row(RUNNING, [], |row| row.get(0))
        .map_err(storage)
}
pub(super) fn pending(conn: &Connection) -> Result<bool> {
    stages(conn, &runtime_queries::utc_clock()).map(|stages| !stages.is_empty())
}
fn stages(
    conn: &Connection,
    clock: &ExecutionClockAnchor,
) -> Result<Vec<devdesk_core::SchedulingStage>> {
    use devdesk_core::SchedulingStage as S;
    let segments = runtime_queries::segments_pending_sql();
    let repositories = format!(
        "EXISTS(SELECT 1 FROM repository_operations WHERE {}) OR EXISTS(SELECT 1 FROM tasks
            WHERE status!='completed' AND worktree=''
              AND EXISTS(SELECT 1 FROM heads WHERE id=tasks.feature_id AND revision!='')
              AND NOT EXISTS(SELECT 1 FROM repository_operations
                WHERE json_extract(action,'$.kind')='create_worktree' AND json_extract(action,'$.owner')=tasks.id))",
        repository_contexts::RUNNABLE_OPERATION,
    );
    let work = &[
        (
            S::Conversations,
            "EXISTS(SELECT 1 FROM turns WHERE status NOT IN ('completed','failed','cancelled')) OR EXISTS(SELECT 1 FROM governance_records WHERE (type IN ('provider_poll_pending','provider_tool_intent','conversation_capture_intent','captured_experience_intent','sealed_command_intent') AND json_extract(data,'$.status') IN ('prepared','capturing','captured')) OR (type='worker_cleanup_work' AND json_extract(data,'$.status')='pending') OR (type='worker_cleanup_intent' AND json_extract(data,'$.status')='prepared'))",
        ),
        (
            S::Workflows,
            "EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared') OR EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_reconciliation' AND json_extract(data,'$.status') IN ('prepared','inspecting'))",
        ),
        (
            S::Controllers,
            "EXISTS(SELECT 1 FROM governance_records WHERE type='controller_reply_work' AND json_extract(data,'$.status')='pending')",
        ),
        (
            S::Tools,
            "EXISTS(SELECT 1 FROM governance_records WHERE type='tool_call' AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR (coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL)))",
        ),
        (
            S::Platform,
            "EXISTS(SELECT 1 FROM governance_records WHERE (type IN ('platform_call','platform_test_cleanup','agent_context_call') AND json_extract(data,'$.status') IN ('prepared','running')) OR (type='platform_reply_work' AND json_extract(data,'$.status')='pending'))",
        ),
        (S::Repositories, repositories.as_str()),
        (
            S::PlanChanges,
            "EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_change' AND json_extract(data,'$.status') IN ('prepared','waiting_quiescence'))",
        ),
        (S::Segments, segments.as_str()),
        (
            S::Drivers,
            "EXISTS(SELECT 1 FROM governance_records d JOIN governance_records w ON w.id=d.definition_id WHERE d.type='workflow_driver' AND json_extract(d.data,'$.mode')='rust' AND json_extract(w.data,'$.status') NOT IN ('completed','cancelled') AND coalesce(json_extract(w.data,'$.paused'),0)=0)",
        ),
    ];
    let sql = format!(
        "SELECT {}",
        work.iter()
            .map(|(_, sql)| format!("({sql})"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut ready = conn
        .query_row(&sql, [], |row| {
            work.iter()
                .enumerate()
                .filter_map(|(n, (stage, _))| match row.get::<_, bool>(n) {
                    Ok(true) => Some(Ok(*stage)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e)),
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(storage)?;
    if !ready.contains(&S::Workflows) && runtime_queries::workflow_observations(conn, clock)? {
        ready.push(S::Workflows);
    }
    if !ready.contains(&S::Controllers)
        && (runtime_queries::controllers(conn)?
            || runtime_queries::pending(conn, ports::WorkflowRuntimeWork::ControllerCalls)?)
    {
        ready.push(S::Controllers);
    }
    Ok(ready)
}
pub(super) fn hints(conn: &Connection) -> Result<SchedulingHints> {
    hints_at(conn, &runtime_queries::utc_clock())
}
pub(super) fn hints_at(conn: &Connection, clock: &ExecutionClockAnchor) -> Result<SchedulingHints> {
    let _span = performance::Span::new("read_model.scheduling_hints");
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let live = tx
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM projects WHERE execution_mode!='live')",
            [],
            |row| row.get(0),
        )
        .map_err(storage)?;
    let running_conversations = running(&tx)?;
    let stages = stages(&tx, clock)?;
    tx.commit().map_err(storage)?;
    let pending = !stages.is_empty();
    Ok(SchedulingHints {
        stages,
        pending,
        live,
        running_conversations,
    })
}
