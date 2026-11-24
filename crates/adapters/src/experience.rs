//! Read-only, AgentID-scoped projection over authoritative project stores.
use super::*;

fn projects(root: &Path) -> Result<Vec<Project>> {
    let conn = connection_pool::ManagedConnection::read_only(&root.join("global.sqlite3"))?;
    let mut query = conn
        .prepare("SELECT project FROM project_catalog WHERE project IS NOT NULL ORDER BY id")
        .map_err(storage)?;
    query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
        .map(|row| serde_json::from_str(&row.map_err(storage)?).map_err(storage))
        .collect()
}
fn instance(root: &Path, current_project: &str, id: &str) -> Result<AgentInstance> {
    let mut records = GlobalStore::read_configuration(root)?
        .state
        .governance
        .clone();
    if let Ok(s) = SqliteStore::read_project_catalog(root, current_project) {
        records.extend(s.state.governance.clone());
    }
    if let Ok(s) = SqliteStore::read_global_discussion_catalog(root) {
        records.extend(
            s.state
                .governance
                .iter()
                .filter(|r| r.r#type == "agent_instance")
                .cloned(),
        );
    }
    let r = records
        .iter()
        .find(|r| r.id == id && r.r#type == "agent_instance")
        .ok_or_else(|| err("not_found", "Agent 不存在"))?;
    let i: AgentInstance = r.decode()?;
    require(
        i.scope.visible_from(&Scope::project(current_project)),
        "Agent 不属于当前项目",
    )?;
    Ok(i)
}
pub fn query(root: &Path, current_project: &str, params: &Value) -> Result<Value> {
    let (i, mut rows, unavailable) = visible_entries(root, current_project, params)?;
    rows.retain(|r| {
        ["feature_id", "task_id", "workflow_instance_id"]
            .iter()
            .all(|key| params[*key].as_str().is_none_or(|v| r[*key] == v))
            && params["source_project_id"]
                .as_str()
                .is_none_or(|pid| r["project_id"] == pid)
            && params["keyword"].as_str().is_none_or(|keyword| {
                ["project_name", "feature_name", "summary", "workflow_name"]
                    .iter()
                    .any(|key| {
                        r[*key]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&keyword.to_lowercase())
                    })
            })
            && params["project_name"].as_str().is_none_or(|name| {
                r["project_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&name.to_lowercase())
            })
            && params["feature_name"].as_str().is_none_or(|name| {
                r["feature_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&name.to_lowercase())
            })
            && params["from"].as_u64().is_none_or(|from| {
                r["work_time"]
                    .as_u64()
                    .or(r["captured_at"].as_u64())
                    .unwrap_or(0)
                    >= from
            })
            && params["to"].as_u64().is_none_or(|to| {
                r["work_time"]
                    .as_u64()
                    .or(r["captured_at"].as_u64())
                    .unwrap_or(u64::MAX)
                    <= to
            })
    });
    rows.sort_by(|a, b| {
        b["captured_at"]
            .as_u64()
            .cmp(&a["captured_at"].as_u64())
            .then_with(|| a["snapshot_id"].as_str().cmp(&b["snapshot_id"].as_str()))
            .then_with(|| a["session_id"].as_str().cmp(&b["session_id"].as_str()))
    });
    if let Some(snapshot_id) = params["snapshot_id"].as_str() {
        let entry = rows
            .iter()
            .find(|r| {
                r["snapshot_id"] == snapshot_id
                    && params["source_project_id"]
                        .as_str()
                        .is_none_or(|pid| r["project_id"] == pid)
            })
            .ok_or_else(|| err("not_found", "该 Agent 没有此快照"))?;
        return read_snapshot(root, entry, snapshot_id);
    }
    let mut features = std::collections::BTreeSet::new();
    rows.retain(|row| {
        row["feature_id"].as_str().is_none_or(|feature| {
            features.insert((row["project_id"].to_string(), feature.to_owned()))
        })
    });
    for row in &mut rows {
        row["snapshot_count"] = json!(
            1 + row["history_count"]
                .as_u64()
                .unwrap_or_else(|| row["history"].as_array().map_or(0, Vec::len) as u64)
        );
        row.as_object_mut().unwrap().remove("history");
    }
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
    Ok(
        json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len(),"next_offset":if offset.saturating_add(limit)<rows.len(){Some(offset.saturating_add(limit))}else{None},"unavailable":unavailable,"scope":i.scope,"agent_revision_id":i.agent_revision_id}),
    )
}

type VisibleEntries = (AgentInstance, Vec<Value>, Vec<Value>);

fn visible_entries(root: &Path, current_project: &str, params: &Value) -> Result<VisibleEntries> {
    let id = devdesk_core::read_models::identifier(params, "agent_instance_id")?;
    let instance = instance(root, current_project, &id)?;
    let snapshot = params["snapshot_id"].as_str();
    let source = params["source_project_id"].as_str();
    let domains = domain_packages::experience_domains(root)?;
    let current_domain = domains.get(current_project);
    let visible = |project: &str| {
        domains.get(project) == current_domain
            && source.is_none_or(|expected| expected == project)
            && (project == GLOBAL_DISCUSSION_NAMESPACE
                || instance.scope.project_id().is_none_or(|pid| pid == project))
    };
    let cached = experience_index::entries(root, None, &id, false, snapshot)?;
    let deleted = experience_index::deleted(root, &id)?;
    let mut rows = cached
        .into_iter()
        .filter(|row| row["project_id"].as_str().is_some_and(visible))
        .map(|mut row| {
            row["availability"] = json!("unavailable");
            row
        })
        .collect::<Vec<_>>();
    let mut unavailable = Vec::new();
    let mut add_source = |pid: &str, name: &str, enrich| {
        if !visible(pid) {
            return;
        }
        match experience_index::entries(root, Some(pid), &id, enrich, snapshot) {
            Ok(entries) => {
                // A reachable source supersedes its cached head, even if the
                // projection has not caught up with the latest capture.
                rows.retain(|row| row["project_id"] != pid);
                rows.extend(entries);
            }
            Err(_) if pid != GLOBAL_DISCUSSION_NAMESPACE => unavailable
                .push(json!({"project_id":pid,"project_name":name,"availability":"unavailable"})),
            Err(_) => {}
        }
    };
    add_source(GLOBAL_DISCUSSION_NAMESPACE, "全局 / 跨项目", false);
    if source != Some(GLOBAL_DISCUSSION_NAMESPACE) {
        for project in projects(root)? {
            add_source(&project.id, &project.title, true);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    rows.reverse();
    rows.retain(|row| {
        let pid = row["project_id"].as_str().unwrap_or_default();
        let sid = row["snapshot_id"].as_str().unwrap_or_default();
        let owner = &row["work_owner"];
        let matches_owner = params["entity_id"].as_str().map_or_else(
            || params["exclude_entity_scope"] != true || owner["kind"] != "entity",
            |entity| owner["kind"] == "entity" && owner["id"] == entity && owner["project"] == pid,
        );
        matches_owner
            && !deleted.contains(&(pid.to_owned(), sid.to_owned()))
            && seen.insert((
                pid.to_owned(),
                sid.to_owned(),
                row["session_id"].to_string(),
            ))
    });
    Ok((instance, rows, unavailable))
}

pub fn entry_visible(root: &Path, current_project: &str, params: &Value) -> Result<bool> {
    let (_, rows, _) = visible_entries(root, current_project, params)?;
    Ok(!rows.is_empty())
}

fn read_snapshot(root: &Path, entry: &Value, snapshot_id: &str) -> Result<Value> {
    use rusqlite::OptionalExtension;

    let pid = devdesk_core::read_models::identifier(entry, "project_id")?;
    let path = experience_index::source_path(root, &pid)?;
    let conn = connection_pool::ManagedConnection::read_only(&path.join("engineering.sqlite3"))?;
    conn.execute_batch("BEGIN;").map_err(storage)?;
    let raw: Value = load_table_filtered_params(&conn, "snapshots", "id=?1", &[&snapshot_id])?
        .pop()
        .ok_or_else(|| err("not_found", "快照内容已不可用"))?;
    let mut state = EngineeringState {
        governance: source_store::load_catalog_records(&conn)?,
        ..Default::default()
    };
    state.refresh_configuration_views()?;
    let snapshot: Snapshot = serde_json::from_value(source_views::restore_with(
        &state,
        &raw,
        &mut Default::default(),
    )?)
    .map_err(storage)?;
    let manifest: Option<(String, Option<String>)> = conn.query_row(
        "SELECT json_extract(data,'$.artifact_id'),json_extract(data,'$.turn_id') FROM governance_records
         WHERE type IN ('conversation_capture_intent','captured_experience_intent') AND json_extract(data,'$.snapshot.id')=?1 AND json_type(data,'$.artifact_id')='text' LIMIT 1",
        [snapshot_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(storage)?;
    let manifests: Vec<Artifact> = match &manifest {
        Some((id, _)) => load_table_filtered_params(&conn, "artifacts", "id=?1", &[id])?,
        None => load_table_filtered_params(
            &conn,
            "artifacts",
            "feature_id IS ?1 AND relative_path LIKE '%/snapshots/%'",
            &[&snapshot.feature_id],
        )?,
    };
    let files = FileArtifacts::new(path);
    let digest = configuration_hash(&snapshot);
    let sealed = manifests
        .into_iter()
        .find(|artifact| {
            files
                .read(artifact)
                .ok()
                .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
                .and_then(|value| {
                    serde_json::from_value::<Snapshot>(
                        value.get("snapshot").unwrap_or(&value).clone(),
                    )
                    .ok()
                })
                .is_some_and(|sealed| configuration_hash(&sealed) == digest)
        })
        .ok_or_else(|| err("blocked", "快照密封清单缺失或哈希验证失败"))?;
    let documents: Vec<Document> = load_table_filtered_params(
        &conn,
        "documents",
        "id IN (SELECT value FROM json_each(?1))",
        &[&json!(snapshot.documents).to_string()],
    )?;
    let mut evidence = std::collections::BTreeSet::from([sealed.id]);
    evidence.extend(documents.iter().filter_map(|d| d.artifact_id.clone()));
    let mut executions = conn.prepare(
        "SELECT protocol_artifact FROM executions WHERE id IN (SELECT value FROM json_each(?1)) AND protocol_artifact IS NOT NULL"
    ).map_err(storage)?;
    evidence.extend(
        executions
            .query_map([json!(snapshot.execution_instances).to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?,
    );
    let captured = serde_json::from_str::<Value>(&snapshot.content).unwrap_or(Value::Null);
    if let Some(messages) = captured["messages"].as_array() {
        evidence.extend(
            messages
                .iter()
                .filter_map(|event| event["artifact_id"].as_str().map(str::to_owned)),
        );
    }
    let artifacts: Vec<Artifact> = load_table_filtered_params(
        &conn,
        "artifacts",
        "id IN (SELECT value FROM json_each(?1))",
        &[&json!(evidence).to_string()],
    )?;
    let mut entry = entry.clone();
    if let Some((_, Some(turn))) = manifest {
        entry["turn_id"] = json!(turn);
    }
    Ok(
        json!({"entry":entry,"snapshot":snapshot,"documents":documents,
        "evidence":artifacts.iter().map(|a|json!({"id":a.id,"path":a.relative_path,"hash":a.sha256})).collect::<Vec<_>>(),"integrity":"verified"}),
    )
}

#[cfg(test)]
fn enrich_entry(state: &EngineeringState, value: &Value) -> Value {
    let mut entry = value.clone();
    entry["availability"] = json!("available");
    let source = state
        .governance
        .iter()
        .find(|r| r.r#type == "configured_session" && value["session_id"] == r.definition_id);
    if let Some(source) = source {
        entry["workflow_instance_id"] = source.data["instance_id"].clone();
        entry["step_execution_id"] = source.data["execution_id"].clone();
        entry["plan_revision_id"] = source.data["plan_revision_id"].clone();
        if entry["plan_revision_id"].is_null() {
            let mut plans = std::collections::BTreeSet::new();
            for row in &state.governance {
                if row.r#type == "workflow_instance" && row.id == source.data["instance_id"] {
                    if let Some(steps) = row.data["steps"].as_array() {
                        for step in steps
                            .iter()
                            .filter(|step| step["id"] == source.data["execution_id"])
                        {
                            if let Some(plan) = step["plan_revision_id"].as_str() {
                                plans.insert(plan.to_owned());
                            }
                        }
                    }
                } else if row.r#type == "workflow_execution_history"
                    && row.data["workflow_instance_id"] == source.data["instance_id"]
                    && row.data["id"] == source.data["execution_id"]
                    && let Some(plan) = row.data["plan_revision_id"].as_str()
                {
                    plans.insert(plan.to_owned());
                }
            }
            if plans.len() == 1 {
                entry["plan_revision_id"] = json!(plans.first());
            }
        }
        if let Some(workflow) = source.data["instance_id"]
            .as_str()
            .and_then(|id| state.configuration(id).ok())
        {
            entry["workflow_name"] = json!(workflow.name);
        }
    }
    if let Some(snapshot) = state
        .snapshots
        .iter()
        .find(|s| value["snapshot_id"] == s.id)
    {
        entry["summary"] = json!(snapshot.content.chars().take(320).collect::<String>());
        entry["content_digest"] = json!(configuration_hash(snapshot));
    }
    entry
}

/// Shared read-only context adapter for UI and Agent calls.
pub struct Reader {
    root: PathBuf,
}
impl Reader {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

/// Agent identities and their immutable dependency closure, read without activation.
pub fn discussion_catalog(
    root: &Path,
    visibility: Option<&domain_packages::ProjectVisibility>,
) -> Result<Vec<ConfigurationRecord>> {
    let includes = |project: &str| visibility.is_none_or(|scope| scope.includes(project));
    let global = GlobalStore::read_configuration(root)?
        .state
        .governance
        .clone();
    let global_instances: std::collections::BTreeSet<_> = global
        .iter()
        .filter(|r| r.r#type == "agent_instance")
        .map(|r| r.id.clone())
        .collect();
    let mut readable_projects = std::collections::BTreeSet::new();
    let mut states = Vec::new();
    for project in projects(root)?
        .into_iter()
        .filter(|project| includes(&project.id))
    {
        if let Ok(catalog) = SqliteStore::read_project_catalog(root, &project.id) {
            readable_projects.insert(project.id.clone());
            states.push(catalog.state.as_ref().clone());
        }
    }
    if let Ok(catalog) = SqliteStore::read_global_discussion_catalog(root) {
        let mut cached = catalog.state.as_ref().clone();
        cached.governance.retain(|r| {
            r.r#type != "agent_instance"
                || r.scope
                    .project_id()
                    .map_or(!global_instances.contains(&r.id), |pid| {
                        includes(pid) && !readable_projects.contains(pid)
                    })
        });
        states.push(cached);
    }
    let shared = EngineeringState {
        governance: global,
        ..Default::default()
    };
    states.push(shared);
    let mut rows: std::collections::BTreeMap<String, ConfigurationRecord> =
        std::collections::BTreeMap::new();
    for state in states {
        for instance in state.governance.iter().filter(|r| {
            r.r#type == "agent_instance" && !devdesk_core::configuration_retired(&state, r)
        }) {
            let mut pending = vec![
                instance.id.clone(),
                instance.data["agent_revision_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            ];
            while let Some(id) = pending.pop() {
                if let Some(existing) = rows.get(&id) {
                    require(
                        existing.content_hash == state.configuration(&id)?.content_hash,
                        "同一配置引用在来源库中的内容冲突",
                    )?;
                    continue;
                }
                let record = state.configuration(&id)?;
                for edge in configuration_edges(record)? {
                    if edge.relation != "content_digest" {
                        pending.push(edge.to);
                    }
                }
                for package in state
                    .governance
                    .iter()
                    .filter(|r| r.r#type == "skill_package" && r.definition_id == record.id)
                {
                    rows.insert(package.id.clone(), package.clone());
                }
                rows.insert(id, record.clone());
            }
        }
    }
    Ok(rows.into_values().collect())
}
impl devdesk_core::ports::AgentContextReader for Reader {
    fn project_knowledge(
        &self,
        instance_id: &str,
        origin: Option<&str>,
        target: Option<&str>,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let project =
            target.ok_or_else(|| err("context_required", "读取项目资料需要明确的当前项目"))?;
        let identity = instance(&self.root, origin.unwrap_or(project), instance_id)?;
        require(
            identity.scope.visible_from(&Scope::project(project)),
            "该实例不能读取其他项目资料",
        )?;
        require(
            params["project_id"].as_str().is_none_or(|id| id == project),
            "工具参数不能改变当前项目范围",
        )?;
        let catalog = SqliteStore::read_project_catalog(&self.root, project)?;
        let path = SqliteStore::project_path(&self.root, project)?;
        let pool = connection_pool::ConnectionPool::read_only(&path.join("engineering.sqlite3"))?;
        if matches!(
            method,
            "project.overview" | "project.records.list" | "project.record.read"
        ) {
            let selected = project_records::read(&pool, &catalog, method, params)?;
            return if method == "project.record.read" {
                devdesk_core::render_project_record(&selected, params)
            } else {
                Ok(selected)
            };
        }
        let files = FileArtifacts::new(path);
        if method == "project.artifact.read" {
            let key = devdesk_core::concurrency::ResourceKey::new(
                "artifact",
                params["id"]
                    .as_str()
                    .ok_or_else(|| err("invalid_params", "需要产物 ID"))?,
            );
            let row = resource_commits::read(&pool, &[key])?.remove(0);
            let artifact: Artifact =
                serde_json::from_value(row.value.ok_or_else(|| err("not_found", "产物不存在"))?)
                    .map_err(storage)?;
            return devdesk_core::render_project_artifact(
                &artifact,
                files.read(&artifact)?,
                params,
            );
        }
        devdesk_core::read_project_knowledge(
            &catalog.state,
            &files,
            json!({"available":false}),
            method,
            project,
            params,
        )
    }

    fn experience(&self, instance_id: &str, origin: Option<&str>, params: &Value) -> Result<Value> {
        let mut params = params.clone();
        require(params.is_object(), "经验查询参数必须为对象")?;
        params["agent_instance_id"] = json!(instance_id);
        if params["snapshot_id"].is_null() && params["project_id"].is_string() {
            params["source_project_id"] = params["project_id"].clone();
        }
        let mut detail = query(
            &self.root,
            origin.unwrap_or(GLOBAL_DISCUSSION_NAMESPACE),
            &params,
        )?;
        if params["snapshot_id"].is_null() {
            return Ok(detail);
        }
        if let Some(document) = params["document_id"].as_str() {
            let value = detail["documents"]
                .as_array()
                .and_then(|rows| rows.iter().find(|r| r["id"] == document))
                .cloned()
                .ok_or_else(|| err("not_found", "文档不属于此经验快照"))?;
            detail =
                json!({"entry":detail["entry"],"integrity":detail["integrity"],"document":value});
        } else if let Some(artifact) = params["artifact_id"].as_str() {
            require(
                detail["evidence"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["id"] == artifact)),
                "证据不属于此经验来源",
            )?;
            let project = detail["entry"]["project_id"]
                .as_str()
                .ok_or_else(|| err("storage", "经验来源缺失"))?;
            let path = experience_index::source_path(&self.root, project)?;
            let conn =
                connection_pool::ManagedConnection::read_only(&path.join("engineering.sqlite3"))?;
            let artifact: Artifact =
                load_table_filtered_params(&conn, "artifacts", "id=?1", &[&artifact])?
                    .pop()
                    .ok_or_else(|| err("not_found", "历史证据不可用"))?;
            let bytes = FileArtifacts::new(path).read(&artifact)?;
            let (content, encoding) = match String::from_utf8(bytes) {
                Ok(text) => (text, "utf-8"),
                Err(error) => {
                    use base64::Engine as _;
                    (
                        base64::engine::general_purpose::STANDARD.encode(error.into_bytes()),
                        "base64",
                    )
                }
            };
            detail = json!({"entry":detail["entry"],"integrity":"verified","artifact":{"id":artifact.id,"sha256":artifact.sha256,"media_type":artifact.media_type,"encoding":encoding,"content":content}});
        }
        let body = serde_json::to_string(&detail).map_err(storage)?;
        let mut offset = (params["offset"].as_u64().unwrap_or(0) as usize).min(body.len());
        while !body.is_char_boundary(offset) {
            offset -= 1;
        }
        let mut end = offset
            .saturating_add(
                params["max_bytes"]
                    .as_u64()
                    .unwrap_or(16384)
                    .clamp(1024, 65536) as usize,
            )
            .min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        Ok(
            json!({"entry":detail["entry"],"integrity":detail["integrity"],"content_digest":configuration_hash(&detail),
            "content_chunk":&body[offset..end],"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"total_bytes":body.len()}),
        )
    }

    fn workflow_outputs(
        &self,
        instance_id: &str,
        origin: Option<&str>,
        params: &Value,
    ) -> Result<Value> {
        let identity = instance(
            &self.root,
            origin.unwrap_or(GLOBAL_DISCUSSION_NAMESPACE),
            instance_id,
        )?;
        let workflow = params["instance_id"]
            .as_str()
            .ok_or_else(|| err("context_required", "读取历史产物需要 instance_id"))?;
        for project in projects(&self.root)? {
            if identity
                .scope
                .project_id()
                .is_some_and(|id| id != project.id)
            {
                continue;
            }
            let path = SqliteStore::project_path(&self.root, &project.id)?;
            let pool =
                match connection_pool::ConnectionPool::read_only(&path.join("engineering.sqlite3"))
                {
                    Ok(pool) => pool,
                    Err(_) => continue,
                };
            let catalog = SqliteStore::read_project_catalog(&self.root, &project.id)?;
            let conn = pool.read()?;
            conn.execute_batch("BEGIN;").map_err(storage)?;
            let selected = resource_commits::read_on(
                &conn,
                &[devdesk_core::concurrency::ResourceKey::new(
                    "workflow_run_authority",
                    workflow,
                )],
            )?
            .remove(0);
            let Some(run) = selected.value else {
                continue;
            };
            let belongs:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records c JOIN sessions s ON s.id=c.definition_id WHERE c.type='configured_session' AND json_extract(c.data,'$.instance_id')=?1 AND coalesce(json_extract(s.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(s.runtime_binding,'$.agent_configuration.agent_instance_id'))=?2)",rusqlite::params![workflow,instance_id],|r|r.get(0)).map_err(storage)?;
            require(belongs, "该 Agent 没有此工作流的历史参与记录")?;
            let ids: Vec<Id> =
                serde_json::from_value(params.get("ids").cloned().unwrap_or(json!([])))
                    .map_err(storage)?;
            require(ids.len() <= 100, "最多读取 100 项原产物")?;
            let budget = params["max_bytes"]
                .as_u64()
                .unwrap_or(16384)
                .clamp(1024, 65536) as usize
                / ids.len().max(1);
            let mut outputs = vec![];
            for id in ids {
                let selected = load_table_filtered_params::<ConfigurationRecord>(
                    &conn,
                    "governance_records",
                    "id=?1",
                    &[&id],
                )?
                .pop()
                .ok_or_else(|| err("not_found", "原产物不存在"))?;
                require(
                    selected.immutable
                        && selected.scope.project_id() == Some(project.id.as_str())
                        && matches!(
                            selected.r#type.as_str(),
                            "workflow_output" | "typed_material"
                        ),
                    "原产物跨项目或类型不符",
                )?;
                let same_root = if selected.r#type == "typed_material" {
                    selected.data["root_instance_id"] == run["root_instance_id"]
                } else {
                    conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records w WHERE w.type='workflow_instance' AND json_extract(w.data,'$.root_instance_id')=?1 AND (EXISTS(SELECT 1 FROM json_each(w.data,'$.steps') e WHERE json_extract(e.value,'$.id')=?2) OR EXISTS(SELECT 1 FROM governance_records h WHERE h.type='workflow_execution_history' AND json_extract(h.data,'$.workflow_instance_id')=w.id AND json_extract(h.data,'$.id')=?2)))",rusqlite::params![run["root_instance_id"].as_str(),selected.data["execution_id"].as_str()],|r|r.get::<_,bool>(0)).map_err(storage)?
                };
                require(same_root, "原产物不属于当前流程根")?;
                let row: ConfigurationRecord = ConfigurationRecord::from_storage(
                    source_views::restore(&catalog.state, &json!(selected))?,
                )?;
                let value = row.data["value"]
                    .pointer(params["pointer"].as_str().unwrap_or(""))
                    .ok_or_else(|| err("invalid_params", "产物字段不存在"))?;
                let body = value.to_string();
                let mut offset = (params["offset"].as_u64().unwrap_or(0) as usize).min(body.len());
                while !body.is_char_boundary(offset) {
                    offset -= 1;
                }
                if body.len() > budget || offset > 0 {
                    let mut end = (offset + budget).min(body.len());
                    while !body.is_char_boundary(end) {
                        end -= 1;
                    }
                    let mut data = row.data.clone();
                    data.as_object_mut().unwrap().remove("value");
                    outputs.push(json!({"id":row.id,"data":data,"value_chunk":&body[offset..end],"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"total_bytes":body.len()}));
                } else {
                    let mut raw = json!(row);
                    raw["data"]["value"] = value.clone();
                    outputs.push(raw);
                }
            }
            conn.execute_batch("COMMIT;").map_err(storage)?;
            return Ok(
                json!({"outputs":outputs,"source":{"project_id":project.id,"workflow_instance_id":workflow}}),
            );
        }
        Err(err("not_found", "历史工作流不存在或来源项目不可用"))
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    fn fact(kind: &str, key: &str, definition: &str, data: Value) -> ConfigurationRecord {
        ConfigurationRecord {
            id: key.into(),
            r#type: kind.into(),
            definition_id: definition.into(),
            scope: Scope::global(),
            name: kind.into(),
            version: 1,
            supersedes: None,
            content_hash: configuration_hash(&data),
            refs: References::new(),
            data,
            immutable: true,
            revision: 1,
            created_at: 0,
        }
    }
    #[test]
    fn legacy_experience_uses_exact_execution_plan_and_keeps_ambiguous_sources_unknown() {
        let mut state = EngineeringState::default();
        state.governance.push(fact(
            "configured_session",
            "link",
            "old-session",
            json!({"instance_id":"workflow","execution_id":"old-step"}),
        ));
        state.governance.push(fact(
            "workflow_instance",
            "workflow",
            "workflow",
            json!({"active_plan_revision_id":"latest-plan","steps":[]}),
        ));
        state.governance.push(fact("workflow_execution_history","history","old-step",json!({"id":"old-step","workflow_instance_id":"workflow","plan_revision_id":"original-plan"})));
        let entry = json!({"session_id":"old-session"});
        assert_eq!(
            enrich_entry(&state, &entry)["plan_revision_id"],
            "original-plan"
        );
        state.governance.push(fact("workflow_execution_history","conflicting-history","old-step",json!({"id":"old-step","workflow_instance_id":"workflow","plan_revision_id":"other-plan"})));
        assert!(enrich_entry(&state, &entry)["plan_revision_id"].is_null());
    }
}
