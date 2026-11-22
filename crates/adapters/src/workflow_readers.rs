//! Paged workflow readers select actual scoped rows and exact immutable sources.
//! They never construct EngineeringState or compile a runtime permission view.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{concurrency::ResourceKey, read_models::*};
use rusqlite::{OptionalExtension, params};
use std::collections::BTreeMap;

const SCOPE: &str = "WITH RECURSIVE owners(id,kind,title) AS MATERIALIZED (
 SELECT id,'project',title FROM projects WHERE id=?1 AND id=?2
 UNION ALL SELECT id,'feature',title FROM features WHERE (id=?1 OR project_id=?1) AND project_id=?2
 UNION ALL SELECT t.id,'task',t.title FROM tasks t JOIN features f ON f.id=t.feature_id WHERE (t.id=?1 OR t.feature_id=?1 OR f.project_id=?1) AND f.project_id=?2
), roots(id) AS MATERIALIZED (
 SELECT i.id FROM governance_records i JOIN owners o ON json_extract(i.data,'$.owner_id')=o.id
 WHERE i.type='workflow_instance' AND i.owner_project=?2
 AND (json_extract(i.data,'$.parent_execution_id') IS NULL OR EXISTS(SELECT 1 FROM governance_records v WHERE v.type='workflow_invocation' AND v.owner_project=?2 AND json_extract(v.data,'$.child_instance_id')=i.id AND json_extract(v.data,'$.member_identity')=o.id))

), selected(id) AS (
 SELECT id FROM roots
 UNION SELECT c.id FROM selected x JOIN governance_records p ON p.id=x.id JOIN json_each(p.data,'$.steps') e JOIN governance_records c ON c.type='workflow_instance' AND json_extract(c.data,'$.parent_execution_id')=json_extract(e.value,'$.id') AND c.owner_project=?2 JOIN owners o ON o.id=json_extract(c.data,'$.owner_id')
), entries(instance_id,instance_name,owner_id,owner_kind,owner_title,workflow_ref,execution,historical,ordering,position) AS (
 SELECT i.id,i.name,o.id,o.kind,o.title,json_extract(i.data,'$.workflow_ref'),e.value,CASE WHEN EXISTS(SELECT 1 FROM governance_records newer WHERE newer.type='workflow_instance' AND newer.owner_project=?2 AND json_extract(newer.data,'$.owner_id')=o.id AND newer.rowid>i.rowid AND json_extract(newer.data,'$.parent_execution_id') IS NULL) THEN 1 ELSE 0 END,i.rowid,CAST(e.key AS INTEGER) FROM selected x JOIN governance_records i ON i.id=x.id JOIN owners o ON o.id=json_extract(i.data,'$.owner_id') JOIN json_each(i.data,'$.steps') e
 UNION ALL SELECT i.id,i.name,o.id,o.kind,o.title,json_extract(i.data,'$.workflow_ref'),h.data,1,h.rowid,0 FROM selected x JOIN governance_records i ON i.id=x.id JOIN owners o ON o.id=json_extract(i.data,'$.owner_id') JOIN governance_records h ON h.type='workflow_execution_history' AND h.owner_project=?2 AND json_extract(h.data,'$.workflow_instance_id')=i.id
)";

