//! Physical representation of immutable snapshot bodies and committed receipts.
use super::*;
use devdesk_core::concurrency::ResourceChange;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use std::{borrow::Cow, collections::HashMap};

const SNAPSHOT_FILE: &str = "\u{1}devdesk-snapshot-file/v1:";
const SNAPSHOT_INLINE: &str = "\u{1}devdesk-snapshot-inline/v1:";
const SUMMARY_CHARS: usize = 320;
const COMMAND_HASH: &str = "devdesk-command-sha256/v1:";
pub(super) const EVENT_VERSIONS: &str = r#"{"$devdesk_event_versions":1}"#;

pub(super) fn stored_fingerprint(fingerprint: &str) -> Cow<'_, str> {
    if fingerprint.starts_with("[\"") && fingerprint.len() > COMMAND_HASH.len() + 64 {
        Cow::Owned(format!(
            "{COMMAND_HASH}{}",
            configuration_hash(&fingerprint)
        ))
    } else {
        Cow::Borrowed(fingerprint)
    }
}

pub(super) fn fingerprint_matches(stored: &str, expected: &str) -> bool {
    if stored.starts_with(COMMAND_HASH) {
        matches!(stored_fingerprint(expected), Cow::Owned(hash) if hash == stored)
    } else {
        stored == expected
    }
}

pub(super) fn connection_root(conn: &Connection) -> Option<PathBuf> {
    Path::new(conn.path()?).parent().map(Path::to_owned)
}

#[derive(Serialize, Deserialize)]
struct SnapshotFile {
    artifact_id: String,
    summary: String,
}

fn pointer(content: &str) -> Result<Option<SnapshotFile>> {
    content
        .strip_prefix(SNAPSHOT_FILE)
        .map(|encoded| serde_json::from_str(encoded).map_err(storage))
        .transpose()
}

fn sealed_snapshot(files: &FileArtifacts, artifact: &Artifact) -> Result<Snapshot> {
    let bytes = files.read(artifact)?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(storage)?;
    let snapshot = value.get_mut("snapshot").map(Value::take).unwrap_or(value);
    serde_json::from_value(snapshot).map_err(storage)
}

fn snapshot_body(conn: &Connection, id: &str, file: &SnapshotFile) -> Result<String> {
    let root = connection_root(conn).ok_or_else(|| err("storage", "快照目录缺失"))?;
    let artifact: Artifact =
        load_table_filtered_params(conn, "artifacts", "id=?1", &[&file.artifact_id])?
            .pop()
            .ok_or_else(|| err("storage", "快照封存文件记录缺失"))?;
    let snapshot = sealed_snapshot(&FileArtifacts::new(root), &artifact)?;
    require(snapshot.id == id, "封存文件中的快照身份不匹配")?;
    Ok(snapshot.content)
}

pub(super) fn restore_snapshot(conn: &Connection, value: &mut Value) -> Result<()> {
    let Some(content) = value["content"].as_str() else {
        return Ok(());
    };
    if let Some(content) = content.strip_prefix(SNAPSHOT_INLINE) {
        value["content"] = json!(content);
    } else if let Some(file) = pointer(content)? {
        let id = value["id"]
            .as_str()
            .ok_or_else(|| err("storage", "快照身份缺失"))?;
        value["content"] = json!(snapshot_body(conn, id, &file)?);
    }
    Ok(())
}

pub(super) fn snapshot_excerpt(
    conn: &Connection,
    id: &str,
    chars: usize,
) -> Result<Option<(String, String)>> {
    let row: Option<(String, String, bool)> = conn
        .query_row(
            "SELECT kind, CASE WHEN substr(content,1,length(?2))=?2 THEN content
             WHEN substr(content,1,length(?3))=?3 THEN substr(content,length(?3)+1,?4)
             ELSE substr(content,1,?4) END, substr(content,1,length(?2))=?2
             FROM snapshots WHERE id=?1",
            params![id, SNAPSHOT_FILE, SNAPSHOT_INLINE, chars],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage)?;
    row.map(|(kind, content, external)| {
        let excerpt = match if external { pointer(&content)? } else { None } {
            Some(file) if chars <= SUMMARY_CHARS => file.summary.chars().take(chars).collect(),
            Some(file) => snapshot_body(conn, id, &file)?
                .chars()
                .take(chars)
                .collect(),
            None => content,
        };
        Ok((kind, excerpt))
    })
    .transpose()
}

// A reference is stored only when the file's SHA-256 and snapshot identity
// are verified and its exact body is already present in the SQLite write.
#[derive(Default)]
pub(super) struct SealedSnapshots(HashMap<String, (Artifact, Snapshot)>);

