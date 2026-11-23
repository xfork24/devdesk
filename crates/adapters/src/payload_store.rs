//! Project-local storage for immutable bodies selected by the SQLite codec.
use super::*;
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::FileExt;

const KEY: &str = "$devdesk_project_payload_v1";
const TEXT_PREFIX: &str = "\u{1d}devdesk-payload/v1:";
const MIN_BYTES: usize = 512;

thread_local! {
    static STAGED_DIRECTORIES: std::cell::RefCell<Option<BTreeSet<PathBuf>>> = const { std::cell::RefCell::new(None) };
    static OPEN_PACKS: std::cell::RefCell<Option<BTreeMap<PathBuf, (File, u64)>>> = const { std::cell::RefCell::new(None) };
}

pub(super) struct ReadScope(bool);

pub(super) fn read_scope() -> ReadScope {
    ReadScope(OPEN_PACKS.with_borrow_mut(|scope| {
        if scope.is_some() {
            return false;
        }
        *scope = Some(BTreeMap::new());
        true
    }))
}

impl Drop for ReadScope {
    fn drop(&mut self) {
        if self.0 {
            OPEN_PACKS.with_borrow_mut(|scope| *scope = None);
        }
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    if STAGED_DIRECTORIES.with_borrow_mut(|scope| {
        scope.as_mut().is_some_and(|dirs| {
            dirs.insert(path.to_owned());
            true
        })
    }) {
        return Ok(());
    }
    File::open(path).and_then(|f| f.sync_all()).map_err(storage)
}

/// The offline candidate cannot be published until every directory is durable.
pub(super) fn stage<T>(write: impl FnOnce() -> Result<T>) -> Result<T> {
    STAGED_DIRECTORIES.with_borrow_mut(|scope| *scope = Some(BTreeSet::new()));
    let result = write();
    let directories = STAGED_DIRECTORIES.with_borrow_mut(Option::take).unwrap();
    let value = result?;
    for dir in directories.into_iter().rev() {
        sync_directory(&dir)?;
    }
    Ok(value)
}

pub(super) fn enabled(conn: &Connection) -> Result<bool> {
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage)?;
    Ok(version == PROJECT_SCHEMA_VERSION)
}

// The leading decimal length keeps the history byte budget available in SQL.
pub(super) const CONVERSATION_BYTES: &str =
    "CASE WHEN substr(content,1,20)=char(29)||'devdesk-payload/v1:'
          THEN CAST(substr(content,21) AS INTEGER) ELSE length(CAST(content AS BLOB)) END";

#[derive(Clone, serde::Deserialize, Serialize)]
struct Body {
    sha256: String,
    bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact: Option<String>,
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn object_path(root: &Path, hash: &str, create: bool) -> Result<PathBuf> {
    require(valid_hash(hash), "无效正文摘要")?;
    let mut dir = root.to_path_buf();
    for part in ["payloads", "objects", &hash[..2], &hash[2..4]] {
        let parent = dir;
        dir = parent.join(part);
        reject_symlink(&dir)?;
        if create && !dir.is_dir() {
            match fs::create_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(storage(e)),
            }
            reject_symlink(&dir)?;
            sync_directory(&parent)?;
        }
    }
    let path = dir.join(hash);
    reject_symlink(&path)?;
    Ok(path)
}

fn verify(body: &Body, bytes: &[u8]) -> Result<()> {
    require(
        bytes.len() == body.bytes && format!("{:x}", Sha256::digest(bytes)) == body.sha256,
        "项目正文长度或摘要不符",
    )
}

