#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
#[path = "../../../tests/support/permissions.rs"]
mod permission_fixture;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::{Command, Engine, ports::TestRequest};
use devdesk_domain::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/schemas.rs"]
mod schemas;
use schemas::SCHEMAS;

fn definition(name: &str) -> String {
    format!("rpc.schema.json#/$defs/{name}")
}
fn round_trip<T: DeserializeOwned + Serialize>(name: &str, input: Value) -> Value {
    SCHEMAS.assert_valid(&definition(name), &input);
    let decoded: T = serde_json::from_value(input).unwrap();
    let output = serde_json::to_value(decoded).unwrap();
    SCHEMAS.assert_valid(&definition(name), &output);
    // Open serde objects may accept unknown fields. Still require the published schema to
    // describe every field that the actual serializer emits, so additions cannot go unnoticed.
    if let Some(properties) =
        SCHEMAS.documents["rpc.schema.json"]["$defs"][name]["properties"].as_object()
    {
        for field in output.as_object().unwrap().keys() {
            assert!(
                properties.contains_key(field),
                "{name}.{field} is undocumented"
            );
        }
    }
    output
}

#[test]
fn all_published_schemas_and_definitions_resolve_offline() {
    for (file, document) in &SCHEMAS.documents {
        jsonschema::meta::validate(document).unwrap_or_else(|error| panic!("{file}: {error}"));
        SCHEMAS.validator(file);
        if let Some(definitions) = document["$defs"].as_object() {
            for name in definitions.keys() {
                SCHEMAS.validator(&format!("{file}#/$defs/{name}"));
            }
        }
    }
}

#[test]
fn published_configuration_wrappers_accept_current_builtin_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let engine = Engine::new(
        store,
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    let state = engine.store.snapshot().unwrap();
    let mut covered = std::collections::BTreeSet::new();
    for record in &state.governance {
        round_trip::<ConfigurationRecord>("ConfigurationRecord", json!(record));
        let schema = match record.r#type.as_str() {
            "model" => "model.schema.json",
            "tool" => "tool.schema.json",
            "agent" => "agent-composition.schema.json",
            "role" => "role.schema.json",
            "workflow" => "workflow.schema.json",
            "harness" => {
                round_trip::<HarnessDefinition>("HarnessDefinition", record.data.clone());
                continue;
            }
            _ => continue,
        };
        let value = if record.r#type == "agent" {
            json!({"role_ref":reference_value(&record.refs,"role_ref"),"capability_ref":reference_value(&record.refs,"capability_ref")})
        } else {
            record.data.clone()
        };
        SCHEMAS.assert_valid(schema, &value);
        covered.insert(schema);
    }
    assert_eq!(covered.len(), 5);
    for definition in &state.definitions {
        if let Some(profile) = &definition.agent_profile {
            round_trip::<AgentProfile>("AgentProfile", json!(profile));
        }
    }
}

