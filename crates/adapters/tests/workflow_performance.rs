//! Opt-in component measurements. No model, worker, network, or user dataset.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::*;
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write, sync::Arc, time::Instant};

fn command(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Value {
    let revision = target.and_then(|id| {
        let state = e.store.snapshot().unwrap();
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
    e.execute(
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

struct ProfileEnvironment(std::sync::atomic::AtomicU64);
impl ports::ApplicationEnvironment for ProfileEnvironment {
    fn allocate_id(&self) -> String {
        format!(
            "00000000-0000-4000-8000-{:012x}",
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    fn unix_millis(&self) -> u64 {
        1794800000000
    }
}

#[test]
#[ignore = "explicit isolated workflow component benchmark; set DEVDESK_WORKFLOW_PERF_OUTPUT"]
fn workflow_component_profile() {
    with_application_environment(
        Arc::new(ProfileEnvironment(std::sync::atomic::AtomicU64::new(1))),
        workflow_component_profile_inner,
    );
}
fn workflow_component_profile_inner() {
    let output =
        std::env::var("DEVDESK_WORKFLOW_PERF_OUTPUT").expect("explicit output path required");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(directory.path()).unwrap()),
        Arc::new(FileArtifacts::new(directory.path().into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"workflow performance fixture"}),
    );
    let steps:Vec<_>=(0..64).map(|n|json!({"key":format!("step{n}"),"name":"Pending independent control","stage":"work","executor":{"type":"control"}})).collect();
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"bounded evaluation fixture","document":{"schema_version":6,"stages":[{"key":"work","name":"work"}],"steps":steps}}),
    );
    let workflow = command(&e, "workflow.publish", draft["id"].as_str(), json!({}));
    let pid = project["id"].as_str().unwrap();
    let started = command(
        &e,
        "workflow.start",
        Some(pid),
        json!({"workflow_ref":workflow["id"],"role_agent_bindings":e.configuration_defaults(Some(pid)).unwrap()["role_agent_bindings"]}),
    );
    let initial = e.store.snapshot().unwrap();
    let mut measure = |stage: &str, samples: usize, operation: &mut dyn FnMut()| {
        for sample in 0..samples {
            let trace = performance::Trace::start();
            let start = Instant::now();
            operation();
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let measurement = trace.finish();
            writeln!(
                output,
                "{}",
                json!({"stage":stage,"sample":sample,"ms":elapsed,"measurement":measurement})
            )
            .unwrap();
        }
    };
    measure("running_count", 1000, &mut || {
        assert_eq!(e.conversation_running().unwrap(), 0)
    });
    measure("idle_evaluation_64_steps", 30, &mut || {
        e.evaluate_workflows().unwrap()
    });
    measure("idle_segments", 30, &mut || {
        e.drive_workflow_segments().unwrap()
    });
    measure("idle_changes", 30, &mut || {
        e.drive_workflow_changes().unwrap()
    });
    let final_state = e.store.snapshot().unwrap();
    assert_eq!(initial.events.len(), final_state.events.len());
    assert_eq!(
        initial
            .configuration(started["id"].as_str().unwrap())
            .unwrap()
            .revision,
        final_state
            .configuration(started["id"].as_str().unwrap())
            .unwrap()
            .revision
    );
}