fn optional<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p[key].as_str().filter(|value| !value.is_empty())
}
fn identifier(p: &Value, key: &str) -> Result<String> {
    devdesk_core::read_models::identifier(p, key)
}
fn record(conn: &Connection, id: &str) -> Result<ConfigurationRecord> {
    let row = resource_commits::read_on(conn, &[ResourceKey::new("governance", id)])?.remove(0);
    ConfigurationRecord::from_storage(
        row.value
            .ok_or_else(|| err("not_found", "所选流程记录缺失"))?,
    )
}
fn restored(
    catalog: &ConfigurationCatalogSnapshot,
    mut row: ConfigurationRecord,
) -> Result<ConfigurationRecord> {
    row.data = source_views::restore(&catalog.state, &row.data)?;
    row.content_hash = content_digest(&row.refs, &row.data);
    Ok(row)
}
fn source_document(
    catalog: &ConfigurationCatalogSnapshot,
    plan: &ConfigurationRecord,
) -> Result<Value> {
    let descriptor = &plan.data["plan"];
    let reference = descriptor["workflow_ref"]
        .as_str()
        .or_else(|| descriptor["workflow_revision_id"].as_str())
        .ok_or_else(|| err("storage", "原计划缺少确切工作流引用"))?;
    let mut document: WorkflowDefinition = if descriptor["document"].is_object() {
        serde_json::from_value(source_views::restore(
            &catalog.state,
            &descriptor["document"],
        )?)
        .map_err(storage)?
    } else {
        catalog.state.configuration(reference)?.decode()?
    };
    document.assign_node_ids(reference);
    Ok(json!(document))
}
fn step_source(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    instance: &str,
    e: &Value,
    cache: &mut BTreeMap<String, Value>,
) -> Result<Value> {
    let plan = e["plan_revision_id"]
        .as_str()
        .ok_or_else(|| err("storage", "执行缺少原始计划"))?;
    let key = json!([instance, plan]).to_string();
    if !cache.contains_key(&key) {
        let row = record(conn, plan)?;
        require(
            row.r#type == "workflow_plan_revision"
                && row.scope.project_id() == Some(project)
                && row.data["instance_id"] == instance,
            "原执行计划不属于所选流程",
        )?;
        cache.insert(key.clone(), source_document(catalog, &row)?);
    }
    let document = &cache[&key];
    let step = document["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["node_id"] == e["node_id"])
        .ok_or_else(|| err("not_found", "原执行步骤合同缺失"))?;
    Ok(json!({"document":document,"step":step}))
}
fn actor(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    e: &Value,
    step: &Value,
) -> Result<Value> {
    if let Some(session) = e["session_id"].as_str() {
        let actual=conn.query_row("SELECT agent,coalesce(json_extract(runtime_binding,'$.agent_configuration.instance_ref'),json_extract(runtime_binding,'$.agent_configuration.agent_instance_id')) FROM sessions WHERE id=?1 AND project_id=?2",params![session,project],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).optional().map_err(storage)?;
        if let Some((reference, instance)) = actual {
            let agent = catalog.state.configuration(&reference)?;
            return Ok(
                json!({"kind":"agent","name":agent.name,"agent_revision_id":reference,"agent_instance_id":instance,"session_id":session,"role_ref":agent.refs.get("role_ref")}),
            );
        }
    }
    let kind = step["executor"]["type"].as_str().unwrap_or("unknown");
    let name = match kind {
        "agent" => "尚无实际 Agent 会话",
        "workflow" => "子流程",
        "control" => "流程控制",
        "tool" => step["executor"]["tool_ref"]
            .as_str()
            .and_then(|id| catalog.state.configuration(id).ok())
            .map(|r| r.name.as_str())
            .unwrap_or("工具"),
        _ => "未记录执行方式",
    };
    Ok(
        json!({"kind":kind,"name":name,"assigned_agent_ref":e["agent_revision_id"],"role_ref":step["executor"]["role_ref"]}),
    )
}
fn page(p: &Value) -> Result<(u64, usize)> {
    let offset = p
        .get("offset")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(|| err("invalid_params", "无效的读取偏移"))
        })
        .transpose()?
        .unwrap_or(0);
    let limit = p
        .get("limit")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0 && *n <= 100)
                .ok_or_else(|| err("invalid_params", "读取页大小必须为 1–100"))
        })
        .transpose()?
        .unwrap_or(20) as usize;
    Ok((offset, limit))
}
pub(super) fn read(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    view: WorkflowReadingView,
    p: &Value,
) -> Result<Value> {
    if p["project_id"].as_str().is_some_and(|id| id != project) {
        return Err(err("scope_violation", "流程读取跨项目"));
    }
    let owner = identifier(p, "owner_id")?;
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err("catalog_refresh_required", "流程读取需要更新源目录"));
    }
    let found: bool = conn
        .query_row(
            &format!("{SCOPE} SELECT EXISTS(SELECT 1 FROM owners WHERE id=?1)"),
            params![owner, project],
            |r| r.get(0),
        )
        .map_err(storage)?;
    require(found, "工作归属不属于当前项目")?;
    let value = match view {
        WorkflowReadingView::Materials => materials(&conn, catalog, project, &owner, p)?,
        WorkflowReadingView::Executions => executions(&conn, catalog, project, &owner, p)?,
        WorkflowReadingView::Record => read_record(&conn, catalog, project, &owner, p)?,
        WorkflowReadingView::Contexts => contexts(&conn, catalog, project, &owner, p)?,
    };
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(value)
}

