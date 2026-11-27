//! One page of a project's domain entities with the workflow state of each owner.
//! Rows are filtered and ordered in SQL; the kernel decodes what it reads.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::read_models::{DomainEntityPage, DomainEntityQuery, DomainEntityRow};
use rusqlite::params;

/// Root workflow instances of `owner` that have not reached a terminal status.
/// `owner` is a SQL expression for the owner id.
pub(crate) fn active_root_workflows(owner: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.owner_id')={owner} AND json_extract(data,'$.parent_execution_id') IS NULL AND json_extract(data,'$.status') NOT IN ('completed','cancelled'))"
    )
}

pub(super) fn page(
    pool: &ConnectionPool,
    project: Option<&str>,
    query: &DomainEntityQuery,
) -> Result<DomainEntityPage> {
    let project = project.ok_or_else(|| err("invalid_scope", "领域对象分页需要项目库"))?;
    let _span = performance::Span::new("read_model.domain_entity_page");
    let conn = pool.read()?;
    let types = json!(query.types).to_string();
    // The entity is inside the row as a JSON string; its id and type are read from there.
    let payload = "json_extract(e.data,'$.payload')";
    let owner = format!("json_extract({payload},'$.id')");
    // rowid is the start order of the instances, which their updates do not change.
    let latest = format!(
        "SELECT json_object('id',i.id,'status',json_extract(i.data,'$.status'),'workflow_ref',json_extract(i.data,'$.plan.workflow_ref')) FROM governance_records i WHERE i.type='workflow_instance' AND i.owner_project=?1 AND json_extract(i.data,'$.owner_id')={owner} AND json_extract(i.data,'$.parent_execution_id') IS NULL ORDER BY i.rowid DESC LIMIT 1"
    );
    let mut statement = conn
        .prepare_cached(&format!(
            "SELECT e.revision,{payload},{} AS active,({latest}) AS last FROM governance_records e WHERE e.type='domain_entity' AND e.owner_project=?1 AND json_extract({payload},'$.type_ref') IN (SELECT value FROM json_each(?2)) AND e.id>?3 ORDER BY e.id LIMIT ?4",
            active_root_workflows(&owner)
        ))
        .map_err(storage)?;
    let limit = i64::try_from(query.limit + 1)
        .map_err(|_| err("invalid_params", "领域对象分页超出限制"))?;
    let mut rows = statement
        .query_map(
            params![project, types, query.after.as_deref().unwrap_or(""), limit],
            |row| {
                Ok(DomainEntityRow {
                    revision: row.get(0)?,
                    payload: row.get(1)?,
                    active: row.get(2)?,
                    last: row.get(3)?,
                })
            },
        )
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = rows.len() > query.limit;
    rows.truncate(query.limit);
    Ok(DomainEntityPage {
        records: rows,
        more,
    })
}
