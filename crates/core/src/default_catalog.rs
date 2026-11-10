//! Shipped Markdown methods and stage contracts. No runtime or network side effects.
use crate::governance::record;
use crate::*;
use std::collections::BTreeMap;
fn entry(s: &mut EngineeringState, id: &str, kind: &str, name: &str, data: Value) {
    crate::builtin_defaults::seed(s, id, kind, name, data);
}
pub(crate) fn seed_stage_catalog(s: &mut EngineeringState) {
    crate::deliverables::seed(s);
    let skills = [
        (
            "prd",
            "PRD 需求分析",
            include_str!("../../../resources/configuration/skills/prd.md"),
        ),
        (
            "architecture",
            "Design Doc 架构设计",
            include_str!("../../../resources/configuration/skills/architecture.md"),
        ),
        (
            "project-planning",
            "Task Plan 项目管理",
            include_str!("../../../resources/configuration/skills/project-planning.md"),
        ),
        (
            "test-design",
            "独立测试与 TDD",
            include_str!("../../../resources/configuration/skills/test-design.md"),
        ),
        (
            "tdd-implementation",
            "Task 实现与文档",
            include_str!("../../../resources/configuration/skills/tdd-implementation.md"),
        ),
        (
            "release-verification",
            "Feature 与候选验证",
            include_str!("../../../resources/configuration/skills/release-verification.md"),
        ),
        (
            "independent-review",
            "分阶段独立评审",
            include_str!("../../../resources/configuration/skills/independent-review.md"),
        ),
    ];
    for (key, name, body) in skills {
        let id = format!("builtin-skill-{key}-v2");
        entry(s, &id, "skill", name, json!({"body":body,"rules":[]}));
        if !s.definitions.iter().any(|r| r.id == id) {
            s.definitions.push(Definition {
                id: id.clone(),
                family: "skill".into(),
                name: name.into(),
                version: s.configuration(&id).unwrap().version,
                supersedes: s.configuration(&id).unwrap().supersedes.clone(),
                body: body.into(),
                rules: vec![],
                slots: vec![],
                agent_profile: None,
            });
        }
    }
    let specs = [
        (
            "prd",
            "PRD 需求规范",
            include_str!("../../../resources/configuration/specs/prd.md"),
        ),
        (
            "architecture",
            "Design Doc 设计规范",
            include_str!("../../../resources/configuration/specs/architecture.md"),
        ),
        (
            "project-planning",
            "Task Plan 拆分规范",
            include_str!("../../../resources/configuration/specs/project-planning.md"),
        ),
        (
            "test-design",
            "Test Plan 测试设计规范",
            include_str!("../../../resources/configuration/specs/test-design.md"),
        ),
        (
            "solution",
            "方案交付规范",
            include_str!("../../../resources/configuration/specs/solution.md"),
        ),
        (
            "implementation",
            "Task TDD 交付规范",
            include_str!("../../../resources/configuration/specs/implementation.md"),
        ),
        (
            "closure",
            "Feature 完整验收规范",
            include_str!("../../../resources/configuration/specs/closure.md"),
        ),
        (
            "release",
            "候选与合入规范",
            include_str!("../../../resources/configuration/specs/release.md"),
        ),
    ];
    for (key, name, body) in specs {
        let contexts: &[(&str, Option<&str>, Option<&str>)] = match key {
            "prd" => &[
                ("agent.interact", Some("requirement"), None),
                ("attempt.execute", Some("requirement_definition"), None),
                ("delivery.review", None, Some("requirement")),
            ],
            "architecture" => &[("agent.interact", Some("design"), None)],
            "project-planning" => &[("agent.interact", Some("task_plan"), None)],
            "test-design" => &[("agent.interact", Some("test_plan"), None)],
            "solution" => &[
                ("attempt.execute", Some("solution_planning"), None),
                ("delivery.review", None, Some("plan")),
            ],
            "implementation" => &[
                ("attempt.execute", Some("implementation"), None),
                ("delivery.review", None, Some("implementation")),
            ],
            "closure" => &[
                ("attempt.execute", Some("closure"), None),
                ("delivery.review", None, Some("closure")),
            ],
            _ => &[("merge.prepare", None, None), ("merge.review", None, None)],
        };
        let selectors: Vec<_> = contexts
            .iter()
            .map(|(kind, phase, delivery)| ActionSelector {
                kind: (*kind).into(),
                phase: phase.map(str::to_owned),
                delivery_kind: delivery.map(str::to_owned),
            })
            .collect();
        let id = format!("builtin-spec-stage-{key}-v1");
        entry(
            s,
            &id,
            "spec",
            name,
            json!({"body":body,"applicable_actions":selectors,"rules":["traceability","failure_paths","sections"]}),
        );
        if !s.definitions.iter().any(|r| r.id == id) {
            s.definitions.push(Definition {
                id,
                family: "spec".into(),
                name: name.into(),
                version: 1,
                supersedes: None,
                body: body.into(),
                rules: vec![
                    "traceability".into(),
                    "failure_paths".into(),
                    "sections".into(),
                ],
                slots: vec![],
                agent_profile: None,
            });
        }
    }
    entry(
        s,
        &default_role_id("project-manager"),
        "role",
        "项目经理",
        json!({"execution_role":"architect","instructions":include_str!("../../../resources/configuration/roles/project-manager.md"),"visibility":{"kind":"global"}}),
    );
    for (role, name, methods) in [
        ("analyst", "需求分析师", vec!["prd"]),
        ("architect", "架构师", vec!["architecture"]),
        ("project-manager", "项目经理", vec!["project-planning"]),
        ("developer", "开发", vec!["tdd-implementation"]),
        (
            "tester",
            "测试",
            vec!["test-design", "release-verification"],
        ),
        ("reviewer", "评审", vec!["independent-review"]),
    ] {
        let cap_id = format!("builtin-capability-{role}-codex-v2");
        let mut skills = vec!["builtin-skill-engineering-context-v2".into()];
        skills.extend(methods.iter().map(|key| format!("builtin-skill-{key}-v2")));
        let cap = CapabilityDefinition {
            model_ref: DEFAULT_MODEL_ID.into(),
            harness_ref: DEFAULT_HARNESS_ID.into(),
            tools: vec![DEFAULT_NATIVE_TOOL_ID.into()],
            skills,
        };
        entry(
            s,
            &cap_id,
            "capability",
            &format!("{name}能力 · Sol High / Codex"),
            json!(cap),
        );
        let role_ref = default_role_id(role);
        let definition = s.configuration(&role_ref).unwrap().clone();
        let duty: RoleDefinition = definition.decode().unwrap();
        let composition = AgentComposition {
            permission_selection: None,
            role: duty.execution_role,
            role_ref: Some(role_ref),
            capability_ref: Some(cap_id),
            instructions: duty.instructions,
            model_ref: cap.model_ref,
            harness_ref: cap.harness_ref,
            tools: cap.tools,
            skills: cap.skills,
            permissions: vec![
                "permission-container-full-access-v1".into(),
                "permission-container-full-access-v1".into(),
            ],
            approval: ApprovalPolicy::Never,
        };
        let id = default_configured_agent_id(role);
        if !s.governance.iter().any(|r| r.id == id) {
            let identity = format!("builtin-agent-sol-high-{role}");
            let previous = s
                .governance
                .iter()
                .filter(|r| r.kind == "agent" && r.definition_id == identity)
                .max_by_key(|r| r.version);
            let mut revision = record(
                "agent",
                previous
                    .map(|r| r.definition_id.clone())
                    .unwrap_or_else(|| format!("builtin-agent-sol-high-{role}")),
                Scope::Global,
                format!("{name} · Sol High / Codex"),
                previous.map_or(3, |r| (r.version + 1).max(3)),
                previous.map(|r| r.id.clone()),
                json!(composition),
                true,
            );
            revision.id = id.clone();
            revision.created_at = 0;
            s.governance.push(revision);
        }
        let r = s.configuration(&id).unwrap().clone();
        crate::governance::install_agent_profile(s, &r, &composition);
    }
}
pub(crate) fn stage_description(key: &str) -> &'static str {
    match key {
        "requirements" => include_str!("../../../resources/configuration/specs/prd.md"),
        "architecture" => include_str!("../../../resources/configuration/specs/architecture.md"),
        "planning" => include_str!("../../../resources/configuration/specs/project-planning.md"),
        "test_design" => include_str!("../../../resources/configuration/specs/test-design.md"),
        "solution_validation" => include_str!("../../../resources/configuration/specs/solution.md"),
        "closure" => include_str!("../../../resources/configuration/specs/closure.md"),
        "development" | "implementation" | "review" => {
            include_str!("../../../resources/configuration/specs/implementation.md")
        }
        _ => include_str!("../../../resources/configuration/specs/release.md"),
    }
}
pub(crate) fn stage_workflow(s: &EngineeringState, doc: &mut WorkflowDocument) {
    let feature = doc.process_kind == "feature";
    let stages = if feature {
        vec![
            ("requirements", "需求分析与 PRD"),
            ("architecture", "架构设计与 Design Doc"),
            ("planning", "项目管理与 Task Plan"),
            ("test_design", "测试设计与 Test Plan"),
            ("solution_validation", "方案交付与评审"),
            ("development", "Task 并行开发"),
            ("closure", "Feature 收尾与验收"),
            ("release", "候选、确认与交付"),
        ]
    } else {
        vec![
            ("implementation", "Task TDD 实现"),
            ("review", "Task 独立评审"),
            ("integration", "候选验证与汇合"),
            ("snapshot", "交付快照"),
        ]
    };
    doc.stages = stages
        .into_iter()
        .map(|(key, name)| WorkflowStage {
            key: key.into(),
            name: name.into(),
            description: stage_description(key).into(),
        })
        .collect();
    for step in &mut doc.steps {
        let (stage, spec) = if !feature {
            match step.kind.as_str() {
                "attempt.execute" => ("implementation", Some("implementation")),
                "delivery.review" => ("review", Some("implementation")),
                "snapshot.capture" => ("snapshot", None),
                _ => (
                    "integration",
                    ["merge.prepare", "merge.review"]
                        .contains(&step.kind.as_str())
                        .then_some("release"),
                ),
            }
        } else {
            match step.key.as_str() {
                "initial_snapshot"
                | "requirement_discussion"
                | "requirement_adopt"
                | "requirement_execute"
                | "requirement_review" => ("requirements", Some("prd")),
                "design_discussion" | "design_adopt" => ("architecture", Some("architecture")),
                "task_discussion" | "task_adopt" => ("planning", Some("project-planning")),
                "test_discussion" | "test_adopt" => ("test_design", Some("test-design")),
                "solution_execute" | "solution_review" => ("solution_validation", Some("solution")),
                "tasks" => ("development", None),
                "closure" | "closure_review" => ("closure", Some("closure")),
                _ => ("release", Some("release")),
            }
        };
        step.stage = stage.into();
        if [
            "agent.interact",
            "attempt.execute",
            "delivery.review",
            "merge.prepare",
            "merge.review",
        ]
        .contains(&step.kind.as_str())
        {
            step.spec_refs = spec
                .map(|key| vec![format!("builtin-spec-stage-{key}-v1")])
                .unwrap_or_default();
        }
        if feature && ["task_discussion", "task_adopt"].contains(&step.key.as_str()) {
            let pm = s
                .configuration(&default_role_id("project-manager"))
                .unwrap();
            step.role_ref = Some(pm.definition_id.clone());
            step.participant_slot = Some(pm.definition_id.clone());
            step.name = if step.kind == "agent.interact" {
                "项目经理拆分 Task Plan"
            } else {
                "采纳 Task Plan"
            }
            .into();
        }
        if step.kind == "agent.interact" {
            let spec = s.configuration(&step.spec_refs[0]).unwrap();
            step.interaction.goal = format!(
                "{}\n\n## 阶段交付要求\n\n{}",
                step.interaction.goal,
                spec.data["body"].as_str().unwrap()
            );
        }
    }
    // Source definitions do not persist a project's Agent choices.
    doc.binding_slots = BTreeMap::new();
}
