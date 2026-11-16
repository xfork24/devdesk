//! Business decisions remain human-owned regardless of the selected driver policy.
use crate::*;

fn governed(state: &EngineeringState, instance: &ConfigurationRecord) -> bool {
    let root = instance.data["root_instance_id"]
        .as_str()
        .unwrap_or(&instance.id);
    state.governance.iter().any(|row| {
        row.r#type == "workflow_instance"
            && (row.id == root || row.data["root_instance_id"] == root)
            && (row.data["human_governance"] == true
                || row.data["plan"]["document"]["steps"]
                    .as_array()
                    .is_some_and(|steps| steps.iter().any(|step| step["confirmation"] == "human")))
    })
}

fn governed_owner(state: &EngineeringState, owner: &str) -> bool {
    let feature = state
        .tasks
        .iter()
        .find(|task| task.id == owner)
        .map(|task| task.feature_id.as_str());
    state.governance.iter().any(|row| {
        row.r#type == "workflow_instance"
            && (row.data["owner_id"] == owner
                || feature.is_some_and(|id| row.data["owner_id"] == id))
            && governed(state, row)
    })
}

// A manager may relay an answer, but only verbatim from the human input
// frozen into this exact controller Turn. Model text never grants authority.
fn relays_human_input(
    state: &EngineeringState,
    instance: &ConfigurationRecord,
    command: &Command,
    actor: &WorkflowActor,
) -> bool {
    let WorkflowActor::Controller {
        controller_id,
        session_id,
        turn_id,
        epoch,
    } = actor
    else {
        return false;
    };
    let Some(input) = command.payload["input"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
    else {
        return false;
    };
    let Some(controller) = state.governance.iter().find(|row| {
        row.id == *controller_id
            && row.r#type == "workflow_controller"
            && row.data["session_id"] == *session_id
            && row.data["epoch"] == *epoch
            && row.data["root_instance_id"] == instance.data["root_instance_id"]
    }) else {
        return false;
    };
    let Some(turn) = state
        .turns
        .iter()
        .find(|turn| turn.id == *turn_id && turn.session_id == *session_id)
    else {
        return false;
    };
    state.governance.iter().any(|intent| {
        let payload = &intent.data["command"]["payload"];
        intent.r#type == "workflow_intent"
            && intent.definition_id == controller.id
            && intent.data["method"] == "conversation.send"
            && intent.data["command"]["target"] == *session_id
            && payload["controller_epoch"] == *epoch
            && payload["client_message_id"] == turn.client_message_id
            && payload["controller_input_refs"]
                .as_array()
                .is_some_and(|refs| {
                    state.governance.iter().any(|source| {
                        source.r#type == "controller_input"
                            && source.definition_id == controller.id
                            && refs.contains(&json!(source.id))
                            && source.data["input"] == input
                    })
                })
    })
}

pub(crate) fn authorize(
    state: &EngineeringState,
    method: &str,
    command: &Command,
    actor: &WorkflowActor,
) -> Result<()> {
    if matches!(actor, WorkflowActor::Human) {
        return Ok(());
    }
    // This check also covers internal callers which do not use orchestration_command.
    require(
        ![
            "workflow.step.confirm",
            "workflow.step.approve",
            "workflow.decision.resolve",
        ]
        .contains(&method),
        "非人类调用不能签署人工决定",
    )?;
    let target = command.target.as_deref().unwrap_or("");
    if ["workflow.start", "work.configuration.update"].contains(&method) {
        require(
            !governed_owner(state, target),
            "受人工决策保护的工作只能由人类切换规范或启动后继流程；Agent 请等待用户决定",
        )?;
        return Ok(());
    }
    let Some(instance) = state
        .governance
        .iter()
        .find(|r| r.id == target && r.r#type == "workflow_instance")
    else {
        return Ok(());
    };
    if !governed(state, instance) {
        return Ok(());
    }
    if method == "workflow.step.input" {
        if relays_human_input(state, instance, command, actor) {
            return Ok(());
        }
        return Err(err(
            "human_input_required",
            "澄清问题必须由人类回答；项目经理可原样转交本轮收到的用户指令，不能代答或改写",
        ));
    }
    if method == "workflow.change.apply" {
        let mut arguments = command.payload.clone();
        let decision_ref = arguments
            .as_object_mut()
            .and_then(|args| args.remove("decision_ref"));
        let signed = decision_ref
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|id| state.configuration(id).ok());
        if !signed.is_some_and(|row| {
            row.r#type == "workflow_human_decision"
                && row.data["actor"]["kind"] == "human"
                && row.data["approved"] == true
                && row.data["plan_revision_id"] == instance.data["active_plan_revision_id"]
                && row.data["request"]["operation"] == method
                && row.data["request"]["arguments"] == arguments
                && state.governance.iter().any(|pending| {
                    pending.id == row.definition_id
                        && pending.r#type == "workflow_pending_decision"
                        && pending.definition_id == instance.id
                        && pending.data["status"] == "approved"
                        && pending.data["decision_ref"] == row.id
                })
        }) {
            return Err(err(
                "human_decision_required",
                "范围、验收标准、方案或流程变更必须先请求人类批准确切变更；不能自行改写、跳过或豁免人工门禁",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governance::record;

    fn state() -> EngineeringState {
        let mut state = EngineeringState::default();
        let mut root = record(
            "workflow_instance",
            "feature".into(),
            Scope::global(),
            "governed".into(),
            1,
            None,
            json!({"owner_id":"feature","root_instance_id":"root","active_plan_revision_id":"plan","human_governance":true}),
            false,
        );
        root.id = "root".into();
        let mut child = root.clone();
        child.id = "child".into();
        child.data["human_governance"] = json!(false);
        state.governance.extend([root, child]);
        state.tasks.push(Task {
            id: "unstarted-task".into(),
            feature_id: "feature".into(),
            title: "已确认的任务".into(),
            status: Status::Queued,
            revision: 1,
            inherited_snapshot: None,
            dependencies: vec![],
            worktree: String::new(),
            scope: TaskScope::default(),
        });
        state
    }
    fn command(target: &str) -> Command {
        Command {
            command_id: "request".into(),
            target: Some(target.into()),
            expected_revision: Some(1),
            payload: json!({"instance_id":target,"plan_revision_id":"plan","preview_hash":"exact-preview","changeset":{"reason":"改变范围"}}),
        }
    }
    fn actors() -> Vec<WorkflowActor> {
        vec![
            WorkflowActor::Controller {
                controller_id: "manager".into(),
                session_id: "session".into(),
                turn_id: "turn".into(),
                epoch: 1,
            },
            WorkflowActor::Driver {
                driver_id: "rust".into(),
                epoch: 1,
            },
            WorkflowActor::Segment {
                authorization_id: "segment".into(),
            },
        ]
    }
    fn sign(state: &mut EngineeringState, command: &mut Command, approved: bool) {
        let mut pending = record(
            "workflow_pending_decision",
            command.target.clone().unwrap(),
            Scope::global(),
            "请求决定".into(),
            1,
            None,
            json!({"status":if approved {"approved"} else {"rejected"}}),
            false,
        );
        let signed = record(
            "workflow_human_decision",
            pending.id.clone(),
            Scope::global(),
            "人类决定".into(),
            1,
            None,
            json!({"actor":{"kind":"human"},"approved":approved,"plan_revision_id":"plan","request":{"operation":"workflow.change.apply","arguments":command.payload}}),
            true,
        );
        pending.data["decision_ref"] = json!(signed.id);
        command.payload["decision_ref"] = json!(signed.id);
        state.governance.extend([pending, signed]);
    }

    #[test]
    fn every_nonhuman_driver_must_wait_for_actual_answers_and_business_decisions() {
        let state = state();
        for target in ["root", "child"] {
            for actor in actors() {
                let c = command(target);
                assert_eq!(
                    authorize(&state, "workflow.step.input", &c, &actor)
                        .unwrap_err()
                        .code,
                    "human_input_required"
                );
                assert_eq!(
                    authorize(&state, "workflow.change.apply", &c, &actor)
                        .unwrap_err()
                        .code,
                    "human_decision_required"
                );
                for method in [
                    "workflow.step.confirm",
                    "workflow.step.approve",
                    "workflow.decision.resolve",
                ] {
                    assert!(authorize(&state, method, &c, &actor).is_err());
                }
                assert!(authorize(&state, "workflow.step.dispatch", &c, &actor).is_ok());
            }
            assert!(
                authorize(
                    &state,
                    "workflow.step.input",
                    &command(target),
                    &WorkflowActor::Human
                )
                .is_ok()
            );
        }
        for owner in ["feature", "unstarted-task"] {
            for method in ["workflow.start", "work.configuration.update"] {
                assert!(authorize(&state, method, &command(owner), &actors()[0]).is_err());
                assert!(authorize(&state, method, &command(owner), &WorkflowActor::Human).is_ok());
            }
        }
    }

    #[test]
    fn manager_relays_only_verbatim_human_input_bound_to_the_current_turn() {
        let mut state = state();
        let mut controller = record(
            "workflow_controller",
            "feature".into(),
            Scope::global(),
            "manager".into(),
            1,
            None,
            json!({"root_instance_id":"root","session_id":"session","epoch":1}),
            false,
        );
        controller.id = "manager".into();
        let source = record(
            "controller_input",
            "manager".into(),
            Scope::global(),
            "human".into(),
            1,
            None,
            json!({"input":"只导出 ID 和名称。","consumed":true}),
            false,
        );
        let intent = record(
            "workflow_intent",
            "manager".into(),
            Scope::global(),
            "send".into(),
            1,
            None,
            json!({"method":"conversation.send","command":{"target":"session","payload":{"controller_epoch":1,"client_message_id":"message","controller_input_refs":[source.id]}}}),
            false,
        );
        state.governance.extend([controller, source, intent]);
        state.turns.push(serde_json::from_value(json!({"id":"turn","project_id":"project","session_id":"session","client_message_id":"message","input":"frozen snapshot","input_digest":"digest","status":"running","external_message":null,"external_turn":null,"consumed":true,"started":1,"finished":null,"revision":1})).unwrap());
        let mut c = command("root");
        c.payload["input"] = json!("只导出 ID 和名称。");
        let actor = actors().remove(0);
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_ok());
        c.target = Some("child".into());
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_ok());
        c.payload["input"] = json!("额外导出电话。");
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_err());
        c.payload["input"] = json!("只导出 ID 和名称。");
        state.turns[0].client_message_id = "another-turn".into();
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_err());
        state.turns[0].client_message_id = "message".into();
        state
            .governance
            .iter_mut()
            .find(|row| row.id == "manager")
            .unwrap()
            .data["epoch"] = json!(2);
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_err());
        state
            .governance
            .iter_mut()
            .find(|row| row.id == "manager")
            .unwrap()
            .data["epoch"] = json!(1);
        state
            .governance
            .iter_mut()
            .find(|row| row.id == "child")
            .unwrap()
            .data["root_instance_id"] = json!("foreign-root");
        state
            .governance
            .iter_mut()
            .find(|row| row.id == "child")
            .unwrap()
            .data["human_governance"] = json!(true);
        assert!(authorize(&state, "workflow.step.input", &c, &actor).is_err());
    }

    #[test]
    fn an_approval_is_bound_to_the_exact_instance_plan_and_change_preview() {
        let mut state = state();
        let mut c = command("root");
        sign(&mut state, &mut c, true);
        assert!(authorize(&state, "workflow.change.apply", &c, &actors()[0]).is_ok());
        let original = c.clone();
        for key in [
            "preview_hash",
            "plan_revision_id",
            "changeset",
            "instance_id",
        ] {
            c = original.clone();
            c.payload[key] = json!("changed");
            assert!(
                authorize(&state, "workflow.change.apply", &c, &actors()[0]).is_err(),
                "{key}"
            );
        }
        c = original;
        c.target = Some("child".into());
        assert!(authorize(&state, "workflow.change.apply", &c, &actors()[0]).is_err());
        let mut rejected = command("root");
        sign(&mut state, &mut rejected, false);
        assert!(authorize(&state, "workflow.change.apply", &rejected, &actors()[0]).is_err());
        let mut unsigned = command("root");
        unsigned.payload["actor"] = json!({"kind":"human"});
        assert!(authorize(&state, "workflow.change.apply", &unsigned, &actors()[0]).is_err());
    }
}
