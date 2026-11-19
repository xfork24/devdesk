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
        "SELECT id,substr(name,1,128),revision,json_extract(data,'$.project_id'),json_extract(data,'$.owner_id'),json_extract(data,'$.root_instance_id'),json_extract(data,'$.role_ref'),json_extract(data,'$.agent_revision_id'),json_extract(data,'$.harness_permission_ref'),json_extract(data,'$.session_id'),json_extract(data,'$.epoch'),json_extract(data,'$.status'),substr(coalesce(json_extract(data,'$.reason'),''),1,256),length(coalesce(json_extract(data,'$.reason'),''))>256 FROM governance_records WHERE type='workflow_controller' AND {selector}=?1 AND owner_project=?2 AND (?3 IS NULL OR coalesce(json_extract(data,'$.workspace_only'),0)=?3) AND json_extract(data,'$.project_id')=?4 ORDER BY rowid DESC LIMIT 1"
    );
    let record = conn
        .query_row(
            &sql,
            params![id, project, request.workspace_only, project],
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
        let mut query=conn.prepare_cached("SELECT id,substr(json_extract(data,'$.method'),1,128),json_extract(data,'$.status'),substr(json_extract(data,'$.receipt.status'),1,128),substr(json_extract(data,'$.error.message'),1,256) FROM governance_records WHERE type='controller_tool_call' AND definition_id=?1 AND owner_project=?2 ORDER BY rowid DESC LIMIT 6").map_err(storage)?;
        result.calls = query
            .query_map(params![record.id, project], |row| {
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
    }
    let root = result
        .record
        .as_ref()
        .and_then(|r| r.data.root_instance_id.as_deref())
        .or(match &request.selector {
            ControllerSelector::Root(id) => Some(id.as_str()),
            _ => None,
        });
    if let Some(root) = root {
        let mut query=conn.prepare_cached("SELECT d.id,substr(d.name,1,128),d.revision,json_extract(d.data,'$.status'),coalesce(json_extract(d.data,'$.plan_revision_id'),''),coalesce(substr(json_extract(d.data,'$.request.operation'),1,128),''),d.rowid FROM governance_records d WHERE d.type='workflow_pending_decision' AND (?3 IS NULL OR d.rowid<?3) AND json_extract(d.data,'$.status')='pending' AND d.owner_project=?1 AND d.definition_id IN (SELECT i.id FROM governance_records i WHERE i.type='workflow_instance' AND i.owner_project=?1 AND json_extract(i.data,'$.root_instance_id')=?2) ORDER BY d.rowid DESC LIMIT 11").map_err(storage)?;
        let mut rows = query
            .query_map(params![project, root, request.decision_before], |row| {
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
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        result.more.decisions = trim_page(&mut rows, 10);
        if result.more.decisions {
            result.next_decision_before = rows.last().map(|row| row.0);
        }
        result.decisions = rows.into_iter().map(|row| row.1).collect();
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
    let (revision,root):(u64,String)=conn.query_row("SELECT revision,coalesce(json_extract(data,'$.root_instance_id'),id) FROM governance_records WHERE id=?1 AND type='workflow_instance' AND owner_project=?2",params![id,project],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(storage)?.ok_or_else(||err("not_found","工作流不属于本次请求的项目"))?;
    let mut driver=conn.query_row("SELECT id,substr(name,1,128),revision,json_extract(data,'$.mode'),json_extract(data,'$.epoch'),substr(coalesce(json_extract(data,'$.reason.message'),json_extract(data,'$.reason'),''),1,256),length(coalesce(json_extract(data,'$.reason.message'),json_extract(data,'$.reason'),''))>256 FROM governance_records WHERE type='workflow_driver' AND definition_id=?1 AND owner_project=?2 ORDER BY rowid DESC LIMIT 1",params![root,project],|row|Ok(RuntimeSummary {id:row.get(0)?,name:row.get(1)?,revision:row.get(2)?,data:DriverState {mode:row.get(3)?,epoch:row.get(4)?,reason:row.get(5)?,reason_truncated:row.get(6)?}})).optional().map_err(storage)?;
    let mut waiting = {
        let mut query=conn.prepare_cached("SELECT h.instance_id,json_extract(h.record,'$.data.owner_id'),s.execution_id,substr(json_extract(s.data,'$.step'),1,128),substr(coalesce(json_extract(s.data,'$.path'),''),1,256),json_extract(s.data,'$.status'),substr(coalesce(json_extract(s.data,'$.reason'),''),1,256),(coalesce(json_extract(s.data,'$.reason_truncated'),0) OR length(coalesce(json_extract(s.data,'$.reason'),''))>256),coalesce((SELECT feature_id FROM tasks WHERE id=json_extract(h.record,'$.data.owner_id')),(SELECT id FROM features WHERE id=json_extract(h.record,'$.data.owner_id'))),(SELECT id FROM tasks WHERE id=json_extract(h.record,'$.data.owner_id')),json_extract(h.record,'$.revision'),coalesce(json_extract(h.record,'$.data.active_plan_revision_id'),''),coalesce(json_extract(h.record,'$.data.control_version'),0),s.step_revision FROM workflow_display_headers h JOIN workflow_display_steps s ON s.instance_id=h.instance_id WHERE json_extract(h.record,'$.data.project_id')=?1 AND json_extract(h.record,'$.data.root_instance_id')=?2 AND coalesce(json_extract(h.record,'$.data.status'),'') NOT IN ('completed','cancelled') AND json_extract(s.data,'$.status') IN ('waiting_input','waiting_confirmation','awaiting_acceptance','failed','needs_reconciliation') ORDER BY h.instance_id,json_extract(s.data,'$.path'),s.execution_id LIMIT 21").map_err(storage)?;
        query
            .query_map(params![project, root], |row| {
                Ok(WaitingStep {
                    revision: row.get(10)?,
                    plan_revision_id: row.get(11)?,
                    control_version: row.get(12)?,
                    step_revision: row.get(13)?,
                    feature_id: row.get(8)?,
                    task_id: row.get(9)?,
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
    let workflow_approval_pending: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM workflow_display_steps s JOIN workflow_display_headers h ON h.instance_id=s.instance_id WHERE json_extract(h.record,'$.data.root_instance_id')=?1 AND (json_extract(s.data,'$.status')='waiting_confirmation' OR (json_extract(s.data,'$.status')='awaiting_acceptance' AND EXISTS(SELECT 1 FROM governance_records r WHERE r.type='workflow_rule_hint' AND r.definition_id=s.execution_id AND json_extract(r.data,'$.satisfied')=0))))",[&root],|r|r.get(0)).map_err(storage)?;
    let runtime_approval_pending: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM interactions a WHERE a.kind='tool_approval' AND a.status='pending' AND a.turn_id IN (SELECT json_extract(s.data,'$.turn_id') FROM workflow_display_steps s JOIN workflow_display_headers h ON h.instance_id=s.instance_id WHERE json_extract(h.record,'$.data.root_instance_id')=?1))",[&root],|r|r.get(0)).map_err(storage)?;
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
        runtime_approval_pending,
        workflow_approval_pending,
        observed_only: true,
    })
}

pub(super) fn decision(pool: &ConnectionPool, project: &str, id: &str) -> Result<DecisionDetail> {
    let _span = performance::Span::new("read_model.decision_detail");
    let conn = pool.read()?;
    conn.query_row(
        "SELECT id,name,revision,json_extract(data,'$.status'),coalesce(json_extract(data,'$.plan_revision_id'),''),json_extract(data,'$.request') FROM governance_records WHERE type='workflow_pending_decision' AND id=?1 AND owner_project=?2",
        params![id,project],
        |row| Ok(RuntimeSummary {
            id: row.get(0)?, name: row.get(1)?, revision: row.get(2)?,
            data: DecisionRequest {
                status: row.get(3)?, plan_revision_id: row.get(4)?, request_json: row.get(5)?,
            },
        }),
    ).optional().map_err(storage)?.ok_or_else(||err("not_found","待决定请求不属于本次项目"))
}

pub(super) fn decision_context(
    pool: &ConnectionPool,
    project: &str,
    instance: &str,
    execution: Option<&str>,
) -> Result<DecisionContext> {
    let _span = performance::Span::new("read_model.decision_context");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let header = |id: &str| -> Result<(String, Option<String>, String, u64)> {
        conn.query_row(
            "SELECT json_extract(data,'$.active_plan_revision_id'),json_extract(data,'$.parent_execution_id'),coalesce(json_extract(data,'$.root_instance_id'),id),coalesce(json_extract(data,'$.control_version'),0) FROM governance_records WHERE id=?1 AND type='workflow_instance' AND owner_project=?2 AND coalesce(json_extract(data,'$.project_id'),?2)=?2",
            params![id, project],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| err("not_found", "审批流程不属于当前项目"))
    };
    let (active_plan_revision_id, mut parent, root, control_version) = header(instance)?;
    let execution = execution
        .map(|id| {
            conn.query_row(
                "SELECT json_object('revision',step_revision,'status',json_extract(data,'$.status'),'input_digest',json_extract(data,'$.input_digest'),'outputs',json(coalesce(data->'$.outputs','{}'))) FROM workflow_display_steps WHERE instance_id=?1 AND execution_id=?2",
                params![instance, id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage)?
            .map(|value| serde_json::from_str(&value).map_err(storage))
            .transpose()
        })
        .transpose()?
        .flatten();
    let mut ancestor_instance_ids = Vec::new();
    let mut finished = false;
    for _ in 0..WORKFLOW_DEPTH_LIMIT {
        let Some(execution) = parent else {
            finished = true;
            break;
        };
        let mut query = conn.prepare_cached("SELECT json_extract(v.resource_id,'$[0]') FROM resource_versions v JOIN governance_records i ON i.id=json_extract(v.resource_id,'$[0]') AND i.type='workflow_instance' AND i.owner_project=?2 WHERE v.kind='workflow_step_authority' AND json_extract(v.resource_id,'$[1]')=?1 LIMIT 2").map_err(storage)?;
        let ids = query
            .query_map(params![execution, project], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        require(ids.len() == 1, "审批祖先执行缺失或存在歧义")?;
        let id = &ids[0];
        require(
            id != instance && !ancestor_instance_ids.contains(id),
            "审批流程祖先循环",
        )?;
        let (_, next, actual_root, _) = header(id)?;
        require(actual_root == root, "审批流程祖先不属于同一根")?;
        ancestor_instance_ids.push(id.clone());
        parent = next;
    }
    require(finished, "审批流程祖先超过预算")?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(DecisionContext {
        active_plan_revision_id,
        control_version,
        execution,
        ancestor_instance_ids,
    })
}

fn delivery_summary(
    conn: &Connection,
    project: &str,
    instance: &str,
) -> Result<Option<WorkflowDeliverySummary>> {
    let value:Option<String>=conn.query_row("SELECT json_object('pull_request_url',substr(json_extract(data,'$.publication.pull_request.url'),1,1024),'pull_request_number',json_extract(data,'$.publication.pull_request.number'),'source_revision',substr(json_extract(data,'$.publication.source_revision'),1,64),'target_revision',substr(json_extract(data,'$.publication.target_revision'),1,64),'candidate_revision',substr(json_extract(data,'$.publication.revision'),1,64),'merged_revision',substr(json_extract(data,'$.completion.revision'),1,64),'merged',json(CASE WHEN json_extract(data,'$.completion.pull_request.merged')=1 THEN 'true' ELSE 'false' END),'synchronized',json(CASE WHEN json_extract(data,'$.synchronized')=1 THEN 'true' ELSE 'false' END)) FROM governance_records WHERE id=?1 AND type='workflow_delivery' AND owner_project=?2",params![format!("workflow-delivery:{instance}"),project],|row|row.get(0)).optional().map_err(storage)?;
    let mut summary: Option<WorkflowDeliverySummary> = value
        .map(|v| serde_json::from_str(&v).map_err(storage))
        .transpose()?;
    if let Some(current) = &mut summary
        && !current.merged
    {
        let remote:Option<String>=conn.query_row("SELECT substr(json_extract(o.data,'$.result.Ok.merge_commit'),1,64) FROM governance_records w,json_each(w.data,'$.steps') s JOIN governance_records call ON call.id=json_extract(s.value,'$.tool_call_id') AND call.type='platform_call' JOIN governance_records stage ON stage.id='platform-publish-stage:'||json_extract(call.data,'$.effect_id')||':merge' AND stage.type='platform_publish_stage' JOIN governance_records o ON o.type='platform_effect_observation' AND o.definition_id=stage.id WHERE w.id=?1 AND w.owner_project=?2 AND json_extract(call.data,'$.handler')='git.delivery.merge' AND json_extract(o.data,'$.phase') IN ('result','recovery_merge') AND json_extract(o.data,'$.result.Ok.merged')=1 AND json_extract(o.data,'$.result.Ok.head')=?3 ORDER BY o.rowid DESC LIMIT 1",params![instance,project,current.candidate_revision],|r|r.get(0)).optional().map_err(storage)?.flatten();
        if let Some(revision) = remote {
            current.merged = true;
            current.merged_revision = Some(revision);
        }
    }
    Ok(summary)
}

pub(super) fn management(
    pool: &ConnectionPool,
    project: &str,
    id: &str,
    child_offset: u64,
) -> Result<WorkflowManagementSummary> {
    require(child_offset <= 1_000_000, "子流程页偏移过大")?;
    let _span = performance::Span::new("read_model.workflow_management");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let record=conn.query_row(
        "SELECT id,substr(name,1,128),revision,json_extract(data,'$.owner_id'),coalesce(json_extract(data,'$.root_instance_id'),id),json_extract(data,'$.parent_execution_id'),json_extract(data,'$.workflow_ref'),json_extract(data,'$.active_plan_revision_id'),coalesce(json_extract(data,'$.control_version'),0),json_extract(data,'$.status'),coalesce(json_extract(data,'$.paused'),0),coalesce(json_extract(data,'$.cancel_requested'),0),json_extract(data,'$.plan.standard_ref'),json_extract(data,'$.plan.harness_permission_ref'),CASE WHEN length(coalesce(json_extract(data,'$.role_agent_bindings'),'{}'))<=8192 THEN coalesce(json_extract(data,'$.role_agent_bindings'),'{}') ELSE '{}' END,length(coalesce(json_extract(data,'$.role_agent_bindings'),'{}'))>8192 FROM governance_records WHERE id=?1 AND type='workflow_instance' AND owner_project=?2 AND coalesce(json_extract(data,'$.project_id'),?3)=?3",
        params![id,project,project],|row| {
            let bindings:String=row.get(14)?;
            Ok(RuntimeSummary {id:row.get(0)?,name:row.get(1)?,revision:row.get(2)?,data:InstanceManagementState {
                project_id:project.into(),owner_id:row.get(3)?,root_instance_id:row.get(4)?,parent_execution_id:row.get(5)?,workflow_ref:row.get(6)?,active_plan_revision_id:row.get(7)?,control_version:row.get(8)?,status:row.get(9)?,paused:row.get(10)?,cancel_requested:row.get(11)?,plan:PlanSelectionSummary {standard_ref:row.get(12)?,harness_permission_ref:row.get(13)?},
                role_agent_bindings:serde_json::from_str(&bindings).map_err(|error|rusqlite::Error::FromSqlConversionFailure(14,rusqlite::types::Type::Text,Box::new(error)))?,configuration_truncated:row.get(15)?,
            }})
        }).optional().map_err(storage)?.ok_or_else(||err("not_found","工作流不属于本次请求的项目"))?;
    let mut children = {
        let mut query=conn.prepare_cached("SELECT c.id,substr(c.name,1,128),c.revision,json_extract(c.data,'$.owner_id'),coalesce(json_extract(c.data,'$.status'),'queued'),(SELECT count(*) FROM json_each(coalesce(t.dependencies,'[]')) d LEFT JOIN tasks upstream ON upstream.id=d.value WHERE upstream.id IS NULL OR upstream.status!='completed') FROM governance_records c LEFT JOIN tasks t ON t.id=json_extract(c.data,'$.owner_id') WHERE c.type='workflow_instance' AND c.owner_project=?1 AND coalesce(json_extract(c.data,'$.project_id'),?4)=?4 AND json_extract(c.data,'$.parent_execution_id') IN (SELECT json_extract(s.value,'$.id') FROM governance_records p,json_each(p.data,'$.steps') s WHERE p.id=?2 AND p.type='workflow_instance' AND p.owner_project=?1) ORDER BY c.rowid LIMIT 21 OFFSET ?3").map_err(storage)?;
        query
            .query_map(params![project, id, child_offset, project], |row| {
                Ok(WorkflowChildSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    revision: row.get(2)?,
                    owner_id: row.get(3)?,
                    status: row.get(4)?,
                    dependency_wait_count: row.get(5)?,
                    delivery: None,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    let children_more = trim_page(&mut children, 20);
    for child in &mut children {
        child.delivery = delivery_summary(&conn, project, &child.id)?;
    }
    let delivery = delivery_summary(&conn, project, id)?;
    let mut child_slots = Vec::new();
    {
        let mut query=conn.prepare_cached("SELECT g.definition_id,json_extract(g.data,'$.max_concurrency'),(SELECT count(*) FROM json_each(g.data,'$.children') members JOIN governance_records c ON c.id=members.value AND c.type='workflow_instance' AND c.owner_project=?1 WHERE json_extract(c.data,'$.status') IN ('running','needs_reconciliation')) FROM governance_records g WHERE g.type='workflow_children' AND g.owner_project=?1 AND g.definition_id IN (SELECT json_extract(s.value,'$.id') FROM governance_records p,json_each(p.data,'$.steps') s WHERE p.id=?2 AND p.type='workflow_instance' AND p.owner_project=?1) ORDER BY g.rowid LIMIT 20").map_err(storage)?;
        for row in query
            .query_map(params![project, id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, u64>(2)?,
                ))
            })
            .map_err(storage)?
        {
            let (execution_id, limit, active) = row.map_err(storage)?;
            child_slots.push(WorkflowChildSlots {
                execution_id,
                limit,
                active,
                available: limit.saturating_sub(active),
            });
        }
    }

    let cursor = cursor(&conn)?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(WorkflowManagementSummary {
        project_id: project.into(),
        cursor,
        record,
        children,
        children_more,
        next_child_offset: children_more.then_some(child_offset + 20),
        observed_only: true,
        delivery,
        child_slots,
    })
}

fn step_counts(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<WorkflowStepCounts> {
    Ok(WorkflowStepCounts {
        total: row.get(start)?,
        completed: row.get(start + 1)?,
        running: row.get(start + 2)?,
        waiting: row.get(start + 3)?,
        failed: row.get(start + 4)?,
    })
}

pub(super) fn progress(
    pool: &ConnectionPool,
    project: &str,
    id: &str,
    stage_offset: u64,
) -> Result<WorkflowProgress> {
    require(stage_offset <= 1_000_000, "阶段页偏移无效")?;
    let _span = performance::Span::new("read_model.workflow_progress");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let record=conn.query_row(
        "SELECT id,substr(name,1,128),revision,json_extract(data,'$.owner_id'),coalesce(json_extract(data,'$.root_instance_id'),id),json_extract(data,'$.status'),coalesce(json_extract(data,'$.paused'),0),coalesce(json_extract(data,'$.cancel_requested'),0) FROM governance_records WHERE id=?1 AND type='workflow_instance' AND owner_project=?2 AND coalesce(json_extract(data,'$.project_id'),?2)=?2",
        params![id,project],|row|Ok(RuntimeSummary {id:row.get(0)?,name:row.get(1)?,revision:row.get(2)?,data:WorkflowProgressState {owner_id:row.get(3)?,root_instance_id:row.get(4)?,status:row.get(5)?,paused:row.get(6)?,cancel_requested:row.get(7)?}})
    ).optional().map_err(storage)?.ok_or_else(||err("not_found","工作流不属于本次请求的项目"))?;
    let counts=conn.query_row(
        "SELECT count(*),coalesce(sum(json_extract(s.data,'$.status') IN ('completed','skipped')),0),coalesce(sum(json_extract(s.data,'$.status')='running'),0),coalesce(sum(json_extract(s.data,'$.status') IN ('waiting_input','waiting_confirmation','awaiting_acceptance')),0),coalesce(sum(json_extract(s.data,'$.status') IN ('failed','needs_reconciliation')),0) FROM workflow_display_steps s WHERE s.instance_id=?1 AND EXISTS(SELECT 1 FROM workflow_display_headers h WHERE h.instance_id=s.instance_id AND json_extract(h.record,'$.data.project_id')=?2)",params![id,project],|row|step_counts(row,0)
    ).map_err(storage)?;
    // The packed plan names its exact immutable workflow source. Never compile
    // it, follow today's head/default, or reconstruct unrelated engineering rows.
    let source_exists:bool=conn.query_row(
        "SELECT json_type(i.data,'$.plan.document')='object' OR EXISTS(SELECT 1 FROM source_definitions w WHERE w.type='workflow' AND w.reference=coalesce(json_extract(i.data,'$.plan.workflow_ref'),json_extract(i.data,'$.workflow_ref'))) FROM governance_records i WHERE i.id=?1",[id],|row|row.get::<_,Option<bool>>(0)
    ).map_err(storage)?.unwrap_or(false);
    require(
        source_exists,
        "冻结工作流源缺失，不能以当前默认替换阶段进度",
    )?;
    let mut stages = {
        let mut statement=conn.prepare_cached(
            "WITH instance AS MATERIALIZED (SELECT h.instance_id,coalesce(json_extract(h.record,'$.data.plan.document'),w.data) AS document FROM workflow_display_headers h LEFT JOIN source_definitions w ON w.type='workflow' AND w.reference=coalesce(json_extract(h.record,'$.data.plan.workflow_ref'),json_extract(h.record,'$.data.workflow_ref')) WHERE h.instance_id=?1 AND json_extract(h.record,'$.data.project_id')=?2), definitions AS MATERIALIZED (SELECT json_extract(d.value,'$.key') AS step,json_extract(d.value,'$.stage') AS stage FROM instance,json_each(instance.document,'$.steps') d), labels AS MATERIALIZED (SELECT s.key AS position,json_extract(s.value,'$.key') AS stage,substr(json_extract(s.value,'$.name'),1,128) AS name FROM instance,json_each(instance.document,'$.stages') s), progress AS MATERIALIZED (SELECT d.stage,count(*) AS total,sum(json_extract(e.data,'$.status') IN ('completed','skipped')) AS completed,sum(json_extract(e.data,'$.status')='running') AS running,sum(json_extract(e.data,'$.status') IN ('waiting_input','waiting_confirmation','awaiting_acceptance')) AS waiting,sum(json_extract(e.data,'$.status') IN ('failed','needs_reconciliation')) AS failed FROM instance JOIN workflow_display_steps e ON e.instance_id=instance.instance_id JOIN definitions d ON d.step=json_extract(e.data,'$.step') GROUP BY d.stage) SELECT l.stage,l.name,coalesce(p.total,0),coalesce(p.completed,0),coalesce(p.running,0),coalesce(p.waiting,0),coalesce(p.failed,0) FROM labels l LEFT JOIN progress p ON p.stage=l.stage ORDER BY l.position LIMIT 21 OFFSET ?3"
        ).map_err(storage)?;
        statement
            .query_map(params![id, project, stage_offset], |row| {
                Ok(WorkflowStageProgress {
                    key: row.get(0)?,
                    name: row.get(1)?,
                    counts: step_counts(row, 2)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    let more = trim_page(&mut stages, 20);
    let result = WorkflowProgress {
        project_id: project.into(),
        cursor: cursor(&conn)?,
        record,
        counts,
        stages,
        next_stage_offset: more.then_some(stage_offset + 20),
        observed_only: true,
    };
    require(
        serde_json::to_vec(&result).map_err(storage)?.len() <= 64 * 1024,
        "阶段进度摘要超过 64 KiB",
    )?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(result)
}
