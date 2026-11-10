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

// The initial catalog (b0e8819) used ordinary timestamps for shipped v1 records.
// Match its exact published prose, never a prefix or a user's subsequent revision.
fn original_timestamped_seed(record: &ConfigurationRecord, field: &str) -> bool {
    if record.version != 1 || record.id != format!("{}-v1", record.definition_id) {
        return false;
    }
    let expected = match record.definition_id.as_str() {
        "builtin-role-analyst" => {
            "f5da61f931cac4bebef726a903a125c87c2e7cdcc5d3870e92efcbcaf38b6088"
        }
        "builtin-role-architect" => {
            "b3ce5c0af6e6ba56708eb3ddefff29cb68e91e7a90307bc9273c02f99171bf4d"
        }
        "builtin-role-developer" => {
            "97a87ef877c8b1ba61cf7cd32160926b2833e47e67b77c237c59bd9b52a57ce1"
        }
        "builtin-role-reviewer" => {
            "8e35859443a0e64520bea66c178e5dfaae32362d311bc7419e15705e4ee90c01"
        }
        "builtin-role-tester" => "bccc35417fd62a0bf14247276a061e1dd64601eb2dc82de837e1f70f430bd463",
        "builtin-skill-analyst" => {
            "f5da61f931cac4bebef726a903a125c87c2e7cdcc5d3870e92efcbcaf38b6088"
        }
        "builtin-skill-architect" => {
            "b3ce5c0af6e6ba56708eb3ddefff29cb68e91e7a90307bc9273c02f99171bf4d"
        }
        "builtin-skill-developer" => {
            "97a87ef877c8b1ba61cf7cd32160926b2833e47e67b77c237c59bd9b52a57ce1"
        }
        "builtin-skill-reviewer" => {
            "8e35859443a0e64520bea66c178e5dfaae32362d311bc7419e15705e4ee90c01"
        }
        "builtin-skill-tester" => {
            "bccc35417fd62a0bf14247276a061e1dd64601eb2dc82de837e1f70f430bd463"
        }
        "builtin-spec-implementation" => {
            "fb047ebc8936f30f731cf7c7f99c45262c1770acf9da9c1e2be2047338e9b6a2"
        }
        "builtin-spec-merge" => "8022e2858a36936578099c1870df37dbfc65a21f8704f4f66b7f2e9a647bc4a4",
        "builtin-spec-planning" => {
            "b0bf01cfadbf7c92693dc7d46e2aa39930c7ad21c56d0c97d9ae0abdd100240e"
        }
        "builtin-standard-engineering" => {
            "2785cb1150f952654855a69ef24b55c004dc8150f440446daaf35ab67f6ba75a"
        }
        _ => return false,
    };
    configuration_hash(&record.data[field]) == expected
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
    for (key, body) in [
        (
            "prd",
            include_str!("../../../resources/configuration/skills/prd.md"),
        ),
        (
            "architecture",
            include_str!("../../../resources/configuration/skills/architecture.md"),
        ),
        (
            "project-planning",
            include_str!("../../../resources/configuration/skills/project-planning.md"),
        ),
        (
            "test-design",
            include_str!("../../../resources/configuration/skills/test-design.md"),
        ),
        (
            "tdd-implementation",
            include_str!("../../../resources/configuration/skills/tdd-implementation.md"),
        ),
        (
            "release-verification",
            include_str!("../../../resources/configuration/skills/release-verification.md"),
        ),
        (
            "independent-review",
            include_str!("../../../resources/configuration/skills/independent-review.md"),
        ),
    ] {
        prose.push((format!("builtin-skill-{key}"), "body", body));
    }
    for (key, body) in [
        (
            "prd",
            include_str!("../../../resources/configuration/deliveries/prd.md"),
        ),
        (
            "design",
            include_str!("../../../resources/configuration/deliveries/design.md"),
        ),
        (
            "diff",
            include_str!("../../../resources/configuration/deliveries/diff.md"),
        ),
        (
            "test-report",
            include_str!("../../../resources/configuration/deliveries/test-report.md"),
        ),
    ] {
        prose.push((format!("builtin-delivery-{key}"), "body", body));
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
        if (old.created_at != 0 && !original_timestamped_seed(&old, field))
            || old.data[field].as_str() == Some(body)
            || crate::configuration::retired(s, &old)
        {
            continue;
        }
        let mut data = old.data.clone();
        data[field] = json!(body);
        publish(s, &old, data);
    }
    refresh_derived(s);
}

