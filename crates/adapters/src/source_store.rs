//! Only source revisions and execution facts cross the persistence boundary.
use super::*;
use rusqlite::OptionalExtension;

pub(super) fn is_source(r: &ConfigurationRecord) -> bool {
    r.is_definition()
        || ["project_configuration", "integration_project_configuration"]
            .contains(&r.r#type.as_str())
}

pub(super) fn load_records(conn: &Connection) -> Result<Vec<ConfigurationRecord>> {
    load_records_selected(conn, false)
}
pub(super) fn load_catalog_records(conn: &Connection) -> Result<Vec<ConfigurationRecord>> {
    load_records_selected(conn, true)
}
fn load_records_selected(conn: &Connection, catalog: bool) -> Result<Vec<ConfigurationRecord>> {
    let _span = performance::Span::new("source.load_records");
    let filter = if catalog {
        format!(
            "type IN ({})",
            CATALOG_RECORD_KINDS
                .iter()
                .map(|k| format!("'{k}'"))
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        "1".into()
    };
    let mut records: Vec<ConfigurationRecord> =
        load_table_filtered::<Value>(conn, "governance_records", &filter)?
            .into_iter()
            .map(ConfigurationRecord::from_storage)
            .collect::<Result<_>>()?;
    let mut query = conn
        .prepare(
            "SELECT type,identity,revision,name,scope,refs,data FROM source_definitions ORDER BY rowid",
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
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(storage)?;
    for source in sources {
        let (r#type, identity, revision, name, scope, refs, data) = source.map_err(storage)?;
        let definition = SourceDefinition {
            r#type,
            identity,
            revision,
            name,
            scope: serde_json::from_str(&scope).map_err(storage)?,
            refs: serde_json::from_str(&refs).map_err(storage)?,
            data: serde_json::from_str(&data).map_err(storage)?,
        };
        let _span = performance::Span::new("source.validate_and_hash");
        records.push(definition.into_record()?);
    }
    for index in 0..records.len() {
        if records[index].r#type == "skill_package" {
            let owner = records[index].definition_id.clone();
            if let Some(body) = records
                .iter()
                .find(|r| r.id == owner && r.r#type == "skill")
                .map(|r| r.data["body"].clone())
            {
                records[index].data["body"] = body;
                records[index].content_hash =
                    content_digest(&records[index].refs, &records[index].data);
            }
        }
    }
    for index in 0..records.len() {
        if is_source(&records[index]) {
            records[index].supersedes = records
                .iter()
                .filter(|r| {
                    r.r#type == records[index].r#type
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
        validate_source_data(&record.r#type, &record.refs, &record.data)?;
        require(
            record.id == source_reference(&record.definition_id, record.version)?,
            "源修订必须使用 identity:revision 引用",
        )?;
        conn.execute("INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES(?1,?2,?3,?4,?5,?6,?7)",rusqlite::params![record.definition_id,record.version,record.r#type,record.name,serde_json::to_string(&record.scope).map_err(storage)?,serde_json::to_value(&record.refs).map_err(storage)?.to_string(),record.data.to_string()]).map_err(storage)?;
    }
    let packed: Vec<ConfigurationRecord> = state
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
    // Transitional shared writer: controller replacement still revokes pending
    // intents through its legacy aggregate. It must never blindly overwrite a
    // callback that committed after that aggregate was read. This runs inside
    // the caller's SQL transaction and does not add a business mutex.
    for row in packed.iter().filter(|r| r.r#type == "workflow_intent") {
        let expected = before
            .governance
            .iter()
            .find(|old| old.id == row.id)
            .map_or(0, |old| old.revision);
        let actual = conn
            .query_row(
                "SELECT revision FROM governance_records WHERE id=?1",
                [&row.id],
                |r| r.get::<_, u64>(0),
            )
            .optional()
            .map_err(storage)?
            .unwrap_or(0);
        if expected != actual {
            return Err(err("revision_conflict", "原工作流意图在旧协调保存前已变化"));
        }
        require(
            expected == 0 || row.revision > expected,
            "旧协调不能绕过工作流意图版本递增",
        )?;
    }
    let runtime: Vec<Value> = packed.iter().map(|r| r.to_storage()).collect();
    save_table(conn, "governance_records", &runtime, &[])
}

pub(super) fn pack_record(
    state: &EngineeringState,
    record: &ConfigurationRecord,
) -> Result<ConfigurationRecord> {
    let mut stored = record.clone();
    if record.r#type == "skill_package" {
        stored.data.as_object_mut().unwrap().remove("body");
    } else if matches!(
        record.r#type.as_str(),
        "workflow_pending_decision" | "workflow_human_decision"
    ) {
        // The submitted decision request is an immutable input fact.
        let request = stored
            .data
            .as_object_mut()
            .and_then(|data| data.remove("request"));
        stored.data = source_views::pack(state, &stored.data)?;
        if let Some(request) = request {
            stored
                .data
                .as_object_mut()
                .ok_or_else(|| err("storage", "决定记录需要对象正文"))?
                .insert("request".into(), request);
        }
    } else {
        stored.data = source_views::pack(state, &record.data)?;
    }
    if record.r#type == "configured_execution_task" {
        require(
            stored.data["task"]["$source"] == "task",
            "任务必须能由源修订和实际输入完整重建",
        )?;
    }
    stored.content_hash = content_digest(&stored.refs, &stored.data);
    Ok(stored)
}
pub(super) fn restore_records(
    state: &mut EngineeringState,
    memo: &mut std::collections::BTreeMap<String, Value>,
) -> Result<()> {
    let _span = performance::Span::new("source.restore_records");
    let snapshot = state.clone();
    for record in &mut state.governance {
        if !is_source(record) {
            let request = if matches!(
                record.r#type.as_str(),
                "workflow_pending_decision" | "workflow_human_decision"
            ) {
                record
                    .data
                    .as_object_mut()
                    .and_then(|data| data.remove("request"))
            } else {
                None
            };
            record.data = source_views::restore_with(&snapshot, &record.data, memo)?;
            if let Some(request) = request {
                record
                    .data
                    .as_object_mut()
                    .ok_or_else(|| err("storage", "决定记录需要对象正文"))?
                    .insert("request".into(), request);
            }
            record.content_hash = content_digest(&record.refs, &record.data);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_decision_inputs_do_not_enter_the_configuration_codec() {
        let request = json!({
            "operation":"apply",
            "reason":"\u{1e}devdesk-source-text:{\"$source\":\"source\",\"ref\":\"missing:1\"}",
            "prospective_input":{"$source":"source","ref":"missing:1"},
            "arguments":{"wide_integer":u64::MAX}
        });
        for kind in ["workflow_pending_decision", "workflow_human_decision"] {
            let record = ConfigurationRecord {
                id: "decision".into(),
                r#type: kind.into(),
                definition_id: "root".into(),
                scope: Scope::global(),
                name: "Exact decision".into(),
                version: 1,
                supersedes: None,
                content_hash: String::new(),
                refs: References::new(),
                data: json!({"status":"pending","request":request}),
                immutable: true,
                revision: 1,
                created_at: 1,
            };
            let mut state = EngineeringState::default();
            let packed = pack_record(&state, &record).unwrap();
            assert_eq!(packed.data["request"], request);
            // Exercise the persisted JSON representation, then normal historical restore.
            state.governance.push(
                ConfigurationRecord::from_storage(
                    serde_json::from_str(&packed.to_storage().to_string()).unwrap(),
                )
                .unwrap(),
            );
            restore_records(&mut state, &mut Default::default()).unwrap();
            assert_eq!(state.governance[0].data, record.data);
        }
    }
}
