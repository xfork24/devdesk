#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/workflows.rs"]
mod workflow_fixture;
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn engine(root: &Path) -> Engine {
    engine_with_store(root, Arc::new(SqliteStore::open(root).unwrap()))
}
fn project_engine(root: &Path) -> Engine {
    engine_with_store(root, Arc::new(SqliteStore::open_fixture(root).unwrap()))
}
fn engine_with_store(root: &Path, store: Arc<dyn EngineeringStore>) -> Engine {
    let adapters = runtime::adapters();
    let e = Engine::new(store, Arc::new(FileArtifacts::new(root.into())), adapters);
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, mut payload: Value) -> Result<Value> {
    if method.starts_with("workflow.") {
        for field in ["data", "document"] {
            if payload.get(field).is_some() {
                payload[field] = workflow_fixture::current(payload[field].clone());
            }
        }
    }
    if method == "project.configuration.update" && payload.get("agent_refs").is_some() {
        let mut bindings = e.configuration_defaults(target).unwrap()["role_agent_bindings"]
            .as_object()
            .unwrap()
            .clone();
        let state = e.store.snapshot().unwrap();
        for (role, agent) in payload["agent_refs"].as_object().unwrap() {
            let key = state
                .configuration(agent.as_str().unwrap())
                .ok()
                .and_then(|r| reference_value(&r.refs, "role_ref").as_str())
                .and_then(|id| state.configuration(id).ok())
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(|| format!("builtin-role-{role}"));
            bindings.insert(key, agent.clone());
        }
        payload.as_object_mut().unwrap().remove("agent_refs");
        payload["role_agent_bindings"] = serde_json::to_value(bindings).unwrap();
    }
    let s = e.store.snapshot()?;
    let expected_revision = target.and_then(|id| {
        s.governance
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|p| p.id == id).map(|p| p.revision))
            .or_else(|| {
                s.repository_operations
                    .iter()
                    .find(|op| op.id == id)
                    .map(|op| op.revision)
            })
    });
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision,
            payload,
        },
    )
}
fn setup_agent(e: &Engine, role: &str) -> String {
    let model=command(e,"model.create",None,json!({"name":"Model A","data":{"provider":"openai","model":"model-a","parameters":{},"capabilities":[]}})).unwrap();
    let harness_data = e
        .store
        .snapshot()
        .unwrap()
        .configuration(DEFAULT_HARNESS_ID)
        .unwrap()
        .data
        .clone();
    let harness = command(
        e,
        "harness.create",
        None,
        json!({"name":"Harness","model_providers":["builtin-model-provider-openai:1"],"data":harness_data}),
    )
    .unwrap();
    let tool=command(e,"tool.create",None,json!({"name":"原生工具","data":{"qualified_name":"fixture.native","implementation":"harness.native","input_schema":{},"output_schema":{},"capabilities":[]}})).unwrap();
    let state = e.store.snapshot().unwrap();
    let target = permission_target(
        &state,
        PermissionTargetKind::Harness,
        harness["id"].as_str().unwrap(),
    )
    .unwrap();
    command(e,"permission.bindings.apply",None,json!({"bindings":[{"target":target,"permission_revision_id":"builtin-permission-harness-full-access:1","expected_revision":0,"enforcement_contract":devdesk_adapters::permission_profiles::permission_contract(&state,&target).unwrap()}]})).unwrap();
    let cap=command(e,"capability.create",None,json!({"name":"Fixture capability","model_ref":model["id"],"harness_ref":harness["id"],"tools":[tool["id"]],"skills":[],"data":{}})).unwrap();
    command(e,"agent.create",None,json!({"name":format!("{role} Agent"),"role_ref":default_role_id(role),"capability_ref":cap["id"],"data":{}})).unwrap()["id"].as_str().unwrap().into()
}
#[test]
fn agent_instances_pin_composition_and_choose_independent_scope() {
    let root = tempfile::tempdir().unwrap();
    let directory = Arc::new(GlobalStore::open(root.path()).unwrap());
    let e = engine_with_store(root.path(), directory.clone());
    let agent = setup_agent(&e, "analyst");
    let create = Command {
        command_id: id(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"P","workspace":"logical"}),
    };
    let project = directory
        .reserve_project(&create, &InvocationContext::human(None))
        .unwrap();
    let project_root = SqliteStore::project_path(root.path(), &project).unwrap();
    let local_engine = engine_with_store(
        &project_root,
        Arc::new(SqliteStore::open_project(root.path(), &project).unwrap()),
    );
    local_engine
        .sync_shared_configuration(directory.as_ref())
        .unwrap();
    local_engine.execute("project.create", create).unwrap();
    let global = command(
        &e,
        "agent.instance.create",
        None,
        json!({"name":"全局实例","agent_revision_id":agent,"scope":"global"}),
    )
    .unwrap();
    let local = command(
        &local_engine,
        "agent.instance.create",
        None,
        json!({"name":"项目实例","agent_revision_id":agent,"scope":format!("project:{project}")}),
    )
    .unwrap();
    assert_ne!(global["id"], local["id"]);
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.governance
                    .iter_mut()
                    .find(|r| r.id == global["id"].as_str().unwrap())
                    .unwrap()
                    .data["agent_revision_id"] = json!("other");
                Ok(Value::Null)
            })
            .is_err()
    );
    let old = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&agent)
        .unwrap()
        .clone();
    let mut payload = json!({"data":old.data});
    for (key, value) in &old.refs {
        payload[key] = value.clone();
    }
    let revised = command(&e, "agent.revise", Some(&agent), payload).unwrap();
    assert_ne!(revised["id"], agent);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(global["id"].as_str().unwrap())
            .unwrap()
            .data["agent_revision_id"],
        agent
    );
}

