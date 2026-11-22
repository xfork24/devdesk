//! Read adoption records without reconstructing the execution history.
use super::*;
use rusqlite::OptionalExtension;

const WORK_HISTORY: &str = "
type='workflow_plan_revision' AND owner_project=?2
AND json_extract(data,'$.owner_id')=?1
AND json_extract(data,'$.instance_id') IN (
    SELECT i.id FROM governance_records i
    WHERE i.type='workflow_instance' AND i.owner_project=?2
    AND (
        json_extract(i.data,'$.parent_execution_id') IS NULL
        OR EXISTS (
            SELECT 1 FROM governance_records v
            WHERE v.type='workflow_invocation' AND v.owner_project=?2
            AND json_extract(v.data,'$.child_instance_id')=i.id
            AND json_extract(v.data,'$.member_identity')=json_extract(i.data,'$.owner_id')
        )
    )
)";

fn records(
    conn: &Connection,
    predicate: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<Vec<ConfigurationRecord>> {
    load_table_filtered_params::<Value>(conn, "governance_records", predicate, parameters)?
        .into_iter()
        .map(ConfigurationRecord::from_storage)
        .collect()
}

fn restore_selected<'a>(
    state: &EngineeringState,
    selected: impl IntoIterator<Item = &'a mut ConfigurationRecord>,
) -> Result<()> {
    let _span = performance::Span::new("standard.restore_selected");
    let mut memo = Default::default();
    for record in selected {
        record.data = source_views::restore_with(state, &record.data, &mut memo)?;
        record.content_hash = content_digest(&record.refs, &record.data);
    }
    Ok(())
}

pub(super) fn work_configuration(
    store: &SqliteStore,
    owner: &str,
    project: Option<&str>,
    include_history: bool,
) -> Result<Value> {
    let _span = performance::Span::new("standard.work_configuration");
    workflow_details::with_catalog(store, |catalog| {
        let conn = store.connection.read()?;
        let transaction = conn.unchecked_transaction().map_err(storage)?;
        let owner_project = transaction.query_row(
            "SELECT id FROM projects WHERE id=?1
             UNION ALL SELECT project_id FROM features WHERE id=?1
             UNION ALL SELECT f.project_id FROM tasks t JOIN features f ON t.feature_id=f.id WHERE t.id=?1
             LIMIT 1",
            [owner],
            |row| row.get::<_, String>(0),
        ).optional().map_err(storage)?
            .ok_or_else(|| err("not_found", "工作对象不存在"))?;
        require(project.is_none_or(|id| id == owner_project), "工作跨项目")?;
        let mut configuration = records(
            &transaction,
            "id=(SELECT id FROM governance_records
            WHERE type='work_configuration' AND definition_id=?1 AND owner_project=?2
            ORDER BY version DESC LIMIT 1)",
            &[&owner, &owner_project],
        )?;
        let mut history = if include_history {
            records(&transaction, WORK_HISTORY, &[&owner, &owner_project])?
        } else {
            Vec::new()
        };
        require_current_catalog(&transaction, catalog)?;
        transaction.commit().map_err(storage)?;
        drop(conn);
        restore_selected(&catalog.state, configuration.iter_mut().chain(&mut history))?;
        Ok(json!({"configuration":configuration.pop(),"history":history}))
    })
}

pub(super) fn snapshot(store: &SqliteStore, usage_ref: Option<&str>) -> Result<EngineeringState> {
    let _span = performance::Span::new("standard.work_projection");
    workflow_details::with_catalog(store, |catalog| {
        let conn = store.connection.read()?;
        let transaction = conn.unchecked_transaction().map_err(storage)?;
        let mut state = catalog.state.as_ref().clone();
        let mut selected = if let Some(reference) = usage_ref {
            records(
                &transaction,
                "type IN ('project_configuration','work_configuration','workflow_plan_revision')
                AND json_extract(data,'$.standard_ref')=?1",
                &[&reference],
            )?
        } else {
            state.features = load_table(&transaction, "features")?;
            state.tasks = load_table(&transaction, "tasks")?;
            records(
                &transaction,
                "type IN ('workflow_instance','workflow_plan_revision','workflow_invocation','workflow_waiver')",
                &[],
            )?
        };
        require_current_catalog(&transaction, catalog)?;
        transaction.commit().map_err(storage)?;
        drop(conn);
        // Comparisons use the persisted workflow and plan metadata directly.
        // History and evidence returned to clients retain their original shapes.
        restore_selected(
            &state,
            selected.iter_mut().filter(|record| {
                usage_ref.is_some()
                    || matches!(
                        record.r#type.as_str(),
                        "workflow_invocation" | "workflow_waiver"
                    )
            }),
        )?;
        if let Some(reference) = usage_ref {
            state.governance.retain(|record| {
                !matches!(
                    record.r#type.as_str(),
                    "project_configuration" | "work_configuration"
                ) || record.data["standard_ref"] != reference
            });
        }
        state.governance.extend(selected);
        state.refresh_configuration_views()?;
        Ok(state)
    })
}

fn require_current_catalog(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
) -> Result<()> {
    if configuration_cache::revision(conn)? != catalog.revision {
        return Err(err("catalog_refresh_required", "采用的规范需要更新源目录"));
    }
    Ok(())
}
