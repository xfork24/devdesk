use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn engine(root: &Path) -> Engine {
    let adapters = simulation::adapters();
    let e = Engine::new(
        Arc::new(SqliteStore::open(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        adapters,
    );
    e.bootstrap().unwrap();
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
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
    let model=command(e,"model.create",None,json!({"name":"Model A","data":{"provider":"test-provider","model":"model-a","parameters":{},"capabilities":[]}})).unwrap();
    let harness=command(e,"harness.create",None,json!({"name":"Harness","data":{"provider":"simulation","runtime_version":"1","capabilities":[],"model_providers":["test-provider"]}})).unwrap();
    let tool=command(e,"tool.create",None,json!({"name":"原生工具","data":{"qualified_name":"simulation.native","implementation":"harness.native","input_schema":{},"output_schema":{},"capabilities":[]}})).unwrap();
    command(e,"agent.create",None,json!({"name":format!("{role} Agent"),"data":{"role":role,"instructions":"Execute the frozen scope","model_ref":model["id"],"harness_ref":harness["id"],"skills":[],"tools":[tool["id"]],"permissions":["permission-read-only-v1"]}})).unwrap()["id"].as_str().unwrap().into()
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
        json!({"name":"全局实例","agent_revision_id":agent,"scope":{"kind":"global"}}),
    )
    .unwrap();
    let local=command(&e,"agent.instance.create",None,json!({"name":"项目实例","agent_revision_id":agent,"scope":{"kind":"project","project_id":project}})).unwrap();
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
    let mut data = old.data.clone();
    data["instructions"] = json!("new instructions");
    let revised = command(&e, "agent.revise", Some(&agent), json!({"data":data})).unwrap();
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
fn workflow_publication_is_immutable_and_replay_does_not_publish_twice() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"项目开发流程","template_ref":"builtin-workflow-feature-v1"}),
    )
    .unwrap();
    let did = draft["id"].as_str().unwrap();
    let c = Command {
        command_id: id(),
        target: Some(did.into()),
        expected_revision: Some(1),
        payload: json!({}),
    };
    let published = e.execute("workflow.publish", c.clone()).unwrap();
    assert_eq!(published, e.execute("workflow.publish", c).unwrap());
    assert!(
        command(
            &e,
            "workflow.draft.update",
            Some(did),
            json!({"document":draft["data"]["document"]})
        )
        .is_err()
    );
    let db = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    assert!(
        db.execute(
            "UPDATE governance_records SET name='changed' WHERE id=?1",
            [published["id"].as_str().unwrap()]
        )
        .is_err()
    );
    drop(e);
    let reopened = engine(root.path());
    assert_eq!(
        reopened
            .store
            .snapshot()
            .unwrap()
            .configuration(published["id"].as_str().unwrap())
            .unwrap()
            .content_hash,
        published["content_hash"]
    );
}
#[test]
fn feature_freezes_defaults_and_cannot_rebind_after_project_updates() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
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
    let mut agents = serde_json::Map::new();
    for role in ["analyst", "architect", "developer", "tester", "reviewer"] {
        agents.insert(role.into(), json!(setup_agent(&e, role)));
    }
    let standard = e
        .store
        .snapshot()
        .unwrap()
        .definitions
        .iter()
        .find(|d| d.family == "standard")
        .unwrap()
        .id
        .clone();
    let binding = json!({"workflow_ref":"builtin-workflow-feature-v1","standard_ref":standard,"agent_refs":agents});
    let original = command(
        &e,
        "project.configuration.update",
        Some(&project),
        binding.clone(),
    )
    .unwrap();
    let feature = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":project,"title":"A","require_configuration":true}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = e
        .store
        .snapshot()
        .unwrap()
        .feature_configuration(&feature)
        .unwrap();
    let next = command(&e, "project.configuration.update", Some(&project), binding).unwrap();
    assert_ne!(original["id"], next["id"]);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .feature_configuration(&feature)
            .unwrap()
            .project_configuration_id,
        before.project_configuration_id
    );
    let raw = json!(
        e.store
            .snapshot()
            .unwrap()
            .feature(&feature)
            .unwrap()
            .binding
    );
    assert!(command(&e, "binding.update", Some(&feature), raw).is_err());
}
#[test]
fn publication_rejects_cycle_and_cross_project_scope() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration("builtin-workflow-feature-v1")
        .unwrap()
        .data
        .clone();
    doc["steps"][0]["dependencies"] = json!(["cleanup"]);
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"bad","document":doc}),
    )
    .unwrap();
    assert!(command(&e, "workflow.publish", draft["id"].as_str(), json!({})).is_err());
    let agent = setup_agent(&e, "analyst");
    assert!(command(&e,"agent.instance.create",None,json!({"name":"bad scope","agent_revision_id":agent,"scope":{"kind":"project","project_id":"missing"}})).is_err());
}

