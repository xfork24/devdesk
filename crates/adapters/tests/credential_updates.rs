//! Versioned publication uses a test-only OS vault port. No system key is stored.
use devdesk_adapters::{FileArtifacts, GlobalStore, SqliteStore};
use devdesk_core::{
    Engine, EngineeringStore, InvocationContext, concurrency::*, credential_updates::*,
};
use devdesk_domain::{ProviderFacts, Result, configuration_hash, err};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[derive(Default)]
struct Vault {
    values: Mutex<BTreeMap<String, String>>,
    calls: AtomicUsize,
    writes: AtomicUsize,
    fail_after_store: AtomicBool,
    unavailable: AtomicBool,
    after_store: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl CredentialCandidates for Vault {
    fn ensure(
        &self,
        provider: &ProviderFacts,
        candidate_id: &str,
        secret: &str,
    ) -> Result<CredentialCandidate> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.unavailable.load(Ordering::Relaxed) {
            return Err(err(
                "credential_store_unavailable",
                "test vault unavailable",
            ));
        }
        let mut values = self.values.lock().unwrap();
        if let Some(value) = values.get(candidate_id) {
            assert_eq!(value, secret);
        } else {
            values.insert(candidate_id.into(), secret.into());
            self.writes.fetch_add(1, Ordering::Relaxed);
        }
        drop(values);
        if let Some(action) = self.after_store.lock().unwrap().take() {
            action();
        }
        if self.fail_after_store.swap(false, Ordering::Relaxed) {
            return Err(err(
                "needs_reconciliation",
                "test transport lost after storing candidate",
            ));
        }
        let reference = format!("test-vault:{candidate_id}");
        Ok(CredentialCandidate {
            reference: reference.clone(),
            status: json!({"provider":"dsh","model_provider":provider.id,"reference":reference,"configured":true,"available":true,"storage":"test_vault","auth_kind":"api_key"}),
        })
    }
}
fn fixture(root: &std::path::Path) -> (Arc<GlobalStore>, String) {
    let store = Arc::new(GlobalStore::open(root).unwrap());
    Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(root.to_owned())),
        runtime::adapters(),
    )
    .bootstrap()
    .unwrap();
    let snapshot = store.configuration_snapshot().unwrap();
    let provider = snapshot
        .state
        .governance
        .iter()
        .filter(|row| row.r#type == "model_provider" && row.data["id"] == "minimax-cn")
        .max_by_key(|row| row.version)
        .unwrap()
        .id
        .clone();
    (store, provider)
}
fn command(id: &str, provider: &str, revision: u64) -> ConfigureCredential {
    parse(&json!({"command_id":id,"expected_revision":revision,"provider":"dsh","model_provider":"minimax-cn","provider_ref":provider,"api_key":"dummy-test-key-12345"})).unwrap()
}
#[test]
fn one_hundred_original_version_publications_have_one_winner() {
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let vault = Arc::new(Vault::default());
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let (store, provider, vault, barrier) = (
                store.clone(),
                provider.clone(),
                vault.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                configure(
                    store.as_ref(),
                    vault.as_ref(),
                    &command(&format!("credential-{n}"), &provider, 0),
                    &InvocationContext::human(None),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut winners = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(receipt) => {
                winners += 1;
                assert_eq!(receipt.result["credential_version"]["revision"], 1);
            }
            Err(error) => assert_eq!(error.code, "revision_conflict", "{error}"),
        }
    }
    assert_eq!(winners, 1);
    let binding = store
        .read_resources(&[binding_key("minimax-cn")])
        .unwrap()
        .remove(0);
    assert_eq!(binding.revision, 1);
    assert!(
        binding
            .value
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("test-vault:")
    );
}
#[test]
fn one_hundred_same_commands_replay_one_publication_and_one_logical_vault_candidate() {
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let vault = Arc::new(Vault::default());
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, provider, vault, barrier) = (
                store.clone(),
                provider.clone(),
                vault.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                configure(
                    store.as_ref(),
                    vault.as_ref(),
                    &command("same-command", &provider, 0),
                    &InvocationContext::human(None),
                )
            })
        })
        .collect::<Vec<_>>();
    let receipts = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert!(receipts.iter().all(
        |receipt| receipt.cursor == receipts[0].cursor && receipt.result == receipts[0].result
    ));
    assert_eq!(vault.writes.load(Ordering::Relaxed), 1);
    assert_eq!(
        store.read_resources(&[binding_key("minimax-cn")]).unwrap()[0].revision,
        1
    );
    let request = command("same-command", &provider, 0);
    vault.unavailable.store(true, Ordering::Relaxed);
    let calls = vault.calls.load(Ordering::Relaxed);
    let replay = configure(
        store.as_ref(),
        vault.as_ref(),
        &request,
        &InvocationContext::human(None),
    )
    .unwrap();
    assert_eq!(replay.cursor, receipts[0].cursor);
    assert_eq!(vault.calls.load(Ordering::Relaxed), calls);
    let mut changed = request;
    changed.api_key = "another-dummy-test-key".into();
    assert_eq!(
        configure(
            store.as_ref(),
            vault.as_ref(),
            &changed,
            &InvocationContext::human(None)
        )
        .unwrap_err()
        .code,
        "invalid_params"
    );
}
#[test]
fn an_unknown_vault_outcome_reconciles_the_original_candidate_and_never_persists_the_secret() {
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let vault = Vault::default();
    vault.fail_after_store.store(true, Ordering::Relaxed);
    let request = command("unknown-vault-result", &provider, 0);
    assert_eq!(
        configure(
            store.as_ref(),
            &vault,
            &request,
            &InvocationContext::human(None)
        )
        .unwrap_err()
        .code,
        "needs_reconciliation"
    );
    let binding = store
        .read_resources(&[binding_key("minimax-cn")])
        .unwrap()
        .remove(0);
    assert_eq!(binding.revision, 0);
    assert!(binding.value.is_none());
    let intent = ResourceKey::new(
        "setting",
        format!(
            "credential.intent.{}",
            configuration_hash(&request.command_id)
        ),
    );
    let prepared = store
        .read_resources(std::slice::from_ref(&intent))
        .unwrap()
        .remove(0);
    assert_eq!(prepared.revision, 1);
    assert_eq!(prepared.value.unwrap()["status"], "prepared");
    let receipt = configure(
        store.as_ref(),
        &vault,
        &request,
        &InvocationContext::human(None),
    )
    .unwrap();
    assert_eq!(receipt.result["credential_version"]["revision"], 1);
    assert_eq!(vault.writes.load(Ordering::Relaxed), 1);
    assert_eq!(
        store.read_resources(&[intent]).unwrap()[0]
            .value
            .as_ref()
            .unwrap()["status"],
        "completed"
    );
    let connection = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    for table in ["settings", "operations", "events"] {
        let contents = connection
            .prepare(&format!("SELECT * FROM {table}"))
            .unwrap()
            .query_map([], |row| {
                Ok((0..row.as_ref().column_count())
                    .filter_map(|index| row.get::<_, String>(index).ok())
                    .collect::<Vec<_>>()
                    .join(" "))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join(" ");
        assert!(
            !contents.contains(&request.api_key),
            "secret persisted in {table}"
        );
    }
}
#[test]
fn a_stale_binding_or_foreign_namespace_is_rejected_before_vault_access() {
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let vault = Vault::default();
    configure(
        store.as_ref(),
        &vault,
        &command("first", &provider, 0),
        &InvocationContext::human(None),
    )
    .unwrap();
    let calls = vault.calls.load(Ordering::Relaxed);
    assert_eq!(
        configure(
            store.as_ref(),
            &vault,
            &command("stale", &provider, 0),
            &InvocationContext::human(None)
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
    assert_eq!(vault.calls.load(Ordering::Relaxed), calls);
    let project =
        SqliteStore::open_project(root.path(), "c8516249-60a3-444a-988d-e5bf0e257611").unwrap();
    assert_eq!(
        configure(
            &project,
            &vault,
            &command("wrong-scope", &provider, 0),
            &InvocationContext::human(None)
        )
        .unwrap_err()
        .code,
        "invalid_scope"
    );
    assert_eq!(vault.calls.load(Ordering::Relaxed), calls);
}

#[test]
fn a_provider_change_during_vault_io_fences_publication_and_preserves_the_old_binding() {
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let vault = Vault::default();
    let original = command("old-binding", &provider, 0);
    let first = configure(
        store.as_ref(),
        &vault,
        &original,
        &InvocationContext::human(None),
    )
    .unwrap();
    let previous = store
        .read_resources(&[binding_key("minimax-cn")])
        .unwrap()
        .remove(0);
    let publisher = store.clone();
    let original_source = store
        .read_resources(&[ResourceKey::new("source", &provider)])
        .unwrap()
        .remove(0);
    let mut successor = original_source.value.unwrap();
    successor["revision"] = json!(2);
    let successor_id = format!("{}:2", successor["identity"].as_str().unwrap());
    let head = ResourceKey::new("source_head", successor["identity"].as_str().unwrap());
    let source = ResourceKey::new("source", successor_id);
    *vault.after_store.lock().unwrap() = Some(Box::new(move || {
        // Real immutable-source publication interleaved after the vault candidate
        // is stored and before the original credential dependencies are committed.
        publisher.commit_changes(&PreparedChangeSet {
            command_id: "interleaved-provider-publication".into(),
            fingerprint: configuration_hash(&successor),
            read_set: ReadSet::new(vec![
                ResourceVersion { resource: head.clone(), revision: 1 },
                ResourceVersion { resource: source.clone(), revision: 0 },
            ]).unwrap(),
            changes: vec![
                ResourceChange { resource: source, value: successor.clone() },
                ResourceChange { resource: head, value: json!({"reference":format!("{}:2", successor["identity"].as_str().unwrap()),"revision":2}) },
            ],
            result: json!({"revision":2}), time: 1,
        }).unwrap();
    }));
    assert_eq!(
        configure(
            store.as_ref(),
            &vault,
            &command("provider-race", &provider, 1),
            &InvocationContext::human(None)
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
    let current = store
        .read_resources(&[binding_key("minimax-cn")])
        .unwrap()
        .remove(0);
    assert_eq!(current.revision, previous.revision);
    assert_eq!(current.value, previous.value);
    vault.unavailable.store(true, Ordering::Relaxed);
    let replay = configure(
        store.as_ref(),
        &vault,
        &original,
        &InvocationContext::human(None),
    )
    .unwrap();
    assert_eq!(replay.cursor, first.cursor);
    assert_eq!(replay.result, first.result);
}

#[test]
fn credential_publication_does_not_wait_for_the_legacy_writer_or_restore_catalogs() {
    use std::{sync::mpsc, time::Duration};
    let root = tempfile::tempdir().unwrap();
    let (store, provider) = fixture(root.path());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = store.clone();
    let thread = std::thread::spawn(move || {
        writer.seed_fixture_state(None, &mut |_| {
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(json!(null))
        })
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let trace = devdesk_adapters::performance::Trace::start();
    let outcome = configure(
        store.as_ref(),
        &Vault::default(),
        &command("independent-writer", &provider, 0),
        &InvocationContext::human(None),
    );
    let trace = trace.finish();
    release_tx.send(()).unwrap();
    thread.join().unwrap().unwrap();
    outcome.unwrap();
    for name in [
        "global.transaction",
        "global.connection_wait",
        "catalog.build",
        "source.restore_records",
    ] {
        assert!(trace["spans"][name].is_null(), "{trace}");
    }
}
