//! Regression fixtures use real proposal commands and persisted workflow evaluation,
//! without dispatching a model, Git operation, or container.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/workflows.rs"]
mod workflow_fixture;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::{Command, Engine};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn engine(path: &Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, mut payload: Value) -> Value {
    if method.starts_with("workflow.") {
        for field in ["data", "document"] {
            if payload.get(field).is_some() {
                payload[field] = workflow_fixture::current(payload[field].clone());
            }
        }
    }
    if method == "project.configuration.update" && payload.get("agent_refs").is_some() {
        let mut bindings = e.configuration_defaults(target).unwrap()["role_agent_bindings"]
            .as_object()
            .unwrap()
            .clone();
        let state = e.store.snapshot().unwrap();
        for (role, agent) in payload["agent_refs"].as_object().unwrap() {
            let key = state
                .configuration(agent.as_str().unwrap())
                .ok()
                .and_then(|r| r.data["role_ref"].as_str())
                .and_then(|id| state.configuration(id).ok())
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(|| format!("builtin-role-{role}"));
            bindings.insert(key, agent.clone());
        }
        payload.as_object_mut().unwrap().remove("agent_refs");
        payload["role_agent_bindings"] = serde_json::to_value(bindings).unwrap();
    }
    let s = e.store.snapshot().unwrap();
    let expected_revision = target.and_then(|id| {
        s.projects
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.revision)
            .or_else(|| s.features.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.proposals.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.governance.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision,
            payload,
        },
    )
    .unwrap_or_else(|error| panic!("{method}: {error}"))
}
fn workflow(e: &Engine, feature: &str) -> WorkflowRuntime {
    e.store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == feature)
        .unwrap()
        .decode()
        .unwrap()
}
fn update_workflow(e: &Engine, feature: &str, edit: impl Fn(&mut WorkflowRuntime)) {
    e.store
        .transact(None, &mut |s| {
            let record = s
                .governance
                .iter_mut()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == feature)
                .unwrap();
            let mut r: WorkflowRuntime = record.decode()?;
            edit(&mut r);
            record.data = json!(r);
            record.content_hash = configuration_hash(&record.data);
            record.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
}
fn step<'a>(r: &'a WorkflowRuntime, key: &str) -> &'a WorkflowStepRuntime {
    r.steps.iter().find(|s| s.key == key).unwrap()
}
struct Scenario {
    feature: String,
    session: String,
    turn: String,
    discussion: String,
    adoption: String,
    next: String,
    content: Value,
}
impl Scenario {
    fn new(e: &Engine, phase: &str) -> Self {
        Self::with_workflow(e, phase, "builtin-workflow-feature-sol-high-v4")
    }
    fn with_workflow(e: &Engine, phase: &str, workflow_ref: &str) -> Self {
        let project = command(
            e,
            "project.create",
            None,
            json!({"title":"Reference regression"}),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        // Use the historical v1 plan so each of its four proposal kinds is tested.
        let standard = e
            .store
            .snapshot()
            .unwrap()
            .definitions
            .iter()
            .find(|d| d.family == "standard")
            .unwrap()
            .id
            .clone();
        let agents: serde_json::Map<String, Value> =
            ["analyst", "architect", "developer", "tester", "reviewer"]
                .into_iter()
                .map(|role| (role.into(), json!(default_configured_agent_id(role))))
                .collect();
        let schema = e
            .store
            .snapshot()
            .unwrap()
            .configuration(workflow_ref)
            .unwrap()
            .data["schema_version"]
            .as_u64()
            .unwrap();
        let config = if schema == 1 {
            json!({"workflow_ref":workflow_ref,"standard_ref":standard,"agent_refs":agents})
        } else {
            json!({"workflow_ref":workflow_ref,"role_agent_bindings":e.configuration_defaults(Some(&project)).unwrap()["role_agent_bindings"]})
        };
        command(e, "project.configuration.update", Some(&project), config);
        let feature = command(
            e,
            "feature.create",
            None,
            json!({"project_id":project,"title":"References"}),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let req = command(e, "document.revise", Some(&feature), json!({"kind":"requirement","title":"Requirement","body":"Counter","acceptance":[{"key":"AC-1","description":"Count clicks"}]}))["id"].as_str().unwrap().to_owned();
        let design = command(
            e,
            "document.revise",
            Some(&feature),
            json!({"kind":"design","title":"Design","body":"Local state"}),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (discussion, adoption, next, content) = match phase {
            "requirement" => (
                "requirement_discussion",
                "requirement_adopt",
                "requirement_execute",
                json!({"kind":"requirement","title":"Counter","body":"Count clicks","acceptance":[{"key":"AC-1","description":"Count clicks"}]}),
            ),
            "design" => (
                "design_discussion",
                "design_adopt",
                "task_discussion",
                json!({"kind":"design","title":"Counter design","body":"Local state","requirement":req}),
            ),
            "task_plan" => (
                "task_discussion",
                "task_adopt",
                "test_discussion",
                json!({"kind":"task_plan","requirement":req,"design":design,"tasks":[{"key":"counter","title":"Implement counter","acceptance":["AC-1"],"dependencies":[]}]}),
            ),
            "test_plan" => (
                "test_discussion",
                "test_adopt",
                "solution_execute",
                json!({"kind":"test_plan","title":"Counter tests","body":"Verify clicks","requirement":req,"design":design,"verifies":["AC-1"],"test_recipe":{"runner":"node_test","image":format!("sha256:{}", "a".repeat(64)),"files":["tests/counter.test.mjs"],"timeout_seconds":30}}),
            ),
            _ => panic!("unknown phase"),
        };
        let (template_session, template_turn) = runtime::queue_conversation(e, &project);
        let session = id();
        let turn = id();
        e.store
            .transact(None, &mut |s| {
                let template = s.turns.iter_mut().find(|v| v.id == template_turn).unwrap();
                template.status = TurnStatus::Cancelled;
                template.finished = Some(1);
                let mut row = s
                    .sessions
                    .iter()
                    .find(|v| v.id == template_session)
                    .unwrap()
                    .clone();
                row.id = session.clone();
                row.owner_id = feature.clone();
                row.feature_id = Some(feature.clone());
                row.scope = Some(SessionScope::Feature);
                row.external_session = Some(format!("fixture-{session}"));
                s.sessions.push(row);
                let mut row = s
                    .turns
                    .iter()
                    .find(|v| v.id == template_turn)
                    .unwrap()
                    .clone();
                row.id = turn.clone();
                row.session_id = session.clone();
                row.client_message_id = id();
                row.status = TurnStatus::Completed;
                row.consumed = true;
                row.finished = Some(1);
                s.turns.push(row);
                Ok(Value::Null)
            })
            .unwrap();
        update_workflow(e, &feature, |r| {
            for state in &mut r.steps {
                if state.key == discussion {
                    state.status = "waiting_input".into();
                    state.outputs.insert("session".into(), session.clone());
                    state.outputs.insert("turn".into(), turn.clone());
                    break;
                }
                state.status = "completed".into();
            }
        });
        Self {
            feature,
            session,
            turn,
            discussion: discussion.into(),
            adoption: adoption.into(),
            next: next.into(),
            content,
        }
    }
    fn proposal(&self, e: &Engine) -> String {
        self.proposal_from(e, &self.turn)
    }
    fn proposal_from(&self, e: &Engine, turn: &str) -> String {
        command(
            e,
            "proposal.create",
            None,
            json!({"turn_id":turn,"target":self.feature,"content":self.content}),
        )["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn extra_session(&self, e: &Engine, predecessor: Option<&str>) -> (String, String) {
        let next = id();
        let turn = id();
        e.store
            .transact(None, &mut |s| {
                let mut session = s
                    .sessions
                    .iter()
                    .find(|v| v.id == self.session)
                    .unwrap()
                    .clone();
                session.id = next.clone();
                session.external_session = Some(format!("fixture-{next}"));
                session.predecessor = predecessor.map(str::to_owned);
                s.sessions.push(session);
                let mut row = s.turns.iter().find(|v| v.id == self.turn).unwrap().clone();
                row.id = turn.clone();
                row.session_id = next.clone();
                row.client_message_id = id();
                s.turns.push(row);
                Ok(Value::Null)
            })
            .unwrap();
        (next, turn)
    }
}

fn publish(e: &Engine, document: Value) -> String {
    let draft = command(
        e,
        "workflow.create",
        None,
        json!({"name":"Reference regression plan","document":document}),
    );
    command(e, "workflow.publish", draft["id"].as_str(), json!({}))["id"]
        .as_str()
        .unwrap()
        .into()
}
fn confirm(e: &Engine, state: &WorkflowStepRuntime) -> Result<Value> {
    e.execute(
        "workflow.step.confirm",
        Command {
            command_id: id(),
            target: Some(state.id.clone()),
            expected_revision: Some(state.revision),
            payload: json!({"input_hash":state.input_hash}),
        },
    )
}

#[test]
fn all_proposal_stages_follow_replacements_and_advance_after_adoption_across_restart() {
    for phase in ["requirement", "design", "task_plan", "test_plan"] {
        let dir = tempfile::tempdir().unwrap();
        let e = engine(dir.path());
        let case = Scenario::new(&e, phase);
        let old = case.proposal(&e);
        e.evaluate_workflows().unwrap();
        assert_eq!(
            step(&workflow(&e, &case.feature), &case.discussion).outputs["proposal"],
            old
        );
        command(&e, "proposal.reject", Some(&old), json!({}));
        let replacement = case.proposal(&e);
        // Reproduce the existing stuck data: adopted replacement, completed source
        // still pointing at a rejected draft, adoption still waiting on that draft.
        command(&e, "proposal.adopt", Some(&replacement), json!({}));
        update_workflow(&e, &case.feature, |r| r.dirty = false);
        assert!(!e.store.has_pending_work().unwrap());
        drop(e);
        let e = engine(dir.path());
        assert!(e.store.has_pending_work().unwrap());
        e.evaluate_workflows().unwrap();
        let r = workflow(&e, &case.feature);
        assert_eq!(
            step(&r, &case.discussion).outputs["proposal"],
            replacement,
            "{phase}"
        );
        assert_eq!(
            step(&r, &case.adoption).outputs["proposal"],
            replacement,
            "{phase}"
        );
        assert_eq!(step(&r, &case.adoption).status, "completed", "{phase}");
        assert_ne!(step(&r, &case.next).status, "pending", "{phase}");
        assert_eq!(
            e.store
                .snapshot()
                .unwrap()
                .proposals
                .iter()
                .find(|p| p.id == old)
                .unwrap()
                .status,
            ProposalStatus::Rejected
        );
    }
}

#[test]
fn draft_revision_replaces_the_old_draft_and_rejection_reopens_discussion() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let case = Scenario::new(&e, "requirement");
    let old = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    let replacement = command(
        &e,
        "proposal.revise",
        Some(&old),
        json!({"content":case.content}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    assert_eq!(step(&r, &case.adoption).outputs["proposal"], replacement);
    command(&e, "proposal.reject", Some(&replacement), json!({}));
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    assert_eq!(step(&r, &case.discussion).status, "waiting_input");
    assert!(!step(&r, &case.discussion).outputs.contains_key("proposal"));
    assert!(!step(&r, &case.adoption).outputs.contains_key("proposal"));
    // Rejecting a successor must never revive its superseded parent draft.
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .proposals
            .iter()
            .find(|p| p.id == old)
            .unwrap()
            .status,
        ProposalStatus::Draft
    );
}

#[test]
fn discussion_follows_its_successor_but_ignores_an_unrelated_session() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let case = Scenario::new(&e, "requirement");
    let old = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    let (_, unrelated_turn) = case.extra_session(&e, None);
    let unrelated = case.proposal_from(&e, &unrelated_turn);
    e.evaluate_workflows().unwrap();
    assert_eq!(
        step(&workflow(&e, &case.feature), &case.adoption).outputs["proposal"],
        old
    );
    let (successor, successor_turn) = case.extra_session(&e, Some(&case.session));
    let replacement = case.proposal_from(&e, &successor_turn);
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    assert_eq!(step(&r, &case.discussion).outputs["session"], successor);
    assert_eq!(step(&r, &case.discussion).outputs["turn"], successor_turn);
    assert_eq!(step(&r, &case.adoption).outputs["proposal"], replacement);
    assert_ne!(replacement, unrelated);
}

#[test]
fn adopted_inputs_stay_frozen_when_later_unadopted_drafts_appear() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let case = Scenario::new(&e, "requirement");
    let accepted = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    command(&e, "proposal.adopt", Some(&accepted), json!({}));
    e.evaluate_workflows().unwrap();
    let before = workflow(&e, &case.feature);
    let later = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    let after = workflow(&e, &case.feature);
    assert_ne!(accepted, later);
    for key in [&case.discussion, &case.adoption] {
        assert_eq!(json!(step(&before, key)), json!(step(&after, key)));
    }
}

#[test]
fn a_reworked_discussion_needs_a_new_turn_even_when_its_session_is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let case = Scenario::new(&e, "requirement");
    case.proposal(&e);
    e.evaluate_workflows().unwrap();
    let previous = step(&workflow(&e, &case.feature), &case.discussion).clone();
    e.execute(
        "workflow.step.rework",
        Command {
            command_id: id(),
            target: Some(previous.id),
            expected_revision: Some(previous.revision),
            payload: json!({"from_step":case.discussion}),
        },
    )
    .unwrap();
    update_workflow(&e, &case.feature, |r| {
        let state = r
            .steps
            .iter_mut()
            .find(|s| s.key == case.discussion)
            .unwrap();
        state.outputs.insert("session".into(), case.session.clone());
    });
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    assert_eq!(step(&r, &case.discussion).status, "waiting_input");
    assert!(!step(&r, &case.discussion).outputs.contains_key("proposal"));
    assert!(!step(&r, &case.adoption).outputs.contains_key("proposal"));
    assert!(
        e.pending_workflow_intents()
            .unwrap()
            .iter()
            .any(|r| r.data["method"] == "conversation.send")
    );
}

#[test]
fn a_new_draft_invalidates_previous_discussion_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let mut plan = e
        .store
        .snapshot()
        .unwrap()
        .configuration("builtin-workflow-feature-sol-high-v4")
        .unwrap()
        .data
        .clone();
    plan["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["key"] == "requirement_discussion")
        .unwrap()["advance_mode"] = json!("human_confirmation");
    let reference = publish(&e, plan);
    let case = Scenario::with_workflow(&e, "requirement", &reference);
    let old = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    let first = step(&workflow(&e, &case.feature), &case.discussion).clone();
    confirm(&e, &first).unwrap();
    e.evaluate_workflows().unwrap();
    let replacement = command(
        &e,
        "proposal.revise",
        Some(&old),
        json!({"content":case.content}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    let current = step(&r, &case.discussion);
    assert_eq!(current.outputs["proposal"], replacement);
    assert_eq!(current.status, "waiting_confirmation");
    assert_ne!(current.input_hash, first.input_hash);
    assert!(current.confirmation.is_none());
    assert!(!step(&r, &case.adoption).outputs.contains_key("proposal"));
    assert!(confirm(&e, &first).is_err());
    confirm(&e, current).unwrap();
    e.evaluate_workflows().unwrap();
    assert_eq!(
        step(&workflow(&e, &case.feature), &case.adoption).outputs["proposal"],
        replacement
    );
}

#[test]
fn compiled_subworkflow_alias_exports_follow_the_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let mut parent = e
        .store
        .snapshot()
        .unwrap()
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    let discussion = parent["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "requirement_discussion")
        .unwrap()
        .clone();
    let mut child_step = discussion.clone();
    child_step["key"] = json!("discuss");
    child_step["stage"] = json!("discussion");
    child_step["dependencies"] = json!([]);
    let child = publish(
        &e,
        json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"discussion","name":"Discussion"}],"steps":[child_step],"output_ports":{"draft":{"step":"discuss","port":"proposal"}}}),
    );
    let steps = parent["steps"].as_array_mut().unwrap();
    let call = steps
        .iter_mut()
        .find(|s| s["key"] == "requirement_discussion")
        .unwrap();
    call["kind"] = json!("workflow.call");
    call["contract_version"] = json!(1);
    for key in ["task_instructions", "role_ref", "participant_slot"] {
        call.as_object_mut().unwrap().remove(key);
    }
    call["interaction"] = json!({});
    call["subworkflow_ref"] = json!(child);
    call["spec_refs"] = json!([]);
    steps
        .iter_mut()
        .find(|s| s["key"] == "requirement_adopt")
        .unwrap()["inputs"] = json!({"proposal":{"step":"requirement_discussion","port":"draft"}});
    let reference = publish(&e, parent);
    let mut case = Scenario::with_workflow(&e, "requirement", &reference);
    let s = e.store.snapshot().unwrap();
    let lock = s.feature_configuration(&case.feature).unwrap();
    case.discussion = lock
        .plan
        .steps
        .iter()
        .find(|step| step.kind == "agent.interact" && step.key.ends_with("discuss"))
        .unwrap()
        .key
        .clone();
    update_workflow(&e, &case.feature, |r| {
        let source = r
            .steps
            .iter_mut()
            .find(|s| s.key == case.discussion)
            .unwrap();
        source.status = "waiting_input".into();
        source
            .outputs
            .insert("session".into(), case.session.clone());
        source.outputs.insert("turn".into(), case.turn.clone());
    });
    let old = case.proposal(&e);
    e.evaluate_workflows().unwrap();
    command(&e, "proposal.reject", Some(&old), json!({}));
    let replacement = case.proposal(&e);
    command(&e, "proposal.adopt", Some(&replacement), json!({}));
    e.evaluate_workflows().unwrap();
    let r = workflow(&e, &case.feature);
    assert_eq!(step(&r, &case.discussion).outputs["proposal"], replacement);
    assert_eq!(
        step(&r, "requirement_discussion").outputs["draft"],
        replacement
    );
    assert_eq!(step(&r, &case.adoption).outputs["proposal"], replacement);
    assert_eq!(step(&r, &case.adoption).status, "completed");
}

