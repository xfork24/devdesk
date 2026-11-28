//! Only source revisions and execution facts cross the persistence boundary.
use super::*;
#[cfg(any(test, feature = "test-fixtures"))]
use rusqlite::OptionalExtension;

pub(super) fn is_source(r: &ConfigurationRecord) -> bool {
    r.is_definition()
        || ["project_configuration", "integration_project_configuration"]
            .contains(&r.r#type.as_str())
}

pub(super) fn load_records(conn: &Connection) -> Result<Vec<ConfigurationRecord>> {
    load_records_selected(conn, "1")
}
pub(super) fn load_catalog_records(conn: &Connection) -> Result<Vec<ConfigurationRecord>> {
    load_records_selected(conn, &catalog_filter())
}

/// Correct the built-in Codex label after taking the dataset lock. Exact revision
/// references and executable content are used by existing workflows and grants.
pub(super) fn update_builtin_codex_name(conn: &Connection) -> Result<()> {
    const REFERENCE: &str = "builtin-harness-codex-managed:2";
    const LEGACY_NAME: &str = "Codex";
    let needed: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_definitions WHERE reference=?1 AND name=?2)",
            [REFERENCE, LEGACY_NAME],
            |row| row.get(0),
        )
        .map_err(storage)?;
    if !needed {
        return Ok(());
    }
    let current = factory_bundle::packaged_harness(HarnessProvider::Codex)?;
    if current.id != REFERENCE {
        return Ok(());
    }
    let mut legacy = current.clone();
    legacy.name = LEGACY_NAME.into();
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let key = concurrency::ResourceKey::new("source", REFERENCE);
    let rows = resource_commits::read_on(&tx, &[key])?;
    if rows[0].value.as_ref() != Some(&json!(legacy.source())) {
        return Ok(());
    }

    let original_hash = template_source_hash(&legacy);
    let updated_hash = template_source_hash(&current);
    let members = {
        let mut query = tx.prepare("SELECT id,refs,data FROM governance_records WHERE type='template_catalog_member' AND definition_id=?1 AND name=?2").map_err(storage)?;
        query
            .query_map([REFERENCE, LEGACY_NAME], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?
    };
    // SQLite rolls back both the data and these trigger changes on failure.
    let guards = ["source_immutable_update", "governance_immutable_update"];
    let definitions = guards.map(|name| {
        tx.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get::<_, String>(0),
        )
    });
    let [source_guard, governance_guard] = definitions;
    let source_guard = source_guard.map_err(storage)?;
    let governance_guard = governance_guard.map_err(storage)?;
    tx.execute_batch(
        "DROP TRIGGER source_immutable_update; DROP TRIGGER governance_immutable_update;",
    )
    .map_err(storage)?;
    tx.execute(
        "UPDATE source_definitions SET name=?1 WHERE reference=?2 AND name=?3",
        [&current.name, REFERENCE, LEGACY_NAME],
    )
    .map_err(storage)?;
    for (id, refs, data) in members {
        let refs: References = serde_json::from_str(&refs).map_err(storage)?;
        let mut data: Value = serde_json::from_str(&data).map_err(storage)?;
        if data["template_key"] == REFERENCE && data["source_hash"] == original_hash {
            data["source_hash"] = json!(updated_hash);
        }
        tx.execute(
            "UPDATE governance_records SET name=?1, data=?2, content_hash=?3 WHERE id=?4",
            rusqlite::params![
                current.name,
                data.to_string(),
                content_digest(&refs, &data),
                id
            ],
        )
        .map_err(storage)?;
    }
    tx.execute_batch(&source_guard).map_err(storage)?;
    tx.execute_batch(&governance_guard).map_err(storage)?;
    tx.execute(
        "UPDATE catalog_clock SET revision=revision+1 WHERE id=1",
        [],
    )
    .map_err(storage)?;
    tx.commit().map_err(storage)
}

