//! Only source revisions and execution facts cross the persistence boundary.
use super::*;

pub(super) fn is_source(r: &ConfigurationRecord) -> bool {
    r.is_definition()
        || [
            "permission_context",
            "project_configuration",
            "integration_project_configuration",
        ]
        .contains(&r.kind.as_str())
}

pub(super) fn load_records(conn: &Connection) -> Result<Vec<ConfigurationRecord>> {
    let mut records: Vec<ConfigurationRecord> = load_table(conn, "governance_records")?;
    let mut query = conn
        .prepare(
            "SELECT kind,identity,revision,name,scope,data FROM source_definitions ORDER BY rowid",
        )
        .map_err(storage)?;
    let sources = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(storage)?;
    for source in sources {
        let (kind, identity, revision, name, scope, data) = source.map_err(storage)?;
        let definition = SourceDefinition {
            kind,
            identity,
            revision,
            name,
            scope: serde_json::from_str(&scope).map_err(storage)?,
            data: serde_json::from_str(&data).map_err(storage)?,
        };
        records.push(definition.into_record()?);
    }
    for index in 0..records.len() {
        if records[index].kind == "skill_package" {
            let owner = records[index].definition_id.clone();
            if let Some(body) = records
                .iter()
                .find(|r| r.id == owner && r.kind == "skill")
                .map(|r| r.data["body"].clone())
            {
                records[index].data["body"] = body;
                records[index].content_hash = configuration_hash(&records[index].data);
            }
        }
    }
    for index in 0..records.len() {
        if is_source(&records[index]) {
            records[index].supersedes = records
                .iter()
                .filter(|r| {
                    r.kind == records[index].kind
                        && r.definition_id == records[index].definition_id
                        && r.version < records[index].version
                })
                .max_by_key(|r| r.version)
                .map(|r| r.id.clone());
        }
    }
    Ok(records)
}

pub(super) fn save_records(
    conn: &Connection,
    state: &EngineeringState,
    before: &EngineeringState,
) -> Result<()> {
    let source_rows: Vec<_> = state.governance.iter().filter(|r| is_source(r)).collect();
    for record in source_rows {
        if before.governance.iter().any(|old| old.id == record.id) {
            continue;
        }
        validate_source_data(&record.kind, &record.data)?;
        require(
            record.id == source_reference(&record.definition_id, record.version)?,
            "源修订必须使用 identity:revision 引用",
        )?;
        conn.execute("INSERT INTO source_definitions(identity,revision,kind,name,scope,data) VALUES(?1,?2,?3,?4,?5,?6)",rusqlite::params![record.definition_id,record.version,record.kind,record.name,serde_json::to_string(&record.scope).map_err(storage)?,record.data.to_string()]).map_err(storage)?;
    }
    let runtime: Vec<_> = state
        .governance
        .iter()
        .filter(|r| {
            !is_source(r)
                && before
                    .governance
                    .iter()
                    .find(|old| old.id == r.id)
                    .is_none_or(|old| old != *r)
        })
        .map(|r| pack_record(state, r))
        .collect::<Result<_>>()?;
    save_table(conn, "governance_records", &runtime, &[])
}

fn pack_record(
    state: &EngineeringState,
    record: &ConfigurationRecord,
) -> Result<ConfigurationRecord> {
    let mut stored = record.clone();
    if record.kind == "skill_package" {
        stored.data.as_object_mut().unwrap().remove("body");
    } else {
        stored.data = source_views::pack(state, &record.data)?;
    }
    if record.kind == "configured_execution_task" {
        require(
            stored.data["task"]["$source"] == "task",
            "任务必须能由源修订和实际输入完整重建",
        )?;
    }
    stored.content_hash = configuration_hash(&stored.data);
    Ok(stored)
}
pub(super) fn restore_records(state: &mut EngineeringState) -> Result<()> {
    let snapshot = state.clone();
    let mut memo = std::collections::BTreeMap::new();
    for record in &mut state.governance {
        if !is_source(record) {
            record.data = source_views::restore_with(&snapshot, &record.data, &mut memo)?;
            record.content_hash = configuration_hash(&record.data);
        }
    }
    Ok(())
}
