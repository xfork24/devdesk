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
    let id = params["agent_instance_id"]
        .as_str()
        .ok_or_else(|| err("invalid_params", "需要 Agent"))?;
    let i = instance(root, current_project, id)?;
    let mut rows: Vec<Value> = experience_index::entries(root, None, id, false)?
        .into_iter()
        .filter(|r| {
            i.scope
                .project_id()
                .is_none_or(|pid| r["project_id"] == pid)
        })
        .map(|mut data| {
            data["availability"] = json!("unavailable");
            data
        })
        .collect();
    if let Ok(entries) =
        experience_index::entries(root, Some(GLOBAL_DISCUSSION_NAMESPACE), id, false)
    {
        rows.extend(entries);
    }
    let mut unavailable = vec![];
    for project in projects(root)? {
        if i.scope.project_id().is_some_and(|pid| pid != project.id) {
            continue;
        }
        match experience_index::entries(root,Some(&project.id),id,true){Ok(entries)=>{
            rows.extend(entries);
        },Err(_)=>unavailable.push(json!({"project_id":project.id,"project_name":project.title,"availability":"unavailable"}))}
    }
    let mut seen = std::collections::BTreeSet::new();
    rows.reverse();
    rows.retain(|r| {
        seen.insert((
            r["project_id"].to_string(),
            r["snapshot_id"].to_string(),
            r["session_id"].to_string(),
        ))
    });
    // Historical snapshot references remain readable even though a Feature
    // has one current experience entry for this Agent.
    if let Some(snapshot) = params["snapshot_id"].as_str() {
        let historical: Vec<_> = rows
            .iter()
            .flat_map(|row| {
                row["history"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|entry| entry["snapshot_id"] == snapshot)
                    .cloned()
            })
            .collect();
        rows.extend(historical);
    }
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
        let pid = entry["project_id"].as_str().unwrap();
        let global = pid == GLOBAL_DISCUSSION_NAMESPACE;
        let s = if global {
            SqliteStore::read_global_discussion_snapshot(root)?
        } else {
            SqliteStore::read_project_snapshot(root, pid)?
        };
        let snapshot = s
            .snapshots
            .iter()
            .find(|snap| snap.id == snapshot_id)
            .ok_or_else(|| err("not_found", "快照内容已不可用"))?;
        // Snapshot manifests use artifact IDs in paths; find the matching sealed manifest by content.
        let files = FileArtifacts::new(if global {
            SqliteStore::global_discussion_path(root)?
        } else {
            SqliteStore::project_path(root, pid)?
        });
        let mut evidence = std::collections::BTreeSet::new();
        for a in s.artifacts.iter().filter(|a| {
            a.relative_path.contains("/snapshots/") && a.feature_id == snapshot.feature_id
        }) {
            if let Ok(body) = files.read(a)
                && serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| {
                        serde_json::from_value::<Snapshot>(v.get("snapshot").unwrap_or(&v).clone())
                            .ok()
                    })
                    .is_some_and(|sealed| {
                        configuration_hash(&sealed) == configuration_hash(snapshot)
                    })
            {
                evidence.insert(a.id.clone());
                break;
            }
        }
        require(!evidence.is_empty(), "快照密封清单缺失或哈希验证失败")?;
        evidence.extend(
            s.documents
                .iter()
                .filter(|d| snapshot.documents.contains(&d.id))
                .filter_map(|d| d.artifact_id.clone()),
        );
        evidence.extend(
            s.executions
                .iter()
                .filter(|e| snapshot.execution_instances.contains(&e.id))
                .filter_map(|e| e.protocol_artifact.clone()),
        );
        let captured = serde_json::from_str::<Value>(&snapshot.content).unwrap_or(Value::Null);
        if let Some(messages) = captured["messages"].as_array() {
            evidence.extend(
                messages
                    .iter()
                    .filter_map(|event| event["artifact_id"].as_str().map(str::to_owned)),
            );
        }
        return Ok(
            json!({"entry":entry,"snapshot":snapshot,"documents":s.documents.iter().filter(|d|snapshot.documents.contains(&d.id)).collect::<Vec<_>>(),"evidence":s.artifacts.iter().filter(|a|evidence.contains(&a.id)).map(|a|json!({"id":a.id,"path":a.relative_path,"hash":a.sha256})).collect::<Vec<_>>(),"integrity":"verified"}),
        );
    }
    let mut features = std::collections::BTreeSet::new();
    rows.retain(|row| {
        row["feature_id"].as_str().is_none_or(|feature| {
            features.insert((row["project_id"].to_string(), feature.to_owned()))
        })
    });
    for row in &mut rows {
        row["snapshot_count"] = json!(1 + row["history"].as_array().map_or(0, Vec::len));
        row.as_object_mut().unwrap().remove("history");
    }
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
    Ok(
        json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len(),"next_offset":if offset.saturating_add(limit)<rows.len(){Some(offset.saturating_add(limit))}else{None},"unavailable":unavailable,"scope":i.scope,"agent_revision_id":i.agent_revision_id}),
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
pub fn discussion_catalog(root: &Path) -> Result<Vec<ConfigurationRecord>> {
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
    for project in projects(root)? {
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
                        !readable_projects.contains(pid)
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
        let state = SqliteStore::read_project_snapshot(&self.root, project)?;
        let files = FileArtifacts::new(SqliteStore::project_path(&self.root, project)?);
        devdesk_core::read_project_knowledge(
            &state,
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
            let (state, path) = if project == GLOBAL_DISCUSSION_NAMESPACE {
                (
                    SqliteStore::read_global_discussion_snapshot(&self.root)?,
                    SqliteStore::global_discussion_path(&self.root)?,
                )
            } else {
                (
                    SqliteStore::read_project_snapshot(&self.root, project)?,
                    SqliteStore::project_path(&self.root, project)?,
                )
            };
            let artifact = state
                .artifacts
                .iter()
                .find(|r| r.id == artifact)
                .ok_or_else(|| err("not_found", "历史证据不可用"))?;
            let bytes = FileArtifacts::new(path).read(artifact)?;
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
            let state = match SqliteStore::read_project_snapshot(&self.root, &project.id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let Some(row) = state
                .governance
                .iter()
                .find(|r| r.id == workflow && r.r#type == "workflow_instance")
            else {
                continue;
            };
            let belongs = state
                .governance
                .iter()
                .filter(|r| r.r#type == "configured_session" && r.data["instance_id"] == workflow)
                .any(|r| {
                    state.sessions.iter().any(|s| {
                        s.id == r.definition_id
                            && s.runtime_binding
                                .as_ref()
                                .and_then(|b| b.agent_configuration.as_ref())
                                .is_some_and(|a| a.agent_instance_id == instance_id)
                    })
                });
            require(belongs, "该 Agent 没有此工作流的历史参与记录")?;
            let instance: devdesk_core::ConfiguredWorkflowInstance = row.decode()?;
            let mut result = devdesk_core::read_workflow_outputs(&state, &instance, params)?;
            result["source"] = json!({"project_id":project.id,"workflow_instance_id":workflow});
            return Ok(result);
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
