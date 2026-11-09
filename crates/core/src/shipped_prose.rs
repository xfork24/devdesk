//! Publish improved shipped prose without mutating frozen revisions or user edits.
use crate::*;

pub(crate) fn role_method(role: &str) -> &'static str {
    match role {
        "analyst" => include_str!("../../../resources/configuration/skills/method-analyst.md"),
        "architect" => include_str!("../../../resources/configuration/skills/method-architect.md"),
        "developer" => include_str!("../../../resources/configuration/skills/method-developer.md"),
        "tester" => include_str!("../../../resources/configuration/skills/method-tester.md"),
        "reviewer" => include_str!("../../../resources/configuration/skills/method-reviewer.md"),
        _ => unreachable!("shipped execution role"),
    }
}

pub(crate) fn upgrade(s: &mut EngineeringState) {
    let mut prose = vec![
        (
            "builtin-skill-engineering-context".to_string(),
            "body",
            include_str!("../../../resources/configuration/skills/engineering-context.md"),
        ),
        (
            "builtin-skill-engineering-method".to_string(),
            "body",
            include_str!("../../../resources/configuration/skills/engineering-method.md"),
        ),
        (
            "builtin-standard-engineering".to_string(),
            "body",
            include_str!("../../../resources/configuration/standards/engineering.md"),
        ),
        (
            "builtin-spec-planning".to_string(),
            "body",
            include_str!("../../../resources/configuration/specs/legacy-planning.md"),
        ),
        (
            "builtin-spec-implementation".to_string(),
            "body",
            include_str!("../../../resources/configuration/specs/legacy-implementation.md"),
        ),
        (
            "builtin-spec-merge".to_string(),
            "body",
            include_str!("../../../resources/configuration/specs/legacy-merge.md"),
        ),
    ];
    for (role, body) in [
        (
            "analyst",
            include_str!("../../../resources/configuration/roles/analyst.md"),
        ),
        (
            "architect",
            include_str!("../../../resources/configuration/roles/architect.md"),
        ),
        (
            "developer",
            include_str!("../../../resources/configuration/roles/developer.md"),
        ),
        (
            "tester",
            include_str!("../../../resources/configuration/roles/tester.md"),
        ),
        (
            "reviewer",
            include_str!("../../../resources/configuration/roles/reviewer.md"),
        ),
        (
            "project-manager",
            include_str!("../../../resources/configuration/roles/project-manager.md"),
        ),
    ] {
        prose.push((format!("builtin-role-{role}"), "instructions", body));
        if role != "project-manager" {
            prose.push((format!("builtin-skill-{role}"), "body", role_method(role)));
        }
    }
    for (key, body) in [
        (
            "prd",
            include_str!("../../../resources/configuration/specs/prd.md"),
        ),
        (
            "architecture",
            include_str!("../../../resources/configuration/specs/architecture.md"),
        ),
        (
            "project-planning",
            include_str!("../../../resources/configuration/specs/project-planning.md"),
        ),
        (
            "test-design",
            include_str!("../../../resources/configuration/specs/test-design.md"),
        ),
        (
            "solution",
            include_str!("../../../resources/configuration/specs/solution.md"),
        ),
        (
            "implementation",
            include_str!("../../../resources/configuration/specs/implementation.md"),
        ),
        (
            "closure",
            include_str!("../../../resources/configuration/specs/closure.md"),
        ),
        (
            "release",
            include_str!("../../../resources/configuration/specs/release.md"),
        ),
    ] {
        prose.push((format!("builtin-spec-stage-{key}"), "body", body));
    }
    for (identity, field, body) in prose {
        let Some(old) = s
            .governance
            .iter()
            .filter(|r| r.definition_id == identity)
            .max_by_key(|r| r.version)
            .cloned()
        else {
            continue;
        };
        // User-published revisions are never silently superseded by an application update.
        if old.created_at != 0
            || old.data[field].as_str() == Some(body)
            || crate::configuration::retired(s, &old)
        {
            continue;
        }
        let mut next = old.clone();
        next.version += 1;
        next.supersedes = Some(old.id.clone());
        next.data[field] = json!(body);
        next.content_hash = configuration_hash(&next.data);
        next.id = format!(
            "{}-prose-{}-v{}",
            identity,
            &next.content_hash[..12],
            next.version
        );
        next.revision = 1;
        if let Some(mut definition) = s.definitions.iter().find(|r| r.id == old.id).cloned() {
            definition.id = next.id.clone();
            definition.version = next.version;
            definition.supersedes = Some(old.id.clone());
            definition.body = body.into();
            s.definitions.push(definition);
        }
        s.governance.push(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn improved_prose_publishes_once_preserves_frozen_content_and_respects_user_revisions() {
        let mut state = EngineeringState::default();
        crate::seed_configuration(&mut state);
        let id = "builtin-skill-engineering-context-v2";
        let old = state.governance.iter_mut().find(|r| r.id == id).unwrap();
        old.data["body"] = json!("旧的一行说明");
        old.content_hash = configuration_hash(&old.data);
        let frozen = old.clone();
        let latest_role = state
            .governance
            .iter_mut()
            .filter(|r| r.definition_id == "builtin-role-analyst")
            .max_by_key(|r| r.version)
            .unwrap();
        latest_role.data["instructions"] = json!("用户自定义职责");
        latest_role.content_hash = configuration_hash(&latest_role.data);
        latest_role.created_at = 123;
        let user = latest_role.clone();
        upgrade(&mut state);
        assert_eq!(json!(state.configuration(id).unwrap()), json!(frozen));
        let revision = state
            .governance
            .iter()
            .filter(|r| r.definition_id == frozen.definition_id)
            .max_by_key(|r| r.version)
            .unwrap();
        assert_eq!(revision.supersedes.as_deref(), Some(id));
        assert!(
            revision.data["body"]
                .as_str()
                .unwrap()
                .matches("## ")
                .count()
                >= 6
        );
        assert_eq!(json!(state.configuration(&user.id).unwrap()), json!(user));
        assert_eq!(
            state
                .governance
                .iter()
                .filter(|r| r.definition_id == user.definition_id)
                .count(),
            1
        );
        let count = state.governance.len();
        crate::seed_configuration(&mut state);
        assert_eq!(state.governance.len(), count);
        assert_eq!(json!(state.configuration(id).unwrap()), json!(frozen));
    }
}
