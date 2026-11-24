//! Exact entity rows and collection predicates share one committed read snapshot.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{concurrency::*, repository_updates::*};

// A failed connection is idle unless its original attempt is queued for
// reconciliation or compatible recovery. Keep this in step with dispatch.
pub(super) const RUNNABLE_OPERATION: &str = "status IN ('prepared','dispatched','observed')
    OR (status='needs_reconciliation' AND (reason='reconcile_requested'
        OR (json_extract(action,'$.kind')='connect' AND reason='原物理观察已保留，原业务接纳版本冲突')))";

pub(super) fn domain_project_activity(conn: &Connection, project: &str) -> Result<Value> {
    let sql = format!(
        "SELECT
            EXISTS(SELECT 1 FROM governance_records
                WHERE type='workflow_instance' AND json_extract(data,'$.project_id')=?1
                  AND json_extract(data,'$.parent_execution_id') IS NULL
                  AND json_extract(data,'$.status') NOT IN ('completed','cancelled')),
            EXISTS(SELECT 1 FROM repository_operations
                WHERE project_id=?1 AND ({RUNNABLE_OPERATION}
                    OR (status='needs_reconciliation' AND json_extract(action,'$.kind') IS NOT 'connect'))),
            EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1
                AND status='needs_reconciliation' AND json_extract(action,'$.kind')='connect')"
    );
    conn.query_row(&sql, [project], |row| {
        let active_when_removed = row.get::<_, bool>(0)? || row.get::<_, bool>(1)?;
        let connection_review = row.get::<_, bool>(2)?;
        Ok(json!({"active": active_when_removed || connection_review, "active_when_removed": active_when_removed}))
    })
    .map_err(storage)
}

pub(super) fn registration(
    pool: &ConnectionPool,
    project: &str,
) -> Result<RepositoryRegistrationFacts> {
    let _span = performance::Span::new("mutation_context.repository_registration");
    let connection = pool.read()?;
    let tx = connection.unchecked_transaction().map_err(storage)?;
    let facts = registration_on(&tx, project)?;
    tx.commit().map_err(storage)?;
    Ok(facts)
}
fn registration_on(conn: &Connection, project: &str) -> Result<RepositoryRegistrationFacts> {
    let [project_row, repository_scope, feature_scope]: [_; 3] = resource_commits::read_on(
        conn,
        &[
            ResourceKey::new("project", project),
            repository_collection(project),
            feature_collection(project),
        ],
    )?
    .try_into()
    .map_err(|_| err("storage", "仓库上下文读取不完整"))?;
    let (has_features,has_registration,has_mainline)=conn.query_row("SELECT EXISTS(SELECT 1 FROM features WHERE project_id=?1),EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND json_extract(action,'$.kind')='connect'),EXISTS(SELECT 1 FROM repository_worktrees WHERE project_id=?1 AND kind='mainline')",[project],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(storage)?;
    Ok(RepositoryRegistrationFacts {
        project: project_row,
        repository_scope,
        feature_scope,
        has_features,
        has_registration,
        has_mainline,
    })
}
pub(super) fn connect_projection(
    pool: &ConnectionPool,
    project: &str,
    operation: &str,
) -> Result<devdesk_core::repository_projection::RepositoryConnectProjectionFacts> {
    let _span = performance::Span::new("mutation_context.repository_connect_projection");
    let connection = pool.read()?;
    let tx = connection.unchecked_transaction().map_err(storage)?;
    let registration = registration_on(&tx, project)?;
    let [operation_row, head, mainline]: [_; 3] = resource_commits::read_on(
        &tx,
        &[
            ResourceKey::new("repository_operation", operation),
            ResourceKey::new("head", format!("main:{project}")),
            ResourceKey::new("repository_worktree", project),
        ],
    )?
    .try_into()
    .map_err(|_| err("storage", "仓库接纳上下文读取不完整"))?;
    let (has_other_pending_operation,has_git_call)=tx.query_row("SELECT EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND id!=?2 AND status!='completed'),EXISTS(SELECT 1 FROM governance_records WHERE type='platform_call' AND owner_project=?1 AND substr(json_extract(data,'$.request.definition.qualified_name'),1,4)='git.' AND json_extract(data,'$.status') IN ('prepared','running','needs_reconciliation'))",rusqlite::params![project,operation],|row|Ok((row.get(0)?,row.get(1)?))).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(
        devdesk_core::repository_projection::RepositoryConnectProjectionFacts {
            registration,
            operation: operation_row,
            head,
            mainline,
            has_other_pending_operation,
            has_git_call,
        },
    )
}
pub(super) fn refresh(pool: &ConnectionPool, project: &str) -> Result<RepositoryRefreshFacts> {
    let _span = performance::Span::new("mutation_context.repository_refresh");
    let connection = pool.read()?;
    let tx = connection.unchecked_transaction().map_err(storage)?;
    let [project_row, repository_scope, mainline, head]: [_; 4] = resource_commits::read_on(
        &tx,
        &[
            ResourceKey::new("project", project),
            repository_collection(project),
            ResourceKey::new("repository_worktree", project),
            ResourceKey::new("head", format!("main:{project}")),
        ],
    )?
    .try_into()
    .map_err(|_| err("storage", "主干上下文读取不完整"))?;
    let (has_pending_operation,has_git_call)=tx.query_row("SELECT EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND status!='completed'),EXISTS(SELECT 1 FROM governance_records WHERE type='platform_call' AND owner_project=?1 AND substr(json_extract(data,'$.request.definition.qualified_name'),1,4)='git.' AND json_extract(data,'$.status') IN ('prepared','running','needs_reconciliation'))",[project],|row|Ok((row.get(0)?,row.get(1)?))).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(RepositoryRefreshFacts {
        project: project_row,
        repository_scope,
        mainline,
        head,
        has_pending_operation,
        has_git_call,
    })
}
