//! Test-only native port doubles. They never run Git, Docker, a model or a network request.
#![allow(dead_code)]
#[path = "defaults.rs"]
mod defaults;
use defaults::*;
use devdesk_core::{Command, Engine, ports};
use devdesk_domain::*;
use serde_json::json;
use std::sync::Arc;

pub struct IdleHarness;
impl ports::Harness for IdleHarness {
    fn identity(&self) -> String {
        "fixture:native-idle-harness:v1".into()
    }
    fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        Ok(ports::SessionReceipt {
            external_session: format!("fixture-native-{}", r.session.id),
            container_id: "fixture-container".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, _: &ports::TurnDispatch) -> Result<()> {
        Ok(())
    }
    fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
        Ok(vec![])
    }
}
struct UnavailableGit;
impl ports::Git for UnavailableGit {
    fn candidate(&self, _: &GitContext, _: &str, _: &str, _: &str) -> Result<String> {
        Err(err("fixture_unavailable", "No candidate fixture supplied"))
    }
    fn advance(&self, _: &GitContext, _: &str, _: &str, _: &str, _: &str) -> Result<String> {
        Err(err("fixture_unavailable", "No advance fixture supplied"))
    }
}
struct UnavailableTests;
impl ports::Tests for UnavailableTests {}
pub fn adapters() -> ports::ExecutionAdapters {
    ports::ExecutionAdapters {
        harness: Arc::new(IdleHarness),
        git: Arc::new(UnavailableGit),
        tests: Arc::new(UnavailableTests),
        pull_requests: None,
    }
}
/// Explicit queued conversation facts for scheduler/quota tests, without the Host credential preflight.
pub fn queue_conversation(e: &Engine, project: &str) -> (Id, Id) {
    let sid = id();
    let tid = id();
    e.store
        .transact(None, &mut |s| {
            s.sessions.push(Session {
                id: sid.clone(),
                owner_id: project.into(),
                agent: "builtin-agent-sol-high-analyst:4".into(),
                harness: "fixture-runtime".into(),
                sandbox: "fixture-docker".into(),
                snapshot_id: None,
                instance: None,
                origin: Origin::External,
                project_id: Some(project.into()),
                feature_id: None,
                task_id: None,
                scope: Some(SessionScope::Project),
                lifecycle: Some(SessionLifecycle::Prepared),
                runtime_binding: Some(RuntimeBinding {
                    model_connection: None,
                    model_profile_digest: None,
                    agent_configuration: None,
                    model_parameters: Default::default(),
                    permissions: None,
                    installation: RuntimeInstallation {
                        id: "fixture-runtime".into(),
                        key: RuntimeKey {
                            provider: HarnessProvider::Codex,
                            version: "fixture".into(),
                            os: "linux".into(),
                            arch: "aarch64".into(),
                        },
                        artifact: RuntimeArtifact::DockerImage {
                            image_id: format!("sha256:{}", "a".repeat(64)),
                        },
                        source: "explicit native port fixture".into(),
                        license: "test".into(),
                    },
                    adapter_version: "fixture/v1".into(),
                    model: "fixture-model".into(),
                    tool_policy: ToolPolicy::ReadOnly,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                    work_type: WorkType::Discussion,
                    configuration_digest: "a".repeat(64),
                    credential_ref: None,
                }),
                external_session: None,
                predecessor: None,
                revision: 1,
            });
            s.turns.push(ConversationTurn {
                id: tid.clone(),
                project_id: project.into(),
                session_id: sid.clone(),
                client_message_id: id(),
                input: "Native scheduler fixture".into(),
                input_digest: "b".repeat(64),
                input_snapshot: None,
                code_view: None,
                source_revision: None,
                status: TurnStatus::Queued,
                external_message: None,
                external_turn: None,
                consumed: false,
                cancel_requested: false,
                started: None,
                finished: None,

                revision: 1,
            });
            Ok(json!(null))
        })
        .unwrap();
    (sid, tid)
}

/// Tests explicitly opt into reference configuration; product creation never calls this helper.
pub fn configure_project(e: &Engine, project: &str) {
    let s = e.store.snapshot().unwrap();
    let defaults = e.configuration_defaults(Some(project)).unwrap();
    e.execute("project.configuration.update",Command{command_id:id(),target:Some(project.into()),expected_revision:Some(s.projects.iter().find(|p|p.id==project).unwrap().revision),payload:json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":defaults["role_agent_bindings"]})}).unwrap();
}

/// Explicit fixture grants for current built-in targets, through the production v2 command.
pub fn bind_default_permissions(e: &Engine) {
    e.store.transact(None, &mut |s| {
        if s.configuration("fixture-spec:1").is_err() {
            let spec = (
                "spec",
                "fixture-spec",
                References::new(),
                json!({"schema_version":3,"body":"测试场景明确声明的输入、输出和断言；逐项核对类型、值和来源，不作为产品默认规范。","checks":[{"key":"fixture_content","name":"场景内容","instructions":"核对本测试明确声明的字段、预期和实际来源。"}]}),
            );
            let delivery = (
                "delivery_type",
                "fixture-delivery-object",
                References::from([("spec_refs".to_string(), json!(["fixture-spec:1"]))]),
                json!({"format":"json","content_schema":{"type":"object"}}),
            );
            for (kind, identity, refs, data) in [spec, delivery] {
                s.governance.push(SourceDefinition {r#type:kind.into(),identity:identity.into(),revision:1,name:identity.into(),scope:Scope::global(),refs,data}.into_record()?);
            }
        }
        Ok(json!(null))
    }).unwrap();
    let s = e.store.snapshot().unwrap();
    let mut bindings = Vec::new();
    for (kind, id, profile) in [
        (
            PermissionTargetKind::Harness,
            DEFAULT_HARNESS_ID,
            "builtin-permission-harness-full-access:1",
        ),
        (
            PermissionTargetKind::Harness,
            "builtin-harness-dsh-multi-provider:1",
            "builtin-permission-harness-full-access:1",
        ),
        (
            PermissionTargetKind::Sandbox,
            DEFAULT_PERMISSION_SANDBOX,
            "builtin-permission-sandbox-full-access:1",
        ),
    ] {
        if s.configuration(id).is_err() {
            continue;
        }
        let target = permission_target(&s, kind, id).unwrap();
        if active_permission_binding(&s, &target).is_none() {
            bindings.push(json!({"target":target,"permission_revision_id":profile,"expected_revision":0,"enforcement_contract":devdesk_adapters::permission_profiles::permission_contract(&s,&target).unwrap()}));
        }
    }
    if !bindings.is_empty() {
        e.execute(
            "permission.bindings.apply",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"bindings":bindings}),
            },
        )
        .unwrap();
    }
}
