use devdesk_adapters::{FileArtifacts, GlobalStore, SqliteStore};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
fn engine(store: Arc<dyn EngineeringStore>) -> Engine {
    Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    )
}
fn source(kind: &str, id: &str, scope: Scope, data: Value) -> SourceDefinition {
    SourceDefinition {
        r#type: kind.into(),
        identity: id.into(),
        revision: 1,
        name: id.into(),
        scope,
        refs: References::new(),
        data,
    }
}
fn spec(id: &str) -> SourceDefinition {
    source(
        "spec",
        id,
        Scope::global(),
        json!({"schema_version":98765,"body":"A reusable contract","checks":[{"key":"shape","name":"Shape","instructions":"Check actual content"}]}),
    )
}
fn apply(e: &Engine, changes: &ConfigurationChangeSet, command_id: &str) -> Result<Value> {
    let preview = e.preview_configuration_change(changes)?;
    e.execute(
        "configuration.change.apply",
        Command {
            command_id: command_id.into(),
            target: None,
            expected_revision: None,
            payload: json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
        },
    )
}
#[test]
fn reverse_order_sources_publish_atomically_and_plain_text_is_not_an_edge() {
    let temp = tempfile::tempdir().unwrap();
    let e = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    e.bootstrap().unwrap();
    let mut delivery = source(
        "delivery_type",
        "report",
        Scope::global(),
        json!({"format":"markdown","body":"not-a-ref:9"}),
    );
    delivery
        .refs
        .insert("spec_refs".into(), json!(["contract:1"]));
    let changes = ConfigurationChangeSet {
        operations: vec![
            ConfigurationOperation::Create { source: delivery },
            ConfigurationOperation::Create {
                source: spec("contract"),
            },
        ],
    };
    let result = apply(&e, &changes, "atomic").unwrap();
    assert_eq!(result["published_refs"], json!(["report:1", "contract:1"]));
    let state = e.store.snapshot().unwrap();
    assert_eq!(configuration_usage(&state, "contract:1").unwrap().len(), 1);
    assert!(
        configuration_usage(&state, "not-a-ref:9")
            .unwrap()
            .is_empty()
    );
}
#[test]
fn invalid_candidate_and_stale_preview_do_not_partially_commit() {
    let temp = tempfile::tempdir().unwrap();
    let e = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    e.bootstrap().unwrap();
    let mut delivery = source(
        "delivery_type",
        "invalid",
        Scope::global(),
        json!({"format":"markdown"}),
    );
    delivery
        .refs
        .insert("spec_refs".into(), json!(["absent:1"]));
    let bad = ConfigurationChangeSet {
        operations: vec![
            ConfigurationOperation::Create {
                source: spec("valid"),
            },
            ConfigurationOperation::Create { source: delivery },
        ],
    };
    let error = e.preview_configuration_change(&bad).unwrap_err();
    assert!(error.message.contains("/spec_refs/0"));
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .configuration("valid:1")
            .is_err()
    );
    let change = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::Create {
            source: spec("stale"),
        }],
    };
    let preview = e.preview_configuration_change(&change).unwrap();
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![ConfigurationOperation::Create {
                source: spec("other"),
            }],
        },
        "other",
    )
    .unwrap();
    let command = Command {
        command_id: "stale".into(),
        target: None,
        expected_revision: None,
        payload: json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
    };
    assert!(e.execute("configuration.change.apply", command).is_err());
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .configuration("stale:1")
            .is_err()
    );
}
#[test]
fn revisions_are_exact_and_a_committed_request_replays_after_head_changes() {
    let temp = tempfile::tempdir().unwrap();
    let e = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    e.bootstrap().unwrap();
    let change = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::Create {
            source: spec("history"),
        }],
    };
    let preview = e.preview_configuration_change(&change).unwrap();
    let command = Command {
        command_id: "publish".into(),
        target: None,
        expected_revision: None,
        payload: json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
    };
    let first = e
        .execute("configuration.change.apply", command.clone())
        .unwrap();
    for version in [2, 3] {
        let state = e.store.snapshot().unwrap();
        let old = state
            .configuration(&format!("history:{}", version - 1))
            .unwrap();
        let mut next = old.source();
        next.revision = version;
        next.data["body"] = json!(format!("revision {version}"));
        apply(
            &e,
            &ConfigurationChangeSet {
                operations: vec![ConfigurationOperation::Revise {
                    base_ref: old.id.clone(),
                    expected_revision: old.revision,
                    source: next,
                }],
            },
            &format!("next-{version}"),
        )
        .unwrap();
    }
    assert_eq!(
        e.execute("configuration.change.apply", command).unwrap(),
        first
    );
    let state = e.store.snapshot().unwrap();
    assert_ne!(
        state.configuration("history:1").unwrap().data,
        state.configuration("history:2").unwrap().data
    );
    assert!(state.configuration("history:3").is_ok());
}
struct Fault {
    inner: Arc<GlobalStore>,
    fail: Arc<AtomicBool>,
}
impl EngineeringStore for Fault {
    fn snapshot(&self) -> Result<EngineeringState> {
        self.inner.snapshot()
    }
    fn root(&self) -> String {
        self.inner.root()
    }
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        self.inner.transact(key, &mut |s| {
            let result = action(s)?;
            if self.fail.load(Ordering::SeqCst)
                && s.governance.iter().any(|r| {
                    r.r#type == "configuration_transfer" && r.data["status"] == "completed"
                })
            {
                return Err(err("injected_exit", "before global commit"));
            }
            Ok(result)
        })
    }
}
#[test]
fn staged_sources_are_hidden_and_cross_store_recovery_preserves_one_project_commit() {
    let temp = tempfile::tempdir().unwrap();
    let global_store = Arc::new(GlobalStore::open(temp.path()).unwrap());
    let fail = Arc::new(AtomicBool::new(false));
    let global = engine(Arc::new(Fault {
        inner: global_store.clone(),
        fail: fail.clone(),
    }));
    global.bootstrap().unwrap();
    let project_id = id();
    let project_store = Arc::new(SqliteStore::open_project(temp.path(), &project_id).unwrap());
    let project = engine(project_store.clone());
    project.bootstrap().unwrap();
    project
        .execute(
            "project.create",
            Command {
                command_id: "project".into(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Project"}),
            },
        )
        .unwrap();
    let mut local = source(
        "delivery_type",
        "local-report",
        Scope::project(&project_id),
        json!({"format":"markdown"}),
    );
    local
        .refs
        .insert("spec_refs".into(), json!(["transferred:1"]));
    let staged=global.stage_configuration_transfer(&project,Command{command_id:"transfer".into(),target:None,expected_revision:None,payload:json!({"global_changeset":ConfigurationChangeSet{operations:vec![ConfigurationOperation::Create{source:spec("transferred")}]},"project_changeset":ConfigurationChangeSet{operations:vec![ConfigurationOperation::Create{source:local}]}})}).unwrap();
    let transfer = staged["id"].as_str().unwrap();
    assert!(
        global
            .store
            .snapshot()
            .unwrap()
            .configuration("transferred:1")
            .is_err()
    );
    assert!(
        project
            .store
            .snapshot()
            .unwrap()
            .configuration("local-report:1")
            .is_err()
    );
    fail.store(true, Ordering::SeqCst);
    assert!(
        global
            .apply_configuration_transfer(&project, transfer)
            .is_err()
    );
    assert!(
        global
            .store
            .snapshot()
            .unwrap()
            .configuration("transferred:1")
            .is_err()
    );
    assert!(
        project
            .store
            .snapshot()
            .unwrap()
            .configuration("local-report:1")
            .is_ok()
    );
    fail.store(false, Ordering::SeqCst);
    drop(global);
    drop(project);
    drop(global_store);
    drop(project_store);
    let global = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    let project = engine(Arc::new(
        SqliteStore::open_project(temp.path(), &project_id).unwrap(),
    ));
    global.recover_configuration_transfers(&project).unwrap();
    let result = global
        .apply_configuration_transfer(&project, transfer)
        .unwrap();
    assert_eq!(result["published_refs"], json!(["transferred:1"]));
    assert_eq!(
        project
            .store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .filter(|r| r.id == "local-report:1")
            .count(),
        1
    );
    assert!(
        global
            .store
            .snapshot()
            .unwrap()
            .configuration("transferred:1")
            .is_ok()
    );
}
#[test]
fn retirement_checks_all_revisions_but_does_not_follow_free_text() {
    let temp = tempfile::tempdir().unwrap();
    let e = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    e.bootstrap().unwrap();
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![
                ConfigurationOperation::Create {
                    source: spec("retired-contract"),
                },
                ConfigurationOperation::Create {
                    source: source(
                        "skill",
                        "notes",
                        Scope::global(),
                        json!({"body":"retired-contract:1"}),
                    ),
                },
            ],
        },
        "base",
    )
    .unwrap();
    let mut second = spec("retired-contract");
    second.revision = 2;
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![ConfigurationOperation::Revise {
                base_ref: "retired-contract:1".into(),
                expected_revision: 1,
                source: second,
            }],
        },
        "second",
    )
    .unwrap();
    let mut delivery = source(
        "delivery_type",
        "consumer",
        Scope::global(),
        json!({"format":"markdown"}),
    );
    delivery
        .refs
        .insert("spec_refs".into(), json!(["retired-contract:1"]));
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![ConfigurationOperation::Create { source: delivery }],
        },
        "consumer",
    )
    .unwrap();
    let retire = ConfigurationOperation::Retire {
        reference: "retired-contract:2".into(),
        expected_revision: 1,
        reason: "Replace contract".into(),
    };
    assert!(
        e.preview_configuration_change(&ConfigurationChangeSet {
            operations: vec![retire.clone()]
        })
        .is_err()
    );
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![
                retire,
                ConfigurationOperation::Retire {
                    reference: "consumer:1".into(),
                    expected_revision: 1,
                    reason: "Remove dependent".into(),
                },
            ],
        },
        "retire",
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    assert!(configuration_retired(
        &state,
        state.configuration("retired-contract:1").unwrap()
    ));
    assert!(state.configuration("notes:1").is_ok());
    assert!(state.configuration("consumer:1").is_ok());
}
#[test]
fn work_selection_preview_has_stable_identity_and_does_not_start_a_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let pid = id();
    let e = engine(Arc::new(
        SqliteStore::open_project(temp.path(), &pid).unwrap(),
    ));
    e.bootstrap().unwrap();
    e.execute(
        "project.create",
        Command {
            command_id: "project".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"Selection"}),
        },
    )
    .unwrap();
    let flow = source(
        "workflow",
        "local-flow",
        Scope::project(&pid),
        json!({"schema_version":9999,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"check","name":"Check","stage":"work","executor":{"type":"control"}}]}),
    );
    let change = ConfigurationChangeSet {
        operations: vec![
            ConfigurationOperation::Create { source: flow },
            ConfigurationOperation::SelectWork {
                owner_id: pid.clone(),
                expected_revision: 1,
                selection: json!({"workflow_ref":"local-flow:1","permission_context_ref":"builtin-permission-context-engineering:4","role_agent_bindings":{}}),
            },
        ],
    };
    let first = e.preview_configuration_change(&change).unwrap();
    let second = e.preview_configuration_change(&change).unwrap();
    assert_eq!(first["preview_hash"], second["preview_hash"]);
    apply(&e, &change, "select").unwrap();
    let state = e.store.snapshot().unwrap();
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.id == format!("work-configuration.{pid}:1"))
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_instance")
    );
}
#[test]
fn agent_capability_skill_chain_can_be_published_in_reverse_order_without_rebinding_consumers() {
    let temp = tempfile::tempdir().unwrap();
    let e = engine(Arc::new(GlobalStore::open(temp.path()).unwrap()));
    e.bootstrap().unwrap();
    let defaults = e.configuration_defaults(None).unwrap();
    let original = defaults["role_agent_bindings"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .as_str()
        .unwrap();
    let state = e.store.snapshot().unwrap();
    let old = state.resolve_agent(original).unwrap();
    let mut agent = source("agent", "chain-agent", Scope::global(), json!({}));
    agent
        .refs
        .insert("role_ref".into(), json!(old.role_ref.unwrap()));
    agent
        .refs
        .insert("capability_ref".into(), json!("chain-capability:1"));
    let old_cap = state
        .configuration(old.capability_ref.as_deref().unwrap())
        .unwrap();
    let mut capability = source("capability", "chain-capability", Scope::global(), json!({}));
    capability.refs = old_cap.refs.clone();
    capability
        .refs
        .insert("skills".into(), json!(["chain-skill:1"]));
    let skill = source(
        "skill",
        "chain-skill",
        Scope::global(),
        json!({"body":"An independently evolved method","rules":[]}),
    );
    apply(
        &e,
        &ConfigurationChangeSet {
            operations: vec![
                ConfigurationOperation::Create { source: agent },
                ConfigurationOperation::Create { source: capability },
                ConfigurationOperation::Create { source: skill },
            ],
        },
        "chain",
    )
    .unwrap();
    let next = e.store.snapshot().unwrap();
    assert_eq!(
        next.resolve_agent("chain-agent:1").unwrap().skills,
        vec!["chain-skill:1"]
    );
    assert_eq!(
        next.resolve_agent(original).unwrap().capability_ref,
        old.capability_ref
    );
}
