//! Exact frozen facts for one initial or resumed conversation dispatch.
use crate::{connection_pool::ConnectionPool, *};
use devdesk_core::{
    ConversationAuthority, ConversationStartSource, concurrency::*,
    tool_calls::WorkflowToolAuthority,
};
use rusqlite::OptionalExtension;

fn text(value: &Value, field: &str) -> Result<String> {
    value[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("storage", format!("冻结会话缺少 {field}")))
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn selected(conn: &Connection, kind: &str, definition: &str) -> Result<Option<ResourceSnapshot>> {
    let id=conn.query_row("SELECT id FROM governance_records WHERE type=?1 AND definition_id=?2 ORDER BY rowid LIMIT 1",rusqlite::params![kind,definition],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    id.map(|id| {
        resource_commits::read_on(conn, &[ResourceKey::new("governance", id)])
            .map(|mut r| r.remove(0))
    })
    .transpose()
}
fn workflow_authority(
    conn: &Connection,
    binding: &ConfigurationRecord,
    guards: &mut Vec<ResourceVersion>,
) -> Result<WorkflowToolAuthority> {
    let mut instance = text(&binding.data, "instance_id")?;
    let mut execution = text(&binding.data, "execution_id")?;
    let mut instances = Vec::new();
    let mut executions = Vec::new();
    let mut leaf = None;
    for _ in 0..9 {
        require(!instances.contains(&instance), "冻结任务祖先存在环")?;
        let keys = [
            ResourceKey::new("workflow_run_authority", &instance),
            ResourceKey::new(
                "workflow_step_authority",
                json!([instance, execution]).to_string(),
            ),
        ];
        let rows = resource_commits::read_on(conn, &keys)?;
        let run = rows[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "冻结任务原实例缺失"))?;
        let step = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "冻结任务原步骤缺失"))?;
        require(
            run["project_id"].as_str() == binding.scope.project_id(),
            "冻结任务祖先跨项目",
        )?;
        guards.extend(rows.iter().map(version));
        if leaf.is_none() {
            leaf = Some(step.clone());
        }
        instances.push(instance);
        executions.push(execution);
        let Some(parent) = run["parent_execution_id"].as_str() else {
            break;
        };
        let owner=conn.query_row("SELECT json_extract(resource_id,'$[0]') FROM resource_versions WHERE kind='workflow_step_authority' AND json_extract(resource_id,'$[1]')=?1 ORDER BY resource_id LIMIT 1",[parent],|r|r.get::<_,String>(0)).optional().map_err(storage)?.ok_or_else(||err("not_found","冻结任务原祖先归属缺失"))?;
        instance = owner;
        execution = parent.into();
    }
    require(
        instances.len() < 9
            || resource_commits::read_on(
                conn,
                &[ResourceKey::new(
                    "workflow_run_authority",
                    instances.last().unwrap(),
                )],
            )?[0]
                .value
                .as_ref()
                .is_some_and(|r| r["parent_execution_id"].is_null()),
        "冻结任务祖先超出预算",
    )?;
    let leaf = leaf.ok_or_else(|| err("storage", "冻结任务缺少原步骤"))?;
    Ok(WorkflowToolAuthority {
        instances,
        execution_id: executions[0].clone(),
        executions,
        plan_revision_id: text(&leaf, "plan_revision_id")?,
        input_digest: text(&leaf, "input_digest")?,
        contract_digest: text(&leaf, "contract_digest")?,
        node_id: text(&leaf, "node_id")?,
        session_id: Some(binding.definition_id.clone()),
    })
}

