//! The released current catalog. Markdown is embedded at build time; no upgrade chain or runtime file reads.
use crate::*;

pub(crate) fn catalog() -> EngineeringState {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../resources/configuration/catalog.json"
    ))
    .expect("current catalog");
    fn expand(value: &mut Value) {
        match value {
            Value::Object(fields) if fields.len() == 1 && fields.contains_key("$markdown") => {
                let path = fields["$markdown"].as_str().expect("Markdown path");
                let text = match path {
                    "deliveries/design.md" => {
                        include_str!("../../../resources/configuration/deliveries/design.md")
                    }
                    "deliveries/diff.md" => {
                        include_str!("../../../resources/configuration/deliveries/diff.md")
                    }
                    "deliveries/prd.md" => {
                        include_str!("../../../resources/configuration/deliveries/prd.md")
                    }
                    "deliveries/test-report.md" => {
                        include_str!("../../../resources/configuration/deliveries/test-report.md")
                    }
                    "roles/analyst.md" => {
                        include_str!("../../../resources/configuration/roles/analyst.md")
                    }
                    "roles/architect.md" => {
                        include_str!("../../../resources/configuration/roles/architect.md")
                    }
                    "roles/developer.md" => {
                        include_str!("../../../resources/configuration/roles/developer.md")
                    }
                    "roles/project-manager.md" => {
                        include_str!("../../../resources/configuration/roles/project-manager.md")
                    }
                    "roles/reviewer.md" => {
                        include_str!("../../../resources/configuration/roles/reviewer.md")
                    }
                    "roles/tester.md" => {
                        include_str!("../../../resources/configuration/roles/tester.md")
                    }
                    "skills/architecture.md" => {
                        include_str!("../../../resources/configuration/skills/architecture.md")
                    }
                    "skills/engineering-context.md" => include_str!(
                        "../../../resources/configuration/skills/engineering-context.md"
                    ),
                    "skills/engineering-method.md" => include_str!(
                        "../../../resources/configuration/skills/engineering-method.md"
                    ),
                    "skills/independent-review.md" => include_str!(
                        "../../../resources/configuration/skills/independent-review.md"
                    ),
                    "skills/prd.md" => {
                        include_str!("../../../resources/configuration/skills/prd.md")
                    }
                    "skills/project-planning.md" => {
                        include_str!("../../../resources/configuration/skills/project-planning.md")
                    }
                    "skills/release-verification.md" => include_str!(
                        "../../../resources/configuration/skills/release-verification.md"
                    ),
                    "skills/tdd-implementation.md" => include_str!(
                        "../../../resources/configuration/skills/tdd-implementation.md"
                    ),
                    "skills/test-design.md" => {
                        include_str!("../../../resources/configuration/skills/test-design.md")
                    }
                    "specs/architecture.md" => {
                        include_str!("../../../resources/configuration/specs/architecture.md")
                    }
                    "specs/closure.md" => {
                        include_str!("../../../resources/configuration/specs/closure.md")
                    }
                    "specs/implementation.md" => {
                        include_str!("../../../resources/configuration/specs/implementation.md")
                    }
                    "specs/prd.md" => include_str!("../../../resources/configuration/specs/prd.md"),
                    "specs/project-planning.md" => {
                        include_str!("../../../resources/configuration/specs/project-planning.md")
                    }
                    "specs/release.md" => {
                        include_str!("../../../resources/configuration/specs/release.md")
                    }
                    "specs/solution.md" => {
                        include_str!("../../../resources/configuration/specs/solution.md")
                    }
                    "specs/test-design.md" => {
                        include_str!("../../../resources/configuration/specs/test-design.md")
                    }
                    "standards/engineering.md" => {
                        include_str!("../../../resources/configuration/standards/engineering.md")
                    }
                    "tasks/attempt-closure.md" => {
                        include_str!("../../../resources/configuration/tasks/attempt-closure.md")
                    }
                    "tasks/attempt-implementation.md" => include_str!(
                        "../../../resources/configuration/tasks/attempt-implementation.md"
                    ),
                    "tasks/attempt-planning.md" => {
                        include_str!("../../../resources/configuration/tasks/attempt-planning.md")
                    }
                    "tasks/attempt-requirement.md" => include_str!(
                        "../../../resources/configuration/tasks/attempt-requirement.md"
                    ),
                    "tasks/author-feature-tests.md" => include_str!(
                        "../../../resources/configuration/tasks/author-feature-tests.md"
                    ),
                    "tasks/author-task-tests.md" => {
                        include_str!("../../../resources/configuration/tasks/author-task-tests.md")
                    }
                    "tasks/design.md" => {
                        include_str!("../../../resources/configuration/tasks/design.md")
                    }
                    "tasks/discussion-tests.md" => {
                        include_str!("../../../resources/configuration/tasks/discussion-tests.md")
                    }
                    "tasks/docs.md" => {
                        include_str!("../../../resources/configuration/tasks/docs.md")
                    }
                    "tasks/implement.md" => {
                        include_str!("../../../resources/configuration/tasks/implement.md")
                    }
                    "tasks/planning.md" => {
                        include_str!("../../../resources/configuration/tasks/planning.md")
                    }
                    "tasks/prd.md" => include_str!("../../../resources/configuration/tasks/prd.md"),
                    "tasks/review-candidate-feature.md" => include_str!(
                        "../../../resources/configuration/tasks/review-candidate-feature.md"
                    ),
                    "tasks/review-candidate-task.md" => include_str!(
                        "../../../resources/configuration/tasks/review-candidate-task.md"
                    ),
                    "tasks/review-feature.md" => {
                        include_str!("../../../resources/configuration/tasks/review-feature.md")
                    }
                    "tasks/review-plan.md" => {
                        include_str!("../../../resources/configuration/tasks/review-plan.md")
                    }
                    "tasks/review-requirement.md" => {
                        include_str!("../../../resources/configuration/tasks/review-requirement.md")
                    }
                    "tasks/review-task.md" => {
                        include_str!("../../../resources/configuration/tasks/review-task.md")
                    }
                    "tasks/validate-plan.md" => {
                        include_str!("../../../resources/configuration/tasks/validate-plan.md")
                    }
                    "tasks/validate-requirement.md" => include_str!(
                        "../../../resources/configuration/tasks/validate-requirement.md"
                    ),
                    "tasks/validate-tests.md" => {
                        include_str!("../../../resources/configuration/tasks/validate-tests.md")
                    }
                    _ => panic!("unknown embedded catalog Markdown: {path}"),
                };
                *value = json!(text);
            }
            Value::Object(fields) => fields.values_mut().for_each(expand),
            Value::Array(items) => items.iter_mut().for_each(expand),
            _ => {}
        }
    }
    expand(&mut value);
    let mut defaults = json!(EngineeringState::default());
    for (key, value) in value.as_object().expect("catalog object") {
        if key != "schema_version" {
            defaults[key] = value.clone();
        }
    }
    let mut state: EngineeringState =
        serde_json::from_value(defaults).expect("current configuration types");
    for record in &mut state.governance {
        record.content_hash = configuration_hash(&record.data);
    }
    state
}
