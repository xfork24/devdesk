//! Witnesses for the exact source revisions consumed by stored governance bodies.
use super::*;
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet};

const CODEC: u32 = 1;

pub(super) fn create(conn: &Connection) -> Result<()> {
    let definitions = include_str!("../../../sql/project/schema.sql")
        .split_once("-- Source dependency index.\n")
        .and_then(|(_, end)| end.split_once("PRAGMA user_version="))
        .map(|(definitions, _)| definitions)
        .ok_or_else(|| err("storage", "工程库缺少源依赖索引定义"))?;
    conn.execute_batch(definitions).map_err(storage)
}

pub(super) fn enabled(conn: &Connection) -> Result<bool> {
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage)?;
    Ok(version >= SOURCE_PROJECT_SCHEMA_VERSION)
}

pub(super) fn classify(data: &Value) -> Option<BTreeSet<String>> {
    let mut references = BTreeSet::new();
    source_views::collect_dependencies(data, &mut references).then_some(references)
}

/// Absence is unknown. An indexed consumer with no edge has been scanned and
/// contains no dependency. Witnesses obtained from a previous revision cannot
/// authorize exclusion of a rewritten body.
pub(super) fn write(
    conn: &Connection,
    consumer: &str,
    digest: &str,
    references: Option<&BTreeSet<String>>,
) -> Result<()> {
    conn.prepare_cached("DELETE FROM governance_source_scans WHERE consumer=?1")
        .map_err(storage)?
        .execute([consumer])
        .map_err(storage)?;
    let Some(references) = references else {
        return Ok(());
    };
    conn.prepare_cached(
        "INSERT INTO governance_source_scans(consumer,content_hash,codec,reference_count) VALUES(?1,?2,?3,?4)",
    )
    .map_err(storage)?
    .execute(params![consumer, digest, CODEC, references.len()])
    .map_err(storage)?;
    let mut insert = conn
        .prepare_cached("INSERT INTO governance_source_refs(consumer,reference) VALUES(?1,?2)")
        .map_err(storage)?;
    for reference in references {
        insert
            .execute(params![consumer, reference])
            .map_err(storage)?;
    }
    Ok(())
}

/// Only references with the same contents and the same typed dependency
/// closure may remain packed. Unknown references and failed edge validation
/// leave the consumer eligible for the original conservative scan.
pub(super) fn unchanged(
    original: &[ConfigurationRecord],
    incoming: &[ConfigurationRecord],
) -> BTreeSet<String> {
    let previous: BTreeMap<_, _> = original.iter().map(|row| (row.id.as_str(), row)).collect();
    let next: BTreeMap<_, _> = incoming.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut unchanged = BTreeMap::new();
    fn visit<'a>(
        id: &'a str,
        previous: &BTreeMap<&'a str, &'a ConfigurationRecord>,
        next: &BTreeMap<&'a str, &'a ConfigurationRecord>,
        memo: &mut BTreeMap<&'a str, bool>,
        path: &mut BTreeSet<&'a str>,
    ) -> bool {
        if let Some(result) = memo.get(id) {
            return *result;
        }
        if !path.insert(id) {
            return false;
        }
        let result = previous
            .get(id)
            .zip(next.get(id))
            .is_some_and(|(old, new)| {
                old == new
                    && configuration_edges(old).is_ok_and(|edges| {
                        edges.iter().all(|edge| {
                            previous
                                .get_key_value(edge.to.as_str())
                                .is_some_and(|(key, _)| visit(key, previous, next, memo, path))
                        })
                    })
            });
        path.remove(id);
        memo.insert(id, result);
        result
    }
    let mut path = BTreeSet::new();
    for id in previous.keys() {
        visit(id, &previous, &next, &mut unchanged, &mut path);
    }
    unchanged
        .into_iter()
        .filter(|(_, same)| *same)
        .map(|(id, _)| id.to_owned())
        .collect()
}

pub(super) fn reset_candidate_filter(legacy: &str) -> String {
    format!(
        "CASE WHEN EXISTS(
          SELECT 1 FROM governance_source_scans scan
          WHERE scan.consumer=governance_records.id
            AND scan.content_hash=governance_records.content_hash AND scan.codec={CODEC}
            AND (scan.reference_count=0 OR scan.reference_count=(
                SELECT count(*) FROM governance_source_refs counted WHERE counted.consumer=scan.consumer
            ))
            AND NOT EXISTS(SELECT 1 FROM governance_source_refs dependency
             WHERE dependency.consumer=scan.consumer
               AND dependency.reference NOT IN (SELECT value FROM json_each(?1)))
        ) THEN 0 ELSE ({legacy}) END"
    )
}

