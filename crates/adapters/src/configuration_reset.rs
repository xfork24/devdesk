//! Explicit destructive catalog reset; execution evidence becomes self-contained.
use super::*;
use devdesk_core::concurrency::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare(
    pool: &connection_pool::ConnectionPool,
    project: bool,
    records: &[ConfigurationRecord],
    expected_catalog: u64,
) -> Result<ConfigurationPublication> {
    let mut keys = vec![
        ResourceKey::new("catalog_clock", "configuration"),
        ResourceKey::new("configuration_reset", "factory"),
    ];
    for table in ["governance", "sessions", "turns", "executions", "snapshots"] {
        keys.push(ResourceKey::new(
            "collection",
            format!("record-summary:{table}"),
        ));
    }
    let _span = performance::Span::for_database("catalog.reset_prepare", pool.observation_id());
    let conn = pool.maintenance_read()?;
    let transaction = conn.unchecked_transaction().map_err(storage)?;
    let _payloads = payload_store::reset_read_scope(&transaction);
    let guards = resource_commits::read_on(&transaction, &keys)?;
    if guards[0].revision != expected_catalog {
        return Err(err("revision_conflict", "配置目录已变化，请重新预览").technical_not_applied());
    }
    let load = performance::Span::new("catalog.reset_load");
    let mut before = EngineeringState {
        governance: source_store::load_reset_records(&transaction, records)?,
        ..Default::default()
    };
    if project {
        before.projects = load_table(&transaction, "projects")?;
        before.conversation_events = load_table_filtered(
            &transaction,
            "conversation_events",
            "kind='user' AND turn_id IN (
                SELECT definition_id FROM governance_records
                WHERE type IN ('configured_execution_task','archived_configured_execution_task')
            )",
        )?;
    }
    drop(load);
    let archive = performance::Span::new("catalog.reset_archive");
    before.refresh_configuration_views()?;
    let sources = source_views::ResetSources::new(&before);
    let mut memo = BTreeMap::new();
    let available: BTreeSet<_> = records
        .iter()
        .filter(|r| r.is_definition())
        .map(|r| r.id.as_str())
        .collect();
    let historical = if project {
        historical_sources(&transaction, &before, records, &available)?
    } else {
        Vec::new()
    };
    let mut bindings = BTreeMap::new();
    for row in before.governance.iter().filter(|r| {
        matches!(
            r.r#type.as_str(),
            "project_configuration" | "work_configuration" | "integration_project_configuration"
        )
    }) {
        let key = (row.r#type.clone(), row.definition_id.clone());
        if bindings
            .get(&key)
            .is_none_or(|old: &&ConfigurationRecord| row.version > old.version)
        {
            bindings.insert(key, row);
        }
    }
    let mut retained = Vec::new();
    for row in bindings.into_values() {
        let mut row = row.clone();
        if source_views::uses_source_codec(&row.data) {
            row.data = sources.restore(&row.data, &mut memo)?;
            row.content_hash = content_digest(&row.refs, &row.data);
        }
        if configuration_edges(&row).is_ok_and(|edges| {
            edges
                .iter()
                .all(|edge| available.contains(edge.to.as_str()))
        }) {
            retained.push(row);
        }
    }
    let retained_ids = retained
        .iter()
        .map(|row| row.id.as_str())
        .collect::<BTreeSet<_>>();
    let removed = before
        .governance
        .iter()
        .filter(|row| {
            (source_store::is_source(row) || row.r#type == "work_configuration")
                && !available.contains(row.id.as_str())
                && !retained_ids.contains(row.id.as_str())
        })
        .map(|row| row.id.as_str())
        .collect::<BTreeSet<_>>();
    let indexed = source_dependencies::enabled(&transaction)?;
    let external_root = if payload_store::enabled(&transaction)? {
        project_storage::connection_root(&transaction)
    } else {
        None
    };
    let mut dependencies = BTreeMap::new();
    let evidence = payload_store::stage(|| -> Result<Vec<Value>> {
        let mut evidence = Vec::new();
        for original in before
            .governance
            .iter()
            .filter(|row| !catalog_kind(&row.r#type) && row.r#type != "template_reset_receipt")
        {
            let may_pause = original.r#type == "workflow_instance"
                && !matches!(
                    original.data["status"].as_str(),
                    Some("completed" | "cancelled")
                );
            if !may_pause && !source_views::uses_source_codec(&original.data) {
                continue;
            }
            let mut row = original.clone();
            let request = if matches!(
                row.r#type.as_str(),
                "workflow_pending_decision" | "workflow_human_decision"
            ) {
                row.data
                    .as_object_mut()
                    .and_then(|data| data.remove("request"))
            } else {
                None
            };
            let encoded = source_views::uses_source_codec(&row.data);
            if encoded {
                row.data = sources.restore(&row.data, &mut memo)?;
            }
            let pause = may_pause
                && [
                    &row.data["workflow_ref"],
                    &row.data["plan"],
                    &row.data["role_agent_bindings"],
                ]
                .into_iter()
                .any(|value| references_removed(value, &removed));
            if !encoded && !pause {
                continue;
            }
            if pause {
                row.data["paused"] = json!(true);
                row.data["configuration_reset"] = json!(true);
                row.revision += 1;
            }
            source_views::seal_reset_evidence(&mut row.data);
            if let Some(request) = request {
                row.data
                    .as_object_mut()
                    .ok_or_else(|| err("storage", "决定记录需要对象正文"))?
                    .insert("request".into(), request);
            }
            row.content_hash = content_digest(&row.refs, &row.data);
            if indexed {
                dependencies.insert(row.id.clone(), source_dependencies::classify(&row.data));
            }
            if let Some(root) = &external_root {
                payload_store::governance(root, &row.r#type, &mut row.data, None)?;
            }
            evidence.push(row.to_storage());
        }
        Ok(evidence)
    })?;

    drop(archive);
    let _runtime = performance::Span::new("catalog.reset_runtime");
    let mut catalog = records.to_vec();
    catalog.extend(retained);
    catalog.extend(historical);
    let mut changes = json!({"records":catalog,"evidence":evidence,"dependencies":dependencies});
    if project {
        for table in ["sessions", "turns", "executions", "snapshots"] {
            let mut updated = Vec::new();
            for row in load_rows::<Value>(
                &transaction,
                table,
                &runtime_source_filter(&transaction, table)?,
                &[],
                false,
            )? {
                if source_views::uses_source_codec(&row) {
                    let mut restored = sources.restore(&row, &mut memo)?;
                    source_views::seal_reset_evidence(&mut restored);
                    updated.push(restored);
                }
            }
            changes[table] = json!(updated);
        }
    }
    transaction.commit().map_err(storage)?;
    Ok(ConfigurationPublication {
        read_set: ReadSet::new(
            guards
                .into_iter()
                .map(|r| ResourceVersion {
                    resource: r.resource,
                    revision: r.revision,
                })
                .collect(),
        )?,
        changes: vec![ResourceChange {
            resource: ResourceKey::new("configuration_reset", "factory"),
            value: changes,
        }],
    })
}

fn runtime_source_filter(conn: &Connection, table: &str) -> Result<String> {
    let columns = columns(conn, table)?;
    Ok(columns
        .iter()
        .filter(|(_, kind)| matches!(kind.as_str(), "TEXT" | "JSON"))
        .map(|(column, _)| {
            format!(
                r#"instr("{column}", 'devdesk-source-text:')>0 OR instr("{column}", '"$source"')>0"#
            )
        })
        .collect::<Vec<_>>()
        .join(" OR "))
}

/// Sessions keep their original Agent revision through an actual SQLite foreign key.
/// Retain its source closure as historical catalog members so reopening can also
/// resolve the original Role, Capability and their dependencies.
fn historical_sources(
    conn: &Connection,
    before: &EngineeringState,
    incoming: &[ConfigurationRecord],
    available: &BTreeSet<&str>,
) -> Result<Vec<ConfigurationRecord>> {
    let mut statement = conn
        .prepare("SELECT DISTINCT agent FROM sessions")
        .map_err(storage)?;
    let mut pending = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let mut retained = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if available.contains(reference.as_str()) || !retained.insert(reference.clone()) {
            continue;
        }
        let source = before.configuration(&reference)?;
        require(source.is_definition(), "历史会话引用的配置定义缺失")?;
        pending.extend(configuration_edges(source)?.into_iter().map(|edge| edge.to));
    }
    let generation = incoming
        .iter()
        .find(|row| row.id == TEMPLATE_CATALOG_STATE)
        .and_then(|row| row.data["generation"].as_u64())
        .unwrap_or(TEMPLATE_INITIAL_GENERATION)
        .saturating_sub(1);
    let mut history = Vec::new();
    for source in before
        .governance
        .iter()
        .filter(|row| retained.contains(&row.id))
    {
        let kind = template_member_kind(&source.r#type);
        let old = template_catalog_member(before, source);
        let data = json!({
            "kind":kind,"generation":generation,"retained_for_sessions":true,
            "template_key":old.map(|row| &row.data["template_key"]),
            "source_hash":old.map(|row| &row.data["source_hash"]),
            "origin":old.map(|row| &row.data["origin"]),
        });
        let refs = References::new();
        history.push(source.clone());
        history.push(ConfigurationRecord {
            id: format!(
                "template-history-member:{}",
                configuration_hash(&(kind, &source.id, generation))
            ),
            r#type: "template_catalog_member".into(),
            definition_id: source.id.clone(),
            scope: source.scope.clone(),
            name: source.name.clone(),
            version: 1,
            supersedes: None,
            content_hash: content_digest(&refs, &data),
            refs,
            data,
            immutable: true,
            revision: 1,
            created_at: 0,
        });
    }
    let incoming_ids = incoming
        .iter()
        .map(|row| row.id.as_str())
        .collect::<BTreeSet<_>>();
    history.extend(
        before
            .governance
            .iter()
            .filter(|row| {
                row.r#type == "skill_package"
                    && retained.contains(&row.definition_id)
                    && !incoming_ids.contains(row.id.as_str())
            })
            .cloned(),
    );
    Ok(history)
}

pub(super) fn apply(conn: &Connection, value: &Value) -> Result<()> {
    require(!conn.is_autocommit(), "完全重置配置必须位于原子事务中")?;
    // DDL is transactional in SQLite. The explicit reset temporarily suspends
    // only deletion/codec guards and restores their exact definitions before commit.
    let mut query = conn.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name IN ('source_no_delete','governance_no_delete','governance_immutable_update','workflow_intent_basis_immutable','turn_identity_immutable','session_authority_immutable','execution_owner_immutable','snapshot_immutable_update')").map_err(storage)?;
    let guards = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    drop(query);
    for (name, _) in &guards {
        conn.execute_batch(&format!("DROP TRIGGER \"{name}\";"))
            .map_err(storage)?;
    }
    // Keep resource epochs monotonic even when an explicit reset removes and
    // later recreates the same logical source reference.
    conn.execute(
        "UPDATE resource_versions SET revision=revision+1 WHERE kind IN ('source','source_head')",
        [],
    )
    .map_err(storage)?;
    conn.execute("INSERT OR IGNORE INTO resource_versions(kind,resource_id,revision) SELECT 'source',reference,2 FROM source_definitions", []).map_err(storage)?;
    conn.execute("INSERT OR IGNORE INTO resource_versions(kind,resource_id,revision) SELECT 'source_head',identity,max(revision)+1 FROM source_definitions GROUP BY identity", []).map_err(storage)?;
    let records: Vec<ConfigurationRecord> =
        serde_json::from_value(value["records"].clone()).map_err(storage)?;
    let mut kinds = CATALOG_RECORD_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect::<BTreeSet<_>>();
    kinds.extend([
        "permission_binding".into(),
        "permission_assignment".into(),
        "template_reset_receipt".into(),
    ]);
    for kind in kinds {
        conn.execute("DELETE FROM governance_records WHERE type=?1", [&kind])
            .map_err(storage)?;
    }
    conn.execute("DELETE FROM source_definitions", [])
        .map_err(storage)?;
    for row in records {
        if source_store::is_source(&row) {
            conn.execute("INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES(?1,?2,?3,?4,?5,?6,?7)", rusqlite::params![row.definition_id,row.version,row.r#type,row.name,serde_json::to_string(&row.scope).map_err(storage)?,serde_json::to_string(&row.refs).map_err(storage)?,row.data.to_string()]).map_err(storage)?;
        } else {
            write_rows(conn, "governance_records", &[row.to_storage()])?;
        }
    }
    for table in [
        "sessions",
        "turns",
        "executions",
        "snapshots",
        "governance_records",
    ] {
        let field = if table == "governance_records" {
            "evidence"
        } else {
            table
        };
        if let Some(rows) = value[field].as_array().filter(|rows| !rows.is_empty()) {
            write_rows(conn, table, rows)?;
            if table == "governance_records" {
                for row in rows.iter().filter(|row| row["type"] == "workflow_instance") {
                    let id = row["id"]
                        .as_str()
                        .ok_or_else(|| err("storage", "流程证据缺少身份"))?;
                    workflow_projection::refresh_after_reset(conn, id)?;
                }
            }
        }
    }
    if let Some(dependencies) = value["dependencies"].as_object() {
        for row in value["evidence"].as_array().into_iter().flatten() {
            let id = row["id"]
                .as_str()
                .ok_or_else(|| err("storage", "治理证据缺少身份"))?;
            if let Some(dependencies) = dependencies.get(id) {
                let digest = row["content_hash"]
                    .as_str()
                    .ok_or_else(|| err("storage", "治理证据缺少内容摘要"))?;
                let references: Option<BTreeSet<String>> =
                    serde_json::from_value(dependencies.clone()).map_err(storage)?;
                source_dependencies::write(conn, id, digest, references.as_ref())?;
            }
        }
    }
    for (_, sql) in guards {
        conn.execute_batch(&sql).map_err(storage)?;
    }
    Ok(())
}

fn references_removed(value: &Value, removed: &BTreeSet<&str>) -> bool {
    match value {
        Value::String(value) => removed.contains(value.as_str()),
        Value::Array(values) => values
            .iter()
            .any(|value| references_removed(value, removed)),
        Value::Object(values) => values
            .values()
            .any(|value| references_removed(value, removed)),
        _ => false,
    }
}

fn write_rows(conn: &Connection, table: &str, rows: &[Value]) -> Result<()> {
    let cols = columns(conn, table)?;
    let names = cols
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>();
    let placeholders = (1..=cols.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>();
    let updates = names
        .iter()
        .filter(|name| name.as_str() != "\"id\"")
        .map(|name| format!("{name}=excluded.{name}"))
        .collect::<Vec<_>>();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        names.join(","),
        placeholders.join(","),
        updates.join(",")
    );
    let mut statement = conn.prepare_cached(&sql).map_err(storage)?;
    for row in rows {
        let values = cols
            .iter()
            .map(|(name, kind)| resource_commits::encode_column(&row[name], kind))
            .collect::<Result<Vec<_>>>()?;
        statement
            .execute(rusqlite::params_from_iter(values))
            .map_err(storage)?;
    }
    Ok(())
}
