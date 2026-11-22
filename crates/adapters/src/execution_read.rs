//! Bounded execution-page facts, read from one committed snapshot.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::read_models::*;
use rusqlite::{OptionalExtension, params};

pub(super) fn project(conn: &Connection, id: &str) -> Result<Project> {
    load_table_filtered_params(conn, "projects", "id=?1", &[&id])?
        .into_iter()
        .next()
        .ok_or_else(|| err("not_found", "项目不存在"))
}
fn feature(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeatureBrief> {
    Ok(FeatureBrief {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        title_truncated: row.get(3)?,
        status: row.get(4)?,
        revision: row.get(5)?,
        input_version: row.get(6)?,
    })
}

pub(super) fn feature_branches(
    pool: &ConnectionPool,
    project: &str,
    feature: &str,
    task_offset: u64,
) -> Result<Value> {
    const TASK_PAGE_SIZE: u64 = 50;
    let _span = performance::Span::new("read_model.feature_branches");
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let branch: Option<String> = tx
        .query_row(
            "SELECT w.branch FROM features f LEFT JOIN repository_worktrees w
           ON w.project_id=f.project_id AND w.owner_id=f.id AND w.kind='feature'
         WHERE f.id=?1 AND f.project_id=?2",
            params![feature, project],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| err("not_found", "Feature 不属于本次项目"))?;

    let candidate = feature_candidate(&tx, project, feature)?;
    let mut statement = tx.prepare_cached(
        "SELECT t.id,substr(t.title,1,128),w.branch,
                CASE WHEN json_extract(d.data,'$.completion.pull_request.merged')=1
                       AND json_extract(d.data,'$.publication.owner_id')=t.id
                       AND json_extract(d.data,'$.publication.source_branch')=w.branch
                     THEN 'completed' ELSE t.status END
         FROM tasks t
         LEFT JOIN repository_worktrees w ON w.project_id=?2 AND w.owner_id=t.id AND w.kind='task'
         LEFT JOIN governance_records d ON d.type='workflow_delivery' AND d.owner_project=?2
           AND d.id='workflow-delivery:'||(SELECT i.id FROM governance_records i
             WHERE i.type='workflow_instance' AND i.definition_id=t.id AND i.owner_project=?2
               AND json_extract(i.data,'$.owner_id')=t.id
               AND (json_extract(i.data,'$.parent_execution_id') IS NULL OR EXISTS(
                 SELECT 1 FROM governance_records v WHERE v.type='workflow_invocation' AND v.owner_project=?2
                   AND json_extract(v.data,'$.child_instance_id')=i.id
                   AND json_extract(v.data,'$.member_identity')=t.id))
             ORDER BY i.rowid DESC LIMIT 1)
         WHERE t.feature_id=?1 ORDER BY t.rowid LIMIT ?3 OFFSET ?4",
    ).map_err(storage)?;
    // Read one extra row to expose a continuation without counting the entire list.
    let mut tasks = statement
        .query_map(
            params![feature, project, TASK_PAGE_SIZE + 1, task_offset],
            |row| {
                Ok(
                    json!({"id":row.get::<_, String>(0)?,"title":row.get::<_, String>(1)?,
            "branch":row.get::<_, Option<String>>(2)?,"status":row.get::<_, String>(3)?}),
                )
            },
        )
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let next_task_offset =
        (tasks.len() as u64 > TASK_PAGE_SIZE).then_some(task_offset + TASK_PAGE_SIZE);
    tasks.truncate(TASK_PAGE_SIZE as usize);
    drop(statement);
    tx.commit().map_err(storage)?;
    Ok(
        json!({"feature_branch":branch,"candidate":candidate,"tasks":tasks,"next_task_offset":next_task_offset}),
    )
}

fn feature_candidate(conn: &Connection, project: &str, feature: &str) -> Result<Option<Value>> {
    let root: Option<(String, String)> = conn.query_row(
        "SELECT id,json_extract(data,'$.status') FROM governance_records
         WHERE type='workflow_instance' AND definition_id=?1 AND owner_project=?2
           AND json_extract(data,'$.owner_id')=?1 AND json_extract(data,'$.parent_execution_id') IS NULL
         ORDER BY rowid DESC LIMIT 1",
        params![feature, project], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(storage)?;
    let Some((instance, status)) = root else {
        return Ok(None);
    };
    let projection = |row: &rusqlite::Row<'_>| {
        Ok(
            json!({"branch":row.get::<_, String>(0)?,"pull_request_number":row.get::<_, Option<u64>>(1)?,
            "status":if row.get::<_, bool>(2)? { "completed" } else { "running" }}),
        )
    };
    let publication = conn.query_row(
        "SELECT json_extract(data,'$.publication.pull_request.head_branch'),
                json_extract(data,'$.publication.pull_request.number'),
                coalesce(json_extract(data,'$.completion.pull_request.merged'),0)
         FROM governance_records WHERE id='workflow-delivery:'||?1 AND type='workflow_delivery' AND owner_project=?2
           AND json_extract(data,'$.publication.owner_id')=?3
           AND json_extract(data,'$.publication.pull_request.head_branch') GLOB 'devdesk/candidates/*'",
        params![instance, project, feature], projection,
    ).optional().map_err(storage)?;
    if publication.is_some() {
        return Ok(publication);
    }

    // Older workflows have stage receipts instead of a delivery record. This also
    // covers a ref published by the current workflow while its PR is being opened.
    conn.query_row(
        "SELECT coalesce(json_extract(o.data,'$.result.Ok.head_branch'),json_extract(o.data,'$.result.Ok.branch')),
                json_extract(o.data,'$.result.Ok.number'), ?3='completed'
         FROM governance_records i,json_each(i.data,'$.steps') step
         JOIN governance_records c ON c.id=json_extract(step.value,'$.tool_call_id') AND c.type='platform_call'
         JOIN governance_records p ON p.definition_id=json_extract(c.data,'$.effect_id') AND p.type='platform_publish_stage'
         JOIN governance_records o ON o.definition_id=p.id AND o.type='platform_effect_observation'
         WHERE i.id=?1 AND i.owner_project=?2 AND p.owner_project=?2 AND o.owner_project=?2
           AND json_extract(c.data,'$.handler') IN ('git.publish','git.delivery.open')
           AND p.name IN ('candidate','candidate-ref','projected-ref','pull_request','pr','projected-pr')
           AND json_extract(o.data,'$.phase')='result'
           AND coalesce(json_extract(o.data,'$.result.Ok.head_branch'),json_extract(o.data,'$.result.Ok.branch')) GLOB 'devdesk/candidates/*'
         ORDER BY o.rowid DESC LIMIT 1",
        params![instance, project, status], projection,
    ).optional().map_err(storage)
}

pub(super) fn workspace(
    pool: &ConnectionPool,
    id: &str,
    query: &ExecutionWorkspaceQuery,
) -> Result<ExecutionWorkspace> {
    let _span = performance::Span::new("read_model.execution_workspace");
    require(
        query.limit > 0
            && query.limit <= 100
            && query.feature_offset <= 1_000_000
            && query.task_offset <= 1_000_000,
        "无效的执行分页",
    )?;
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let current = project(&tx, id)?;
    let fields =
        "id,project_id,substr(title,1,128),length(title)>128,status,revision,input_version";
    // The filter describes current work; historical nested executions cannot keep
    // a finished feature in a running/blocked queue.
    let predicate = "project_id=?1 AND (?2='' OR (?2='tasks' AND EXISTS(SELECT 1 FROM tasks t WHERE t.feature_id=features.id)) OR (?2 IN ('running','waiting_confirmation','blocked') AND EXISTS(SELECT 1 FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.type='workflow_instance' AND i.definition_id=features.id AND i.owner_project=?1 AND json_extract(i.data,'$.owner_id')=features.id AND json_extract(i.data,'$.parent_execution_id') IS NULL AND i.rowid=(SELECT max(j.rowid) FROM governance_records j WHERE j.type='workflow_instance' AND j.definition_id=features.id AND j.owner_project=?1 AND json_extract(j.data,'$.parent_execution_id') IS NULL) AND ((?2 IN ('running','waiting_confirmation') AND json_extract(s.value,'$.status')=?2) OR (?2='blocked' AND json_extract(s.value,'$.status') IN ('failed','needs_reconciliation','waiting_input'))))))";
    let total_features = tx
        .query_row(
            &format!("SELECT count(*) FROM features WHERE {predicate}"),
            params![id, query.work_filter],
            |row| row.get(0),
        )
        .map_err(storage)?;
    let features = {
        let mut statement = tx.prepare_cached(&format!("SELECT {fields} FROM features WHERE {predicate} ORDER BY rowid DESC LIMIT ?3 OFFSET ?4")).map_err(storage)?;
        statement
            .query_map(
                params![id, query.work_filter, query.limit, query.feature_offset],
                feature,
            )
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    let detail = query.feature_id.as_deref().map(|feature_id| -> Result<_> {
        let selected = tx.query_row(&format!("SELECT {fields} FROM features WHERE id=?1 AND project_id=?2"),params![feature_id,id],feature)
            .optional().map_err(storage)?.ok_or_else(||err("not_found","Feature 不属于本次项目"))?;
        let total_tasks = tx.query_row("SELECT count(*) FROM tasks WHERE feature_id=?1",[feature_id],|row|row.get(0)).map_err(storage)?;
        let tasks = {
            let mut statement = tx.prepare_cached("SELECT id,feature_id,substr(title,1,128),length(title)>128,status,revision,json_array_length(dependencies) FROM tasks WHERE feature_id=?1 ORDER BY rowid LIMIT ?2 OFFSET ?3").map_err(storage)?;
            statement.query_map(params![feature_id,query.limit,query.task_offset],|row|Ok(TaskBrief {
                id:row.get(0)?,feature_id:row.get(1)?,title:row.get(2)?,title_truncated:row.get(3)?,status:row.get(4)?,revision:row.get(5)?,dependency_count:row.get(6)?,
            })).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?
        };
        Ok(ExecutionFeatureDetail {feature:selected,tasks,total_tasks})
    }).transpose()?;
    let mut owners = vec![id];
    if let Some(detail) = &detail {
        owners.push(&detail.feature.id);
        owners.extend(detail.tasks.iter().map(|task| task.id.as_str()));
    }
    let mut configurations = Vec::new();
    let mut statement = tx.prepare_cached("SELECT i.id,substr(i.name,1,128),i.revision,json_extract(i.data,'$.owner_id'),coalesce(json_extract(i.data,'$.root_instance_id'),i.id),json_extract(i.data,'$.parent_execution_id'),json_extract(i.data,'$.active_plan_revision_id'),json_extract(i.data,'$.workflow_ref'),json_extract(i.data,'$.status'),json_extract(i.data,'$.plan.standard_ref'),json_extract(i.data,'$.plan.harness_permission_ref') FROM governance_records i WHERE i.type='workflow_instance' AND i.definition_id=?1 AND i.owner_project=?2 AND json_extract(i.data,'$.owner_id')=?1 AND coalesce(json_extract(i.data,'$.project_id'),?2)=?2 AND (json_extract(i.data,'$.parent_execution_id') IS NULL OR EXISTS(SELECT 1 FROM governance_records v WHERE v.type='workflow_invocation' AND v.owner_project=?2 AND json_extract(v.data,'$.child_instance_id')=i.id AND json_extract(v.data,'$.member_identity')=?1)) ORDER BY i.rowid DESC LIMIT 1").map_err(storage)?;
    for owner in owners {
        if let Some(root) = statement
            .query_row(params![owner, id], |row| {
                Ok(WorkRootMetadata {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    revision: row.get(2)?,
                    r#type: "workflow_instance",
                    work_root: true,
                    metadata_only: true,
                    data: WorkRootState {
                        owner_id: row.get(3)?,
                        root_instance_id: row.get(4)?,
                        parent_execution_id: row.get(5)?,
                        active_plan_revision_id: row.get(6)?,
                        workflow_ref: row.get(7)?,
                        status: row.get(8)?,
                        plan: PlanSelectionSummary {
                            standard_ref: row.get(9)?,
                            harness_permission_ref: row.get(10)?,
                        },
                    },
                })
            })
            .optional()
            .map_err(storage)?
        {
            configurations.push(root);
        }
    }
    drop(statement);
    let cursor = tx
        .query_row("SELECT coalesce(max(id),0) FROM events", [], |row| {
            row.get(0)
        })
        .map_err(storage)?;
    let repository_ready = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repository_worktrees WHERE kind='mainline')",
            [],
            |row| row.get(0),
        )
        .map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(ExecutionWorkspace {
        kind: "execution_workspace",
        project_id: id.into(),
        cursor,
        active_project: current,
        repository_ready,
        features,
        total_features,
        detail,
        configurations,
        observed_only: true,
    })
}
