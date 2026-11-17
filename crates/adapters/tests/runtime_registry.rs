use devdesk_adapters::GlobalStore;
use devdesk_core::{Command, EngineeringStore, InvocationContext};
use devdesk_domain::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Arc};

fn native(path: &Path, suffix: &[u8]) -> RuntimeRegistration {
    // Explicit synthetic ELF fixture; no execution, only artifact integrity is tested.
    let mut bytes = vec![0; 64];
    bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
    bytes[18..20].copy_from_slice(&183_u16.to_le_bytes());
    bytes.extend_from_slice(suffix);
    fs::write(path, &bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    RuntimeRegistration {
        key: RuntimeKey {
            provider: HarnessProvider::Codex,
            version: "1.0+fixture".into(),
            os: "linux".into(),
            arch: "aarch64".into(),
        },
        artifact: RuntimeArtifact::Executable {
            canonical_path: path.to_string_lossy().into_owned(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
        source: "synthetic test fixture".into(),
        license: "test-only".into(),
    }
}

fn image(version: &str) -> RuntimeRegistration {
    RuntimeRegistration {
        key: RuntimeKey {
            provider: HarnessProvider::Codex,
            version: version.into(),
            os: "linux".into(),
            arch: "aarch64".into(),
        },
        artifact: RuntimeArtifact::OciImage {
            reference: format!("example.com/fixture/runtime@sha256:{}", "a".repeat(64)),
        },
        source: "immutable image protocol fixture; no process is run".into(),
        license: "test-only".into(),
    }
}
fn request(id: &str, registration: &RuntimeRegistration) -> Command {
    Command {
        command_id: id.into(),
        target: None,
        expected_revision: Some(0),
        payload: json!(registration),
    }
}

#[test]
fn concurrent_public_creations_have_one_winner_and_independent_installations_all_commit() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(temp.path()).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let handles: Vec<_> = (0..100)
        .map(|n| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                let command = request(&format!("racing-{n}"), &image("same"));
                barrier.wait();
                store.register_runtime_command(&command, &InvocationContext::human(None))
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| result
                .as_ref()
                .is_err_and(|error| error.code == "revision_conflict"))
            .count(),
        99
    );
    let handles: Vec<_> = (0..100)
        .map(|n| {
            let store = store.clone();
            std::thread::spawn(move || {
                store.register_runtime_command(
                    &request(&format!("independent-{n}"), &image(&format!("version-{n}"))),
                    &InvocationContext::human(None),
                )
            })
        })
        .collect();
    for thread in handles {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(store.runtime_installations().unwrap().len(), 101);
    let page = store.events_after(0, 100).unwrap();
    assert_eq!(page.len(), 100);
    assert_eq!(
        store
            .events_after(page.last().unwrap().id, 100)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn original_receipts_replay_without_rechecking_the_external_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(&temp.path().join("data")).unwrap());
    let registration = native(&temp.path().join("original"), b"content");
    let command = request("original-command", &registration);
    let context = InvocationContext::human(None);
    let saved = store.register_runtime_command(&command, &context).unwrap();
    fs::remove_file(temp.path().join("original")).unwrap();
    let handles: Vec<_> = (0..100)
        .map(|_| {
            let (store, command) = (store.clone(), command.clone());
            std::thread::spawn(move || {
                store.register_runtime_command(&command, &InvocationContext::human(None))
            })
        })
        .collect();
    for handle in handles {
        let receipt = handle.join().unwrap().unwrap();
        assert_eq!(receipt.result, saved.result);
        assert_eq!(receipt.cursor, saved.cursor);
        assert_eq!(receipt.versions, saved.versions);
    }
    assert_eq!(store.events_after(0, 100).unwrap().len(), 1);
    let mut reused = command.clone();
    reused.payload["source"] = json!("changed input");
    assert_eq!(
        store
            .register_runtime_command(&reused, &context)
            .unwrap_err()
            .code,
        "invalid_params"
    );
    let mut missing = request("missing", &image("another"));
    missing.expected_revision = None;
    assert_eq!(
        store
            .register_runtime_command(&missing, &context)
            .unwrap_err()
            .code,
        "precondition_required"
    );
}

#[test]
fn missing_registry_repair_preserves_the_seal_tombstone_and_original_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(temp.path()).unwrap());
    let registration = image("sealed");
    let command = request("original-sealed", &registration);
    let original = store
        .register_runtime_command(&command, &InvocationContext::human(None))
        .unwrap();
    let id = original.result["id"].as_str().unwrap().to_owned();
    // Fault fixture: the verified artifact cache and version/seal survive a missing row.
    let conn = rusqlite::Connection::open(temp.path().join("global.sqlite3")).unwrap();
    conn.execute("DELETE FROM runtime_installations WHERE id=?1", [&id])
        .unwrap();
    let mut forged = registration.clone();
    forged.artifact = RuntimeArtifact::OciImage {
        reference: format!("example.com/fixture/runtime@sha256:{}", "b".repeat(64)),
    };
    assert!(store.register_runtime(forged).is_err());
    assert!(store.runtime_installations().unwrap().is_empty());
    let handles: Vec<_> = (0..100)
        .map(|_| {
            let (store, registration) = (store.clone(), registration.clone());
            std::thread::spawn(move || store.register_runtime(registration))
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap().unwrap().id, id);
    }
    assert_eq!(
        store
            .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
                "runtime_installation",
                &id
            )])
            .unwrap()[0]
            .revision,
        2
    );
    assert_eq!(store.events_after(0, 100).unwrap().len(), 2);
    let replay = store
        .register_runtime_command(&command, &InvocationContext::human(None))
        .unwrap();
    assert_eq!(replay.versions, original.versions);
    assert_eq!(replay.cursor, original.cursor);
}