fn public_record(row: &ConfigurationRecord) -> Value {
    if matches!(row.r#type.as_str(), "platform_call" | "tool_call") {
        json!({"id":row.id,"type":row.r#type,"name":row.name,"created_at":row.created_at,"data":{"status":row.data["status"],"receipt":row.data["receipt"],"error":row.data["error"]}})
    } else if row.r#type == "workflow_intent" && row.data["method"] == "conversation.send" {
        let mut value = json!(row);
        value["metadata_only"] = json!(true);
        value["data"]["command"]["payload"]
            .as_object_mut()
            .map(|p| p.remove("input"));
        value
    } else {
        json!(row)
    }
}
const RELATED: &str = "((r.type='workflow_decision' AND json_extract(r.data,'$.instance_id')=e.instance_id AND json_extract(r.data,'$.execution_id')=json_extract(e.execution,'$.id')) OR (r.type IN ('workflow_confirmation','workflow_dispatch','workflow_intent') AND r.definition_id=json_extract(e.execution,'$.id')) OR (r.type IN ('platform_call','tool_call') AND (r.id=json_extract(e.execution,'$.tool_call_id') OR (json_extract(r.data,'$.request.instance_id')=e.instance_id AND json_extract(r.data,'$.request.caller.execution_id')=json_extract(e.execution,'$.id')))))";
fn read_record(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    owner: &str,
    p: &Value,
) -> Result<Value> {
    let id = identifier(p, "record_id")?;
    let sql = format!(
        "{SCOPE} SELECT EXISTS(SELECT 1 FROM governance_records r WHERE r.id=?3 AND r.owner_project=?2 AND ((r.type='workflow_plan_revision' AND json_extract(r.data,'$.instance_id') IN (SELECT id FROM selected)) OR (r.type='workflow_change' AND r.definition_id IN (SELECT id FROM selected)) OR EXISTS(SELECT 1 FROM entries e WHERE {RELATED})))"
    );
    let allowed: bool = conn
        .query_row(&sql, params![owner, project, id], |r| r.get(0))
        .map_err(storage)?;
    require(allowed, "记录不属于当前工作流程")?;
    let raw = record(conn, &id)?;
    // Strip raw platform requests before source decoding: a public receipt
    // never reconstructs private native credentials or execution permissions.
    if matches!(raw.r#type.as_str(), "platform_call" | "tool_call") {
        let visible = source_views::restore(&catalog.state, &public_record(&raw))?;
        return Ok(json!({"representation":"public_execution_receipt","record":visible}));
    }
    let mut raw = raw;
    if raw.r#type == "workflow_intent"
        && let Some(descriptor) =
            source_views::task_descriptor(&raw.data["command"]["payload"]["input"])?
    {
        let task = workflow_intents::task_facts(conn, &raw, descriptor)?;
        raw.data["command"]["payload"]
            .as_object_mut()
            .ok_or_else(|| err("storage", "任务意图缺少正文"))?
            .remove("input");
        let mut raw = restored(catalog, raw)?;
        raw.data["command"]["payload"]["input"] = json!(workflow_intents::restore_task(
            catalog,
            task,
            &mut BTreeMap::new()
        )?);
        raw.content_hash = content_digest(&raw.refs, &raw.data);
        return Ok(json!({"record":raw}));
    }
    Ok(json!({"record":restored(catalog,raw)?}))
}

fn executions(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    owner: &str,
    p: &Value,
) -> Result<Value> {
    let (offset, limit) = page(p)?;
    let instance = optional(p, "instance_id");
    let execution = optional(p, "execution_id");
    let projection = if execution.is_some() {
        "execution"
    } else {
        "json_object('id',json_extract(execution,'$.id'),'node_id',json_extract(execution,'$.node_id'),'plan_revision_id',json_extract(execution,'$.plan_revision_id'),'step',json_extract(execution,'$.step'),'round',json_extract(execution,'$.round'),'status',json_extract(execution,'$.status'),'reason',json_extract(execution,'$.reason'),'started',json_extract(execution,'$.started'),'finished',json_extract(execution,'$.finished'),'session_id',json_extract(execution,'$.session_id'),'turn_id',json_extract(execution,'$.turn_id'),'agent_revision_id',json_extract(execution,'$.agent_revision_id'),'outputs',json_extract(execution,'$.outputs'))"
    };
    let sql = format!(
        "{SCOPE} SELECT instance_id,instance_name,owner_id,owner_title,{projection},historical FROM entries WHERE (?3 IS NULL OR instance_id=?3) AND (?4 IS NULL OR json_extract(execution,'$.id')=?4) ORDER BY historical,ordering,position LIMIT ?5 OFFSET ?6"
    );
    let mut statement = conn.prepare_cached(&sql).map_err(storage)?;
    let mut rows = statement
        .query_map(
            params![owner, project, instance, execution, limit + 1, offset],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            },
        )
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let mut records = Vec::new();
    let mut plans = BTreeMap::new();
    let mut exact = None;
    let mut instance_metadata = BTreeMap::<String, Value>::new();
    for (instance, name, owner, title, raw, historical) in rows {
        let e: Value =
            source_views::restore(&catalog.state, &payload_store::decode_step(conn, &raw)?)?;
        let frozen = step_source(conn, catalog, project, &instance, &e, &mut plans)?;
        let step = &frozen["step"];
        let outputs=e["outputs"].as_object().into_iter().flatten().map(|(port,id)|{
            let reference=step["outputs"][port]["delivery_ref"].as_str().unwrap_or("");
            json!({"id":id,"port":port,"name":catalog.state.configuration(reference).ok().map(|r|&r.name)})
        }).collect::<Vec<_>>();
        if !instance_metadata.contains_key(&instance) {
            let raw:String=conn.query_row("SELECT json_object('parent_execution_id',json_extract(data,'$.parent_execution_id'),'root_instance_id',json_extract(data,'$.root_instance_id')) FROM governance_records WHERE id=?1 AND type='workflow_instance'",[&instance],|r|r.get(0)).map_err(storage)?;
            instance_metadata.insert(
                instance.clone(),
                serde_json::from_str(&raw).map_err(storage)?,
            );
        }
        let header = &instance_metadata[&instance];
        let feature_id: Option<String> = conn.query_row("SELECT feature_id FROM tasks WHERE id=?1 UNION ALL SELECT id FROM features WHERE id=?1 LIMIT 1", [&owner], |r| r.get(0)).optional().map_err(storage)?;
        let metadata = json!({"feature_id":feature_id,"parent_execution_id":header["parent_execution_id"],"root_instance_id":header["root_instance_id"],"context_available":e["status"]!="queued","execution_available":e["status"]!="queued","id":e["id"],"instance_id":instance,"instance_name":name,"owner_id":owner,"owner_title":title,
            "step_name":step["name"],"round":e["round"],"status":e["status"],"reason":e["reason"],"started":e["started"],"finished":e["finished"],"historical":historical,
            "session_id":e["session_id"],"turn_id":e["turn_id"],"actor":actor(conn,catalog,project,&e,step)?,"outputs":outputs});
        if execution.is_some() {
            exact = Some((metadata.clone(), e));
        }
        records.push(metadata);
    }
    if let Some(id) = execution {
        let (metadata, e) = exact.ok_or_else(|| err("not_found", "执行不属于当前工作范围"))?;
        let sql = format!(
            "{SCOPE} SELECT DISTINCT r.id FROM entries e JOIN governance_records r ON r.owner_project=?2 WHERE json_extract(e.execution,'$.id')=?3 AND {RELATED} ORDER BY r.rowid LIMIT 101 OFFSET ?4"
        );
        let fact_offset = p
            .get("fact_offset")
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n <= 1_000_000)
                    .ok_or_else(|| err("invalid_params", "无效的事实分页"))
            })
            .transpose()?
            .unwrap_or(0);
        let mut statement = conn.prepare_cached(&sql).map_err(storage)?;
        let mut ids = statement
            .query_map(params![owner, project, id, fact_offset], |r| {
                r.get::<_, String>(0)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let more = ids.len() > 100;
        ids.truncate(100);
        let facts = ids
            .iter()
            .map(|id| {
                let raw = record(conn, id)?;
                source_views::restore(&catalog.state, &public_record(&raw))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(
            json!({"record":metadata,"execution":e,"facts":facts,"facts_more":more,"next_fact_offset":more.then_some(fact_offset+100)}),
        );
    }
    Ok(
        json!({"records":records,"offset":offset,"limit":limit,"more":more,"next_offset":more.then_some(offset+limit as u64)}),
    )
}

fn materials(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    owner: &str,
    p: &Value,
) -> Result<Value> {
    if let Some(id) = optional(p, "output_id") {
        let sql = format!(
            "{SCOPE} SELECT d.id FROM documents d WHERE d.id=?3 AND coalesce(d.task_id,d.feature_id,d.project_id) IN (SELECT id FROM owners)"
        );
        if let Some(id) = conn
            .query_row(&sql, params![owner, project, id], |r| r.get::<_, String>(0))
            .optional()
            .map_err(storage)?
        {
            let document =
                load_table_filtered_params::<Document>(conn, "documents", "id=?1", &[&id])?
                    .remove(0);
            return Ok(json!({"document":document}));
        }
    }
    let (offset, limit) = page(p)?;
    let output = optional(p, "output_id");
    let execution = optional(p, "execution_id");
    let search = p["search"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>()
        .to_lowercase();
    let cte = format!(
        "{SCOPE}, material_rows AS MATERIALIZED (SELECT e.instance_id,e.instance_name,e.owner_id,e.owner_kind,e.owner_title,e.workflow_ref,e.execution,e.historical,o.id,o.created_at,o.data,ports.key AS port,d.reference AS delivery_ref,d.name AS delivery_name,json_extract(d.data,'$.format') AS format,s.value AS step,st.value AS stage FROM entries e JOIN json_each(e.execution,'$.outputs') ports JOIN governance_records o ON o.id=ports.value AND o.type='workflow_output' AND o.owner_project=?2 AND json_extract(o.data,'$.execution_id')=json_extract(e.execution,'$.id') JOIN governance_records p ON p.id=json_extract(e.execution,'$.plan_revision_id') AND p.type='workflow_plan_revision' AND p.owner_project=?2 AND json_extract(p.data,'$.instance_id')=e.instance_id LEFT JOIN source_definitions w ON w.reference=coalesce(json_extract(p.data,'$.plan.workflow_ref'),json_extract(p.data,'$.plan.workflow_revision_id')) AND w.type='workflow' JOIN json_each(coalesce(json_extract(p.data,'$.plan.document'),w.data),'$.steps') s ON json_extract(s.value,'$.node_id')=json_extract(e.execution,'$.node_id') OR (coalesce(json_extract(s.value,'$.node_id'),'')='' AND json_extract(s.value,'$.key')=json_extract(e.execution,'$.step')) LEFT JOIN json_each(coalesce(json_extract(p.data,'$.plan.document'),w.data),'$.stages') st ON json_extract(st.value,'$.key')=json_extract(s.value,'$.stage') JOIN source_definitions d ON d.reference=json_extract(o.data,'$.delivery_ref') AND d.type='delivery_type' WHERE (?3 IS NULL OR o.id=?3) AND (?4 IS NULL OR json_extract(e.execution,'$.id')=?4) AND (?5='' OR instr(lower(d.name||' '||coalesce(json_extract(s.value,'$.name'),'')||' '||coalesce(json_extract(st.value,'$.name'),'')||' '||e.owner_title||' '||ports.key),?5)>0))"
    );
    let total: u64 = conn
        .query_row(
            &format!("{cte} SELECT count(*) FROM material_rows"),
            params![owner, project, output, execution, search],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let mut statement=conn.prepare_cached(&format!("{cte} SELECT instance_id,instance_name,owner_id,owner_kind,owner_title,workflow_ref,json_object('id',json_extract(execution,'$.id'),'step',json_extract(execution,'$.step'),'path',json_extract(execution,'$.path'),'status',json_extract(execution,'$.status'),'round',json_extract(execution,'$.round'),'plan_revision_id',json_extract(execution,'$.plan_revision_id'),'turn_id',json_extract(execution,'$.turn_id'),'session_id',json_extract(execution,'$.session_id'),'historical',json(CASE historical WHEN 0 THEN 'false' ELSE 'true' END)),id,created_at,port,delivery_name,format,step,stage,delivery_ref FROM material_rows ORDER BY created_at,id LIMIT ?6 OFFSET ?7")).map_err(storage)?;
    let rows=statement.query_map(params![owner,project,output,execution,search,if output.is_some(){1}else{limit},offset],|r|{
        let e:Value=serde_json::from_str(&r.get::<_,String>(6)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(6,rusqlite::types::Type::Text,Box::new(e)))?;
        let step:Value=serde_json::from_str(&r.get::<_,String>(12)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(12,rusqlite::types::Type::Text,Box::new(e)))?;
        let stage:Option<String>=r.get(13)?;let stage=stage.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
        Ok(json!({"instance_id":r.get::<_,String>(0)?,"workflow_name":r.get::<_,String>(1)?,"owner_id":r.get::<_,String>(2)?,"owner_kind":r.get::<_,String>(3)?,"owner_title":r.get::<_,String>(4)?,"workflow_ref":r.get::<_,String>(5)?,"execution_id":e["id"],"turn_id":e["turn_id"],"session_id":e["session_id"],"historical":e["historical"],"status":e["status"],"round":e["round"],"path":e["path"],"step":e["step"],"step_name":step["name"],"stage_name":stage["name"],"plan_revision_id":e["plan_revision_id"],"id":r.get::<_,String>(7)?,"created_at":r.get::<_,u64>(8)?,"port":r.get::<_,String>(9)?,"delivery_ref":r.get::<_,String>(14)?,"delivery_name":r.get::<_,String>(10)?,"format":r.get::<_,Option<String>>(11)?}))
    }).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    let mut rows = rows
        .into_iter()
        .map(|v| source_views::restore(&catalog.state, &v))
        .collect::<Result<Vec<_>>>()?;
    if let Some(id) = output {
        let mut metadata = rows
            .pop()
            .ok_or_else(|| err("not_found", "产物不属于当前工作流程"))?;
        let output = restored(catalog, record(conn, id)?)?;
        let reference = output.data["delivery_ref"]
            .as_str()
            .ok_or_else(|| err("storage", "交付引用缺失"))?;
        let delivery = catalog.state.configuration(reference)?;
        require(delivery.r#type == "delivery_type", "交付源类型无效")?;
        metadata["delivery_ref"] = json!(reference);
        let mut definitions = BTreeMap::new();
        definitions.insert(reference.to_owned(), delivery.clone());
        for spec in delivery
            .refs
            .get("spec_refs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let spec = catalog.state.configuration(spec)?;
            require(spec.r#type == "spec", "冻结内容规范类型无效")?;
            definitions.insert(spec.id.clone(), spec.clone());
        }
        return Ok(json!({"record":metadata,"output":output.data,"definitions":definitions}));
    }
    Ok(json!({"records":rows,"total":total,"offset":offset,"limit":limit}))
}

fn contexts(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    project: &str,
    owner: &str,
    p: &Value,
) -> Result<Value> {
    if let Some(id) = optional(p, "context_id") {
        let sql = format!(
            "{SCOPE} SELECT c.id FROM contexts c WHERE c.id=?3 AND c.owner_id IN (SELECT id FROM owners)"
        );
        let id = conn
            .query_row(&sql, params![owner, project, id], |r| r.get::<_, String>(0))
            .optional()
            .map_err(storage)?
            .ok_or_else(|| err("not_found", "手工背景不属于当前工作范围"))?;
        let context =
            load_table_filtered_params::<Context>(conn, "contexts", "id=?1", &[&id])?.remove(0);
        return Ok(json!({"context":context,"source":"current_manual_context"}));
    }
    let cte = format!(
        "{SCOPE}, scoped_sessions AS MATERIALIZED (SELECT DISTINCT e.instance_id,e.instance_name,e.owner_title,e.execution,e.historical,'step' AS kind,s.id AS session_id,s.owner_id,s.snapshot_id,s.agent,s.runtime_binding FROM entries e JOIN sessions s ON s.project_id=?2 AND s.scope IS NOT NULL AND (s.id=json_extract(e.execution,'$.session_id') OR EXISTS(SELECT 1 FROM governance_records b WHERE b.type='configured_session' AND b.definition_id=s.id AND b.owner_project=?2 AND json_extract(b.data,'$.execution_id')=json_extract(e.execution,'$.id'))) WHERE (?3 IS NULL OR e.instance_id=?3) UNION ALL SELECT json_extract(c.data,'$.root_instance_id'),'编排控制','',json_object('id',c.id),0,'controller',s.id,s.owner_id,s.snapshot_id,s.agent,s.runtime_binding FROM governance_records c JOIN sessions s ON s.id=json_extract(c.data,'$.session_id') AND s.project_id=?2 WHERE c.type='workflow_controller' AND c.owner_project=?2 AND json_extract(c.data,'$.root_instance_id') IN (SELECT id FROM selected) AND (?3 IS NULL OR json_extract(c.data,'$.root_instance_id')=?3))"
    );
    if let Some(snapshot) = optional(p, "snapshot_id") {
        let session = identifier(p, "session_id")?;
        let allowed:bool=conn.query_row(&format!("{cte} SELECT EXISTS(SELECT 1 FROM scoped_sessions WHERE session_id=?4 AND snapshot_id=?5)"),params![owner,project,optional(p, "instance_id"),session,snapshot],|r|r.get(0)).map_err(storage)?;
        require(allowed, "快照不属于当前 Agent 上下文")?;
        let value = resource_commits::read_on(conn, &[ResourceKey::new("snapshot", snapshot)])?
            .remove(0)
            .value
            .ok_or_else(|| err("not_found", "原会话快照缺失"))?;
        return Ok(json!({"snapshot":source_views::restore(&catalog.state,&value)?}));
    }
    let (offset, limit) = page(p)?;
    let sql = format!(
        "{cte} SELECT x.instance_id,x.instance_name,x.owner_title,json_object('id',json_extract(x.execution,'$.id'),'node_id',json_extract(x.execution,'$.node_id'),'plan_revision_id',json_extract(x.execution,'$.plan_revision_id'),'step',json_extract(x.execution,'$.step'),'turn_id',json_extract(x.execution,'$.turn_id'),'round',json_extract(x.execution,'$.round')),x.kind,x.session_id,x.owner_id,x.snapshot_id,x.agent,coalesce(json_extract(x.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(x.runtime_binding,'$.agent_configuration.agent_instance_id')),t.id,t.status,t.started,EXISTS(SELECT 1 FROM governance_records q WHERE q.type IN ('configured_execution_task','archived_configured_execution_task') AND q.definition_id=t.id AND q.owner_project=?2),EXISTS(SELECT 1 FROM governance_records q WHERE q.type='archived_configured_execution_task' AND q.definition_id=t.id AND q.owner_project=?2),t.id=(SELECT n.id FROM turns n WHERE n.session_id=x.session_id ORDER BY n.rowid DESC LIMIT 1),x.historical FROM scoped_sessions x JOIN turns t ON t.session_id=x.session_id WHERE (?8 IS NULL OR t.id=?8) AND (?9 IS NULL OR json_extract(x.execution,'$.id')=?9) AND (?4 IS NULL OR coalesce(json_extract(x.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(x.runtime_binding,'$.agent_configuration.agent_instance_id'),'session:'||x.session_id)=?4) AND (?5 IS NULL OR x.session_id=?5) ORDER BY t.started DESC,t.id LIMIT ?6 OFFSET ?7"
    );
    let mut statement = conn.prepare_cached(&sql).map_err(storage)?;
    let mut rows=statement.query_map(params![owner,project,optional(p, "instance_id"),optional(p, "agent_instance_id"),optional(p, "session_id"),limit+1,offset,optional(p, "turn_id"),optional(p, "execution_id")],|r|{
        let e:Value=serde_json::from_str(&r.get::<_,String>(3)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(3,rusqlite::types::Type::Text,Box::new(e)))?;
        let kind:String=r.get(4)?;let turn:String=r.get(10)?;let latest:bool=r.get(15)?;
        Ok((r.get::<_,String>(8)?,r.get::<_,Option<String>>(9)?,json!({"kind":kind,"instance_id":r.get::<_,String>(0)?,"instance_name":r.get::<_,String>(1)?,"owner_title":r.get::<_,String>(2)?,"execution_id":e["id"],"step":e["step"],"node_id":e["node_id"],"plan_revision_id":e["plan_revision_id"],"round":e["round"].as_u64().unwrap_or(0),"session_id":r.get::<_,String>(5)?,"owner_id":r.get::<_,String>(6)?,"snapshot_id":r.get::<_,Option<String>>(7)?,"turn_id":turn,"status":r.get::<_,String>(11)?,"started":r.get::<_,Option<u64>>(12)?,"task_available":r.get::<_,bool>(13)?,"archived":r.get::<_,bool>(14)?,"historical":if kind=="controller"{!latest}else{r.get::<_,bool>(16)? || e["turn_id"]!=turn}})))
    }).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let mut groups = BTreeMap::<String, Value>::new();
    let mut plans = BTreeMap::new();
    for (reference, instance, mut entry) in rows {
        let agent = catalog.state.configuration(&reference)?;
        let actor = json!({"agent_revision_id":reference,"agent_instance_id":instance,"name":agent.name,"role_ref":agent.refs.get("role_ref"),"session_id":entry["session_id"]});
        let key = instance
            .unwrap_or_else(|| format!("session:{}", entry["session_id"].as_str().unwrap_or("")));
        if entry["kind"] == "step" {
            let frozen = step_source(
                conn,
                catalog,
                project,
                entry["instance_id"].as_str().unwrap(),
                &entry,
                &mut plans,
            )?;
            entry["step_name"] = frozen["step"]["name"].clone();
        } else {
            entry["step_name"] = json!("编排控制");
        }
        entry["actor"] = actor.clone();
        let group = groups
            .entry(key.clone())
            .or_insert_with(|| json!({"id":key,"actor":actor,"entries":[]}));
        group["entries"].as_array_mut().unwrap().push(entry);
    }
    Ok(
        json!({"agents":groups.into_values().collect::<Vec<_>>(),"offset":offset,"limit":limit,"more":more,"next_offset":more.then_some(offset+limit as u64)}),
    )
}
