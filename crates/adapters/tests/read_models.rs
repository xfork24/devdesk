//! SQL projection fixtures deliberately contain an unreadable historical plan.
//! They do not represent a dispatched worker or a workflow acceptance run.
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{Command, Engine, EngineeringStore, read_models::*};
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_domain::{References, content_digest};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

const PROJECT: &str = "c8516249-60a3-444a-988d-e5bf0e257611";
const OTHER: &str = "f62ddc57-2b83-4c67-b8d6-a7dbbfaaeea8";
fn record(conn: &Connection, project: &str, id: &str, kind: &str, definition: &str, data: Value) {
    record_in_scope(
        conn,
        &format!("project:{project}"),
        id,
        kind,
        definition,
        data,
    );
}
fn record_in_scope(
    conn: &Connection,
    scope: &str,
    id: &str,
    kind: &str,
    definition: &str,
    mut data: Value,
) {
    if kind == "workflow_instance"
        && let Some(project) = scope.split(':').nth(1)
    {
        data["project_id"] = json!(project);
    }
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,supersedes,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,?2,?3,?4,?1,1,NULL,?5,'{}',?6,0,7,1)",
        params![id,kind,definition,json!(scope).to_string(),content_digest(&References::new(),&data),data.to_string()]).unwrap();
    if kind == "workflow_instance" {
        // Direct SQL fault fixtures must seed the transactional display rows
        // too; production resource commits maintain them atomically.
        conn.execute(
            "INSERT INTO workflow_display_headers(instance_id,record) VALUES(?1,?2)",
            params![id, json!({"id":id,"revision":7,"data":data}).to_string()],
        )
        .unwrap();
        for step in data["steps"].as_array().into_iter().flatten() {
            conn.execute("INSERT INTO workflow_display_steps(instance_id,execution_id,data,step_revision) VALUES(?1,?2,?3,1)",params![id,step["id"].as_str().unwrap(),step.to_string()]).unwrap();
        }
    }
}
fn fixture(
    root: &std::path::Path,
    project: &str,
    text: &str,
) -> (Arc<SqliteStore>, Connection, Value) {
    let store = Arc::new(SqliteStore::open_project(root, project).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(std::path::PathBuf::from(store.root()))),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    engine
        .execute(
            "project.create",
            Command {
                command_id: "fixture-project".into(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Fixture"}),
            },
        )
        .unwrap();
    let (session, turn) = runtime::queue_conversation(&engine, project);
    let conn = Connection::open(
        SqliteStore::project_path(root, project)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    // Runtime rows below are projection/fault fixtures, not accepted worker receipts.
    let steps=(0..30).map(|n|json!({"id":format!("execution-{n}"),"step":format!("step-{n}"),"path":format!("work/{n}"),"status":"waiting_input","reason":"需补充输入"})).collect::<Vec<_>>();
    record(
        &conn,
        project,
        "root",
        "workflow_instance",
        "root",
        json!({"owner_id":project,"root_instance_id":"root","active_plan_revision_id":"frozen-plan","control_version":3,"status":"running","steps":steps,"plan":{"$source":"not-a-restorable-plan","large_history":"x".repeat(100_000)}}),
    );
    record(
        &conn,
        project,
        "controller",
        "workflow_controller",
        "root",
        json!({"project_id":project,"owner_id":project,"root_instance_id":"root","role_ref":"role:1","agent_revision_id":"agent:1","harness_permission_ref":"permission:1","session_id":session,"epoch":3,"status":"active","reason":text}),
    );
    record(
        &conn,
        project,
        "driver",
        "workflow_driver",
        "root",
        json!({"mode":"rust","epoch":4,"reason":text}),
    );
    for n in 0..35 {
        conn.execute("INSERT INTO conversation_events(id,session_id,turn_id,sequence,deduplication_key,kind,content,time) VALUES(?1,?2,?3,?4,?1,'assistant',?5,1)",params![format!("message-{n}"),session,turn,n,text]).unwrap();
        record(
            &conn,
            project,
            &format!("call-{n}"),
            "controller_tool_call",
            "controller",
            json!({"method":"workflow.inspect","status":"succeeded","receipt":{"status":"applied","body":"x".repeat(10_000)}}),
        );
    }
    let request = json!({"operation":"apply","arguments":{"exact_content":"原始内容".repeat(10_000),"wide_integer":u64::MAX},"reason":"需要明确决定"});
    for n in 0..15 {
        record(
            &conn,
            project,
            &format!("decision-{n}"),
            "workflow_pending_decision",
            "root",
            json!({"status":"pending","plan_revision_id":"plan:1","request":request}),
        );
    }
    for n in 0..25 {
        record(
            &conn,
            project,
            &format!("child-{n}"),
            "workflow_instance",
            "child",
            json!({"owner_id":project,"root_instance_id":"root","parent_execution_id":"execution-0","status":"running"}),
        );
    }
    conn.execute(
        "INSERT INTO events(target,kind,detail,time) VALUES('root','fixture','',1)",
        [],
    )
    .unwrap();
    (store, conn, request)
}

#[test]
fn summaries_are_bounded_and_never_restore_unreadable_history_or_authorize_actions() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, original) = fixture(directory.path(), PROJECT, &"甲".repeat(5000));
    let query = ControllerQuery::parse(&json!({"root_instance_id":"root"})).unwrap();
    let trace = performance::Trace::start();
    let controller = store.orchestration_summary(&query).unwrap();
    let driver = store.workflow_driver_summary("root").unwrap();
    let management = store.workflow_management_summary("root", 0).unwrap();
    let measurement = trace.finish();
    for stage in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_reference",
        "source.restore_records",
    ] {
        assert!(
            measurement["spans"][stage].is_null(),
            "{stage}: {measurement}"
        );
    }
    assert_eq!(controller.cursor, conn.last_insert_rowid() as u64);
    assert_eq!(controller.messages.len(), 10);
    assert!(
        controller
            .messages
            .iter()
            .all(|m| m.truncated && m.content_excerpt.chars().count() == 512)
    );
    assert!(controller.more.messages && controller.more.calls && controller.more.decisions);
    assert_eq!(controller.calls.len(), 5);
    assert_eq!(controller.decisions.len(), 10);
    let older=store.orchestration_summary(&ControllerQuery::parse(&json!({"root_instance_id":"root","decision_before":controller.next_decision_before})).unwrap()).unwrap();
    assert_eq!(older.decisions.len(), 5);
    assert!(!older.more.decisions);
    assert!(
        older
            .decisions
            .iter()
            .all(|row| !controller.decisions.iter().any(|newer| newer.id == row.id))
    );
    assert!(serde_json::to_vec(&controller).unwrap().len() <= 64 * 1024);
    assert!(serde_json::to_vec(&driver).unwrap().len() <= 64 * 1024);
    assert!(driver.observed_only && driver.waiting_more);
    assert_eq!(driver.waiting.len(), 20);
    assert_eq!(driver.instance_revision, 7);
    for step in &driver.waiting {
        assert_eq!(step.revision, 7);
        assert_eq!(step.plan_revision_id, "frozen-plan");
        assert_eq!(step.control_version, 3);
        assert_eq!(step.step_revision, 1);
    }
    assert_eq!(management.record.revision, 7);
    assert!(management.observed_only && management.children_more);
    assert_eq!(management.children.len(), 20);
    assert!(serde_json::to_vec(&management).unwrap().len() <= 64 * 1024);
    let older = store
        .workflow_management_summary("root", management.next_child_offset.unwrap())
        .unwrap();
    assert_eq!(older.children.len(), 5);
    assert!(!older.children_more);
    let detail = store
        .workflow_decision(&controller.decisions[0].id)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&detail.data.request_json).unwrap(),
        original
    );
    assert_eq!(detail.revision, controller.decisions[0].revision);
}

