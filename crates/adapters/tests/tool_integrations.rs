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
        Arc::new(SqliteStore::open(root).unwrap()),
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
    use std::io::{Read, Write};
    let server = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let port = server.local_addr().unwrap().port();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let done = stop.clone();
    let requests = count.clone();
    let handle = std::thread::spawn(move || {
        while !done.load(std::sync::atomic::Ordering::SeqCst) {
            match server.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = [0; 8192];
                    let mut n = 0;
                    while n < bytes.len() {
                        let size = stream.read(&mut bytes[n..]).unwrap_or(0);
                        if size == 0 {
                            break;
                        }
                        n += size;
                        if bytes[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    if n == 0 {
                        continue;
                    }
                    requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let body = r#"{"ok":true,"count":1}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
    });
    let root = tempfile::tempdir().unwrap();
    let executor = NativeTools::new(root.path()).unwrap();
    let mut i = invocation(root.path(), port);
    i.tool.definition.output_schema = Some(
        json!({"type":"object","properties":{"ok":{"type":"boolean"},"count":{"type":"integer"}},"required":["ok","count"]}),
    );
    executor.start(&i).unwrap();
    let r = wait_receipt(&executor, &i);
    assert!(r.success, "{r:?}");
    assert_eq!(r.structured_content.as_ref().unwrap()["ok"], true);
    assert_eq!(r.origin, Origin::External);
    executor.start(&i).unwrap();
    assert!(wait_receipt(&executor, &i).success);
    assert_eq!(
        count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "same operation must not dispatch twice"
    );
    let restored = NativeTools::new(root.path()).unwrap();
    restored.start(&i).unwrap();
    assert!(wait_receipt(&restored, &i).success);
    let mut conflict = i.clone();
    conflict.arguments = json!({"id":"different"});
    conflict.input_hash = configuration_hash(&conflict.arguments);
    assert!(restored.start(&conflict).is_err());
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut denied = i.clone();
    denied.id = id();
    denied.tool.binding.allow_private_network = false;
    executor.start(&denied).unwrap();
    assert!(!wait_receipt(&executor, &denied).success);
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut invalid = i.clone();
    invalid.id = id();
    invalid.tool.definition.output_schema = Some(json!({"type":"array"}));
    executor.start(&invalid).unwrap();
    let result = wait_receipt(&executor, &invalid);
    assert!(!result.success);
    assert_eq!(result.error.unwrap()["code"], "output_schema");
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();
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
    let first = e
        .prepare_integration_operation("integration.discover", &c, json!({"frozen":"input"}))
        .unwrap();
    let replay = e
        .prepare_integration_operation("integration.discover", &c, json!({"different":"ignored"}))
        .unwrap();
    assert_eq!(first, replay);
    e.finish_integration_operation(
        first["id"].as_str().unwrap(),
        &Ok(json!({"catalog":"actual receipt"})),
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    let r = state.configuration(first["id"].as_str().unwrap()).unwrap();
    assert_eq!(r.data["status"], "succeeded");
    assert_eq!(r.data["input"]["frozen"], "input");
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
fn test_command() -> Command {
    Command {
        command_id: id(),
        target: None,
        expected_revision: None,
        payload: json!({}),
    }
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
    e.enqueue_tool_test(&test_command(), &i).unwrap();
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
    i.permissions.approval = ApprovalPolicy::OnRequest;
    i.tool.definition.effect = "write".into();
    e.enqueue_tool_test(&test_command(), &i).unwrap();
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
    i.permissions.approval = ApprovalPolicy::Never;
    e.enqueue_tool_test(&test_command(), &i).unwrap();
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
    e.enqueue_tool_test(&test_command(), &i).unwrap();
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
    command(&e,"capability.create",None,json!({"name":"DSH native","model_ref":model["id"],"harness_ref":"builtin-harness-dsh-multi-provider:1","tools":[DEFAULT_DSH_NATIVE_TOOL_ID],"skills":[],"data":{}})).unwrap();
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