impl SealedSnapshots {
    fn insert(&mut self, files: &FileArtifacts, artifact: Artifact) {
        if artifact.relative_path.contains("/snapshots/")
            && let Ok(snapshot) = sealed_snapshot(files, &artifact)
        {
            self.0.insert(snapshot.id.clone(), (artifact, snapshot));
        }
    }

    fn matching(&self, value: &Value) -> Option<&Artifact> {
        let (artifact, sealed) = self.0.get(value["id"].as_str()?)?;
        (value["content"] == sealed.content
            && value["kind"] == sealed.kind
            && value["project_id"] == json!(sealed.project_id)
            && value["feature_id"] == json!(sealed.feature_id)
            && value["task_id"] == json!(sealed.task_id))
        .then_some(artifact)
    }

    pub(super) fn reference(&self, value: &Value) -> Result<Option<String>> {
        let Some(content) = value["content"].as_str() else {
            return Ok(None);
        };
        let escaped = || {
            (content.starts_with(SNAPSHOT_FILE) || content.starts_with(SNAPSHOT_INLINE))
                .then(|| format!("{SNAPSHOT_INLINE}{content}"))
        };
        let Some(artifact) = self.matching(value) else {
            return Ok(escaped());
        };
        let file = SnapshotFile {
            artifact_id: artifact.id.clone(),
            summary: content.chars().take(SUMMARY_CHARS).collect(),
        };
        let encoded = format!(
            "{SNAPSHOT_FILE}{}",
            serde_json::to_string(&file).map_err(storage)?
        );
        Ok((encoded.len() < content.len())
            .then_some(encoded)
            .or_else(escaped))
    }
}

pub(super) fn prepared_snapshots(
    root: Option<&Path>,
    changes: &[ResourceChange],
) -> Result<SealedSnapshots> {
    let mut index = SealedSnapshots::default();
    if let Some(root) = root.filter(|_| changes.iter().any(|c| c.resource.kind == "snapshot")) {
        let files = FileArtifacts::new(root.to_owned());
        for change in changes.iter().filter(|c| c.resource.kind == "artifact") {
            let artifact = serde_json::from_value(change.value.clone()).map_err(storage)?;
            index.insert(&files, artifact);
        }
    }
    Ok(index)
}

fn legacy_snapshots(conn: &Connection, root: &Path) -> Result<SealedSnapshots> {
    let needed: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots
                       WHERE length(content)>?2 AND substr(content,1,length(?1))!=?1
                       AND substr(content,1,length(?3))!=?3)
         OR EXISTS(SELECT 1 FROM governance_records
                  WHERE type IN ('conversation_capture_intent','captured_experience_intent')
                  AND json_type(data,'$.snapshot.content')='text')",
            params![SNAPSHOT_FILE, SUMMARY_CHARS, SNAPSHOT_INLINE],
            |row| row.get(0),
        )
        .map_err(storage)?;
    let mut index = SealedSnapshots::default();
    if needed {
        let artifacts: Vec<Artifact> =
            load_table_filtered(conn, "artifacts", "relative_path LIKE '%/snapshots/%'")?;
        let files = FileArtifacts::new(root.to_owned());
        for artifact in artifacts {
            index.insert(&files, artifact);
        }
    }
    Ok(index)
}

fn replace_snapshot_bodies(conn: &Connection, sealed: &SealedSnapshots) -> Result<usize> {
    if sealed.0.is_empty() {
        return Ok(0);
    }
    let rows: Vec<Value> = load_table_filtered_params(
        conn,
        "snapshots",
        "length(content)>?2 AND substr(content,1,length(?1))!=?1 AND substr(content,1,length(?3))!=?3",
        &[&SNAPSHOT_FILE, &SUMMARY_CHARS, &SNAPSHOT_INLINE],
    )?;
    let replacements = rows
        .iter()
        .filter_map(|row| {
            sealed
                .reference(row)
                .transpose()
                .map(|reference| reference.map(|v| (&row["id"], v)))
        })
        .collect::<Result<Vec<_>>>()?;
    if replacements.is_empty() {
        return Ok(0);
    }
    // The enclosing exclusive dataset maintenance transaction restores the
    // immutability trigger even if any write fails. Logical content is identical.
    let trigger: String = conn.query_row(
        "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='snapshot_immutable_update'",
        [], |row| row.get(0),
    ).map_err(storage)?;
    conn.execute_batch("DROP TRIGGER snapshot_immutable_update")
        .map_err(storage)?;
    for (id, reference) in &replacements {
        conn.execute(
            "UPDATE snapshots SET content=?1 WHERE id=?2",
            params![reference, id.as_str()],
        )
        .map_err(storage)?;
    }
    conn.execute_batch(&trigger).map_err(storage)?;
    Ok(replacements.len())
}

