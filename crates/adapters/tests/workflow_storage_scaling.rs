//! Opt-in storage attribution, not a business or capacity acceptance run.
//! Only fresh explicitly named datasets; no model, worker or external effect.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{concurrency::*, *};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, sync::Arc, time::Instant};

fn command(engine: &Engine, method: &str, target: Option<&str>, payload: Value) -> Value {
    let state = engine.store.snapshot().unwrap();
    let revision = target.and_then(|id| {
        state
            .configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| {
                state
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.revision)
            })
    });
    engine
        .execute(
            method,
            Command {
                command_id: id(),
                target: target.map(str::to_owned),
                expected_revision: revision,
                payload,
            },
        )
        .unwrap()
}

fn create_workflow(engine: &Engine, steps: usize) -> String {
    let definitions: Vec<_> = (0..steps).map(|n| json!({
        "key":format!("step{n}"),"name":"Storage attribution control","stage":"work",
        "dependencies":if n == 0 { vec![] } else { vec![format!("step{}", n - 1)] },
        "executor":{"type":"control"},"budget":{"max_turns":8,"timeout_seconds":300,"retry_limit":10}
    })).collect();
    let draft = command(
        engine,
        "workflow.create",
        None,
        json!({"name":"Storage attribution graph","document":{"schema_version":6,"stages":[{"key":"work","name":"work"}],"steps":definitions}}),
    );
    command(engine, "workflow.publish", draft["id"].as_str(), json!({}))["id"]
        .as_str()
        .unwrap()
        .into()
}

fn start(engine: &Engine, project: &str, workflow: &str) -> String {
    command(
        engine,
        "workflow.start",
        Some(project),
        json!({"workflow_ref":workflow,"role_agent_bindings":{}}),
    )["id"]
        .as_str()
        .unwrap()
        .into()
}

fn run_data(engine: &Engine, root: &str) -> Value {
    engine
        .store
        .snapshot()
        .unwrap()
        .configuration(root)
        .unwrap()
        .data
        .clone()
}

fn process_io() -> Value {
    // Linux accounting, not a claim about physical device traffic. Absence is explicit.
    let Ok(text) = fs::read_to_string("/proc/self/io") else {
        return Value::Null;
    };
    json!(
        text.lines()
            .filter_map(|line| {
                let (key, value) = line.split_once(':')?;
                Some((key, value.trim().parse::<u64>().ok()?))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    )
}

fn memory() -> Value {
    let Ok(text) = fs::read_to_string("/proc/self/status") else {
        return Value::Null;
    };
    json!(
        text.lines()
            .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
            .collect::<Vec<_>>()
    )
}

fn checkpoint(conn: &Connection) -> Value {
    let began = Instant::now();
    let (busy, frames, checkpointed) = conn
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })
        .unwrap();
    json!({"busy":busy,"log_frames":frames,"checkpointed_frames":checkpointed,"ms":began.elapsed().as_secs_f64()*1000.0})
}

fn samples(
    output: &mut fs::File,
    case: &str,
    stage: &str,
    mut operation: impl FnMut(usize) -> usize,
) {
    // One unmeasured warmup, retained as a separate row. Thirty diagnostic samples.
    for sample in 0..=30 {
        let trace = performance::Trace::start();
        let began = Instant::now();
        let bytes = operation(sample);
        let ms = began.elapsed().as_secs_f64() * 1000.0;
        let measurement = trace.finish();
        writeln!(output, "{}", json!({"case":case,"stage":stage,"warmup":sample==0,"sample":sample,"ms":ms,"bytes":bytes,"measurement":measurement})).unwrap();
    }
}