#[test]
fn manager_cards_use_exact_display_facts_without_rebuilding_historical_plans() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "经理摘要");
    let update = |id: &str, data: Value| {
        conn.execute(
            "UPDATE governance_records SET data=?2,content_hash=?3 WHERE id=?1",
            params![
                id,
                data.to_string(),
                content_digest(&References::new(), &data)
            ],
        )
        .unwrap();
        if data["steps"].is_array() {
            conn.execute(
                "UPDATE workflow_display_headers SET record=json_set(record,'$.data',json(?2)) WHERE instance_id=?1",
                params![id, data.to_string()],
            )
            .unwrap();
            for step in data["steps"].as_array().unwrap() {
                conn.execute(
                    "UPDATE workflow_display_steps SET data=?3 WHERE instance_id=?1 AND execution_id=?2",
                    params![id, step["id"].as_str().unwrap(), step.to_string()],
                )
                .unwrap();
            }
        }
    };
    let mut root: Value = conn
        .query_row(
            "SELECT data FROM governance_records WHERE id='root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap();
    root["active_plan_revision_id"] = json!("plan:1");
    root["steps"][0]["status"] = json!("waiting_confirmation");
    root["steps"][0]["input_digest"] = json!("original-input");
    root["steps"][0]["outputs"] = json!({"result":"original-output"});
    update("root", root.clone());
    let request = json!({"operation":"workflow.step.confirm","arguments":{"execution_id":"execution-0","input_digest":"original-input","outputs":{"result":"original-output"}},"interaction":{"title":"真实人类请求"}});
    for n in 0..15 {
        update(
            &format!("decision-{n}"),
            json!({"status":"pending","plan_revision_id":"plan:1","request":request}),
        );
    }
    update(
        "child-0",
        json!({"project_id":PROJECT,"owner_id":PROJECT,"root_instance_id":"root","parent_execution_id":"execution-0","active_plan_revision_id":"child-plan:1","status":"running"}),
    );
    record(
        &conn,
        PROJECT,
        "signed-child-decision",
        "workflow_pending_decision",
        "child-0",
        json!({"status":"approved","plan_revision_id":"child-plan:1","request":{"operation":"inspect","arguments":{}},"decision_ref":"exact-human-decision","reason":"人类原始意见"}),
    );
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(std::path::PathBuf::from(store.root()))),
        runtime::adapters(),
    );
    // Measure the ordinary refresh path separately from the initial immutable
    // catalog load; the unreadable historical plan must never be expanded.
    engine.store.configuration_snapshot().unwrap();
    let trace = performance::Trace::start();
    let view = engine
        .query(
            "query.orchestration",
            &json!({"root_instance_id":"root"}),
            1,
        )
        .unwrap();
    let measurement = trace.finish();
    for stage in [
        "project.full_snapshot",
        "source.restore_reference",
        "source.restore_records",
    ] {
        assert!(measurement["spans"][stage].is_null(), "{measurement}");
    }
    let cards = view["decisions"].as_array().unwrap();
    assert_eq!(cards.len(), 11);
    assert!(cards.iter().all(|card| card["current"] == true));
    let signed = cards
        .iter()
        .find(|card| card["id"] == "signed-child-decision")
        .unwrap();
    assert_eq!(signed["ancestor_instance_ids"], json!(["root"]));
    assert_eq!(signed["data"]["decision_ref"], "exact-human-decision");
    assert_eq!(signed["data"]["reason"], "人类原始意见");
    assert!(serde_json::to_vec(&view).unwrap().len() < 32 * 1024);
    root["steps"][0]["outputs"] = json!({"result":"different-output"});
    update("root", root.clone());
    let changed = engine
        .query(
            "query.orchestration",
            &json!({"root_instance_id":"root"}),
            1,
        )
        .unwrap();
    assert!(
        changed["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|card| card["definition_id"] == "root")
            .all(|card| card["current"] == false)
    );
    root["steps"][0]["outputs"] = json!({"result":"original-output"});
    root["steps"][0]["status"] = json!("completed");
    update("root", root.clone());
    let context = store
        .workflow_decision_context("root", Some("execution-0"))
        .unwrap();
    assert_eq!(
        context.execution.unwrap().status,
        devdesk_domain::StepExecutionStatus::Completed
    );
    assert!(
        store
            .workflow_decision_context("root", Some("not-current-execution"))
            .unwrap()
            .execution
            .is_none()
    );
    let other = SqliteStore::open_project(directory.path(), OTHER).unwrap();
    assert!(other.workflow_decision_context("root", None).is_err());
    root["parent_execution_id"] = json!("execution-0");
    update("root", root);
    assert!(
        store
            .workflow_decision_context("root", None)
            .unwrap_err()
            .message
            .contains("循环")
    );
}

