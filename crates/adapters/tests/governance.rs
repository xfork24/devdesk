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
    let adapters = runtime::adapters();
    let e = Engine::new(
        Arc::new(SqliteStore::open(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        adapters,
    );
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
    let harness=command(e,"harness.create",None,json!({"name":"Harness","model_providers":["builtin-model-provider-openai:1"],"data":{"provider":"codex","runtime_version":devdesk_adapters::runtime_preparation::default_version(HarnessProvider::Codex),"recipe_digest":devdesk_adapters::runtime_preparation::default_codex_recipe_digest(),"capabilities":[]}})).unwrap();
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
    let e = engine(root.path());
    let agent = setup_agent(&e, "analyst");
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"P","workspace":"logical"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let global = command(
        &e,
        "agent.instance.create",
        None,
        json!({"name":"全局实例","agent_revision_id":agent,"scope":"global"}),
    )
    .unwrap();
    let local = command(
        &e,
        "agent.instance.create",
        None,
        json!({"name":"项目实例","agent_revision_id":agent,"scope":format!("project:{project}")}),
    )
    .unwrap();
    assert_ne!(global["id"], local["id"]);
    assert!(
        e.store
            .transact(None, &mut |s| {
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
        let pid = global.reserve_project(&create).unwrap();
        let e = Engine::new(
            Arc::new(SqliteStore::open_project(root.path(), &pid).unwrap()),
            Arc::new(FileArtifacts::new(
                SqliteStore::project_path(root.path(), &pid).unwrap(),
            )),
            runtime::adapters(),
        );
        e.bootstrap().unwrap();
        runtime::bind_default_permissions(&e);
        let config = global.snapshot().unwrap();
        e.store
            .transact(None, &mut |s| {
                for r in &config.governance {
                    if !s.governance.iter().any(|old| old.id == r.id) {
                        s.governance.push(r.clone());
                    }
                }
                for d in &config.definitions {
                    if !s.definitions.iter().any(|old| old.id == d.id) {
                        s.definitions.push(d.clone());
                    }
                }
                Ok(Value::Null)
            })
            .unwrap();
        e.execute("project.create", create).unwrap();
        global
            .publish_project(&e.store.snapshot().unwrap().projects[0])
            .unwrap();
        if index == 0 {
            local_instance=command(&e,"agent.instance.create",None,json!({"name":"Project experience","agent_revision_id":agent,"scope":format!("project:{pid}")})).unwrap()["id"].as_str().unwrap().to_owned();
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
            .transact(None, &mut |s| {
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
            .transact(None, &mut |s| {
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
    let e = engine(root.path());
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
        .transact(None, &mut |s| {
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
    e.store
        .transact(None, &mut |s| {
            let intent = s
                .repository_operations
                .iter_mut()
                .find(|r| r.id == op)
                .unwrap();
            intent.status = RepositoryOperationStatus::Observed;
            intent.receipt = Some(RepositoryReceipt::MainlineRefreshed {
                previous: old.clone(),
                revision: "3".repeat(40),
            });
            Ok(Value::Null)
        })
        .unwrap();
    e.tick_repositories().unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().heads[0].revision,
        old,
        "mismatched receipt cannot change the mainline fact"
    );
    e.store
        .transact(None, &mut |s| {
            let intent = s
                .repository_operations
                .iter_mut()
                .find(|r| r.id == op)
                .unwrap();
            intent.status = RepositoryOperationStatus::Observed;
            intent.receipt = Some(RepositoryReceipt::MainlineRefreshed {
                previous: old.clone(),
                revision: new.clone(),
            });
            Ok(Value::Null)
        })
        .unwrap();
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
    let e = engine(root.path());
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
