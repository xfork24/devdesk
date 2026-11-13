//! Read-only, AgentID-scoped projection over authoritative project stores.
use super::*;
use rusqlite::OpenFlags;

fn global_records(root: &Path) -> Result<Vec<ConfigurationRecord>> {
    let conn = Connection::open_with_flags(
        root.join("global.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(storage)?;
    load_table::<Value>(&conn, "governance_records")?
        .into_iter()
        .map(ConfigurationRecord::from_storage)
        .collect()
}
fn projects(root: &Path) -> Result<Vec<Project>> {
    let conn = Connection::open_with_flags(
        root.join("global.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(storage)?;
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
    let mut records = global_records(root)?;
    if let Ok(s) = SqliteStore::read_project_snapshot(root, current_project) {
        records.extend(s.governance);
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
    let mut rows: Vec<Value> = global_records(root)?
        .into_iter()
        .filter(|r| {
            r.r#type == "experience"
                && r.data["agent_instance_id"] == id
                && i.scope
                    .project_id()
                    .is_none_or(|pid| r.data["project_id"] == pid)
        })
        .map(|r| {
            let mut data = r.data;
            data["availability"] = json!("unavailable");
            data
        })
        .collect();
    if i.scope == Scope::global() {
        if let Ok(s) = SqliteStore::read_global_discussion_snapshot(root) {
            rows.extend(
                s.governance
                    .into_iter()
                    .filter(|r| r.r#type == "experience" && r.data["agent_instance_id"] == id)
                    .map(|r| {
                        let mut data = r.data;
                        data["availability"] = json!("available");
                        data
                    }),
            );
        }
    }
    let mut unavailable = vec![];
    for project in projects(root)? {
        if i.scope.project_id().is_some_and(|pid| pid != project.id) {
            continue;
        }
        match SqliteStore::read_project_snapshot(root,&project.id){Ok(s)=>{
            rows.extend(s.governance.into_iter().filter(|r|r.r#type=="experience"&&r.data["agent_instance_id"]==id).map(|r|{let mut data=r.data;data["availability"]=json!("available");data}));
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
    rows.retain(|r| {
        params["project_name"].as_str().is_none_or(|name| {
            r["project_name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&name.to_lowercase())
        }) && params["feature_name"].as_str().is_none_or(|name| {
            r["feature_name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&name.to_lowercase())
        }) && params["from"].as_u64().is_none_or(|from| {
            r["work_time"]
                .as_u64()
                .or(r["captured_at"].as_u64())
                .unwrap_or(0)
                >= from
        }) && params["to"].as_u64().is_none_or(|to| {
            r["work_time"]
                .as_u64()
                .or(r["captured_at"].as_u64())
                .unwrap_or(u64::MAX)
                <= to
        })
    });
    rows.sort_by_key(|r| std::cmp::Reverse(r["captured_at"].as_u64().unwrap_or(0)));
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
        let mut integrity = false;
        for a in s.artifacts.iter().filter(|a| {
            a.relative_path.contains("/snapshots/") && a.feature_id == snapshot.feature_id
        }) {
            if let Ok(body) = files.read(a) {
                if serde_json::from_slice::<Value>(&body)
                    .ok()
                    .is_some_and(|v| v["snapshot"]["id"] == snapshot_id || v["id"] == snapshot_id)
                {
                    integrity = true;
                    break;
                }
            }
        }
        require(integrity, "快照密封清单缺失或哈希验证失败")?;
        return Ok(
            json!({"entry":entry,"snapshot":snapshot,"documents":s.documents.iter().filter(|d|snapshot.documents.contains(&d.id)).collect::<Vec<_>>(),"evidence":s.artifacts.iter().filter(|a|a.feature_id==snapshot.feature_id&&a.task_id==snapshot.task_id).map(|a|json!({"id":a.id,"path":a.relative_path,"hash":a.sha256})).collect::<Vec<_>>(),"integrity":"verified"}),
        );
    }
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
    Ok(
        json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len(),"unavailable":unavailable,"scope":i.scope,"agent_revision_id":i.agent_revision_id}),
    )
}