#[test]
fn identities_are_scoped_and_a_global_session_cannot_leak_through_a_controller_reference() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "Project A");
    let (other, _other_conn, _) = fixture(directory.path(), OTHER, "Project B");
    let query = ControllerQuery::parse(&json!({"id":"controller"})).unwrap();
    assert_eq!(
        store.orchestration_summary(&query).unwrap().messages[0].content_excerpt,
        "Project A"
    );
    assert_eq!(
        other.orchestration_summary(&query).unwrap().messages[0].content_excerpt,
        "Project B"
    );
    record(
        &conn,
        OTHER,
        "foreign-controller",
        "workflow_controller",
        "root",
        json!({"project_id":PROJECT,"owner_id":PROJECT,"epoch":1}),
    );
    assert!(
        store
            .orchestration_summary(
                &ControllerQuery::parse(&json!({"id":"foreign-controller"})).unwrap()
            )
            .unwrap()
            .record
            .is_none()
    );
    record(
        &conn,
        OTHER,
        "foreign-decision",
        "workflow_pending_decision",
        "root",
        json!({"status":"pending","plan_revision_id":"plan:1","request":{}}),
    );
    assert_eq!(
        store
            .workflow_decision("foreign-decision")
            .unwrap_err()
            .code,
        "not_found"
    );
    conn.execute("INSERT INTO sessions(id,owner_id,agent,harness,sandbox,origin,project_id,runtime_binding,scope) VALUES('global-session','global','simplified-agent-analyst:8','fixture','fixture','external','00000000-0000-4000-8000-000000000000','{}','global')",[]).unwrap();
    conn.execute("INSERT INTO turns(id,project_id,session_id,client_message_id,input,input_digest,status,consumed,revision) VALUES('global-turn','00000000-0000-4000-8000-000000000000','global-session','global-message','fixture','digest','queued',0,1)",[]).unwrap();
    conn.execute("INSERT INTO conversation_events(id,session_id,turn_id,sequence,deduplication_key,kind,content,time) VALUES('global-event','global-session','global-turn',1,'global-event','assistant','Global secret',1)",[]).unwrap();
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.session_id','global-session') WHERE id='controller'",[]).unwrap();
    assert!(
        store
            .orchestration_summary(&query)
            .unwrap()
            .messages
            .is_empty()
    );
}