fn profile(
    output: &mut fs::File,
    directory: &Path,
    case: &str,
    cold: usize,
    dag: usize,
    attempts: usize,
) {
    fs::create_dir(directory).unwrap();
    let engine = Engine::new(
        Arc::new(SqliteStore::open_fixture(directory).unwrap()),
        Arc::new(FileArtifacts::new(directory.into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    runtime::bind_default_permissions(&engine);
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Storage profile"}),
    );
    let project = project["id"].as_str().unwrap();
    let cold_workflow = create_workflow(&engine, 32);
    // Real cancelled roots, not duplicated rows or a larger active DAG in disguise.
    for _ in 0..cold {
        let root = start(&engine, project, &cold_workflow);
        command(&engine, "workflow.cancel", Some(&root), json!({}));
        for _ in 0..128 {
            engine.evaluate_workflows().unwrap();
            if run_data(&engine, &root)["status"] == "cancelled" {
                break;
            }
        }
        assert_eq!(run_data(&engine, &root)["status"], "cancelled");
    }
    let workflow = if dag == 32 {
        cold_workflow
    } else {
        create_workflow(&engine, dag)
    };
    let root = start(&engine, project, &workflow);
    // Known quiescent Control executions can be stopped/retried without external I/O.
    // The production retry command archives each actual predecessor.
    for _ in 0..attempts {
        let execution = run_data(&engine, &root)["steps"][0]["id"].clone();
        command(
            &engine,
            "workflow.execution.stop",
            Some(&root),
            json!({"execution_id":execution,"reason":"Explicit storage profile retry"}),
        );
        command(
            &engine,
            "workflow.step.retry",
            Some(&root),
            json!({"execution_id":execution}),
        );
    }
    let state = engine.store.snapshot().unwrap();
    let run = &state.configuration(&root).unwrap().data;
    assert_eq!(run["steps"].as_array().unwrap().len(), dag);
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_execution_history")
            .count(),
        attempts
    );
    let key = ResourceKey::new(
        "workflow_step",
        json!([root, run["steps"][0]["id"]]).to_string(),
    );
    drop(state);
    let original = engine
        .store
        .read_resources(std::slice::from_ref(&key))
        .unwrap()
        .remove(0);
    let encoded = serde_json::to_vec(original.value.as_ref().unwrap()).unwrap();
    let query = json!({"project_id":project,"id":root,"view":"status","step_limit":25});
    let database = directory.join("engineering.sqlite3");
    let conn = Connection::open(&database).unwrap();
    let canonical_bytes: u64 = conn
        .query_row(
            "SELECT length(CAST(data AS BLOB)) FROM governance_records WHERE id=?1",
            [&root],
            |r| r.get(0),
        )
        .unwrap();
    let active_writers: u64 = conn
        .query_row(
            "SELECT count(*) FROM executions WHERE writer_revoked=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_writers, 0);
    writeln!(output, "{}", json!({"case":case,"stage":"identity","cold_instances":cold,"dag_steps":dag,"historical_attempts":attempts,"project_id":project,"root_id":root,"resource":key,"canonical_instance_bytes":canonical_bytes,"step_bytes":encoded.len(),"body_group":"empty Control execution input/output; no rich Artifact","active_writers":active_writers,"page_size":25,"memory_after_seed":memory()})).unwrap();
    samples(output, case, "read_single_step", |_| {
        let rows = engine
            .store
            .read_resources(std::slice::from_ref(&key))
            .unwrap();
        assert_eq!(rows[0].value, original.value);
        encoded.len()
    });
    samples(output, case, "status_page_and_encode", |_| {
        serde_json::to_vec(&engine.query("query.workflow_instance", &query, 4).unwrap())
            .unwrap()
            .len()
    });
    samples(output, case, "encode_step", |_| {
        serde_json::to_vec(original.value.as_ref().unwrap())
            .unwrap()
            .len()
    });
    samples(output, case, "decode_step", |_| {
        let value: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(value, *original.value.as_ref().unwrap());
        encoded.len()
    });
    // Reset only this fresh profile's WAL before an isolated fixed batch. No reader
    // snapshot pins it, and no checkpoint runs inside the timed commit samples.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let io_before = process_io();
    let mut commits = Vec::new();
    for sample in 0..31 {
        let current = engine
            .store
            .read_resources(std::slice::from_ref(&key))
            .unwrap()
            .remove(0);
        let mut value = current.value.unwrap();
        value["reason"] = json!(if sample % 2 == 0 {
            "Storage profile A"
        } else {
            "Storage profile B"
        });
        let prepared = PreparedChangeSet {
            command_id: id(),
            fingerprint: format!("storage-profile:{sample}"),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: current.revision,
            }])
            .unwrap(),
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: value.clone(),
            }],
            result: json!({}),
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        };
        let trace = performance::Trace::start();
        let began = Instant::now();
        let receipt = engine.store.commit_changes(&prepared).unwrap();
        let ms = began.elapsed().as_secs_f64() * 1000.0;
        let measurement = trace.finish();
        assert_eq!(receipt.versions[0].revision, current.revision + 1);
        let readback = engine
            .store
            .read_resources(std::slice::from_ref(&key))
            .unwrap()
            .remove(0);
        assert_eq!(readback.value, Some(value));
        commits.push(json!({"case":case,"stage":"commit_step_reason","sample":sample,"warmup":sample==0,"ms":ms,"measurement":measurement,"cursor":receipt.cursor}));
    }
    let io_after = process_io();
    let wal_bytes = fs::metadata(directory.join("engineering.sqlite3-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    let after = checkpoint(&conn);
    for row in commits {
        writeln!(output, "{row}").unwrap();
    }
    let quick_check: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .unwrap();
    let fk_errors: usize = conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(quick_check, "ok");
    assert_eq!(fk_errors, 0);
    writeln!(output,"{}",json!({"case":case,"stage":"batch_storage","commits_including_warmup":31,"wal_file_bytes":wal_bytes,"checkpoint":after,"process_io_before":io_before,"process_io_after":io_after,"memory_after_samples":memory(),"quick_check":quick_check,"foreign_key_errors":fk_errors,"scope":"One serial trusted storage resource patch per sample, not business authorization, contention or capacity. IO includes readback, excludes result-file writes and seeding."})).unwrap();
}

#[test]
#[ignore = "explicit fresh-dataset storage profile; DEVDESK_STORAGE_PROFILE_OUTPUT must name a new directory"]
fn workflow_storage_growth_profile() {
    let directory = std::path::PathBuf::from(
        std::env::var("DEVDESK_STORAGE_PROFILE_OUTPUT")
            .expect("explicit new output directory required"),
    );
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join(".workflow-storage-profile"),
        "fresh diagnostic datasets only\n",
    )
    .unwrap();
    let mut output = fs::File::create_new(directory.join("samples.jsonl")).unwrap();
    for (case, cold, dag, attempts) in [
        ("base", 1, 32, 1),
        ("cold10", 10, 32, 1),
        ("dag10", 1, 320, 1),
        ("attempts10", 1, 32, 10),
    ] {
        profile(
            &mut output,
            &directory.join(case),
            case,
            cold,
            dag,
            attempts,
        );
    }
}