fn trim_intents(conn: &Connection, sealed: &SealedSnapshots) -> Result<usize> {
    let mut query = conn
        .prepare(
            "SELECT id FROM governance_records WHERE immutable=0 AND (
            (type='sealed_command_intent' AND json_extract(data,'$.status')='completed'
             AND json_type(data,'$.changes')='array') OR
            (type='platform_call_preparation' AND json_extract(data,'$.status')='committed'
             AND json_type(data,'$.command')='object') OR
            (type='conversation_open_intent' AND json_extract(data,'$.status')='completed'
             AND json_type(data,'$.request.execution')='object'
             AND EXISTS(SELECT 1 FROM turns WHERE id=governance_records.definition_id
                        AND status IN ('completed','failed','cancelled'))) OR
            (type IN ('conversation_capture_intent','captured_experience_intent')
             AND json_extract(data,'$.status') IN ('captured','completed')
             AND json_type(data,'$.snapshot.content')='text'))",
        )
        .map_err(storage)?;
    let ids = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    let mut changed = 0;
    let mut catalog = None;
    for id in ids {
        let raw: Value = load_table_filtered_params(conn, "governance_records", "id=?1", &[&id])?
            .pop()
            .ok_or_else(|| err("storage", "待清理的运行记录缺失"))?;
        let mut record = ConfigurationRecord::from_storage(raw)?;
        let data = &mut record.data;
        let fields: &[&str] = match record.r#type.as_str() {
            "sealed_command_intent" => {
                let mut fingerprint = Cow::Borrowed(&data["fingerprint"]);
                if fingerprint
                    .as_str()
                    .is_some_and(source_views::is_encoded_text)
                {
                    if catalog.is_none() {
                        let mut state = EngineeringState {
                            governance: source_store::load_catalog_records(conn)?,
                            ..Default::default()
                        };
                        state.refresh_configuration_views()?;
                        catalog = Some(state);
                    }
                    let Ok(restored) =
                        source_views::restore(catalog.as_ref().unwrap(), &fingerprint)
                    else {
                        continue;
                    };
                    fingerprint = Cow::Owned(restored);
                }
                if !has_receipt(conn, &data["command_id"], &fingerprint, &data["result"])? {
                    continue;
                }
                &["changes", "read_set", "result"]
            }
            "platform_call_preparation" => {
                let call_id = &data["call_id"];
                let command_id = &data["command"]["command_id"];
                let origin_id = &data["origin"]["resource"]["id"];
                let call: Option<String> = conn
                    .query_row(
                        "SELECT CAST(c.data AS TEXT) FROM governance_records c
                        WHERE c.id=?1 AND c.type='platform_call'
                        AND json_extract(c.data,'$.preparation_id')=?2
                        AND json_type(c.data,'$.request')='object'
                        AND json_extract(c.data,'$.status') IN ('succeeded','failed','cancelled')
                        AND EXISTS(SELECT 1 FROM governance_records
                                   WHERE id=?3 AND type='command_context' AND immutable=1)",
                        params![call_id.as_str(), id, origin_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(storage)?;
                let Some(call) = call else { continue };
                let mut call: Value = serde_json::from_str(&call).map_err(storage)?;
                call["request"]["runtime"] = Value::Null;
                if call["request"] != data["request"]
                    || call["workflow_authority"] != data["authority"]
                    || !has_receipt(
                        conn,
                        command_id,
                        &data["fingerprint"],
                        &json!({"id":call_id}),
                    )?
                {
                    continue;
                }
                data["command_id"] = command_id.clone();
                data["origin_id"] = data["origin"]["resource"]["id"].clone();
                &[
                    "command",
                    "fingerprint",
                    "request",
                    "authority",
                    "source_basis",
                    "origin",
                ]
            }
            "conversation_open_intent" => {
                let request = &data["request"];
                data["request"] = json!({
                    "code_view": request["code_view"],
                    "configured_task": request["configured_task"].is_object().then(|| json!({})),
                    "session": {
                        "id": request["session"]["id"],
                        "agent": request["session"]["agent"],
                        "runtime_binding": request["session"]["runtime_binding"]
                    }
                });
                &[]
            }
            _ => {
                let snapshot = &data["snapshot"];
                let Some(artifact) = sealed.matching(snapshot) else {
                    continue;
                };
                if data["artifact_id"] != artifact.id {
                    continue;
                }
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id=?1)",
                        [snapshot["id"].as_str()],
                        |row| row.get(0),
                    )
                    .map_err(storage)?;
                if !exists {
                    continue;
                }
                data["snapshot"] = json!({"id":snapshot["id"]});
                &[]
            }
        };
        for field in fields {
            data.as_object_mut().unwrap().remove(*field);
        }
        changed += conn
            .execute(
                "UPDATE governance_records SET data=?1,content_hash=?2 WHERE id=?3",
                params![data.to_string(), content_digest(&record.refs, data), id],
            )
            .map_err(storage)?;
    }
    Ok(changed)
}