#[test]
fn an_uncommitted_writer_does_not_hold_up_committed_summary_reads() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "Committed");
    conn.execute_batch("BEGIN IMMEDIATE; UPDATE governance_records SET revision=8,data=json_set(data,'$.status','closed') WHERE id='controller';").unwrap();
    let view = store
        .orchestration_summary(&ControllerQuery::parse(&json!({"owner_id":PROJECT})).unwrap())
        .unwrap();
    assert_eq!(view.record.as_ref().unwrap().revision, 7);
    assert_eq!(view.record.as_ref().unwrap().data.status, "active");
    conn.execute_batch("ROLLBACK;").unwrap();
    assert!(ControllerQuery::parse(&json!({"id":"controller","owner_id":PROJECT})).is_err());
    assert!(ControllerQuery::parse(&json!({"owner_id":PROJECT,"workspace_only":"false"})).is_err());
    assert!(ControllerQuery::parse(&json!({"id":"controller","decision_before":null})).is_err());
    assert!(ControllerQuery::parse(&json!({"id":"controller","owner_id":null})).is_err());
    assert!(ControllerQuery::parse(&json!({"id":"controller","other_project":OTHER})).is_err());
}

#[test]
fn large_configuration_bindings_are_marked_without_expanding_the_plan() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "Committed");
    let bindings = json!({"long-role":"x".repeat(9000)});
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.role_agent_bindings',json(?1)) WHERE id='root'",[bindings.to_string()]).unwrap();
    let view = store.workflow_management_summary("root", 0).unwrap();
    assert!(view.record.data.configuration_truncated);
    assert!(view.record.data.role_agent_bindings.is_empty());
    assert!(serde_json::to_vec(&view).unwrap().len() <= 64 * 1024);
}