#[test]
fn execution_and_interaction_schemas_match_serde_defaults_and_nested_values() {
    let installation = json!({"id":"runtime-1","key":{"provider":"codex","version":"1.0","os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"fixture","license":"test"});
    round_trip::<RuntimeInstallation>("RuntimeInstallation", installation.clone());
    round_trip::<RuntimeRegistration>(
        "RuntimeRegistration",
        json!({"key":installation["key"],"artifact":installation["artifact"],"source":"fixture","license":"test"}),
    );
    round_trip::<RuntimeBinding>(
        "RuntimeBinding",
        json!({"installation":installation,"adapter_version":"fixture/v1","model":"fixture-model","tool_policy":"read_only","work_type":"discussion","configuration_digest":"a".repeat(64)}),
    );
    round_trip::<RuntimeBinding>(
        "RuntimeBinding",
        json!({"installation":installation,"adapter_version":"fixture/v1","model":"fixture-model","model_parameters":{"reasoning_effort":"ultra"},"tool_policy":"read_only","work_type":"discussion","configuration_digest":"a".repeat(64)}),
    );
    round_trip::<AgentProfile>(
        "AgentProfile",
        json!({"role":"analyst","skill_revisions":[]}),
    );
    round_trip::<RuntimeExecution>(
        "RuntimeExecution",
        json!({"id":"execution-1","session_id":"session-1","operation_id":"operation-1","image_digest":"fixture","status":"released","writer_revoked":true}),
    );
    round_trip::<InteractionAnswer>(
        "InteractionAnswer",
        json!({"kind":"user_questions","answers":[{"id":"question-1","selected":["yes"]}],"cancelled":false}),
    );
    let answer = InteractionAnswer::UserQuestions {
        answers: vec![QuestionAnswer {
            id: "question-1".into(),
            selected: vec![],
            custom: None,
        }],
        cancelled: true,
    };
    SCHEMAS.assert_valid(&definition("InteractionAnswer"), &json!(answer));
    for intent in [
        json!(null),
        json!("clarification"),
        json!(["opaque", 1]),
        json!({"provider":"value"}),
    ] {
        round_trip::<InteractionQuestion>(
            "InteractionQuestion",
            json!({"id":"question-1","question":"Continue?","options":[],"intent":intent}),
        );
    }
    round_trip::<AgentInstance>(
        "AgentInstance",
        json!({"agent_revision_id":"agent-1","scope":"global"}),
    );
    let workflow = json!({"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"finish","name":"Finish","stage":"work","executor":{"type":"control"}}]});
    let doc: WorkflowDefinition = serde_json::from_value(workflow.clone()).unwrap();
    doc.validate().unwrap();
    round_trip::<WorkflowDefinition>("WorkflowDefinition", workflow);
}

#[test]
fn test_request_schema_covers_frozen_code_documents_and_execution_instances() {
    let context = GitContext {
        project: "project-1".into(),
        workspace: Workspace {
            repository: "owner/repo".into(),
            path: "/workspace".into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: "task-1".into(),
            branch: "task".into(),
            worktree: "/workspace/task".into(),
        },
        target: CodeLocation {
            owner: "project-1".into(),
            branch: "main".into(),
            worktree: "/workspace/main".into(),
        },
        origin: Origin::External,
    };
    round_trip::<GitContext>("GitContext", json!(context));
    round_trip::<FrozenCode>(
        "FrozenCode",
        json!({"context":context,"revision":"a".repeat(40)}),
    );
    let recipe = TestRecipe {
        runner: TestRunner::NodeTest,
        files: vec!["tests/example.test.mjs".into()],
        image: format!("sha256:{}", "a".repeat(64)),
        timeout_seconds: 30,
    };
    let document = Document {
        id: "test-1".into(),
        feature_id: Some("feature-1".into()),
        project_id: None,
        task_id: Some("task-1".into()),
        kind: "test_plan".into(),
        title: "Tests".into(),
        body: "Run native tests".into(),
        version: 1,
        supersedes: None,
        input_refs: vec![],
        origin: Origin::Human,
        artifact_id: None,
    };
    round_trip::<Document>("Document", json!(document));
    let instance = ExecutionInstance {
        id: "instance-1".into(),
        operation_id: "operation-1".into(),
        harness: Harness {
            permission: None,
            id: "harness-1".into(),
            provider: "codex".into(),
            version: "1".into(),
            capabilities: vec![],
            origin: Origin::External,
        },
        sandbox: Sandbox {
            permission: None,
            id: "sandbox-1".into(),
            provider: "docker".into(),
            version: "1".into(),
            platform: "linux".into(),
            image: None,
            network: false,
            origin: Origin::External,
        },
        external_session: None,
        external_environment: None,
        status: Status::Running,
        started: Some(1),
        finished: None,
    };
    round_trip::<ExecutionInstance>("ExecutionInstance", json!(instance));
    let request = TestRequest {
        expect_assertion_failure: false,
        recipe: Some(recipe.clone()),
        operation_id: "operation-1".into(),
        session_id: Some("session-1".into()),

        candidate: "a".repeat(40),

        environment: "docker".into(),
        origin: Origin::External,

        code: Some(context),
        runtime: Some(instance),
    };
    let mut value = round_trip::<TestRequest>("TestRequest", json!(request));
    value.as_object_mut().unwrap().remove("operation_id");
    assert!(
        !SCHEMAS
            .validator(&definition("TestRequest"))
            .is_valid(&value)
    );
    value["operation_id"] = json!("operation-1");
    value["code"] = json!({});
    assert!(
        !SCHEMAS
            .validator(&definition("TestRequest"))
            .is_valid(&value)
    );
    round_trip::<TestRequest>(
        "TestRequest",
        json!({"operation_id":"operation-1","phase":"green","candidate":"revision","test_definitions":[],"environment":"docker","origin":"external"}),
    );
}

#[test]
fn standalone_permission_contract_matches_rpc_and_preserves_null_rejection() {
    let standalone = &SCHEMAS.documents["permission.schema.json"];
    let rpc = &SCHEMAS.documents["rpc.schema.json"]["$defs"];
    for (name, definition) in standalone["$defs"].as_object().unwrap() {
        assert_eq!(
            definition, &rpc[name],
            "duplicate Permission definition {name}"
        );
    }
    for file in std::fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/configuration/permissions/profiles"
    ))
    .unwrap()
    {
        let value: Value =
            serde_json::from_slice(&std::fs::read(file.unwrap().path()).unwrap()).unwrap();
        // Shipped profiles must declare a document shape this parser understands.
        // The set is a capability declaration, not a pinned version gate.
        assert!(
            value["data"]["schema_version"]
                .as_u64()
                .is_some_and(|v| (1..=2).contains(&v))
        );
        SCHEMAS.assert_valid("permission-profile.schema.json", &value);
        serde_json::from_value::<PermissionProfile>(value)
            .unwrap()
            .normalize()
            .unwrap();
    }
    round_trip::<SandboxIsolation>(
        "SandboxIsolation",
        json!({"read_only_root":true,"executable_tmp":true,"nested_sandbox":false,"backend":"docker","credential_boundary":"private"}),
    );
    let backend: Value =
        serde_json::from_str(include_str!("../../../permissions/backends/manifest.json")).unwrap();
    SCHEMAS.assert_valid("permission-backend.schema.json", &backend);
    let mut invalid = backend;
    invalid["backends"][1]
        .as_object_mut()
        .unwrap()
        .remove("apparmor_profile");
    assert!(
        !SCHEMAS
            .validator("permission-backend.schema.json")
            .is_valid(&invalid)
    );
}

#[test]
fn compiled_native_permissions_match_the_published_execution_contract() {
    let mut state = EngineeringState::default();
    devdesk_adapters::seed_factory_configuration(&mut state);
    for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
        {
            let permissions = permission_fixture::compile(
                provider,
                ExecutionScope {
                    work_type: WorkType::Discussion,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                },
                PermissionPolicy::full_access(),
            )
            .unwrap();
            round_trip::<ExecutionPermissions>("ExecutionPermissions", json!(permissions));
            round_trip::<PermissionEnforcementPlan>(
                "PermissionEnforcementPlan",
                json!(permissions.enforcement),
            );
            if let Some(options) = permissions.enforcement.native_options {
                round_trip::<NativePermissionOptions>("NativePermissionOptions", json!(options));
            }
        }
    }
}

#[test]
fn rpc_request_dispatch_validates_the_current_parameter_contracts() {
    let validator = SCHEMAS.validator("rpc.schema.json");
    for method in [
        "feature.create",
        "workflow.create",
        "execution.retry_cleanup",
        "permission.bindings.apply",
        "permission.source.select",
        "permission.profile.reload",
        "workflow.step.rework",
    ] {
        assert!(
            validator.is_valid(
                &json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"command_id":"command-1"}})
            ),
            "{method}"
        );
        assert!(
            !validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}})),
            "{method}"
        );
    }
    round_trip::<Command>("Command", json!({"command_id":"command-1"}));
    assert!(validator.is_valid(&json!({"jsonrpc":"2.0","id":null,"method":"project.open_github","params":{"command_id":"command-1","target":"project-1","payload":null}})));
    assert!(!validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"project.open_github","params":{"command_id":"command-1","payload":null}})));
    assert!(validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"query.workspace"})));
    assert!(validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"project.open_github","params":{"command_id":"command-1","target":"project-1"}})));
    assert!(!validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"project.open_github","params":{"command_id":"command-1"}})));
    assert!(validator.is_valid(
        &json!({"jsonrpc":"2.0","id":1,"method":"runtime.prepare","params":{"provider":"codex"}})
    ));
    assert!(!validator.is_valid(
        &json!({"jsonrpc":"2.0","id":1,"method":"runtime.prepare","params":{"provider":"sim"}})
    ));
    assert!(!validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"runtime.prepare"})));
}

