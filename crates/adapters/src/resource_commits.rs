//! Finite resource reads and atomic optimistic commits over existing fact tables.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::concurrency::*;
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::time::{Duration, Instant};

fn table(kind: &str) -> Result<&'static str> {
    match kind {
        "project" => Ok("projects"),
        "feature" => Ok("features"),
        "task" => Ok("tasks"),
        "context" => Ok("contexts"),
        "document" => Ok("documents"),
        "session" => Ok("sessions"),
        "turn" => Ok("turns"),
        "interaction" => Ok("interactions"),
        "conversation_event" => Ok("conversation_events"),
        "proposal" => Ok("proposals"),
        "execution" => Ok("executions"),
        "repository_operation" => Ok("repository_operations"),
        "orchestration_lease" => Ok("orchestration_leases"),
        "orchestration_wakeup" => Ok("orchestration_wakeups"),
        "repository_worktree" => Ok("repository_worktrees"),
        "snapshot" => Ok("snapshots"),
        "artifact" => Ok("artifacts"),
        "head" => Ok("heads"),
        "governance" => Ok("governance_records"),
        "runtime_installation" => Ok("runtime_installations"),
        "project_directory" => Ok("project_catalog"),
        _ => Err(err("invalid_params", "不支持的资源类型")),
    }
}

fn owner_documents(owner: &MaterialOwner) -> (&'static str, Vec<SqlValue>) {
    match owner {
        MaterialOwner::Global => (
            "project_id IS NULL AND feature_id IS NULL AND task_id IS NULL",
            vec![],
        ),
        MaterialOwner::Project { id } => (
            "project_id=?1 AND feature_id IS NULL AND task_id IS NULL",
            vec![SqlValue::Text(id.clone())],
        ),
        MaterialOwner::Feature { id } => (
            "project_id IS NULL AND feature_id=?1 AND task_id IS NULL",
            vec![SqlValue::Text(id.clone())],
        ),
        MaterialOwner::Task { feature, id } => (
            "project_id IS NULL AND feature_id=?1 AND (task_id IS NULL OR task_id=?2)",
            vec![SqlValue::Text(feature.clone()), SqlValue::Text(id.clone())],
        ),
    }
}