#[test]
fn a_real_feature_root_and_its_driver_are_read_without_restoring_the_plan() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_project(root.path(), PROJECT).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(std::path::PathBuf::from(store.root()))),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(root.path()).unwrap(),
    ));
    engine.bootstrap().unwrap();
    let command = |key: &str,
                   target: Option<String>,
                   expected_revision: Option<u64>,
                   payload: Value| Command {
        command_id: key.into(),
        target,
        expected_revision,
        payload,
    };
    engine
        .execute(
            "project.create",
            command(
                "project",
                None,
                None,
                json!({"title":"Real feature source scope"}),
            ),
        )
        .unwrap();
    runtime::bind_default_permissions(&engine);
    runtime::configure_project(&engine, PROJECT);
    let feature = engine
        .execute(
            "feature.create",
            command(
                "feature",
                None,
                None,
                json!({"project_id":PROJECT,"title":"Feature metadata"}),
            ),
        )
        .unwrap();
    let fid = feature["id"].as_str().unwrap();
    let state = store.snapshot().unwrap();
    let instance = state
        .governance
        .iter()
        .find(|row| row.r#type == "workflow_instance" && row.data["owner_id"] == fid)
        .unwrap()
        .clone();
    let id = instance.id.as_str();
    assert!(instance.scope.is_feature());
    let epoch = state
        .governance
        .iter()
        .find(|row| row.r#type == "workflow_driver" && row.definition_id == id)
        .and_then(|row| row.data["epoch"].as_u64())
        .unwrap_or(0);
    engine
        .execute(
            "workflow.driver.select",
            command(
                "driver",
                Some(id.into()),
                Some(instance.revision),
                json!({"mode":"rust","expected_driver_epoch":epoch}),
            ),
        )
        .unwrap();
    // Read projection fixture, not a signed decision or worker acceptance.
    let conn = Connection::open(
        SqliteStore::project_path(root.path(), PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    record_in_scope(
        &conn,
        &instance.scope.0,
        "feature-decision",
        "workflow_pending_decision",
        id,
        json!({"status":"pending","plan_revision_id":instance.data["active_plan_revision_id"],"request":{"operation":"pause","arguments":{}}}),
    );
    let trace = performance::Trace::start();
    let management = store.workflow_management_summary(id, 0).unwrap();
    let driver = store.workflow_driver_summary(id).unwrap();
    let decision = store.workflow_decision("feature-decision").unwrap();
    let measurement = trace.finish();
    assert_eq!(management.record.data.owner_id, fid);
    assert_eq!(management.record.data.project_id, PROJECT);
    assert_eq!(driver.root_instance_id, id);
    assert_eq!(driver.driver.unwrap().data.mode, "rust");
    assert_eq!(decision.revision, 7);
    assert!(measurement["spans"]["project.full_snapshot"].is_null());
    assert!(measurement["spans"]["source.restore_reference"].is_null());
}

#[test]
fn execution_workspace_pages_native_facts_and_excludes_unrelated_history() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(root.path(), PROJECT, "excerpt");
    // SQL-only large-history/fault fixtures; no worker or external action is claimed.
    for n in 0..101 {
        conn.execute("INSERT INTO features(id,project_id,title,status,revision,input_version,predecessor,source_materials) VALUES(?1,?2,?3,'running',3,2,NULL,?4)",params![format!("f-{n}"),PROJECT,"title".repeat(100),json!(vec!["large".repeat(10_000)]).to_string()]).unwrap();
    }
    for n in 0..101 {
        conn.execute("INSERT INTO tasks(id,feature_id,title,status,revision,inherited_snapshot,dependencies,worktree,scope) VALUES(?1,'f-0','Task','pending',4,NULL,?2,'',?3)",params![format!("task-{n}"),json!(vec!["dependency";1000]).to_string(),json!({"input_refs":[],"acceptance":["large".repeat(10000)]}).to_string()]).unwrap();
    }
    let data = |owner: &str, parent: Option<&str>| json!({"owner_id":owner,"root_instance_id":"feature-root","parent_execution_id":parent,"status":"running","steps":[{"id":"invoke","status":"running"}],"plan":{"$source":"cannot-be-restored","standard_ref":"standard:1"},"role_agent_bindings":{"enormous":"x".repeat(100_000)}});
    record_in_scope(
        &conn,
        &format!("feature:{PROJECT}:f-0"),
        "feature-root",
        "workflow_instance",
        "f-0",
        data("f-0", None),
    );
    record(
        &conn,
        PROJECT,
        "task-root",
        "workflow_instance",
        "task-0",
        data("task-0", Some("invoke")),
    );
    record(
        &conn,
        PROJECT,
        "invocation",
        "workflow_invocation",
        "invoke",
        json!({"child_instance_id":"task-root","member_identity":"task-0"}),
    );
    // A later nested child shares its owner, but it is not a work root.
    record(
        &conn,
        PROJECT,
        "nested",
        "workflow_instance",
        "task-0",
        data("task-0", Some("nested-call")),
    );
    record_in_scope(
        &conn,
        "global",
        "leak",
        "workflow_instance",
        "task-0",
        data("task-0", None),
    );
    record(
        &conn,
        OTHER,
        "other",
        "workflow_instance",
        "f-0",
        data("f-0", None),
    );
    let request = ExecutionWorkspaceQuery::parse(
        &json!({"project_id":PROJECT,"feature_id":"f-0","limit":50}),
    )
    .unwrap();
    let trace = performance::Trace::start();
    let first = store.execution_workspace(&request).unwrap();
    let trace = trace.finish();
    assert_eq!(first.total_features, 101);
    assert_eq!(first.features.len(), 50);
    assert!(
        first
            .features
            .iter()
            .all(|row| row.title_truncated && row.title.len() <= 128)
    );
    let detail = first.detail.as_ref().unwrap();
    assert_eq!(detail.total_tasks, 101);
    assert_eq!(detail.tasks.len(), 50);
    assert_eq!(detail.tasks[0].dependency_count, 1000);
    assert_eq!(
        first
            .configurations
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["feature-root", "task-root"]
    );
    assert!(first.configurations.iter().all(|row| row.metadata_only));
    assert!(serde_json::to_vec(&first).unwrap().len() < 64 * 1024);
    for name in [
        "project.full_snapshot",
        "source.restore_records",
        "catalog.build",
        "project.transaction",
    ] {
        assert!(trace["spans"][name].is_null(), "{trace}");
    }
    let second = store
        .execution_workspace(&ExecutionWorkspaceQuery {
            feature_offset: 50,
            task_offset: 50,
            ..request
        })
        .unwrap();
    assert!(
        first
            .features
            .iter()
            .all(|a| second.features.iter().all(|b| a.id != b.id))
    );
    assert!(detail.tasks.iter().all(|a| {
        second
            .detail
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .all(|b| a.id != b.id)
    }));
    let filter =
        ExecutionWorkspaceQuery::parse(&json!({"project_id":PROJECT,"work_filter":"running"}))
            .unwrap();
    assert_eq!(
        store.execution_workspace(&filter).unwrap().total_features,
        1
    );
    let mut foreign = filter;
    foreign.feature_id = Some("missing".into());
    assert_eq!(
        store.execution_workspace(&foreign).unwrap_err().code,
        "not_found"
    );

    conn.execute(
        "INSERT INTO repository_operations(id,project_id,action,status,reason,revision) VALUES('register',?1,'{}','completed','',1)",
        [PROJECT],
    ).unwrap();
    for (owner, kind, branch) in [
        ("f-0", "feature", "devdesk/features/f-0"),
        ("task-0", "task", "devdesk/tasks/recovered-source"),
    ] {
        conn.execute(
            "INSERT INTO repository_worktrees(id,project_id,owner_id,kind,branch,path,base,operation_id) VALUES(?1,?2,?1,?3,?4,?1,'base','register')",
            params![owner, PROJECT, kind, branch],
        ).unwrap();
    }
    record(
        &conn,
        PROJECT,
        "workflow-delivery:feature-root",
        "workflow_delivery",
        "feature-root",
        json!({"publication":{"owner_id":"f-0","pull_request":{"head_branch":"devdesk/candidates/published","number":31}},"completion":{"pull_request":{"merged":true}}}),
    );
    record(
        &conn,
        PROJECT,
        "workflow-delivery:task-root",
        "workflow_delivery",
        "task-root",
        json!({"publication":{"owner_id":"task-0","source_branch":"devdesk/tasks/recovered-source"},"completion":{"pull_request":{"merged":true}}}),
    );
    let tree = store.feature_branches("f-0", 0).unwrap();
    assert_eq!(tree["feature_branch"], "devdesk/features/f-0");
    assert_eq!(
        tree["candidate"],
        json!({"branch":"devdesk/candidates/published","pull_request_number":31,"status":"completed"})
    );
    assert_eq!(tree["tasks"][0]["branch"], "devdesk/tasks/recovered-source");
    assert_eq!(tree["tasks"][0]["status"], "completed");
    assert!(tree["tasks"][1]["branch"].is_null());
    assert_eq!(tree["tasks"][1]["status"], "pending");
    assert_eq!(tree["tasks"].as_array().unwrap().len(), 50);
    assert_eq!(tree["next_task_offset"], 50);
    let last = store.feature_branches("f-0", 100).unwrap();
    assert_eq!(last["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(last["tasks"][0]["id"], "task-100");
    assert!(last["next_task_offset"].is_null());
    assert!(store.feature_branches("f-1", 0).unwrap()["candidate"].is_null());
    assert_eq!(
        store.feature_branches("missing", 0).unwrap_err().code,
        "not_found"
    );
}

#[test]
fn scheduling_counts_active_turns_once_and_reads_committed_facts_without_restoration() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(root.path(), PROJECT, "excerpt");
    let session: String = conn
        .query_row("SELECT id FROM sessions LIMIT 1", [], |row| row.get(0))
        .unwrap();
    for n in 0..2 {
        conn.execute("INSERT INTO executions(id,session_id,operation_id,image_digest,status,writer_revoked) VALUES(?1,?2,?1,'fixture','running',0)",params![format!("count-execution-{n}"),session]).unwrap();
    }
    for (status, expected) in [
        ("queued", 0),
        ("dispatching", 1),
        ("running", 1),
        ("waiting_interaction", 1),
        ("needs_reconciliation", 1),
        ("completed", 0),
        ("failed", 0),
        ("cancelled", 0),
    ] {
        conn.execute("UPDATE turns SET status=?1", [status])
            .unwrap();
        assert_eq!(
            store.active_conversation_count().unwrap(),
            expected,
            "{status}"
        );
        assert_eq!(
            store.scheduling_hints().unwrap().running_conversations,
            expected,
            "{status}"
        );
    }
    conn.execute("UPDATE turns SET status='running'", [])
        .unwrap();
    conn.execute_batch("BEGIN IMMEDIATE; UPDATE executions SET writer_revoked=1;")
        .unwrap();
    let trace = performance::Trace::start();
    let hints = store.scheduling_hints().unwrap();
    assert!(hints.live && hints.pending);
    assert_eq!(hints.running_conversations, 1);
    assert_eq!(store.active_conversation_count().unwrap(), 1);
    let trace = trace.finish();
    for name in [
        "project.full_snapshot",
        "source.restore_records",
        "project.transaction",
        "catalog.build",
    ] {
        assert!(trace["spans"][name].is_null(), "{trace}");
    }
    conn.execute_batch("COMMIT;").unwrap();
    assert_eq!(store.active_conversation_count().unwrap(), 0);
}

#[test]
fn event_tails_mark_large_events_for_reset_without_copying_their_bodies() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(root.path(), PROJECT, "excerpt");
    let cursor = store.event_cursor().unwrap();
    conn.execute(
        "INSERT INTO events(target,kind,detail,time) VALUES('root','large',?1,1)",
        ["x".repeat(1_000_000)],
    )
    .unwrap();
    conn.execute("INSERT INTO events(target,kind,detail,time) VALUES('root','small','exact persisted detail',1)",[]).unwrap();
    let hints = store.event_hints_after(cursor, 100).unwrap();
    assert_eq!(hints.len(), 2);
    assert!(hints[0].omitted && hints[0].event.detail.is_empty());
    assert!(!hints[1].omitted);
    assert_eq!(hints[1].event.detail, "exact persisted detail");
    let exact = store.events_after(cursor, 100).unwrap();
    assert_eq!(exact[0].detail.len(), 1_000_000);
    assert_eq!(exact[1].id, hints[1].event.id);
}

#[test]
fn operational_tool_filter_recognizes_compacted_bound_tool_references() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(root.path(), PROJECT, "excerpt");
    let data = json!({"status":"succeeded","remote_effect":"applied","invocation":{"tool":{"$source":"bound_tool","tool_ref":"original-tool:1","binding_ref":"original-binding:1"}}});
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('compacted-call','tool_call','compacted-call',?1,'Compacted external call',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),data.to_string()]).unwrap();
    store.configuration_snapshot().unwrap();
    let trace = performance::Trace::start();
    let value = store
        .operational_read(
            "query.tool_calls",
            &json!({"tool_ref":"original-tool:1","view":"summary"}),
        )
        .unwrap();
    assert_eq!(value["total"], 1);
    assert_eq!(value["records"][0]["id"], "compacted-call");
    assert_eq!(value["records"][0]["data"]["status"], "succeeded");
    assert_eq!(value["records"][0]["metadata_only"], true);
    assert_eq!(
        store
            .operational_read(
                "query.tool_calls",
                &json!({"tool_ref":"different-tool:1","view":"summary"})
            )
            .unwrap()["total"],
        0
    );
    let measured = trace.finish();
    for stage in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(measured["spans"][stage].is_null(), "{measured}");
    }
}

