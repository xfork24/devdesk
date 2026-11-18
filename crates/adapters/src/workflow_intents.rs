//! Bounded native intent scan and exact task-source decoding. No runtime
//! aggregate is assembled for business validation or scheduling decisions.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{WorkflowIntentFacts, WorkflowIntentPage, concurrency::*, frozen_views::*};
use rusqlite::{OptionalExtension, params};

pub(super) fn page(pool: &ConnectionPool, after: Option<&str>) -> Result<WorkflowIntentPage> {
    let _span = performance::Span::new("workflow_intents.page");
    let conn = pool.read()?;
    let mut query=conn.prepare_cached("SELECT id FROM governance_records INDEXED BY idx_workflow_intent_prepared WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared' AND id>?1 ORDER BY id LIMIT 17").map_err(storage)?;
    let mut ids = query
        .query_map([after.unwrap_or("")], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let more = ids.len() > 16;
    ids.truncate(16);
    Ok(WorkflowIntentPage {
        next_after: more.then(|| ids.last().unwrap().clone()),
        ids,
    })
}
fn read(
    conn: &Connection,
    id: &str,
    project: Option<&str>,
) -> Result<(ResourceSnapshot, ConfigurationRecord)> {
    let mut rows = resource_commits::read_on(conn, &[ResourceKey::new("governance", id)])?;
    let row = rows.remove(0);
    let record = ConfigurationRecord::from_storage(
        row.value
            .clone()
            .ok_or_else(|| err("not_found", "工作流意图缺失"))?,
    )?;
    require(
        record.r#type == "workflow_intent"
            && record.scope.project_id().is_some()
            && project.is_none_or(|p| record.scope.project_id() == Some(p)),
        "工作流意图不属于原项目",
    )?;
    Ok((row, record))
}
fn eligible(conn: &Connection, row: &ConfigurationRecord) -> Result<bool> {
    if row.data["status"] != "prepared" {
        return Ok(false);
    }
    let project = row
        .scope
        .project_id()
        .ok_or_else(|| err("invalid_scope", "工作流意图缺少项目"))?;
    if row.data["controller"] == true {
        let controller=conn.query_row("SELECT json_extract(data,'$.status'),json_extract(data,'$.epoch'),json_extract(data,'$.owner_id') FROM governance_records WHERE type='workflow_controller' AND id=?1 AND owner_project=?2 AND json_extract(data,'$.project_id')=?2",params![row.definition_id,project],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?,r.get::<_,String>(2)?))).optional().map_err(storage)?;
        return Ok(controller.is_some_and(|(status, epoch, owner)| {
            status == "active"
                && row.data["command"]["payload"]["controller_epoch"] == epoch
                && row.data["owner_id"] == owner
        }));
    }
    let authority: devdesk_core::tool_calls::WorkflowToolAuthority =
        serde_json::from_value(row.data["execution_authority"].clone()).map_err(storage)?;
    require(
        authority.execution_id == row.definition_id,
        "工作流意图原步骤不符",
    )?;
    let facts = tool_calls::workflow_on(conn, &authority, project)?;
    if !facts.allowed {
        return Ok(false);
    }
    let owner = row.data["owner_id"]
        .as_str()
        .ok_or_else(|| err("storage", "工作流意图缺少原工作身份"))?;
    let paused=conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND owner_project=?1 AND definition_id=?2 AND json_extract(data,'$.paused')=1)",params![project,owner],|r|r.get::<_,bool>(0)).map_err(storage)?;
    Ok(!paused)
}
pub(super) fn active(
    pool: &ConnectionPool,
    id: &str,
    revision: Option<u64>,
    project: Option<&str>,
) -> Result<bool> {
    let _span = performance::Span::new("workflow_intents.active");
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let (_, row) = read(&tx, id, project)?;
    let result = revision.is_none_or(|r| r == row.revision) && eligible(&tx, &row)?;
    tx.commit().map_err(storage)?;
    Ok(result)
}
pub(super) struct TaskFacts {
    descriptor: Value,
    instance: ConfigurationRecord,
    plan: ConfigurationRecord,
    history: Option<StepExecution>,
    delivered: Option<(String, String)>,
}
pub(super) fn task_facts(
    conn: &Connection,
    row: &ConfigurationRecord,
    descriptor: Value,
) -> Result<TaskFacts> {
    let command = &row.data["command"];
    let delivered=conn.query_row("SELECT e.content,json_extract(g.data,'$.digest') FROM turns t JOIN sessions s ON s.id=t.session_id JOIN conversation_events e ON e.turn_id=t.id AND e.session_id=s.id AND e.kind='user' JOIN governance_records g ON g.definition_id=t.id AND g.type IN ('configured_execution_task','archived_configured_execution_task') WHERE t.session_id=?1 AND t.client_message_id=?2 AND s.project_id=?3 ORDER BY e.rowid LIMIT 1",params![command["target"].as_str().unwrap_or(""),command["payload"]["client_message_id"].as_str().unwrap_or(""),row.scope.project_id()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(storage)?;
    task_source_facts(conn, &row.scope, &row.definition_id, descriptor, delivered)
}
pub(super) fn task_source_facts(
    conn: &Connection,
    scope: &Scope,
    execution: &str,
    descriptor: Value,
    delivered: Option<(String, String)>,
) -> Result<TaskFacts> {
    let source = &descriptor["source"];
    let instance_id = source["workflow_instance_id"]
        .as_str()
        .ok_or_else(|| err("storage", "任务缺少原实例"))?;
    let plan_id = source["plan_revision_id"]
        .as_str()
        .ok_or_else(|| err("storage", "任务缺少原计划"))?;
    let rows = resource_commits::read_on(
        conn,
        &[
            ResourceKey::new("governance", instance_id),
            ResourceKey::new("governance", plan_id),
        ],
    )?;
    let instance = ConfigurationRecord::from_storage(
        rows[0]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "任务原实例缺失"))?,
    )?;
    let plan = ConfigurationRecord::from_storage(
        rows[1]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "任务原计划缺失"))?,
    )?;
    require(
        instance.r#type == "workflow_instance"
            && instance.scope == *scope
            && plan.scope == *scope
            && plan.r#type == "workflow_plan_revision"
            && plan.data["instance_id"] == instance.id
            && source["execution_id"] == execution,
        "任务原事实类型或范围不符",
    )?;
    let execution = source["execution_id"]
        .as_str()
        .ok_or_else(|| err("storage", "任务缺少原步骤"))?;
    let history = if instance.data["steps"]
        .as_array()
        .is_some_and(|s| s.iter().any(|s| s["id"] == execution))
    {
        None
    } else {
        load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_execution_history' AND definition_id=?1 AND json_extract(data,'$.workflow_instance_id')=?2 ORDER BY rowid LIMIT 1)",&[&execution,&instance_id])?.into_iter().next().map(|v|{
            let history=ConfigurationRecord::from_storage(v)?;
            require(history.scope==*scope,"任务历史跨项目")?;
            history.decode()
        }).transpose()?
    };
    Ok(TaskFacts {
        descriptor,
        instance,
        plan,
        history,
        delivered,
    })
}
pub(super) fn restore_task(
    catalog: &ConfigurationCatalogSnapshot,
    facts: TaskFacts,
    memo: &mut std::collections::BTreeMap<String, Value>,
) -> Result<String> {
    let TaskFacts {
        descriptor,
        instance,
        plan,
        history,
        delivered,
    } = facts;
    if let Some((body, digest)) = delivered {
        let task: Value = serde_json::from_str(&body).map_err(storage)?;
        if configuration_hash(&task) == digest
            && descriptor["task_digest"] == digest
            && task["source"] == descriptor["source"]
            && task["inputs"] == descriptor["inputs"]
            && task["input_sources"] == descriptor["input_sources"]
            && task["agent"]["id"] == descriptor["agent_ref"]
            && task["supplemental_input"] == descriptor["supplemental_input"]
        {
            performance::record("workflow_intents.delivered_task", 0.0);
            return Ok(body);
        }
    }
    performance::record("workflow_intents.rebuild_task", 0.0);
    let compiled = source_views::restore_with(&catalog.state, &plan.data["plan"], memo)?;
    let mut data = instance.data;
    data.as_object_mut()
        .ok_or_else(|| err("storage", "任务实例正文无效"))?
        .remove("plan");
    let data = source_views::restore_with(&catalog.state, &data, memo)?;
    let task = restore_task_source(
        &descriptor,
        compiled,
        FrozenTaskSource {
            instance_data: &data,
            plan_record: &plan,
            historical_execution: history.as_ref(),
        },
    )?;
    require(
        descriptor["task_digest"]
            .as_str()
            .is_none_or(|digest| configuration_hash(&task) == digest),
        "原任务源重建摘要不符",
    )?;
    Ok(task.to_string())
}
pub(super) fn facts(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    id: &str,
    runnable_only: bool,
    project: Option<&str>,
) -> Result<Option<WorkflowIntentFacts>> {
    let _span = performance::Span::new("workflow_intents.facts");
    let (row, mut record, active, task) = {
        let conn = pool.read()?;
        let tx = conn.unchecked_transaction().map_err(storage)?;
        let (row, record) = read(&tx, id, project)?;
        let active = eligible(&tx, &record)?;
        if runnable_only && !active {
            tx.commit().map_err(storage)?;
            return Ok(None);
        }
        let task = source_views::task_descriptor(&record.data["command"]["payload"]["input"])?
            .map(|d| task_facts(&tx, &record, d))
            .transpose()?;
        tx.commit().map_err(storage)?;
        (row, record, active, task)
    };
    // No SQL handle or business mutex is held during frozen source compilation.
    let mut memo = Default::default();
    if task.is_some() {
        record.data["command"]["payload"]
            .as_object_mut()
            .ok_or_else(|| err("storage", "任务命令正文无效"))?
            .remove("input");
    }
    record.data = source_views::restore_with(&catalog.state, &record.data, &mut memo)?;
    if let Some(task) = task {
        record.data["command"]["payload"]["input"] = json!(restore_task(catalog, task, &mut memo)?);
    }
    record.content_hash = content_digest(&record.refs, &record.data);
    Ok(Some(WorkflowIntentFacts {
        row,
        record,
        active,
    }))
}