fn put(root: &Path, bytes: &[u8], artifact: Option<&Artifact>) -> Result<Body> {
    let mut body = Body {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len(),
        artifact: None,
    };
    if let Some(artifact) = artifact.filter(|a| a.sha256 == body.sha256)
        && FileArtifacts::new(root.to_owned())
            .read(artifact)
            .is_ok_and(|sealed| sealed == bytes)
    {
        body.artifact = Some(artifact.id.clone());
        return Ok(body);
    }
    let path = object_path(root, &body.sha256, false)?;
    if path.is_file() {
        verify(&body, &fs::read(&path).map_err(storage)?)?;
        // A concurrent publisher may have linked the file before syncing its directory.
        sync_directory(path.parent().unwrap())?;
        return Ok(body);
    }
    if root.join("payloads/packs").is_dir() {
        let conn = Connection::open_with_flags(
            root.join("engineering.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(storage)?;
        if enabled(&conn)?
            && let Some(bytes) = packed(&conn, root, &body.sha256, body.bytes)?
        {
            verify(&body, &bytes)?;
            return Ok(body);
        }
    }
    let path = object_path(root, &body.sha256, true)?;
    let parent = path.parent().unwrap();
    let pending = parent.join(format!(".{}.pending", id()));
    let outcome = (|| -> Result<()> {
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(storage)?;
        out.write_all(bytes)
            .and_then(|()| out.sync_all())
            .map_err(storage)?;
        match fs::hard_link(&pending, &path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                reject_symlink(&path)?;
                verify(&body, &fs::read(&path).map_err(storage)?)?;
            }
            Err(e) => return Err(storage(e)),
        }
        sync_directory(parent)
    })();
    let _ = fs::remove_file(pending);
    outcome?;
    Ok(body)
}

fn read(conn: &Connection, body: &Body) -> Result<Vec<u8>> {
    let root =
        project_storage::connection_root(conn).ok_or_else(|| err("storage", "项目正文目录缺失"))?;
    let bytes = if let Some(id) = &body.artifact {
        let artifact: Artifact = load_table_filtered_params(conn, "artifacts", "id=?1", &[id])?
            .pop()
            .ok_or_else(|| err("storage", "正文引用的封存记录缺失"))?;
        require(body.sha256 == artifact.sha256, "正文引用的封存摘要改变")?;
        FileArtifacts::new(root).read(&artifact)?
    } else {
        match fs::read(object_path(&root, &body.sha256, false)?) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                packed(conn, &root, &body.sha256, body.bytes)?
                    .ok_or_else(|| err("storage", "项目正文文件缺失"))?
            }
            Err(e) => return Err(storage(e)),
        }
    };
    verify(body, &bytes)?;
    Ok(bytes)
}

fn pack_path(root: &Path, pack: &str) -> Result<PathBuf> {
    require(uuid::Uuid::parse_str(pack).is_ok(), "无效正文归档包身份")?;
    let base = root.join("payloads");
    let dir = base.join("packs");
    let path = dir.join(format!("{pack}.pack"));
    for entry in [&base, &dir, &path] {
        reject_symlink(entry)?;
    }
    Ok(path)
}

