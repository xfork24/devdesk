#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
#[path = "../../../tests/support/permissions.rs"]
mod permission_fixture;
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
        Arc::new(SqliteStore::open_fixture(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(root).unwrap()));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
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
    assert!(command(&e,"integration.create",None,json!({"name":"Wrong scope","scope":"project:other","data":{"source":{"kind":"http_api","base_url":"https://example.test"}}})).is_err());
    let before = e.store.snapshot().unwrap().governance.len();
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
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
    let binding=command(&e,"integration.binding.create",None,json!({"name":"Account A","data":{"integration_ref":source["id"],"harness_ref":DEFAULT_HARNESS_ID,"sandbox_ref":DEFAULT_PERMISSION_SANDBOX,"allowed_origins":["https://tickets.example.test"],"allowed_tools":[tool["id"]]}})).unwrap();
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Integration scope"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    let s = e.store.snapshot().unwrap();
    let mut a = s
        .resolve_agent(&default_configured_agent_id("analyst"))
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

fn invocation(_root: &std::path::Path, port: u16) -> ToolInvocation {
    let permissions = permission_fixture::compile(
        HarnessProvider::Codex,
        ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec![],
            read_only_paths: vec![],
        },
        PermissionPolicy::full_access(),
    )
    .unwrap();
    let origin = format!("http://host.docker.internal:{port}");
    let binding = IntegrationBinding {
        integration_ref: "fixture-source".into(),
        auth: IntegrationAuth::None,
        harness_ref: DEFAULT_HARNESS_ID.into(),
        sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
        allowed_origins: vec![origin.clone()],
        allow_private_network: true,
        allowed_tools: vec!["fixture-tool".into()],
        timeout_ms: 5000,
        max_result_bytes: 65536,
    };
    let definition: ExternalTool = serde_json::from_value(api_tool("fixture-source")).unwrap();
    let arguments = json!({"id":"A-1"});
    ToolInvocation {
        id: id(),
        project_id: None,
        feature_id: None,
        task_id: None,
        agent_instance_id: None,
        session_id: None,
        turn_id: None,

        operation_id: None,
        request_id: None,
        call_id: id(),
        tool: BoundTool {
            revision_id: "fixture-tool".into(),
            content_hash: configuration_hash(&definition),
            definition,
            integration: IntegrationDefinition {
                source: IntegrationSource::HttpApi { base_url: origin },
                description: "Fixture HTTP service".into(),
                oauth: None,
            },
            integration_hash: "source-digest".into(),
            binding_ref: "fixture-binding".into(),
            binding,
            binding_hash: "binding-digest".into(),
        },
        arguments: arguments.clone(),
        permissions,
        image_id: std::env::var("DEVDESK_TOOL_TEST_IMAGE")
            .expect("set DEVDESK_TOOL_TEST_IMAGE to a pinned Codex Docker image ID"),
        worktree: String::new(),
        input_hash: configuration_hash(&arguments),
    }
}
fn wait_receipt(executor: &NativeTools, i: &ToolInvocation) -> ToolReceipt {
    use devdesk_core::ports::ToolExecutor;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
    loop {
        if let Some(value) = executor.poll(i).unwrap() {
            return value;
        }
        assert!(std::time::Instant::now() < deadline, "tool call timed out");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
#[test]
#[ignore = "requires real Docker and DEVDESK_TOOL_TEST_IMAGE; no model call"]
fn docker_http_executor_validates_real_results_and_blocks_private_network_without_grant() {
    use devdesk_core::ports::ToolExecutor;
    use std::io::{BufRead, Write};
    struct Service {
        child: std::process::Child,
        output: std::io::BufReader<std::process::ChildStdout>,
    }
    impl Service {
        fn read(&mut self) -> Value {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "HTTP fixture exited"
            );
            serde_json::from_str(&line).unwrap()
        }
        fn count(&mut self) -> u64 {
            writeln!(self.child.stdin.as_mut().unwrap(), "count").unwrap();
            self.read()["count"].as_u64().unwrap()
        }
    }
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/tool-integrations/http-lookup.mjs");
    let mut child = std::process::Command::new("node")
        .arg(fixture)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let output = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut service = Service { child, output };
    let port = service.read()["port"].as_u64().unwrap() as u16;
    let root = tempfile::tempdir().unwrap();
    let executor = NativeTools::new(root.path()).unwrap();
    let mut i = invocation(root.path(), port);
    i.tool.definition.output_schema = Some(
        json!({"type":"object","properties":{"ok":{"type":"boolean"},"count":{"type":"integer"}},"required":["ok","count"]}),
    );
    executor.start(&i).unwrap();
    let r = wait_receipt(&executor, &i);
    assert!(
        r.success,
        "{r:?}; fixture port={port}, requests={}",
        service.count()
    );
    assert_eq!(r.structured_content.as_ref().unwrap()["ok"], true);
    assert_eq!(r.origin, Origin::External);
    executor.start(&i).unwrap();
    assert!(wait_receipt(&executor, &i).success);
    assert_eq!(service.count(), 1, "same operation must not dispatch twice");
    let restored = NativeTools::new(root.path()).unwrap();
    restored.start(&i).unwrap();
    assert!(wait_receipt(&restored, &i).success);
    let mut conflict = i.clone();
    conflict.arguments = json!({"id":"different"});
    conflict.input_hash = configuration_hash(&conflict.arguments);
    assert!(restored.start(&conflict).is_err());
    assert_eq!(service.count(), 1);
    let mut denied = i.clone();
    denied.id = id();
    // A literal private address makes the negative assertion independent of
    // host.docker.internal DNS (absent on Linux without a host-gateway grant).
    let private_origin = format!("http://127.0.0.1:{port}");
    denied.tool.integration.source = IntegrationSource::HttpApi {
        base_url: private_origin.clone(),
    };
    denied.tool.binding.allowed_origins = vec![private_origin];
    denied.tool.binding.allow_private_network = false;
    executor.start(&denied).unwrap();
    let refused = wait_receipt(&executor, &denied);
    assert!(!refused.success);
    assert_eq!(refused.error.unwrap()["code"], "endpoint_denied");
    assert_eq!(service.count(), 1);
    let mut invalid = i.clone();
    invalid.id = id();
    invalid.tool.definition.output_schema = Some(json!({"type":"array"}));
    executor.start(&invalid).unwrap();
    let result = wait_receipt(&executor, &invalid);
    assert!(!result.success);
    assert_eq!(result.error.unwrap()["code"], "output_schema");
    assert_eq!(service.count(), 2);
}

#[test]
#[ignore = "requires real Docker and DEVDESK_TOOL_TEST_IMAGE; no model call"]
fn docker_mcp_both_versions_and_transports_discover_call_and_reject_drift() {
    use devdesk_core::ports::ToolExecutor;
    use std::io::BufRead;
    struct Service(std::process::Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/tool-integrations/mcp-service.mjs");
    let mut service = Service(
        std::process::Command::new("node")
            .arg(&fixture)
            .arg("--http")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    std::io::BufReader::new(service.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
        .as_u64()
        .unwrap() as u16;
    for version in ["2025-11-25", "2026-07-28"] {
        for transport in ["streamable_http", "stdio"] {
            let root = tempfile::tempdir().unwrap();
            let executor = NativeTools::new(root.path()).unwrap();
            let mut i = invocation(root.path(), port);
            i.tool.integration.source = IntegrationSource::Mcp {
                transport: transport.into(),
                url: (transport == "streamable_http")
                    .then(|| format!("http://host.docker.internal:{port}/mcp")),
                command: if transport == "stdio" {
                    vec![
                        "node".into(),
                        "--input-type=module".into(),
                        "-e".into(),
                        include_str!("../../../tests/fixtures/tool-integrations/mcp-service.mjs")
                            .into(),
                    ]
                } else {
                    vec![]
                },
                protocol_version: version.into(),
            };
            let discovery = executor
                .discover(
                    &i.tool.integration,
                    &i.tool.binding,
                    &i.image_id,
                    &i.permissions,
                )
                .unwrap();
            assert_eq!(
                discovery["tools"].as_array().unwrap().len(),
                2,
                "{transport} {version}: {discovery}"
            );
            let tool = &discovery["tools"][0];
            i.tool.definition.qualified_name = "mcp_sum".into();
            i.tool.definition.input_schema = tool["inputSchema"].clone();
            i.tool.definition.output_schema = tool.get("outputSchema").cloned();
            i.tool.definition.implementation = ToolImplementation::Mcp {
                name: "sum".into(),
                descriptor_hash: configuration_hash(tool),
            };
            i.arguments = json!({"a":7,"b":8});
            i.input_hash = configuration_hash(&i.arguments);
            executor.start(&i).unwrap();
            let result = wait_receipt(&executor, &i);
            assert!(result.success, "{transport} {version}: {result:?}");
            assert_eq!(result.structured_content.unwrap()["sum"], 15);
            let mut unselected = i.clone();
            unselected.id = id();
            unselected.tool.revision_id = "not-selected-tool".into();
            assert!(executor.start(&unselected).is_err());
            let mut drift = i.clone();
            drift.id = id();
            drift.tool.definition.implementation = ToolImplementation::Mcp {
                name: "sum".into(),
                descriptor_hash: "wrong-descriptor".into(),
            };
            executor.start(&drift).unwrap();
            let result = wait_receipt(&executor, &drift);
            assert!(!result.success);
            assert_eq!(result.error.unwrap()["code"], "mcp_descriptor_drift");
        }
    }
}

#[test]
fn schema_keyword_names_in_data_are_not_misread_as_remote_references() {
    use devdesk_core::ports::ToolExecutor;
    let root = tempfile::tempdir().unwrap();
    let executor = NativeTools::new(root.path()).unwrap();
    let schema = json!({"type":"object","properties":{"$ref":{"type":"string"}},"default":{"$ref":"https://data.example"},"examples":[{"$schema":"a value"}]});
    executor
        .validate_arguments(&schema, &json!({"$ref":"an ordinary property"}))
        .unwrap();
    assert!(
        executor
            .validate_schema(&json!({"properties":{"x":{"$ref":"https://untrusted.test"}}}))
            .is_err()
    );
}
#[test]
fn openapi_requires_explicit_alternative_auth_and_does_not_drop_and_requirements() {
    use devdesk_adapters::integrations::import_openapi_with_auth;
    let mut doc = json!({"openapi":"3.1.2","components":{"securitySchemes":{"token":{"type":"http","scheme":"bearer"},"key":{"type":"apiKey","in":"header","name":"X-API-Key"}}},"security":[{"token":[]},{"key":[]}],"paths":{"/items":{"get":{"operationId":"list_items","responses":{"200":{"description":"ok"}}}}}});
    assert!(import_openapi(&doc, "source", &[]).is_err());
    let tools = import_openapi_with_auth(&doc, "source", &[], Some("key")).unwrap();
    let auth = tools[0].required_auth.as_ref().unwrap();
    assert!(!auth.satisfied_by(&IntegrationAuth::None));
    assert!(auth.satisfied_by(&IntegrationAuth::ApiKey {
        credential_ref: "integration-secret:fixture".into(),
        header: "x-api-key".into()
    }));
    doc["security"] = json!([{"token":[],"key":[]}]);
    assert!(import_openapi_with_auth(&doc, "source", &[], Some("key")).is_err());
}
#[test]
fn durable_external_operation_replays_intent_without_running_io_again() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let c = Command {
        command_id: id(),
        target: None,
        expected_revision: None,
        payload: json!({"binding_ref":"fixture"}),
    };
    let context = devdesk_core::InvocationContext::human(e.store.project_id());
    let input =
        json!({"package_hash":devdesk_domain::configuration_hash(&c.payload),"frozen":"input"});
    let first = match e
        .prepare_integration_operation("skill.import_package", &c, input, &context)
        .unwrap()
    {
        devdesk_core::integration_operations::IntegrationOperationAdmission::Admitted(
            operation,
        ) => operation,
        _ => panic!("first intent must authorize one external attempt"),
    };
    assert!(
        matches!(e.prepare_integration_operation("skill.import_package",&c,json!({"different":"ignored"}),&context),Err(error) if error.code=="needs_reconciliation")
    );
    e.finish_integration_operation(&first, &Ok(json!({"catalog":"actual receipt"})))
        .unwrap();
    assert_eq!(
        e.replay_integration_operation("skill.import_package", &c, &context)
            .unwrap()
            .unwrap(),
        json!({"catalog":"actual receipt"})
    );
    let state = e.store.snapshot().unwrap();
    let r = state.configuration(first.id()).unwrap();
    assert_eq!(r.data["status"], "succeeded");
    assert_eq!(r.data["inputs"]["frozen"], "input");
}
#[derive(Default)]
struct MemorySecrets(std::sync::Mutex<std::collections::BTreeMap<String, String>>);
impl devdesk_adapters::integration_oauth::IntegrationSecrets for MemorySecrets {
    fn create(&self, s: &str) -> Result<String> {
        let key = format!("integration-secret:{}", id());
        self.0.lock().unwrap().insert(key.clone(), s.into());
        Ok(key)
    }
    fn read(&self, r: &str) -> Result<String> {
        self.0
            .lock()
            .unwrap()
            .get(r)
            .cloned()
            .ok_or_else(|| err("missing", "missing secret"))
    }
    fn write(&self, r: &str, s: &str) -> Result<()> {
        self.0.lock().unwrap().insert(r.into(), s.into());
        Ok(())
    }
}
#[test]
fn oauth_pkce_replay_refresh_scope_and_revocation_lifecycle() {
    use devdesk_adapters::integration_oauth::*;
    let root = tempfile::tempdir().unwrap();
    let vault = MemorySecrets::default();
    let config = OAuthConfig {
        client_id: "fixture-client".into(),
        authorization_url: "https://auth.example/authorize".into(),
        token_url: "https://auth.example/token".into(),
        revocation_url: Some("https://auth.example/revoke".into()),
        scopes: vec!["read".into()],
    };
    let flow = start(
        root.path(),
        &config,
        "binding",
        None,
        "http://127.0.0.1:4318/oauth/integrations/callback",
        "fixture account",
        vec!["read".into()],
        &vault,
    )
    .unwrap();
    assert!(
        flow.authorization_url
            .contains("code_challenge_method=S256")
    );
    assert!(
        !std::fs::read_to_string(
            root.path()
                .join("integration-oauth")
                .join(format!("{}.json", flow.id))
        )
        .unwrap()
        .contains("code_verifier")
    );
    let finished=finish(root.path(),&flow.id,"test-code",&vault,|_,form|{assert_eq!(form["grant_type"],"authorization_code");assert!(form["code_verifier"].as_str().unwrap().len()>=43);Ok(json!({"access_token":"short-lived","refresh_token":"refresh-once","token_type":"Bearer","scope":"read","expires_in":0}))}).unwrap();
    let replay = finish(root.path(), &flow.id, "test-code", &vault, |_, _| {
        panic!("authorization code must not be exchanged twice")
    })
    .unwrap();
    assert_eq!(replay.credential_ref, finished.credential_ref);
    assert!(
        finish(
            root.path(),
            &flow.id,
            "different-code",
            &vault,
            |_, _| panic!()
        )
        .is_err()
    );
    let reference = finished.credential_ref.unwrap();
    let access = access_token(
        &reference,
        &config,
        "fixture account",
        &["read".into()],
        &vault,
        |_, form| {
            assert_eq!(form["refresh_token"], "refresh-once");
            Ok(json!({"access_token":"renewed","expires_in":3600,"scope":"read"}))
        },
    )
    .unwrap();
    assert_eq!(access, "renewed");
    assert!(
        access_token(
            &reference,
            &config,
            "other account",
            &["read".into()],
            &vault,
            |_, _| panic!()
        )
        .is_err()
    );
    assert!(
        access_token(
            &reference,
            &config,
            "fixture account",
            &["write".into()],
            &vault,
            |_, _| panic!()
        )
        .is_err()
    );
    let result = revoke(&reference, &config, &vault, |_, form| {
        assert_eq!(form["token"], "refresh-once");
        Ok(json!({}))
    })
    .unwrap();
    assert_eq!(result["local_revoked"], true);
    assert!(
        access_token(
            &reference,
            &config,
            "fixture account",
            &["read".into()],
            &vault,
            |_, _| panic!()
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires real Docker and DEVDESK_TOOL_TEST_IMAGE; in-memory vault isolates protocol test; no model call"]
fn docker_oauth_pkce_exchange_refresh_and_revoke_use_isolated_network() {
    use devdesk_adapters::integration_oauth::*;
    use std::io::BufRead;
    struct Service(std::process::Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/tool-integrations/oauth-service.mjs");
    let mut service = Service(
        std::process::Command::new("node")
            .arg(fixture)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    std::io::BufReader::new(service.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
        .as_u64()
        .unwrap() as u16;
    let root = tempfile::tempdir().unwrap();
    let i = invocation(root.path(), port);
    let executor = NativeTools::new(root.path()).unwrap();
    let vault = MemorySecrets::default();
    let config = OAuthConfig {
        client_id: "fixture".into(),
        authorization_url: format!("http://127.0.0.1:{port}/authorize"),
        token_url: format!("http://host.docker.internal:{port}/token"),
        revocation_url: Some(format!("http://host.docker.internal:{port}/revoke")),
        scopes: vec!["read".into()],
    };
    let flow = start(
        root.path(),
        &config,
        "binding",
        None,
        "http://127.0.0.1:4318/oauth/integrations/callback",
        "fixture",
        vec!["read".into()],
        &vault,
    )
    .unwrap();
    let exchange = |url: &str, form: Value| {
        executor.oauth_request(url, form, &i.tool.binding, &i.image_id, &i.permissions)
    };
    let finished = finish(
        root.path(),
        &flow.id,
        "code-from-authorize",
        &vault,
        exchange,
    )
    .unwrap();
    let reference = finished.credential_ref.unwrap();
    assert_eq!(
        access_token(
            &reference,
            &config,
            "fixture",
            &["read".into()],
            &vault,
            exchange
        )
        .unwrap(),
        "renewed-token"
    );
    assert_eq!(
        revoke(&reference, &config, &vault, exchange).unwrap()["remote_revocation"],
        "revoked"
    );
    assert!(
        access_token(
            &reference,
            &config,
            "fixture",
            &["read".into()],
            &vault,
            exchange
        )
        .is_err()
    );
    let logs = std::fs::read_dir(root.path().join("tool-executions")).unwrap();
    for entry in logs {
        let directory = entry.unwrap().path();
        assert!(
            !directory.join("receipt.json").exists(),
            "OAuth payload must not become an ordinary tool receipt"
        );
    }
}

struct ObservedExecutor {
    native: NativeTools,
    starts: std::sync::atomic::AtomicUsize,
    receipts: std::sync::Mutex<std::collections::BTreeMap<String, ToolReceipt>>,
}
impl devdesk_core::ports::ToolExecutor for ObservedExecutor {
    fn validate_schema(&self, s: &Value) -> Result<()> {
        devdesk_core::ports::ToolExecutor::validate_schema(&self.native, s)
    }
    fn validate_arguments(&self, s: &Value, v: &Value) -> Result<()> {
        devdesk_core::ports::ToolExecutor::validate_arguments(&self.native, s, v)
    }
    fn start(&self, _: &ToolInvocation) -> Result<()> {
        self.starts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn poll(&self, i: &ToolInvocation) -> Result<Option<ToolReceipt>> {
        Ok(self.receipts.lock().unwrap().get(&i.id).cloned())
    }
    fn cancel(&self, _: &ToolInvocation) -> Result<()> {
        Ok(())
    }
    fn discover(
        &self,
        _: &IntegrationDefinition,
        _: &IntegrationBinding,
        _: &str,
        _: &ExecutionPermissions,
    ) -> Result<Value> {
        unreachable!()
    }
}
fn registered_invocation(root: &std::path::Path) -> (Engine, ToolInvocation) {
    let e = engine(root);
    let source=command(&e,"integration.create",None,json!({"name":"Local API","data":{"source":{"kind":"http_api","base_url":"https://example.test"}}})).unwrap();
    let tool = command(
        &e,
        "integration.tool.create",
        None,
        json!({"name":"Lookup","data":api_tool(source["id"].as_str().unwrap())}),
    )
    .unwrap();
    let binding=command(&e,"integration.binding.create",None,json!({"name":"Account","data":{"integration_ref":source["id"],"harness_ref":DEFAULT_HARNESS_ID,"sandbox_ref":DEFAULT_PERMISSION_SANDBOX,"allowed_origins":["https://example.test"],"allowed_tools":[tool["id"]]}})).unwrap();
    runtime::bind_default_permissions(&e);
    let state = e.store.snapshot().unwrap();
    let lock = resolve_permission_lock(
        &state,
        DEFAULT_HARNESS_ID,
        &permission_selection(&state).unwrap(),
    )
    .unwrap();
    let permissions = devdesk_adapters::permission_profiles::compile_permission_profiles(
        &state,
        &lock,
        Some(ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec![],
            read_only_paths: vec![],
        }),
    )
    .unwrap();
    let arguments = json!({"id":"A-1"});
    let i = ToolInvocation {
        id: id(),
        project_id: e.store.project_id(),
        feature_id: None,
        task_id: None,
        agent_instance_id: None,
        session_id: None,
        turn_id: None,

        operation_id: None,
        request_id: None,
        call_id: id(),
        tool: BoundTool {
            revision_id: tool["id"].as_str().unwrap().into(),
            content_hash: tool["content_hash"].as_str().unwrap().into(),
            definition: serde_json::from_value(tool["data"].clone()).unwrap(),
            integration: serde_json::from_value(source["data"].clone()).unwrap(),
            integration_hash: source["content_hash"].as_str().unwrap().into(),
            binding_ref: binding["id"].as_str().unwrap().into(),
            binding: serde_json::from_value(binding["data"].clone()).unwrap(),
            binding_hash: binding["content_hash"].as_str().unwrap().into(),
        },
        arguments: arguments.clone(),
        permissions,
        image_id: format!("sha256:{}", "a".repeat(64)),
        worktree: String::new(),
        input_hash: configuration_hash(&arguments),
    };
    (e, i)
}
fn test_command(invocation: &ToolInvocation) -> Command {
    Command {
        command_id: id(),
        target: None,
        expected_revision: Some(0),
        payload: json!({"binding_ref":invocation.tool.binding_ref,"tool_ref":invocation.tool.revision_id,"arguments":invocation.arguments}),
    }
}
// A test-only admitted native observation, independent from the human RPC API.
fn seed_provider_call(engine: &Engine, invocation: &ToolInvocation) {
    engine.store.seed_fixture_state(None,&mut |state|{
        let data=json!({"status":"prepared","remote_effect":"not_applied","invocation":invocation,"created_at":1,"cancel_requested":false,"reply_sent":false});
        state.governance.push(ConfigurationRecord{id:invocation.id.clone(),r#type:"tool_call".into(),definition_id:invocation.id.clone(),scope:invocation.project_id.as_ref().map_or_else(Scope::global,Scope::project),name:invocation.tool.definition.qualified_name.clone(),version:1,supersedes:None,content_hash:content_digest(&References::new(),&data),refs:References::new(),data,immutable:false,revision:1,created_at:1});
        Ok(Value::Null)
    }).unwrap();
}
fn select_write_tool(engine: &Engine, invocation: &mut ToolInvocation) {
    let mut definition = json!(invocation.tool.definition);
    definition["qualified_name"] = json!("ticket_write");
    definition["effect"] = json!("write");
    let tool = command(
        engine,
        "integration.tool.create",
        None,
        json!({"name":"Explicit write fixture","data":definition}),
    )
    .unwrap();
    let mut binding = invocation.tool.binding.clone();
    binding.allowed_tools = vec![tool["id"].as_str().unwrap().into()];
    let binding = command(
        engine,
        "integration.binding.create",
        None,
        json!({"name":"Write fixture account","data":binding}),
    )
    .unwrap();
    invocation.tool.revision_id = tool["id"].as_str().unwrap().into();
    invocation.tool.content_hash = tool["content_hash"].as_str().unwrap().into();
    invocation.tool.definition = serde_json::from_value(tool["data"].clone()).unwrap();
    invocation.tool.binding_ref = binding["id"].as_str().unwrap().into();
    invocation.tool.binding_hash = binding["content_hash"].as_str().unwrap().into();
    invocation.tool.binding = serde_json::from_value(binding["data"].clone()).unwrap();
}
fn select_tool_approval(
    engine: &Engine,
    invocation: &mut ToolInvocation,
    approval: ApprovalPolicy,
) {
    let state = engine.store.snapshot().unwrap();
    let mut policy = state
        .configuration(
            &invocation
                .permissions
                .binding_lock
                .as_ref()
                .unwrap()
                .harness
                .profile
                .id,
        )
        .unwrap()
        .data["policy"]
        .clone();
    policy["approval"] = json!(approval);
    // The existing harness target is shared. Author its shared Profile as
    // an explicit offline catalog fixture; a project command cannot publish
    // a new global definition or bind a project-only Profile to this target.
    let source = SourceDefinition {
        r#type: "permission_profile".into(),
        identity: format!("fixture-test-approval-{}", id()),
        revision: 1,
        name: "Test call approval".into(),
        scope: state
            .configuration(&invocation.tool.binding.harness_ref)
            .unwrap()
            .scope
            .clone(),
        refs: References::new(),
        data: json!({"schema_version":2,"target_kind":"harness","policy":policy}),
    };
    let profile = json!(source);
    engine
        .store
        .seed_fixture_state(None, &mut |s| {
            s.governance.push(source.clone().into_record()?);
            Ok(Value::Null)
        })
        .unwrap();
    let target = permission_target(
        &state,
        PermissionTargetKind::Harness,
        &invocation.tool.binding.harness_ref,
    )
    .unwrap();
    let revision = active_permission_binding(&state, &target).map_or(0, |r| r.version);
    let profile = source_reference(
        profile["identity"].as_str().unwrap(),
        profile["revision"].as_u64().unwrap(),
    )
    .unwrap();
    command(engine,"permission.bindings.apply",None,json!({"bindings":[{"target":target,"permission_revision_id":profile,"expected_revision":revision,"enforcement_contract":devdesk_adapters::permission_profiles::permission_contract(&state,&target).unwrap()}]})).unwrap();
    let state = engine.store.snapshot().unwrap();
    let lock = resolve_permission_lock(
        &state,
        &invocation.tool.binding.harness_ref,
        &ExecutionPermissionSelection {
            sandbox_ref: invocation.tool.binding.sandbox_ref.clone(),
            harness: PermissionSelection::FollowTarget,
            sandbox: PermissionSelection::FollowTarget,
        },
    )
    .unwrap();
    invocation.permissions = devdesk_adapters::permission_profiles::compile_permission_profiles(
        &state,
        &lock,
        Some(invocation.permissions.scope.clone()),
    )
    .unwrap();
}
fn observed(root: &std::path::Path) -> (Arc<Engine>, ToolInvocation, Arc<ObservedExecutor>) {
    let (engine, invocation) = registered_invocation(root);
    let executor = Arc::new(ObservedExecutor {
        native: NativeTools::new(root).unwrap(),
        starts: Default::default(),
        receipts: Default::default(),
    });
    let engine = Arc::new(engine.with_tool_executor(executor.clone()));
    engine
        .enqueue_tool_test(
            &test_command(&invocation),
            &invocation,
            &InvocationContext::human(engine.store.project_id()),
        )
        .unwrap();
    engine.store.configuration_snapshot().unwrap();
    (engine, invocation, executor)
}
fn success(id: &str) -> ToolReceipt {
    ToolReceipt {
        invocation_id: id.into(),
        success: true,
        content: vec![],
        structured_content: Some(json!({"$source":"literal-remote-value","ref":"not-a-source"})),
        remote_effect: "applied".into(),
        error: None,
        origin: Origin::External,
    }
}
#[test]
fn one_hundred_parallel_dispatch_claims_grant_one_external_start_without_old_writer() {
    use devdesk_core::tool_calls::ToolCall;
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation, executor) = observed(root.path());
    let calls = (0..100)
        .map(|_| ToolCall::read(engine.store.as_ref(), &invocation.id).unwrap())
        .collect::<Vec<_>>();
    let (ready, wait) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let legacy = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    wait.recv().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = calls
        .into_iter()
        .map(|call| {
            let (engine, executor, barrier) = (engine.clone(), executor.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let grant = call.claim(engine.store.as_ref())?;
                if let Some(grant) = grant {
                    devdesk_core::ports::ToolExecutor::start(
                        executor.as_ref(),
                        grant.invocation(),
                    )?;
                }
                Ok::<_, Error>(())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    legacy.join().unwrap().unwrap();
    for result in results {
        result.unwrap();
    }
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_dispatch")
            .count(),
        1
    );
    assert_eq!(
        state.configuration(&invocation.id).unwrap().data["status"],
        "dispatched"
    );
}
#[test]
fn duplicate_receipts_commit_once_and_conflicting_callbacks_keep_all_original_facts() {
    use devdesk_core::tool_calls::ToolCall;
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation, _) = observed(root.path());
    let dispatch = ToolCall::read(engine.store.as_ref(), &invocation.id)
        .unwrap()
        .claim(engine.store.as_ref())
        .unwrap()
        .unwrap();
    let receipt = success(&invocation.id);
    let calls = (0..100)
        .map(|_| ToolCall::read(engine.store.as_ref(), &invocation.id).unwrap())
        .collect::<Vec<_>>();
    let before = engine.store.event_cursor().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = calls
        .into_iter()
        .map(|call| {
            let (engine, receipt, barrier) = (engine.clone(), receipt.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                call.observe(engine.store.as_ref(), &receipt)
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(engine.store.event_cursor().unwrap(), before + 2);
    let mut different = receipt.clone();
    different.structured_content = Some(json!({"other":"original-observation"}));
    assert_eq!(
        dispatch
            .observe(engine.store.as_ref(), &different)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_observation")
            .count(),
        2
    );
    assert_eq!(
        state.configuration(&invocation.id).unwrap().data["receipt"],
        json!(receipt)
    );
}
#[test]
fn original_callback_cannot_overwrite_later_cancel_or_human_reconciliation() {
    use devdesk_core::tool_calls::ToolCall;
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation, executor) = observed(root.path());
    let dispatch = ToolCall::read(engine.store.as_ref(), &invocation.id)
        .unwrap()
        .claim(engine.store.as_ref())
        .unwrap()
        .unwrap();
    let receipt = success(&invocation.id);
    command(&engine, "tool.call.cancel", Some(&invocation.id), json!({})).unwrap();
    assert_eq!(
        dispatch
            .observe(engine.store.as_ref(), &receipt)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    engine.drive_tool_calls().unwrap();
    let stale = ToolCall::read(engine.store.as_ref(), &invocation.id).unwrap();
    command(
        &engine,
        "tool.call.reconcile",
        Some(&invocation.id),
        json!({"remote_effect":"applied","evidence":"operator observation"}),
    )
    .unwrap();
    assert_eq!(
        stale
            .observe(engine.store.as_ref(), &receipt)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    executor
        .receipts
        .lock()
        .unwrap()
        .insert(invocation.id.clone(), receipt);
    engine.drive_tool_calls().unwrap();
    let state = engine.store.snapshot().unwrap();
    let row = state.configuration(&invocation.id).unwrap();
    assert_eq!(row.data["status"], "failed");
    assert_eq!(row.data["receipt"]["origin"], "human");
    assert_eq!(row.data["cancel_requested"], true);
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
}
#[test]
fn native_tool_controls_require_original_versions_and_replay_the_exact_result() {
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation, _) = observed(root.path());
    let mut request = Command {
        command_id: id(),
        target: Some(invocation.id.clone()),
        expected_revision: None,
        payload: json!({}),
    };
    assert_eq!(
        engine
            .execute("tool.call.cancel", request.clone())
            .unwrap_err()
            .code,
        "precondition_required"
    );
    request.expected_revision = Some(1);
    let first = engine.execute("tool.call.cancel", request.clone()).unwrap();
    assert_eq!(
        engine.execute("tool.call.cancel", request.clone()).unwrap(),
        first
    );
    request.command_id = id();
    assert_eq!(
        engine
            .execute("tool.call.cancel", request)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
}
#[test]
fn drivable_tool_pages_seek_an_index_and_skip_unrestorable_settled_history() {
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation, _) = observed(root.path());
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('settled-unreadable','tool_call','fixture','\"global\"','History',1,'fixture','{}',?1,0,1,1)",[json!({"status":"succeeded","reply_sent":true,"invocation":{"$source":"missing-historical-source"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let mut query=conn.prepare("EXPLAIN QUERY PLAN SELECT id FROM governance_records INDEXED BY idx_tool_call_drivable WHERE type='tool_call' AND id>?1 AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR (json_extract(data,'$.status') IN ('succeeded','failed','cancelled') AND coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL)) ORDER BY id LIMIT 17").unwrap();
    let plan = query
        .query_map([""], |r| r.get::<_, String>(3))
        .unwrap()
        .map(|r| r.unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plan.contains("idx_tool_call_drivable") && !plan.contains("TEMP B-TREE"),
        "{plan}"
    );
    let trace = devdesk_adapters::performance::Trace::start();
    assert_eq!(
        engine.store.tool_call_page(None).unwrap().ids,
        vec![invocation.id]
    );
    engine.drive_tool_calls().unwrap();
    let report = trace.finish();
    for stage in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
    ] {
        assert!(report["spans"][stage].is_null(), "{report}");
    }
}
struct UncertainReply(std::sync::atomic::AtomicUsize);
impl ports::Harness for UncertainReply {
    fn answer_tool_call(&self, _: &Session, _: &Value, _: &ToolReceipt) -> Result<()> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(err(
            "needs_reconciliation",
            "fixture acknowledgement lost after delivery",
        ))
    }
}
#[test]
fn uncertain_native_replies_are_journaled_and_never_automatically_resent() {
    use devdesk_core::tool_calls::ToolCall;
    let root = tempfile::tempdir().unwrap();
    let (mut engine, mut invocation) = registered_invocation(root.path());
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Native reply fixture"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (session, turn) = runtime::queue_conversation(&engine, &project);
    engine.tick_conversations(4).unwrap();
    let state = engine.store.snapshot().unwrap();
    let worker = state
        .executions
        .iter()
        .find(|e| e.session_id == session && !e.writer_revoked)
        .unwrap();
    invocation.project_id = Some(project);
    invocation.session_id = Some(session);
    invocation.turn_id = Some(turn);
    invocation.operation_id = Some(worker.operation_id.clone());
    invocation.request_id = Some(json!({"id":"native-tool-request"}));
    seed_provider_call(&engine, &invocation);
    let reply = Arc::new(UncertainReply(Default::default()));
    engine.harness = reply.clone();
    let dispatch = ToolCall::read(engine.store.as_ref(), &invocation.id)
        .unwrap()
        .claim(engine.store.as_ref())
        .unwrap()
        .unwrap();
    dispatch
        .observe(engine.store.as_ref(), &success(&invocation.id))
        .unwrap();
    for _ in 0..5 {
        ToolCall::read(engine.store.as_ref(), &invocation.id)
            .unwrap()
            .deliver(engine.store.as_ref(), reply.as_ref())
            .unwrap();
    }
    assert_eq!(reply.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    let state = engine.store.snapshot().unwrap();
    let row = state.configuration(&invocation.id).unwrap();
    assert_eq!(row.data["reply_status"], "needs_reconciliation");
    assert_eq!(row.data["reply_sent"], false);
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_reply_intent")
            .count(),
        1
    );
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.r#type == "tool_observation"
                && r.name == "reply"
                && r.data["value"]["error"]["code"] == "needs_reconciliation")
    );
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    invocation.id = id();
    seed_provider_call(&engine, &invocation);
    let before_revocation = ToolCall::read(engine.store.as_ref(), &invocation.id).unwrap();
    conn.execute(
        "UPDATE executions SET writer_revoked=1 WHERE operation_id=?1",
        [&worker.operation_id],
    )
    .unwrap();
    assert!(
        before_revocation
            .claim(engine.store.as_ref())
            .unwrap()
            .is_none()
    );
    assert!(
        ToolCall::read(engine.store.as_ref(), &invocation.id)
            .unwrap()
            .claim(engine.store.as_ref())
            .unwrap()
            .is_none()
    );
    let stopped = engine
        .store
        .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
            "governance",
            &invocation.id,
        )])
        .unwrap()
        .remove(0)
        .value
        .unwrap();
    assert_eq!(stopped["data"]["status"], "cancelled");
    assert_eq!(stopped["data"]["remote_effect"], "not_applied");
    assert!(
        conn.execute(
            "UPDATE executions SET writer_revoked=0 WHERE operation_id=?1",
            [&worker.operation_id]
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "DELETE FROM executions WHERE operation_id=?1",
            [&worker.operation_id]
        )
        .is_err()
    );
}
#[test]
fn workflow_tool_dispatch_respects_ancestor_pause_and_original_execution_fences() {
    use devdesk_core::{
        concurrency::ResourceKey,
        tool_calls::{ToolCall, WorkflowToolAuthority},
    };
    let root = tempfile::tempdir().unwrap();
    let (engine, mut invocation, _) = observed(root.path());
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Frozen tool authority"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    invocation.id = id();
    invocation.project_id = Some(project.clone());
    invocation.operation_id = Some("child-execution".into());
    seed_provider_call(&engine, &invocation);
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    for (id, execution, paused, parent) in [
        ("child", "child-execution", false, Some("parent-execution")),
        ("root", "parent-execution", true, None),
    ] {
        let data = json!({"project_id":project,"root_instance_id":"root","status":"running","paused":paused,"cancel_requested":false,"parent_execution_id":parent,"steps":[{"id":execution,"status":"running","plan_revision_id":"plan","node_id":"node","input_digest":"input","contract_digest":"contract"}]});
        conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,'workflow_instance',?2,?3,'Unrestored workflow',1,'fixture','{}',?4,0,1,1)",rusqlite::params![id,project,serde_json::to_string(&Scope::project(&project)).unwrap(),data.to_string()]).unwrap();
    }
    let authority = WorkflowToolAuthority {
        instances: vec!["child".into(), "root".into()],
        executions: vec!["child-execution".into(), "parent-execution".into()],
        execution_id: "child-execution".into(),
        plan_revision_id: "plan".into(),
        node_id: "node".into(),
        input_digest: "input".into(),
        contract_digest: "contract".into(),
        session_id: None,
    };
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.workflow_authority',json(?1)),revision=revision+1 WHERE id=?2",rusqlite::params![serde_json::to_string(&authority).unwrap(),invocation.id]).unwrap();
    assert!(
        ToolCall::read(engine.store.as_ref(), &invocation.id)
            .unwrap()
            .claim(engine.store.as_ref())
            .unwrap()
            .is_none()
    );
    let saved = engine
        .store
        .read_resources(&[ResourceKey::new("governance", &invocation.id)])
        .unwrap()
        .remove(0)
        .value
        .unwrap();
    assert_eq!(saved["data"]["status"], "prepared");
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.paused',json('false')),revision=2 WHERE id='root'",[]).unwrap();
    let original = ToolCall::read(engine.store.as_ref(), &invocation.id).unwrap();
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.steps[0].id','replacement-execution'),revision=3 WHERE id='root'",[]).unwrap();
    assert!(original.claim(engine.store.as_ref()).unwrap().is_none());
    assert!(
        ToolCall::read(engine.store.as_ref(), &invocation.id)
            .unwrap()
            .claim(engine.store.as_ref())
            .unwrap()
            .is_none()
    );
    let saved = engine
        .store
        .read_resources(&[ResourceKey::new("governance", &invocation.id)])
        .unwrap()
        .remove(0)
        .value
        .unwrap();
    assert_eq!(saved["data"]["status"], "cancelled");
    assert_eq!(
        saved["data"]["local_pre_dispatch_stop"]["operation_id"],
        "child-execution"
    );
}
fn configured_tool_fixture(
    rust_driver: bool,
) -> (
    tempfile::TempDir,
    Engine,
    ToolInvocation,
    Command,
    PreparedConfiguredTool,
) {
    let root = tempfile::tempdir().unwrap();
    let (engine, mut invocation) = registered_invocation(root.path());
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Configured external tool"}),
    )
    .unwrap();
    let project = project["id"].as_str().unwrap();
    command(&engine,"integration.project.bind",None,json!({"project_id":project,"bindings":{invocation.tool.binding.integration_ref.clone():invocation.tool.binding_ref.clone()}})).unwrap();
    let workflow=command(&engine,"workflow.create",None,json!({"name":"External lookup","document":{"schema_version":6,"stages":[{"key":"work","name":"Lookup"}],"steps":[{"key":"lookup","name":"Lookup","stage":"work","executor":{"type":"tool","tool_ref":invocation.tool.revision_id,"arguments":{"id":{"type":"constant","value":"A-1"}}}}]}})).unwrap();
    let workflow = command(
        &engine,
        "workflow.publish",
        workflow["id"].as_str(),
        json!({}),
    )
    .unwrap();
    let run = command(
        &engine,
        "workflow.start",
        Some(project),
        json!({"workflow_ref":workflow["id"],"role_agent_bindings":{},"orchestration":{"mode":if rust_driver {"rust"}else{"manual"}}}),
    )
    .unwrap();
    let baseline = engine
        .store
        .snapshot()
        .unwrap()
        .configuration(run["id"].as_str().unwrap())
        .unwrap()
        .clone();
    if rust_driver {
        engine.drive_rust_workflows().unwrap();
    } else {
        command(
        &engine,
        "workflow.step.dispatch",
        run["id"].as_str(),
        json!({"step":"lookup","plan_revision_id":baseline.data["active_plan_revision_id"],"control_version":baseline.data["control_version"]}),
    )
    .unwrap();
    }
    let state = engine.store.snapshot().unwrap();
    let intent = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_intent" && r.data["method"] == "workflow.tool.execute")
        .unwrap();
    let original: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    let prepared = engine.prepare_configured_tool(&original).unwrap();
    let request = prepared.request().clone();
    invocation.id = id();
    invocation.operation_id = Some(request.execution_id.clone());
    invocation.project_id = Some(project.into());
    invocation.tool = request.tool;
    invocation.permissions = devdesk_adapters::permission_profiles::compile_permission_profiles(
        &state,
        &request.permissions,
        Some(ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec![],
            read_only_paths: vec![],
        }),
    )
    .unwrap();
    (root, engine, invocation, original, prepared)
}
#[test]
fn configured_tool_enqueue_preserves_the_actual_frozen_execution_authority() {
    use devdesk_core::tool_calls::ToolCall;
    let (_root, engine, invocation, original, prepared) = configured_tool_fixture(false);
    let request = prepared.request();
    let admitted = engine
        .enqueue_configured_tool(&original, &invocation)
        .unwrap();
    assert_eq!(
        admitted["data"]["workflow_authority"],
        json!(request.authority)
    );
    assert!(
        ToolCall::read(engine.store.as_ref(), &invocation.id)
            .unwrap()
            .claim(engine.store.as_ref())
            .unwrap()
            .is_some()
    );
}
#[test]
fn workflow_tool_creation_preserves_actual_driver_actor_and_one_hundred_replays() {
    let (root, engine, invocation, original, prepared) = configured_tool_fixture(true);
    let engine = Arc::new(engine);
    let prepared = Arc::new(prepared);
    let actor = json!(prepared.request().provenance.context)["actor"].clone();
    assert_eq!(actor["kind"], "driver");
    engine.store.configuration_snapshot().unwrap();
    let (ready, wait) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    wait.recv().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, invocation, original, prepared, barrier) = (
                engine.clone(),
                invocation.clone(),
                original.clone(),
                prepared.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                engine.enqueue_prepared_configured_tool(&original, &invocation, &prepared)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let first = results[0].as_ref().unwrap();
    for result in &results {
        assert_eq!(result.as_ref().unwrap(), first);
    }
    let context = first["data"]["origin_context"].as_str().unwrap();
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(context).unwrap().data["context"]["actor"],
        actor
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_call")
            .count(),
        1
    );
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable-history','workflow_instance','fixture',?1,'History',1,'fixture','{}',?2,0,1,1)",rusqlite::params![serde_json::to_string(&Scope::project(invocation.project_id.as_ref().unwrap())).unwrap(),json!({"root_instance_id":"unrelated-history","plan":{"$source":"absent-history"}}).to_string()]).unwrap();
    let trace = devdesk_adapters::performance::Trace::start();
    assert_eq!(
        engine
            .enqueue_configured_tool(&original, &invocation)
            .unwrap(),
        *first
    );
    let report = trace.finish();
    assert!(
        report["spans"]["project.full_snapshot"].is_null(),
        "{report}"
    );
}
fn workflow_control(engine: &Engine, method: &str, instance: &str) {
    let row = engine
        .store
        .snapshot()
        .unwrap()
        .configuration(instance)
        .unwrap()
        .clone();
    command(engine,method,Some(instance),json!({"plan_revision_id":row.data["active_plan_revision_id"],"control_version":row.data["control_version"]})).unwrap();
}
#[test]
fn prepared_workflow_tools_defer_pause_and_reject_cancellation_or_rebound_requests() {
    let (_root, engine, invocation, original, prepared) = configured_tool_fixture(false);
    let instance = &prepared.request().authority.instances[0];
    let mut changed = original.clone();
    changed.payload["execution_id"] = json!("other-execution");
    assert!(
        engine
            .enqueue_prepared_configured_tool(&changed, &invocation, &prepared)
            .is_err()
    );
    workflow_control(&engine, "workflow.pause", instance);
    let deferred = engine
        .enqueue_prepared_configured_tool(&original, &invocation, &prepared)
        .unwrap_err();
    assert_eq!(deferred.code, "workflow_tool_suspended");
    engine
        .observe_workflow_intent(
            &engine
                .prepare_workflow_intent(&prepared.request().provenance.intent.id)
                .unwrap(),
            &Err(deferred),
        )
        .unwrap();
    assert_eq!(
        engine
            .store
            .snapshot()
            .unwrap()
            .configuration(&prepared.request().provenance.intent.id)
            .unwrap()
            .data["status"],
        "prepared"
    );
    workflow_control(&engine, "workflow.resume", instance);
    assert!(
        engine
            .enqueue_prepared_configured_tool(&original, &invocation, &prepared)
            .is_ok()
    );
    let (_other, engine, invocation, original, prepared) = configured_tool_fixture(false);
    workflow_control(
        &engine,
        "workflow.cancel",
        &prepared.request().authority.instances[0],
    );
    assert_eq!(
        engine
            .enqueue_prepared_configured_tool(&original, &invocation, &prepared)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    assert!(
        !engine
            .store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.id == invocation.id)
    );
}
#[test]
fn prepared_workflow_tool_authority_cannot_be_used_in_another_project_namespace() {
    let (_root, _engine, invocation, original, prepared) = configured_tool_fixture(false);
    let other = tempfile::tempdir().unwrap();
    devdesk_adapters::GlobalStore::open(other.path()).unwrap();
    let store = Arc::new(SqliteStore::open_project(other.path(), &id()).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    assert_eq!(
        engine
            .enqueue_prepared_configured_tool(&original, &invocation, &prepared)
            .unwrap_err()
            .code,
        "invalid_scope"
    );
    assert_eq!(store.event_cursor().unwrap(), 0);
}
#[test]
fn manual_tool_creation_replays_original_packed_result_and_does_not_wait_for_old_writer() {
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation) = registered_invocation(root.path());
    let engine = Arc::new(engine);
    let original = test_command(&invocation);
    let context = InvocationContext::human(engine.store.project_id());
    engine.store.configuration_snapshot().unwrap();
    let (ready, wait) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    wait.recv().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let trace = devdesk_adapters::performance::Trace::start();
    let threads = (0..100)
        .map(|_| {
            let (engine, invocation, original, context, barrier) = (
                engine.clone(),
                invocation.clone(),
                original.clone(),
                context.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                engine.enqueue_tool_test(&original, &invocation, &context)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let report = trace.finish();
    let first = results[0].as_ref().unwrap();
    for result in &results {
        assert_eq!(result.as_ref().unwrap(), first);
    }
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    let packed: String = conn
        .query_row(
            "SELECT result FROM operations WHERE id=?1",
            [&original.command_id],
            |r| r.get(0),
        )
        .unwrap();
    let pointer: Value = serde_json::from_str(&packed).unwrap();
    let stored = &pointer["$devdesk_project_payload_v1"];
    let hash = stored["sha256"].as_str().unwrap();
    let bytes = std::fs::read(
        root.path()
            .join("payloads/objects")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(hash),
    )
    .unwrap();
    assert_eq!(Some(bytes.len() as u64), stored["bytes"].as_u64());
    let packed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        packed["data"]["invocation"]["tool"]["$source"],
        "bound_tool"
    );
    assert_eq!(
        packed["data"]["invocation"]["permissions"]["$source"],
        "execution_permissions"
    );
    assert!(packed["data"]["invocation"]["tool"]["definition"].is_null());
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM governance_records WHERE type='tool_call'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    let observed = Arc::new(ObservedExecutor {
        native: NativeTools::new(root.path()).unwrap(),
        starts: Default::default(),
        receipts: Default::default(),
    });
    // The stored creation result survives later dispatch and source-head changes.
    let dispatch = devdesk_core::tool_calls::ToolCall::read(engine.store.as_ref(), &invocation.id)
        .unwrap()
        .claim(engine.store.as_ref())
        .unwrap()
        .unwrap();
    devdesk_core::ports::ToolExecutor::start(observed.as_ref(), dispatch.invocation()).unwrap();
    command(
        &engine,
        "integration.revise",
        Some(&invocation.tool.definition.integration_ref),
        json!({"name":"Newer service presentation","data":invocation.tool.integration}),
    )
    .unwrap();
    assert_eq!(
        engine
            .replay_tool_test(&original, &context)
            .unwrap()
            .unwrap(),
        *first
    );
    let mut changed = original;
    changed.payload["arguments"] = json!({"id":"different"});
    assert!(engine.replay_tool_test(&changed, &context).is_err());
    assert!(
        report["spans"]["project.full_snapshot"].is_null(),
        "{report}"
    );
}
#[test]
fn manual_tool_creation_rejects_missing_absence_original_source_or_permission_drift() {
    let root = tempfile::tempdir().unwrap();
    let (engine, mut invocation) = registered_invocation(root.path());
    let context = InvocationContext::human(engine.store.project_id());
    let mut original = test_command(&invocation);
    original.expected_revision = None;
    assert_eq!(
        engine
            .enqueue_tool_test(&original, &invocation, &context)
            .unwrap_err()
            .code,
        "precondition_required"
    );
    original.expected_revision = Some(0);
    let original_invocation = invocation.clone();
    invocation.tool.definition.effect = "write".into();
    assert!(
        engine
            .enqueue_tool_test(&original, &invocation, &context)
            .is_err()
    );
    invocation = original_invocation.clone();
    select_tool_approval(&engine, &mut invocation, ApprovalPolicy::OnRequest);
    assert_eq!(
        engine
            .enqueue_tool_test(
                &test_command(&original_invocation),
                &original_invocation,
                &context
            )
            .unwrap_err()
            .code,
        "revision_conflict"
    );
}
#[test]
fn one_hundred_independent_manual_calls_share_read_only_configuration_guards() {
    let root = tempfile::tempdir().unwrap();
    let (engine, invocation) = registered_invocation(root.path());
    let engine = Arc::new(engine);
    engine.store.configuration_snapshot().unwrap();
    let clock = engine.store.catalog_revision().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, mut invocation, barrier) =
                (engine.clone(), invocation.clone(), barrier.clone());
            invocation.id = id();
            std::thread::spawn(move || {
                let request = test_command(&invocation);
                barrier.wait();
                let trace = devdesk_adapters::performance::Trace::start();
                let result = engine.enqueue_tool_test(
                    &request,
                    &invocation,
                    &InvocationContext::human(engine.store.project_id()),
                );
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    let failed = results
        .iter()
        .filter_map(|(r, _)| r.as_ref().err())
        .collect::<Vec<_>>();
    for stage in [
        "resource.sql_transaction",
        "resource.commit",
        "sqlite.pool_wait",
        "catalog.build",
        "resource.busy_retry",
    ] {
        let mut values = results
            .iter()
            .map(|(_, report)| report["spans"][stage]["total_ms"].as_f64().unwrap_or(0.0))
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        eprintln!(
            "{stage}: p50={} p95={} max={}",
            values[49], values[94], values[99]
        );
    }
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(engine.store.catalog_revision().unwrap(), clock);
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM governance_records WHERE type='tool_call'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        100
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM governance_records WHERE type='command_context' AND name='integration.test'",[],|r|r.get::<_,u64>(0)).unwrap(),100);
}
#[test]
fn cancel_approval_restart_and_human_reconciliation_do_not_resend_unknown_calls() {
    let root = tempfile::tempdir().unwrap();
    let (e, mut i) = registered_invocation(root.path());
    let observed = Arc::new(ObservedExecutor {
        native: NativeTools::new(root.path()).unwrap(),
        starts: Default::default(),
        receipts: Default::default(),
    });
    let e = e.with_tool_executor(observed.clone());
    e.enqueue_tool_test(
        &test_command(&i),
        &i,
        &InvocationContext::human(e.store.project_id()),
    )
    .unwrap();
    command(&e, "tool.call.cancel", Some(&i.id), json!({})).unwrap();
    e.drive_tool_calls().unwrap();
    assert_eq!(observed.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&i.id)
            .unwrap()
            .data["remote_effect"],
        "not_applied"
    );
    i.id = id();
    select_tool_approval(&e, &mut i, ApprovalPolicy::OnRequest);
    select_write_tool(&e, &mut i);
    e.enqueue_tool_test(
        &test_command(&i),
        &i,
        &InvocationContext::human(e.store.project_id()),
    )
    .unwrap();
    e.drive_tool_calls().unwrap();
    assert_eq!(observed.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    command(
        &e,
        "tool.call.approve",
        Some(&i.id),
        json!({"approve":false}),
    )
    .unwrap();
    e.drive_tool_calls().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&i.id)
            .unwrap()
            .data["status"],
        "cancelled"
    );
    i.id = id();
    select_tool_approval(&e, &mut i, ApprovalPolicy::Never);
    e.enqueue_tool_test(
        &test_command(&i),
        &i,
        &InvocationContext::human(e.store.project_id()),
    )
    .unwrap();
    e.drive_tool_calls().unwrap();
    assert_eq!(observed.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e.drive_tool_calls().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&i.id)
            .unwrap()
            .data["status"],
        "needs_reconciliation"
    );
    assert_eq!(observed.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        e.store.has_pending_work().unwrap(),
        "late receipts must remain eligible for scheduler reconciliation"
    );
    observed.receipts.lock().unwrap().insert(
        i.id.clone(),
        ToolReceipt {
            invocation_id: i.id.clone(),
            success: true,
            content: vec![],
            structured_content: None,
            remote_effect: "applied".into(),
            error: None,
            origin: Origin::External,
        },
    );
    // A receipt may arrive after dispatch/poll raced or after a restart.
    // Normal scheduling must reconcile it without starting the write again.
    e.drive_tool_calls().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&i.id)
            .unwrap()
            .data["status"],
        "succeeded"
    );
    assert_eq!(observed.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        !e.store.has_pending_work().unwrap(),
        "settled manual calls return the project to idle"
    );
    i.id = id();
    e.enqueue_tool_test(
        &test_command(&i),
        &i,
        &InvocationContext::human(e.store.project_id()),
    )
    .unwrap();
    e.drive_tool_calls().unwrap();
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    command(&e,"tool.call.reconcile",Some(&i.id),json!({"remote_effect":"applied","evidence":"Server transaction fixture-2 confirmed by operator"})).unwrap();
    let state = e.store.snapshot().unwrap();
    let reconciled = state.configuration(&i.id).unwrap();
    assert_eq!(reconciled.data["receipt"]["origin"], "human");
    assert_eq!(reconciled.data["receipt"]["success"], false);
}