#[test]
fn experience_is_agent_id_scoped_across_read_only_project_stores_and_checks_integrity() {
    let root = tempfile::tempdir().unwrap();
    let global = Arc::new(GlobalStore::open(root.path()).unwrap());
    let shared = Engine::new(
        global.clone(),
        Arc::new(FileArtifacts::new(root.path().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(&root).unwrap(),
    ));
    shared.bootstrap().unwrap();
    let agent = setup_agent(&shared, "analyst");
    let instance = command(
        &shared,
        "agent.instance.create",
        None,
        json!({"name":"Global experience","agent_revision_id":agent,"scope":"global"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = command(
        &shared,
        "agent.instance.create",
        None,
        json!({"name":"Separate instance","agent_revision_id":agent,"scope":"global"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut projects = vec![];
    let mut local_instance = String::new();
    let mut artifact = None;
    for (index, title) in ["Project Alpha", "Project Beta"].iter().enumerate() {
        let create = Command {
            command_id: id(),
            target: None,
            expected_revision: None,
            payload: json!({"title":title}),
        };
        let pid = global
            .reserve_project(&create, &devdesk_core::InvocationContext::human(None))
            .unwrap();
        let e = Engine::new(
            Arc::new(SqliteStore::open_project(root.path(), &pid).unwrap()),
            Arc::new(FileArtifacts::new(
                SqliteStore::project_path(root.path(), &pid).unwrap(),
            )),
            runtime::adapters(),
        );
        e.bootstrap().unwrap();
        runtime::bind_default_permissions(&e);
        e.sync_shared_configuration(global.as_ref()).unwrap();
        e.execute("project.create", create).unwrap();
        global
            .publish_project(&e.store.snapshot().unwrap().projects[0])
            .unwrap();
        if index == 0 {
            let state = e.store.snapshot().unwrap();
            let original = state.configuration(&agent).unwrap();
            let private=command(&e,"agent.create",None,json!({"name":"Project private Agent","scope":format!("project:{pid}"),"role_ref":original.refs["role_ref"],"capability_ref":original.refs["capability_ref"],"data":{}})).unwrap();
            local_instance=command(&e,"agent.instance.create",None,json!({"name":"Project experience","agent_revision_id":private["id"],"scope":format!("project:{pid}")})).unwrap()["id"].as_str().unwrap().to_owned();
        }
        let snapshot = command(&e, "snapshot.save", Some(&pid), json!({})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let metadata = json!({"agent_instance_id":instance,"agent_revision_id":agent,"snapshot_id":snapshot,"session_id":format!("synthetic-index-{index}"),"project_id":pid,"project_name":title,"feature_name":format!("Feature {index}"),"captured_at":1000+index as u64,"work_time":1000+index as u64,"kind":"project_context"});
        let row = ConfigurationRecord {
            id: id(),
            r#type: "experience".into(),
            definition_id: snapshot,
            scope: Scope::project(pid.clone()),
            name: title.to_string(),
            version: 1,
            supersedes: None,
            content_hash: content_digest(&References::new(), &metadata),
            refs: References::new(),
            data: metadata,
            immutable: true,
            revision: 1,
            created_at: 1000 + index as u64,
        };
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.governance.push(row.clone());
                if index == 0 {
                    let mut local = row.clone();
                    local.id = id();
                    local.data["agent_instance_id"] = json!(local_instance);
                    local.content_hash = content_digest(&local.refs, &local.data);
                    s.governance.push(local);
                }
                Ok(Value::Null)
            })
            .unwrap();
        global
            .seed_fixture_state(None, &mut |s| {
                s.governance.push(row.clone());
                Ok(Value::Null)
            })
            .unwrap();
        if index == 0 {
            artifact = Some((
                pid.clone(),
                e.store
                    .snapshot()
                    .unwrap()
                    .artifacts
                    .last()
                    .unwrap()
                    .clone(),
            ));
        }
        projects.push(pid);
    }
    let lookup = |pid: &str, instance: &str, extra: Value| {
        let mut p = extra;
        p["agent_instance_id"] = json!(instance);
        experience::query(root.path(), pid, &p)
    };
    assert_eq!(
        lookup(&projects[1], &instance, json!({})).unwrap()["total"],
        2
    );
    assert_eq!(
        lookup(&projects[0], &local_instance, json!({})).unwrap()["total"],
        1
    );
    assert!(lookup(&projects[1], &local_instance, json!({})).is_err());
    assert_eq!(lookup(&projects[0], &other, json!({})).unwrap()["total"], 0);
    let filtered = lookup(
        &projects[0],
        &instance,
        json!({"project_name":"Alpha","feature_name":"Feature 0","from":1000,"to":1000}),
    )
    .unwrap();
    assert_eq!(filtered["total"], 1);
    let sid = filtered["records"][0]["snapshot_id"].as_str().unwrap();
    let reader = experience::Reader::new(root.path().into());
    let context = devdesk_core::ports::AgentContextReader::experience(
        &reader,
        &instance,
        None,
        &json!({"project_id":projects[0],"limit":1}),
    )
    .unwrap();
    assert_eq!(context["total"], 1);
    assert_eq!(context["records"][0]["snapshot_id"], sid);
    let mut offset = 0;
    let mut text = String::new();
    let mut digest = None;
    loop {
        let part=devdesk_core::ports::AgentContextReader::experience(&reader,&instance,None,&json!({"source_project_id":projects[0],"snapshot_id":sid,"offset":offset,"max_bytes":1024})).unwrap();
        if let Some(digest) = &digest {
            assert_eq!(&part["content_digest"], digest);
        } else {
            digest = Some(part["content_digest"].clone());
        }
        text.push_str(part["content_chunk"].as_str().unwrap());
        match part["next_offset"].as_u64() {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    let from_agent: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        from_agent,
        lookup(
            &projects[0],
            &instance,
            json!({"snapshot_id":sid,"source_project_id":projects[0]})
        )
        .unwrap()
    );
    assert!(
        devdesk_core::ports::AgentContextReader::experience(
            &reader,
            &other,
            None,
            &json!({"source_project_id":projects[0],"snapshot_id":sid})
        )
        .is_err()
    );
    let catalog = experience::discussion_catalog(root.path()).unwrap();
    let detached_path = SqliteStore::global_discussion_path(root.path()).unwrap();
    let detached = Engine::new(
        Arc::new(SqliteStore::open(&detached_path).unwrap()),
        Arc::new(FileArtifacts::new(detached_path)),
        runtime::adapters(),
    );
    detached.bootstrap().unwrap();
    runtime::bind_default_permissions(&detached);
    detached
        .import_discussion_agent(&catalog, &local_instance)
        .unwrap();
    let mut open = Command {
        command_id: id(),
        target: Some("global".into()),
        expected_revision: Some(0),
        payload: json!({"agent_instance_id":local_instance}),
    };
    detached
        .resolve_session_composition(
            &runtime::ResourceFixture {
                store: detached.store.as_ref(),
            },
            &mut open,
        )
        .unwrap();
    assert_eq!(open.payload["agent_instance_id"], local_instance);
    assert_eq!(
        open.payload["agent_configuration"]["context_protocol"],
        AGENT_CONTEXT_PROTOCOL
    );
    assert!(detached.store.snapshot().unwrap().projects.is_empty());
    assert_eq!(
        lookup(
            &projects[0],
            &instance,
            json!({"snapshot_id":sid,"source_project_id":projects[0]})
        )
        .unwrap()["integrity"],
        "verified"
    );
    let (pid, a) = artifact.unwrap();
    let evidence = devdesk_core::ports::AgentContextReader::experience(
        &reader,
        &instance,
        None,
        &json!({"source_project_id":pid,"snapshot_id":sid,"artifact_id":a.id,"max_bytes":65536}),
    )
    .unwrap();
    let original: Value =
        serde_json::from_str(evidence["content_chunk"].as_str().unwrap()).unwrap();
    assert_eq!(original["artifact"]["sha256"], a.sha256);
    assert!(
        devdesk_core::ports::AgentContextReader::experience(
            &reader,
            &instance,
            None,
            &json!({"source_project_id":pid,"snapshot_id":sid,"artifact_id":"unrelated-evidence"})
        )
        .is_err()
    );
    // A current Feature index can advance while old sealed references stay readable.
    let source_engine = Engine::new(
        Arc::new(SqliteStore::open_project(root.path(), &projects[0]).unwrap()),
        Arc::new(FileArtifacts::new(
            SqliteStore::project_path(root.path(), &projects[0]).unwrap(),
        )),
        runtime::adapters(),
    );
    let mut current = source_engine
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|row| row.r#type == "experience" && row.data["agent_instance_id"] == instance)
        .unwrap();
    current.id = "feature-experience-index".into();
    current.definition_id = "feature-experience".into();
    current.immutable = false;
    current.data["feature_id"] = json!("feature-experience");
    current.content_hash = content_digest(&current.refs, &current.data);
    source_engine
        .store
        .seed_fixture_state(None, &mut |state| {
            state.governance.push(current.clone());
            Ok(Value::Null)
        })
        .unwrap();
    source_engine.publish_experience_index(&*global).unwrap();
    let next_snapshot = command(
        &source_engine,
        "snapshot.save",
        Some(&projects[0]),
        json!({}),
    )
    .unwrap()["id"]
        .clone();
    source_engine
        .store
        .seed_fixture_state(None, &mut |state| {
            let row = state
                .governance
                .iter_mut()
                .find(|row| row.id == current.id)
                .unwrap();
            let previous = row.data.clone();
            row.data["history"] = json!([previous]);
            row.data["snapshot_id"] = next_snapshot.clone();
            row.data["captured_at"] = json!(2000);
            row.content_hash = content_digest(&row.refs, &row.data);
            row.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    // Publication is a bounded, eventually consistent cursor pass. A slow
    // first row can consume the 5 ms budget before this target is visited.
    let key = devdesk_core::experience_directory::resource_key(
        &source_engine.store.namespace(),
        &current.id,
    );
    let sources = source_engine
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .filter(|r| r.r#type == "experience")
        .count();
    for _ in 0..=sources {
        source_engine.publish_experience_index(&*global).unwrap();
        if global.read_resources(std::slice::from_ref(&key)).unwrap()[0]
            .value
            .as_ref()
            .is_some_and(|row| row["data"]["source_revision"] == 2)
        {
            break;
        }
    }
    assert_eq!(
        global
            .snapshot()
            .unwrap()
            .configuration(
                &devdesk_core::experience_directory::resource_key(
                    &source_engine.store.namespace(),
                    &current.id
                )
                .id
            )
            .unwrap()
            .data["source_revision"],
        2
    );
    let grouped = lookup(
        &projects[0],
        &instance,
        json!({"feature_id":"feature-experience"}),
    )
    .unwrap();
    assert_eq!(grouped["total"], 1);
    assert_eq!(grouped["records"][0]["snapshot_id"], next_snapshot);
    assert_eq!(grouped["records"][0]["snapshot_count"], 2);
    assert!(grouped["records"][0].get("history").is_none());
    assert_eq!(
        lookup(
            &projects[0],
            &instance,
            json!({"feature_id":"feature-experience","snapshot_id":sid})
        )
        .unwrap()["integrity"],
        "verified"
    );

    let delete = Command {
        command_id: id(),
        target: Some(next_snapshot.as_str().unwrap().to_owned()),
        expected_revision: Some(0),
        payload: json!({"agent_instance_id":instance,"source_project_id":pid,"snapshot_id":next_snapshot}),
    };
    let remove = || {
        devdesk_core::experience_directory::delete(&*global, &pid, &delete, || {
            experience::entry_visible(root.path(), &pid, &delete.payload)
        })
    };
    assert_eq!(remove().unwrap().result, remove().unwrap().result);
    assert_eq!(
        lookup(&pid, &instance, json!({"feature_id":"feature-experience"})).unwrap()["total"],
        0
    );
    assert!(
        lookup(
            &pid,
            &instance,
            json!({"snapshot_id":next_snapshot,"source_project_id":pid})
        )
        .is_err()
    );
    assert!(
        lookup(
            &pid,
            &instance,
            json!({"snapshot_id":sid,"source_project_id":pid})
        )
        .is_ok()
    );
    assert!(
        source_engine
            .store
            .snapshot()
            .unwrap()
            .snapshots
            .iter()
            .any(|row| row.id == next_snapshot)
    );
    source_engine.publish_experience_index(&*global).unwrap();
    assert_eq!(
        lookup(&pid, &instance, json!({"feature_id":"feature-experience"})).unwrap()["total"],
        0
    );

    std::fs::write(
        SqliteStore::project_path(root.path(), &pid)
            .unwrap()
            .join(a.relative_path),
        "tampered",
    )
    .unwrap();
    assert!(
        lookup(
            &projects[0],
            &instance,
            json!({"snapshot_id":sid,"source_project_id":projects[0]})
        )
        .is_err()
    );
    let db = SqliteStore::project_path(root.path(), &projects[1])
        .unwrap()
        .join("engineering.sqlite3");
    std::fs::rename(&db, db.with_extension("offline")).unwrap();
    let offline = lookup(&projects[0], &instance, json!({})).unwrap();
    assert_eq!(offline["total"], 2);
    assert!(
        offline["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["project_id"] == projects[1] && r["availability"] == "unavailable")
    );
}

#[test]
fn mainline_refresh_is_an_explicit_frozen_intent_and_accepts_only_matching_receipts() {
    let root = tempfile::tempdir().unwrap();
    struct RefreshPort(std::sync::atomic::AtomicBool);
    impl ports::Git for RefreshPort {
        fn repository_operation(&self, op: &RepositoryOperation) -> Result<RepositoryReceipt> {
            let RepositoryAction::RefreshMainline { expected, .. } = &op.action else {
                panic!("unexpected operation")
            };
            Ok(RepositoryReceipt::MainlineRefreshed {
                previous: expected.clone(),
                revision: "3".repeat(40),
            })
        }
        fn inspect_repository_operation(
            &self,
            op: &RepositoryOperation,
        ) -> Result<Option<RepositoryReceipt>> {
            assert!(self.0.load(std::sync::atomic::Ordering::Relaxed));
            let RepositoryAction::RefreshMainline {
                expected, revision, ..
            } = &op.action
            else {
                panic!("unexpected operation")
            };
            Ok(Some(RepositoryReceipt::MainlineRefreshed {
                previous: expected.clone(),
                revision: revision.clone(),
            }))
        }
        fn candidate(&self, _: &GitContext, _: &str, _: &str, _: &str) -> Result<String> {
            unreachable!()
        }
        fn advance(&self, _: &GitContext, _: &str, _: &str, _: &str, _: &str) -> Result<String> {
            unreachable!()
        }
    }
    let port = Arc::new(RefreshPort(false.into()));
    let mut e = project_engine(root.path());
    e.git = port.clone();
    let pid = command(
        &e,
        "project.create",
        None,
        json!({"title":"Refresh fixture"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let old = "1".repeat(40);
    let new = "2".repeat(40);
    let create = id();
    e.store
        .seed_fixture_state(None, &mut |s| {
            let project = s.projects.iter_mut().find(|p| p.id == pid).unwrap();
            project.execution_mode = "live".into();
            project.workspace_definition = Some(Workspace {
                repository: "fixture/test".into(),
                path: "owned/repository".into(),
                mainline: "main".into(),
            });
            s.heads.push(GitHead {
                id: format!("main:{pid}"),
                project_id: pid.clone(),
                revision: old.clone(),
            });
            let worktree = RepositoryWorktree {
                id: pid.clone(),
                project_id: pid.clone(),
                owner_id: pid.clone(),
                kind: WorktreeKind::Mainline,
                branch: "main".into(),
                path: "owned/mainline".into(),
                base: old.clone(),
                operation_id: create.clone(),
            };
            s.repository_operations.push(RepositoryOperation {
                id: create.clone(),
                project_id: pid.clone(),
                action: RepositoryAction::CreateWorktree {
                    owner: pid.clone(),
                    worktree_kind: WorktreeKind::Mainline,
                    base: old.clone(),
                },
                status: RepositoryOperationStatus::Completed,
                receipt: Some(RepositoryReceipt::Worktree {
                    worktree: worktree.clone(),
                }),
                reason: String::new(),
                revision: 1,
            });
            s.repository_worktrees.push(worktree);
            Ok(Value::Null)
        })
        .unwrap();
    assert!(
        command(
            &e,
            "repository.refresh",
            Some(&pid),
            json!({"revision":"not-a-sha"})
        )
        .is_err()
    );
    let op = command(
        &e,
        "repository.refresh",
        Some(&pid),
        json!({"revision":new}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        e.store.snapshot().unwrap().heads[0].revision,
        old,
        "command only persists an intent"
    );
    assert!(
        command(
            &e,
            "repository.refresh",
            Some(&pid),
            json!({"revision":new})
        )
        .is_err(),
        "in-flight operation must settle first"
    );
    e.tick_repositories().unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().heads[0].revision,
        old,
        "mismatched receipt cannot change the mainline fact"
    );
    let rejected = e.store.snapshot().unwrap();
    assert!(rejected.repository_operations.iter().any(|r| r.id == op
        && r.status == RepositoryOperationStatus::NeedsReconciliation
        && r.reason.contains("receipt_rejected")));
    assert!(
        rejected
            .governance
            .iter()
            .any(|r| r.r#type == "repository_effect_observation"
                && r.data["result"]["Ok"]["revision"] == "3".repeat(40))
    );
    port.0.store(true, std::sync::atomic::Ordering::Relaxed);
    command(&e, "repository.reconcile", Some(&op), json!({})).unwrap();
    e.tick_repositories().unwrap();
    let after = e.store.snapshot().unwrap();
    assert_eq!(after.heads[0].revision, new);
    assert_eq!(
        after
            .repository_operations
            .iter()
            .find(|r| r.id == op)
            .unwrap()
            .status,
        RepositoryOperationStatus::Completed
    );
    e.tick_repositories().unwrap();
    assert_eq!(e.store.snapshot().unwrap().heads[0].revision, new);
}

#[test]
fn reference_catalog_is_idempotent_and_does_not_configure_new_projects() {
    let root = tempfile::tempdir().unwrap();
    let e = project_engine(root.path());
    let before = e.store.snapshot().unwrap();
    assert!(
        !before
            .governance
            .iter()
            .any(|r| r.r#type == "defaults_bundle")
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    assert_eq!(
        before.governance.len(),
        e.store.snapshot().unwrap().governance.len()
    );
    assert!(
        command(
            &e,
            "project.create",
            None,
            json!({"title":"Obsolete defaults","use_builtin_defaults":true})
        )
        .is_err()
    );
    let p = command(&e, "project.create", None, json!({"title":"Pending"})).unwrap();
    assert!(
        command(
            &e,
            "feature.create",
            None,
            json!({"project_id":p["id"],"title":"Blocked"})
        )
        .is_err()
    );
    assert!(
        !e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "project_configuration")
    );
}

#[test]
fn custom_role_create_edit_delete_preserves_history_without_deriving_agents() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let before = e.store.snapshot().unwrap();
    let role=command(&e,"role.create",None,json!({"name":"安全评审员","data":{"instructions":"# Duty\nReview security evidence.","visibility":{"kind":"global"}}})).unwrap();
    let revised=command(&e,"role.revise",role["id"].as_str(),json!({"name":"高级安全评审员","data":{"instructions":"Review failure handling.","visibility":{"kind":"global"}}})).unwrap();
    command(&e, "role.delete", revised["id"].as_str(), json!({})).unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(before.definitions.len(), state.definitions.len());
    assert!(state.configuration(role["id"].as_str().unwrap()).is_ok());
    assert!(
        !e.query("query.roles", &json!({}), 4).unwrap()["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["definition_id"] == role["id"])
    );
}

fn sealing_interleaving(mode: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let e = Arc::new(project_engine(root.path()));
    let project = command(&e, "project.create", None, json!({"title":"Sealing race"})).unwrap();
    let cmd = Command {
        command_id: "original-seal".into(),
        target: Some(project["id"].as_str().unwrap().into()),
        expected_revision: Some(1),
        payload: json!({"kind":"prd","title":"Exact material","body":"Original body"}),
    };
    let fired = Arc::new(AtomicBool::new(false));
    let entered = fired.clone();
    let weak = Arc::downgrade(&e);
    let duplicate = cmd.clone();
    let mode = mode.to_owned();
    let action = mode.clone();
    let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if entered.load(Ordering::Acquire) {
            return;
        }
        let e = weak.upgrade().unwrap();
        let state = e.store.snapshot().unwrap();
        let Some(intent) = state
            .governance
            .iter()
            .find(|r| r.r#type == "sealed_command_intent" && r.data["status"] == "prepared")
        else {
            return;
        };
        if entered.swap(true, Ordering::AcqRel) {
            return;
        }
        if action == "interrupted" {
            panic!("interrupt after durable intent, before file I/O");
        }
        // A real concurrent Conversations recovery scan sees the prepared
        // intent before its original file observations have been journaled.
        let other = e.clone();
        std::thread::spawn(move || other.tick_conversations(0).unwrap())
            .join()
            .unwrap();
        assert_eq!(
            e.store
                .snapshot()
                .unwrap()
                .configuration(&intent.id)
                .unwrap()
                .data["status"],
            "prepared"
        );
        assert_eq!(
            e.execute("document.revise", duplicate.clone())
                .unwrap_err()
                .code,
            "resource_pending"
        );
        if action == "owner_changed" {
            command(
                &e,
                "project.update",
                duplicate.target.as_deref(),
                json!({"title":"Changed after grant"}),
            )
            .unwrap();
        }
    });
    e.store.subscribe_changes(Arc::downgrade(&listener));
    if mode == "interrupted" {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || e.execute("document.revise", cmd)
            ))
            .is_err()
        );
        drop(listener);
        drop(e);
        let restarted = project_engine(root.path());
        assert!(
            restarted
                .store
                .scheduling_hints()
                .unwrap()
                .stages
                .contains(&SchedulingStage::Conversations),
            "an interrupted seal with no Turn must still be discoverable"
        );
        restarted.tick_conversations(0).unwrap();
        assert!(
            !restarted
                .store
                .scheduling_hints()
                .unwrap()
                .stages
                .contains(&SchedulingStage::Conversations)
        );
        let state = restarted.store.snapshot().unwrap();
        let intent = state
            .governance
            .iter()
            .find(|r| r.r#type == "sealed_command_intent")
            .unwrap();
        assert_eq!(intent.data["status"], "needs_reconciliation");
        assert!(state.documents.is_empty());
        assert!(state.artifacts.is_empty());
    } else {
        let result = e.execute("document.revise", cmd.clone());
        assert!(fired.load(Ordering::Acquire));
        let state = e.store.snapshot().unwrap();
        let intent = state
            .governance
            .iter()
            .find(|r| r.r#type == "sealed_command_intent")
            .unwrap();
        if mode == "owner_changed" {
            assert_eq!(result.unwrap_err().code, "revision_conflict");
            assert_eq!(intent.data["status"], "conflicted");
            assert!(state.documents.is_empty());
        } else {
            assert_eq!(e.execute("document.revise", cmd).unwrap(), result.unwrap());
            assert_eq!(intent.data["status"], "completed");
            assert_eq!(state.documents.len(), 1);
            assert_eq!(state.artifacts.len(), 1);
        }
    }
}
#[test]
fn a_live_material_seal_is_not_recovered_as_an_interrupted_attempt() {
    sealing_interleaving("live");
}
#[test]
fn live_material_sealing_retains_its_original_owner_witness() {
    sealing_interleaving("owner_changed");
}
#[test]
fn interrupted_material_sealing_remains_unknown_after_restart() {
    sealing_interleaving("interrupted");
}
