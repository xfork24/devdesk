//! Public execution facts. Never project credentials or model connection settings.
use crate::*;

#[cfg(test)]
pub(crate) fn observed(state: &EngineeringState, turn_id: &str) -> Vec<Value> {
    let Some(turn) = state.turns.iter().find(|turn| turn.id == turn_id) else {
        return vec![];
    };
    state.executions.iter().filter(|run| {
        run.session_id == turn.session_id
            && run.kind == RuntimeExecutionKind::Harness
            && run.container_id.is_some()
            && run.writer_revoked
            && state.conversation_events.iter().any(|event| {
                event.turn_id == turn_id
                    && event.session_id == run.session_id
                    && event.kind == ConversationEventKind::Status
                    && event.deduplication_key == format!("{}:stop-proof", run.id)
                    && serde_json::from_str::<Value>(&event.content).is_ok_and(|proof| {
                        proof["operation_id"] == run.operation_id && proof["writer_revoked"] == true
                    })
            })
    }).map(|run| json!({
        "execution_id":run.id,"session_id":run.session_id,"turn_id":turn_id,
        "operation_id":run.operation_id,"image_digest":run.image_digest,
        "writer_revoked":run.writer_revoked,"protocol_artifact":run.protocol_artifact,
        "scope":"Observed outer harness execution for this exact Turn; does not attest arbitrary nested or remote programs."
    })).collect()
}

pub(crate) fn public_binding(session_id: &str, binding: &RuntimeBinding) -> Value {
    let artifact = match &binding.installation.artifact {
        RuntimeArtifact::DockerImage { image_id } => {
            json!({"kind":"docker_image","image_id":image_id})
        }
        RuntimeArtifact::OciImage { reference } => {
            json!({"kind":"oci_image","reference":reference})
        }
        RuntimeArtifact::Executable { sha256, .. } => json!({"kind":"executable","sha256":sha256}),
    };
    json!({"session_id":session_id,"installation_id":binding.installation.id,
                    "runtime_key":binding.installation.key,"artifact":artifact,
                    "recipe_digest":binding.installation.recipe_digest,"adapter_version":binding.adapter_version})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_binding_excludes_credentials_parameters_and_host_paths() {
        let binding: RuntimeBinding = serde_json::from_value(json!({
            "installation":{"id":"runtime","key":{"provider":"codex","version":"future-label","os":"linux","arch":"aarch64"},
                "artifact":{"kind":"executable","canonical_path":"/private/executable","sha256":"content"},
                "source":"/private/source","license":"test"},
            "adapter_version":"protocol","model":"private-model",
            "model_parameters":{"private-token":"parameter-secret"},
            "credential_ref":"credential-secret","tool_policy":"read_only",
            "work_type":"workflow","configuration_digest":"configuration"
        })).unwrap();
        let value = public_binding("session", &binding);
        assert_eq!(
            value["artifact"],
            json!({"kind":"executable","sha256":"content"})
        );
        assert_eq!(value["runtime_key"]["version"], "future-label");
        let serialized = value.to_string();
        for private in [
            "credential-secret",
            "parameter-secret",
            "/private/",
            "private-model",
        ] {
            assert!(!serialized.contains(private));
        }
    }

    #[test]
    fn observed_image_requires_exact_turn_operation_and_stopped_worker() {
        let mut state = EngineeringState::default();
        state.turns.push(
            serde_json::from_value(json!({
                "id":"turn","project_id":"project","session_id":"session",
                "client_message_id":"message","input":"task","input_digest":"digest",
                "status":"completed","consumed":true,"revision":1
            }))
            .unwrap(),
        );
        state.executions.push(
            serde_json::from_value(json!({
                "id":"execution","session_id":"session","operation_id":"operation",
                "container_id":"actual-container","image_digest":"sha256:actual",
                "status":"released","writer_revoked":true
            }))
            .unwrap(),
        );
        state.conversation_events.push(
            serde_json::from_value(json!({
                "id":"proof","session_id":"session","turn_id":"turn","sequence":1,
                "deduplication_key":"execution:stop-proof","kind":"status","time":1,
                "content":json!({"operation_id":"operation","writer_revoked":true}).to_string()
            }))
            .unwrap(),
        );
        assert_eq!(observed(&state, "turn")[0]["image_digest"], "sha256:actual");
        assert!(observed(&state, "other-turn").is_empty());
        state.conversation_events[0].turn_id = "other-turn".into();
        assert!(observed(&state, "turn").is_empty());
        state.conversation_events[0].turn_id = "turn".into();
        state.conversation_events[0].content =
            json!({"operation_id":"forged","writer_revoked":true}).to_string();
        assert!(observed(&state, "turn").is_empty());
        state.conversation_events[0].content =
            json!({"operation_id":"operation","writer_revoked":true}).to_string();
        state.executions[0].writer_revoked = false;
        assert!(observed(&state, "turn").is_empty());
        state.executions[0].writer_revoked = true;
        state.executions[0].container_id = None;
        assert!(observed(&state, "turn").is_empty());
    }
}
