//! Indexed, committed reads of one actual workflow and its referenced facts.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::read_models::*;
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

/// Refreshing a disposable read view never changes a command's original basis.
pub(super) fn with_catalog<T>(
    store: &SqliteStore,
    mut read: impl FnMut(&ConfigurationCatalogSnapshot) -> Result<T>,
) -> Result<T> {
    for attempt in 0..3 {
        let catalog = store.configuration_snapshot()?;
        match read(&catalog) {
            Err(error) if error.code == "catalog_refresh_required" && attempt < 2 => continue,
            result => return result,
        }
    }
    Err(err(
        "catalog_refresh_required",
        "配置目录正在发布，请重新读取",
    ))
}

fn records(
    conn: &Connection,
    predicate: &str,
    parameters: &[&dyn rusqlite::ToSql],
    limit: usize,
) -> Result<Vec<ConfigurationRecord>> {
    let selected = format!(
        "id IN (SELECT id FROM governance_records WHERE {predicate} ORDER BY rowid LIMIT {})",
        limit + 1
    );
    let rows =
        load_table_filtered_params::<Value>(conn, "governance_records", &selected, parameters)?;
    require(
        rows.len() <= limit,
        "流程关联事实超过有界读取预算，请缩小范围",
    )?;
    rows.into_iter()
        .map(ConfigurationRecord::from_storage)
        .collect()
}
fn decode_record(
    catalog: &ConfigurationCatalogSnapshot,
    raw: &ConfigurationRecord,
    memo: &mut BTreeMap<String, Value>,
) -> Result<ConfigurationRecord> {
    let mut row = raw.clone();
    row.data = source_views::restore_with(&catalog.state, &row.data, memo)?;
    row.content_hash = content_digest(&row.refs, &row.data);
    Ok(row)
}
fn compact_step(step: &Value) -> Value {
    let mut selected = serde_json::Map::new();
    for field in [
        "id",
        "node_id",
        "plan_revision_id",
        "step",
        "path",
        "round",
        "status",
        "input_digest",
        "outputs",
        "agent_revision_id",
        "session_id",
        "turn_id",
        "started",
        "finished",
        "reason",
    ] {
        selected.insert(field.into(), step[field].clone());
    }
    Value::Object(selected)
}
fn compact_instance(row: &mut ConfigurationRecord) {
    let mut selected = serde_json::Map::new();
    for field in [
        "active_plan_revision_id",
        "control_version",
        "schema_version",
        "owner_id",
        "workflow_ref",
        "project_id",
        "status",
        "paused",
        "cancel_requested",
        "parent_execution_id",
        "root_instance_id",
        "inputs",
    ] {
        selected.insert(field.into(), row.data[field].clone());
    }
    selected.insert(
        "steps".into(),
        json!(
            row.data["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(compact_step)
                .collect::<Vec<_>>()
        ),
    );
    // Retain the exact plan source descriptor for decoding without compilation.
    selected.insert("plan".into(), row.data["plan"].clone());
    row.data = Value::Object(selected);
}

pub(super) fn detail(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    query: &WorkflowDetailQuery,
) -> Result<Option<WorkflowDetailFacts>> {
    let _span = performance::Span::new("read_model.workflow_detail");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "流程读取需要更新源目录"));
    }
    let id = if let Some(id) = &query.id {
        Some(id.clone())
    } else {
        conn.query_row("SELECT i.id FROM governance_records i WHERE i.type='workflow_instance' AND i.owner_project=?1 AND json_extract(i.data,'$.owner_id')=?2 AND (json_extract(i.data,'$.parent_execution_id') IS NULL OR EXISTS(SELECT 1 FROM governance_records v WHERE v.type='workflow_invocation' AND v.owner_project=?1 AND json_extract(v.data,'$.child_instance_id')=i.id AND json_extract(v.data,'$.member_identity')=?2)) ORDER BY i.rowid DESC LIMIT 1",params![project,query.owner_id],|r|r.get::<_,String>(0)).optional().map_err(storage)?
    };
    let Some(id) = id else {
        conn.execute_batch("COMMIT;").map_err(storage)?;
        return Ok(None);
    };
    if query.view == WorkflowDetailView::Status {
        let mut focused = query.clone();
        let mut focus = json!({});
        if let (Some(target), Some(execution)) =
            (&query.focus_instance_id, &query.focus_execution_id)
        {
            let mut cursor = target.clone();
            let mut selected_execution = execution.clone();
            let mut branch = None;
            for _ in 0..=WORKFLOW_DEPTH_LIMIT {
                if cursor == id {
                    let offset: u64 = conn.query_row("SELECT count(*) FROM workflow_display_steps WHERE instance_id=?1 AND execution_id<?2",params![id,selected_execution],|r|r.get(0)).map_err(storage)?;
                    focused.step_offset = offset / focused.step_limit * focused.step_limit;
                    if let Some(child) = &branch {
                        let offset: u64 = conn.query_row("SELECT count(*) FROM workflow_display_headers WHERE json_extract(record,'$.data.parent_execution_id') IN (SELECT execution_id FROM workflow_display_steps WHERE instance_id=?1) AND instance_id<?2",params![id,child],|r|r.get(0)).map_err(storage)?;
                        focused.child_offset = offset / 100 * 100;
                    }
                    focus = json!({"child_id":branch,"execution_id":selected_execution});
                    break;
                }
                let parent: Option<String> = conn.query_row("SELECT json_extract(record,'$.data.parent_execution_id') FROM workflow_display_headers WHERE instance_id=?1 AND json_extract(record,'$.data.project_id')=?2",params![cursor,project],|r|r.get(0)).optional().map_err(storage)?.flatten();
                let Some(parent) = parent else { break };
                let owner: Option<String> = conn.query_row("SELECT s.instance_id FROM workflow_display_steps s JOIN workflow_display_headers h ON h.instance_id=s.instance_id WHERE s.execution_id=?1 AND json_extract(h.record,'$.data.project_id')=?2",params![parent,project],|r|r.get(0)).optional().map_err(storage)?;
                let Some(owner) = owner else { break };
                branch = Some(cursor);
                cursor = owner;
                selected_execution = parent;
            }
        }
        let mut result = status(&conn, catalog, project, &id, &focused)?;
        result.focus = focus;
        conn.execute_batch("COMMIT;").map_err(storage)?;
        return Ok(Some(result));
    }
    let mut row = records(
        &conn,
        "id=?1 AND type='workflow_instance' AND owner_project=?2",
        &[&id, &project],
        1,
    )?
    .into_iter()
    .next()
    .ok_or_else(|| err("not_found", "工作流不属于本次请求的项目"))?;
    require(
        row.scope.project_id() == Some(project)
            && query
                .owner_id
                .as_deref()
                .is_none_or(|owner| row.data["owner_id"] == owner),
        "工作流与请求范围不符",
    )?;
    let steps = row.data["steps"]
        .as_array()
        .ok_or_else(|| err("storage", "流程步骤结构无效"))?;
    require(
        steps.len() <= WORKFLOW_STEP_LIMIT,
        "流程步骤超过有界读取预算",
    )?;
    let executions = steps
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect::<BTreeSet<_>>();
    let execution_ids = json!(executions).to_string();
    let output_ids = json!(
        steps
            .iter()
            .flat_map(|e| e["outputs"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, v)| v.as_str()))
            .collect::<BTreeSet<_>>()
    )
    .to_string();
    let outputs = if query.view == WorkflowDetailView::Detail {
        records(
            &conn,
            "type='workflow_output' AND owner_project=?1 AND id IN (SELECT value FROM json_each(?2))",
            &[&project, &output_ids],
            4096,
        )?
    } else {
        vec![]
    };
    let confirmations = if query.view != WorkflowDetailView::History {
        records(
            &conn,
            "type='workflow_confirmation' AND owner_project=?1 AND definition_id IN (SELECT value FROM json_each(?2)) AND EXISTS(SELECT 1 FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.id=?3 AND json_extract(s.value,'$.id')=governance_records.definition_id AND json_extract(s.value,'$.input_digest')=json_extract(governance_records.data,'$.input_digest') AND json_extract(s.value,'$.outputs')=json_extract(governance_records.data,'$.outputs'))",
            &[&project, &execution_ids, &id],
            WORKFLOW_STEP_LIMIT,
        )?
    } else {
        vec![]
    };
    let plan_id = row.data["active_plan_revision_id"].as_str().unwrap_or("");
    let waivers = if query.view != WorkflowDetailView::History {
        records(
            &conn,
            "type='workflow_waiver' AND owner_project=?1 AND json_extract(data,'$.plan_revision_id')=?2",
            &[&project, &plan_id],
            WORKFLOW_STEP_LIMIT * 2,
        )?
    } else {
        vec![]
    };
    let root = row.data["root_instance_id"].as_str().unwrap_or(&id);
    let materials = if query.view == WorkflowDetailView::Detail {
        records(
            &conn,
            "type='typed_material' AND owner_project=?1 AND json_extract(data,'$.project_id')=?1 AND json_extract(data,'$.root_instance_id')=?2",
            &[&project, &root],
            4096,
        )?
    } else {
        vec![]
    };
    let mut actors = BTreeMap::new();
    if query.view != WorkflowDetailView::History {
        let sessions = json!(
            steps
                .iter()
                .filter_map(|e| e["session_id"].as_str())
                .collect::<BTreeSet<_>>()
        )
        .to_string();
        let mut statement=conn.prepare_cached("SELECT id,agent,json_extract(runtime_binding,'$.agent_configuration.instance_ref'),json_extract(runtime_binding,'$.agent_configuration.agent_instance_id') FROM sessions WHERE id IN (SELECT value FROM json_each(?1)) AND project_id=?2").map_err(storage)?;
        let values = statement
            .query_map(params![sessions, project], |r| {
                Ok(WorkflowSessionActor {
                    session_id: r.get(0)?,
                    agent_ref: r.get(1)?,
                    agent_instance_id: r.get::<_, Option<String>>(2)?.or(r.get(3)?),
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        for value in values {
            actors.insert(value.session_id.clone(), value);
        }
    }
    let (children, children_more) = if query.view != WorkflowDetailView::History {
        let mut statement=conn.prepare_cached("SELECT id,substr(name,1,256),revision,json_extract(data,'$.owner_id'),json_extract(data,'$.parent_execution_id'),json_extract(data,'$.status') FROM governance_records WHERE type='workflow_instance' AND owner_project=?1 AND json_extract(data,'$.parent_execution_id') IN (SELECT value FROM json_each(?2)) ORDER BY rowid LIMIT 101 OFFSET ?3").map_err(storage)?;
        let mut rows=statement.query_map(params![project,execution_ids,query.child_offset],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"revision":r.get::<_,u64>(2)?,"metadata_only":true,"data":{"owner_id":r.get::<_,String>(3)?,"parent_execution_id":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?}}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        let more = rows.len() > 100;
        rows.truncate(100);
        (rows, more)
    } else {
        (vec![], false)
    };
    let mut history = Vec::new();
    let mut next_history_before = None;
    if query.view != WorkflowDetailView::Status {
        let mut statement=conn.prepare_cached("SELECT rowid,id,type,name,CASE type WHEN 'workflow_plan_revision' THEN json_object('parent_revision_id',json_extract(data,'$.parent_revision_id'),'reason',json_extract(data,'$.reason')) WHEN 'workflow_change' THEN json_object('status',json_extract(data,'$.status'),'new_plan_revision_id',json_extract(data,'$.new_plan_revision_id'),'affected',json_extract(data,'$.preview.affected'),'bypassed',json_extract(data,'$.preview.bypassed')) ELSE json_object('id',json_extract(data,'$.id'),'node_id',json_extract(data,'$.node_id'),'plan_revision_id',json_extract(data,'$.plan_revision_id'),'status',json_extract(data,'$.status'),'step',json_extract(data,'$.step'),'round',json_extract(data,'$.round'),'reason',json_extract(data,'$.reason')) END FROM governance_records WHERE owner_project=?1 AND (?3 IS NULL OR rowid<?3) AND ((type='workflow_plan_revision' AND json_extract(data,'$.instance_id')=?2) OR (type='workflow_execution_history' AND json_extract(data,'$.workflow_instance_id')=?2) OR (type='workflow_change' AND definition_id=?2)) ORDER BY rowid DESC LIMIT 101").map_err(storage)?;
        let mut rows=statement.query_map(params![project,id,query.history_before],|r|Ok((r.get::<_,u64>(0)?,json!({"id":r.get::<_,String>(1)?,"type":r.get::<_,String>(2)?,"name":r.get::<_,String>(3)?,"data":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(4,rusqlite::types::Type::Text,Box::new(e)))?})))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        let more = rows.len() > 100;
        rows.truncate(100);
        next_history_before = more.then(|| rows.last().unwrap().0);
        history = rows.into_iter().map(|(_, v)| v).collect();
    }
    let mut rule_hints = BTreeMap::new();
    if query.view == WorkflowDetailView::Status {
        let hints = records(
            &conn,
            "type='workflow_rule_hint' AND definition_id IN (SELECT value FROM json_each(?1))",
            &[&execution_ids],
            WORKFLOW_STEP_LIMIT,
        )?;
        for hint in hints {
            let basis: devdesk_core::concurrency::ReadSet =
                serde_json::from_value(hint.data["basis"].clone()).map_err(storage)?;
            let value = match resource_commits::verify_read_set(&conn, &basis) {
                Ok(()) => hint.data["satisfied"].as_bool(),
                Err(error) if error.code == "revision_conflict" => None,
                Err(error) => return Err(error),
            };
            rule_hints.insert(hint.definition_id, value);
        }
    }
    let mut epochs=conn.prepare_cached("SELECT json_extract(resource_id,'$[1]'),revision FROM resource_versions WHERE kind='workflow_step' AND json_extract(resource_id,'$[0]')=?1").map_err(storage)?;
    let step_revisions = epochs
        .query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
        .map_err(storage)?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .map_err(storage)?;
    drop(epochs);
    let cursor = conn
        .query_row("SELECT coalesce(max(id),0) FROM events", [], |r| {
            r.get::<_, u64>(0)
        })
        .map_err(storage)?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let mut memo = Default::default();
    let plan_document = if query.view == WorkflowDetailView::History {
        Value::Null
    } else if query.view == WorkflowDetailView::Status {
        let reference = row.data["plan"]["workflow_ref"]
            .as_str()
            .or_else(|| row.data["workflow_ref"].as_str())
            .ok_or_else(|| err("storage", "流程缺少确切源引用"))?;
        let mut document: WorkflowDefinition = if row.data["plan"]["document"].is_object() {
            serde_json::from_value(source_views::restore_with(
                &catalog.state,
                &row.data["plan"]["document"],
                &mut memo,
            )?)
            .map_err(storage)?
        } else {
            catalog.state.configuration(reference)?.decode()?
        };
        document.assign_node_ids(reference);
        let document = json!(document);
        compact_instance(&mut row);
        row.data.as_object_mut().unwrap().remove("plan");
        row.data = source_views::restore_with(&catalog.state, &row.data, &mut memo)?;
        row.data["plan"] = json!({"document":document});
        document
    } else {
        row = decode_record(catalog, &row, &mut memo)?;
        row.data["plan"]["document"].clone()
    };
    let restore = |rows: Vec<ConfigurationRecord>, memo: &mut BTreeMap<String, Value>| {
        rows.iter()
            .map(|r| decode_record(catalog, r, memo))
            .collect::<Result<Vec<_>>>()
    };
    let outputs = restore(outputs, &mut memo)?;
    let confirmations = restore(confirmations, &mut memo)?;
    let materials = restore(materials, &mut memo)?;
    let waivers = restore(waivers, &mut memo)?;
    let history = history
        .into_iter()
        .map(|v| source_views::restore_with(&catalog.state, &v, &mut memo))
        .collect::<Result<Vec<_>>>()?;
    if query.view == WorkflowDetailView::History {
        compact_instance(&mut row);
        row.data.as_object_mut().unwrap().remove("plan");
    }
    Ok(Some(WorkflowDetailFacts {
        step_offset: query.step_offset,
        child_offset: query.child_offset,
        focus: Value::Null,
        cursor,
        step_total: row.data["steps"].as_array().map_or(0, |s| s.len()) as u64,
        next_step_offset: None,
        record: row,
        plan_document,
        step_revisions,
        rule_hints,
        outputs,
        confirmations,
        waivers,
        materials,
        actors,
        children,
        children_more,
        next_child_offset: children_more.then_some(query.child_offset + 100),
        history,
        next_history_before,
    }))
}

fn status(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    id: &str,
    query: &WorkflowDetailQuery,
) -> Result<WorkflowDetailFacts> {
    let raw = conn
        .query_row(
            "SELECT record FROM workflow_display_headers WHERE instance_id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| err("read_projection_pending", "该流程状态投影尚未接纳"))?;
    let mut row = ConfigurationRecord::from_storage(serde_json::from_str(&raw).map_err(storage)?)?;
    require(
        row.scope.project_id() == Some(project)
            && query
                .owner_id
                .as_deref()
                .is_none_or(|owner| row.data["owner_id"] == owner),
        "流程状态跨归属",
    )?;
    let step_total: u64 = conn
        .query_row(
            "SELECT count(*) FROM workflow_display_steps WHERE instance_id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let mut q=conn.prepare_cached("SELECT data,step_revision FROM workflow_display_steps WHERE instance_id=?1 ORDER BY execution_id LIMIT ?2 OFFSET ?3").map_err(storage)?;
    let raw = q
        .query_map(params![id, query.step_limit, query.step_offset], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    require(raw.len() <= WORKFLOW_STEP_LIMIT, "流程状态步骤超过预算")?;
    let mut steps = vec![];
    let mut step_revisions = BTreeMap::new();
    for (body, version) in raw {
        let mut step: Value = serde_json::from_str(&body).map_err(storage)?;
        if step["reason"]
            .as_str()
            .is_some_and(source_views::is_encoded_text)
        {
            step["reason"] = json!("");
            step["reason_truncated"] = json!(true);
        }
        if step["status"] == "needs_reconciliation" {
            let latest: Option<String> = conn.query_row(
                "SELECT json_object('id',id,'status',json_extract(data,'$.status'),'revision',revision,'resolution',json_extract(data,'$.resolution'),'error',CASE WHEN json_type(data,'$.error')='object' THEN json_object('code',json_extract(data,'$.error.code'),'message',substr(json_extract(data,'$.error.message'),1,2048),'message_truncated',length(json_extract(data,'$.error.message'))>2048) ELSE NULL END) FROM governance_records WHERE type='workflow_reconciliation' AND owner_project=?1 AND definition_id=?2 AND json_extract(data,'$.instance_id')=?3 ORDER BY rowid DESC LIMIT 1",
                params![project, step["id"].as_str(), id],
                |r| r.get(0),
            ).optional().map_err(storage)?;
            if let Some(latest) = latest {
                step["reconciliation"] = serde_json::from_str(&latest).map_err(storage)?;
                let error = &mut step["reconciliation"]["error"];
                if error["message"]
                    .as_str()
                    .is_some_and(source_views::is_encoded_text)
                {
                    error["message"] = json!("");
                    error["message_truncated"] = json!(true);
                }
            }
        }
        step_revisions.insert(
            step["id"]
                .as_str()
                .ok_or_else(|| err("storage", "实际显示步骤缺少身份"))?
                .to_owned(),
            version,
        );
        steps.push(step);
    }
    let ids = json!(step_revisions.keys().collect::<Vec<_>>()).to_string();
    let reference = row.data["plan"]["workflow_ref"]
        .as_str()
        .or_else(|| row.data["workflow_ref"].as_str())
        .ok_or_else(|| err("storage", "流程显示缺少确切源引用"))?;
    let plan_revision = row.data["active_plan_revision_id"].as_str().unwrap_or("");
    let mut document: WorkflowDefinition = match catalog.state.configuration(reference) {
        Ok(source) => source.decode()?,
        Err(error) if error.code == "not_found" && !plan_revision.is_empty() => {
            let plan = workflow_readers::record(conn, plan_revision)?;
            require(
                plan.r#type == "workflow_plan_revision"
                    && plan.scope.project_id() == Some(project)
                    && plan.data["instance_id"] == id
                    && plan.data["plan"]["workflow_ref"]
                        .as_str()
                        .or_else(|| plan.data["plan"]["workflow_revision_id"].as_str())
                        == Some(reference),
                "冻结计划不属于所选流程版本",
            )?;
            serde_json::from_value(workflow_readers::source_document(catalog, &plan)?)
                .map_err(storage)?
        }
        Err(error) => return Err(error),
    };
    document.assign_node_ids(reference);
    document.steps.retain(|s| {
        steps.iter().any(|e| {
            e["node_id"] == s.node_id
                || (e["node_id"].as_str().is_none_or(str::is_empty) && e["step"] == s.key)
        })
    });
    let plan_document = json!(document);
    row.data["plan"] = json!({"document":plan_document});
    row.data["steps"] = json!(steps);
    let confirmations = records(
        conn,
        "type='workflow_confirmation' AND owner_project=?1 AND definition_id IN (SELECT value FROM json_each(?2)) AND EXISTS(SELECT 1 FROM workflow_display_steps s WHERE s.instance_id=?3 AND s.execution_id=governance_records.definition_id AND json_extract(s.data,'$.input_digest')=json_extract(governance_records.data,'$.input_digest') AND json_extract(s.data,'$.outputs')=json_extract(governance_records.data,'$.outputs'))",
        &[&project, &ids, &id],
        WORKFLOW_STEP_LIMIT,
    )?;
    let plan = row.data["active_plan_revision_id"].as_str().unwrap_or("");
    let waivers = records(
        conn,
        "type='workflow_waiver' AND owner_project=?1 AND json_extract(data,'$.plan_revision_id')=?2",
        &[&project, &plan],
        WORKFLOW_STEP_LIMIT * 2,
    )?;
    let mut rule_hints = BTreeMap::new();
    for hint in records(
        conn,
        "type='workflow_rule_hint' AND definition_id IN (SELECT value FROM json_each(?1))",
        &[&ids],
        WORKFLOW_STEP_LIMIT,
    )? {
        let basis: devdesk_core::concurrency::ReadSet =
            serde_json::from_value(hint.data["basis"].clone()).map_err(storage)?;
        let value = match resource_commits::verify_read_set(conn, &basis) {
            Ok(()) => hint.data["satisfied"].as_bool(),
            Err(e) if e.code == "revision_conflict" => None,
            Err(e) => return Err(e),
        };
        rule_hints.insert(hint.definition_id, value);
    }
    let sessions = json!(
        row.data["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["session_id"].as_str())
            .collect::<Vec<_>>()
    )
    .to_string();
    let mut q=conn.prepare_cached("SELECT id,agent,coalesce(json_extract(runtime_binding,'$.agent_configuration.instance_ref'),json_extract(runtime_binding,'$.agent_configuration.agent_instance_id')) FROM sessions WHERE project_id=?1 AND id IN (SELECT value FROM json_each(?2))").map_err(storage)?;
    let actors = q
        .query_map(params![project, sessions], |r| {
            Ok(WorkflowSessionActor {
                session_id: r.get(0)?,
                agent_ref: r.get(1)?,
                agent_instance_id: r.get(2)?,
            })
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?
        .into_iter()
        .map(|a| (a.session_id.clone(), a))
        .collect();
    let root = row.data["root_instance_id"].as_str().unwrap_or(id);
    let mut q=conn.prepare_cached("SELECT json_object('id',instance_id,'name',substr(json_extract(record,'$.name'),1,256),'revision',json_extract(record,'$.revision'),'metadata_only',json('true'),'data',json_object('parent_execution_id',json_extract(record,'$.data.parent_execution_id'),'status',json_extract(record,'$.data.status'),'owner_id',json_extract(record,'$.data.owner_id'))) FROM workflow_display_headers WHERE json_extract(record,'$.data.root_instance_id')=?1 AND json_extract(record,'$.data.parent_execution_id') IN (SELECT execution_id FROM workflow_display_steps WHERE instance_id=?2) ORDER BY instance_id LIMIT 101 OFFSET ?3").map_err(storage)?;
    let mut children = q
        .query_map(params![root, id, query.child_offset], |r| {
            r.get::<_, String>(0)
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?
        .into_iter()
        .map(|s| serde_json::from_str::<Value>(&s).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    let more = children.len() > 100;
    children.truncate(100);
    let cursor = conn
        .query_row("SELECT coalesce(max(id),0) FROM events", [], |r| {
            r.get::<_, u64>(0)
        })
        .map_err(storage)?;
    Ok(WorkflowDetailFacts {
        step_offset: query.step_offset,
        child_offset: query.child_offset,
        focus: Value::Null,
        step_total,
        next_step_offset: (query.step_offset + query.step_limit < step_total)
            .then_some(query.step_offset + query.step_limit),
        cursor,
        record: row,
        plan_document,
        step_revisions,
        rule_hints,
        outputs: vec![],
        confirmations,
        waivers,
        materials: vec![],
        actors,
        children,
        children_more: more,
        next_child_offset: more.then_some(query.child_offset + 100),
        history: vec![],
        next_history_before: None,
    })
}

pub(super) fn task(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    turn: &str,
) -> Result<Option<Value>> {
    let _span = performance::Span::new("read_model.execution_task");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "原任务读取需要更新源目录"));
    }
    let record=records(&conn,"type IN ('configured_execution_task','archived_configured_execution_task') AND definition_id=?1 AND owner_project=?2",&[&turn,&project],1)?.into_iter().next();
    let Some(record) = record else {
        conn.execute_batch("COMMIT;").map_err(storage)?;
        return Ok(None);
    };
    let delivered=conn.query_row("SELECT content FROM conversation_events WHERE turn_id=?1 AND kind='user' AND EXISTS(SELECT 1 FROM sessions WHERE sessions.id=conversation_events.session_id AND sessions.project_id=?2) ORDER BY rowid LIMIT 1",params![turn,project],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    let delivered = delivered
        .map(|body| payload_store::unpack_text(&conn, body))
        .transpose()?;
    let current_feature = conn
        .query_row(
            "SELECT f.id,f.title FROM turns t
             JOIN sessions s ON s.id=t.session_id AND s.project_id=t.project_id
             JOIN features f ON f.id=s.feature_id AND f.project_id=s.project_id
             WHERE t.id=?1 AND t.project_id=?2",
            params![turn, project],
            |row| Ok(json!({"id":row.get::<_, String>(0)?,"title":row.get::<_, String>(1)?})),
        )
        .optional()
        .map_err(storage)?;
    let descriptor = record.data["task"].clone();
    let execution = descriptor["source"]["execution_id"]
        .as_str()
        .ok_or_else(|| err("storage", "原任务缺少执行引用"))?;
    let facts = workflow_intents::task_source_facts(
        &conn,
        &record.scope,
        execution,
        descriptor.clone(),
        delivered.map(|body| (body, record.data["digest"].as_str().unwrap_or("").into())),
    )?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let mut memo = Default::default();
    let body = workflow_intents::restore_task(catalog, facts, &mut memo)?;
    let mut data = record.data.clone();
    data.as_object_mut().unwrap().remove("task");
    data.as_object_mut().unwrap().remove("rendered_input");
    let mut data = source_views::restore_with(&catalog.state, &data, &mut memo)?;
    data["task"] = serde_json::from_str(&body).map_err(storage)?;
    data["rendered_input"] = json!(body);
    let task: ConfiguredAgentTask = serde_json::from_value(data.clone()).map_err(storage)?;
    task.validate()?;
    require(task.turn_id == turn, "原任务轮次不符")?;
    let input_ids = task.task["input_sources"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, source)| source["source_output_id"].as_str())
        .collect::<Vec<_>>();
    let mut input_material_names = BTreeMap::new();
    if !input_ids.is_empty() {
        let conn = pool.read()?;
        let mut query = conn.prepare_cached("SELECT o.id,d.name FROM json_each(?1) ids JOIN governance_records o ON o.id=ids.value AND o.owner_project=?2 AND o.type='workflow_output' JOIN source_definitions d ON d.reference=json_extract(o.data,'$.delivery_ref') AND d.type='delivery_type'").map_err(storage)?;
        input_material_names = query
            .query_map(params![json!(input_ids).to_string(), project], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(storage)?;
    }
    Ok(Some(
        json!({"configured":true,"archived":record.r#type=="archived_configured_execution_task","task":data,"turn_id":turn,"current_feature":current_feature,"input_material_names":input_material_names}),
    ))
}