fn packed(conn: &Connection, root: &Path, hash: &str, size: usize) -> Result<Option<Vec<u8>>> {
    let entry: Option<(String, u64, usize)> = conn
        .prepare_cached("SELECT pack,offset,bytes FROM payload_pack_objects WHERE sha256=?1")
        .map_err(storage)?
        .query_row([hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(storage)?;
    let Some((pack, offset, bytes)) = entry else {
        return Ok(None);
    };
    require(bytes == size, "正文归档长度与引用不符")?;
    let extent = |file: &File, length: u64| -> Result<Option<Vec<u8>>> {
        require(
            offset
                .checked_add(bytes as u64)
                .is_some_and(|end| end <= length),
            "正文归档偏移越界",
        )?;
        let mut value = vec![0; bytes];
        file.read_exact_at(&mut value, offset).map_err(storage)?;
        Ok(Some(value))
    };
    OPEN_PACKS.with_borrow_mut(|scope| {
        let key = root.join(&pack);
        if let Some((file, length)) = scope.as_ref().and_then(|packs| packs.get(&key)) {
            return extent(file, *length);
        }
        let file = File::open(pack_path(root, &pack)?).map_err(storage)?;
        let length = file.metadata().map_err(storage)?.len();
        let value = extent(&file, length)?;
        if let Some(packs) = scope.as_mut().filter(|packs| packs.len() < 8) {
            packs.insert(key, (file, length));
        }
        Ok(value)
    })
}

fn field(root: &Path, value: &mut Value, keep: &[&str], artifact: Option<&Artifact>) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(storage)?;
    let text = std::str::from_utf8(&bytes).map_err(storage)?;
    if bytes.len() < MIN_BYTES && !text.contains(KEY) && !text.contains("devdesk-payload/v1:") {
        return Ok(());
    }
    let mut projection = serde_json::Map::new();
    for key in keep {
        if let Some(item) = value.get(*key) {
            projection.insert((*key).to_owned(), item.clone());
        }
    }
    projection.insert(KEY.into(), json!(put(root, &bytes, artifact)?));
    *value = Value::Object(projection);
    Ok(())
}

fn fields(root: &Path, value: &mut Value, paths: &[&str]) -> Result<()> {
    for path in paths {
        if let Some(body) = value.pointer_mut(path) {
            field(root, body, &[], None)?;
        }
    }
    Ok(())
}

pub(super) fn result(root: &Path, value: &mut Value) -> Result<()> {
    if value.get(KEY).is_none() && value["execution"].is_object() {
        fields(
            root,
            &mut value["execution"],
            &["/inputs", "/rework_workspace", "/runtime_context"],
        )?;
    } else {
        field(root, value, &[], None)?;
    }
    Ok(())
}

pub(super) fn step(root: &Path, value: &mut Value) -> Result<()> {
    fields(
        root,
        value,
        &["/inputs", "/rework_workspace", "/runtime_context"],
    )
}

// SQL-facing fields stay in the small projection. Only these positions carry
// references, so a literal object elsewhere cannot become a file read.
fn governance_fields(
    kind: &str,
    data: &mut Value,
    mut visit: impl FnMut(&mut Value, &[&str]) -> Result<()>,
) -> Result<()> {
    let paths: &[&str] = match kind {
        "workflow_output" => {
            if let Some(value) = data.get_mut("value") {
                visit(value, &["owner_id", "workspace_id", "revision"])?;
            }
            return Ok(());
        }
        "workflow_instance" => {
            for e in data
                .get_mut("steps")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                visit_step(e, &mut visit)?;
            }
            &["/inputs"]
        }
        "workflow_execution_history" => return visit_step(data, visit),
        "workflow_change" => &["/preview"],
        "platform_call" => {
            if let Some(receipt) = data.get_mut("receipt") {
                visit(receipt, &["workspace_id", "revision", "status"])?;
            }
            &["/request/arguments"]
        }
        "platform_effect_intent" => &["/plan/action", "/plan/basis"],
        "platform_effect_observation" => {
            if let Some(value) = data.pointer_mut("/result/Ok") {
                visit(
                    value,
                    &[
                        "head",
                        "head_branch",
                        "branch",
                        "number",
                        "merge_commit",
                        "merged",
                    ],
                )?;
            }
            &["/result/Err"]
        }
        "controller_action_observation" => &["/result/Ok", "/result/Err"],
        "controller_tool_call" => &["/arguments", "/receipt"],
        "platform_reply_intent"
        | "platform_tool_reply"
        | "controller_reply_grant"
        | "controller_tool_reply" => &["/receipt"],
        "provider_poll_observation" => &["/session", "/value", "/authority", "/turn"],
        "workflow_intent" => &["/command/payload/arguments", "/command/payload/input"],
        "command_context" => &["/request_digest"],
        "sealed_command_intent" => &["/fingerprint", "/changes"],
        "configured_execution_task" | "archived_configured_execution_task" => {
            if let Some(task) = data.get_mut("task") {
                visit(task, &["$source"])?;
            }
            &["/rendered_input"]
        }
        "conversation_capture_intent" | "captured_experience_intent" => &["/session"],
        "provider_observation_batch" => &["/checkpoint"],
        "maintenance_recovery_archive" => &["/originals"],
        "workflow_reconciliation_observation" => &["/value"],
        _ => &[],
    };
    visit_paths(data, paths, visit)
}

const STEP_BODIES: &[&str] = &["/inputs", "/rework_workspace", "/runtime_context"];
const COMMAND_BODIES: &[&str] = &["/3/arguments", "/3/input"];

fn visit_paths(
    value: &mut Value,
    paths: &[&str],
    mut visit: impl FnMut(&mut Value, &[&str]) -> Result<()>,
) -> Result<()> {
    for path in paths {
        if let Some(body) = value.pointer_mut(path) {
            visit(body, &[])?;
        }
    }
    Ok(())
}

fn visit_step(
    value: &mut Value,
    visit: impl FnMut(&mut Value, &[&str]) -> Result<()>,
) -> Result<()> {
    visit_paths(value, STEP_BODIES, visit)
}

pub(super) fn governance(
    root: &Path,
    kind: &str,
    data: &mut Value,
    artifact: Option<&Artifact>,
) -> Result<()> {
    governance_fields(kind, data, |value, keep| {
        if kind == "command_context" {
            // Retain the exact audit text while sharing its large literal parameters.
            let canonical = value.as_str().and_then(|s| {
                serde_json::from_str::<Value>(s).ok().filter(|v| {
                    v.is_array() && serde_json::to_string(v).is_ok_and(|encoded| encoded == s)
                })
            });
            if let Some(mut command) = canonical {
                fields(root, &mut command, COMMAND_BODIES)?;
                let encoded = format!("{TEXT_PREFIX}json:{}", command);
                if encoded.len() < value.as_str().unwrap().len() {
                    *value = json!(encoded);
                    return Ok(());
                }
            }
        }
        field(
            root,
            value,
            keep,
            if kind == "workflow_output" {
                artifact
            } else {
                None
            },
        )
    })
}