fn publish(s: &mut EngineeringState, old: &ConfigurationRecord, data: Value) {
    if old.data == data {
        return;
    }
    let mut next = old.clone();
    next.version += 1;
    next.supersedes = Some(old.id.clone());
    next.data = data;
    next.content_hash = configuration_hash(&next.data);
    next.id = format!(
        "{}-prose-{}-v{}",
        old.definition_id,
        &next.content_hash[..12],
        next.version
    );
    next.revision = 1;
    next.created_at = 0;
    if next.kind == "agent" {
        if let Ok(composition) = next.decode::<AgentComposition>() {
            crate::governance::install_agent_profile(s, &next, &composition);
        }
    } else if let Some(mut definition) = s.definitions.iter().find(|r| r.id == old.id).cloned() {
        definition.id = next.id.clone();
        definition.version = next.version;
        definition.supersedes = Some(old.id.clone());
        if let Some(body) = next.data["body"].as_str() {
            definition.body = body.into();
        }
        s.definitions.push(definition);
    }
    s.governance.push(next);
}

// Advance exact references only through application-authored revisions. Identity references
// retain their meaning; user revisions and every previous immutable snapshot stay untouched.
fn refresh_references(s: &EngineeringState, data: &mut Value) {
    match data {
        Value::String(id) => {
            let Some(old) = s
                .governance
                .iter()
                .find(|r| r.id == *id && r.definition_id.starts_with("builtin-"))
            else {
                return;
            };
            let Some(latest) = s
                .governance
                .iter()
                .filter(|r| r.definition_id == old.definition_id)
                .max_by_key(|r| r.version)
            else {
                return;
            };
            if latest.created_at == 0
                && latest.id.contains("-prose-")
                && !crate::configuration::retired(s, latest)
            {
                *id = latest.id.clone();
            }
        }
        Value::Array(values) => {
            for value in values {
                refresh_references(s, value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                refresh_references(s, value);
            }
        }
        _ => {}
    }
}

fn refresh_derived(s: &mut EngineeringState) {
    // Dependencies first: skills -> capabilities -> Agents; Task workflows -> Features.
    for kind in ["capability", "agent", "workflow"] {
        let mut records: Vec<_> = s
            .governance
            .iter()
            .filter(|r| {
                r.kind == kind
                    && r.definition_id.starts_with("builtin-")
                    && !s.governance.iter().any(|newer| {
                        newer.definition_id == r.definition_id && newer.version > r.version
                    })
                    && r.created_at == 0
                    && !crate::configuration::retired(s, r)
            })
            .cloned()
            .collect();
        if kind == "workflow" {
            records.sort_by_key(|r| r.data["process_kind"] != "task");
        }
        for old in records {
            let mut data = old.data.clone();
            if kind == "agent" {
                if let Some(role) = data["role_ref"]
                    .as_str()
                    .and_then(|id| s.configuration(id).ok())
                {
                    let latest = s
                        .governance
                        .iter()
                        .filter(|r| r.definition_id == role.definition_id)
                        .max_by_key(|r| r.version)
                        .unwrap();
                    if latest.created_at == 0 && !crate::configuration::retired(s, latest) {
                        data["instructions"] = latest.data["instructions"].clone();
                    }
                }
            }
            if kind == "workflow" {
                let Ok(doc) = old.decode::<WorkflowDocument>() else {
                    continue;
                };
                if doc.schema_version != 3 {
                    continue;
                }
                let mut legacy = doc.clone();
                legacy.schema_version = 2;
                let Ok(mut next) = crate::default_tasks::upgrade_document(&legacy, false) else {
                    continue;
                };
                for stage in &mut next.stages {
                    stage.description =
                        crate::default_catalog::stage_description(&stage.key).into();
                }
                // Compare typed values so optional/default serialization cannot create a revision.
                if json!(next) != json!(doc) {
                    data = json!(next);
                }
            }
            refresh_references(s, &mut data);
            publish(s, &old, data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_timestamped_seeds_upgrade_without_claiming_user_revisions() {
        let mut state = EngineeringState::default();
        crate::seed_configuration(&mut state);
        let role = state
            .governance
            .iter_mut()
            .find(|r| r.id == "builtin-role-analyst-v1")
            .unwrap();
        role.created_at = 123;
        role.data["instructions"] = json!(
            "Clarify goals, constraints, acceptance criteria and failure paths. Brainstorm with the user, ask concrete questions when needed, and produce structured Requirement proposals. Complete and review requirements before requesting solution artifacts. Never mutate code, adopt proposals or confirm workflow steps for a human."
        );
        role.content_hash = configuration_hash(&role.data);
        assert!(original_timestamped_seed(role, "instructions"));
        let frozen = role.clone();
        let custom = state
            .governance
            .iter_mut()
            .find(|r| r.id == "builtin-role-developer-v1")
            .unwrap();
        custom.created_at = 123;
        custom.data["instructions"] = json!("用户在原标识下导入的职责");
        custom.content_hash = configuration_hash(&custom.data);
        let user = custom.clone();
        upgrade(&mut state);
        let latest = state
            .governance
            .iter()
            .filter(|r| r.definition_id == frozen.definition_id)
            .max_by_key(|r| r.version)
            .unwrap();
        assert_eq!(latest.created_at, 0);
        assert_eq!(latest.supersedes.as_deref(), Some(frozen.id.as_str()));
        assert!(
            latest.data["instructions"]
                .as_str()
                .unwrap()
                .starts_with("# 需求分析师职责说明")
        );
        assert_eq!(
            json!(state.configuration(&frozen.id).unwrap()),
            json!(frozen)
        );
        assert_eq!(json!(state.configuration(&user.id).unwrap()), json!(user));
        assert!(
            !state
                .governance
                .iter()
                .any(|r| r.supersedes.as_deref() == Some(user.id.as_str()))
        );
        let count = state.governance.len();
        crate::seed_configuration(&mut state);
        assert_eq!(state.governance.len(), count);
    }

    #[test]
    fn refreshed_defaults_keep_old_snapshots_and_follow_new_exact_dependencies() {
        let mut state = EngineeringState::default();
        crate::seed_configuration(&mut state);
        let original_count = state.governance.len();
        upgrade(&mut state);
        assert_eq!(
            state.governance.len(),
            original_count,
            "fresh defaults need no synthetic revision"
        );
        for (id, field) in [
            ("builtin-role-analyst-v1", "instructions"),
            ("builtin-skill-prd-v2", "body"),
            ("builtin-spec-stage-prd-v1", "body"),
            ("builtin-delivery-prd-v1", "body"),
        ] {
            let old = state.governance.iter_mut().find(|r| r.id == id).unwrap();
            old.data[field] = json!("旧版简短说明");
            old.content_hash = configuration_hash(&old.data);
        }
        let task = state
            .governance
            .iter_mut()
            .find(|r| r.id == DEFAULT_TASK_WORKFLOW_V3_ID)
            .unwrap();
        task.data["stages"][0]["description"] = json!("旧版阶段说明");
        task.data["steps"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|step| step["task_instructions"].is_string())
            .for_each(|step| step["task_instructions"] = json!("旧版任务"));
        task.content_hash = configuration_hash(&task.data);
        let frozen = state.governance.clone();
        upgrade(&mut state);
        for old in &frozen {
            assert_eq!(json!(state.configuration(&old.id).unwrap()), json!(old));
        }
        let latest = |identity: &str| {
            state
                .governance
                .iter()
                .filter(|r| r.definition_id == identity)
                .max_by_key(|r| r.version)
                .unwrap()
        };
        let skill = latest("builtin-skill-prd");
        let cap = latest("builtin-capability-analyst-codex");
        let role = latest("builtin-role-analyst");
        let agent = latest("builtin-agent-sol-high-analyst");
        assert!(
            cap.data["skills"]
                .as_array()
                .unwrap()
                .contains(&json!(skill.id))
        );
        assert_eq!(agent.data["capability_ref"], cap.id);
        assert_eq!(agent.data["role_ref"], role.id);
        assert_eq!(agent.data["instructions"], role.data["instructions"]);
        assert!(
            state
                .definitions
                .iter()
                .any(|d| d.id == agent.id && d.body == role.data["instructions"].as_str().unwrap())
        );
        let task = latest("builtin-workflow-task-sol-high");
        let feature = latest("builtin-workflow-feature-sol-high");
        assert_eq!(feature.data["subworkflow_refs"]["task"], task.id);
        assert!(
            task.data["stages"][0]["description"]
                .as_str()
                .unwrap()
                .contains("## 输入与前置条件")
        );
        assert!(
            feature.data["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|step| {
                    step["spec_refs"].as_array().is_some_and(|refs| {
                        refs.contains(&json!(latest("builtin-spec-stage-prd").id))
                    })
                })
        );
        let count = state.governance.len();
        crate::seed_configuration(&mut state);
        assert_eq!(
            state.governance.len(),
            count,
            "reopening cannot keep publishing the same content"
        );
    }

    #[test]
    fn user_authored_workflow_and_agent_revisions_remain_authoritative() {
        let mut state = EngineeringState::default();
        crate::seed_configuration(&mut state);
        for id in [
            DEFAULT_TASK_WORKFLOW_V3_ID,
            default_configured_agent_id("analyst").as_str(),
        ] {
            let record = state.governance.iter_mut().find(|r| r.id == id).unwrap();
            record.created_at = 123;
            if record.kind == "workflow" {
                record.data["stages"][0]["description"] = json!("用户自己的阶段正文");
            } else {
                record.data["instructions"] = json!("用户自己的附加指令");
            }
            record.content_hash = configuration_hash(&record.data);
        }
        let before = json!(state.governance);
        upgrade(&mut state);
        assert_eq!(json!(state.governance), before);
    }

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
