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
fn time_limits_are_opt_in_and_legacy_budget_serialization_stays_exact() {
    let legacy = json!({"max_turns":8,"timeout_seconds":300,"retry_limit":3});
    assert_eq!(
        round_trip::<ExecutionBudget>("ExecutionBudget", legacy.clone()),
        legacy
    );
    let limits = json!({"version":1,"wall_seconds":3600,"human_wait_seconds":600});
    let mut timed = legacy.clone();
    timed["time_limits"] = limits.clone();
    assert_eq!(
        round_trip::<ExecutionBudget>("ExecutionBudget", timed.clone()),
        timed
    );
    round_trip::<ExecutionTimeLimits>("ExecutionTimeLimits", limits);
    assert_eq!(
        serde_json::to_value(ExecutionBudget::default()).unwrap(),
        legacy
    );
}

#[test]
fn published_configuration_wrappers_accept_current_builtin_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_fixture(dir.path()).unwrap());
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
        command: None,
        preparation: vec![],
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
fn host_lifecycle_requests_carry_instance_and_version_preconditions() {
    for (name, mut request) in [
        (
            "SetupRequest",
            json!({"command_id":"setup","instance_id":"instance","expected_revision":0,"path":"dataset"}),
        ),
        (
            "ShutdownRequest",
            json!({"command_id":"stop","instance_id":"instance","expected_revision":2}),
        ),
    ] {
        let validator = SCHEMAS.validator(&format!("host-lifecycle.schema.json#/$defs/{name}"));
        assert!(validator.is_valid(&request));
        request.as_object_mut().unwrap().remove("expected_revision");
        assert!(!validator.is_valid(&request));
        request["expected_revision"] = json!(2);
        request.as_object_mut().unwrap().remove("instance_id");
        assert!(!validator.is_valid(&request));
    }
}

#[test]
fn settings_request_requires_an_explicit_resource_version_envelope() {
    let validator = SCHEMAS.validator("rpc.schema.json#/$defs/SettingsUpdate");
    let valid = json!({"command_id":"settings-1", "preconditions":[{
        "resource":{"kind":"setting","id":"global_concurrency"},"revision":0
    }], "payload":{"global_concurrency":7}});
    assert!(validator.is_valid(&valid));
    for invalid in [
        json!({"global_concurrency":7}),
        json!({"command_id":"settings-1","payload":{"global_concurrency":7}}),
        json!({"command_id":"settings-1","preconditions":[],"payload":{"global_concurrency":0}}),
        json!({"command_id":"settings-1","preconditions":[],"payload":{"default_harness":null}}),
    ] {
        assert!(!validator.is_valid(&invalid), "{invalid}");
    }
}

