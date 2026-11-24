use super::*;

fn add_history(engine: &Engine, name: &str) {
    let scope = engine
        .store
        .project_id()
        .map_or(Scope::global(), Scope::project);
    engine.store.seed_fixture_state(None, &mut |s| {
        let time = 9_000_000_000_000;
        let data = json!({"status":"succeeded","invocation":{"tool":{"definition":{}},"arguments":{}}});
        s.governance.push(ConfigurationRecord {
            id: format!("{name}-call"), r#type: "tool_call".into(), definition_id: format!("{name}-call"),
            scope: scope.clone(), name: name.into(), version: 1, supersedes: None, content_hash: content_digest(&References::new(), &data),
            refs: References::new(), data, immutable: false, revision: 1, created_at: time,
        });
        s.events.push(Event {
            id: s.events.last().map_or(1, |event| event.id + 1), target: name.into(),
            kind: format!("{name}-event"), detail: name.into(), time: time + 1,
        });
        Ok(Value::Null)
    }).unwrap();
}

#[test]
fn combined_history_keeps_global_and_each_project_in_their_own_database() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
    add_history(&w.configuration, "global");
    let project = w
        .dispatch(
            "project.create",
            json!({"command_id":id(),"payload":{"title":"First"}}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let engine = w.project_engine(Some(&project)).unwrap();
    add_history(&engine, "project-one");
    let global = w
        .dispatch(
            "query.history",
            json!({"scope":"global","include_tool_calls":true}),
        )
        .unwrap();
    assert_eq!(global["records"][0]["event"]["kind"], "global-event");
    assert_eq!(global["records"][1]["call"]["name"], "global");
    assert!(!global.to_string().contains("project-one"));
    let local = w
        .dispatch(
            "query.history",
            json!({"scope":"project","project_id":project,"include_tool_calls":true}),
        )
        .unwrap();
    assert_eq!(local["records"][0]["event"]["kind"], "project-one-event");
    assert_eq!(local["records"][1]["call"]["name"], "project-one");
    assert!(
        !local["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["call"]["name"] == "global")
    );
    let second = w
        .dispatch(
            "project.create",
            json!({"command_id":id(),"payload":{"title":"Second"}}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    add_history(&w.project_engine(Some(&second)).unwrap(), "project-two");
    let other = w
        .dispatch(
            "query.history",
            json!({"scope":"project","project_id":second,"include_tool_calls":true}),
        )
        .unwrap();
    assert_eq!(other["records"][0]["event"]["kind"], "project-two-event");
    assert_eq!(other["records"][1]["call"]["name"], "project-two");
    assert!(!other.to_string().contains("project-one"));
    assert_eq!(
        w.dispatch(
            "query.history",
            json!({"scope":"project","project_id":project,"include_tool_calls":true})
        )
        .unwrap(),
        local
    );
    let current = w
        .dispatch(
            "query.history",
            json!({"scope":"global","include_tool_calls":true}),
        )
        .unwrap();
    // Each project reservation now has an atomic global directory receipt and
    // event. Project business history still stays in its own database.
    assert_eq!(
        current["total"].as_u64().unwrap(),
        global["total"].as_u64().unwrap() + 1
    );
    let business = |value: &Value| {
        value["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["event"]["kind"] != "resource.changed")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(business(&current), business(&global));
    assert!(!current.to_string().contains("project-one"));
    assert!(!current.to_string().contains("project-two"));
}

#[test]
fn operational_pages_do_not_restore_aggregate_history_and_preserve_exact_calls() {
    let dir = tempfile::tempdir().unwrap();
    let workspace =
        Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
    let project = workspace
        .dispatch(
            "project.create",
            json!({"command_id":id(),"payload":{"title":"Bounded operational pages"}}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let engine = workspace.project_engine(Some(&project)).unwrap();
    add_history(&engine, "bounded");
    let artifact = "sample artifact";
    let bytes = serde_json::to_vec(artifact).unwrap();
    // NativeTools addresses this directory by the first 16 SHA-256 bytes of "bounded-call".
    let artifact_dir = PathBuf::from(engine.store.root())
        .join("tool-executions/3dab190a-51da-7996-7fb1-cc56b4260635");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(artifact_dir.join("result.bin"), &bytes).unwrap();
    engine.store.seed_fixture_state(None, &mut |state| {
        let call = state.governance.iter_mut().find(|r| r.id == "bounded-call").unwrap();
        call.data["receipt"] = json!({"invocation_id":call.id,"success":true,"remote_effect":"none",
            "origin":Origin::External,"content":[{"type":"resource_link","uri":"devdesk-tool-artifact:bounded-call",
                "size":bytes.len(),"sha256":configuration_hash(&artifact),"mimeType":"application/json"}]});
        call.content_hash = content_digest(&call.refs, &call.data);
        call.revision += 1;
        Ok(Value::Null)
    }).unwrap();
    // Unrelated opaque history must never trigger aggregate restoration.
    let mut broken = engine
        .store
        .snapshot()
        .unwrap()
        .configuration("bounded-call")
        .unwrap()
        .clone();
    broken.id = "unrelated-broken-history".into();
    broken.definition_id = broken.id.clone();
    broken.r#type = "unrelated_runtime".into();
    broken.revision = 1;
    broken.data = json!({"value":{"$source":"tool_definition","ref":"unavailable:1"}});
    broken.content_hash = content_digest(&broken.refs, &broken.data);
    let key = devdesk_core::concurrency::ResourceKey::new("governance", &broken.id);
    engine
        .store
        .commit_changes(&devdesk_core::concurrency::PreparedChangeSet {
            command_id: id(),
            fingerprint: configuration_hash(&broken),
            read_set: devdesk_core::concurrency::ReadSet::new(vec![
                devdesk_core::concurrency::ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                },
            ])
            .unwrap(),
            changes: vec![devdesk_core::concurrency::ResourceChange {
                resource: key,
                value: broken.to_storage(),
            }],
            result: Value::Null,
            time: 1,
        })
        .unwrap();
    let trace = devdesk_adapters::performance::Trace::start();
    let cursor = engine.store.event_cursor().unwrap();
    for (method, params) in [
        ("query.history", json!({"project_id":project,"limit":1})),
        (
            "query.history",
            json!({"project_id":project,"limit":1,"include_tool_calls":true,"view":"summary"}),
        ),
        (
            "query.tool_calls",
            json!({"project_id":project,"limit":1,"view":"summary"}),
        ),
        ("query.repository", json!({"project_id":project,"limit":1})),
        ("query.runtimes", json!({"project_id":project})),
    ] {
        let result = workspace.dispatch(method, params).unwrap();
        assert!(result.to_string().len() < 128 * 1024, "{method}");
    }
    let detail = workspace
        .dispatch(
            "query.tool_calls",
            json!({"project_id":project,"id":"bounded-call","include_history":true,"limit":1}),
        )
        .unwrap();
    assert_eq!(detail["total"], 1);
    assert_eq!(detail["records"][0]["name"], "bounded");
    assert!(detail["records"][0]["data"]["invocation"]["arguments"].is_object());
    let artifact = workspace
        .dispatch(
            "tool.call.artifact",
            json!({"project_id":project,"target":"bounded-call"}),
        )
        .unwrap();
    assert_eq!(
        artifact,
        json!({"base64":"InNhbXBsZSBhcnRpZmFjdCI=","mime":"application/json"})
    );
    assert_eq!(
        engine.store.event_cursor().unwrap(),
        cursor,
        "ordinary reads cannot publish business events"
    );
    let measurement = trace.finish();
    assert!(
        measurement["spans"]["project.full_snapshot"].is_null(),
        "{measurement}"
    );
    assert!(
        measurement["spans"]["global.full_snapshot"].is_null(),
        "{measurement}"
    );
}

#[test]
fn immutable_catalog_copy_interleaves_with_a_local_source_publication() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for incoming in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Concurrent source copy"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let e = w.project_engine(Some(&project)).unwrap();
        e.execute("role.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Local role","scope":format!("project:{project}"),"data":{"instructions":"A local role copied to the directory"}})}).unwrap();
        let remote = if incoming {
            Some(w.dispatch("skill.create",json!({"command_id":id(),"payload":{"name":"New global skill","data":{"body":"An immutable global source"}}})).unwrap()["id"].as_str().unwrap().to_owned())
        } else {
            None
        };
        let fired = Arc::new(AtomicBool::new(false));
        let entered = fired.clone();
        let local = e.clone();
        let project_scope = project.clone();
        let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if entered.swap(true, Ordering::AcqRel) {
                return;
            }
            local.execute("skill.create",Command{command_id:"local-source-during-copy".into(),target:None,expected_revision:None,payload:json!({"name":"Concurrent local skill","scope":format!("project:{project_scope}"),"data":{"body":"Must survive the immutable source copy"}})}).unwrap();
        });
        w.global.subscribe_changes(Arc::downgrade(&listener));
        e.sync_shared_configuration(w.global.as_ref(), None)
            .unwrap();
        assert!(fired.load(Ordering::Acquire));
        let state = e.store.configuration_snapshot().unwrap();
        assert_eq!(
            state
                .state
                .governance
                .iter()
                .filter(|r| r.r#type == "skill" && r.name == "Concurrent local skill")
                .count(),
            1
        );
        if let Some(remote) = remote {
            assert!(state.record(&remote).is_some());
        }
    }
}

#[test]
fn opened_step_qualification_is_exact_and_never_restores_engineering_history() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
    let draft=w.dispatch("workflow.create",json!({"command_id":id(),"payload":{"name":"Exact readiness","document":{"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"confirm","name":"Confirm","stage":"work","executor":{"type":"control"},"confirmation":"human"}]}}})).unwrap();
    let workflow=w.dispatch("workflow.publish",json!({"command_id":id(),"target":draft["id"],"expected_revision":draft["revision"],"payload":{}})).unwrap();
    let project = w
        .dispatch(
            "project.create",
            json!({"command_id":id(),"payload":{"title":"Readiness project"}}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let run=w.dispatch("workflow.start",json!({"project_id":project,"command_id":id(),"target":project,"expected_revision":1,"payload":{"workflow_ref":workflow["id"],"role_agent_bindings":{}}})).unwrap();
    let status = w
        .dispatch(
            "query.workflow_instance",
            json!({"project_id":project,"id":run["id"],"view":"status"}),
        )
        .unwrap();
    let execution = status["runtime"]["steps"][0]["id"].clone();
    let engine = w.project_engine(Some(&project)).unwrap();
    let baseline = engine.store.snapshot().unwrap();
    let original = baseline.configuration(run["id"].as_str().unwrap()).unwrap();
    let trace = performance::Trace::start();
    let summary = w
        .dispatch(
            "query.project_workflow_summary",
            json!({"project_id":project,"limit":1}),
        )
        .unwrap();
    assert_eq!(summary, json!({"total":1,"records":[original]}));
    assert_eq!(
        w.dispatch(
            "query.project_workflow_summary",
            json!({"project_id":project,"offset":1})
        )
        .unwrap(),
        json!({"total":1,"records":[]})
    );
    let ready=w.dispatch_display_read("query.workflow_instance",json!({"project_id":project,"id":run["id"],"execution_id":execution,"view":"qualification"}), None).unwrap().value;
    assert_eq!(
        ready["completion_status"][execution.as_str().unwrap()]["satisfied"],
        true
    );
    assert_eq!(
        ready["completion_status"][execution.as_str().unwrap()]["human_satisfied"],
        false
    );
    assert_eq!(ready["advisory"], true);
    assert!(
        w.dispatch_display_read(
            "query.workflow_instance",
            json!({"project_id":project,"id":run["id"],"execution_id":id(),"view":"qualification"}),
            None,
        )
        .is_err()
    );
    let report = trace.finish();
    for name in [
        "project.full_snapshot",
        "project.transaction",
        "workflow.compile",
    ] {
        assert!(report["spans"][name].is_null(), "{report}");
    }
}
