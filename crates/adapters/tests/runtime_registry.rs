use devdesk_adapters::GlobalStore;
use devdesk_domain::*;
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
        "devdesk-source-v2\n",
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