pub(super) fn backfill(conn: &Connection) -> Result<Value> {
    let _payloads = payload_store::reset_read_scope(conn);
    let mut cursor = 0i64;
    let mut indexed = 0u64;
    let mut dependent = 0u64;
    let mut inspected = 0u64;
    loop {
        let rows = conn
            .prepare_cached(
                "SELECT rowid,id,type,content_hash,CAST(data AS TEXT) FROM governance_records
             WHERE rowid>?1 ORDER BY rowid LIMIT 128",
            )
            .map_err(storage)?
            .query_map([cursor], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        if rows.is_empty() {
            break;
        }
        for (rowid, id, kind, digest, raw) in rows {
            cursor = rowid;
            inspected += 1;
            let mut data = serde_json::from_str(&raw).map_err(storage)?;
            payload_store::restore_governance(conn, &kind, &mut data)?;
            let references = classify(&data);
            indexed += u64::from(references.is_some());
            dependent += u64::from(references.as_ref().is_some_and(|refs| !refs.is_empty()));
            write(conn, &id, &digest, references.as_ref())?;
        }
    }
    let witnessed: u64 = conn
        .query_row("SELECT count(*) FROM governance_source_scans", [], |row| {
            row.get(0)
        })
        .map_err(storage)?;
    let consumers: u64 = conn
        .query_row("SELECT count(*) FROM governance_records", [], |row| {
            row.get(0)
        })
        .map_err(storage)?;
    require(
        witnessed == indexed && consumers == inspected,
        "治理源依赖回填数量不符",
    )?;
    let event_cursor: u64 = conn
        .query_row("SELECT coalesce(max(id),0) FROM events", [], |row| {
            row.get(0)
        })
        .map_err(storage)?;
    Ok(
        json!({"cursor":cursor,"event_cursor":event_cursor,"consumers":consumers,
        "indexed":indexed,"dependent":dependent,"unknown":consumers-indexed}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_literals_nested_sources_and_unknown_descriptors() {
        let body = json!({
            "nested": [
                {"version":{"$source":"source_definition","ref":"tool:2"}},
                {"message":"\u{001e}devdesk-source-text:{\"$source\":\"source\",\"ref\":\"role:3\"}"},
                {"$source":"literal","value":{"$source":"source","ref":"ignored:1"}},
                {"inputs":{"$source":"source","ref":"ignored:2"}}
            ],
        });
        assert_eq!(
            classify(&body).unwrap(),
            BTreeSet::from(["tool:2".into(), "role:3".into()])
        );
        assert_eq!(
            classify(&json!({
                "notes":{"$source":"literal","value":{"$source":"source","ref":"ignored:1"}},
                "marked":"\u{001e}devdesk-source-text:{\"literal\":\"unchanged\"}"
            })),
            Some(BTreeSet::new())
        );
        assert!(classify(&json!({"nested":{"$source":"task"}})).is_none());
        assert!(classify(&json!({"nested":{"$devdesk_project_payload_v1":{}}})).is_none());
    }

    #[test]
    fn transitive_integration_change_selects_exact_tool_consumers() {
        let entry = |kind: &str, identity: &str, data: Value| {
            SourceDefinition {
                r#type: kind.into(),
                identity: identity.into(),
                revision: 1,
                name: identity.into(),
                scope: Scope::global(),
                refs: References::new(),
                data,
            }
            .into_record()
            .unwrap()
        };
        let role = entry(
            "role",
            "role-one",
            json!({"instructions":"# duty","visibility":{"kind":"global"}}),
        );
        let mut tool = role.clone();
        tool.id = "tool:1".into();
        tool.r#type = "tool".into();
        tool.data = json!({"integration_ref":"integration:1"});
        let mut integration = role.clone();
        integration.r#type = "integration".into();
        integration.id = "integration:1".into();
        let old = [role, tool, integration];
        let mut next = old.clone();
        next[2].data["instructions"] = json!("updated revision contents");
        let anchors = unchanged(&old, &next);
        assert!(anchors.contains("role-one:1"));
        assert!(!anchors.contains("tool:1"));
        assert!(!anchors.contains("integration:1"));

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
            CREATE TABLE governance_records(id TEXT PRIMARY KEY,content_hash TEXT,data TEXT);
            INSERT INTO governance_records VALUES
            ('affected','h','source'),('untouched','h','source'),('empty','h','source'),
            ('missing_edge','h','source'),('stale','h2','source'),('unknown','h','source');",
        )
        .unwrap();
        create(&conn).unwrap();
        for (id, refs) in [
            ("affected", BTreeSet::from(["tool:1".into()])),
            ("untouched", BTreeSet::from(["role-one:1".into()])),
            ("empty", BTreeSet::new()),
            ("missing_edge", BTreeSet::from(["tool:1".into()])),
            ("stale", BTreeSet::new()),
        ] {
            write(&conn, id, "h", Some(&refs)).unwrap();
        }
        conn.execute(
            "DELETE FROM governance_source_refs WHERE consumer='missing_edge'",
            [],
        )
        .unwrap();
        let filter = reset_candidate_filter("data='source'");
        let actual: Vec<String> = conn
            .prepare(&format!(
                "SELECT id FROM governance_records WHERE {filter} ORDER BY id"
            ))
            .unwrap()
            .query_map([json!(anchors).to_string()], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(actual, ["affected", "missing_edge", "stale", "unknown"]);
    }

    #[test]
    fn attached_reset_plan_rejects_corrupt_evidence_and_catalog_conflicts() {
        use devdesk_core::concurrency::{
            PreparedChangeSet, ReadSet, ResourceChange, ResourceKey, ResourceVersion,
        };
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        let references = References::new();
        let scope = Scope::global();
        let conn = store.connection.lock().unwrap();
        conn.execute(
            "INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data)
             VALUES('reset-role',1,'role','Reset Role',?1,'{}',?2)",
            params![
                json!(scope).to_string(),
                json!({"instructions":"# Removed role",
                "visibility":{"kind":"global"}})
                .to_string()
            ],
        )
        .unwrap();
        drop(conn);
        let body = json!({"preview":{"notes":"x".repeat(12000),
            "definition":{"$source":"source_definition","ref":"reset-role:1"}}});
        let row = ConfigurationRecord {
            id: "affected-change".into(),
            r#type: "workflow_change".into(),
            name: "Affected change".into(),
            definition_id: "change".into(),
            version: 1,
            revision: 1,
            immutable: true,
            created_at: 0,
            supersedes: None,
            scope,
            content_hash: content_digest(&references, &body),
            refs: references,
            data: body,
        };
        let resource = ResourceKey::new("governance", &row.id);
        store
            .commit_changes(&PreparedChangeSet {
                command_id: "create-evidence".into(),
                fingerprint: "create-evidence".into(),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: resource.clone(),
                    revision: 0,
                }])
                .unwrap(),
                changes: vec![ResourceChange {
                    resource,
                    value: row.to_storage(),
                }],
                result: json!({}),
                time: 1,
            })
            .unwrap();
        let (digest, reference): (String, String) = store.connection.read().unwrap().query_row(
            r#"SELECT json_extract(data,'$.preview."$devdesk_project_payload_v1".sha256'), reference
                FROM governance_records g JOIN governance_source_refs d ON d.consumer=g.id WHERE g.id=?1"#,
            [&row.id], |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(reference, "reset-role:1");
        let object = dir
            .path()
            .join("payloads/objects")
            .join(&digest[..2])
            .join(&digest[2..4])
            .join(digest);
        let bytes = fs::read(&object).unwrap();
        fs::write(&object, b"invalid").unwrap();
        let revision = store.catalog_revision().unwrap();
        assert!(
            store
                .prepare_configuration_reset(&[], revision)
                .err()
                .unwrap()
                .message
                .contains("正文长度或摘要")
        );
        assert_eq!(store.catalog_revision().unwrap(), revision);
        fs::write(&object, bytes).unwrap();

        let reset = store.prepare_configuration_reset(&[], revision).unwrap();
        let conn = store.connection.lock().unwrap();
        conn.execute("UPDATE catalog_clock SET revision=revision+1", [])
            .unwrap();
        drop(conn);
        let commit = |publication: ConfigurationPublication| {
            store.commit_changes(&PreparedChangeSet {
                command_id: "commit-reset".into(),
                fingerprint: "commit-reset".into(),
                read_set: publication.read_set,
                changes: publication.changes,
                result: json!({}),
                time: 2,
            })
        };
        assert_eq!(commit(reset).unwrap_err().code, "revision_conflict");
        let prepared = store
            .prepare_configuration_reset(&[], revision + 1)
            .unwrap();
        commit(prepared).unwrap();
        let rows = load_table_filtered_params::<Value>(
            &store.connection.read().unwrap(),
            "governance_records",
            "id=?1",
            &[&row.id],
        )
        .unwrap();
        assert_eq!(
            rows[0]["data"]["preview"]["definition"]["identity"],
            "reset-role"
        );
        assert_eq!(rows[0]["data"]["preview"]["notes"], "x".repeat(12000));
    }

    #[test]
    fn backfill_reads_external_bodies_and_rejects_corrupt_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
        conn.execute_batch(include_str!("../../../sql/project/schema.sql"))
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        let body = json!({"preview":{"notes":"x".repeat(12000),
            "definition":{"$source":"source_definition","ref":"role-one:1"}}});
        let mut packed = body.clone();
        payload_store::governance(dir.path(), "workflow_change", &mut packed, None).unwrap();
        assert_ne!(packed, body);
        assert!(classify(&packed).is_none());
        conn.execute(
            "INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at)
             VALUES('change','workflow_change','change','{\"kind\":\"global\"}','change',1,'digest','{}',?1,1,1,0)",
            [packed.to_string()],
        ).unwrap();
        let result = backfill(&conn).unwrap();
        assert_eq!(result["dependent"], 1);
        assert_eq!(result["unknown"], 0);
        let selected: String = conn
            .query_row(
                "SELECT reference FROM governance_source_refs WHERE consumer='change'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(selected, "role-one:1");
        let payloads = dir.path().join("payloads");
        let mut dirs = vec![payloads];
        while let Some(folder) = dirs.pop() {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    fs::write(path, b"corrupt").unwrap();
                }
            }
        }
        assert!(backfill(&conn).is_err());
    }
}