fn catalog_filter() -> String {
    format!(
        "type IN ({})",
        CATALOG_RECORD_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(super) fn load_reset_records(
    conn: &Connection,
    incoming: &[ConfigurationRecord],
) -> Result<Vec<ConfigurationRecord>> {
    // Reconstruction facts must remain available even when they need no rewrite.
    let facts = format!(
        "{} OR type IN ('permission_binding','permission_assignment','workflow_instance',
           'workflow_plan_revision','workflow_execution_history','configured_execution_task',
           'archived_configured_execution_task')",
        catalog_filter()
    );
    let legacy = "instr(data, '\"$source\"') > 0 OR instr(data, 'devdesk-source-text:') > 0
         OR instr(data, '\"$devdesk_project_payload_v1\"') > 0
         OR instr(data, 'devdesk-payload/v1:') > 0";
    let _span = performance::Span::new("source.load_records");
    let rows = if source_dependencies::enabled(conn)? {
        let sources = load_records_selected(
            conn,
            &format!(
                "{} OR type IN ('permission_binding','permission_assignment')",
                catalog_filter()
            ),
        )?;
        let unchanged = serde_json::to_string(&source_dependencies::unchanged(&sources, incoming))
            .map_err(storage)?;
        let candidate = source_dependencies::reset_candidate_filter(legacy);
        load_rows::<Value>(
            conn,
            "governance_records",
            &format!("{facts} OR {candidate}"),
            &[&unchanged],
            false,
        )?
    } else {
        load_rows::<Value>(
            conn,
            "governance_records",
            &format!("{facts} OR ({legacy})"),
            &[],
            false,
        )?
    };
    let mut records = Vec::new();
    for row in rows {
        let mut row = ConfigurationRecord::from_storage(row)?;
        let reconstruction_fact = catalog_kind(&row.r#type)
            || matches!(
                row.r#type.as_str(),
                "workflow_instance"
                    | "workflow_plan_revision"
                    | "workflow_execution_history"
                    | "configured_execution_task"
                    | "archived_configured_execution_task"
            );
        if reconstruction_fact
            || payload_store::governance_uses_sources(conn, &row.r#type, &row.data)?
        {
            payload_store::restore_governance(conn, &row.r#type, &mut row.data)?;
            records.push(row);
        }
    }
    append_source_definitions(conn, records)
}

fn load_records_selected(conn: &Connection, filter: &str) -> Result<Vec<ConfigurationRecord>> {
    let _span = performance::Span::new("source.load_records");
    let records = load_table_filtered::<Value>(conn, "governance_records", filter)?
        .into_iter()
        .map(ConfigurationRecord::from_storage)
        .collect::<Result<_>>()?;
    append_source_definitions(conn, records)
}

fn append_source_definitions(
    conn: &Connection,
    mut records: Vec<ConfigurationRecord>,
) -> Result<Vec<ConfigurationRecord>> {
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
    let mut histories = std::collections::BTreeMap::<_, Vec<_>>::new();
    for (index, row) in records.iter().enumerate().filter(|(_, row)| is_source(row)) {
        histories
            .entry((row.r#type.clone(), row.definition_id.clone()))
            .or_default()
            .push(index);
    }
    for indices in histories.values_mut() {
        indices.sort_by_key(|&index| records[index].version);
        for &index in indices.iter() {
            let first_current =
                indices.partition_point(|&prior| records[prior].version < records[index].version);
            records[index].supersedes = first_current
                .checked_sub(1)
                .map(|prior| records[indices[prior]].id.clone());
        }
    }
    Ok(records)
}

#[cfg(any(test, feature = "test-fixtures"))]
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
    // Offline aggregate fixtures must not overwrite an interleaved native
    // commit. Production writers use the resource commit protocol.
    for row in &packed {
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
            return Err(err("revision_conflict", "原运行事实在旧协调保存前已变化"));
        }
        require(
            expected == 0 || row.revision > expected,
            "旧协调不能绕过运行事实版本递增",
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
    if matches!(
        record.r#type.as_str(),
        "platform_call"
            | "agent_platform_request"
            | "platform_call_preparation"
            | "platform_reply_work"
    ) && record.data["request"].is_object()
    {
        let request: devdesk_core::PlatformToolRequest =
            serde_json::from_value(record.data["request"].clone()).map_err(storage)?;
        let reference = request
            .definition_ref
            .clone()
            .or_else(|| request.caller.as_ref().map(|c| c.tool_ref.clone()))
            .or_else(|| {
                let instance = state.configuration(&request.instance_id).ok()?;
                let execution = instance.data["steps"]
                    .as_array()?
                    .iter()
                    .find(|s| s["id"] == request.execution_id)?;
                instance.data["plan"]["document"]["steps"]
                    .as_array()?
                    .iter()
                    .find(|s| s["key"] == execution["step"])?["executor"]["tool_ref"]
                    .as_str()
                    .map(str::to_owned)
            })
            .ok_or_else(|| err("storage", "平台请求缺少原始工具源引用"))?;
        let source = state.configuration(&reference)?;
        require(
            source.r#type == "tool"
                && json!(source.decode::<ToolDefinition>()?) == json!(request.definition),
            "平台请求改变了确切工具源定义",
        )?;
        // Pack all other metadata normally, then replace the expanded schema
        // with its exact immutable source. Factual arguments stay literal.
        let mut metadata = stored.data.clone();
        metadata["request"]
            .as_object_mut()
            .unwrap()
            .remove("definition");
        stored.data = source_views::pack(state, &metadata)?;
        stored.data["request"]["definition_ref"] = json!(reference);
        stored.data["request"]["definition"] = json!({"$source":"tool_definition","ref":reference});
        stored.content_hash = content_digest(&stored.refs, &stored.data);
        return Ok(stored);
    }
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
    if record.r#type == "agent_context_call" {
        let definition: ToolDefinition =
            serde_json::from_value(record.data["definition"].clone()).map_err(storage)?;
        let reference = record.data["invocation"]["tool"]["revision_id"]
            .as_str()
            .ok_or_else(|| err("storage", "上下文调用缺少原工具引用"))?;
        let unbound = definition.implementation == "unbound";
        let expected = devdesk_core::context_tool_definition(
            state,
            reference,
            &definition.qualified_name,
            unbound,
        )?;
        require(
            json!(expected) == json!(definition),
            "上下文调用改变了原工具源定义",
        )?;
        let descriptor = json!({"$source":"context_tool","ref":reference,"name":definition.qualified_name,"unbound":unbound});
        stored.data["definition"] = descriptor.clone();
        stored.data["invocation"]["tool"]["definition"] = descriptor;
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
    fn updates_codex_label_in_existing_catalogs_without_changing_the_execution_source() {
        let current = factory_bundle::packaged_harness(HarnessProvider::Codex).unwrap();
        let mut legacy = current.clone();
        legacy.name = "Codex".into();
        let membership = json!({"kind":"harness","generation":1,"template_key":current.id,"source_hash":template_source_hash(&legacy)});
        for schema in [
            include_str!("../../../sql/global/schema.sql"),
            include_str!("../../../sql/project/schema.sql"),
        ] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(schema).unwrap();
            conn.execute("INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES (?1,?2,'harness',?3,'\"global\"',?4,?5)",
                rusqlite::params![legacy.definition_id, legacy.version, legacy.name, json!(legacy.refs).to_string(), legacy.data.to_string()]).unwrap();
            conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES ('member','template_catalog_member',?1,'\"global\"',?2,1,?3,'{}',?4,1,1,0)",
                rusqlite::params![legacy.id, legacy.name, content_digest(&References::new(), &membership), membership.to_string()]).unwrap();

            update_builtin_codex_name(&conn).unwrap();
            let state = load_catalog_records(&conn).unwrap();
            let updated = state.iter().find(|row| row.id == current.id).unwrap();
            assert_eq!(updated.name, "Codex Harness");
            assert_eq!(updated.content_hash, legacy.content_hash);
            let member = state.iter().find(|row| row.id == "member").unwrap();
            assert_eq!(member.name, current.name);
            assert_eq!(member.data["source_hash"], template_source_hash(&current));
            assert!(
                conn.execute("UPDATE source_definitions SET name='Changed'", [])
                    .is_err()
            );
            assert!(
                conn.execute("UPDATE governance_records SET name='Changed'", [])
                    .is_err()
            );
            let revision = configuration_cache::revision(&conn).unwrap();
            update_builtin_codex_name(&conn).unwrap();
            assert_eq!(configuration_cache::revision(&conn).unwrap(), revision);
        }
    }

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
