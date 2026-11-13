use super::*;

fn add_history(engine: &Engine, name: &str) {
    let scope = engine
        .store
        .project_id()
        .map_or(Scope::global(), Scope::project);
    engine.store.transact(None, &mut |s| {
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
    let engine = w.active_engine(Some(&project)).unwrap();
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
    add_history(&w.active_engine(Some(&second)).unwrap(), "project-two");
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
        .unwrap_err()
        .code,
        "project_changed"
    );
    assert_eq!(
        w.dispatch(
            "query.history",
            json!({"scope":"global","include_tool_calls":true})
        )
        .unwrap(),
        global
    );
}