fn has_receipt(
    conn: &Connection,
    command: &Value,
    fingerprint: &Value,
    result: &Value,
) -> Result<bool> {
    let Some((command, fingerprint)) = command.as_str().zip(fingerprint.as_str()) else {
        return Ok(false);
    };
    let receipt: Option<(String, String)> = conn
        .query_row(
            "SELECT fingerprint,CAST(result AS TEXT) FROM operations WHERE id=?1",
            [command],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((stored, encoded)) = receipt else {
        return Ok(false);
    };
    Ok(fingerprint_matches(&stored, fingerprint)
        && serde_json::from_str::<Value>(&encoded).map_err(storage)? == *result)
}

fn compact_receipts(conn: &Connection) -> Result<usize> {
    let mut query = conn.prepare(
        "SELECT id,fingerprint FROM operations WHERE substr(fingerprint,1,2)='[\"' AND length(fingerprint)>?1",
    ).map_err(storage)?;
    let rows = query
        .query_map([COMMAND_HASH.len() + 64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    for (id, fingerprint) in &rows {
        conn.execute(
            "UPDATE operations SET fingerprint=?1 WHERE id=?2",
            params![stored_fingerprint(fingerprint), id],
        )
        .map_err(storage)?;
    }
    let versions = conn
        .execute(
            "UPDATE operations SET versions=?1 WHERE cursor>0 AND versions!=?1
         AND EXISTS(SELECT 1 FROM events WHERE id=operations.cursor AND kind='resource.changed'
                    AND CAST(detail AS TEXT)=CAST(operations.versions AS TEXT))",
            [EVENT_VERSIONS],
        )
        .map_err(storage)?;
    Ok(rows.len() + versions)
}

pub(super) fn maintain(conn: &mut Connection, root: &Path) -> Result<()> {
    let sealed = legacy_snapshots(conn, root)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(storage)?;
    let snapshots = replace_snapshot_bodies(&tx, &sealed)?;
    let intents = trim_intents(&tx, &sealed)?;
    let receipts = compact_receipts(&tx)?;
    tx.commit().map_err(storage)?;
    let free: u64 = conn
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .map_err(storage)?;
    let page_size: u64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(storage)?;
    let reclaim = free * page_size >= 32 * 1024 * 1024;
    if reclaim {
        conn.execute_batch("VACUUM").map_err(storage)?;
    }
    if reclaim || snapshots + intents + receipts > 0 {
        storage_maintenance::checkpoint(conn)?;
    }
    if snapshots + intents + receipts > 0 {
        eprintln!(
            "DevDesk SQLite storage maintenance: {snapshots} snapshot bodies, {intents} runtime records, {receipts} receipt fields"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devdesk_core::concurrency::{PreparedChangeSet, ReadSet, ResourceKey, ResourceVersion};

    #[test]
    fn sealed_body_and_receipt_round_trip_before_and_after_maintenance() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let files = FileArtifacts::new(root.into());
        let snapshot = Snapshot {
            id: "archive-snapshot".into(),
            feature_id: None,
            project_id: None,
            task_id: None,
            kind: "manual".into(),
            content: "会话中的原始文本 🔎 ".repeat(160),
            documents: vec![],
            inherited_from: None,
            capture: SnapshotCapture {
                step_execution_ids: vec![],
                sessions: vec![],
                captured_at: Some(1),
                reason: "fixture".into(),
            },
            runtime_bindings: vec![],
            execution_instances: vec![],
        };
        let body = json!(snapshot).to_string();
        let seal = ArtifactSeal::prepare(
            "sealed-file".into(),
            &MaterialOwner::Global,
            "snapshots",
            &body,
            Origin::Human,
        )
        .unwrap();
        let store = SqliteStore::open(root).unwrap();
        let internal = Connection::open(root.join("engineering.sqlite3")).unwrap();
        let key = ResourceKey::new("snapshot", &snapshot.id);
        let artifact_key = ResourceKey::new("artifact", &seal.artifact.id);
        let command = PreparedChangeSet {
            command_id: "publish-snapshot".into(),
            fingerprint: json!(["snapshot.save", snapshot.content]).to_string(),
            read_set: ReadSet::new(vec![
                ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                },
                ResourceVersion {
                    resource: artifact_key.clone(),
                    revision: 0,
                },
            ])
            .unwrap(),
            changes: vec![
                ResourceChange {
                    resource: key.clone(),
                    value: json!(snapshot),
                },
                ResourceChange {
                    resource: artifact_key,
                    value: json!(seal.artifact),
                },
            ],
            result: json!({"id":snapshot.id}),
            time: 1,
        };
        // Until the file exists and verifies, retain the original inline value.
        let committed = store.commit_changes(&command).unwrap();
        let stored_body = |conn: &Connection| {
            conn.query_row(
                "SELECT content FROM snapshots WHERE id=?1",
                [&snapshot.id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(stored_body(&internal), snapshot.content);
        let artifact = files.seal_prepared(&seal, &body).unwrap();
        let version_json = json!(committed.versions).to_string();
        internal
            .execute(
                "UPDATE operations SET fingerprint=?1, versions=?2 WHERE id=?3",
                params![command.fingerprint, version_json, command.command_id],
            )
            .unwrap();
        internal
            .pragma_update(None, "user_version", LEGACY_PROJECT_SCHEMA_VERSION)
            .unwrap();
        drop(internal);
        drop(store);

        let store = SqliteStore::open(root).unwrap();
        let conn = store.connection.read().unwrap();
        assert!(stored_body(&conn).starts_with(SNAPSHOT_FILE));
        assert_eq!(
            store.read_resources(&[key]).unwrap()[0].value,
            Some(json!(snapshot))
        );
        for length in [20, 500] {
            let (_, prefix) = snapshot_excerpt(&conn, &snapshot.id, length)
                .unwrap()
                .unwrap();
            assert_eq!(
                prefix,
                snapshot.content.chars().take(length).collect::<String>()
            );
        }
        let replay = store
            .replay_commit(&command.command_id, &command.fingerprint)
            .unwrap()
            .unwrap();
        assert_eq!(json!(replay), json!(committed));
        assert_eq!(
            store
                .replay_commit(&command.command_id, "different fingerprint")
                .unwrap_err()
                .code,
            "invalid_params",
        );
        assert!(
            conn.execute("UPDATE snapshots SET content='changed'", [])
                .is_err()
        );
        fs::write(root.join(artifact.relative_path), "damaged").unwrap();
        let key = ResourceKey::new("snapshot", &snapshot.id);
        assert!(store.read_resources(&[key]).is_err());
    }

    #[test]
    fn trimming_requires_the_exact_original_commit_receipt() {
        let temp = tempfile::tempdir().unwrap();
        drop(SqliteStore::open(temp.path()).unwrap());
        let mut conn = Connection::open(temp.path().join("engineering.sqlite3")).unwrap();
        let original = json!({"status":"completed","command_id":"cmd","fingerprint":"fingerprint",
            "result":{"id":"out"},"read_set":[],"changes":[{"value":"runtime payload"}],"seals":[]});
        for (id, status) in [("finished", "completed"), ("pending", "prepared")] {
            let mut data = original.clone();
            data["status"] = json!(status);
            conn.execute(
                "INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at)
                 VALUES(?1,'sealed_command_intent',?1,'\"global\"','fixture',1,?2,'{}',?3,0,1,1)",
                params![id, content_digest(&References::new(), &data), data.to_string()],
            ).unwrap();
        }
        let load = |conn: &Connection, id: &str| {
            let value: Value =
                load_table_filtered_params(conn, "governance_records", "id=?1", &[&id])
                    .unwrap()
                    .pop()
                    .unwrap();
            ConfigurationRecord::from_storage(value).unwrap()
        };
        maintain(&mut conn, temp.path()).unwrap();
        assert_eq!(load(&conn, "finished").data, original);
        conn.execute(
            "INSERT INTO operations (id,fingerprint,result) VALUES('cmd','fingerprint',?1)",
            [json!({"id":"out"}).to_string()],
        )
        .unwrap();
        maintain(&mut conn, temp.path()).unwrap();
        let finished = load(&conn, "finished");
        assert_eq!(
            finished.data,
            json!({"status":"completed","command_id":"cmd","fingerprint":"fingerprint","seals":[]})
        );
        assert_eq!(
            finished.content_hash,
            content_digest(&finished.refs, &finished.data)
        );
        assert_eq!(load(&conn, "pending").data["changes"], original["changes"]);
    }
}