#[test]
fn runtime_registration_requires_command_identity_and_explicit_absence() {
    let validator = SCHEMAS.validator("rpc.schema.json");
    let registration = json!({"key":{"provider":"codex","version":"fixture","os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"protocol fixture","license":"test"});
    let valid = json!({"jsonrpc":"2.0","id":1,"method":"runtime.register","params":{"command_id":"register","expected_revision":0,"payload":registration}});
    assert!(validator.is_valid(&valid));
    let mut missing = valid.clone();
    missing["params"]
        .as_object_mut()
        .unwrap()
        .remove("expected_revision");
    assert!(!validator.is_valid(&missing));
    let mut replacement = valid.clone();
    replacement["params"]["expected_revision"] = json!(1);
    assert!(!validator.is_valid(&replacement));
    assert!(!validator.is_valid(
        &json!({"jsonrpc":"2.0","id":1,"method":"runtime.register","params":registration})
    ));
}

#[test]
fn execution_summary_reads_are_scoped_and_driver_selection_requires_both_versions() {
    let validator = SCHEMAS.validator("rpc.schema.json");
    for (method, params) in [
        (
            "query.orchestration_summary",
            json!({"project_id":"project", "root_instance_id":"root"}),
        ),
        (
            "query.orchestration_summary",
            json!({"project_id":"project", "owner_id":"owner", "workspace_only":true,"decision_before":123}),
        ),
        (
            "query.workflow_driver_summary",
            json!({"project_id":"project", "id":"root"}),
        ),
        (
            "query.execution_workspace",
            json!({"project_id":"project","feature_id":"feature","offset":50,"task_offset":100,"limit":50,"github_only":true,"work_filter":"running"}),
        ),
        (
            "query.workflow_management_summary",
            json!({"project_id":"project","id":"root","child_offset":20}),
        ),
        (
            "query.orchestration_decision",
            json!({"project_id":"project", "id":"decision"}),
        ),
        (
            "workflow.driver.select",
            json!({"project_id":"project", "command_id":"select", "target":"root", "expected_revision":7,"payload":{"mode":"rust", "expected_driver_epoch":0}}),
        ),
    ] {
        let mut request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        assert!(validator.is_valid(&request), "{request}");
        request["params"]
            .as_object_mut()
            .unwrap()
            .remove("project_id");
        assert!(!validator.is_valid(&request), "{request}");
    }
    for params in [
        json!({"project_id":"project", "root_instance_id":"root", "owner_id":"owner"}),
        json!({"project_id":"project", "root_instance_id":"root", "decision_before":0}),
        json!({"project_id":"project", "root_instance_id":"root", "workspace_only":null}),
    ] {
        assert!(!validator.is_valid(
            &json!({"jsonrpc":"2.0","id":1,"method":"query.orchestration_summary","params":params})
        ));
    }
    for params in [
        json!({"project_id":"project", "command_id":"select", "target":"root", "payload":{"mode":"rust", "expected_driver_epoch":0}}),
        json!({"project_id":"project", "command_id":"select", "target":"root", "expected_revision":7,"payload":{"mode":"rust"}}),
    ] {
        assert!(!validator.is_valid(
            &json!({"jsonrpc":"2.0","id":1,"method":"workflow.driver.select","params":params})
        ));
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
    let automatic = ProjectTestConfiguration::default();
    automatic.validate().unwrap();
    round_trip::<ProjectTestConfiguration>("ProjectTestConfiguration", json!(automatic));
    assert!(automatic.regression.is_none());
    for path in [
        "tests/example.test.mjs",
        "tests/nested/test.mjs",
        "a..b.mjs",
    ] {
        let recipe = TestRecipe {
            command: None,
            preparation: vec![],
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
            command: None,
            preparation: vec![],
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
    let project = json!({"schema_version":4,"workflow_ref":"workflow:1","standard_ref":"standard:1","role_agent_bindings":{"product":"analyst-v2"},"harness_permission_ref":"authorization:1"});
    SCHEMAS.assert_valid("project-configuration.schema.json", &project);
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open_fixture(dir.path()).unwrap()),
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

#[test]
fn workflow_action_eligibility_has_a_published_bounded_shape() {
    let ready = devdesk_core::WorkflowActionEligibility {
        allowed: true,
        code: "ready".into(),
        reason: String::new(),
    };
    let blocked = devdesk_core::WorkflowActionEligibility {
        allowed: false,
        code: "foreach_capacity".into(),
        reason: "子流程并发已满".into(),
    };
    SCHEMAS.assert_valid(&definition("WorkflowActionEligibility"), &json!(ready));
    SCHEMAS.assert_valid(
        &definition("WorkflowStepActions"),
        &json!(devdesk_core::WorkflowStepActions {
            dispatch: blocked,
            accept: ready.clone(),
            retry: ready
        }),
    );
}

#[test]
fn trusted_execution_failure_fields_and_unknown_effects_round_trip() {
    round_trip::<ExecutionFailureDisposition>(
        "ExecutionFailureDisposition",
        json!({"kind":"technical","effect":"not_applied"}),
    );
    round_trip::<WorkflowFailure>(
        "WorkflowFailure",
        json!({"code":"fixture_transient","message":"port rejected before apply","source_ref":"receipt-id","kind":"technical","effect":"not_applied"}),
    );
    round_trip::<WorkflowFailure>(
        "WorkflowFailure",
        json!({"code":"fixture_unknown","message":"result unavailable","source_ref":"operation-id","kind":"technical","effect":"unknown"}),
    );
}

#[test]
fn execution_workspace_schema_matches_strict_pagination() {
    let validator = SCHEMAS.validator("rpc.schema.json");
    for params in [
        json!({"project_id":"project","limit":0}),
        json!({"project_id":"project","limit":101}),
        json!({"project_id":"project","feature_id":null}),
        json!({"project_id":"project","task_offset":-1}),
        json!({"project_id":"project","offset":1000001}),
        json!({"project_id":"project","github_only":null}),
        json!({"project_id":"project","work_filter":"unknown"}),
        json!({"project_id":"project","detail_pages":{}}),
    ] {
        assert!(!validator.is_valid(&json!({"jsonrpc":"2.0","id":1,"method":"query.execution_workspace","params":params})),"{params}");
        assert!(
            devdesk_core::read_models::ExecutionWorkspaceQuery::parse(&params).is_err(),
            "{params}"
        );
    }
}

#[test]
fn credential_publication_requires_original_binding_and_provider_versions() {
    let validator = SCHEMAS.validator("rpc.schema.json");
    let params = json!({"command_id":"credential-test","expected_revision":0,"provider":"dsh","model_provider":"custom-provider","provider_ref":"custom-provider:1","api_key":"dummy-test-key"});
    assert!(validator.is_valid(
        &json!({"jsonrpc":"2.0","id":1,"method":"credentials.configure","params":params})
    ));
    assert!(devdesk_core::credential_updates::parse(&params).is_ok());
    for key in [
        "command_id",
        "expected_revision",
        "model_provider",
        "provider_ref",
        "api_key",
    ] {
        let mut missing = params.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            !validator.is_valid(
                &json!({"jsonrpc":"2.0","id":1,"method":"credentials.configure","params":missing})
            ),
            "{missing}"
        );
        assert!(devdesk_core::credential_updates::parse(&missing).is_err());
    }
    for (key, value) in [
        ("expected_revision", json!(-1)),
        ("provider_ref", serde_json::Value::Null),
        ("api_key", json!("invalid key with spaces")),
        ("unexpected", json!(true)),
    ] {
        let mut invalid = params.clone();
        invalid[key] = value;
        assert!(
            !validator.is_valid(
                &json!({"jsonrpc":"2.0","id":1,"method":"credentials.configure","params":invalid})
            ),
            "{invalid}"
        );
        assert!(devdesk_core::credential_updates::parse(&invalid).is_err());
    }
}