#[test]
fn immutable_manifests_reject_incomplete_current_shapes() {
    let validator = SCHEMAS.validator("manifest.schema.json");
    for invalid in [
        json!({"schema_version":1}),
        json!({"schema_version":1,"origin":"external","delivery_id":"delivery-1"}),
        json!({"schema_version":1,"snapshot":null,"execution_mode":"live","merge":null,"repository_receipts":[]}),
    ] {
        assert!(!validator.is_valid(&invalid));
    }
}

#[test]
fn test_recipe_schema_rejects_the_same_unsafe_paths_as_the_domain() {
    for path in [
        "tests/example.test.mjs",
        "tests/nested/test.mjs",
        "a..b.mjs",
    ] {
        let recipe = TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec![path.into()],
            image: format!("sha256:{}", "a".repeat(64)),
            timeout_seconds: 30,
        };
        recipe.validate().unwrap();
        SCHEMAS.assert_valid(&definition("TestRecipe"), &json!(recipe));
    }
    for path in [
        "/absolute.mjs",
        "-option.mjs",
        "../escape.mjs",
        "tests/../escape.mjs",
        "tests/./example.mjs",
        "tests//example.mjs",
        "tests/",
        "tests\\example.mjs",
        "tests/line\nbreak.mjs",
    ] {
        let recipe = TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec![path.into()],
            image: format!("sha256:{}", "a".repeat(64)),
            timeout_seconds: 30,
        };
        assert!(recipe.validate().is_err(), "{path}");
        assert!(
            !SCHEMAS
                .validator(&definition("TestRecipe"))
                .is_valid(&json!(recipe)),
            "{path}"
        );
    }
}

