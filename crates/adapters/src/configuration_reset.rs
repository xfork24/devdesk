//! Explicit destructive catalog reset; execution evidence becomes self-contained.
use super::*;
use devdesk_core::concurrency::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare(
    store: &dyn ports::EngineeringStore,
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
    let guards = store.read_resources(&keys)?;
    require(
        guards[0].revision == expected_catalog,
        "配置目录已变化，请重新预览",
    )?;
    let before = store.snapshot()?;
    let mut next = EngineeringState {
        governance: records.to_vec(),
        projects: before.projects.clone(),
        ..Default::default()
    };
    next.refresh_configuration_views()?;
    let available: BTreeSet<_> = next
        .governance
        .iter()
        .filter(|r| r.is_definition())
        .map(|r| r.id.as_str())
        .collect();
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
    let retained = bindings
        .into_values()
        .filter(|row| {
            configuration_edges(row).is_ok_and(|edges| {
                edges
                    .iter()
                    .all(|edge| available.contains(edge.to.as_str()))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
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
    let mut evidence = Vec::new();
    for row in before
        .governance
        .iter()
        .filter(|r| !catalog_kind(&r.r#type) && r.r#type != "template_reset_receipt")
    {
        let mut row = row.clone();
        if row.r#type == "workflow_instance"
            && [
                &row.data["workflow_ref"],
                &row.data["plan"],
                &row.data["role_agent_bindings"],
            ]
            .into_iter()
            .any(|value| references_removed(value, &removed))
            && !matches!(row.data["status"].as_str(), Some("completed" | "cancelled"))
        {
            row.data["paused"] = json!(true);
            row.data["configuration_reset"] = json!(true);
            row.revision += 1;
        }
        row.data = source_views::seal_reset_evidence(&row.data);
        row.content_hash = content_digest(&row.refs, &row.data);
        evidence.push(row.to_storage());
    }
    let mut catalog = records.to_vec();
    catalog.extend(retained);
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
            value: json!({"records":catalog,"evidence":evidence,
                "sessions":source_views::seal_reset_evidence(&json!(before.sessions)),
                "turns":source_views::seal_reset_evidence(&json!(before.turns)),
                "executions":source_views::seal_reset_evidence(&json!(before.executions)),
                "snapshots":source_views::seal_reset_evidence(&json!(before.snapshots))}),
        }],
    })
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
        let rows: Vec<Value> = serde_json::from_value(value[field].clone()).map_err(storage)?;
        if !rows.is_empty() {
            write_rows(conn, table, &rows)?;
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