pub(super) fn restore_governance(conn: &Connection, kind: &str, data: &mut Value) -> Result<()> {
    governance_fields(kind, data, |value, _| {
        if kind == "command_context"
            && let Some(text) = value.as_str().filter(|s| s.starts_with(TEXT_PREFIX))
        {
            *value = json!(unpack_text(conn, text.to_owned())?);
        } else {
            restore_field(conn, value)?;
        }
        Ok(())
    })
}

pub(super) fn restore_step(conn: &Connection, value: &mut Value) -> Result<()> {
    visit_step(value, |value, _| restore_field(conn, value))
}

pub(super) fn pack_text(root: &Path, text: &str) -> Result<String> {
    if text.len() < MIN_BYTES && !text.starts_with(TEXT_PREFIX) {
        return Ok(text.into());
    }
    let body = put(root, text.as_bytes(), None)?;
    Ok(format!("{TEXT_PREFIX}{}:{}", body.bytes, body.sha256))
}

pub(super) fn unpack_text(conn: &Connection, mut text: String) -> Result<String> {
    let Some(marker) = text.strip_prefix(TEXT_PREFIX) else {
        return Ok(text);
    };
    if !enabled(conn)? {
        return Ok(text);
    }
    if let Some(encoded) = marker.strip_prefix("json:") {
        let mut value: Value = serde_json::from_str(encoded).map_err(storage)?;
        visit_paths(&mut value, COMMAND_BODIES, |v, _| restore_field(conn, v))?;
        return Ok(value.to_string());
    }
    let (size, hash) = marker
        .split_once(':')
        .ok_or_else(|| err("storage", "无效正文引用"))?;
    let body = Body {
        sha256: hash.into(),
        bytes: size.parse().map_err(storage)?,
        artifact: None,
    };
    text.clear();
    String::from_utf8(read(conn, &body)?).map_err(storage)
}

fn restore_field(conn: &Connection, value: &mut Value) -> Result<()> {
    if let Some(pointer) = value.get(KEY)
        && enabled(conn)?
    {
        let body = serde_json::from_value(pointer.clone()).map_err(storage)?;
        *value = serde_json::from_slice(&read(conn, &body)?).map_err(storage)?;
    }
    Ok(())
}

pub(super) fn decode_result(conn: &Connection, raw: &str) -> Result<Value> {
    let mut value: Value = serde_json::from_str(raw).map_err(storage)?;
    if value.get(KEY).is_some() {
        restore_field(conn, &mut value)?;
    } else if let Some(execution) = value.get_mut("execution") {
        restore_step(conn, execution)?;
    }
    Ok(value)
}

pub(super) fn decode_step(conn: &Connection, raw: &str) -> Result<Value> {
    let mut value: Value = serde_json::from_str(raw).map_err(storage)?;
    restore_step(conn, &mut value)?;
    Ok(value)
}

