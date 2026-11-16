//! Indexed display reads; no EngineeringState construction or source expansion.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::read_models::*;
use rusqlite::{OptionalExtension, params};

fn cursor(conn: &Connection) -> Result<u64> {
    conn.query_row("SELECT coalesce(max(id),0) FROM events", [], |row| {
        row.get(0)
    })
    .map_err(storage)
}
fn scope(project: &str) -> String {
    format!("project:{project}")
}
fn trim_page<T>(rows: &mut Vec<T>, limit: usize) -> bool {
    let more = rows.len() > limit;
    rows.truncate(limit);
    more
}

fn hide_encoded_reason(reason: &mut String, truncated: &mut bool) {
    if source_views::is_encoded_text(reason) {
        reason.clear();
        *truncated = true;
    }
}

pub(super) fn orchestration(
    pool: &ConnectionPool,
    project: &str,
    request: &ControllerQuery,
) -> Result<OrchestrationSummary> {
    let _span = performance::Span::new("read_model.orchestration");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let (selector, id) = match &request.selector {
        ControllerSelector::Id(id) => ("id", id),
        ControllerSelector::Root(id) => ("json_extract(data,'$.root_instance_id')", id),
        ControllerSelector::Owner(id) => ("json_extract(data,'$.owner_id')", id),
    };
    let sql = format!(
        "SELECT id,substr(name,1,128),revision,json_extract(data,'$.project_id'),json_extract(data,'$.owner_id'),json_extract(data,'$.root_instance_id'),json_extract(data,'$.role_ref'),json_extract(data,'$.agent_revision_id'),json_extract(data,'$.harness_permission_ref'),json_extract(data,'$.session_id'),json_extract(data,'$.epoch'),json_extract(data,'$.status'),substr(coalesce(json_extract(data,'$.reason'),''),1,256),length(coalesce(json_extract(data,'$.reason'),''))>256 FROM governance_records WHERE type='workflow_controller' AND {selector}=?1 AND json_extract(scope,'$')=?2 AND (?3 IS NULL OR coalesce(json_extract(data,'$.workspace_only'),0)=?3) AND json_extract(data,'$.project_id')=?4 ORDER BY rowid DESC LIMIT 1"
    );
    let record = conn
        .query_row(
            &sql,
            params![id, scope(project), request.workspace_only, project],
            |row| {
                let reason: String = row.get(12)?;
                let opaque = source_views::is_encoded_text(&reason);
                Ok(RuntimeSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    revision: row.get(2)?,
                    data: ControllerState {
                        project_id: row.get(3)?,
                        owner_id: row.get(4)?,
                        root_instance_id: row.get(5)?,
                        role_ref: row.get(6)?,
                        agent_revision_id: row.get(7)?,
                        harness_permission_ref: row.get(8)?,
                        session_id: row.get(9)?,
                        epoch: row.get(10)?,
                        status: row.get(11)?,
                        reason: if opaque { String::new() } else { reason },
                        reason_truncated: opaque || row.get(13)?,
                    },
                })
            },
        )
        .optional()
        .map_err(storage)?;
    let mut result = OrchestrationSummary {
        project_id: project.into(),
        cursor: cursor(&conn)?,
        record,
        next_decision_before: None,
        messages: vec![],
        calls: vec![],
        decisions: vec![],
        more: MoreControllerItems {
            messages: false,
            calls: false,
            decisions: false,
        },
    };
    if let Some(record) = &result.record {
        if let Some(session) = &record.data.session_id {
            let mut query=conn.prepare_cached("SELECT id,session_id,turn_id,sequence,substr(content,1,512),length(content)>512 FROM conversation_events WHERE session_id=?1 AND kind='assistant' AND EXISTS(SELECT 1 FROM sessions WHERE sessions.id=conversation_events.session_id AND sessions.project_id=?2 AND sessions.scope!='global') ORDER BY rowid DESC LIMIT 11").map_err(storage)?;
            result.messages = query
                .query_map(params![session, project], |row| {
                    Ok(MessagePreview {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        turn_id: row.get(2)?,
                        sequence: row.get(3)?,
                        content_excerpt: row.get(4)?,
                        truncated: row.get(5)?,
                    })
                })
                .map_err(storage)?
                .collect::<rusqlite::Result<_>>()
                .map_err(storage)?;
            result.more.messages = trim_page(&mut result.messages, 10);
        }
        let mut query=conn.prepare_cached("SELECT id,substr(json_extract(data,'$.method'),1,128),json_extract(data,'$.status'),substr(json_extract(data,'$.receipt.status'),1,128),substr(json_extract(data,'$.error.message'),1,256) FROM governance_records WHERE type='controller_tool_call' AND definition_id=?1 AND json_extract(scope,'$')=?2 ORDER BY rowid DESC LIMIT 6").map_err(storage)?;
        result.calls = query
            .query_map(params![record.id, scope(project)], |row| {
                Ok(CallSummary {
                    id: row.get(0)?,
                    method: row.get(1)?,
                    status: row.get(2)?,
                    receipt_status: row.get(3)?,
                    error: row.get(4)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        result.more.calls = trim_page(&mut result.calls, 5);
        if let Some(root) = &record.data.root_instance_id {
            let mut query=conn.prepare_cached("SELECT d.id,substr(d.name,1,128),d.revision,json_extract(d.data,'$.status'),coalesce(json_extract(d.data,'$.plan_revision_id'),''),coalesce(substr(json_extract(d.data,'$.request.operation'),1,128),''),d.rowid FROM governance_records d WHERE d.type='workflow_pending_decision' AND (?3 IS NULL OR d.rowid<?3) AND json_extract(d.data,'$.status')='pending' AND json_extract(d.scope,'$')=?1 AND d.definition_id IN (SELECT i.id FROM governance_records i WHERE i.type='workflow_instance' AND json_extract(i.scope,'$')=?1 AND json_extract(i.data,'$.root_instance_id')=?2) ORDER BY d.rowid DESC LIMIT 11").map_err(storage)?;
            let mut rows = query
                .query_map(
                    params![scope(project), root, request.decision_before],
                    |row| {
                        Ok((
                            row.get::<_, u64>(6)?,
                            RuntimeSummary {
                                id: row.get(0)?,
                                name: row.get(1)?,
                                revision: row.get(2)?,
                                data: DecisionState {
                                    status: row.get(3)?,
                                    plan_revision_id: row.get(4)?,
                                    operation: row.get(5)?,
                                },
                            },
                        ))
                    },
                )
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?;
            result.more.decisions = trim_page(&mut rows, 10);
            if result.more.decisions {
                result.next_decision_before = rows.last().map(|row| row.0);
            }
            result.decisions = rows.into_iter().map(|row| row.1).collect();
        }
    }
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}

pub(super) fn driver(
    pool: &ConnectionPool,
    project: &str,
    id: &str,
) -> Result<WorkflowDriverSummary> {
    let _span = performance::Span::new("read_model.driver");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let (revision,root):(u64,String)=conn.query_row("SELECT revision,coalesce(json_extract(data,'$.root_instance_id'),id) FROM governance_records WHERE id=?1 AND type='workflow_instance' AND json_extract(scope,'$')=?2",params![id,scope(project)],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(storage)?.ok_or_else(||err("not_found","工作流不属于本次请求的项目"))?;
    let mut driver=conn.query_row("SELECT id,substr(name,1,128),revision,json_extract(data,'$.mode'),json_extract(data,'$.epoch'),substr(coalesce(json_extract(data,'$.reason.message'),json_extract(data,'$.reason'),''),1,256),length(coalesce(json_extract(data,'$.reason.message'),json_extract(data,'$.reason'),''))>256 FROM governance_records WHERE type='workflow_driver' AND definition_id=?1 AND json_extract(scope,'$')=?2 ORDER BY rowid DESC LIMIT 1",params![root,scope(project)],|row|Ok(RuntimeSummary {id:row.get(0)?,name:row.get(1)?,revision:row.get(2)?,data:DriverState {mode:row.get(3)?,epoch:row.get(4)?,reason:row.get(5)?,reason_truncated:row.get(6)?}})).optional().map_err(storage)?;
    let mut waiting = {
        let mut query=conn.prepare_cached("SELECT i.id,json_extract(i.data,'$.owner_id'),json_extract(s.value,'$.id'),substr(json_extract(s.value,'$.step'),1,128),substr(coalesce(json_extract(s.value,'$.path'),''),1,256),json_extract(s.value,'$.status'),substr(coalesce(json_extract(s.value,'$.reason'),''),1,256),length(coalesce(json_extract(s.value,'$.reason'),''))>256 FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.type='workflow_instance' AND coalesce(json_extract(i.data,'$.status'),'') NOT IN ('completed','cancelled') AND json_extract(i.scope,'$')=?1 AND json_extract(i.data,'$.root_instance_id')=?2 AND json_extract(s.value,'$.status') IN ('waiting_input','waiting_confirmation','awaiting_acceptance','failed','needs_reconciliation') ORDER BY i.rowid,CAST(s.key AS INTEGER) LIMIT 21").map_err(storage)?;
        query
            .query_map(params![scope(project), root], |row| {
                Ok(WaitingStep {
                    instance_id: row.get(0)?,
                    owner_id: row.get(1)?,
                    execution_id: row.get(2)?,
                    step: row.get(3)?,
                    path: row.get(4)?,
                    status: row.get(5)?,
                    reason: row.get(6)?,
                    reason_truncated: row.get(7)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    if let Some(driver) = &mut driver {
        hide_encoded_reason(&mut driver.data.reason, &mut driver.data.reason_truncated);
    }
    for row in &mut waiting {
        hide_encoded_reason(&mut row.reason, &mut row.reason_truncated);
    }
    let waiting_more = trim_page(&mut waiting, 20);
    let cursor = cursor(&conn)?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(WorkflowDriverSummary {
        project_id: project.into(),
        cursor,
        root_instance_id: root,
        instance_revision: revision,
        driver,
        waiting,
        waiting_more,
        observed_only: true,
    })
}

pub(super) fn decision(pool: &ConnectionPool, project: &str, id: &str) -> Result<DecisionDetail> {
    let _span = performance::Span::new("read_model.decision_detail");
    let conn = pool.read()?;
    conn.query_row(
        "SELECT id,name,revision,json_extract(data,'$.status'),coalesce(json_extract(data,'$.plan_revision_id'),''),json_extract(data,'$.request') FROM governance_records WHERE type='workflow_pending_decision' AND id=?1 AND json_extract(scope,'$')=?2",
        params![id,scope(project)],
        |row| Ok(RuntimeSummary {
            id: row.get(0)?, name: row.get(1)?, revision: row.get(2)?,
            data: DecisionRequest {
                status: row.get(3)?, plan_revision_id: row.get(4)?, request_json: row.get(5)?,
            },
        }),
    ).optional().map_err(storage)?.ok_or_else(||err("not_found","待决定请求不属于本次项目"))
}