fn value(conn: &Connection, key: &ResourceKey) -> Result<Option<Value>> {
    key.validate()?;
    match key.kind.as_str() {
        "selected_record"=>{
            let (kind,id,selected):(String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            require(kind!="selected_record","选择见证不能递归")?;
            Ok(value(conn,&ResourceKey::new(kind,id))?.filter(|v|v["id"]==selected))
        }
        // Collection tokens have no entity payload. Their version is maintained
        // by fact-table triggers and is read atomically with the selected facts.
        "collection" => Ok(None),
        "catalog_clock" | "configuration_reset" => Ok(None),
        "event_entry"=>Ok(None),
        "workflow_step"|"workflow_step_replace"|"workflow_step_restore"|"workflow_step_delete"|"workflow_step_create"=>{
            let (instance,execution):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT s.value FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.id=?1 AND i.type='workflow_instance' AND json_extract(s.value,'$.id')=?2",rusqlite::params![instance,execution],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose()
        }
        "workflow_header"=>conn.query_row("SELECT json_remove(data,'$.steps') FROM governance_records WHERE id=?1 AND type='workflow_instance'",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "workflow_execution_owner"=>{
            let mut q=conn.prepare_cached("SELECT json_extract(resource_id,'$[0]') FROM resource_versions WHERE kind='workflow_step_authority' AND json_extract(resource_id,'$[1]')=?1 LIMIT 2").map_err(storage)?;
            let ids=q.query_map([&key.id],|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
            require(ids.len()<=1,"步骤执行归属存在歧义")?;Ok(ids.first().map(|id|json!({"instance_id":id})))
        }
        "workflow_dispatch_source"=>{
            let (execution,plan):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_dispatch' AND definition_id=?1 AND json_extract(data,'$.plan_revision_id')=?2 ORDER BY rowid DESC LIMIT 1)",&[&execution,&plan])?.into_iter().next())
        }
        "permission_transition"=>{
            let (session,client):(String,Option<String>)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='permission_switch' AND definition_id=?1 AND ((?2 IS NULL AND json_extract(data,'$.status')!='adopted') OR (?2 IS NOT NULL AND json_extract(data,'$.status')='adopted' AND json_extract(data,'$.command.payload.client_message_id')=?2)) ORDER BY rowid DESC LIMIT 1)",&[&session,&client])?.into_iter().next())
        }
        "repository_pending_page"|"tasks_need_worktree_page"=>{
            let (after,):(String,)=serde_json::from_str(&key.id).map_err(storage)?;let sql=if key.kind=="repository_pending_page"{"SELECT id FROM repository_operations WHERE id>?1 AND (status IN ('prepared','dispatched','observed') OR (status='needs_reconciliation' AND reason='reconcile_requested')) ORDER BY id LIMIT 16"}else{"SELECT t.id FROM tasks t WHERE t.id>?1 AND t.worktree='' AND t.status!='completed' AND NOT EXISTS(SELECT 1 FROM repository_operations o WHERE json_extract(o.action,'$.kind')='create_worktree' AND json_extract(o.action,'$.owner')=t.id) ORDER BY t.id LIMIT 16"};
            let mut q=conn.prepare_cached(sql).map_err(storage)?;let ids=q.query_map([&after],|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;Ok(Some(json!(ids)))
        }
        "repository_worktree_operation"=>Ok(load_table_filtered_params::<Value>(conn,"repository_operations","id=(SELECT id FROM repository_operations WHERE json_extract(action,'$.kind')='create_worktree' AND json_extract(action,'$.owner')=?1 ORDER BY id LIMIT 1)",&[&key.id])?.into_iter().next()),
        "repository_project_state"=>conn.query_row("SELECT json_object('connect_pending',json(CASE WHEN EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND json_extract(action,'$.kind')='connect' AND status!='completed') THEN 'true' ELSE 'false' END),'connect_exists',json(CASE WHEN EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND json_extract(action,'$.kind')='connect') THEN 'true' ELSE 'false' END),'pending',json(CASE WHEN EXISTS(SELECT 1 FROM repository_operations WHERE project_id=?1 AND status!='completed') THEN 'true' ELSE 'false' END),'git_pending',json(CASE WHEN EXISTS(SELECT 1 FROM governance_records WHERE type='platform_call' AND owner_project=?1 AND json_extract(data,'$.handler') LIKE 'git.%' AND json_extract(data,'$.status') IN ('prepared','running','needs_reconciliation')) THEN 'true' ELSE 'false' END),'worktree_exists',json(CASE WHEN EXISTS(SELECT 1 FROM repository_worktrees WHERE project_id=?1) THEN 'true' ELSE 'false' END))",[&key.id],|r|r.get::<_,String>(0)).map_err(storage).and_then(|s|serde_json::from_str(&s).map(Some).map_err(storage)),
        "project_features_empty"=>conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM features WHERE project_id=?1)",[&key.id],|r|r.get::<_,bool>(0)).map(|empty|Some(json!({"empty":empty}))).map_err(storage),
        "controller_latest_intent"=>{
            let (controller,epoch,method):(String,u64,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_intent' AND definition_id=?1 AND json_extract(data,'$.controller')=1 AND json_extract(data,'$.command.payload.controller_epoch')=?2 AND json_extract(data,'$.method')=?3 ORDER BY rowid DESC LIMIT 1)",&[&controller,&epoch,&method])?.into_iter().next())
        }
        "controller_pending_inputs"=>{
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='controller_input' AND definition_id=?1 AND json_extract(data,'$.consumed')=0 ORDER BY rowid LIMIT 8)",&[&key.id])?;
            require(rows.iter().map(|r|r["data"]["input"].as_str().unwrap_or("").len()).sum::<usize>()<=65536,"原经理输入页超过预算")?;Ok(Some(json!(rows)))
        }
        "controller_material_sources" => {
            let (controller,epoch,session):(String,u64,String)=serde_json::from_str(&key.id).map_err(storage)?;
            let mut q=conn.prepare_cached("SELECT c.id,source.value,coalesce(json_extract(c.data,'$.arguments.pointer'),'') FROM (SELECT id,data FROM governance_records WHERE type='controller_tool_call' AND definition_id=?1 AND json_extract(data,'$.epoch')=?2 AND json_extract(data,'$.session_id')=?3 AND json_extract(data,'$.method')='workflow.outputs.read' AND json_extract(data,'$.status')='succeeded' ORDER BY rowid DESC LIMIT 32) c,json_each(c.data,'$.arguments.ids') source LIMIT 128").map_err(storage)?;
            let rows=q.query_map(rusqlite::params![controller,epoch,session],|r|Ok(json!({"call_id":r.get::<_,String>(0)?,"source_id":r.get::<_,String>(1)?,"pointer":r.get::<_,String>(2)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
            Ok(Some(json!(rows)))
        }
        "workflow_material_inputs" => Ok(Some(json!(load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='workflow_material_input' AND definition_id=?1 ORDER BY rowid DESC LIMIT 64)",&[&key.id])?))),
        "workflow_step_decisions"=>{
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='workflow_pending_decision' AND json_extract(data,'$.status')='pending' AND json_extract(data,'$.request.arguments.execution_id')=?1 ORDER BY rowid LIMIT 33)",&[&key.id])?;
            require(rows.len()<=32,"单个执行待决定项超过预算")?;Ok(Some(json!(rows)))
        }
        "controller_interaction_page" => {
            let mut notices = load_table_filtered_params::<Value>(
                conn,
                "governance_records",
                "id IN (SELECT id FROM governance_records WHERE type='controller_interaction' AND definition_id=?1 ORDER BY rowid DESC LIMIT 20)",
                &[&key.id],
            )?;
            for notice in &mut notices {
                // User messages belong to the notice preceding them in this
                // controller's journal, up to its next notice. Consumption
                // updates preserve rowid, so replies survive manager turns.
                let notice_id = notice["id"].as_str().unwrap_or_default();
                let replies = load_table_filtered_params::<Value>(
                    conn,
                    "governance_records",
                    "id IN (SELECT reply.id FROM governance_records reply WHERE reply.type='controller_input' AND reply.definition_id=?1 AND json_extract(reply.data,'$.actor.kind')='human' AND reply.rowid>(SELECT rowid FROM governance_records WHERE id=?2) AND reply.rowid<coalesce((SELECT min(rowid) FROM governance_records WHERE type='controller_interaction' AND definition_id=?1 AND rowid>(SELECT rowid FROM governance_records WHERE id=?2)),9223372036854775807) ORDER BY reply.rowid DESC LIMIT 20)",
                    &[&key.id, &notice_id],
                )?;
                let mut feedback = replies.iter().map(|reply| json!({
                    "id": reply["id"],
                    "input": reply["data"]["input"],
                })).collect::<Vec<_>>();
                let controller_inputs = feedback.iter().filter_map(|reply| reply["input"].as_str().map(str::to_owned)).collect::<std::collections::BTreeSet<_>>();
                let references = ["materials", "evidence"].into_iter().flat_map(|field| notice["data"]["interaction"][field].as_array().into_iter().flatten()).filter_map(|reference| Some((reference["instance_id"].as_str()?.to_owned(), reference["execution_id"].as_str()?.to_owned()))).collect::<std::collections::BTreeSet<_>>();
                for (instance, execution) in references {
                    let inputs = load_table_filtered_params::<Value>(
                        conn,
                        "governance_records",
                        "id IN (SELECT reply.id FROM governance_records reply WHERE reply.type='workflow_intent' AND reply.definition_id=?1 AND json_extract(reply.data,'$.method')='conversation.send' AND json_extract(reply.data,'$.configured')=1 AND NOT EXISTS(SELECT 1 FROM governance_records material WHERE material.type='workflow_material_input' AND material.definition_id=?1 AND json_extract(reply.data,'$.command.payload.client_message_id')=material.definition_id||':'||json_extract(material.data,'$.sequence')) AND reply.rowid>(SELECT rowid FROM governance_records WHERE id=?2) AND EXISTS(SELECT 1 FROM governance_records decision WHERE decision.type='workflow_decision' AND json_extract(decision.data,'$.operation')='input' AND json_extract(decision.data,'$.execution_id')=?1 AND json_extract(decision.data,'$.actor.kind') IN ('human','controller') AND decision.rowid>reply.rowid) AND reply.rowid<coalesce((SELECT min(later.rowid) FROM governance_records later WHERE later.type='controller_interaction' AND later.definition_id=?3 AND later.rowid>(SELECT rowid FROM governance_records WHERE id=?2) AND (EXISTS(SELECT 1 FROM json_each(later.data,'$.interaction.evidence') reference WHERE json_extract(reference.value,'$.execution_id')=?1) OR EXISTS(SELECT 1 FROM json_each(later.data,'$.interaction.materials') reference WHERE json_extract(reference.value,'$.execution_id')=?1))),9223372036854775807) ORDER BY reply.rowid DESC LIMIT 20)",
                        &[&execution, &notice_id, &key.id],
                    )?;
                    for input in inputs {
                        let raw = &input["data"]["command"]["payload"]["input"];
                        let task = match source_views::task_descriptor(raw)? {
                            Some(task) => task,
                            None => match raw.as_str().and_then(|text| serde_json::from_str::<Value>(text).ok()) {
                                Some(task) => task,
                                None => continue,
                            },
                        };
                        if task["source"]["workflow_instance_id"] != instance
                            || task["source"]["execution_id"] != execution
                            || (!notice["data"]["plan_revision_id"].is_null()
                                && task["source"]["plan_revision_id"] != notice["data"]["plan_revision_id"])
                        {
                            continue;
                        }
                        if let Some(text) = task["supplemental_input"].as_str().filter(|text| !text.is_empty() && !controller_inputs.contains(*text)) {
                            feedback.push(json!({"id":input["id"],"input":text}));
                        }
                    }
                }
                notice["feedback"] = json!(feedback);
            }
            Ok(Some(json!(notices)))
        }
        "controller_runtime_events"=>{
            let (root,cursor):(String,u64)=serde_json::from_str(&key.id).map_err(storage)?;
            let mut q=conn.prepare_cached("SELECT id,target,kind,detail,time FROM events INDEXED BY events_workflow_root WHERE kind='workflow.runtime' AND json_extract(CASE WHEN kind='workflow.runtime' THEN detail ELSE '{}' END,'$.root_instance_id')=?1 AND id>?2 ORDER BY id LIMIT 32").map_err(storage)?;
            let events=q.query_map(rusqlite::params![root,cursor],|r|Ok(json!({"id":r.get::<_,u64>(0)?,"target":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"detail":r.get::<_,String>(3)?,"time":r.get::<_,u64>(4)?}))).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
            let head:u64=conn.query_row("SELECT coalesce(max(id),0) FROM events INDEXED BY events_workflow_root WHERE kind='workflow.runtime' AND json_extract(CASE WHEN kind='workflow.runtime' THEN detail ELSE '{}' END,'$.root_instance_id')=?1",[&root],|r|r.get(0)).map_err(storage)?;
            Ok(Some(json!({"events":events,"head":head})))
        }
        "workflow_root_event"=>{
            conn.query_row("SELECT coalesce(max(e.id),0) FROM events e WHERE e.target IN (SELECT id FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.root_instance_id')=?1)",[&key.id],|r|r.get::<_,u64>(0)).map(|cursor|Some(json!({"cursor":cursor}))).map_err(storage)
        }
        "workflow_root_decisions"=>{
            let mut q=conn.prepare_cached("SELECT json_object('id',d.id,'instance_id',d.definition_id,'status',json_extract(d.data,'$.status'),'decision_ref',json_extract(d.data,'$.decision_ref'),'operation',json_extract(d.data,'$.request.operation')) FROM governance_records d WHERE d.type='workflow_pending_decision' AND d.definition_id IN (SELECT id FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.root_instance_id')=?1) ORDER BY d.rowid DESC LIMIT 32").map_err(storage)?;
            let rows=q.query_map([&key.id],|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?.into_iter().map(|s|serde_json::from_str::<Value>(&s).map_err(storage)).collect::<Result<Vec<_>>>()?;Ok(Some(json!(rows)))
        }
        "workflow_members"=>{
            let (root,after,limit):(String,String,u64)=serde_json::from_str(&key.id).map_err(storage)?;require((1..=32).contains(&limit),"流程成员页超过预算")?;
            let mut q=conn.prepare_cached("SELECT id FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.root_instance_id')=?1 AND id>?2 ORDER BY id LIMIT ?3").map_err(storage)?;let ids=q.query_map(rusqlite::params![root,after,limit],|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;Ok(Some(json!(ids)))
        }
        "workflow_change_for_plan"=>{
            let (instance,plan):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_change' AND definition_id=?1 AND json_extract(data,'$.status')='applied' AND json_extract(data,'$.new_plan_revision_id')=?2 ORDER BY id LIMIT 1)",&[&instance,&plan])?.into_iter().next())
        }
        "workflow_tree_waits"=>{let (root,execution,since):(String,String,u64)=serde_json::from_str(&key.id).map_err(storage)?;workflow_observations::waits(conn,&root,&execution,since).map(Some)}
        "workflow_history_source"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_execution_history' AND definition_id=?1 ORDER BY rowid DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_completion_source"=>{
            let (instance,plan):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_completion' AND definition_id=?1 AND json_extract(data,'$.plan_revision_id')=?2 ORDER BY id LIMIT 1)",&[&instance,&plan])?.into_iter().next())
        }
        "workflow_turn_observation"=>workflow_observations::turn(conn,&key.id),
        "turn_status_reason"=>conn.query_row("SELECT json_object('content',substr(content,1,4096)) FROM conversation_events WHERE turn_id=?1 AND kind='status' ORDER BY CASE WHEN json_valid(content) THEN json_type(content,'$.code')='text' ELSE 0 END DESC,sequence DESC LIMIT 1",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "workflow_pause_intervals"|"workflow_wait_intervals"=>{
            let (owner,since):(String,u64)=serde_json::from_str(&key.id).map_err(storage)?;let kind=if key.kind=="workflow_pause_intervals"{"workflow_pause_interval"}else{"workflow_wait_interval"};
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type=?1 AND definition_id=?2 AND (json_extract(data,'$.finished') IS NULL OR CAST(json_extract(data,'$.finished') AS INTEGER)>=?3) ORDER BY CAST(json_extract(data,'$.started') AS INTEGER),id LIMIT 1025)",&[&kind,&owner,&since])?;require(rows.len()<=1024,"原执行等待区间超过预算")?;Ok(Some(json!(rows)))
        }
        "workflow_active_wait"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_wait_interval' AND definition_id=?1 AND json_extract(data,'$.finished') IS NULL ORDER BY id LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_retry_count"=>{
            let (instance,node,round):(String,String,u64)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT count(DISTINCT definition_id) FROM governance_records WHERE type='workflow_execution_history' AND json_extract(data,'$.workflow_instance_id')=?1 AND json_extract(data,'$.node_id')=?2 AND json_extract(data,'$.round')=?3",rusqlite::params![instance,node,round],|r|r.get::<_,u64>(0)).map(|count|Some(json!({"count":count}))).map_err(storage)
        }
        "workflow_last_outputs"=>{
            let (instance,node):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_execution_history' AND json_extract(data,'$.workflow_instance_id')=?1 AND json_extract(data,'$.node_id')=?2 AND json_extract(data,'$.outputs')!='{}' ORDER BY CAST(json_extract(data,'$.round') AS INTEGER) DESC,CAST(json_extract(data,'$.sequence') AS INTEGER) DESC,CAST(json_extract(data,'$.finished') AS INTEGER) DESC LIMIT 1)",&[&instance,&node])?.into_iter().next())
        }
        "provider_turn_pending"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT p.id FROM governance_records o JOIN governance_records p ON p.type='provider_poll_pending' AND p.definition_id=o.id AND json_extract(p.data,'$.status')='prepared' WHERE o.type='provider_poll_observation' AND o.definition_id=?1 ORDER BY p.rowid LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_snapshot_checkpoint"=>{
            let (instance,owner,workspace,revision):(String,String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT o.id FROM governance_records o JOIN governance_records c ON c.id=json_extract(o.data,'$.provenance.call_id') AND c.type='platform_call' WHERE o.type='workflow_output' AND o.immutable=1 AND json_extract(o.data,'$.value.owner_id')=?2 AND json_extract(o.data,'$.value.workspace_id')=?3 AND json_extract(o.data,'$.value.revision')=?4 AND json_extract(o.data,'$.provenance.kind')='platform' AND json_extract(o.data,'$.provenance.tool') IN ('workspace.snapshot','workspace.checkpoint') AND json_extract(c.data,'$.request.instance_id')=?1 AND json_extract(c.data,'$.status')='succeeded' ORDER BY o.rowid DESC LIMIT 1)",&[&instance,&owner,&workspace,&revision])?.into_iter().next())
        }
        "workflow_reconciliation_active"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_reconciliation' AND definition_id=?1 AND json_extract(data,'$.status') IN ('prepared','inspecting') ORDER BY id LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_reconciliation_latest"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_reconciliation' AND definition_id=?1 ORDER BY rowid DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_stop_source"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT s.id FROM governance_records s WHERE s.type IN ('workflow_stop','workflow_stop_reassertion') AND s.definition_id=?1 AND NOT EXISTS(SELECT 1 FROM governance_records r WHERE r.type='workflow_stop_reconciliation' AND r.definition_id=s.id AND r.immutable=1 AND json_extract(r.data,'$.issuer')='kernel_workspace_reconciliation') ORDER BY s.rowid DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "session_active_turns"=>{
            let rows=load_table_filtered_params::<Value>(conn,"turns","id IN (SELECT id FROM turns WHERE session_id=?1 AND status NOT IN ('completed','failed','cancelled') ORDER BY id LIMIT 65)",&[&key.id])?;require(rows.len()<=64,"原会话活动轮次超过预算")?;Ok(Some(json!(rows)))
        }
        "workflow_capacity"=>{
            let (instance,root):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            let default_limit=WorkflowScheduling::default().max_concurrency as u64;
            conn.query_row("SELECT json_object('instance',coalesce(sum(CASE WHEN i.id=?1 AND json_extract(s.value,'$.status') IN ('running','needs_reconciliation') AND EXISTS(SELECT 1 FROM source_definitions d,json_each(d.data,'$.steps') n WHERE d.reference=json_extract(i.data,'$.plan.workflow_ref') AND d.type='workflow' AND json_extract(n.value,'$.key')=json_extract(s.value,'$.step') AND json_extract(n.value,'$.executor.type') IN ('agent','tool')) THEN 1 ELSE 0 END),0),'root',coalesce(sum(CASE WHEN json_extract(s.value,'$.status') IN ('running','needs_reconciliation') AND EXISTS(SELECT 1 FROM source_definitions d,json_each(d.data,'$.steps') n WHERE d.reference=json_extract(i.data,'$.plan.workflow_ref') AND d.type='workflow' AND json_extract(n.value,'$.key')=json_extract(s.value,'$.step') AND json_extract(n.value,'$.executor.type') IN ('agent','tool')) THEN 1 ELSE 0 END),0),'steps',count(s.value),'root_limit',coalesce((SELECT json_extract(d.data,'$.scheduling.max_concurrency') FROM governance_records r JOIN source_definitions d ON d.reference=json_extract(r.data,'$.plan.workflow_ref') AND d.type='workflow' WHERE r.id=?2 AND r.type='workflow_instance'),?3)) FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.type='workflow_instance' AND json_extract(i.data,'$.root_instance_id')=?2",rusqlite::params![instance,root,default_limit],|r|r.get::<_,String>(0)).map_err(storage).and_then(|s|serde_json::from_str(&s).map(Some).map_err(storage))
        }
        "workflow_any_active_owner"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.owner_id')=?1 AND json_extract(data,'$.status') NOT IN ('completed','cancelled'))",[&key.id],|r|r.get::<_,bool>(0)).map(|active|Some(json!({"active":active}))).map_err(storage),
        "controller_owner_source"=>{
            let (owner,workspace):(String,bool)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.owner_id')=?1 AND coalesce(json_extract(data,'$.workspace_only'),0)=?2 AND json_extract(data,'$.status')!='closed' ORDER BY rowid DESC LIMIT 1)",&[&owner,&workspace])?.into_iter().next())
        }
        "feature_request_source"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='feature_request' AND definition_id=?1 ORDER BY rowid DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_governed_owner"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records i WHERE i.type='workflow_instance' AND (json_extract(i.data,'$.owner_id')=?1 OR json_extract(i.data,'$.owner_id')=(SELECT feature_id FROM tasks WHERE id=?1)) AND EXISTS(SELECT 1 FROM governance_records g WHERE g.type='workflow_instance' AND json_extract(g.data,'$.root_instance_id')=json_extract(i.data,'$.root_instance_id') AND json_extract(g.data,'$.human_governance')=1))",[&key.id],|r|r.get::<_,bool>(0)).map(|governed|Some(json!({"governed":governed}))).map_err(storage),
        "step_platform_calls"=>{
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='platform_call' AND coalesce(json_extract(data,'$.request.caller.execution_id'),definition_id)=?1 AND json_extract(data,'$.status') NOT IN ('succeeded','failed','cancelled') ORDER BY id LIMIT 65)",&[&key.id])?;
            require(rows.len()<=64,"单个执行活动平台调用超出预算")?;Ok(Some(json!(rows)))
        }
        "workflow_driver_root"|"workflow_children_source"=>{
            let kind=if key.kind=="workflow_driver_root"{"workflow_driver"}else{"workflow_children"};
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type=?1 AND definition_id=?2 ORDER BY rowid DESC LIMIT 1)",&[&kind,&key.id])?.into_iter().next())
        }
        "workflow_change_pending"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT c.id FROM governance_records c JOIN governance_records i ON i.id=c.definition_id AND i.type='workflow_instance' JOIN governance_records target ON target.id=?1 AND target.type='workflow_instance' WHERE c.type='workflow_change' AND json_extract(c.data,'$.status') IN ('prepared','waiting_quiescence') AND json_extract(i.data,'$.root_instance_id')=json_extract(target.data,'$.root_instance_id') ORDER BY c.id LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_governed"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.root_instance_id')=?1 AND json_extract(data,'$.human_governance')=1)",[&key.id],|r|r.get::<_,bool>(0)).map(|governed|Some(json!({"governed":governed}))).map_err(storage),
        "workflow_active_owner"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.owner_id')=?1 AND json_extract(data,'$.parent_execution_id') IS NULL AND json_extract(data,'$.status') NOT IN ('completed','cancelled'))",[&key.id],|r|r.get::<_,bool>(0)).map(|active|Some(json!({"active":active}))).map_err(storage),
        "workflow_active_pause"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_pause_interval' AND definition_id=?1 AND json_extract(data,'$.finished') IS NULL ORDER BY id LIMIT 1)",&[&key.id])?.into_iter().next()),
        "driver_lease_active"=>{
            let (driver,epoch):(String,u64)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_driver' AND id=?1 AND json_extract(data,'$.mode')='rust' AND json_extract(data,'$.epoch')=?2 AND json_extract(data,'$.expires_at')>=CAST(unixepoch('subsec')*1000 AS INTEGER))",rusqlite::params![driver,epoch],|r|r.get::<_,bool>(0)).map(|active|Some(json!({"active":active}))).map_err(storage)
        }
        "controller_turn_input"=>{
            let (controller,epoch,turn):(String,u64,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT g.id FROM governance_records g JOIN turns t ON t.id=?3 AND t.client_message_id=json_extract(g.data,'$.command.payload.client_message_id') WHERE g.type='workflow_intent' AND g.definition_id=?1 AND json_extract(g.data,'$.method')='conversation.send' AND json_extract(g.data,'$.command.payload.controller_epoch')=?2 AND json_extract(g.data,'$.command.target')=t.session_id ORDER BY g.id LIMIT 1)",&[&controller,&epoch,&turn])?.into_iter().next())
        }
        "platform_workspace_grant"=>{
            let (root,workspace,revision,handler):(String,String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='platform_call' AND json_extract(data,'$.root_instance_id')=?1 AND json_extract(data,'$.handler')=?4 AND json_extract(data,'$.receipt.workspace_id')=?2 AND json_extract(data,'$.receipt.revision')=?3 AND json_extract(data,'$.status')='succeeded' ORDER BY id LIMIT 1)",&[&root,&workspace,&revision,&handler])?.into_iter().next())
        }
        "platform_receipt"=>{
            let (root,handler,digest):(String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='platform_call' AND json_extract(data,'$.root_instance_id')=?1 AND json_extract(data,'$.handler')=?2 AND json_extract(data,'$.receipt_digest')=?3 AND json_extract(data,'$.status')='succeeded' ORDER BY id LIMIT 1)",&[&root,&handler,&digest])?.into_iter().next())
        }
        "workflow_waiver"=>{
            let (plan,node,requirement):(String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_waiver' AND json_extract(data,'$.plan_revision_id')=?1 AND json_extract(data,'$.node_id')=?2 AND json_extract(data,'$.requirement')=?3 ORDER BY id LIMIT 1)",&[&plan,&node,&requirement])?.into_iter().next())
        }
        "work_configuration_source"=>Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='work_configuration' AND definition_id=?1 ORDER BY version DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "platform_observation_head"=>{
            let (effect,phase):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='platform_effect_observation' AND definition_id=?1 AND json_extract(data,'$.phase')=?2 ORDER BY id LIMIT 1)",&[&effect,&phase])?.into_iter().next())
        }
        "controller_turn_calls"=>conn.query_row("SELECT count(*) FROM governance_records WHERE type='controller_tool_call' AND json_extract(data,'$.turn_id')=?1",[&key.id],|r|r.get::<_,u64>(0)).map(|count|Some(json!({"count":count}))).map_err(storage),
        "workflow_confirmations"=>{
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='workflow_confirmation' AND definition_id=?1 ORDER BY rowid LIMIT 65)",&[&key.id])?;
            require(rows.len()<=64,"同一执行的确认记录超过读取预算")?;Ok(Some(json!(rows)))
        }
        "configured_session_source"|"controller_session_source"|"workflow_command_intent"|"session_latest_turn"=>{
            let (table,predicate,kind)=match key.kind.as_str(){
                "configured_session_source"=>("governance_records","type=?1 AND definition_id=?2","configured_session"),
                "controller_session_source"=>("governance_records","type=?1 AND definition_id=?2","controller_session"),
                "workflow_command_intent"=>("governance_records","type=?1 AND json_extract(data,'$.command.command_id')=?2","workflow_intent"),
                _=>("turns","session_id=?2 AND ?1='turn'","turn"),
            };
            let selected=format!("id=(SELECT id FROM {table} WHERE {predicate} ORDER BY rowid DESC LIMIT 1)");
            Ok(load_table_filtered_params::<Value>(conn,table,&selected,&[&kind,&key.id])?.into_iter().next())
        }
        "repository_owner_eligible"=> {
            let (project,owner,kind,parent):(String,String,String,Option<String>)=serde_json::from_str(&key.id).map_err(storage)?;
            require(matches!(kind.as_str(),"feature"|"task"),"无效工作副本归属类型")?;
            let eligible:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM features WHERE ?3='feature' AND id=?2 AND project_id=?1 AND status!='completed') OR EXISTS(SELECT 1 FROM tasks t JOIN features f ON f.id=t.feature_id WHERE ?3='task' AND t.id=?2 AND t.feature_id=?4 AND f.project_id=?1 AND t.status!='completed' AND f.status!='completed' AND t.worktree='')",rusqlite::params![project,owner,kind,parent],|r|r.get(0)).map_err(storage)?;
            Ok(Some(json!({"eligible":eligible})))
        }
        "project_live"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND execution_mode='live')",[&key.id],|r|r.get::<_,bool>(0)).map(|live|Some(json!({"live":live}))).map_err(storage),
        "document_ownership"=>conn.query_row("SELECT json_object('project_id',project_id,'feature_id',feature_id,'task_id',task_id) FROM documents WHERE id=?1",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "workflow_output_scope"=>conn.query_row("SELECT scope FROM governance_records WHERE id=?1 AND type='workflow_output' AND immutable=1",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str::<Value>(&s).map(|scope|json!({"scope":scope})).map_err(storage)).transpose(),
        "document_successor"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM documents WHERE supersedes=?1)",[&key.id],|r|r.get::<_,bool>(0)).map(|exists|Some(json!({"exists":exists}))).map_err(storage),
        "proposal_successor"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM proposals WHERE supersedes=?1)",[&key.id],|r|r.get::<_,bool>(0)).map(|exists|Some(json!({"exists":exists}))).map_err(storage),
        "session_formal"=>conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='configured_session' AND definition_id=?1)",[&key.id],|r|r.get::<_,bool>(0)).map(|formal|Some(json!({"formal":formal}))).map_err(storage),
        "session_input_state"=>conn.query_row("SELECT json_object('active',json(CASE WHEN EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND status NOT IN ('completed','failed','cancelled')) THEN 'true' ELSE 'false' END),'permission_switch_pending',json(CASE WHEN EXISTS(SELECT 1 FROM governance_records WHERE type='permission_switch' AND definition_id=?1 AND json_extract(data,'$.status')!='adopted') THEN 'true' ELSE 'false' END),'configured',json(CASE WHEN EXISTS(SELECT 1 FROM governance_records WHERE type='configured_session' AND definition_id=?1) THEN 'true' ELSE 'false' END),'controller',json(CASE WHEN EXISTS(SELECT 1 FROM governance_records WHERE type='controller_session' AND definition_id=?1) THEN 'true' ELSE 'false' END))",[&key.id],|r|r.get::<_,String>(0)).map_err(storage).and_then(|s|serde_json::from_str(&s).map(Some).map_err(storage)),
        "discussion_live_turn" => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND status IN ('queued','dispatching','running','waiting_interaction'))",
            [&key.id], |r| r.get::<_,bool>(0),
        ).map(|active|Some(json!(active))).map_err(storage),
        "session_open_candidate"=>{
            let (owner,identity,instance):(String,String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"sessions","id=(SELECT s.id FROM sessions s JOIN source_definitions d ON d.reference=s.agent AND d.type='agent' WHERE s.owner_id=?1 AND d.identity=?2 AND coalesce(json_extract(s.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(s.runtime_binding,'$.agent_configuration.agent_instance_id'))=?3 AND json_extract(s.runtime_binding,'$.work_type')='discussion' AND NOT EXISTS(SELECT 1 FROM governance_records WHERE id='discussion-session-deleted:' || s.id) ORDER BY s.rowid DESC LIMIT 1)",&[&owner,&identity,&instance])?.into_iter().next())
        }
        "conversation_client"=>{
            let (session,client):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT json_object('id',t.id,'input',t.input,'input_digest',t.input_digest,'experience_refs',json(coalesce((SELECT json_extract(g.data,'$.experience_refs') FROM governance_records g WHERE g.type='conversation_context' AND g.definition_id=t.id ORDER BY g.rowid LIMIT 1),'[]'))) FROM turns t WHERE t.session_id=?1 AND t.client_message_id=?2",rusqlite::params![session,client],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose()
        }
        "repository_worktree_owner"=>{
            let (project,owner):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            Ok(load_table_filtered_params::<Value>(conn,"repository_worktrees","id=(SELECT id FROM repository_worktrees WHERE project_id=?1 AND owner_id=?2 ORDER BY rowid LIMIT 1)",&[&project,&owner])?.into_iter().next())
        }
        "owner_context"=>Ok(load_table_filtered_params::<Value>(conn,"contexts","id=(SELECT id FROM contexts WHERE owner_id=?1 ORDER BY rowid LIMIT 1)",&[&key.id])?.into_iter().next()),
        "owner_materials"=>{
            let owner:MaterialOwner=serde_json::from_str(&key.id).map_err(storage)?;
            let (predicate,args)=owner_documents(&owner);let mut q=conn.prepare_cached(&format!("SELECT id FROM documents WHERE {predicate} ORDER BY rowid LIMIT 2049")).map_err(storage)?;
            let ids=q.query_map(rusqlite::params_from_iter(args),|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
            require(ids.len()<=2048,"默认快照材料超出预算，请明确选择材料")?;Ok(Some(json!(ids)))
        }
        "proposal_turn"=>{
            let (tid,extract):(String,bool)=serde_json::from_str(&key.id).map_err(storage)?;
            let raw=conn.query_row("SELECT json_object('id',id,'session_id',session_id,'project_id',project_id,'status',status) FROM turns WHERE id=?1",[&tid],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
            let Some(raw)=raw else{return Ok(None);};let mut value:Value=serde_json::from_str(&raw).map_err(storage)?;
            if extract{
                let boundary:u64=conn.query_row("SELECT coalesce(max(sequence),0) FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND mode='full'",[&tid],|r|r.get(0)).map_err(storage)?;
                let (count,bytes):(u64,u64)=conn.query_row("SELECT count(*),coalesce(sum(length(CAST(content AS BLOB))),0) FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND sequence>=?2",rusqlite::params![tid,boundary],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
                require(count<=1024 && bytes<=2*1024*1024,"提案来源超出提取预算，请明确提供材料内容")?;
                let mut q=conn.prepare_cached("SELECT content FROM conversation_events WHERE turn_id=?1 AND kind='assistant' AND sequence>=?2 ORDER BY sequence").map_err(storage)?;
                let fragments=q.query_map(rusqlite::params![tid,boundary],|r|r.get::<_,String>(0)).map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
                value["assistant"]=json!(fragments.concat());
            }Ok(Some(value))
        }
        "configuration_transfers_active" => {
            let rows=load_table_filtered_params::<Value>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='configuration_transfer' AND json_extract(data,'$.status') IN ('prepared','applying') ORDER BY id LIMIT 65)",&[])?;
            require(rows.len()<=64,"活动配置发布操作超出预算")?;
            Ok(Some(json!(rows)))
        }
        "workflows_for_owner" => conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.owner_id')=?1)",[&key.id],|r|r.get::<_,bool>(0)).map(|exists|Some(json!({"exists":exists}))).map_err(storage),
        "controller_lease_active" => {
            let (controller,epoch):(String,u64)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT EXISTS(SELECT 1 FROM orchestration_leases WHERE controller_id=?1 AND epoch=?2 AND expires_at>=CAST(unixepoch('subsec')*1000 AS INTEGER))",rusqlite::params![controller,epoch],|r|r.get::<_,bool>(0)).map(|active|Some(json!({"active":active}))).map_err(storage)
        }
        "agent_scope" => conn.query_row("SELECT scope,json_extract(data,'$.agent_revision_id') FROM governance_records WHERE id=?1 AND type='agent_instance'",[&key.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(storage)?.map(|(scope,agent)|serde_json::from_str::<Value>(&scope).map(|scope|json!({"scope":scope,"agent_revision_id":agent})).map_err(storage)).transpose(),
        "context_observation_head" => {
            let (call,revision,phase):(String,u64,String)=serde_json::from_str(&key.id).map_err(storage)?;
            require(matches!(phase.as_str(),"read"|"reply"),"上下文观察阶段无效")?;
            Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='context_call_observation' AND definition_id=?1 AND json_extract(data,'$.call_version.revision')=?2 AND json_extract(data,'$.phase')=?3 ORDER BY id LIMIT 1)",&[&call,&revision,&phase])?.into_iter().next())
        }
        "controller_authority" => conn.query_row("SELECT json_object('type',type,'scope',json(scope),'data',json_object('epoch',json_extract(data,'$.epoch'),'status',json_extract(data,'$.status'),'session_id',json_extract(data,'$.session_id'))) FROM governance_records WHERE id=?1 AND type='workflow_controller'",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "workflow_run_authority" => conn.query_row("SELECT json_object('scope',json(scope),'project_id',json_extract(data,'$.project_id'),'root_instance_id',json_extract(data,'$.root_instance_id'),'status',json_extract(data,'$.status'),'paused',json(coalesce(data->'$.paused','false')),'cancel_requested',json(coalesce(data->'$.cancel_requested','false')),'parent_execution_id',json_extract(data,'$.parent_execution_id')) FROM governance_records WHERE id=?1 AND type='workflow_instance'",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "workflow_step_authority" => {
            let (instance,execution):(String,String)=serde_json::from_str(&key.id).map_err(storage)?;
            conn.query_row("SELECT json_object('id',json_extract(value,'$.id'),'status',json_extract(value,'$.status'),'plan_revision_id',json_extract(value,'$.plan_revision_id'),'input_digest',json_extract(value,'$.input_digest'),'contract_digest',json_extract(value,'$.contract_digest'),'node_id',json_extract(value,'$.node_id'),'session_id',json_extract(value,'$.session_id'),'stop_requested',json(CASE WHEN EXISTS(SELECT 1 FROM governance_records s WHERE s.type IN ('workflow_stop','workflow_stop_reassertion') AND s.definition_id=?2 AND NOT EXISTS(SELECT 1 FROM governance_records r WHERE r.type='workflow_stop_reconciliation' AND r.definition_id=s.id AND r.immutable=1 AND json_extract(r.data,'$.issuer')='kernel_workspace_reconciliation')) THEN 'true' ELSE 'false' END)) FROM json_each((SELECT data FROM governance_records WHERE id=?1 AND type='workflow_instance'),'$.steps') WHERE json_extract(value,'$.id')=?2 LIMIT 1",rusqlite::params![instance,execution],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose()
        }
        "conversation_open_seed" => Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='conversation_open_intent' AND definition_id=?1 ORDER BY rowid LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workspace_continuation_source" => Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workspace_continuation_checkpoint' AND definition_id=?1 AND immutable=1 ORDER BY rowid DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "workflow_stop_history" => Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='workflow_stop' AND definition_id=?1 ORDER BY rowid LIMIT 1)",&[&key.id])?.into_iter().next()),
        "immutable_document" | "immutable_snapshot" => {
            let table = if key.kind == "immutable_document" { "documents" } else { "snapshots" };
            conn.query_row(&format!("SELECT id FROM {table} WHERE id=?1"), [&key.id], |r| r.get::<_,String>(0)).optional().map(|v| v.map(|id| json!({"id":id}))).map_err(storage)
        }
        "conversation_capacity" => conn.query_row("SELECT count(*) FROM turns t WHERE t.project_id=?1 AND t.status NOT IN ('queued','completed','failed','cancelled') AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=t.session_id AND e.writer_revoked=0)", [&key.id], |r| r.get::<_,u64>(0)).map(|v| Some(json!({"active":v}))).map_err(storage),
        "conversation_writers" => {
            let mut q = conn.prepare_cached("SELECT json_object('runtime_binding',json(s.runtime_binding),'worktree',json_extract(t.code_view,'$.context.source.worktree')) FROM turns t JOIN sessions s ON s.id=t.session_id WHERE t.project_id=?1 AND t.status NOT IN ('queued','completed','failed','cancelled') AND t.code_view IS NOT NULL AND EXISTS(SELECT 1 FROM executions e WHERE e.session_id=s.id AND e.writer_revoked=0) AND EXISTS(SELECT 1 FROM governance_records g WHERE g.type='configured_execution_task' AND g.definition_id=t.id) LIMIT 2049").map_err(storage)?;
            let mut rows = q.query_map([&key.id], |r| r.get::<_,String>(0)).map_err(storage)?.map(|r| serde_json::from_str::<Value>(&r.map_err(storage)?).map_err(storage)).collect::<Result<Vec<_>>>()?;
            let mut fences=conn.prepare_cached("SELECT json_object('workspace_operation',id,'worktree',json_extract(data,'$.worktree')) FROM governance_records WHERE type='platform_workspace_fence' AND owner_project=?1 AND json_extract(data,'$.status')='active' LIMIT 2049").map_err(storage)?;
            rows.extend(fences.query_map([&key.id],|r|r.get::<_,String>(0)).map_err(storage)?.map(|r|serde_json::from_str::<Value>(&r.map_err(storage)?).map_err(storage)).collect::<Result<Vec<_>>>()?);
            require(rows.len() <= 2048, "活动工作区 writer 超出有界准入预算")?;
            Ok(Some(json!(rows)))
        }
        "session_protocol_exports" => conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records w JOIN executions e ON e.id=w.definition_id WHERE w.type='worker_cleanup_work' AND json_extract(w.data,'$.session_id')=?1 AND json_extract(w.data,'$.status')='pending' AND e.protocol_artifact IS NULL AND NOT EXISTS(SELECT 1 FROM governance_records p WHERE p.type='worker_protocol_observation' AND p.definition_id=e.id AND json_extract(p.data,'$.worker_operation_id')=e.operation_id))",[&key.id],|r|Ok(Some(json!({"pending":r.get::<_,bool>(0)?})))).map_err(storage),
        "session_worker_boundary" => conn.query_row("SELECT coalesce(max(rowid),0) FROM executions WHERE session_id=?1", [&key.id], |r| r.get::<_,u64>(0)).map(|v| Some(json!(v))).map_err(storage),
        "session_live_workers" => {
            let rows = load_table_filtered_params::<Value>(conn, "executions", "id IN (SELECT id FROM executions WHERE session_id=?1 AND writer_revoked=0 ORDER BY id LIMIT 65)", &[&key.id])?;
            require(rows.len() <= 64, "同一 Session 活跃 worker 超出安全预算")?;
            Ok(Some(json!(rows)))
        }
        "turn_pending_interactions" => {
            let rows = load_table_filtered_params::<Value>(conn, "interactions", "id IN (SELECT id FROM interactions WHERE turn_id=?1 AND status='pending' ORDER BY id LIMIT 1025)", &[&key.id])?;
            require(rows.len() <= 1024, "单轮待处理交互超出有界提交预算")?;
            Ok(Some(json!(rows)))
        }
        "conversation_event_key" | "interaction_request" => {
            let (session, external): (String, String) = serde_json::from_str(&key.id).map_err(storage)?;
            let (table, column) = if key.kind == "conversation_event_key" {
                ("conversation_events", "deduplication_key")
            } else {
                ("interactions", "external_request")
            };
            Ok(load_table_filtered_params::<Value>(conn, table, &format!("session_id=?1 AND {column}=?2"), &[&session, &external])?.into_iter().next())
        }
        "integration_discovery_head" => Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=(SELECT id FROM governance_records WHERE type='integration_discovery' AND definition_id=?1 ORDER BY version DESC LIMIT 1)",&[&key.id])?.into_iter().next()),
        "tool_intent" => Ok(load_table_filtered_params::<Value>(conn,"governance_records","id=?1 AND type='workflow_intent'",&[&key.id])?.into_iter().next()),
        "tool_worker" => conn.query_row("SELECT e.session_id,e.writer_revoked=0 AND NOT EXISTS(SELECT 1 FROM governance_records g WHERE g.type='worker_stop_intent' AND g.definition_id=e.id),e.kind,e.image_digest FROM executions e WHERE e.operation_id=?1",[&key.id],|r|Ok(json!({"session_id":r.get::<_,String>(0)?,"active":r.get::<_,bool>(1)?,"kind":r.get::<_,String>(2)?,"image_digest":r.get::<_,String>(3)?}))).optional().map_err(storage),
        "tool_turn" => conn.query_row("SELECT session_id,status,cancel_requested,external_message,external_turn FROM turns WHERE id=?1",[&key.id],|r|Ok(json!({"session_id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"cancel_requested":r.get::<_,bool>(2)?,"external_message":r.get::<_,Option<String>>(3)?,"external_turn":r.get::<_,Option<String>>(4)?}))).optional().map_err(storage),
        "tool_session" => Ok(None),
        "tool_workflow" => conn.query_row("SELECT json_object('scope',json(scope),'project_id',json_extract(data,'$.project_id'),'status',json_extract(data,'$.status'),'paused',json(coalesce(data->'$.paused','false')),'cancel_requested',json(coalesce(data->'$.cancel_requested','false'))) FROM governance_records WHERE id=?1 AND type='workflow_instance'",[&key.id],|r|r.get::<_,String>(0)).optional().map_err(storage)?.map(|s|serde_json::from_str(&s).map_err(storage)).transpose(),
        "project_reservation" => conn.query_row(
            "SELECT id,fingerprint FROM project_catalog WHERE command_id=?1",
            [&key.id],
            |row| Ok(json!({"id":row.get::<_,String>(0)?,"fingerprint":row.get::<_,String>(1)?})),
        ).optional().map_err(storage),
        "runtime_presence" => Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM runtime_installations WHERE id=?1)",[&key.id],|row|row.get::<_,bool>(0)).map_err(storage)?.then_some(json!(true))),
        "setting" => catalog::setting(conn, &key.id),
        "source_head" => conn.query_row(
            "SELECT reference,type,revision FROM source_definitions WHERE identity=?1 ORDER BY revision DESC LIMIT 1",
            [&key.id],
            |r| Ok(json!({"reference":r.get::<_,String>(0)?,"type":r.get::<_,String>(1)?,"revision":r.get::<_,u64>(2)?})),
        ).optional().map_err(storage),
        "source" => {
            let (identity, revision) = parse_source_reference(&key.id)?;
            conn.query_row(
                "SELECT type,name,scope,refs,data FROM source_definitions WHERE identity=?1 AND revision=?2",
                rusqlite::params![identity,revision],
                |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)),
            ).optional().map_err(storage)?.map(|(kind,name,scope,refs,data)| {
                serde_json::to_value(SourceDefinition {
                    r#type:kind,identity:identity.into(),revision,name,
                    scope:serde_json::from_str(&scope).map_err(storage)?,
                    refs:serde_json::from_str(&refs).map_err(storage)?,
                    data:serde_json::from_str(&data).map_err(storage)?,
                }).map_err(storage)
            }).transpose()
        }
        kind => Ok(load_table_filtered_params::<Value>(conn, table(kind)?, "id=?1", &[&key.id])?
            .into_iter().next()),
    }
}

// Selected-record predicates carry their source row epoch. Derive it from
// the payload already read in this same snapshot; commit-time validation uses
// the identical projection against a fresh authoritative read.
fn revision_projection(kind: &str) -> Option<fn(Option<&Value>) -> u64> {
    match kind {
        "repository_worktree_operation"
        | "controller_latest_intent"
        | "workflow_history_source"
        | "workflow_completion_source"
        | "workflow_change_for_plan"
        | "workflow_active_wait"
        | "workflow_last_outputs"
        | "workflow_snapshot_checkpoint"
        | "provider_turn_pending"
        | "workflow_reconciliation_active"
        | "workflow_reconciliation_latest"
        | "workflow_stop_source"
        | "workspace_continuation_source"
        | "workflow_stop_history"
        | "feature_request_source"
        | "controller_owner_source"
        | "workflow_driver_root"
        | "workflow_children_source"
        | "workflow_change_pending"
        | "workflow_active_pause"
        | "controller_turn_input"
        | "platform_workspace_grant"
        | "platform_receipt"
        | "workflow_waiver"
        | "work_configuration_source"
        | "platform_observation_head"
        | "configured_session_source"
        | "controller_session_source"
        | "workflow_command_intent"
        | "session_latest_turn"
        | "workflow_dispatch_source"
        | "permission_transition"
        | "conversation_open_seed"
        | "context_observation_head" => {
            Some(|value| value.and_then(|v| v["revision"].as_u64()).unwrap_or(0))
        }
        _ => None,
    }
}

fn snapshot(conn: &Connection, key: &ResourceKey) -> Result<ResourceSnapshot> {
    let mut value = value(conn, key)?;
    if matches!(
        key.kind.as_str(),
        "workflow_step"
            | "workflow_step_replace"
            | "workflow_step_restore"
            | "workflow_step_create"
            | "workflow_header"
    ) && let Some(value) = value.as_mut()
    {
        payload_store::restore_step(conn, value)?;
    }
    Ok(ResourceSnapshot {
        resource: key.clone(),
        revision: if let Some(project) = revision_projection(&key.kind) {
            project(value.as_ref())
        } else {
            current_revision(conn, key)?
        },
        value,
    })
}

fn current_revision(conn: &Connection, key: &ResourceKey) -> Result<u64> {
    key.validate()?;
    if let Some(project) = revision_projection(&key.kind) {
        return Ok(project(value(conn, key)?.as_ref()));
    }
    if key.kind == "selected_record" {
        let (kind, id, selected): (String, String, String) =
            serde_json::from_str(&key.id).map_err(storage)?;
        require(kind != "selected_record", "选择见证不能递归")?;
        let original = ResourceKey::new(kind, id);
        return if value(conn, &original)?.is_some_and(|v| v["id"] == selected) {
            current_revision(conn, &original)
        } else {
            Ok(0)
        };
    }
    if key.kind == "catalog_clock" {
        require(key.id == "configuration", "无效配置快照条件")?;
        return configuration_cache::revision(conn);
    }
    if matches!(
        key.kind.as_str(),
        "repository_pending_page" | "tasks_need_worktree_page" | "controller_interaction_page"
    ) {
        return Ok(0);
    }

    if key.kind == "repository_project_state" {
        return current_revision(conn, &repository_collection(&key.id));
    }
    if key.kind == "project_features_empty" {
        return current_revision(conn, &feature_collection(&key.id));
    }

    if key.kind == "controller_material_sources" {
        let (controller, epoch, session): (String, u64, String) =
            serde_json::from_str(&key.id).map_err(storage)?;
        return conn.query_row("SELECT coalesce(max(rowid),0) FROM governance_records WHERE type='controller_tool_call' AND definition_id=?1 AND json_extract(data,'$.epoch')=?2 AND json_extract(data,'$.session_id')=?3 AND json_extract(data,'$.method')='workflow.outputs.read' AND json_extract(data,'$.status')='succeeded'",rusqlite::params![controller,epoch,session],|r|r.get(0)).map_err(storage);
    }
    if key.kind == "workflow_material_inputs" {
        return conn.query_row("SELECT coalesce(max(rowid),0) FROM governance_records WHERE type='workflow_material_input' AND definition_id=?1",[&key.id],|r|r.get(0)).map_err(storage);
    }
    if key.kind == "controller_pending_inputs" {
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("controller-inputs:{}", key.id)),
        );
    }
    if matches!(
        key.kind.as_str(),
        "workflow_root_event" | "workflow_root_decisions"
    ) {
        return Ok(
            value(conn, &ResourceKey::new("workflow_root_event", &key.id))?
                .and_then(|v| v["cursor"].as_u64())
                .unwrap_or(0),
        );
    }
    if key.kind == "workflow_step_decisions" {
        let owner = value(conn, &ResourceKey::new("workflow_execution_owner", &key.id))?
            .ok_or_else(|| err("not_found", "原决定执行缺失"))?;
        let instance = value(
            conn,
            &ResourceKey::new(
                "workflow_run_authority",
                owner["instance_id"].as_str().unwrap_or(""),
            ),
        )?
        .ok_or_else(|| err("not_found", "原决定根缺失"))?;
        return current_revision(
            conn,
            &ResourceKey::new(
                "workflow_root_decisions",
                instance["root_instance_id"].as_str().unwrap_or(""),
            ),
        );
    }
    if key.kind == "controller_runtime_events" {
        return Ok(value(conn, key)?
            .and_then(|v| v["head"].as_u64())
            .unwrap_or(0));
    }
    if key.kind == "workflow_members" {
        let (root, _, _): (String, String, u64) = serde_json::from_str(&key.id).map_err(storage)?;
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("workflow-capacity:{root}")),
        );
    }
    if key.kind == "workflow_tree_waits" {
        let (root, _, _): (String, String, u64) = serde_json::from_str(&key.id).map_err(storage)?;
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("workflow-tree-waits:{root}")),
        );
    }

    if matches!(
        key.kind.as_str(),
        "workflow_turn_observation" | "turn_status_reason"
    ) {
        return conn
            .query_row(
                "SELECT coalesce(max(sequence),0) FROM conversation_events WHERE turn_id=?1",
                [&key.id],
                |r| r.get(0),
            )
            .map_err(storage);
    }
    if matches!(
        key.kind.as_str(),
        "workflow_pause_intervals" | "workflow_wait_intervals"
    ) {
        let (owner, _): (String, u64) = serde_json::from_str(&key.id).map_err(storage)?;
        let prefix = if key.kind == "workflow_pause_intervals" {
            "workflow-pauses"
        } else {
            "workflow-waits"
        };
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("{prefix}:{owner}")),
        );
    }
    if key.kind == "workflow_retry_count" {
        return Ok(value(conn, key)?
            .and_then(|v| v["count"].as_u64())
            .unwrap_or(0));
    }

    if key.kind == "session_active_turns" {
        return current_revision(conn, &ResourceKey::new("session_input_state", &key.id));
    }
    if key.kind == "workflow_capacity" {
        let (_, root): (String, String) = serde_json::from_str(&key.id).map_err(storage)?;
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("workflow-capacity:{root}")),
        );
    }

    if key.kind == "step_platform_calls" {
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("platforms:{}", key.id)),
        );
    }
    if matches!(
        key.kind.as_str(),
        "workflow_governed" | "workflow_governed_owner"
    ) {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["governed"] == true),
        ));
    }
    if matches!(
        key.kind.as_str(),
        "workflow_active_owner" | "workflow_any_active_owner"
    ) {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["active"] == true),
        ));
    }
    if key.kind == "driver_lease_active" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["active"] == true),
        ));
    }

    if key.kind == "workflow_execution_owner" {
        return Ok(u64::from(value(conn, key)?.is_some()));
    }
    if key.kind == "controller_turn_calls" {
        return conn.query_row("SELECT count(*) FROM governance_records WHERE type='controller_tool_call' AND json_extract(data,'$.turn_id')=?1",[&key.id],|r|r.get(0)).map_err(storage);
    }
    if key.kind == "workflow_confirmations" {
        return conn.query_row("SELECT count(*) FROM governance_records WHERE type='workflow_confirmation' AND definition_id=?1",[&key.id],|r|r.get(0)).map_err(storage);
    }

    if key.kind == "repository_owner_eligible" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["eligible"] == true),
        ));
    }
    if key.kind == "project_live" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["live"] == true),
        ));
    }
    if key.kind == "session_formal" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["formal"] == true),
        ));
    }
    if key.kind == "conversation_client" {
        let (session, client): (String, String) = serde_json::from_str(&key.id).map_err(storage)?;
        return conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND client_message_id=?2)",
                rusqlite::params![session, client],
                |r| r.get(0),
            )
            .map_err(storage);
    }
    if key.kind == "discussion_live_turn" {
        return Ok(u64::from(value(conn, key)?.is_some_and(|v| v == true)));
    }
    if key.kind == "session_input_state" {
        let flags = value(conn, key)?.ok_or_else(|| err("storage", "会话输入谓词缺失"))?;
        return Ok(u64::from(flags["active"] == true)
            + 2 * u64::from(flags["permission_switch_pending"] == true)
            + 4 * u64::from(flags["configured"] == true)
            + 8 * u64::from(flags["controller"] == true));
    }
    if key.kind == "session_open_candidate" {
        return conn.query_row("SELECT revision FROM resource_versions WHERE kind='session_family' AND resource_id=?1",[&key.id],|r|r.get(0)).optional().map(|v|v.unwrap_or(0)).map_err(storage);
    }
    if key.kind == "repository_worktree_owner" {
        let raw = value(conn, key)?;
        return raw
            .as_ref()
            .map(|r| {
                current_revision(
                    conn,
                    &ResourceKey::new(
                        "repository_worktree",
                        r["id"]
                            .as_str()
                            .ok_or_else(|| err("storage", "工作区缺少身份"))?,
                    ),
                )
            })
            .unwrap_or(Ok(0));
    }
    if key.kind == "owner_context" {
        return conn
            .query_row(
                "SELECT revision FROM contexts WHERE owner_id=?1 ORDER BY rowid LIMIT 1",
                [&key.id],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(storage);
    }
    if key.kind == "owner_materials" {
        let owner: MaterialOwner = serde_json::from_str(&key.id).map_err(storage)?;
        let (predicate, args) = owner_documents(&owner);
        return conn
            .query_row(
                &format!("SELECT count(*) FROM documents WHERE {predicate}"),
                rusqlite::params_from_iter(args),
                |r| r.get(0),
            )
            .map_err(storage);
    }
    if key.kind == "proposal_turn" {
        let (tid, _): (String, bool) = serde_json::from_str(&key.id).map_err(storage)?;
        return current_revision(conn, &ResourceKey::new("turn", tid));
    }
    if matches!(
        key.kind.as_str(),
        "document_ownership" | "workflow_output_scope"
    ) {
        return Ok(u64::from(value(conn, key)?.is_some()));
    }
    if matches!(
        key.kind.as_str(),
        "document_successor" | "proposal_successor"
    ) {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["exists"] == true),
        ));
    }
    if key.kind == "configuration_transfers_active" {
        require(key.id == "configuration", "无效跨库发布谓词")?;
        return current_revision(
            conn,
            &ResourceKey::new("collection", "configuration-transfers"),
        );
    }
    if key.kind == "workflows_for_owner" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["exists"] == true),
        ));
    }
    if key.kind == "controller_lease_active" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["active"] == true),
        ));
    }

    if key.kind == "agent_scope" {
        return Ok(u64::from(value(conn, key)?.is_some()));
    }
    if matches!(
        key.kind.as_str(),
        "workflow_step_replace"
            | "workflow_step_restore"
            | "workflow_step_delete"
            | "workflow_step_create"
    ) {
        return current_revision(conn, &ResourceKey::new("workflow_step", &key.id));
    }
    if matches!(
        key.kind.as_str(),
        "controller_authority"
            | "workflow_run_authority"
            | "workflow_step_authority"
            | "workflow_step"
            | "workflow_header"
    ) {
        return conn
            .query_row(
                "SELECT revision FROM resource_versions WHERE kind=?1 AND resource_id=?2",
                rusqlite::params![key.kind, key.id],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(storage);
    }
    if matches!(
        key.kind.as_str(),
        "immutable_document" | "immutable_snapshot"
    ) {
        return Ok(u64::from(value(conn, key)?.is_some()));
    }
    if matches!(
        key.kind.as_str(),
        "conversation_capacity" | "conversation_writers"
    ) {
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("conversation-capacity:{}", key.id)),
        );
    }
    if key.kind == "turn_pending_interactions" {
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("interactions:{}", key.id)),
        );
    }
    if key.kind == "session_protocol_exports" {
        return Ok(u64::from(
            value(conn, key)?.is_some_and(|v| v["pending"] == true),
        ));
    }
    if matches!(
        key.kind.as_str(),
        "session_live_workers" | "session_worker_boundary"
    ) {
        return current_revision(
            conn,
            &ResourceKey::new("collection", format!("workers:{}", key.id)),
        );
    }
    if matches!(
        key.kind.as_str(),
        "conversation_event_key" | "interaction_request"
    ) {
        return Ok(value(conn, key)?.map_or(0, |v| {
            if key.kind == "interaction_request" {
                v["revision"].as_u64().unwrap_or(0)
            } else {
                1
            }
        }));
    }
    // These read-only predicates track native facts even while their creation
    // families still use legacy insertion. They can never be written as rows.
    if key.kind == "integration_discovery_head" {
        return conn.query_row("SELECT version FROM governance_records WHERE type='integration_discovery' AND definition_id=?1 ORDER BY version DESC LIMIT 1",[&key.id],|r|r.get(0)).optional().map(|v|v.unwrap_or(0)).map_err(storage);
    }
    if key.kind == "tool_worker" {
        return conn.query_row("SELECT EXISTS(SELECT 1 FROM executions e WHERE e.operation_id=?1 AND e.writer_revoked=0 AND NOT EXISTS(SELECT 1 FROM governance_records g WHERE g.type='worker_stop_intent' AND g.definition_id=e.id))",[&key.id],|r|r.get(0)).map_err(storage);
    }
    if matches!(key.kind.as_str(), "tool_workflow" | "tool_intent") {
        return conn
            .query_row(
                "SELECT revision FROM governance_records WHERE type=?1 AND id=?2",
                rusqlite::params![
                    if key.kind == "tool_workflow" {
                        "workflow_instance"
                    } else {
                        "workflow_intent"
                    },
                    key.id
                ],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(storage);
    }
    if matches!(
        key.kind.as_str(),
        "tool_turn"
            | "tool_session"
            | "turn"
            | "session"
            | "interaction"
            | "governance"
            | "project"
            | "feature"
            | "task"
            | "context"
            | "proposal"
            | "repository_operation"
    ) {
        let table = match key.kind.as_str() {
            "tool_turn" | "turn" => "turns",
            "interaction" => "interactions",
            "governance" => "governance_records",
            "project" => "projects",
            "feature" => "features",
            "task" => "tasks",
            "context" => "contexts",
            "proposal" => "proposals",
            "repository_operation" => "repository_operations",
            _ => "sessions",
        };
        return conn
            .query_row(
                &format!("SELECT revision FROM {table} WHERE id=?1"),
                [&key.id],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(storage);
    }
    if key.kind == "project_reservation" {
        return conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE command_id=?1)",
                [&key.id],
                |row| row.get(0),
            )
            .map_err(storage);
    }
    if key.kind == "runtime_presence" {
        return conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_installations WHERE id=?1)",
                [&key.id],
                |row| row.get(0),
            )
            .map_err(storage);
    }
    if let Some(revision) = conn
        .query_row(
            "SELECT revision FROM resource_versions WHERE kind=?1 AND resource_id=?2",
            rusqlite::params![key.kind, key.id],
            |r| r.get::<_, u64>(0),
        )
        .optional()
        .map_err(storage)?
    {
        return Ok(revision);
    }
    let revision = match key.kind.as_str() {
        "collection" | "event_entry" | "configuration_reset" => return Ok(0),
        "setting" => conn.query_row("SELECT 1 FROM settings WHERE key=?1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        "source_head" => conn.query_row("SELECT revision FROM source_definitions WHERE identity=?1 ORDER BY revision DESC LIMIT 1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        "source" => conn.query_row("SELECT 1 FROM source_definitions WHERE reference=?1",[&key.id],|r|r.get::<_,u64>(0)).optional(),
        kind => {
            let expression = if matches!(kind,"project"|"feature"|"task"|"context"|"turn"|"interaction"|"proposal"|"governance"|"repository_operation") { "revision" } else { "1" };
            conn.query_row(&format!("SELECT {expression} FROM {} WHERE id=?1",table(kind)?),[&key.id],|r|r.get::<_,u64>(0)).optional()
        }
    }.map_err(sqlite_error)?;
    Ok(revision.unwrap_or(0))
}

