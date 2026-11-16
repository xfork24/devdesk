//! Read-only codec benchmark over an explicitly selected backup, never worker recovery.
use devdesk_adapters::{SqliteStore, performance};
use serde_json::json;
use std::{fs::OpenOptions, io::Write, path::PathBuf, time::Instant};

#[test]
#[ignore = "explicit read-only historical backup benchmark; no migrations or execution"]
fn frozen_history_profile() {
    let dataset =
        PathBuf::from(std::env::var("DEVDESK_HISTORY_DATASET").expect("explicit backup required"));
    let project = std::env::var("DEVDESK_HISTORY_PROJECT").expect("explicit project required");
    let output = std::env::var("DEVDESK_HISTORY_OUTPUT").expect("explicit output required");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    // Initialize shipped permission assets in a separate empty test dataset.
    let empty = tempfile::tempdir().unwrap();
    drop(SqliteStore::open(empty.path()).unwrap());
    for sample in 0..20 {
        let trace = performance::Trace::start();
        let start = Instant::now();
        let state = SqliteStore::read_project_snapshot(&dataset, &project).unwrap();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let measurement = trace.finish();
        // Equality is checked on the entire restored result, outside the timed region.
        let digest = if sample == 0 {
            Some(devdesk_domain::configuration_hash(&json!(state)))
        } else {
            None
        };
        writeln!(output,"{}",json!({"sample":sample,"ms":elapsed,"turns":state.turns.len(),"sessions":state.sessions.len(),"executions":state.executions.len(),"restored_digest":digest,"measurement":measurement})).unwrap();
    }
}