#[test]
fn rework_clears_downstream_review_merge_and_snapshot_references() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let case = Scenario::new(&e, "requirement");
    update_workflow(&e, &case.feature, |r| {
        let mut downstream = false;
        for state in &mut r.steps {
            downstream |= state.key == "requirement_execute";
            if downstream {
                state.status = "completed".into();
                state.input_hash = "old-confirmation".into();
                state.confirmation = Some(json!({"input_hash":"old-confirmation"}));
                for port in [
                    "session",
                    "turn",
                    "proposal",
                    "delivery",
                    "merge",
                    "snapshot",
                    "review_run",
                ] {
                    state.outputs.insert(port.into(), format!("old-{port}"));
                }
            }
        }
    });
    let before = workflow(&e, &case.feature);
    let target = step(&before, "requirement_execute");
    e.execute(
        "workflow.step.rework",
        Command {
            command_id: id(),
            target: Some(target.id.clone()),
            expected_revision: Some(target.revision),
            payload: json!({"from_step":"requirement_execute"}),
        },
    )
    .unwrap();
    let after = workflow(&e, &case.feature);
    for key in [
        "requirement_execute",
        "requirement_review",
        "solution_execute",
        "solution_review",
        "tasks",
        "closure",
        "closure_review",
        "merge_prepare",
        "merge_review",
        "merge_apply",
        "snapshot",
    ] {
        let old = step(&before, key);
        let new = step(&after, key);
        assert_ne!(old.id, new.id, "{key}");
        assert_eq!(new.round, old.round + 1, "{key}");
        assert!(new.outputs.is_empty(), "{key}");
        assert!(new.confirmation.is_none(), "{key}");
    }
}
