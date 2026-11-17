//! Legacy workspace shape over bounded committed rows. Source and runtime
//! history bodies are loaded only by their explicit readers.
use crate::*;
use devdesk_core::read_models::ExecutionWorkspaceQuery;
use rusqlite::params;
pub(super) fn read(store: &SqliteStore, p: &Value) -> Result<Value> {
    let _span = performance::Span::new("read_model.workspace_metadata");
    let project = store
        .project_id
        .as_deref()
        .ok_or_else(|| err("project_required", "工作台需要明确项目"))?;
    let mut selected = p.clone();
    for key in ["detail_pages", "task_id", "project_document_offset"] {
        if let Some(fields) = selected.as_object_mut() {
            fields.remove(key);
        }
    }
    if let Some(offset) = p["detail_pages"]["tasks"].as_u64() {
        selected["task_offset"] = json!(offset * 50);
    }
    let query = ExecutionWorkspaceQuery::parse(&selected)?;
    let mut result = json!(execution_read::workspace(
        &store.connection,
        project,
        &query
    )?);
    result["kind"] = json!("workspace_metadata");
    let conn = store.connection.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if let Some(feature) = &query.feature_id {
        let actual = load_table_filtered_params::<Value>(
            &conn,
            "features",
            "id=?1 AND project_id=?2",
            &[&feature, &project],
        )?
        .pop()
        .ok_or_else(|| err("not_found", "Feature 不属于当前项目"))?;
        result["detail"]["feature"] = actual;
        let tasks = load_table_filtered_params::<Value>(
            &conn,
            "tasks",
            "id IN (SELECT id FROM tasks WHERE feature_id=?1 ORDER BY rowid LIMIT ?2 OFFSET ?3)",
            &[&feature, &query.limit, &query.task_offset],
        )?;
        result["detail"]["tasks"] = json!(tasks);
        for (kind, key) in [
            ("documents", "documents"),
            ("snapshots", "snapshots"),
            ("executions", "executions"),
        ] {
            let offset = p["detail_pages"][key].as_u64().unwrap_or(0).min(20_000) * query.limit;
            let fields = match kind {
                "documents" => {
                    "json_object('id',id,'title',title,'kind',kind,'version',version,'supersedes',supersedes,'artifact_id',artifact_id,'feature_id',feature_id,'task_id',task_id,'metadata_only',json('true'))"
                }
                "snapshots" => {
                    "json_object('id',id,'kind',kind,'feature_id',feature_id,'task_id',task_id,'metadata_only',json('true'))"
                }
                _ => {
                    "json_object('id',id,'session_id',session_id,'status',status,'kind',kind,'metadata_only',json('true'))"
                }
            };
            let predicate = if kind == "executions" {
                "session_id IN (SELECT id FROM sessions WHERE feature_id=?1)"
            } else {
                "feature_id=?1"
            };
            let sql = format!(
                "SELECT {fields} FROM {kind} WHERE {predicate} ORDER BY rowid DESC LIMIT ?2 OFFSET ?3"
            );
            let mut q = conn.prepare_cached(&sql).map_err(storage)?;
            let rows = q
                .query_map(params![feature, query.limit, offset], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?
                .into_iter()
                .map(|v| serde_json::from_str::<Value>(&v).map_err(storage))
                .collect::<Result<Vec<_>>>()?;
            result["detail"][key] = json!(rows);
        }
        let mut q=conn.prepare_cached("SELECT json_object('id',id,'owner_id',owner_id,'revision',revision,'metadata_only',json('true')) FROM contexts WHERE owner_id=?1 OR owner_id IN (SELECT id FROM tasks WHERE feature_id=?1) ORDER BY rowid LIMIT ?2").map_err(storage)?;
        let rows = q
            .query_map(params![feature, query.limit], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
            .into_iter()
            .map(|v| serde_json::from_str::<Value>(&v).map_err(storage))
            .collect::<Result<Vec<_>>>()?;
        result["detail"]["contexts"] = json!(rows);
    }
    let (features,tasks,running,waiting,blocked):(u64,u64,u64,u64,u64)=conn.query_row("SELECT (SELECT count(*) FROM features),(SELECT count(*) FROM tasks),coalesce(sum(json_extract(data,'$.status')='running'),0),coalesce(sum(json_extract(data,'$.status')='waiting_confirmation'),0),coalesce(sum(json_extract(data,'$.status') IN ('failed','needs_reconciliation','waiting_input')),0) FROM workflow_display_steps",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(storage)?;
    result["summary"] = json!({"features":features,"tasks":tasks,"running":running,"waiting_confirmation":waiting,"blocked":blocked});
    for (kind, key) in [
        ("delivery_type", "delivery_types"),
        ("agent", "definitions"),
    ] {
        let mut rows = conn.prepare_cached("SELECT s.reference,s.name,s.revision,r.identity FROM source_definitions s LEFT JOIN source_definitions r ON r.reference=json_extract(s.refs,'$.role_ref') WHERE s.type=?1 AND NOT EXISTS(SELECT 1 FROM source_definitions n WHERE n.identity=s.identity AND n.revision>s.revision) ORDER BY s.name,s.reference LIMIT 100").map_err(storage)?;
        let values = rows.query_map([kind],|row| {
            Ok(if kind=="agent" {
                json!({"id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"version":row.get::<_,u64>(2)?,"family":"agent","metadata_only":true,"agent_profile":{"role":row.get::<_,Option<String>>(3)?}})
            } else {
                json!({"id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"type":kind,"metadata_only":true})
            })
        }).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        result[key] = json!(values);
    }
    use rusqlite::OptionalExtension;
    let saved = conn.query_row("SELECT reference FROM source_definitions WHERE type='project_configuration' AND scope=?1 ORDER BY revision DESC LIMIT 1",[serde_json::to_string(&Scope::project(project)).map_err(storage)?],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    // Source publication validates the exact immutable graph. This metadata
    // reports the saved selection; explicit configuration readers can recheck
    // current defaults without making ordinary workspace reads compile plans.
    result["configuration_ready"] = json!(saved.is_some());
    result["configuration_status"] = json!({"ready":saved.is_some(),"reference":saved,"validation":"saved_configuration","issues":if saved.is_some(){vec![]}else{vec!["请选择 Workflow 并配置角色 Agent"]}});
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    result["projects"] = json!([result["active_project"]]);
    result["total_projects"] = json!(1);
    result["data_root"] = json!(store.root());
    result["execution_mode"] = json!("live");
    result["harnesses"] = json!([]);
    result["sandboxes"] = json!([]);
    Ok(result)
}