#[test]
fn configuration_schemas_cover_stage_intent_capability_and_standard_snapshot() {
    round_trip::<DefinitionVisibility>(
        "DefinitionVisibility",
        json!({"kind":"projects","project_ids":["one","two"]}),
    );
    round_trip::<HarnessDefinition>(
        "HarnessDefinition",
        json!({"provider":"dsh","runtime_version":"0.2.2-rc.1","capabilities":["tool-calling"]}),
    );
    round_trip::<ConfiguredSpecification>(
        "ConfiguredSpecification",
        json!({"schema_version":3,"body":"# 内容规范","checks":[]}),
    );
    round_trip::<ConfiguredStandard>("ConfiguredStandard", json!({"schema_version":4}));
    let project = json!({"schema_version":4,"workflow_ref":"workflow:1","standard_ref":"standard:1","role_agent_bindings":{"product":"analyst-v2"},"permission_context_ref":"authorization:1"});
    SCHEMAS.assert_valid("project-configuration.schema.json", &project);
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    let state = e.store.snapshot().unwrap();
    for row in state.governance.iter().filter(|r| r.r#type == "workflow") {
        let id = &row.id;
        let value = state.configuration(id).unwrap().data.clone();
        SCHEMAS.assert_valid("workflow.schema.json", &value);
        let doc: WorkflowDefinition = serde_json::from_value(value).unwrap();
        assert!(doc.stages.iter().all(|s| !s.description.is_empty()));
    }
    for role in [
        "analyst",
        "architect",
        "project-manager",
        "developer",
        "tester",
        "reviewer",
    ] {
        let agent = state
            .configuration(&default_configured_agent_id(role))
            .unwrap();
        SCHEMAS.assert_valid(
            "agent-composition.schema.json",
            &json!({"role_ref":reference_value(&agent.refs,"role_ref"),"capability_ref":reference_value(&agent.refs,"capability_ref")}),
        );
        let cap = state
            .configuration(
                reference_value(&agent.refs, "capability_ref")
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        SCHEMAS.assert_valid("capability.schema.json", &json!(cap.refs));
    }
}

#[test]
fn integration_contracts_cover_current_serde_and_optional_structured_output() {
    let source = round_trip::<IntegrationDefinition>(
        "IntegrationDefinition",
        json!({"source":{"kind":"http_api","base_url":"https://example.test"}}),
    );
    SCHEMAS.assert_valid("integration.schema.json", &source);
    let pending = round_trip::<IntegrationAuth>(
        "IntegrationAuth",
        json!({"kind":"oauth_pending","account":"team","scopes":["read"]}),
    );
    assert_eq!(pending["kind"], "oauth_pending");
    let binding = round_trip::<IntegrationBinding>(
        "IntegrationBinding",
        json!({"integration_ref":"source","harness_ref":"harness","sandbox_ref":"sandbox"}),
    );
    SCHEMAS.assert_valid("integration-binding.schema.json", &binding);
    let tool = round_trip::<ExternalTool>(
        "ExternalTool",
        json!({"schema_version":2,"qualified_name":"read_item","description":"Read one scoped item","integration_ref":"source","implementation":{"kind":"http_api","method":"GET","path":"/item"},"input_schema":{"type":"object","additionalProperties":false},"effect":"read_only"}),
    );
    assert!(tool.get("output_schema").is_none());
    SCHEMAS.assert_valid("tool.schema.json", &tool);
    let package = round_trip::<SkillPackage>(
        "SkillPackage",
        json!({"body":"# Method\n\nKeep source.","description":"Review source"}),
    );
    SCHEMAS.assert_valid("skill-package.schema.json", &package);
    round_trip::<IntegrationLock>("IntegrationLock", json!({}));
    round_trip::<ToolReceipt>(
        "ToolReceipt",
        json!({"invocation_id":"call","success":false,"content":[],"remote_effect":"unknown","origin":"external"}),
    );
}

#[test]
fn business_metadata_versions_do_not_gate_structurally_valid_definitions() {
    let specification = json!({"schema_version":999,"body":"明确的内容标准","checks":[]});
    round_trip::<ConfiguredSpecification>("ConfiguredSpecification", specification.clone());
    serde_json::from_value::<ConfiguredSpecification>(specification)
        .unwrap()
        .validate()
        .unwrap();
    let standard = json!({"schema_version":999});
    round_trip::<ConfiguredStandard>("ConfiguredStandard", standard.clone());
    serde_json::from_value::<ConfiguredStandard>(standard)
        .unwrap()
        .validate()
        .unwrap();
    let workflow = json!({"schema_version":999,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"done","name":"Done","stage":"work","executor":{"type":"control"}}]});
    round_trip::<WorkflowDefinition>("WorkflowDefinition", workflow.clone());
    serde_json::from_value::<WorkflowDefinition>(workflow)
        .unwrap()
        .validate()
        .unwrap();
    SCHEMAS.assert_valid("template-manifest.schema.json", &{
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../resources/configuration/manifest.json"
        ))
        .unwrap();
        manifest["schema_version"] = json!(999);
        manifest
    });
}

#[test]
fn configuration_change_contracts_cover_sources_operations_and_precise_edges() {
    round_trip::<devdesk_core::ConfigurationChangeSet>(
        "ConfigurationChangeSet",
        json!({"operations":[{"type":"create","source":{"type":"spec","identity":"contract-test","revision":1,"name":"Contract","scope":"global","data":{"schema_version":123,"body":"Contract"}}}]}),
    );
    round_trip::<ConfigurationEdge>(
        "ConfigurationEdge",
        json!({"from":"flow:1","path":"/data/steps/0/spec_refs/0","relation":"reference","expected_type":"spec","to":"contract-test:1"}),
    );
    assert!(
        !SCHEMAS
            .validator("rpc.schema.json#/$defs/ConfigurationChangeSet")
            .is_valid(&json!({"operations":[]}))
    );
}
