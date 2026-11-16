//! Exact entity rows and collection predicates share one committed read snapshot.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{concurrency::*, repository_updates::*};

pub(super) fn registration(
    pool: &ConnectionPool,
    project: &str,
) -> Result<RepositoryRegistrationFacts> {
    let _span = performance::Span::new("mutation_context.repository_registration");
    let connection = pool.read()?;
    let tx = connection.unchecked_transaction().map_err(storage)?;
    let [project_row, repository_scope, feature_scope]: [_; 3] = resource_commits::read_on(
        &tx,
        &[
            ResourceKey::new("project", project),
            repository_collection(project),
            feature_collection(project),
        ],
    )?
    .try_into()
    .map_err(|_| err("storage", "仓库上下文读取不完整"))?;
    let (has_features,has_registration,has_mainline)=tx.query_row("SELECT EXISTS(SELECT 1 FROM features WHERE project_id=?1),EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND json_extract(action,'$.kind')='connect'),EXISTS(SELECT 1 FROM repository_worktrees WHERE project_id=?1 AND kind='mainline')",[project],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(RepositoryRegistrationFacts {
        project: project_row,
        repository_scope,
        feature_scope,
        has_features,
        has_registration,
        has_mainline,
    })
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
