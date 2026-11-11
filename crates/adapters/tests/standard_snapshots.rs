//! Snapshot and adoption contracts through production commands, native material tools and SQLite.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, integrations::NativeTools};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;

fn engine(path: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(path).unwrap()));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e.store.transact(None,&mut |state| {
        if state.configuration("fixture-standard-collection:1").is_err(){state.governance.push(SourceDefinition{kind:"delivery_type".into(),identity:"fixture-standard-collection".into(),revision:1,name:"Standard test collection".into(),scope:Scope::Global,data:json!({"format":"json","spec_refs":["fixture-spec:1"],"content_schema":{"type":"array"}})}.into_record()?);}
        Ok(Value::Null)
    }).unwrap();
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, mut payload: Value) -> Result<Value> {
    let state = e.store.snapshot()?;
    let revision = target.and_then(|id| {
        state
            .configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| {
                state
                    .projects
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.revision)
            })
            .or_else(|| {
                state
                    .features
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.revision)
            })
            .or_else(|| state.tasks.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    if let Some(row) = target
        .and_then(|id| state.configuration(id).ok())
        .filter(|r| r.kind == "workflow_instance")
    {
        payload["plan_revision_id"] = row.data["active_plan_revision_id"].clone();
    }
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: revision,
            payload,
        },
    )
}
fn publish(e: &Engine, doc: Value) -> String {
    let draft = command(
        e,
        "workflow.create",
        None,
        json!({"name":"规范合同","document":doc}),
    )
    .unwrap();
    command(e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn capture(e: &Engine, roots: Vec<String>) -> String {
    command(
        e,
        "standard.capture",
        None,
        json!({"name":"可复用规范","root_workflow_refs":roots}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn row(e: &Engine, id: &str) -> ConfigurationRecord {
    e.store
        .snapshot()
        .unwrap()
        .configuration(id)
        .unwrap()
        .clone()
}
fn dispatch(e: &Engine, id: &str, step: &str) {
    command(e, "workflow.step.dispatch", Some(id), json!({"step":step})).unwrap();
}
fn accept(e: &Engine, id: &str, step: &str) {
    let r = row(e, id);
    let execution = r.data["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == step)
        .unwrap();
    command(e,"workflow.step.accept",Some(id),json!({"step":step,"execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"],"reason":"核对实际工具回执"})).unwrap();
}
fn tools(e: &Engine) {
    for _ in 0..3 {
        for intent in e.pending_workflow_intents().unwrap() {
            let c: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            assert_eq!(intent.data["method"], "workflow.tool.execute");
            let request = e.configured_platform_request(&c).unwrap().unwrap();
            let result = e.enqueue_platform_tool(&c, &request);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
        e.drive_platform_tools().unwrap();
        e.evaluate_workflows().unwrap();
    }
}
fn child(e: &Engine, marker: &str) -> String {
    let spec=command(e,"spec.create",None,json!({"name":marker,"data":{"schema_version":3,"body":format!("检查 {marker} 的实际材料来源"),"checks":[]}})).unwrap();
    publish(
        e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"材料处理"}],"inputs":{"item":{"delivery_ref":"fixture-delivery-object:1"}},"steps":[
        {"key":"capture","name":"封存材料","stage":"work","spec_refs":[spec["id"]],"executor":{"type":"tool","tool_ref":"builtin-tool-materials-capture:1","arguments":{"marker":{"type":"constant","value":marker},"item":{"type":"source","source":{"step":"$input","port":"item"}}}},"outputs":{"result":{"delivery_ref":"fixture-delivery-object:1"}}},
        {"key":"finish","name":"结束核对","stage":"work","dependencies":["capture"],"executor":{"type":"control"}}
    ],"outputs":{"result":{"step":"capture","port":"result"}}}),
    )
}

#[test]
fn standards_freeze_only_normative_sources_and_detect_relationship_changes() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let workflow = child(&e, "A");
    let standard = capture(&e, vec![workflow.clone()]);
    let snapshot = e
        .query("query.standard", &json!({"id":standard}), 1)
        .unwrap();
    assert!(
        snapshot["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| ["workflow", "delivery_type", "spec"].contains(&n["kind"].as_str().unwrap()))
    );
    assert_eq!(
        row(&e, &standard)
            .data
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["root_workflow_refs", "schema_version"]
    );
    let duplicate = capture(&e, vec![workflow.clone()]);
    assert_eq!(duplicate, standard);
    let mut doc = row(&e, &workflow).data;
    let spec = doc["steps"][0]["spec_refs"].clone();
    doc["steps"][0]["spec_refs"] = json!([]);
    doc["steps"][1]["spec_refs"] = spec;
    let next = publish(&e, doc);
    let next = capture(&e, vec![next]);
    let diff = e
        .query(
            "query.standard_diff",
            &json!({"before_ref":standard,"after_ref":next}),
            1,
        )
        .unwrap();
    assert_eq!(diff["equal"], false);
    assert!(
        diff["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "relationship_removed")
    );
    assert!(
        command(
            &e,
            "standard.create",
            None,
            json!({"name":"旧规则","data":{"schema_version":3,"body":"禁止","baseline_rules":[]}})
        )
        .is_err()
    );
    let before = snapshot["snapshot_digest"].clone();
    drop(e);
    let e = engine(dir.path());
    assert_eq!(
        e.query("query.standard", &json!({"id":standard}), 1)
            .unwrap()["snapshot_digest"],
        before
    );
}

#[test]
fn tasks_in_one_feature_execute_distinct_standards_and_keep_history_after_switch_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let a = child(&e, "A");
    let b = child(&e, "B");
    let sb = capture(&e, vec![b.clone()]);
    let parent = publish(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"工作"}],"steps":[
            {"key":"create","name":"创建两项工作","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-tasks-create:1","arguments":{"tasks":{"type":"constant","value":[{"key":"a","title":"Task A","instructions":"按当前规范处理材料","dependencies":[]},{"key":"b","title":"Task B","instructions":"按当前规范处理材料","dependencies":[]}]}}},"outputs":{"result":{"delivery_ref":"fixture-delivery-object:1"}}},
            {"key":"call","name":"逐工作采用规范","stage":"work","dependencies":["create"],"executor":{"type":"workflow","workflow_ref":a,"work_binding":{"input":"item","pointer":"/work_ref"},"foreach":{"source":{"step":"create","port":"result","pointer":"/tasks"},"item_input":"item","max_concurrency":2,"min_items":2}},"outputs":{"result":{"delivery_ref":"fixture-standard-collection:1"}}}
        ]}),
    );
    let sa = capture(&e, vec![parent.clone(), a.clone()]);
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"统一规范使用者"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &e,
        "project.configuration.update",
        Some(&project),
        json!({"workflow_ref":parent,"standard_ref":sa,"role_agent_bindings":{}}),
    )
    .unwrap();
    let feature = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":project,"title":"不同 Task 采用不同规范"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let root = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["owner_id"] == feature)
        .unwrap()
        .id
        .clone();
    dispatch(&e, &root, "create");
    tools(&e);
    accept(&e, &root, "create");
    let state = e.store.snapshot().unwrap();
    let ta = state
        .tasks
        .iter()
        .find(|t| t.title == "Task A")
        .unwrap()
        .id
        .clone();
    let tb = state
        .tasks
        .iter()
        .find(|t| t.title == "Task B")
        .unwrap()
        .id
        .clone();
    command(
        &e,
        "work.configuration.update",
        Some(&tb),
        json!({"standard_ref":sb,"workflow_ref":b}),
    )
    .unwrap();
    dispatch(&e, &root, "call");
    e.evaluate_workflows().unwrap();
    let state = e.store.snapshot().unwrap();
    let ca = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["owner_id"] == ta)
        .unwrap()
        .id
        .clone();
    let cb = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["owner_id"] == tb)
        .unwrap()
        .id
        .clone();
    assert_eq!(row(&e, &ca).data["plan"]["standard_ref"], sa);
    assert_eq!(row(&e, &cb).data["plan"]["standard_ref"], sb);
    for (instance, marker) in [(&ca, "A"), (&cb, "B")] {
        dispatch(&e, instance, "capture");
        tools(&e);
        accept(&e, instance, "capture");
        let data = row(&e, instance).data;
        let output = data["steps"][0]["outputs"]["result"].as_str().unwrap();
        assert_eq!(row(&e, output).data["value"]["inputs"]["marker"], marker);
    }
    let before = row(&e, &ca);
    let old_plan = before.data["active_plan_revision_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_basis = row(&e, &old_plan);
    let old_execution = before.data["steps"][0].clone();
    let old_output = row(&e, old_execution["outputs"]["result"].as_str().unwrap());
    let count = e.store.snapshot().unwrap().governance.len();
    let changes = json!({"plan_revision_id":old_plan,"reason":"Task A 显式采用 B 规范","operations":[{"type":"adopt_standard","standard_ref":sb,"workflow_ref":b}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":ca,"changeset":changes}),
            1,
        )
        .unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().governance.len(),
        count,
        "预演必须只读"
    );
    command(
        &e,
        "workflow.change.apply",
        Some(&ca),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    assert_eq!(row(&e, &ca).data["plan"]["standard_ref"], sb);
    assert_eq!(
        row(&e, &root).data["plan"]["standard_ref"],
        sa,
        "子工作切换不能改写父工作采用依据"
    );
    assert_eq!(row(&e, &old_plan).data, old_basis.data);
    assert_eq!(row(&e, &old_output.id).data, old_output.data);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_execution_history" && r.data == old_execution)
    );
    dispatch(&e, &ca, "capture");
    tools(&e);
    accept(&e, &ca, "capture");
    let data = row(&e, &ca).data;
    assert_eq!(
        row(&e, data["steps"][0]["outputs"]["result"].as_str().unwrap()).data["value"]["inputs"]["marker"],
        "B"
    );
    let history = e
        .query(
            "query.work_configuration",
            &json!({"owner_id":ta,"project_id":project}),
            1,
        )
        .unwrap();
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
    drop(e);
    let e = engine(dir.path());
    assert_eq!(row(&e, &old_plan).data, old_basis.data);
    assert_eq!(row(&e, &old_output.id).data, old_output.data);
    assert_eq!(row(&e, &ca).data["plan"]["standard_ref"], sb);
    let db = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    let copied:i64=db.query_row("SELECT count(*) FROM source_definitions WHERE kind='standard' AND (json_extract(data,'$.body') IS NOT NULL OR json_extract(data,'$.baseline_rules') IS NOT NULL OR json_extract(data,'$.feature_id') IS NOT NULL)",[],|r|r.get(0)).unwrap();
    assert_eq!(copied, 0);
}

#[test]
fn invalid_standard_shapes_and_wrong_entry_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let a = child(&e, "A");
    let b = child(&e, "B");
    let standard = capture(&e, vec![a.clone()]);
    let state = e.store.snapshot().unwrap();
    assert!(standard_entry(&state, &standard, &b, &Scope::Global).is_err());
    assert!(
        ConfiguredStandard {
            schema_version: 4,
            root_workflow_refs: vec![a.clone(), a]
        }
        .validate()
        .is_err()
    );
    assert!(
        serde_json::from_value::<ConfiguredStandard>(
            json!({"schema_version":4,"root_workflow_refs":[b],"feature_id":"fake"})
        )
        .is_err()
    );
    assert!(
        command(
            &e,
            "standard.capture",
            None,
            json!({"name":"悬空引用","root_workflow_refs":["missing:1"]})
        )
        .is_err()
    );
}