pub(super) fn read(pool: &ConnectionPool, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
    require(keys.len() <= 4096, "一次读取的资源过多")?;
    if !resource_read_scope::active(pool) {
        return read_uncached(pool, keys);
    }
    for key in keys {
        key.validate()?;
    }
    let mut cached = keys
        .iter()
        .map(|key| resource_read_scope::get(pool, key))
        .collect::<Vec<_>>();
    let missing = keys
        .iter()
        .zip(&cached)
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k.clone())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(cached.into_iter().map(Option::unwrap).collect());
    }
    let values = read_uncached(pool, &missing)?;
    resource_read_scope::insert(pool, &values);
    let mut values = values.into_iter();
    for value in &mut cached {
        if value.is_none() {
            *value = values.next();
        }
    }
    Ok(cached.into_iter().map(Option::unwrap).collect())
}

fn read_uncached(pool: &ConnectionPool, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
    let _span = performance::Span::new("resource.read");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let values = read_on(&conn, keys)?;
    conn.execute_batch("COMMIT;").map_err(storage)?;
    Ok(values)
}

/// Compose several indexed facts and predicate tokens in the caller's one read snapshot.
pub(super) fn read_on(conn: &Connection, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
    require(keys.len() <= 4096, "一次读取的资源过多")?;
    if keys
        .iter()
        .filter(|key| key.kind == "workflow_step")
        .count()
        < 2
    {
        return keys.iter().map(|key| snapshot(conn, key)).collect();
    }
    // Header observations witness every native step. Read each selected array
    // once instead of performing one json_each scan per requested identity.
    let mut groups = std::collections::BTreeMap::<String, Vec<(usize, String)>>::new();
    let mut values = vec![None; keys.len()];
    for (index, key) in keys.iter().enumerate() {
        key.validate()?;
        if key.kind == "workflow_step" {
            let (instance, execution): (String, String) =
                serde_json::from_str(&key.id).map_err(storage)?;
            groups.entry(instance).or_default().push((index, execution));
        } else {
            values[index] = Some(snapshot(conn, key)?);
        }
    }
    for (instance, selected) in groups {
        let ids = json!(selected.iter().map(|(_, id)| id).collect::<Vec<_>>()).to_string();
        let mut query = conn.prepare_cached("SELECT json_extract(s.value,'$.id'),s.value FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.id=?1 AND i.type='workflow_instance' AND json_extract(s.value,'$.id') IN (SELECT value FROM json_each(?2))").map_err(storage)?;
        let rows = query
            .query_map(rusqlite::params![instance, ids], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<std::collections::BTreeMap<_, _>>>()
            .map_err(storage)?;
        let resources = json!(
            selected
                .iter()
                .map(|(index, _)| &keys[*index].id)
                .collect::<Vec<_>>()
        )
        .to_string();
        let mut query = conn.prepare_cached("SELECT resource_id,revision FROM resource_versions WHERE kind='workflow_step' AND resource_id IN (SELECT value FROM json_each(?1))").map_err(storage)?;
        let versions = query
            .query_map([resources], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<std::collections::BTreeMap<_, _>>>()
            .map_err(storage)?;
        for (index, execution) in selected {
            let key = &keys[index];
            values[index] = Some(ResourceSnapshot {
                resource: key.clone(),
                revision: versions.get(&key.id).copied().unwrap_or(0),
                value: rows
                    .get(&execution)
                    .map(|value| payload_store::decode_step(conn, value))
                    .transpose()?,
            });
        }
    }
    values
        .into_iter()
        .map(|value| value.ok_or_else(|| err("storage", "批量资源读取不完整")))
        .collect()
}

fn sqlite_error(error: rusqlite::Error) -> Error {
    if matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ) {
        err("storage_busy", "数据库短事务正在竞争")
            .technical_not_applied()
            .with_context("sqlite_error", json!({"cause":format!("{error:?}")}))
    } else {
        storage(error)
    }
}

fn replay_on(
    conn: &Connection,
    command_id: &str,
    expected_fingerprint: &str,
) -> Result<Option<CommitReceipt>> {
    let _packs = payload_store::read_scope();
    let previous = conn
        .query_row(
            "SELECT fingerprint,CAST(result AS TEXT),versions,cursor FROM operations WHERE id=?1",
            [command_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error)?;
    previous
        .map(|(fingerprint, result, versions, cursor)| {
            if !project_storage::fingerprint_matches(&fingerprint, expected_fingerprint) {
                return Err(err("invalid_params", "command_id 被不同参数或身份复用"));
            }
            let versions = if versions == project_storage::EVENT_VERSIONS {
                conn.query_row(
                    "SELECT CAST(detail AS TEXT) FROM events WHERE id=?1 AND kind='resource.changed'",
                    [cursor],
                    |row| row.get(0),
                )
                .map_err(sqlite_error)?
            } else {
                versions
            };
            Ok(CommitReceipt {
                result: payload_store::decode_result(conn, &result)?,
                versions: store_changes::decode_versions(&versions)?,
                cursor,
            })
        })
        .transpose()
}

pub(super) fn replay(
    pool: &ConnectionPool,
    command_id: &str,
    fingerprint: &str,
) -> Result<Option<CommitReceipt>> {
    let conn = pool.read()?;
    replay_on(&conn, command_id, fingerprint)
}

struct SqlWrite {
    sql: String,
    values: Vec<SqlValue>,
}

type ColumnSpecs = std::collections::BTreeMap<&'static str, Vec<(String, String)>>;

pub(super) fn encode_column(value: &Value, kind: &str) -> Result<SqlValue> {
    if value.is_null() {
        return Ok(SqlValue::Null);
    }
    if kind == "JSON" {
        return Ok(SqlValue::Text(value.to_string()));
    }
    match value {
        Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        Value::Number(value) => value
            .as_i64()
            .map(SqlValue::Integer)
            .ok_or_else(|| err("invalid_params", "整数值超出 SQLite 存储范围")),
        Value::String(value) => Ok(SqlValue::Text(value.clone())),
        _ => Err(err("invalid_params", "结构化值需要 JSON 列")),
    }
}

fn prepare_write(change: &ResourceChange, specs: &ColumnSpecs) -> Result<Option<SqlWrite>> {
    if change.resource.kind == "configuration_reset" {
        return Ok(None);
    }
    if matches!(
        change.resource.kind.as_str(),
        "workflow_step"
            | "workflow_step_replace"
            | "workflow_step_restore"
            | "workflow_step_delete"
            | "workflow_step_create"
    ) {
        let (instance, execution): (String, String) =
            serde_json::from_str(&change.resource.id).map_err(storage)?;
        let create = change.resource.kind == "workflow_step_create";
        let delete = change.resource.kind == "workflow_step_delete";
        if !delete {
            require(
                change.value["workflow_instance_id"] == instance && change.value["id"].is_string(),
                "步骤写入缺少实际归属或执行身份",
            )?;
        }
        if matches!(
            change.resource.kind.as_str(),
            "workflow_step" | "workflow_step_create"
        ) {
            require(
                change.value["id"] == execution,
                "步骤更新不能替换原执行身份",
            )?;
        }
        let values = vec![
            SqlValue::Text(instance),
            SqlValue::Text(execution),
            SqlValue::Text(change.value.to_string()),
        ];
        let sql = if create {
            "UPDATE governance_records SET data=json_insert(data,'$.steps[#]',json(?3)),revision=revision+1 WHERE id=?1 AND type='workflow_instance' AND NOT EXISTS(SELECT 1 FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=?2)"
        } else if delete {
            "UPDATE governance_records SET data=json_remove(data,(SELECT '$.steps['||s.key||']' FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=?2)),revision=revision+1 WHERE id=?1 AND type='workflow_instance' AND EXISTS(SELECT 1 FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=?2) AND ?3='null'"
        } else {
            "UPDATE governance_records SET data=json_set(data,(SELECT '$.steps['||s.key||']' FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=?2),json(?3)),revision=revision+1 WHERE id=?1 AND type='workflow_instance' AND EXISTS(SELECT 1 FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=?2) AND (json_extract(?3,'$.id')=?2 OR NOT EXISTS(SELECT 1 FROM json_each(data,'$.steps') s WHERE json_extract(s.value,'$.id')=json_extract(?3,'$.id')))"
        };
        return Ok(Some(SqlWrite {
            sql: sql.into(),
            values,
        }));
    }
    if change.resource.kind == "workflow_header" {
        require(
            change.value.is_object() && change.value.get("steps").is_none(),
            "流程头写入不能覆盖步骤",
        )?;
        return Ok(Some(SqlWrite{sql:"UPDATE governance_records SET data=json_set(json(?2),'$.steps',json_extract(data,'$.steps')),revision=revision+1 WHERE id=?1 AND type='workflow_instance' AND json_extract(data,'$.owner_id')=json_extract(?2,'$.owner_id') AND json_extract(data,'$.project_id')=json_extract(?2,'$.project_id') AND json_extract(data,'$.root_instance_id')=json_extract(?2,'$.root_instance_id') AND json_extract(data,'$.parent_execution_id') IS json_extract(?2,'$.parent_execution_id')".into(),values:vec![SqlValue::Text(change.resource.id.clone()),SqlValue::Text(change.value.to_string())]}));
    }
    if change.resource.kind == "event_entry" {
        return Ok(Some(SqlWrite {
            sql: "INSERT INTO events(target,kind,detail,time) VALUES(?1,?2,?3,?4)".into(),
            values: vec![
                encode_column(&change.value["target"], "TEXT")?,
                encode_column(&change.value["kind"], "TEXT")?,
                encode_column(&change.value["detail"], "TEXT")?,
                encode_column(&change.value["time"], "INTEGER")?,
            ],
        }));
    }
    if change.resource.kind == "source_head" {
        return Ok(None);
    }
    if change.resource.kind == "setting" {
        return Ok(Some(SqlWrite {
            sql: "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value".into(),
            values: vec![SqlValue::Text(change.resource.id.clone()), SqlValue::Text(change.value.to_string())],
        }));
    }
    if change.resource.kind == "source" {
        let source: SourceDefinition =
            serde_json::from_value(change.value.clone()).map_err(storage)?;
        require(
            source.revision <= i64::MAX as u64,
            "源修订超出 SQLite 存储范围",
        )?;
        require(
            source.clone().into_record()?.id == change.resource.id,
            "源身份与提交目标不一致",
        )?;
        return Ok(Some(SqlWrite {
            sql: "INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES(?1,?2,?3,?4,?5,?6,?7)".into(),
            values: vec![
                SqlValue::Text(source.identity), SqlValue::Integer(source.revision as i64),
                SqlValue::Text(source.r#type), SqlValue::Text(source.name),
                SqlValue::Text(serde_json::to_string(&source.scope).map_err(storage)?),
                SqlValue::Text(serde_json::to_string(&source.refs).map_err(storage)?),
                SqlValue::Text(source.data.to_string()),
            ],
        }));
    }
    require(
        change.value["id"].as_str() == Some(&change.resource.id),
        "对象身份与提交目标不一致",
    )?;
    let table = table(&change.resource.kind)?;
    let cols = &specs[table];
    let names: Vec<_> = cols.iter().map(|(name, _)| format!("\"{name}\"")).collect();
    let placeholders: Vec<_> = cols.iter().enumerate().map(|(index, (name, _))| {
        if change.resource.kind == "conversation_event" && name == "sequence" {
            let session = cols.iter().position(|(n, _)| n == "session_id").unwrap() + 1;
            format!("(SELECT coalesce(max(sequence),0)+1 FROM conversation_events WHERE session_id=?{session})")
        } else {
            format!("?{}", index + 1)
        }
    }).collect();
    let updates: Vec<_> = names
        .iter()
        .filter(|name| name.as_str() != "\"id\"")
        .map(|name| format!("{name}=excluded.{name}"))
        .collect();
    let sql = if change.resource.kind == "conversation_event" {
        format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            names.join(","),
            placeholders.join(",")
        )
    } else {
        format!(
            "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
            names.join(","),
            placeholders.join(","),
            updates.join(",")
        )
    };
    let values = cols
        .iter()
        .map(|(name, kind)| encode_column(&change.value[name], kind))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(SqlWrite { sql, values }))
}

fn prepare_writes(
    pool: &ConnectionPool,
    prepared: &PreparedChangeSet,
) -> Result<(Vec<Option<SqlWrite>>, String, bool)> {
    let changed: std::collections::BTreeSet<_> = prepared
        .changes
        .iter()
        .map(|change| &change.resource)
        .collect();
    for change in &prepared.changes {
        if change.resource.kind == "configuration_reset" {
            prepared
                .read_set
                .require(&ResourceKey::new("catalog_clock", "configuration"))?;
        }
        if matches!(
            change.resource.kind.as_str(),
            "workflow_step"
                | "workflow_step_replace"
                | "workflow_step_restore"
                | "workflow_step_delete"
                | "workflow_header"
        ) {
            require(
                prepared.read_set.require(&change.resource)? > 0,
                "流程头和步骤仅能更新实际已存在资源",
            )?;
        }
        if change.resource.kind == "workflow_step_create" {
            require(
                prepared.read_set.require(&change.resource)? == 0,
                "新执行必须验证不存在",
            )?;
        }
        if matches!(
            change.resource.kind.as_str(),
            "workflow_step_replace" | "workflow_step_restore"
        ) {
            let (instance, _): (String, String) =
                serde_json::from_str(&change.resource.id).map_err(storage)?;
            let key = ResourceKey::new(
                "workflow_step",
                serde_json::to_string(&(
                    instance,
                    change.value["id"]
                        .as_str()
                        .ok_or_else(|| err("invalid_params", "替换执行缺少新身份"))?,
                ))
                .map_err(storage)?,
            );
            let target = prepared.read_set.require(&key)?;
            require(
                if change.resource.kind == "workflow_step_restore" {
                    target > 0
                } else {
                    target == 0
                },
                "替换或恢复执行的原身份版本不符",
            )?;
        }
        if change.resource.kind == "event_entry" {
            require(
                prepared.read_set.require(&change.resource)? == 0,
                "领域事件只允许追加",
            )?;
        }
        if change.resource.kind == "conversation_event" {
            require(
                prepared.read_set.require(&change.resource)? == 0 && change.value["sequence"] == 0,
                "会话事件仅允许追加，序号由数据库分配",
            )?;
        }
        if matches!(
            change.resource.kind.as_str(),
            "session"
                | "turn"
                | "interaction"
                | "governance"
                | "project"
                | "feature"
                | "task"
                | "context"
                | "proposal"
                | "repository_operation"
        ) {
            require(
                change.value["revision"].as_u64()
                    == Some(prepared.read_set.require(&change.resource)? + 1),
                format!(
                    "{} {} 必须按原版本 {} 推进一次，提交 revision={}",
                    change.resource.kind,
                    change.resource.id,
                    prepared.read_set.require(&change.resource)?,
                    change.value["revision"]
                ),
            )?;
        }
        if change.resource.kind == "runtime_installation" {
            if prepared.read_set.require(&change.resource)? > 0 {
                require(
                    prepared
                        .read_set
                        .require(&ResourceKey::new("runtime_presence", &change.resource.id))?
                        == 0,
                    "恢复安装必须验证记录确实缺失",
                )?;
                require(
                    prepared.read_set.require(&ResourceKey::new(
                        "setting",
                        format!("runtime.seal.{}", change.resource.id),
                    ))? > 0,
                    "恢复安装需要原始内容封存",
                )?;
            }
            let key: RuntimeKey = serde_json::from_value(json!({"provider":change.value["provider"],"version":change.value["version"],"os":change.value["os"],"arch":change.value["arch"]})).map_err(storage)?;
            require(
                change.resource.id
                    == devdesk_core::runtime_registry::installation_id(
                        &key,
                        change.value["recipe_digest"].as_str(),
                    ),
                "安装记录与版本/平台身份不一致",
            )?;
        }
        if change.resource.kind == "source" {
            let identity = change.value["identity"]
                .as_str()
                .ok_or_else(|| err("invalid_params", "缺少源身份"))?;
            let head = ResourceKey::new("source_head", identity);
            let original = prepared.read_set.require(&head)?;
            if !changed.contains(&head)
                && change.value["revision"]
                    .as_u64()
                    .is_none_or(|revision| revision > original)
            {
                return Err(err(
                    "precondition_required",
                    "源发布必须同时提交 head 的版本变化",
                ));
            }
        }
    }
    let mut specs = ColumnSpecs::new();
    let root;
    let external;
    {
        let conn = pool.read()?;
        root = project_storage::connection_root(&conn);
        external = payload_store::enabled(&conn)?;
        for change in &prepared.changes {
            if !matches!(
                change.resource.kind.as_str(),
                "setting"
                    | "configuration_reset"
                    | "source"
                    | "source_head"
                    | "event_entry"
                    | "workflow_step"
                    | "workflow_step_replace"
                    | "workflow_step_restore"
                    | "workflow_step_delete"
                    | "workflow_step_create"
                    | "workflow_header"
            ) {
                let table = table(&change.resource.kind)?;
                if !specs.contains_key(table) {
                    specs.insert(table, columns(&conn, table)?);
                }
            }
        }
    }
    let artifact_index = project_storage::prepared_snapshots(root.as_deref(), &prepared.changes)?;
    let output_artifacts = if external {
        prepared
            .changes
            .iter()
            .filter(|change| change.resource.kind == "artifact")
            .map(|change| serde_json::from_value::<Artifact>(change.value.clone()).map_err(storage))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let writes = prepared
        .changes
        .iter()
        .map(|change| {
            if change.resource.kind == "snapshot"
                && let Some(content) = artifact_index.reference(&change.value)?
            {
                let mut stored = change.clone();
                stored.value["content"] = json!(content);
                return prepare_write(&stored, &specs);
            }
            if let Some(root) = root.as_deref().filter(|_| external) {
                let mut value = change.value.clone();
                match change.resource.kind.as_str() {
                    "governance" => {
                        if let Some(kind) = value["type"].as_str().map(str::to_owned) {
                            let artifact = output_artifacts
                                .iter()
                                .find(|a| value["data"]["artifact_id"] == a.id);
                            payload_store::governance(root, &kind, &mut value["data"], artifact)?;
                        }
                    }
                    "workflow_step"
                    | "workflow_step_replace"
                    | "workflow_step_restore"
                    | "workflow_step_create" => {
                        payload_store::step(root, &mut value)?;
                    }
                    "workflow_header" => {
                        payload_store::governance(root, "workflow_instance", &mut value, None)?
                    }
                    "conversation_event"
                        if matches!(value["kind"].as_str(), Some("user" | "tool")) =>
                    {
                        if let Some(text) = value["content"].as_str() {
                            value["content"] = json!(payload_store::pack_text(root, text)?);
                        }
                    }
                    _ => return prepare_write(change, &specs),
                }
                return prepare_write(
                    &ResourceChange {
                        resource: change.resource.clone(),
                        value,
                    },
                    &specs,
                );
            }
            prepare_write(change, &specs)
        })
        .collect::<Result<_>>()?;
    let mut result = prepared.result.clone();
    if let Some(root) = root.as_deref().filter(|_| external) {
        payload_store::result(root, &mut result)?;
    }
    Ok((writes, result.to_string(), external))
}

pub(super) fn verify_read_set(conn: &Connection, read_set: &ReadSet) -> Result<()> {
    read_set.validate()?;
    for expected in read_set.versions() {
        let actual = current_revision(conn, &expected.resource)?;
        if expected.revision != actual {
            return Err(err(
                "revision_conflict",
                format!(
                    "{} {} 的版本已变化：预期 {}，当前 {}",
                    expected.resource.kind, expected.resource.id, expected.revision, actual
                ),
            ).technical_not_applied().with_context("verify_read_set",json!({"resource":expected.resource,"expected_revision":expected.revision,"actual_revision":actual})));
        }
    }
    Ok(())
}

fn commit_once(
    conn: &mut Connection,
    prepared: &PreparedChangeSet,
    writes: &[Option<SqlWrite>],
    stored_result: &str,
    compact_versions: bool,
    mut timing: Option<&mut performance::TransactionTiming>,
) -> Result<(CommitReceipt, bool)> {
    if let Some(receipt) = replay_on(conn, &prepared.command_id, &prepared.fingerprint)? {
        return Ok((receipt, false));
    }
    if let Some(timing) = timing.as_mut() {
        timing.beginning();
    }
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    if let Some(timing) = timing.as_mut() {
        timing.began();
    }
    let _span = performance::Span::new("resource.sql_transaction");
    let result = (|| -> Result<(CommitReceipt, bool)> {
        if let Some(receipt) = replay_on(&transaction, &prepared.command_id, &prepared.fingerprint)?
        {
            return Ok((receipt, false));
        }
        verify_read_set(&transaction, &prepared.read_set)?;
        let runtime_events = workflow_events::capture(&transaction, prepared)?;
        let mut versions = Vec::with_capacity(prepared.changes.len());
        for (change, write) in prepared.changes.iter().zip(writes) {
            let expected = prepared.read_set.require(&change.resource)?;
            // Execution epochs also cover transitional writers. Their table
            // triggers advance exactly once for either kind of write.
            if !matches!(
                change.resource.kind.as_str(),
                "execution"
                    | "head"
                    | "repository_worktree"
                    | "orchestration_lease"
                    | "orchestration_wakeup"
                    | "workflow_step"
                    | "workflow_step_replace"
                    | "workflow_step_restore"
                    | "workflow_step_delete"
                    | "workflow_step_create"
                    | "workflow_header"
            ) {
                if matches!(
                    change.resource.kind.as_str(),
                    "session"
                        | "turn"
                        | "interaction"
                        | "governance"
                        | "project"
                        | "feature"
                        | "task"
                        | "context"
                        | "proposal"
                        | "repository_operation"
                ) {
                    // Mixed-writer metadata is aligned only after the authoritative
                    // native epoch has been verified in this same SQL transaction.
                    transaction.execute("INSERT INTO resource_versions(kind,resource_id,revision) VALUES(?1,?2,?3) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=excluded.revision",rusqlite::params![change.resource.kind,change.resource.id,expected]).map_err(sqlite_error)?;
                }
                transaction.execute("INSERT OR IGNORE INTO resource_versions(kind,resource_id,revision) VALUES(?1,?2,?3)",rusqlite::params![change.resource.kind,change.resource.id,expected]).map_err(sqlite_error)?;
                let updated = transaction.execute("UPDATE resource_versions SET revision=revision+1 WHERE kind=?1 AND resource_id=?2 AND revision=?3",rusqlite::params![change.resource.kind,change.resource.id,expected]).map_err(sqlite_error)?;
                if updated != 1 {
                    return Err(err("revision_conflict", "提交时资源版本已变化"));
                }
            }
            if let Some(write) = write {
                transaction
                    .prepare_cached(&write.sql)
                    .map_err(sqlite_error)?
                    .execute(rusqlite::params_from_iter(&write.values))
                    .map_err(sqlite_error)?;
            }
            if change.resource.kind == "configuration_reset" {
                crate::configuration_reset::apply(&transaction, &change.value)?;
            }
            if matches!(
                change.resource.kind.as_str(),
                "workflow_step"
                    | "workflow_step_replace"
                    | "workflow_step_restore"
                    | "workflow_step_delete"
                    | "workflow_step_create"
                    | "workflow_header"
            ) {
                let parent = if change.resource.kind == "workflow_header" {
                    change.resource.id.clone()
                } else {
                    serde_json::from_str::<(String, String)>(&change.resource.id)
                        .map_err(storage)?
                        .0
                };
                // Hash only this selected native record, after applying its field
                // patch inside the short SQL commit. No source compilation, whole
                // project comparison, or replacement of concurrent sibling steps.
                let (refs, data): (String, String) = transaction
                    .query_row(
                        "SELECT refs,data FROM governance_records WHERE id=?1",
                        [&parent],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(sqlite_error)?;
                let refs: References = serde_json::from_str(&refs).map_err(storage)?;
                let data: Value = serde_json::from_str(&data).map_err(storage)?;
                transaction
                    .execute(
                        "UPDATE governance_records SET content_hash=?2 WHERE id=?1",
                        rusqlite::params![parent, content_digest(&refs, &data)],
                    )
                    .map_err(sqlite_error)?;
            }
            if matches!(
                change.resource.kind.as_str(),
                "execution"
                    | "head"
                    | "repository_worktree"
                    | "orchestration_lease"
                    | "orchestration_wakeup"
                    | "workflow_step"
                    | "workflow_step_replace"
                    | "workflow_step_restore"
                    | "workflow_step_delete"
                    | "workflow_step_create"
                    | "workflow_header"
            ) && current_revision(&transaction, &change.resource)? != expected + 1
            {
                return Err(err("revision_conflict", "运行或代码事实未按原版本推进"));
            }
            versions.push(ResourceVersion {
                resource: change.resource.clone(),
                revision: if change.resource.kind == "source_head" {
                    current_revision(&transaction, &change.resource)?
                } else {
                    expected + 1
                },
            });
        }
        workflow_projection::commit(&transaction, prepared)?;
        workflow_events::append(&transaction, prepared, runtime_events)?;
        let encoded_versions = store_changes::encode_versions(&versions, compact_versions)?;
        for change in &prepared.changes {
            if change.resource.kind == "source_head" {
                let actual = value(&transaction, &change.resource)?;
                require(
                    actual
                        .as_ref()
                        .is_some_and(|actual| actual["reference"] == change.value["reference"]),
                    "源 head 与实际修订不一致",
                )?;
            }
        }
        transaction
            .execute(
                "INSERT INTO events(target,kind,detail,time) VALUES(?1,'resource.changed',?2,?3)",
                rusqlite::params![
                    prepared.changes[0].resource.id,
                    encoded_versions,
                    prepared.time
                ],
            )
            .map_err(sqlite_error)?;
        let cursor = transaction.last_insert_rowid() as u64;
        transaction
        .execute(
            "INSERT INTO operations(id,fingerprint,result,versions,cursor) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                prepared.command_id,
                project_storage::stored_fingerprint(&prepared.fingerprint),
                stored_result,
                project_storage::EVENT_VERSIONS,
                cursor
            ],
        )
        .map_err(sqlite_error)?;
        Ok((
            CommitReceipt {
                result: prepared.result.clone(),
                versions,
                cursor,
            },
            true,
        ))
    })();
    match result {
        Ok((receipt, true)) => {
            if let Some(timing) = timing.as_mut() {
                timing.committing();
            }
            let committed = transaction.commit().map_err(sqlite_error);
            if let Some(timing) = timing.as_mut() {
                timing.commit_returned(committed.is_ok());
            }
            committed.map(|()| (receipt, true))
        }
        result => {
            // Observe the explicit rollback result without replacing the original
            // error or receipt. A failed rollback still returns through the pool's
            // existing autocommit/reuse checks, and is not reported as successful.
            if let Some(timing) = timing.as_mut() {
                timing.rolling_back();
            }
            let rolled_back = transaction.rollback();
            if let Some(timing) = timing.as_mut() {
                timing.rollback_returned(rolled_back.is_ok());
            }
            result
        }
    }
}

pub(super) fn commit(
    pool: &ConnectionPool,
    prepared: &PreparedChangeSet,
) -> Result<(CommitReceipt, bool)> {
    resource_read_scope::invalidate(pool);
    prepared.validate()?;
    require(prepared.changes.len() <= 4096, "一次提交的资源过多")?;
    let _span = performance::Span::new("resource.commit");
    match replay(pool, &prepared.command_id, &prepared.fingerprint) {
        Ok(None) => {}
        outcome => {
            let result = outcome.map(|receipt| (receipt.unwrap(), false));
            if let Some(mut timing) =
                performance::TransactionTiming::new(pool.observation_id(), prepared, 0)
            {
                timing.finished(&result);
            }
            return result;
        }
    }
    let (writes, stored_result, compact_versions) = prepare_writes(pool, prepared)?;
    let checkout_deadline = Instant::now() + Duration::from_secs(2);
    let mut technical_spent = Duration::ZERO;
    for attempt in 0..16 {
        // Return the submit connection before backing off. A busy caller must
        // not occupy one of the bounded handles while other independent
        // commits wait for it; every attempt retains the original read set.
        let result = {
            let mut timing =
                performance::TransactionTiming::new(pool.observation_id(), prepared, attempt);
            let waiting = performance::Span::new("resource.submit_pool_wait");
            let connection = pool.lock_until(checkout_deadline);
            if let Err(error) = &connection
                && let Some(timing) = timing.as_mut()
            {
                timing.finished(&Err(error.clone()));
            }
            let mut conn = connection?;
            drop(waiting);
            if let Some(timing) = timing.as_mut() {
                timing.pool_acquired();
            }
            let started = Instant::now();
            let result = commit_once(
                &mut conn,
                prepared,
                &writes,
                &stored_result,
                compact_versions,
                timing.as_mut(),
            );
            if let Some(timing) = timing.as_mut() {
                timing.finished(&result);
            }
            technical_spent += started.elapsed();
            result
        };
        match result {
            Err(error)
                if error.code == "storage_busy"
                    && attempt < 15
                    && technical_spent < Duration::from_millis(500) =>
            {
                performance::record("resource.busy_retry", 0.0);
                let jitter = prepared
                    .command_id
                    .bytes()
                    .take(16)
                    .map(u64::from)
                    .sum::<u64>()
                    % 5;
                let backoff = Duration::from_millis(2 + attempt + jitter);
                std::thread::sleep(backoff);
                technical_spent += backoff;
            }
            result => return result,
        }
    }
    unreachable!("bounded transaction retry")
}