#[test]
fn exact_control_reads_do_not_restore_an_unrelated_plan_and_expose_original_pr_creation() {
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "Control");
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    let execution = json!({"id":"publish-execution","workflow_instance_id":"root","workflow_revision_id":"missing:1","plan_revision_id":"missing-plan","node_id":"node","contract_digest":"digest","path":"/publish","step":"publish","round":1,"sequence":1,"status":"running","input_digest":"inputs","inputs":{"document":"exact input"},"outputs":{},"agent_revision_id":null,"session_id":null,"turn_id":null,"tool_call_id":"publish-execution","started":1,"finished":null,"reason":""});
    conn.execute(
        "UPDATE governance_records SET data=json_set(data,'$.steps',json(?1)) WHERE id='root'",
        [json!([execution]).to_string()],
    )
    .unwrap();
    record(
        &conn,
        PROJECT,
        "material",
        "typed_material",
        "material",
        json!({"root_instance_id":"root","value":{"body":"exact output"}}),
    );
    record(
        &conn,
        PROJECT,
        "publish-execution",
        "platform_call",
        "publish-execution",
        json!({"handler":"git.publish","issuer":"kernel_platform","root_instance_id":"root","status":"dispatched","effect_id":"effect","request":{"execution_id":"publish-execution","instance_id":"root","project_id":PROJECT,"arguments":{"candidate":{"revision":"candidate"}}}}),
    );
    let request = devdesk_domain::PullRequestRequest {
        repository: "fixture/repo".into(),
        project: PROJECT.into(),
        merge: "physical".into(),
        head_branch: "candidate-branch".into(),
        base_branch: "main".into(),
        candidate: "candidate".into(),
        expected_target: "parent".into(),
        number: None,
        projection: None,
    };
    let stage = "platform-publish-stage:effect:pull_request";
    record(
        &conn,
        PROJECT,
        stage,
        "platform_publish_stage",
        "effect",
        json!({"issuer":"kernel_platform","action":{"PullRequest":request}}),
    );
    let receipt = json!({"repository":"fixture/repo","number":7,"url":"https://example.test/7","head":"candidate","base":"parent","head_branch":"candidate-branch","base_branch":"main","merged":false,"merge_commit":null,"marker":request.marker()});
    record(
        &conn,
        PROJECT,
        "pr-observation",
        "platform_effect_observation",
        stage,
        json!({"issuer":"kernel_platform","effect_id":stage,"phase":"result","result":{"Ok":receipt}}),
    );
    conn.execute(
        "UPDATE governance_records SET immutable=1 WHERE id IN ('material',?1,'pr-observation')",
        [stage],
    )
    .unwrap();
    store.configuration_snapshot().unwrap();
    let trace = performance::Trace::start();
    let input = engine
        .query(
            "workflow.execution.inspect",
            &json!({"id":"root","execution_id":"publish-execution","part":"inputs"}),
            4,
        )
        .unwrap();
    assert_eq!(input["value_chunk"], "{\"document\":\"exact input\"}");
    let outputs = engine
        .query(
            "workflow.outputs.read",
            &json!({"id":"root","ids":["material"]}),
            4,
        )
        .unwrap();
    assert_eq!(
        outputs["outputs"][0]["data"]["value"]["body"],
        "exact output"
    );
    let observed = engine
        .query(
            "workflow.execution.inspect",
            &json!({"id":"root","execution_id":"publish-execution"}),
            4,
        )
        .unwrap();
    assert_eq!(observed["publication"]["creation_receipt"], receipt);
    assert_eq!(observed["publication"]["advisory"], true);
    assert_eq!(observed["publication"]["observation_id"], "pr-observation");
    let mut feature_step = execution.clone();
    feature_step["workflow_instance_id"] = json!("feature-read");
    feature_step["tool_call_id"] = Value::Null;
    record_in_scope(
        &conn,
        &format!("feature:{PROJECT}:fixture-feature"),
        "feature-read",
        "workflow_instance",
        "fixture-feature",
        json!({"project_id":PROJECT,"root_instance_id":"feature-read","plan":{"$source":"unreadable"},"steps":[feature_step]}),
    );
    let feature_input = engine
        .query(
            "workflow.execution.inspect",
            &json!({"id":"feature-read","execution_id":"publish-execution","part":"inputs"}),
            4,
        )
        .unwrap();
    assert_eq!(feature_input["value_chunk"], input["value_chunk"]);
    let trace = trace.finish();
    for span in [
        "project.full_snapshot",
        "source.restore_reference",
        "project.transaction",
    ] {
        assert!(trace["spans"][span].is_null(), "{trace}");
    }
    assert!(
        engine
            .query(
                "workflow.execution.inspect",
                &json!({"id":"root","project_id":OTHER,"execution_id":"publish-execution"}),
                4
            )
            .is_err()
    );
    record(
        &conn,
        PROJECT,
        "foreign-material",
        "typed_material",
        "foreign-material",
        json!({"root_instance_id":"foreign","value":"foreign"}),
    );
    conn.execute(
        "UPDATE governance_records SET immutable=1 WHERE id='foreign-material'",
        [],
    )
    .unwrap();
    assert!(
        engine
            .query(
                "workflow.outputs.read",
                &json!({"id":"root","ids":["foreign-material"]}),
                4
            )
            .is_err()
    );
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.root_instance_id','foreign') WHERE id='publish-execution'", []).unwrap();
    assert!(
        engine
            .query(
                "workflow.execution.inspect",
                &json!({"id":"root","execution_id":"publish-execution"}),
                4
            )
            .is_err()
    );
}

