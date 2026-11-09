use devdesk_adapters::{
    FileArtifacts, SqliteStore,
    integrations::{NativeTools, import_openapi, import_skill_zip},
};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
fn engine(root: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(root).unwrap()));
    e.bootstrap().unwrap();
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
    let s = e.store.snapshot()?;
    let revision = target.and_then(|key| {
        s.governance
            .iter()
            .find(|r| r.id == key)
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|p| p.id == key).map(|p| p.revision))
    });
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
fn api_tool(source: &str) -> Value {
    json!({"schema_version":2,"qualified_name":"ticket_lookup","description":"Find an authorized ticket","integration_ref":source,"implementation":{"kind":"http_api","method":"GET","path":"/tickets/{id}","parameters":[{"pointer":"/id","location":"path","name":"id","required":true}]},"input_schema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false},"effect":"read_only"})
}
#[test]
fn resource_revisions_scope_and_actual_schema_validation_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let source=command(&e,"integration.create",None,json!({"name":"Tickets","data":{"source":{"kind":"http_api","base_url":"https://tickets.example.test"}}})).unwrap();
    let tool = command(
        &e,
        "integration.tool.create",
        None,
        json!({"name":"Lookup","data":api_tool(source["id"].as_str().unwrap())}),
    )
    .unwrap();
    let validator = NativeTools::new(root.path()).unwrap();
    use devdesk_core::ports::ToolExecutor;
    assert!(
        validator
            .validate_arguments(&tool["data"]["input_schema"], &json!({"id":"A-1"}))
            .is_ok()
    );
    assert!(
        validator
            .validate_arguments(&tool["data"]["input_schema"], &json!({"id":5}))
            .is_err()
    );
    assert!(
        validator
            .validate_schema(&json!({"$ref":"https://untrusted.test/schema"}))
            .is_err()
    );
    let snapshot = e.store.snapshot().unwrap();
    let old = snapshot
        .configuration(source["id"].as_str().unwrap())
        .unwrap()
        .clone();
    let revision=command(&e,"integration.revise",source["id"].as_str(),json!({"name":"Tickets v2","data":{"source":{"kind":"http_api","base_url":"https://tickets-v2.example.test"}}})).unwrap();
    assert_eq!(revision["supersedes"], source["id"]);
    assert_eq!(
        json!(e.store.snapshot().unwrap().configuration(&old.id).unwrap()),
        json!(old)
    );
    assert!(command(&e,"integration.create",None,json!({"name":"Wrong scope","scope":{"kind":"project","project_id":"other"},"data":{"source":{"kind":"http_api","base_url":"https://example.test"}}})).is_err());
    let before = e.store.snapshot().unwrap().governance.len();
    e.bootstrap().unwrap();
    assert_eq!(e.store.snapshot().unwrap().governance.len(), before);
}
#[test]
fn project_binding_freezes_selected_tools_and_skill_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let source=command(&e,"integration.create",None,json!({"name":"Tickets","data":{"source":{"kind":"http_api","base_url":"https://tickets.example.test"}}})).unwrap();
    let tool = command(
        &e,
        "integration.tool.create",
        None,
        json!({"name":"Lookup","data":api_tool(source["id"].as_str().unwrap())}),
    )
    .unwrap();
    let binding=command(&e,"integration.binding.create",None,json!({"name":"Account A","data":{"integration_ref":source["id"],"harness_ref":DEFAULT_HARNESS_ID,"sandbox_ref":DEFAULT_PERMISSION_SANDBOX,"allowed_origins":["https://tickets.example.test"],"allowed_tools":["ticket_lookup"]}})).unwrap();
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Integration scope"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    let s = e.store.snapshot().unwrap();
    let mut a: AgentComposition = s
        .configuration(&default_configured_agent_id("analyst"))
        .unwrap()
        .decode()
        .unwrap();
    a.tools.push(tool["id"].as_str().unwrap().into());
    assert!(freeze_integrations(&s, &a, Some(pid)).is_err());
    command(
        &e,
        "integration.project.bind",
        None,
        json!({"project_id":pid,"bindings":{source["id"].as_str().unwrap():binding["id"]}}),
    )
    .unwrap();
    let lock = freeze_integrations(&e.store.snapshot().unwrap(), &a, Some(pid)).unwrap();
    assert_eq!(lock.tools.len(), 1);
    assert_eq!(lock.tools[0].revision_id, tool["id"]);
    let skill=command(&e,"skill.import",None,json!({"name":"Ticket method","package":{"body":"# Ticket method\n\nUse the selected lookup tool.","description":"Lookup method","requires_tools":[tool["id"]]}})).unwrap();
    a.skills.push(skill["id"].as_str().unwrap().into());
    assert_eq!(
        freeze_integrations(&e.store.snapshot().unwrap(), &a, Some(pid))
            .unwrap()
            .skills
            .len(),
        1
    );
    a.tools.retain(|id| id != tool["id"].as_str().unwrap());
    assert!(freeze_integrations(&e.store.snapshot().unwrap(), &a, Some(pid)).is_err());
    command(
        &e,
        "integration.disable",
        source["id"].as_str(),
        json!({"reason":"Emergency stop"}),
    )
    .unwrap();
    a.tools.push(tool["id"].as_str().unwrap().into());
    assert!(freeze_integrations(&e.store.snapshot().unwrap(), &a, Some(pid)).is_err());
}
#[test]
fn openapi_import_rejects_unknown_versions_and_preserves_required_parameters() {
    let value = json!({"openapi":"3.1.2","paths":{"/tickets/{id}":{"get":{"operationId":"lookup","parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"content":{"application/json":{"schema":{"type":"array","items":{"type":"string"}}}}}}}}}});
    let tools = import_openapi(&value, "source", &[]).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].input_schema["required"], json!(["id"]));
    assert_eq!(tools[0].output_schema.as_ref().unwrap()["type"], "array");
    let mut unsupported = value.clone();
    unsupported["openapi"] = json!("3.0.0");
    assert!(import_openapi(&unsupported, "source", &[]).is_err());
}
#[test]
fn static_skill_package_rejects_traversal_and_does_not_enable_scripts() {
    let p = SkillPackage {
        body: "# Workflow\n\n- Keep **Markdown**, commas,\n  indentation and `code`.".into(),
        description: "Method".into(),
        files: vec![SkillFile {
            path: "references/example.md".into(),
            content: "# Material".into(),
        }],
        requires_tools: vec![],
        source: "fixture".into(),
        scripts_disabled: false,
    };
    p.validate().unwrap();
    let mut bad = p.clone();
    bad.files[0].path = "../escape".into();
    assert!(bad.validate().is_err());
    bad.files[0].path = "scripts/install.sh".into();
    assert!(bad.validate().is_err());
    assert!(import_skill_zip(b"not a zip").is_err());
}