pub(super) fn source(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    tid: &str,
) -> Result<ConversationStartSource> {
    let _span = performance::Span::new("conversation.start_source");
    let conn = pool.read()?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    if configuration_cache::revision(&conn)? > catalog.revision {
        return Err(err(
            "catalog_refresh_required",
            "冻结会话引用需要新的源目录",
        ));
    }
    let first = resource_commits::read_on(
        &conn,
        &[
            ResourceKey::new("turn", tid),
            ResourceKey::new("conversation_open_seed", tid),
        ],
    )?;
    let mut raw_turn = first[0]
        .value
        .clone()
        .ok_or_else(|| err("not_found", "冻结轮次缺失"))?;
    let sid = text(&raw_turn, "session_id")?;
    let session_row =
        resource_commits::read_on(&conn, &[ResourceKey::new("session", &sid)])?.remove(0);
    let raw_session = session_row
        .value
        .clone()
        .ok_or_else(|| err("not_found", "冻结会话缺失"))?;
    let mut guards = vec![version(&first[0]), version(&session_row)];
    let task_row = selected(&conn, "configured_execution_task", tid)?.or(selected(
        &conn,
        "archived_configured_execution_task",
        tid,
    )?);
    let context = selected(&conn, "conversation_context", tid)?;
    let binding = selected(&conn, "configured_session", &sid)?;
    let controller = selected(&conn, "controller_session", &sid)?;
    for row in [&task_row, &context, &binding, &controller]
        .into_iter()
        .flatten()
    {
        guards.push(version(row));
    }
    let seed = first[1].value.as_ref();
    if let Some(seed) = seed {
        require(
            seed["data"]["session_id"] == sid && seed["data"]["turn_id"] == tid,
            "派发种子跨原会话",
        )?;
        guards.push(ResourceVersion {
            resource: ResourceKey::new("governance", text(seed, "id")?),
            revision: first[1].revision,
        });
        require(
            seed["data"]["task_record_id"] == json!(task_row.as_ref().map(|r| &r.resource.id))
                && seed["data"]["context_record_id"]
                    == json!(context.as_ref().map(|r| &r.resource.id)),
            "恢复不能替换原任务或上下文引用",
        )?;
    }
    // The first input of an explicit continuation uses the sealed source
    // itself, including its documents and the entire bounded transcript.
    let continuation_snapshot = if seed.is_none() && binding.is_none() && controller.is_none() {
        conn.query_row(
            "SELECT id FROM snapshots WHERE id=?1 AND kind='discussion_continuation'
             AND NOT EXISTS(SELECT 1 FROM turns WHERE session_id=?2
                 AND rowid<(SELECT rowid FROM turns WHERE id=?3))",
            rusqlite::params![raw_session["snapshot_id"].as_str(), sid, tid],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(storage)?
    } else {
        None
    };
    let snapshot_id = if let Some(seed) = seed {
        seed["data"]["specification_snapshot_id"].as_str()
    } else {
        continuation_snapshot
            .as_deref()
            .or_else(|| raw_turn["input_snapshot"].as_str())
            .or_else(|| raw_session["snapshot_id"].as_str())
    };
    let snapshot = snapshot_id
        .map(|id| {
            resource_commits::read_on(&conn, &[ResourceKey::new("snapshot", id)])
                .map(|mut r| r.remove(0))
        })
        .transpose()?;
    if let Some(row) = &snapshot {
        guards.push(version(row));
    }
    let user=conn.query_row("SELECT content FROM conversation_events WHERE session_id=?1 AND turn_id=?2 AND kind='user' ORDER BY rowid LIMIT 1",rusqlite::params![sid,tid],|r|r.get::<_,String>(0)).optional().map_err(storage)?;
    let user = user
        .map(|body| payload_store::unpack_text(&conn, body))
        .transpose()?;
    let task = task_row
        .as_ref()
        .map(|row| {
            let task = ConfigurationRecord::from_storage(
                row.value
                    .clone()
                    .ok_or_else(|| err("not_found", "原任务封存缺失"))?,
            )?;
            let digest = text(&task.data, "digest")?;
            let execution = text(&task.data["source"], "execution_id")?;
            let facts = workflow_intents::task_source_facts(
                &conn,
                &task.scope,
                &execution,
                task.data["task"].clone(),
                user.as_ref().map(|body| (body.clone(), digest)),
            )?;
            Ok::<_, Error>((task, facts))
        })
        .transpose()?;
    let authority = if let Some(seed) = seed {
        serde_json::from_value(seed["data"]["authority"].clone()).map_err(storage)?
    } else {
        let workflow = binding
            .as_ref()
            .map(|row| {
                ConfigurationRecord::from_storage(row.value.clone().unwrap())
                    .and_then(|b| workflow_authority(&conn, &b, &mut guards))
            })
            .transpose()?;
        let controller = controller
            .as_ref()
            .map(|row| {
                let row = ConfigurationRecord::from_storage(row.value.clone().unwrap())?;
                Ok::<_, Error>((
                    row.id,
                    text(&row.data, "controller_id")?,
                    row.data["epoch"]
                        .as_u64()
                        .ok_or_else(|| err("storage", "控制器原代次缺失"))?,
                ))
            })
            .transpose()?;
        ConversationAuthority {
            workflow,
            controller,
        }
    };
    let scope: SessionScope =
        serde_json::from_value(raw_session["scope"].clone()).map_err(storage)?;
    let pid = raw_session["project_id"]
        .as_str()
        .ok_or_else(|| err("invalid_scope", "冻结会话缺少项目"))?;
    let feature = raw_session["feature_id"].as_str();
    let task_id = raw_session["task_id"].as_str();
    let mut parameters: Vec<&dyn rusqlite::ToSql> = vec![&pid, &feature, &task_id];
    let ownership = match scope {
        SessionScope::Global => {
            "d.project_id IS NULL AND d.feature_id IS NULL AND d.task_id IS NULL"
        }
        SessionScope::Project => "d.project_id=?1",
        SessionScope::Feature | SessionScope::Task => {
            "(d.feature_id=?2 OR (d.project_id=?1 AND EXISTS(SELECT 1 FROM features f,json_each(f.source_materials) x WHERE f.id=?2 AND x.value=d.id))) AND (d.task_id IS NULL OR d.task_id=?3)"
        }
    };
    let document_ids = snapshot
        .as_ref()
        .and_then(|r| r.value.as_ref())
        .map(|s| s["documents"].to_string());
    let sql = if let Some(ids) = &document_ids {
        parameters.push(ids);
        format!(
            "id IN (SELECT d.id FROM documents d WHERE ({ownership}) AND d.id IN (SELECT value FROM json_each(?4)) ORDER BY d.rowid LIMIT 2049)"
        )
    } else {
        format!(
            "id IN (SELECT d.id FROM documents d WHERE {ownership} ORDER BY d.rowid LIMIT 2049)"
        )
    };
    // Explicit numbered bindings keep the owner predicate identical across scopes.
    let docs = if scope == SessionScope::Global && document_ids.is_none() {
        load_table_filtered_params::<Document>(&conn, "documents", &sql, &[])?
    } else if scope == SessionScope::Project && document_ids.is_none() {
        load_table_filtered_params::<Document>(&conn, "documents", &sql, &[&pid])?
    } else {
        load_table_filtered_params::<Document>(&conn, "documents", &sql, &parameters)?
    };
    require(docs.len() <= 2048, "冻结会话材料超出有界读取预算")?;
    guards.extend(docs.iter().map(|d| ResourceVersion {
        resource: ResourceKey::new("immutable_document", &d.id),
        revision: 1,
    }));
    conn.execute_batch("COMMIT;").map_err(storage)?;
    drop(conn);
    let mut memo = Default::default();
    let session: Session = serde_json::from_value(source_views::restore_with(
        &catalog.state,
        &raw_session,
        &mut memo,
    )?)
    .map_err(storage)?;
    let task = task
        .map(|(raw, facts)| {
            let body = workflow_intents::restore_task(catalog, facts, &mut memo)?;
            let rendered: Value = serde_json::from_str(&body).map_err(storage)?;
            let mut data = raw.data.clone();
            data.as_object_mut()
                .ok_or_else(|| err("storage", "任务封存需要对象"))?
                .remove("task");
            data.as_object_mut().unwrap().remove("rendered_input");
            let mut data = source_views::restore_with(&catalog.state, &data, &mut memo)?;
            data["task"] = rendered;
            data["rendered_input"] = json!(body);
            let task: ConfiguredAgentTask = serde_json::from_value(data).map_err(storage)?;
            task.validate()?;
            require(
                task.turn_id == tid && task.agent_revision_id == session.agent,
                "冻结任务改变了原 Turn 或 Agent",
            )?;
            Ok::<_, Error>(task)
        })
        .transpose()?;
    let input = raw_turn["input"]
        .as_str()
        .ok_or_else(|| err("storage", "原输入缺失"))?;
    raw_turn["input"] = if source_views::task_descriptor(&json!(input))?.is_some() {
        json!(
            task.as_ref()
                .ok_or_else(|| err("not_found", "编码任务缺少原封存"))?
                .rendered_input
        )
    } else {
        source_views::restore_with(&catalog.state, &json!(input), &mut memo)?
    };
    let turn: ConversationTurn = serde_json::from_value(raw_turn).map_err(storage)?;
    require(
        turn.session_id == session.id
            && turn.project_id == session.project_id.clone().unwrap_or_default()
            && task.as_ref().is_none_or(|t| t.rendered_input == turn.input),
        "冻结输入或范围不符",
    )?;
    let agent = catalog
        .state
        .definitions
        .iter()
        .find(|a| a.id == session.agent)
        .cloned()
        .ok_or_else(|| err("not_found", "原 Agent 修订缺失"))?;
    let skills = catalog
        .state
        .definitions
        .iter()
        .filter(|d| {
            agent
                .agent_profile
                .as_ref()
                .is_some_and(|p| p.skill_revisions.contains(&d.id))
        })
        .cloned()
        .collect();
    let specifications = catalog
        .state
        .resolve_agent(&session.agent)?
        .spec_refs
        .iter()
        .map(|id| catalog.state.configuration(id).cloned())
        .collect::<Result<Vec<_>>>()?;
    let snapshot = snapshot
        .map(|row| {
            row.value
                .ok_or_else(|| err("not_found", "原快照缺失"))
                .and_then(|raw| source_views::restore_with(&catalog.state, &raw, &mut memo))
                .and_then(|v| serde_json::from_value(v).map_err(storage))
        })
        .transpose()?;
    let context_references = context
        .as_ref()
        .and_then(|r| r.value.as_ref())
        .and_then(|v| v["data"]["experience_refs"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut unique = std::collections::BTreeMap::new();
    for guard in guards {
        if let Some(old) = unique.insert(guard.resource, guard.revision)
            && old != guard.revision
        {
            return Err(err("revision_conflict", "派发事实读集合不一致"));
        }
    }
    Ok(ConversationStartSource {
        session,
        turn,
        agent,
        skills,
        specifications,
        configured_task: task,
        snapshot,
        documents: docs,
        context_references,
        authority,
        task_record_id: task_row.map(|r| r.resource.id),
        context_record_id: context.map(|r| r.resource.id),
        read_set: ReadSet::new(
            unique
                .into_iter()
                .map(|(resource, revision)| ResourceVersion { resource, revision })
                .collect(),
        )?,
    })
}