#[test]
fn independent_resource_commits_do_not_wait_for_the_legacy_writer_or_overwrite_events() {
    use std::{sync::mpsc, time::Duration};
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(temp.path()).unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let legacy = store.clone();
    let writer = std::thread::spawn(move || {
        legacy.seed_fixture_state(None, &mut |state| {
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            state.events.push(Event {
                id: state.events.last().map_or(1, |e| e.id + 1),
                target: "legacy".into(),
                kind: "legacy.after_resources".into(),
                detail: "append fixture".into(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let finite = store.clone();
    let worker = std::thread::spawn(move || {
        let trace = devdesk_adapters::performance::Trace::start();
        let runtime = finite.register_runtime_command(
            &request("new-installation", &image("isolated")),
            &InvocationContext::human(None),
        );
        let settings = finite.update_preferences(Some(6), None);
        done_tx.send((runtime, settings, trace.finish())).unwrap();
    });
    let result = done_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    worker.join().unwrap();
    let (runtime, settings, trace) =
        result.expect("finite commits waited for the global business writer");
    let receipt = runtime.unwrap();
    settings.unwrap();
    for span in [
        "global.full_snapshot",
        "global.transaction",
        "source.restore_records",
    ] {
        assert!(trace["spans"][span].is_null(), "{trace}");
    }
    let events = store.events_after(0, 100).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].id, receipt.cursor);
    assert_eq!(events[0].kind, "resource.changed");
    assert_eq!(events[1].kind, "resource.changed");
    assert_eq!(events[2].kind, "legacy.after_resources");
    assert!(events.windows(2).all(|pair| pair[0].id < pair[1].id));
}

#[test]
fn registration_deduplicates_across_paths_and_threads_then_detects_drift() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(&temp.path().join("data")).unwrap());
    let a = native(&temp.path().join("a"), b"same");
    let b = native(&temp.path().join("b"), b"same");
    let first = store.register_runtime(a.clone()).unwrap();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let b = b.clone();
            std::thread::spawn(move || store.register_runtime(b).unwrap().id)
        })
        .collect();
    for thread in threads {
        assert_eq!(first.id, thread.join().unwrap());
    }
    assert_eq!(store.runtime_installations().unwrap().len(), 1);
    let integrity = store.runtime_integrity(&first.id).unwrap();
    assert!(integrity.verified);
    assert!(!integrity.inference_verified);
    let conflict = native(&temp.path().join("c"), b"different");
    assert!(store.register_runtime(conflict).is_err());
    fs::write(temp.path().join("a"), b"changed").unwrap();
    assert!(!store.runtime_integrity(&first.id).unwrap().verified);
    drop(store);
    let store = GlobalStore::open(&temp.path().join("data")).unwrap();
    assert_eq!(store.runtime_installations().unwrap()[0], first);
    assert!(temp.path().join("b").is_file());
}

#[test]
fn reject_wrong_platform_hash_scripts_and_mutable_images() {
    let temp = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(&temp.path().join("data")).unwrap();
    let mut registration = native(&temp.path().join("binary"), b"payload");
    registration.key.os = "macos".into();
    assert!(store.register_runtime(registration.clone()).is_err());
    registration.key.os = "linux".into();
    registration.key.arch = "x86_64".into();
    assert!(store.register_runtime(registration.clone()).is_err());
    registration.key.arch = "aarch64".into();
    if let RuntimeArtifact::Executable { sha256, .. } = &mut registration.artifact {
        *sha256 = "0".repeat(64);
    }
    assert!(store.register_runtime(registration.clone()).is_err());
    let script=b"#!/usr/bin/env node\n// incomplete installation\n                                             ";
    fs::write(temp.path().join("script"), script).unwrap();
    registration.artifact = RuntimeArtifact::Executable {
        canonical_path: temp.path().join("script").to_string_lossy().into_owned(),
        sha256: format!("{:x}", Sha256::digest(script)),
    };
    assert!(store.register_runtime(registration.clone()).is_err());
    registration.artifact = RuntimeArtifact::OciImage {
        reference: "codex:latest".into(),
    };
    assert!(store.register_runtime(registration.clone()).is_err());
    registration.artifact = RuntimeArtifact::OciImage {
        reference: format!("example.com/test/runtime@sha256:{}", "a".repeat(64)),
    };
    let first = store.register_runtime(registration.clone()).unwrap();
    assert!(!store.runtime_integrity(&first.id).unwrap().verified);
    registration.key.arch = "x86_64".into();
    assert_ne!(first.id, store.register_runtime(registration).unwrap().id);
}

#[test]
fn current_settings_and_preference_updates_are_atomic() {
    let temp = tempfile::tempdir().unwrap();
    // Initialize a v1 global database with user configuration before the application opens it.
    fs::create_dir_all(temp.path().join("data")).unwrap();
    fs::write(
        temp.path().join("data/.devdesk-dataset"),
        "devdesk-profile-runtime-v2\n",
    )
    .unwrap();
    let conn = rusqlite::Connection::open(temp.path().join("data/global.sqlite3")).unwrap();
    conn.execute_batch(include_str!("../../../sql/global/schema.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('layout_version','2')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('global_concurrency','7')",
        [],
    )
    .unwrap();
    drop(conn);
    let store = GlobalStore::open(&temp.path().join("data")).unwrap();
    assert_eq!(store.concurrency().unwrap(), 7);
    assert_eq!(store.default_harness().unwrap(), HarnessProvider::Codex);
    store
        .update_preferences(None, Some(HarnessProvider::Dsh))
        .unwrap();
    assert!(
        store
            .update_preferences(Some(0), Some(HarnessProvider::Codex))
            .is_err()
    );
    assert_eq!(store.default_harness().unwrap(), HarnessProvider::Dsh);
    assert_eq!(store.concurrency().unwrap(), 7);
    drop(store);
    assert_eq!(
        GlobalStore::open(&temp.path().join("data"))
            .unwrap()
            .default_harness()
            .unwrap(),
        HarnessProvider::Dsh
    );
}