#[test]
fn input_and_agent_binding_changes_preserve_the_normative_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let workflow = publish(
        &e,
        json!({"schema_version":6,"inputs":{"data":{"delivery_ref":"fixture-delivery-object:1"}},"stages":[{"key":"work","name":"处理"}],"steps":[{"key":"inspect","name":"核对材料","stage":"work","executor":{"type":"agent","role_ref":"builtin-role-analyst:1"},"task_instructions":"核对明确输入的材料。","inputs":{"data":{"type":"source","source":{"step":"$input","port":"data"}}},"outputs":{"result":{"delivery_ref":"fixture-delivery-object:1"}}}]}),
    );
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"同一规范下的执行选择"}),
    )
    .unwrap();
    let root=command(&e,"workflow.start",project["id"].as_str(),json!({"workflow_ref":workflow,"role_agent_bindings":{"builtin-role-analyst:1":"builtin-agent-sol-high-analyst:4"},"inputs":{"data":{"value":"before"}}})).unwrap()["id"].as_str().unwrap().to_owned();
    let initial = row(&e, &root);
    let standard = initial.data["plan"]["standard_ref"].clone();
    let replacement = command(
        &e,
        "agent.create",
        None,
        json!({"name":"同职责的另一执行者","data":row(&e,"builtin-agent-sol-high-analyst:4").data}),
    )
    .unwrap();
    let changes = json!({"plan_revision_id":initial.data["active_plan_revision_id"],"reason":"仅调整运行输入和执行者","operations":[{"type":"set_inputs","inputs":{"data":{"value":"after"}}},{"type":"set_bindings","bindings":{"builtin-role-analyst:1":replacement["id"]}}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            1,
        )
        .unwrap();
    assert_eq!(preview["standard_ref"], standard);
    assert_eq!(preview["standard_diff"]["equal"], true);
    command(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    assert_eq!(row(&e, &root).data["plan"]["standard_ref"], standard);
    assert_eq!(row(&e, &root).data["workflow_ref"], workflow);
    assert_eq!(
        row(
            &e,
            initial.data["active_plan_revision_id"].as_str().unwrap()
        )
        .data["inputs"]["data"]["value"],
        "before"
    );
}

#[test]
fn project_execution_selections_rebuild_in_their_own_access_context() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let workflow = publish(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"处理"}],"steps":[{"key":"one","name":"独立执行","stage":"work","executor":{"type":"control"}}]}),
    );
    let mut roots = Vec::new();
    let mut standards = Vec::new();
    for title in ["Project A", "Project B"] {
        let project = command(&e, "project.create", None, json!({"title":title})).unwrap();
        let root = command(
            &e,
            "workflow.start",
            project["id"].as_str(),
            json!({"workflow_ref":workflow}),
        )
        .unwrap();
        let view = e
            .query("workflow.inspect", &json!({"id":root["id"]}), 1)
            .unwrap();
        let standard = view["runtime"]["plan"]["standard_ref"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(row(&e, &standard).scope.project(), project["id"].as_str());
        roots.push(root["id"].as_str().unwrap().to_owned());
        standards.push(standard);
    }
    assert_ne!(
        standards[0], standards[1],
        "access envelopes in separate catalogs need distinct source identities"
    );
    let state = e.store.snapshot().unwrap();
    let snapshots = standards
        .iter()
        .map(|id| standard_snapshot(&state, id, &row(&e, id).scope).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        snapshots[0].snapshot_digest, snapshots[1].snapshot_digest,
        "storage access does not add normative semantics"
    );
    drop(e);
    let e = engine(dir.path());
    for (root, standard) in roots.iter().zip(&standards) {
        assert_eq!(
            e.query("workflow.inspect", &json!({"id":root}), 1).unwrap()["runtime"]["plan"]["standard_ref"],
            *standard
        );
    }
}
