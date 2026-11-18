//! Explicit, typed history for opt-in performance fixtures. No external effects
//! or model completions are fabricated: discussion Turns were cancelled before
//! dispatch, and their messages are labelled fixture inputs.
use devdesk_core::{Engine, id};
use devdesk_domain::*;
use serde_json::{Value, json};

pub(super) fn seed(engine: &Engine, project: &str, scale: usize) {
    assert!([1, 10].contains(&scale));
    let discussion = project == GLOBAL_DISCUSSION_NAMESPACE;
    let (session_id, _) = crate::test_runtime::queue_conversation(engine, project);
    engine
        .store
        .seed_fixture_state(None, &mut |s| {
            let mut session = s
                .sessions
                .iter()
                .find(|r| r.id == session_id)
                .unwrap()
                .clone();
            for turn in s.turns.iter_mut().filter(|t| t.session_id == session_id) {
                turn.status = TurnStatus::Cancelled;
                turn.cancel_requested = true;
                turn.finished = Some(1);
            }
            for original in s.sessions.iter_mut().filter(|r| r.id == session_id) {
                original.lifecycle = Some(SessionLifecycle::Closed);
            }
            session.lifecycle = Some(SessionLifecycle::Closed);
            let feature_count = s.features.len();
            for n in feature_count..if discussion {
                feature_count
            } else {
                100 * scale
            } {
                s.features.push(Feature {
                    id: id(),
                    project_id: project.into(),
                    title: format!("History fixture {n}"),
                    status: Status::Draft,
                    revision: 1,
                    input_version: 1,
                    predecessor: None,
                    source_materials: vec![],
                });
            }
            for n in 0..100 * scale {
                let mut session = session.clone();
                session.id = id();
                for turn_index in 0..20 {
                    let turn_id = id();
                    let input = format!(
                        "Cancelled performance fixture input {n}/{turn_index}; no model dispatched"
                    );
                    s.turns.push(ConversationTurn {
                        id: turn_id.clone(),
                        project_id: project.into(),
                        session_id: session.id.clone(),
                        client_message_id: id(),
                        input_digest: configuration_hash(&input),
                        input,
                        input_snapshot: None,
                        code_view: None,
                        source_revision: None,
                        status: TurnStatus::Cancelled,
                        external_message: None,
                        external_turn: None,
                        consumed: false,
                        cancel_requested: true,
                        started: None,
                        finished: Some(1),
                        revision: 1,
                    });
                    for sequence in 0..5 {
                        s.conversation_events.push(ConversationEvent {
                            id: id(),
                            session_id: session.id.clone(),
                            turn_id: turn_id.clone(),
                            sequence: turn_index * 5 + sequence + 1,
                            deduplication_key: id(),
                            kind: ConversationEventKind::User,
                            content: format!(
                                "Explicit cancelled fixture history {n}/{turn_index}/{sequence}"
                            ),
                            artifact_id: None,
                            time: 1,
                            mode: MessageMode::Full,
                            provider_item: None,
                        });
                    }
                }
                s.sessions.push(session);
            }
            Ok(Value::Null)
        })
        .unwrap();
    let state = engine.store.snapshot().unwrap();
    assert!(discussion || state.features.len() >= 100 * scale);
    assert!(state.turns.len() >= 2_000 * scale);
    assert_eq!(state.conversation_events.len(), 10_000 * scale);
    let sessions = state
        .sessions
        .iter()
        .map(|s| s.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(
        state
            .turns
            .iter()
            .all(|t| sessions.contains(t.session_id.as_str()))
    );
    eprintln!(
        "PERFORMANCE_HISTORY={}",
        json!({"project":project,"features":state.features.len(),"turns":state.turns.len(),"conversation_events":state.conversation_events.len(),"scale":scale,"discussion_kind":"cancelled_before_dispatch"})
    );
}