struct Immediate;
impl HarnessPort for Immediate {
    fn configure(&self, n: &str, s: &str) -> String {
        SimHarness.configure(n, s)
    }
    fn poll(&self, r: &Run, input: &Value, time: u64) -> Result<Option<Value>> {
        SimHarness.poll(r, input, time + 10_000)
    }
    fn reconcile(&self, r: &Run) -> Result<Option<Value>> {
        SimHarness.reconcile(r)
    }
}
fn configured(e: &Engine) -> (String, String) {
    let pid = command(
        e,
        "project.create",
        None,
        json!({"title":"Workflow project"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut agents = serde_json::Map::new();
    for role in ["analyst", "architect", "developer", "tester", "reviewer"] {
        agents.insert(role.into(), json!(setup_agent(e, role)));
    }
    command(e,"project.configuration.update",Some(&pid),json!({"workflow_ref":"builtin-workflow-feature-v1","standard_ref":e.store.snapshot().unwrap().definitions.iter().find(|d|d.family=="standard").unwrap().id,"agent_refs":agents})).unwrap();
    let fid = command(
        e,
        "feature.create",
        None,
        json!({"project_id":pid,"title":"Workflow Feature","require_configuration":true}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (pid, fid)
}
fn drive(e: &Engine, rounds: usize) {
    for _ in 0..rounds {
        e.evaluate_workflows().unwrap();
        for intent in e.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            if ["session.open", "conversation.send", "proposal.extract"].contains(&method) {
                continue;
            }
            let c: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let result = e.execute(method, c);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
        }
        e.tick(4).unwrap();
    }
}
fn runtime(e: &Engine, fid: &str) -> WorkflowRuntime {
    e.store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .rev()
        .find(|r| {
            r.kind == "workflow_instance"
                && r.definition_id == fid
                && r.data["status"] != "cancelled"
        })
        .unwrap()
        .decode()
        .unwrap()
}
fn revise(e: &Engine, fid: &str, kind: &str) -> String {
    let revision = e.store.snapshot().unwrap().feature(fid).unwrap().revision;
    e.execute("document.revise",Command{command_id:id(),target:Some(fid.into()),expected_revision:Some(revision),payload:json!({"kind":kind,"title":kind,"body":"Observable workflow acceptance behavior"})}).unwrap()["id"].as_str().unwrap().into()
}
#[test]
fn requirement_is_delivered_and_reviewed_before_solution_and_unrelated_inputs_preserve_review() {
    let root = tempfile::tempdir().unwrap();
    let mut e = engine(root.path());
    e.harness = Arc::new(Immediate);
    let (_, fid) = configured(&e);
    revise(&e, &fid, "requirement");
    drive(&e, 30);
    let s = e.store.snapshot().unwrap();
    let req = s
        .deliveries
        .iter()
        .find(|d| d.feature_id == fid && d.kind == "requirement")
        .unwrap_or_else(|| {
            panic!(
                "runtime={} attempts={} intents={}",
                json!(runtime(&e, &fid)),
                json!(s.attempts),
                json!(
                    s.governance
                        .iter()
                        .filter(|r| r.kind == "workflow_intent")
                        .collect::<Vec<_>>()
                )
            )
        });
    assert!(
        s.accepted(req),
        "requirement review must finish using only requirement input"
    );
    let reqid = req.id.clone();
    assert!(!s.attempts.iter().any(|a| a.phase == "solution_planning"));
    revise(&e, &fid, "design");
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .accepted(e.store.snapshot().unwrap().delivery(&reqid).unwrap())
    );
    assert!(command(&e, "attempt.start", Some(&fid), json!({"phase":"closure"})).is_err());
    revise(&e, &fid, "requirement");
    let s = e.store.snapshot().unwrap();
    assert!(!s.accepted(s.delivery(&reqid).unwrap()));
    assert_eq!(
        runtime(&e, &fid)
            .steps
            .iter()
            .find(|s| s.key == "requirement_execute")
            .unwrap()
            .round,
        2
    );
}
#[test]
fn old_workflow_round_confirmation_and_direct_merge_cannot_bypass_preconditions() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let (_, fid) = configured(&e);
    let r = runtime(&e, &fid);
    let step = r.steps.iter().find(|s| s.key == "merge_review").unwrap();
    assert!(
        e.execute(
            "workflow.step.confirm",
            Command {
                command_id: id(),
                target: Some(step.id.clone()),
                expected_revision: Some(step.revision),
                payload: json!({"input_hash":step.input_hash})
            }
        )
        .is_err()
    );
    assert!(command(&e, "merge.request", Some(&fid), json!({})).is_err());
    let instance = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
        .unwrap()
        .clone();
    command(&e, "workflow.pause", Some(&instance.id), json!({})).unwrap();
    assert!(e.pending_workflow_intents().unwrap().is_empty());
    assert!(e.store.snapshot().unwrap().workflow_paused(&fid));
    assert!(
        command(
            &e,
            "attempt.start",
            Some(&fid),
            json!({"phase":"requirement_definition"})
        )
        .is_err()
    );
}

#[test]
fn a_workflow_drives_tdd_task_join_and_stops_before_mainline_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let mut e = engine(root.path());
    e.harness = Arc::new(Immediate);
    let (pid, fid) = configured(&e);
    let req = revise(&e, &fid, "requirement");
    drive(&e, 24);
    let design = revise(&e, &fid, "design");
    drive(&e, 2);
    let sid = id();
    let tid = id();
    let scope = Scope::Project {
        project_id: pid.clone(),
    };
    let locked = e
        .store
        .snapshot()
        .unwrap()
        .feature_configuration(&fid)
        .unwrap();
    let agent = locked.agent_refs["architect"].clone();
    e.store
        .transact(None, &mut |s| {
            s.sessions.push(Session {
                id: sid.clone(),
                owner_id: fid.clone(),
                agent: agent.clone(),
                harness: "workflow-domain-fixture".into(),
                sandbox: "fixture".into(),
                snapshot_id: None,
                instance: None,
                origin: Origin::External,
                project_id: Some(pid.clone()),
                feature_id: Some(fid.clone()),
                task_id: None,
                scope: Some(SessionScope::Feature),
                lifecycle: Some(SessionLifecycle::Idle),
                runtime_binding: Some(RuntimeBinding {
                    agent_configuration: None,
                    permissions: None,
                    installation: RuntimeInstallation {
                        id: "workflow-fixture".into(),
                        key: RuntimeKey {
                            provider: HarnessProvider::Codex,
                            version: "fixture".into(),
                            os: "linux".into(),
                            arch: "x86_64".into(),
                        },
                        artifact: RuntimeArtifact::OciImage {
                            reference: format!("example.test/fixture@sha256:{}", "a".repeat(64)),
                        },
                        source: "synthetic workflow domain fixture".into(),
                        license: "test".into(),
                    },
                    adapter_version: "fixture/v1".into(),
                    model: "fixture-model".into(),
                    tool_policy: ToolPolicy::ReadOnly,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                    work_type: WorkType::Design,
                    configuration_digest: "a".repeat(64),
                    credential_ref: None,
                }),
                external_session: Some("fixture-session".into()),
                predecessor: None,
                revision: 1,
            });
            s.turns.push(ConversationTurn {
                id: tid.clone(),
                project_id: pid.clone(),
                session_id: sid.clone(),
                client_message_id: id(),
                input: "Agreed one-task test plan".into(),
                input_digest: "b".repeat(64),
                input_snapshot: None,
                code_view: None,
                source_revision: Some(s.feature(&fid)?.revision),
                status: TurnStatus::Completed,
                external_message: Some("fixture-message".into()),
                external_turn: Some("fixture-turn".into()),
                consumed: true,
                cancel_requested: false,
                started: Some(1),
                finished: Some(2),
                run_id: None,
                revision: 1,
            });
            let record = s
                .governance
                .iter_mut()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
                .unwrap();
            let step = record.data["steps"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|s| s["key"] == "task_discussion")
                .unwrap();
            step["outputs"]["session"] = json!(sid);
            step["outputs"]["turn"] = json!(tid);
            record.content_hash = configuration_hash(&record.data);
            record.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    let p=e.execute("proposal.create",Command{command_id:id(),target:Some(sid),expected_revision:None,payload:json!({"turn_id":tid,"target":fid,"content":{"kind":"task_plan","requirement":req,"design":design,"tasks":[{"key":"one","title":"One task","acceptance":["AC-1"],"dependencies":[]}]}})}).unwrap();
    drive(&e, 2);
    let proposal = p["id"].as_str().unwrap();
    e.execute(
        "proposal.adopt",
        Command {
            command_id: id(),
            target: Some(proposal.into()),
            expected_revision: Some(1),
            payload: json!({}),
        },
    )
    .unwrap();
    revise(&e, &fid, "test");
    drive(&e, 150);
    let state = e.store.snapshot().unwrap();
    let r = runtime(&e, &fid);
    let waiting = r.steps.iter().find(|s| s.key == "merge_review").unwrap();
    assert_eq!(
        waiting.status,
        "waiting_confirmation",
        "runtime {}",
        json!(r)
    );
    assert_eq!(r.task_members.len(), 1);
    assert_eq!(
        state.task(&r.task_members[0]).unwrap().status,
        Status::Completed
    );
    let merge = state.merge(waiting.outputs.get("merge").unwrap()).unwrap();
    assert_ne!(merge.status, Status::Completed);
    assert!(
        e.execute(
            "merge.apply",
            Command {
                command_id: id(),
                target: Some(merge.id.clone()),
                expected_revision: Some(merge.revision),
                payload: json!({})
            }
        )
        .is_err()
    );
    e.execute(
        "workflow.step.confirm",
        Command {
            command_id: id(),
            target: Some(waiting.id.clone()),
            expected_revision: Some(waiting.revision),
            payload: json!({"input_hash":waiting.input_hash}),
        },
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let intent = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|i| i.data["method"] == "merge.apply" && i.data["command"]["target"] == merge.id)
        .unwrap();
    let cmd: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    let result = e.execute("merge.apply", cmd);
    assert!(result.is_ok());
    e.observe_workflow_intent(&intent.id, &result).unwrap();
    // Inject a proved-not-applied preflight receipt; no Git side effect has run.
    e.store
        .transact(None, &mut |s| {
            let advance = s.advances.iter_mut().find(|a| a.id == merge.id).unwrap();
            advance.status = AdvanceStatus::NotApplied;
            advance.reason = "synthetic preflight transport failure".into();
            advance.revision += 1;
            let m = s.merges.iter_mut().find(|m| m.id == merge.id).unwrap();
            m.status = Status::Interrupted;
            m.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    e.evaluate_workflows().unwrap();
    let retry = runtime(&e, &fid)
        .steps
        .into_iter()
        .find(|s| s.key == "merge_apply")
        .unwrap();
    assert_eq!(retry.status, "blocked");
    e.execute(
        "workflow.step.retry",
        Command {
            command_id: id(),
            target: Some(retry.id),
            expected_revision: Some(retry.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    drive(&e, 30);
    assert_eq!(
        e.store.snapshot().unwrap().feature(&fid).unwrap().status,
        Status::Completed
    );
    assert_eq!(runtime(&e, &fid).status, "completed");
    let ended = runtime(&e, &fid);
    let step = &ended.steps[0];
    assert!(
        e.execute(
            "workflow.step.rework",
            Command {
                command_id: id(),
                target: Some(step.id.clone()),
                expected_revision: Some(step.revision),
                payload: json!({})
            }
        )
        .is_err(),
        "completed workflows cannot be revived"
    );
    let instance = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
        .unwrap();
    let actions = e
        .query("query.workflow_actions", &json!({"id":instance.id}), 4)
        .unwrap();
    assert!(
        actions["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["allowed"].as_array().unwrap().is_empty())
    );
    let _ = scope;
}

#[test]
fn experience_is_agent_id_scoped_across_read_only_project_stores_and_checks_integrity() {
    let root = tempfile::tempdir().unwrap();
    let global = Arc::new(GlobalStore::open(root.path()).unwrap());
    let shared = Engine::new(
        global.clone(),
        Arc::new(FileArtifacts::new(root.path().into())),
        simulation::adapters(),
    );
    shared.bootstrap().unwrap();
    let agent = setup_agent(&shared, "analyst");
    let instance = command(
        &shared,
        "agent.instance.create",
        None,
        json!({"name":"Global experience","agent_revision_id":agent,"scope":{"kind":"global"}}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = command(
        &shared,
        "agent.instance.create",
        None,
        json!({"name":"Separate instance","agent_revision_id":agent,"scope":{"kind":"global"}}),
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
            simulation::adapters(),
        );
        e.bootstrap().unwrap();
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
            local_instance=command(&e,"agent.instance.create",None,json!({"name":"Project experience","agent_revision_id":agent,"scope":{"kind":"project","project_id":pid}})).unwrap()["id"].as_str().unwrap().to_owned();
        }
        let snapshot = command(&e, "snapshot.save", Some(&pid), json!({})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let metadata = json!({"agent_instance_id":instance,"agent_revision_id":agent,"snapshot_id":snapshot,"session_id":format!("synthetic-index-{index}"),"project_id":pid,"project_name":title,"feature_name":format!("Feature {index}"),"captured_at":1000+index as u64,"work_time":1000+index as u64,"kind":"project_context"});
        let row = ConfigurationRecord {
            id: id(),
            kind: "experience".into(),
            definition_id: snapshot,
            scope: Scope::Project {
                project_id: pid.clone(),
            },
            name: title.to_string(),
            version: 1,
            supersedes: None,
            content_hash: configuration_hash(&metadata),
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
                    local.content_hash = configuration_hash(&local.data);
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
fn upgrade_locks_provable_legacy_feature_without_inventing_agent_id_or_model() {
    let root = tempfile::tempdir().unwrap();
    let fid;
    let binding;
    {
        let e = engine(root.path());
        let pid = command(&e, "project.create", None, json!({"title":"legacy"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        fid = command(
            &e,
            "feature.create",
            None,
            json!({"project_id":pid,"title":"old Feature"}),
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        binding = e
            .store
            .snapshot()
            .unwrap()
            .feature(&fid)
            .unwrap()
            .binding
            .clone();
        e.store
            .transact(None, &mut |s| {
                s.projects[0].execution_mode = "live".into();
                Ok(Value::Null)
            })
            .unwrap();
    }
    let e = engine(root.path());
    let state = e.store.snapshot().unwrap();
    let lock = state
        .governance
        .iter()
        .find(|r| r.kind == "legacy_feature_lock" && r.definition_id == fid)
        .unwrap();
    assert_eq!(lock.data["binding"], json!(binding));
    assert!(lock.data["agent_instance_id"].is_null());
    assert!(command(&e, "binding.update", Some(&fid), json!(binding)).is_err());
    assert!(
        e.validate_workflow_command(
            "session.open",
            &Command {
                command_id: id(),
                target: Some(fid),
                expected_revision: None,
                payload: json!({"agent":"builtin-agent-analyst-v1"})
            }
        )
        .is_err()
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
            s.heads
                .iter_mut()
                .find(|h| h.id == format!("main:{pid}"))
                .unwrap()
                .revision = old.clone();
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