fn mark(value: &Value, live: &mut BTreeMap<String, Body>) -> Result<()> {
    if let Some(pointer) = value.get(KEY) {
        let body: Body = serde_json::from_value(pointer.clone()).map_err(storage)?;
        require(valid_hash(&body.sha256), "无效正文引用摘要")?;
        if body.artifact.is_none() {
            live.insert(body.sha256.clone(), body);
        }
        return Ok(());
    }
    match value {
        Value::Object(values) => {
            for value in values.values() {
                mark(value, live)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                mark(value, live)?;
            }
        }
        Value::String(text) => {
            if let Some(encoded) = text.strip_prefix(TEXT_PREFIX) {
                if let Some(json) = encoded.strip_prefix("json:") {
                    mark(&serde_json::from_str(json).map_err(storage)?, live)?;
                } else {
                    let (bytes, hash) = encoded
                        .split_once(':')
                        .ok_or_else(|| err("storage", "无效正文引用"))?;
                    require(valid_hash(hash), "无效正文引用摘要")?;
                    live.insert(
                        hash.into(),
                        Body {
                            sha256: hash.into(),
                            bytes: bytes.parse().map_err(storage)?,
                            artifact: None,
                        },
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn loose_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let objects = root.join("payloads/objects");
    reject_symlink(&root.join("payloads"))?;
    reject_symlink(&objects)?;
    if !objects.is_dir() {
        return Ok(paths);
    }
    for first in fs::read_dir(objects).map_err(storage)? {
        let first = first.map_err(storage)?;
        if !first.file_type().map_err(storage)?.is_dir() {
            continue;
        }
        for second in fs::read_dir(first.path()).map_err(storage)? {
            let second = second.map_err(storage)?;
            if !second.file_type().map_err(storage)?.is_dir() {
                continue;
            }
            for object in fs::read_dir(second.path()).map_err(storage)? {
                let object = object.map_err(storage)?;
                if !object.file_type().map_err(storage)?.is_file() {
                    continue;
                }
                let path = object.path();
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if (valid_hash(name) && object_path(root, name, false)? == path)
                    || name
                        .strip_prefix('.')
                        .and_then(|s| s.strip_suffix(".pending"))
                        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
                {
                    paths.push(path);
                }
            }
        }
    }
    Ok(paths)
}

fn publish_pack(conn: &mut Connection, root: &Path, bodies: &[&Body]) -> Result<()> {
    let pack = id();
    let path = pack_path(root, &pack)?;
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(storage)?;
    File::open(root.join("payloads"))
        .and_then(|f| f.sync_all())
        .map_err(storage)?;
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(storage)?;
    let pending = path.with_extension("pending");
    let outcome = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(storage)?;
        for body in bodies {
            file.write_all(&read(conn, body)?).map_err(storage)?;
        }
        file.sync_all().map_err(storage)?;
        fs::rename(&pending, &path).map_err(storage)?;
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        let mut offset = 0u64;
        for body in bodies {
            tx.execute(
                "INSERT INTO payload_pack_objects VALUES(?1,?2,?3,?4)
                 ON CONFLICT(sha256) DO UPDATE SET pack=excluded.pack,offset=excluded.offset,bytes=excluded.bytes",
                params![body.sha256,pack,offset,body.bytes],
            ).map_err(storage)?;
            offset += body.bytes as u64;
        }
        tx.commit().map_err(storage)
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(pending);
    }
    outcome
}

/// Caller owns both dataset and project locks for the entire mark and publish phase.
pub(super) fn compact(conn: &mut Connection, root: &Path) -> Result<Value> {
    let mut live = BTreeMap::new();
    for (sql, text) in [
        (
            "SELECT CAST(data AS TEXT),type FROM governance_records WHERE instr(data,'devdesk_')>0 OR instr(data,'devdesk-payload/v1:')>0",
            false,
        ),
        (
            "SELECT CAST(result AS TEXT),'' FROM operations WHERE instr(result,'devdesk_')>0",
            false,
        ),
        (
            "SELECT content,'' FROM conversation_events WHERE kind IN ('user','tool') AND substr(content,1,20)=char(29)||'devdesk-payload/v1:'",
            true,
        ),
    ] {
        let mut statement = conn.prepare(sql).map_err(storage)?;
        let mut rows = statement.query([]).map_err(storage)?;
        while let Some(row) = rows.next().map_err(storage)? {
            let raw: String = row.get(0).map_err(storage)?;
            let kind: String = row.get(1).map_err(storage)?;
            if text {
                mark(&Value::String(raw), &mut live)?;
            } else {
                let mut value: Value = serde_json::from_str(&raw).map_err(storage)?;
                if !kind.is_empty() {
                    governance_fields(&kind, &mut value, |v, _| mark(v, &mut live))?;
                } else if value.get(KEY).is_some() {
                    mark(&value, &mut live)?;
                } else if let Some(step) = value.get_mut("execution") {
                    visit_step(step, |v, _| mark(v, &mut live))?;
                }
            }
        }
    }
    let entries = {
        let mut q = conn
            .prepare("SELECT sha256,pack FROM payload_pack_objects")
            .map_err(storage)?;
        q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(storage)?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(storage)?
    };
    let stale_packs: BTreeSet<_> = entries
        .iter()
        .filter(|(hash, _)| !live.contains_key(*hash))
        .map(|(_, pack)| pack)
        .collect();
    let mut pending = Vec::new();
    let mut bytes = 0;
    let mut published = 0;
    for body in live.values() {
        let entry = entries.get(&body.sha256);
        if entry.is_some_and(|pack| !stale_packs.contains(pack)) {
            continue;
        }
        if !pending.is_empty() && bytes + body.bytes > 64 * 1024 * 1024 {
            publish_pack(conn, root, &pending)?;
            published += pending.len();
            pending.clear();
            bytes = 0;
        }
        pending.push(body);
        bytes += body.bytes;
    }
    if !pending.is_empty() {
        publish_pack(conn, root, &pending)?;
        published += pending.len();
    }
    let tx = conn.transaction().map_err(storage)?;
    for hash in entries.keys().filter(|hash| !live.contains_key(*hash)) {
        tx.execute("DELETE FROM payload_pack_objects WHERE sha256=?1", [hash])
            .map_err(storage)?;
    }
    tx.commit().map_err(storage)?;
    for body in live.values() {
        let bytes = packed(conn, root, &body.sha256, body.bytes)?
            .ok_or_else(|| err("storage", "正文归档索引缺失"))?;
        verify(body, &bytes)?;
    }
    let mut removed = 0;
    for path in loose_paths(root)? {
        // Every live hash has a committed archive location at this point.
        fs::remove_file(path).map_err(storage)?;
        removed += 1;
    }
    let dir = root.join("payloads/packs");
    reject_symlink(&dir)?;
    if dir.is_dir() {
        for entry in fs::read_dir(&dir).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if !entry.file_type().map_err(storage)?.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(pack) = path.file_stem().and_then(|p| p.to_str()) else {
                continue;
            };
            if uuid::Uuid::parse_str(pack).is_err() {
                continue;
            }
            let delete = match path.extension().and_then(|s| s.to_str()) {
                Some("pending") => true,
                Some("pack") => !conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM payload_pack_objects WHERE pack=?1)",
                        [pack],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(storage)?,
                _ => false,
            };
            if delete {
                fs::remove_file(path).map_err(storage)?;
            }
        }
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
    }
    Ok(json!({"live_objects":live.len(),"packed_objects":published,"loose_files_removed":removed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_preserves_live_results_and_collects_uncommitted_objects() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        drop(SqliteStore::open(&root).unwrap());
        let mut conn = Connection::open(root.join("engineering.sqlite3")).unwrap();
        let original = json!({"result":"内容".repeat(1024)});
        let mut stored = original.clone();
        result(&root, &mut stored).unwrap();
        let orphan = put(&root, b"uncommitted", None).unwrap();
        let body: Body = serde_json::from_value(stored[KEY].clone()).unwrap();
        conn.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES('command','original',?1)",
            [stored.to_string()],
        )
        .unwrap();
        assert_eq!(decode_result(&conn, &stored.to_string()).unwrap(), original);
        assert_eq!(compact(&mut conn, &root).unwrap()["packed_objects"], 1);
        assert!(!object_path(&root, &body.sha256, false).unwrap().exists());
        assert!(!object_path(&root, &orphan.sha256, false).unwrap().exists());
        assert_eq!(decode_result(&conn, &stored.to_string()).unwrap(), original);
        let mut replay = original.clone();
        result(&root, &mut replay).unwrap();
        assert_eq!(replay, stored);
        assert!(!object_path(&root, &body.sha256, false).unwrap().exists());
        assert_eq!(compact(&mut conn, &root).unwrap()["packed_objects"], 0);
        let pack: String = conn
            .query_row("SELECT pack FROM payload_pack_objects", [], |r| r.get(0))
            .unwrap();
        fs::write(pack_path(&root, &pack).unwrap(), b"corrupted").unwrap();
        assert!(decode_result(&conn, &stored.to_string()).is_err());
    }

    #[test]
    fn concurrent_publication_and_literal_markers_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        drop(SqliteStore::open(&root).unwrap());
        let value = json!({KEY: "user-supplied literal", "execution":{"id":"x","inputs":{"content":"v".repeat(1024)}}});
        let stored = std::thread::scope(|scope| {
            let workers = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let mut stored = value.clone();
                        result(&root, &mut stored).unwrap();
                        stored
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(stored.windows(2).all(|pair| pair[0] == pair[1]));
        let conn = Connection::open(root.join("engineering.sqlite3")).unwrap();
        assert_eq!(decode_result(&conn, &stored[0].to_string()).unwrap(), value);
        let mut untouched = value.clone();
        restore_governance(&conn, "workflow_decision", &mut untouched).unwrap();
        assert_eq!(untouched, value);
    }
}
