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
        let schema = match record.kind.as_str() {
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
        SCHEMAS.assert_valid(schema, &record.data);
        covered.insert(schema);
    }
    assert_eq!(covered.len(), 5);
    for definition in &state.definitions {
        if let Some(profile) = &definition.agent_profile {
            round_trip::<AgentProfile>("AgentProfile", json!(profile));
        }
    }
    for permission in &state.permissions {
        SCHEMAS.assert_valid("permission.schema.json", &json!(permission));
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
    round_trip::<AgentProfile>(
        "AgentProfile",
        json!({"role":"analyst","skill_revisions":[],"tool_policy":"read_only","allowed_work":["discussion"],"output_schemas":["proposal"]}),
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
        json!({"agent_revision_id":"agent-1","scope":{"kind":"global"}}),
    );
    round_trip::<WorkflowRunNode>("WorkflowRunNode", json!({"key":"red","role":"tester"}));
    let workflow = json!({"schema_version":1,"process_kind":"project_intake","stages":[{"key":"setup","name":"Setup"}],"steps":[{"key":"finish","name":"Finish","stage":"setup","kind":"join","interaction":{}}]});
    validate_workflow(&serde_json::from_value(workflow.clone()).unwrap()).unwrap();
    round_trip::<WorkflowDocument>("WorkflowDocument", workflow);
    // The 100-turn cap applies to agent.interact only; other actions may leave interaction unused.
    round_trip::<WorkflowStep>(
        "WorkflowStep",
        json!({"key":"finish","name":"Finish","stage":"setup","kind":"join","interaction":{"max_turns":101}}),
    );
    let invalid = json!({"key":"discuss","name":"Discuss","stage":"setup","kind":"agent.interact","participant_slot":"analyst","interaction":{"goal":"Clarify","output_kind":"requirement","max_turns":101}});
    assert!(
        !SCHEMAS
            .validator(&definition("WorkflowStep"))
            .is_valid(&invalid)
    );
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
        requirement_id: None,
        design_id: None,
        origin: Origin::Human,
        acceptance: vec![],
        verifies: vec![],
        artifact_id: None,
        test_recipe: Some(TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec!["tests/example.test.mjs".into()],
            image: format!("sha256:{}", "a".repeat(64)),
            timeout_seconds: 30,
        }),
    };
    round_trip::<Document>("Document", json!(document));
    let instance = ExecutionInstance {
        id: "instance-1".into(),
        run_id: "run-1".into(),
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
        operation_id: "operation-1".into(),
        session_id: Some("session-1".into()),
        phase: "green".into(),
        candidate: "a".repeat(40),
        test_definitions: vec![document.id.clone()],
        environment: "docker".into(),
        origin: Origin::External,
        definitions: vec![document],
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
    let mut root = standalone.clone();
    for key in ["$schema", "$id", "title", "$defs"] {
        root.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(root, rpc["Permission"]);
    for file in std::fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../permissions/presets"
    ))
    .unwrap()
    {
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(file.unwrap().path()).unwrap()).unwrap();
        if value["schema_version"] == 2 {
            SCHEMAS.assert_valid("permission-profile.schema.json", &value);
            serde_json::from_value::<PermissionProfile>(value)
                .unwrap()
                .normalize()
                .unwrap();
            continue;
        }
        value.as_object_mut().unwrap().remove("supersedes");
        SCHEMAS.assert_valid("permission.schema.json", &value);
        serde_json::from_value::<Permission>(value)
            .unwrap()
            .validate()
            .unwrap();
    }
    round_trip::<PermissionPatch>("PermissionPatch", json!({}));
    round_trip::<PermissionBinding>("PermissionBinding", json!({}));
    round_trip::<PermissionOverrides>("PermissionOverrides", json!({}));
    round_trip::<SandboxIsolation>(
        "SandboxIsolation",
        json!({"read_only_root":true,"executable_tmp":true,"nested_sandbox":false,"backend":"docker","credential_boundary":"private"}),
    );
    for (name, input) in [
        ("PermissionPatch", json!({"filesystem":null})),
        ("PermissionBinding", json!({"permission":null})),
        ("PermissionBinding", json!({"overrides":null})),
    ] {
        assert!(!SCHEMAS.validator(&definition(name)).is_valid(&input));
    }
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
    devdesk_core::seed_configuration(&mut state);
    for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
        for legacy in [false, true] {
            let configuration = PermissionConfiguration::defaults(legacy);
            round_trip::<PermissionConfiguration>("PermissionConfiguration", json!(configuration));
            let permissions = devdesk_adapters::permissions::resolve_execution_permissions(
                &state.permissions,
                &configuration,
                provider,
                ExecutionScope {
                    work_type: WorkType::Discussion,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                },
                None,
                None,
                None,
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
        "permission.bind",
        "merge.capture_snapshot",
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
fn immutable_manifests_accept_history_and_reject_incomplete_current_shapes() {
    let artifacts: std::collections::BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/legacy/artifacts.json"
    ))
    .unwrap();
    let mut count = 0;
    for (path, body) in artifacts {
        if path.contains("/manifests/") || path.contains("/snapshots/") {
            let value: Value = serde_json::from_str(&body).unwrap();
            SCHEMAS.assert_valid("manifest.schema.json", &value);
            if value.get("delivery_id").is_some() {
                for origin in [Origin::External, Origin::Human, Origin::Simulated] {
                    let mut current = value.clone();
                    current["origin"] = json!(origin);
                    current["report_artifact"]["origin"] = json!(origin);
                    SCHEMAS.assert_valid("manifest.schema.json", &current);
                }
            }
            count += 1;
        }
    }
    assert_eq!(count, 9);
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
fn v2_configuration_schemas_cover_stage_intent_capability_and_optional_baseline() {
    round_trip::<DefinitionVisibility>(
        "DefinitionVisibility",
        json!({"kind":"projects","project_ids":["one","two"]}),
    );
    round_trip::<CapabilityDefinition>(
        "CapabilityDefinition",
        json!({"model_ref":"model-v1","harness_ref":"harness-v1","tools":["native-v1"],"skills":["prd-v1"]}),
    );
    round_trip::<SpecificationDefinition>(
        "SpecificationDefinition",
        json!({"body":"# PRD\n\n## 验收","applicable_actions":[{"kind":"agent.interact","phase":"requirement"}],"rules":["sections"]}),
    );
    round_trip::<StandardDefinition>(
        "StandardDefinition",
        json!({"body":"# Baseline","baseline_rules":[{"selector":{"kind":"delivery.review","delivery_kind":"requirement"},"required_spec_refs":["prd-v1"],"min_matches":1}]}),
    );
    let project = json!({"schema_version":2,"workflow_ref":"workflow-v2","standard_ref":null,"role_agent_bindings":{"product":"analyst-v2"},"specification_snapshot":[],"baseline_check":null,"definition_hashes":{}});
    SCHEMAS.assert_valid("project-configuration.schema.json", &project);
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    let state = e.store.snapshot().unwrap();
    for id in [DEFAULT_FEATURE_WORKFLOW_V2_ID, DEFAULT_TASK_WORKFLOW_V2_ID] {
        let value = state.configuration(id).unwrap().data.clone();
        SCHEMAS.assert_valid("workflow.schema.json", &value);
        let doc: WorkflowDocument = serde_json::from_value(value).unwrap();
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
        SCHEMAS.assert_valid("agent-composition.schema.json", &agent.data);
        let cap = state
            .configuration(agent.data["capability_ref"].as_str().unwrap())
            .unwrap();
        SCHEMAS.assert_valid("capability.schema.json", &cap.data);
    }
}