#[test]
fn selected_predicate_reads_reuse_the_payload_but_commit_checks_a_fresh_epoch() {
    use devdesk_core::concurrency::*;
    let directory = tempfile::tempdir().unwrap();
    let (store, conn, _) = fixture(directory.path(), PROJECT, "Epoch");
    record(
        &conn,
        PROJECT,
        "waiver",
        "workflow_waiver",
        "plan",
        json!({"plan_revision_id":"plan","node_id":"node","requirement":"confirmation"}),
    );
    let key = ResourceKey::new(
        "workflow_waiver",
        json!(["plan", "node", "confirmation"]).to_string(),
    );
    let trace = performance::Trace::start();
    let original = store
        .read_resources(std::slice::from_ref(&key))
        .unwrap()
        .remove(0);
    let measured = trace.finish();
    assert_eq!(original.revision, 7);
    assert_eq!(
        measured["spans"]["sqlite.table_read_decode"]["calls"], 1,
        "{measured}"
    );
    conn.execute(
        "UPDATE governance_records SET revision=8 WHERE id='waiver'",
        [],
    )
    .unwrap();
    let setting = ResourceKey::new("context", "predicate-fixture");
    let change = PreparedChangeSet {
        command_id: "stale-predicate".into(),
        fingerprint: "predicate-test".into(),
        read_set: ReadSet::new(vec![
            ResourceVersion {
                resource: key.clone(),
                revision: original.revision,
            },
            ResourceVersion {
                resource: setting.clone(),
                revision: 0,
            },
        ])
        .unwrap(),
        changes: vec![ResourceChange {
            resource: setting.clone(),
            value: json!({"id":"predicate-fixture","owner_id":PROJECT,"content":"accepted with a fresh predicate","revision":1}),
        }],
        result: Value::Null,
        time: 1,
    };
    assert_eq!(
        store.commit_changes(&change).unwrap_err().code,
        "revision_conflict"
    );
    let fresh = store.read_resources(&[key]).unwrap().remove(0);
    assert_eq!(fresh.revision, 8);
    let mut change = change;
    change.command_id = "fresh-predicate".into();
    change.read_set = ReadSet::new(vec![
        ResourceVersion {
            resource: fresh.resource,
            revision: fresh.revision,
        },
        ResourceVersion {
            resource: setting,
            revision: 0,
        },
    ])
    .unwrap();
    store.commit_changes(&change).unwrap();
}