#[test]
#[ignore = "requires real Docker and DEVDESK_TOOL_TEST_IMAGE; no model call"]
fn docker_http_side_effect_timeout_recovery_artifact_and_redaction_are_real() {
    use devdesk_core::ports::ToolExecutor;
    use std::io::BufRead;
    struct Service(std::process::Child);
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/tool-integrations/http-service.mjs");
    let mut service = Service(
        std::process::Command::new("node")
            .arg(fixture)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut reader = std::io::BufReader::new(service.0.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
        .as_u64()
        .unwrap() as u16;
    let root = tempfile::tempdir().unwrap();
    let executor = NativeTools::new(root.path()).unwrap();
    let mut i = invocation(root.path(), port);
    i.arguments = json!({});
    i.input_hash = configuration_hash(&i.arguments);
    i.tool.definition.input_schema = json!({"type":"object","additionalProperties":false});
    i.tool.definition.effect = "write".into();
    i.tool.definition.implementation = ToolImplementation::HttpApi {
        method: "POST".into(),
        path: "/write".into(),
        parameters: vec![],
        body_pointer: None,
        idempotency_header: Some("Idempotency-Key".into()),
    };
    executor.start(&i).unwrap();
    let receipt = wait_receipt(&executor, &i);
    assert!(receipt.success);
    assert_eq!(receipt.remote_effect, "applied");
    assert_eq!(receipt.structured_content.unwrap()["writes"], 1);
    let mut slow = i.clone();
    slow.id = id();
    slow.tool.binding.timeout_ms = 200;
    slow.tool.definition.implementation = ToolImplementation::HttpApi {
        method: "POST".into(),
        path: "/slow".into(),
        parameters: vec![],
        body_pointer: None,
        idempotency_header: None,
    };
    executor.start(&slow).unwrap();
    let receipt = wait_receipt(&executor, &slow);
    assert!(!receipt.success);
    assert_eq!(receipt.remote_effect, "unknown");
    let restored = NativeTools::new(root.path()).unwrap();
    restored.start(&slow).unwrap();
    assert!(!wait_receipt(&restored, &slow).success);
    let mut stats = i.clone();
    stats.id = id();
    stats.tool.definition.effect = "read_only".into();
    stats.tool.definition.implementation = ToolImplementation::HttpApi {
        method: "GET".into(),
        path: "/stats".into(),
        parameters: vec![],
        body_pointer: None,
        idempotency_header: None,
    };
    executor.start(&stats).unwrap();
    assert_eq!(
        wait_receipt(&executor, &stats).structured_content.unwrap()["writes"],
        2,
        "an uncertain write must not be resent after executor restart"
    );
    let mut file = stats.clone();
    file.id = id();
    if let ToolImplementation::HttpApi { path, .. } = &mut file.tool.definition.implementation {
        *path = "/binary".into();
    }
    executor.start(&file).unwrap();
    let receipt = wait_receipt(&executor, &file);
    assert!(receipt.success);
    let (bytes, mime) = executor.read_artifact(&file.id, &receipt).unwrap();
    assert_eq!(bytes, vec![0, 1, 2, 3, 255]);
    assert_eq!(mime, "application/octet-stream");
    let mut forged = receipt.clone();
    forged.content[0]["sha256"] = json!("changed");
    assert!(executor.read_artifact(&file.id, &forged).is_err());
    let mut echo = stats.clone();
    echo.id = id();
    echo.arguments = json!({"secret":"sensitive-canary-47"});
    echo.input_hash = configuration_hash(&echo.arguments);
    echo.tool.definition.input_schema =
        json!({"type":"object","properties":{"secret":{"type":"string"}},"required":["secret"]});
    echo.tool.definition.sensitive_parameters = vec!["/secret".into()];
    if let ToolImplementation::HttpApi { parameters, .. } = &mut echo.tool.definition.implementation
    {
        parameters.push(HttpParameter {
            pointer: "/secret".into(),
            location: "query".into(),
            name: "secret".into(),
            required: true,
        });
    }
    executor.start(&echo).unwrap();
    let receipt = wait_receipt(&executor, &echo);
    assert!(receipt.success);
    assert!(
        !serde_json::to_string(&receipt)
            .unwrap()
            .contains("sensitive-canary-47")
    );
}

#[test]
fn deleting_source_retires_descendants_atomically_and_protects_active_references() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let source=command(&e,"integration.create",None,json!({"name":"Disposable API","data":{"source":{"kind":"http_api","base_url":"https://example.test"}}})).unwrap();
    let tool = command(
        &e,
        "integration.tool.create",
        None,
        json!({"name":"Lookup","data":api_tool(source["id"].as_str().unwrap())}),
    )
    .unwrap();
    let binding=command(&e,"integration.binding.create",None,json!({"name":"Disposable binding","data":{"integration_ref":source["id"],"harness_ref":DEFAULT_HARNESS_ID,"sandbox_ref":DEFAULT_PERMISSION_SANDBOX,"allowed_origins":["https://example.test"],"allowed_tools":[tool["id"]]}})).unwrap();
    let cap=command(&e,"capability.create",None,json!({"name":"In-use tool","model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"tools":[DEFAULT_NATIVE_TOOL_ID,tool["id"]],"skills":[],"data":{}})).unwrap();
    let error = command(&e, "integration.delete", source["id"].as_str(), json!({})).unwrap_err();
    assert!(error.message.contains("In-use tool"));
    let before = e.store.snapshot().unwrap();
    assert!(!configuration_retired(
        &before,
        before
            .configuration(source["id"].as_str().unwrap())
            .unwrap()
    ));
    command(
        &e,
        "configuration.retire",
        cap["id"].as_str(),
        json!({"reason":"test cleanup"}),
    )
    .unwrap();
    command(&e, "integration.delete", source["id"].as_str(), json!({})).unwrap();
    let after = e.store.snapshot().unwrap();
    for row in [source, tool, binding] {
        let current = after.configuration(row["id"].as_str().unwrap()).unwrap();
        assert_eq!(json!(current), row);
        assert!(configuration_retired(&after, current));
    }
    assert!(
        e.query("query.integrations", &json!({}), 1).unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn dsh_native_group_is_available_and_rejects_cross_harness_composition() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let model = command(
        &e,
        "model.create",
        None,
        json!({"name":"DSH model","data":{"provider":"deepseek","model":"deepseek-chat"}}),
    )
    .unwrap();
    command(&e,"capability.create",None,json!({"name":"DSH native","model_ref":model["id"],"harness_ref":"builtin-harness-dsh-multi-provider:2","tools":[DEFAULT_DSH_NATIVE_TOOL_ID],"skills":[],"data":{}})).unwrap();
    assert!(command(&e,"capability.create",None,json!({"name":"Wrong harness","model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"tools":[DEFAULT_DSH_NATIVE_TOOL_ID],"skills":[],"data":{}})).unwrap_err().message.contains("原生工具组不适用"));
}

#[test]
fn deleting_an_old_source_protects_pinned_users_and_preserves_tools_moved_to_another_source() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path());
    let make = |name: &str| {
        command(&e,"integration.create",None,json!({"name":name,"data":{"source":{"kind":"http_api","base_url":"https://example.test"}}})).unwrap()
    };
    let a = make("Original source");
    let b = make("Current source");
    let first = command(
        &e,
        "integration.tool.create",
        None,
        json!({"name":"Moved lookup","data":api_tool(a["id"].as_str().unwrap())}),
    )
    .unwrap();
    let second = command(
        &e,
        "integration.tool.revise",
        first["id"].as_str(),
        json!({"name":"Moved lookup","data":api_tool(b["id"].as_str().unwrap())}),
    )
    .unwrap();
    let cap=command(&e,"capability.create",None,json!({"name":"Pins original source","model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"tools":[DEFAULT_NATIVE_TOOL_ID,first["id"]],"skills":[],"data":{}})).unwrap();
    assert!(
        command(&e, "integration.delete", a["id"].as_str(), json!({}))
            .unwrap_err()
            .message
            .contains("Pins original source")
    );
    command(
        &e,
        "configuration.retire",
        cap["id"].as_str(),
        json!({"reason":"replace pinned user"}),
    )
    .unwrap();
    command(&e, "integration.delete", a["id"].as_str(), json!({})).unwrap();
    let state = e.store.snapshot().unwrap();
    assert!(!configuration_retired(
        &state,
        state.configuration(second["id"].as_str().unwrap()).unwrap()
    ));
    assert!(!configuration_retired(
        &state,
        state.configuration(b["id"].as_str().unwrap()).unwrap()
    ));
}
#[test]
fn schema_validation_rejects_recursive_expansion_and_backtracking_patterns() {
    use devdesk_core::ports::ToolExecutor;
    let dir = tempfile::tempdir().unwrap();
    let tools = devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap();
    assert!(tools.validate_schema(&json!({"$ref":"#"})).is_err());
    assert!(
        tools
            .validate_schema(&json!({"type":"string","pattern":"(a)\\1"}))
            .is_err()
    );
    let bounded = json!({"$defs":{"item":{"type":"string","pattern":"^[a-z]+$"}},"type":"object","properties":{"name":{"$ref":"#/$defs/item"}}});
    tools
        .validate_arguments(&bounded, &json!({"name":"valid"}))
        .unwrap();
    assert!(
        tools
            .validate_arguments(&bounded, &json!({"name":"INVALID"}))
            .is_err()
    );
}

#[test]
fn workflow_intent_callbacks_replay_once_without_old_writer_or_history_and_keep_original_receipts()
{
    let (root, engine, invocation, command, prepared) = configured_tool_fixture(false);
    let original = Arc::new(
        engine
            .prepare_workflow_intent(&prepared.request().provenance.intent.id)
            .unwrap(),
    );
    let result = Arc::new(Ok(engine
        .enqueue_prepared_configured_tool(&command, &invocation, &prepared)
        .unwrap()));
    let engine = Arc::new(engine);
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('bad-intent-history','workflow_instance','fixture',?1,'History',1,'fixture','{}',?2,0,1,1)",rusqlite::params![serde_json::to_string(&original.scope).unwrap(),json!({"root_instance_id":"unrelated-history","plan":{"$source":"absent-history"}}).to_string()]).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, original, result, barrier) = (
                engine.clone(),
                original.clone(),
                result.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                let trace = devdesk_adapters::performance::Trace::start();
                let result = engine.observe_workflow_intent(&original, &result);
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    for (result, trace) in results {
        result.unwrap();
        for span in [
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(trace["spans"][span].is_null(), "{trace}");
        }
    }
    let count: u64 = conn
        .query_row(
            "SELECT count(*) FROM governance_records WHERE type='workflow_intent_observation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let row = engine
        .store
        .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
            "governance",
            &original.id,
        )])
        .unwrap()
        .remove(0);
    let row = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(row.revision, 2);
    assert_eq!(row.data["status"], "completed");
    assert_eq!(row.data["result"], json!({"id":invocation.id}));
    let raw: String = conn
        .query_row(
            "SELECT data FROM governance_records WHERE type='workflow_intent_observation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!raw.contains("compiled_permissions"));
    assert!(!raw.contains("qualified_name"));
}
#[test]
fn workflow_intent_callbacks_preserve_conflicts_and_stale_controller_saves_cannot_overwrite_them() {
    let (_root, engine, _invocation, _command, prepared) = configured_tool_fixture(false);
    let original = engine
        .prepare_workflow_intent(&prepared.request().provenance.intent.id)
        .unwrap();
    let engine = Arc::new(engine);
    let intent_id = original.id.clone();
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |s| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            let row = s.governance.iter_mut().find(|r| r.id == intent_id).unwrap();
            row.data["status"] = json!("revoked");
            row.content_hash = content_digest(&row.refs, &row.data);
            row.revision += 1;
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    engine
        .observe_workflow_intent(
            &original,
            &Err(err("original-port-failure", "before admission").technical_not_applied()),
        )
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(
        writer.join().unwrap().unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(
        engine
            .observe_workflow_intent(
                &original,
                &Err(err("late-different-failure", "different callback"))
            )
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    let row = engine
        .store
        .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
            "governance",
            &original.id,
        )])
        .unwrap()
        .remove(0);
    let row = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(row.data["status"], "failed");
    assert_eq!(row.data["error"]["code"], "original-port-failure");
    let (_root, other, _, _, other_prepared) = configured_tool_fixture(false);
    let other_original = other
        .prepare_workflow_intent(&other_prepared.request().provenance.intent.id)
        .unwrap();
    other
        .store
        .seed_fixture_state(None, &mut |s| {
            let row = s
                .governance
                .iter_mut()
                .find(|r| r.id == other_original.id)
                .unwrap();
            row.data["status"] = json!("revoked");
            row.content_hash = content_digest(&row.refs, &row.data);
            row.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(
        other
            .observe_workflow_intent(&other_original, &Err(err("late", "after revocation")))
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    let before = other.store.event_cursor().unwrap();
    assert_eq!(
        other
            .observe_workflow_intent(&original, &Err(err("scope", "wrong dataset")))
            .unwrap_err()
            .code,
        "invalid_scope"
    );
    assert_eq!(other.store.event_cursor().unwrap(), before);
}
#[test]
fn an_unconfirmed_workflow_response_uses_its_durable_original_command_receipt() {
    let (_root, engine, invocation, command, prepared) = configured_tool_fixture(false);
    let original = engine
        .prepare_workflow_intent(&prepared.request().provenance.intent.id)
        .unwrap();
    engine
        .enqueue_prepared_configured_tool(&command, &invocation, &prepared)
        .unwrap();
    engine
        .observe_workflow_intent(
            &original,
            &Err(err("unconfirmed", "response lost after command commit").technical_unknown()),
        )
        .unwrap();
    let row = engine
        .store
        .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
            "governance",
            &original.id,
        )])
        .unwrap()
        .remove(0);
    let row = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(row.data["status"], "completed");
    assert_eq!(row.data["result"]["id"], invocation.id);
}

#[test]
fn native_pending_intent_scan_allows_one_hundred_reads_without_old_writer_or_completed_history() {
    let (root, engine, _, command, prepared) = configured_tool_fixture(false);
    let intent_id = prepared.request().provenance.intent.id.clone();
    let original = engine.prepare_workflow_intent(&intent_id).unwrap();
    let engine = Arc::new(engine);
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('bad-scan-history','workflow_instance','fixture',?1,'History',1,'fixture','{}',?2,0,1,1)",rusqlite::params![serde_json::to_string(&original.scope).unwrap(),json!({"root_instance_id":"unrelated-history","plan":{"$source":"absent-history"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    conn.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<2000) INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) SELECT 'settled-intent-'||x,'workflow_intent','fixture',?1,'Settled',1,'fixture','{}',?2,0,1,1 FROM n",rusqlite::params![serde_json::to_string(&original.scope).unwrap(),json!({"status":"completed","plan":{"$source":"absent-history"}}).to_string()]).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, barrier) = (engine.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let trace = devdesk_adapters::performance::Trace::start();
                let result = engine.pending_workflow_intents().map(|rows| {
                    rows.into_iter()
                        .map(|r| r.data["command"].clone())
                        .collect::<Vec<_>>()
                });
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    for (result, trace) in results {
        assert_eq!(result.unwrap(), vec![json!(command)]);
        for name in ["project.full_snapshot", "project.transaction"] {
            assert!(trace["spans"][name].is_null(), "{trace}");
        }
    }
    assert!(engine.workflow_intent_drivable(&original).unwrap());
    let plan:String=conn.query_row("EXPLAIN QUERY PLAN SELECT id FROM governance_records INDEXED BY idx_workflow_intent_prepared WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared' AND id>?1 ORDER BY id LIMIT 17",[""],|r|r.get(3)).unwrap();
    assert!(plan.contains("idx_workflow_intent_prepared"), "{plan}");
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
}
#[test]
fn pending_intent_pages_advance_past_invalidated_steps_and_recheck_pause_and_cancellation() {
    let (root, engine, _, command, prepared) = configured_tool_fixture(false);
    let intent_id = prepared.request().provenance.intent.id.clone();
    let instance = &prepared.request().authority.instances[0];
    let original = engine.prepare_workflow_intent(&intent_id).unwrap();
    workflow_control(&engine, "workflow.pause", instance);
    assert!(!engine.workflow_intent_drivable(&original).unwrap());
    assert!(engine.pending_workflow_intents().unwrap().is_empty());
    workflow_control(&engine, "workflow.resume", instance);
    assert!(engine.workflow_intent_drivable(&original).unwrap());
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<32) INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) SELECT printf('000-obsolete-intent-%02d',x),type,definition_id,scope,name,version,content_hash,refs,json_set(data,'$.execution_authority.node_id','obsolete-node','$.command.command_id','obsolete-'||x),immutable,revision,created_at FROM governance_records,n WHERE id=?1",[&intent_id]).unwrap();
    let first = engine.pending_workflow_intents().unwrap();
    assert!(first.is_empty());
    let second = engine.pending_workflow_intents().unwrap();
    assert!(second.is_empty());
    let third = engine.pending_workflow_intents().unwrap();
    assert_eq!(third.len(), 1);
    assert_eq!(third[0].data["command"], json!(command));
    workflow_control(&engine, "workflow.cancel", instance);
    assert!(!engine.workflow_intent_drivable(&original).unwrap());
}

#[derive(Default)]
struct NativeToolCaller {
    turn: std::sync::Mutex<Option<Id>>,
    emitted: std::sync::atomic::AtomicBool,
    arguments: std::sync::Mutex<Value>,
    replies: std::sync::Mutex<Vec<ToolReceipt>>,
    poll_gate:
        std::sync::Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}
impl devdesk_core::ports::Harness for NativeToolCaller {
    fn open_session(
        &self,
        r: &devdesk_core::ports::SessionStart,
    ) -> Result<devdesk_core::ports::SessionReceipt> {
        Ok(devdesk_core::ports::SessionReceipt {
            external_session: format!("native-thread:{}", r.session.id),
            container_id: "explicit-protocol-fixture".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &devdesk_core::ports::TurnDispatch) -> Result<()> {
        *self.turn.lock().unwrap() = Some(r.turn.id.clone());
        Ok(())
    }
    fn poll_session(
        &self,
        session: &Session,
    ) -> Result<Vec<devdesk_core::ports::ConversationObservation>> {
        use devdesk_core::ports::ConversationObservation as O;
        if self.emitted.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Ok(vec![]);
        }
        if let Some((ready, release)) = self.poll_gate.lock().unwrap().take() {
            ready.send(()).unwrap();
            release.recv().unwrap();
        }
        let turn = self.turn.lock().unwrap().clone().unwrap();
        Ok(vec![
            O::Admitted {
                turn: turn.clone(),
                message: Some("native-message-1".into()),
                external_turn: Some("native-turn-1".into()),
            },
            O::Consumed {
                turn: turn.clone(),
                message: Some("native-message-1".into()),
            },
            O::ToolCall {
                turn,
                request: json!({"request_id":"native-request-1"}),
                thread: session.external_session.clone().unwrap(),
                external_turn: "native-turn-1".into(),
                call: "native-call-1".into(),
                tool: "ticket_lookup".into(),
                arguments: self.arguments.lock().unwrap().clone(),
                worktree: String::new(),
            },
        ])
    }
    fn answer_tool_call(&self, _: &Session, _: &Value, receipt: &ToolReceipt) -> Result<()> {
        self.replies.lock().unwrap().push(receipt.clone());
        Ok(())
    }
}
fn native_provider_fixture(
    root: &std::path::Path,
    arguments: Value,
) -> (Engine, Arc<NativeToolCaller>, Arc<ObservedExecutor>, Id, Id) {
    let (mut engine, invocation) = registered_invocation(root);
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Native provider admission"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    command(&engine,"integration.project.bind",None,json!({"project_id":pid,"bindings":{invocation.tool.binding.integration_ref.clone():invocation.tool.binding_ref.clone()}})).unwrap();
    let capability=command(&engine,"capability.create",None,json!({"name":"Native external caller","model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"tools":[DEFAULT_NATIVE_TOOL_ID,invocation.tool.revision_id],"skills":[],"data":{}})).unwrap();
    let role=command(&engine,"role.create",None,json!({"name":"Native provider fixture","data":{"instructions":"Call only the exact frozen integration."}})).unwrap();
    let agent=command(&engine,"agent.create",None,json!({"name":"Native provider caller","role_ref":role["id"],"capability_ref":capability["id"],"data":{}})).unwrap();
    let caller = Arc::new(NativeToolCaller::default());
    *caller.arguments.lock().unwrap() = arguments;
    engine.harness = caller.clone();
    let executor = Arc::new(ObservedExecutor {
        native: NativeTools::new(root).unwrap(),
        starts: Default::default(),
        receipts: Default::default(),
    });
    engine.tool_executor = Some(executor.clone());
    let instance=command(&engine,"agent.instance.create",None,json!({"name":"Native tool fixture actor","scope":Scope::project(pid),"agent_revision_id":agent["id"]})).unwrap();
    let agent_id = agent["id"].as_str().unwrap();
    let (sid, tid) =
        runtime::queue_conversation_for_agent(&engine, pid, agent_id, |state, binding| {
            use devdesk_core::ports::SessionResources;
            let a = state.resolve_agent(agent_id)?;
            let record = state.configuration(agent_id)?;
            let model: ModelDefinition = state.configuration(&a.model_ref)?.decode()?;
            binding.installation = runtime::ResourceFixture {
                store: engine.store.as_ref(),
            }
            .prepare_harness(
                state.configuration(&a.harness_ref)?,
                state.configuration(&invocation.tool.binding.sandbox_ref)?,
            )?;
            binding.model = model.model.clone();
            binding.model_parameters.clear();
            binding.agent_configuration = Some(AgentExecutionConfiguration {
                context_protocol: Some(AGENT_CONTEXT_PROTOCOL.into()),
                integration_lock: Some(freeze_discussion_integrations(state, &a, Some(pid))?),
                agent_instance_id: instance["id"].as_str().unwrap().into(),
                agent_revision_id: agent_id.into(),
                composition_hash: record.content_hash.clone(),
                model_revision_id: a.model_ref,
                harness_revision_id: a.harness_ref,
                model_provider: model.provider,
                model_parameters: model.parameters,
                tool_revisions: a.tools,
            });
            runtime::set_permissions(binding, invocation.permissions.clone());
            binding.configuration_digest = devdesk_adapters::runtime_digest(
                state.definitions.iter().find(|d| d.id == agent_id).unwrap(),
                binding,
            )?;
            Ok(())
        });
    engine.tick_conversations(4).unwrap();
    (engine, caller, executor, sid, tid)
}
#[test]
fn provider_tool_creation_preserves_actual_worker_origin_and_uses_one_native_admission() {
    let root = tempfile::tempdir().unwrap();
    let (engine, caller, executor, sid, tid) =
        native_provider_fixture(root.path(), json!({"id":"A-1"}));
    engine.tick_conversations(4).unwrap();
    let state = engine.store.snapshot().unwrap();
    let call = state
        .governance
        .iter()
        .find(|r| r.r#type == "tool_call")
        .unwrap();
    assert_eq!(call.data["status"], "prepared", "{}", call.data);
    assert_eq!(call.data["invocation"]["session_id"], sid);
    assert_eq!(call.data["invocation"]["turn_id"], tid);
    let intent = state
        .governance
        .iter()
        .find(|r| r.r#type == "provider_tool_intent")
        .unwrap();
    assert_eq!(intent.data["status"], "completed");
    let origin = state
        .governance
        .iter()
        .find(|r| r.r#type == "tool_call_origin")
        .unwrap();
    assert_eq!(origin.data["context"]["kind"], "native_worker");
    assert!(!origin.data.to_string().contains("human"));
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "provider_poll_observation")
            .count(),
        1
    );
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    let original = engine.admit_provider_tool(&intent.id).unwrap();
    assert_eq!(original["id"], call.id);
    engine.drive_tool_calls().unwrap();
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    executor
        .receipts
        .lock()
        .unwrap()
        .insert(call.id.clone(), success(&call.id));
    engine.drive_tool_calls().unwrap();
    engine.drive_tool_calls().unwrap();
    assert_eq!(caller.replies.lock().unwrap().len(), 1);
}
#[test]
fn provider_schema_rejection_is_a_local_kernel_reply_without_a_supplier_receipt_or_external_start()
{
    let root = tempfile::tempdir().unwrap();
    let (engine, caller, executor, _, _) = native_provider_fixture(root.path(), json!({"id":7}));
    engine.tick_conversations(4).unwrap();
    let state = engine.store.snapshot().unwrap();
    let call = state
        .governance
        .iter()
        .find(|r| r.r#type == "tool_call")
        .unwrap();
    assert_eq!(call.data["status"], "failed");
    assert!(call.data["receipt"].is_null());
    assert!(call.data["local_rejection"].is_object());
    engine.drive_tool_calls().unwrap();
    engine.drive_tool_calls().unwrap();
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    let replies = caller.replies.lock().unwrap();
    assert_eq!(replies.len(), 1);
    assert!(!replies[0].success);
    assert_eq!(replies[0].error.as_ref().unwrap()["issuer"], "kernel");
}

struct PendingNativeProvider {
    engine: Arc<Engine>,
    executor: Arc<ObservedExecutor>,
    intent: Id,
    release: std::sync::mpsc::Sender<()>,
    tick: std::thread::JoinHandle<Result<()>>,
    _listener: Arc<dyn Fn() + Send + Sync>,
}
fn pending_native_provider(root: &std::path::Path) -> PendingNativeProvider {
    let (engine, _, executor, _, _) = native_provider_fixture(root, json!({"id":"A-1"}));
    let engine = Arc::new(engine);
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let blocked = std::sync::Mutex::new(blocked);
    let path = root.join("engineering.sqlite3");
    let paused = std::sync::atomic::AtomicBool::new(false);
    let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if paused.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let conn = rusqlite::Connection::open(&path).unwrap();
        use rusqlite::OptionalExtension;
        let id=conn.query_row("SELECT id FROM governance_records WHERE type='provider_tool_intent' AND json_extract(data,'$.status')='prepared' ORDER BY id LIMIT 1",[],|r|r.get::<_,String>(0)).optional().unwrap();
        if let Some(id) = id
            && !paused.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            ready.send(id).unwrap();
            blocked.lock().unwrap().recv().unwrap();
        }
    });
    engine.store.subscribe_changes(Arc::downgrade(&listener));
    let worker = engine.clone();
    let tick = std::thread::spawn(move || worker.tick_conversations(4));
    let intent = waiting
        .recv_timeout(std::time::Duration::from_secs(15))
        .expect("provider intent must commit before admission");
    PendingNativeProvider {
        engine,
        executor,
        intent,
        release,
        tick,
        _listener: listener,
    }
}
#[test]
fn one_hundred_provider_admissions_replay_once_without_the_old_writer() {
    let root = tempfile::tempdir().unwrap();
    let pending = pending_native_provider(root.path());
    // The complete configuration catalog is a separate allowed cold read;
    // this case measures whether admission itself restores runtime history.
    pending.engine.store.configuration_snapshot().unwrap();
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let old = pending.engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |state| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            state.events.push(Event {
                id: state.events.len() as u64 + 1,
                target: "legacy".into(),
                kind: "test.provider-interleave".into(),
                detail: "explicit fixture".into(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, intent, barrier) = (
                pending.engine.clone(),
                pending.intent.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                let trace = devdesk_adapters::performance::Trace::start();
                let result = engine.admit_provider_tool(&intent);
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    pending.release.send(()).unwrap();
    pending.tick.join().unwrap().unwrap();
    let expected = results[0].0.as_ref().unwrap().clone();
    for (result, trace) in results {
        assert_eq!(result.unwrap(), expected);
        for stage in [
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(trace["spans"][stage].is_null(), "{trace}");
        }
    }
    let state = pending.engine.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_call")
            .count(),
        1
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "tool_call_origin")
            .count(),
        1
    );
    assert!(
        state
            .events
            .iter()
            .any(|r| r.kind == "test.provider-interleave")
    );
    assert_eq!(
        pending
            .executor
            .starts
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
#[test]
fn cancellation_during_poll_preserves_the_original_observation_and_does_not_rebase_the_batch() {
    let root = tempfile::tempdir().unwrap();
    let (engine, caller, executor, _, tid) =
        native_provider_fixture(root.path(), json!({"id":"A-1"}));
    let engine = Arc::new(engine);
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    *caller.poll_gate.lock().unwrap() = Some((ready, blocked));
    let worker = engine.clone();
    let tick = std::thread::spawn(move || worker.tick_conversations(4));
    waiting
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let original = engine
        .store
        .snapshot()
        .unwrap()
        .turns
        .into_iter()
        .find(|t| t.id == tid)
        .unwrap();
    engine
        .execute(
            "conversation.cancel",
            Command {
                command_id: id(),
                target: Some(tid.clone()),
                expected_revision: Some(original.revision),
                payload: json!({}),
            },
        )
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(tick.join().unwrap().unwrap_err().code, "revision_conflict");
    let state = engine.store.snapshot().unwrap();
    let turn = state.turns.iter().find(|t| t.id == tid).unwrap();
    assert!(turn.cancel_requested);
    assert_eq!(turn.revision, original.revision + 1);
    assert_eq!(turn.status, original.status);
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "tool_call" || r.r#type == "provider_tool_intent")
    );
    let observation = state
        .governance
        .iter()
        .find(|r| r.r#type == "provider_poll_observation")
        .unwrap();
    assert_eq!(
        observation.data["value"]["observations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
}
#[test]
fn cancellation_before_native_dispatch_rejects_a_prepared_provider_call_without_a_supplier_receipt()
{
    let root = tempfile::tempdir().unwrap();
    let (engine, _, executor, _, tid) = native_provider_fixture(root.path(), json!({"id":"A-1"}));
    engine.tick_conversations(4).unwrap();
    let state = engine.store.snapshot().unwrap();
    let turn = state.turns.iter().find(|t| t.id == tid).unwrap();
    let call = state
        .governance
        .iter()
        .find(|r| r.r#type == "tool_call")
        .unwrap();
    let callid = call.id.clone();
    engine
        .execute(
            "conversation.cancel",
            Command {
                command_id: id(),
                target: Some(tid),
                expected_revision: Some(turn.revision),
                payload: json!({}),
            },
        )
        .unwrap();
    engine.drive_tool_calls().unwrap();
    let state = engine.store.snapshot().unwrap();
    let call = state.configuration(&callid).unwrap();
    assert_eq!(call.data["status"], "cancelled");
    assert!(call.data["receipt"].is_null());
    assert!(call.data["local_pre_dispatch_stop"].is_object());
    assert_eq!(executor.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
}
