//! Complete, immutable native defaults. Seeding performs no network, login or Docker I/O.
use crate::governance::{default_workflow, install_agent_profile, record};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

// A dataset may have explicitly compacted its defaults. Build the intermediate
// revisions needed by the seed recipes, but do not restore discarded history.
pub(crate) fn discard_reintroduced_history(
    s: &mut EngineeringState,
    existing: &[ConfigurationRecord],
) {
    fn strings(value: &Value, ids: &mut BTreeSet<String>) {
        match value {
            Value::String(id) => {
                ids.insert(id.clone());
            }
            Value::Array(values) => values.iter().for_each(|v| strings(v, ids)),
            Value::Object(values) => values.values().for_each(|v| strings(v, ids)),
            _ => {}
        }
    }
    fn replace(value: &mut Value, aliases: &BTreeMap<String, String>) {
        match value {
            Value::String(id) => {
                if let Some(latest) = aliases.get(id) {
                    *id = latest.clone();
                }
            }
            Value::Array(values) => values.iter_mut().for_each(|v| replace(v, aliases)),
            Value::Object(values) => values.values_mut().for_each(|v| replace(v, aliases)),
            _ => {}
        }
    }
    let ids: BTreeSet<_> = existing.iter().map(|r| r.id.clone()).collect();
    let mut referenced = BTreeSet::new();
    let mut latest = BTreeMap::new();
    for r in existing {
        strings(&r.data, &mut referenced);
        if r.is_definition() && r.definition_id.starts_with("builtin-") {
            let entry = latest
                .entry((r.kind.clone(), r.definition_id.clone()))
                .or_insert(r);
            if r.version > entry.version {
                *entry = r;
            }
        }
    }
    let mut aliases: BTreeMap<_, _> = s
        .governance
        .iter()
        .filter_map(|r| {
            if ids.contains(&r.id) || referenced.contains(&r.id) {
                return None;
            }
            let old = latest.get(&(r.kind.clone(), r.definition_id.clone()))?;
            let (_, version) = r.id.rsplit_once("-v")?;
            (version.parse::<u64>().ok()? < old.version).then(|| (r.id.clone(), old.id.clone()))
        })
        .collect();
    // Explicit offline compaction records what was removed. Ordinary startup must
    // not recreate those compatibility definitions or their temporary projections.
    let mut removed = BTreeSet::new();
    for receipt in existing.iter().filter(|r| r.kind == "catalog_compaction") {
        if let Some(values) = receipt.data["removed_ids"].as_array() {
            removed.extend(values.iter().filter_map(Value::as_str).filter(|id| !ids.contains(*id)).map(str::to_owned));
        }
        if let Some(values) = receipt.data["aliases"].as_object() {
            aliases.extend(values.iter().filter_map(|(old, target)| {
                let target = target.as_str()?;
                (!ids.contains(old) && ids.contains(target)).then(|| (old.clone(), target.to_owned()))
            }));
        }
    }
    removed.extend(aliases.keys().cloned());
    if removed.is_empty() { return; }
    for r in &s.governance {
        if !ids.contains(&r.id)
            && r.kind == "configuration_conversion"
            && r.data["source_workflow"]
                .as_str()
                .is_some_and(|id| aliases.contains_key(id))
        {
            removed.insert(r.id.clone());
            if let Some(id) = r.data["workflow_ref"]
                .as_str()
                .filter(|id| !ids.contains(*id))
            {
                removed.insert(id.into());
            }
        }
    }
    s.governance.retain(|r| !removed.contains(&r.id));
    s.definitions.retain(|r| !removed.contains(&r.id));
    for r in s.governance.iter_mut().filter(|r| !ids.contains(&r.id)) {
        replace(&mut r.data, &aliases);
        r.content_hash = configuration_hash(&r.data);
        if r.supersedes.as_ref().is_some_and(|id| removed.contains(id)) {
            r.supersedes = None;
        }
    }
    for d in &mut s.definitions {
        for slot in &mut d.slots {
            if let Some(id) = aliases.get(&slot.spec) {
                slot.spec = id.clone();
            }
        }
        if let Some(profile) = &mut d.agent_profile {
            for skill in &mut profile.skill_revisions {
                if let Some(id) = aliases.get(skill) {
                    *skill = id.clone();
                }
            }
        }
        if d.supersedes.as_ref().is_some_and(|id| removed.contains(id)) {
            d.supersedes = None;
        }
    }
}

pub(crate) fn seed(s: &mut EngineeringState, id: &str, kind: &str, name: &str, data: Value) {
    if s.governance.iter().any(|r| r.id == id) {
        return;
    }
    let (identity, version) = id.rsplit_once("-v").expect("versioned builtin ID");
    let requested_version: u64 = version.parse().expect("numeric builtin version");
    let previous = s
        .governance
        .iter()
        .filter(|r| r.kind == kind && r.definition_id == identity)
        .max_by_key(|r| r.version);
    let version = requested_version.max(previous.map_or(1, |r| r.version + 1));
    let mut r = record(
        kind,
        identity.into(),
        Scope::Global,
        name.into(),
        version,
        previous.map(|r| r.id.clone()),
        data,
        true,
    );
    r.id = id.into();
    r.created_at = 0;
    s.governance.push(r);
}
fn document(
    s: &mut EngineeringState,
    id: &str,
    family: &str,
    name: &str,
    body: &str,
    rules: Vec<String>,
    slots: Vec<SpecSlot>,
) {
    seed(
        s,
        id,
        family,
        name,
        json!({"body":body,"rules":rules,"slots":slots}),
    );
    if !s.definitions.iter().any(|r| r.id == id) {
        s.definitions.push(Definition {
            id: id.into(),
            family: family.into(),
            name: name.into(),
            version: s.configuration(id).unwrap().version,
            supersedes: s.configuration(id).unwrap().supersedes.clone(),
            body: body.into(),
            rules,
            slots,
            agent_profile: None,
        });
    }
}
pub(crate) fn seed_defaults(s: &mut EngineeringState) {
    seed(
        s,
        "builtin-harness-dsh-multi-provider-v1",
        "harness",
        "DeepSeek Harness 多模型服务商",
        json!({"provider":"dsh","runtime_version":DEFAULT_DSH_VERSION,"recipe_digest":default_dsh_recipe_digest(),"model_providers":["deepseek","zai-coding-cn","minimax-cn","ollama"],"capabilities":["structured-output","tool-calling","cancel","poll"]}),
    );
    let capabilities = vec!["structured-output", "tool-calling", "cancel", "poll"];
    seed(
        s,
        DEFAULT_MODEL_ID,
        "model",
        "GPT-6.1 Sol High",
        json!({"provider":"openai","model":"gpt-6.1-sol","parameters":{"reasoning_effort":"high"},"capabilities":["structured-output","tool-calling"]}),
    );
    seed(
        s,
        DEFAULT_HARNESS_ID,
        "harness",
        "Codex 默认 Harness",
        json!({"provider":"codex","runtime_version":DEFAULT_CODEX_VERSION,"recipe_digest":default_codex_recipe_digest(),"model_providers":["openai"],"capabilities":capabilities}),
    );
    if !s.harnesses.iter().any(|h| h.id == DEFAULT_HARNESS_ID) {
        s.harnesses.push(Harness {
            id: DEFAULT_HARNESS_ID.into(),
            provider: "codex".into(),
            version: DEFAULT_CODEX_VERSION.into(),
            capabilities: capabilities.iter().map(|v| (*v).into()).collect(),
            permission: Some(PermissionBinding {
                permission: Some(PermissionRef {
                    revision_id: "permission-container-full-access-v1".into(),
                }),
                overrides: PermissionPatch::default(),
            }),
            origin: Origin::External,
        });
    }
    if !s.sandboxes.iter().any(|b| b.id == DEFAULT_SANDBOX_ID) {
        s.sandboxes.push(Sandbox {
            id: DEFAULT_SANDBOX_ID.into(),
            provider: "docker".into(),
            version: "docker-worker/v1".into(),
            platform: "linux".into(),
            image: None,
            network: true,
            permission: Some(PermissionBinding {
                permission: Some(PermissionRef {
                    revision_id: "permission-container-full-access-v1".into(),
                }),
                overrides: PermissionPatch::default(),
            }),
            origin: Origin::External,
        });
    }
    for (id, name, implementation, input, output) in [(
        DEFAULT_NATIVE_TOOL_ID,
        "Codex 原生工具组",
        "harness.native",
        json!({"type":"object","description":"Native Codex tool contracts, constrained by the frozen role/work scope and Permission"}),
        json!({"type":"object"}),
    )] {
        seed(
            s,
            id,
            "tool",
            name,
            json!({"qualified_name":implementation,"implementation":implementation,"input_schema":input,"output_schema":output,"capabilities":[]}),
        );
    }
    seed(
        s,
        DEFAULT_DSH_NATIVE_TOOL_ID,
        "tool",
        "DSH 原生工具组",
        json!({
            "qualified_name":"dsh.native", "implementation":"harness.native", "harness_provider":"dsh",
            "input_schema":{"type":"object","description":"DSH native tools; constrained by frozen permissions and work scope"},
            "output_schema":{"type":"object"}, "capabilities":[]
        }),
    );
    let common = "builtin-skill-engineering-context-v2";
    document(
        s,
        common,
        "skill",
        "工程上下文与工作树规范",
        include_str!("../../../resources/configuration/skills/engineering-context.md"),
        vec![],
        vec![],
    );
    let roles = [
        (
            "analyst",
            "需求分析师",
            include_str!("../../../resources/configuration/roles/analyst.md"),
        ),
        (
            "architect",
            "架构师",
            include_str!("../../../resources/configuration/roles/architect.md"),
        ),
        (
            "developer",
            "开发",
            include_str!("../../../resources/configuration/roles/developer.md"),
        ),
        (
            "tester",
            "测试",
            include_str!("../../../resources/configuration/roles/tester.md"),
        ),
        (
            "reviewer",
            "评审",
            include_str!("../../../resources/configuration/roles/reviewer.md"),
        ),
    ];
    let mut agents = BTreeMap::new();
    let mut instances = BTreeMap::new();
    let mut skills = vec![common.to_string()];
    for (role, name, instructions) in roles {
        let skill = format!("builtin-skill-{role}-v1");
        document(
            s,
            &skill,
            "skill",
            &format!("{name}工作方法"),
            crate::shipped_prose::role_method(role),
            vec![],
            vec![],
        );
        skills.push(skill.clone());
        let role_id = default_role_id(role);
        seed(
            s,
            &role_id,
            "role",
            name,
            json!({"execution_role":role,"instructions":instructions}),
        );
        let aid = default_agent_id(role);
        seed(
            s,
            &aid,
            "agent",
            &format!("{name} · Sol High / Codex"),
            json!({"role":role,"role_ref":role_id,"instructions":instructions,"model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"skills":[common,skill],"tools":[DEFAULT_NATIVE_TOOL_ID],"permissions":["permission-container-full-access-v1","permission-container-full-access-v1"],"approval":"never"}),
        );
        let r = s.governance.iter().find(|r| r.id == aid).unwrap().clone();
        install_agent_profile(s, &r, &r.decode().expect("embedded Agent composition"));
        let iid = default_agent_instance_id(role);
        seed(
            s,
            &iid,
            "agent_instance",
            &format!("{name} · 默认全局实例"),
            json!({"agent_revision_id":aid,"scope":{"kind":"global"},"source_instance_id":null}),
        );
        agents.insert(role.to_string(), aid);
        instances.insert(role.to_string(), iid);
    }
    let mut spec_refs = vec![];
    for (purpose, name, body) in [
        (
            "planning",
            "需求与方案 Spec",
            include_str!("../../../resources/configuration/specs/legacy-planning.md"),
        ),
        (
            "implementation",
            "Task TDD Spec",
            include_str!("../../../resources/configuration/specs/legacy-implementation.md"),
        ),
        (
            "merge",
            "候选与合入 Spec",
            include_str!("../../../resources/configuration/specs/legacy-merge.md"),
        ),
    ] {
        let sid = format!("builtin-spec-{purpose}-v1");
        document(
            s,
            &sid,
            "spec",
            name,
            body,
            vec!["traceability".into()],
            vec![],
        );
        spec_refs.push(SpecSlot {
            purpose: purpose.into(),
            spec: sid,
        });
    }
    document(
        s,
        DEFAULT_STANDARD_ID,
        "standard",
        "默认工程 Standard",
        include_str!("../../../resources/configuration/standards/engineering.md"),
        vec![],
        spec_refs,
    );
    for (task, id, name) in [
        (
            true,
            DEFAULT_TASK_WORKFLOW_ID,
            "默认 Task 工作流 · Sol High / Codex",
        ),
        (
            false,
            DEFAULT_FEATURE_WORKFLOW_ID,
            "默认 Feature 工作流 · Sol High / Codex",
        ),
    ] {
        let mut workflow = default_workflow(task);
        for (role, slot) in &mut workflow.binding_slots {
            slot.agent_ref = agents.get(role).cloned();
        }
        if !task {
            workflow
                .subworkflow_refs
                .insert("task".into(), DEFAULT_TASK_WORKFLOW_ID.into());
        }
        seed(s, id, "workflow", name, json!(workflow));
    }
    for source in [DEFAULT_TASK_WORKFLOW_ID, DEFAULT_FEATURE_WORKFLOW_ID] {
        // Reference entries are selectable templates; no project is bound to them automatically.
        crate::workflow_definition::import_legacy_workflow(s, source, DEFAULT_STANDARD_ID)
            .expect("embedded reference workflow");
    }
    // Ready-to-adopt defaults remain ordinary independent definitions, with immutable references.
    let method = "builtin-skill-engineering-method-v1";
    document(
        s,
        method,
        "skill",
        "默认分阶段开发方法",
        include_str!("../../../resources/configuration/skills/engineering-method.md"),
        vec![],
        vec![],
    );
    let cap = CapabilityDefinition {
        model_ref: DEFAULT_MODEL_ID.into(),
        harness_ref: DEFAULT_HARNESS_ID.into(),
        tools: vec![DEFAULT_NATIVE_TOOL_ID.into()],
        skills: vec![common.into(), method.into()],
    };
    seed(
        s,
        DEFAULT_CAPABILITY_ID,
        "capability",
        "默认能力 · GPT-6.1 Sol High / Codex",
        json!(cap),
    );
    crate::default_catalog::seed_stage_catalog(s);
    for (source, id) in [
        (DEFAULT_TASK_WORKFLOW_ID, DEFAULT_TASK_WORKFLOW_V2_ID),
        (DEFAULT_FEATURE_WORKFLOW_ID, DEFAULT_FEATURE_WORKFLOW_V2_ID),
    ] {
        if s.governance.iter().any(|r| r.id == id) {
            continue;
        }
        let converted =
            crate::workflow_definition::import_legacy_workflow(s, source, DEFAULT_STANDARD_ID)
                .unwrap();
        let imported = s.configuration(&converted).unwrap().clone();
        let old = s.configuration(source).unwrap().clone();
        let mut doc: WorkflowDocument = imported.decode().unwrap();
        crate::default_catalog::stage_workflow(s, &mut doc);
        if doc.process_kind == "feature" {
            doc.subworkflow_refs
                .insert("task".into(), DEFAULT_TASK_WORKFLOW_V2_ID.into());
        }
        let mut revision = record(
            "workflow",
            old.definition_id,
            Scope::Global,
            if doc.process_kind == "feature" {
                "默认 Feature 开发流程"
            } else {
                "默认 Task 开发流程"
            }
            .into(),
            2,
            Some(old.id),
            json!(doc),
            true,
        );
        revision.id = id.into();
        revision.created_at = old.created_at;
        s.governance.push(revision);
    }
}
pub(crate) fn role_deleted(s: &EngineeringState, role: &ConfigurationRecord) -> bool {
    s.governance
        .iter()
        .any(|r| r.kind == "role_retirement" && r.definition_id == role.definition_id)
}

impl Engine {
    pub fn configuration_defaults(&self, project: Option<&str>) -> Result<Value> {
        let s = self.store.snapshot()?;
        let current = |id: &str| -> Result<String> {
            let exact = s.governance.iter().find(|r| r.id == id);
            let identity = exact.map(|r| r.definition_id.as_str()).unwrap_or_else(|| {
                id.rsplit_once("-v")
                    .map(|(identity, _)| identity)
                    .unwrap_or(id)
            });
            let latest = s
                .governance
                .iter()
                .filter(|r| r.definition_id == identity && r.is_definition())
                .max_by_key(|r| r.version);
            if let Some(revision) = latest.filter(|r| {
                r.created_at == 0
                    && r.id.contains("-prose-")
                    && !crate::configuration::retired(&s, r)
            }) {
                return Ok(revision.id.clone());
            }
            exact
                .or(latest)
                .map(|r| r.id.clone())
                .ok_or_else(|| err("not_found", format!("默认配置不存在: {id}")))
        };
        let model = current(DEFAULT_MODEL_ID)?;
        let harness = current(DEFAULT_HARNESS_ID)?;
        let workflow = current(DEFAULT_FEATURE_WORKFLOW_V3_ID)?;
        let task_workflow = current(DEFAULT_TASK_WORKFLOW_V3_ID)?;
        let capability = current(DEFAULT_CAPABILITY_ID)?;
        let scope = project
            .map(|id| Scope::Project {
                project_id: id.into(),
            })
            .unwrap_or(Scope::Global);
        let mut roles = serde_json::Map::new();
        let mut ids = vec![
            workflow.clone(),
            task_workflow.clone(),
            model.clone(),
            harness.clone(),
            capability.clone(),
        ];
        for role in [
            "analyst",
            "architect",
            "project-manager",
            "developer",
            "tester",
            "reviewer",
        ] {
            let r = s.configuration(&current(&default_role_id(role))?)?;
            let agent = current(&default_configured_agent_id(role))?;
            let configured = crate::governance::reference(&s, &agent, "agent", &scope)?;
            crate::governance::validate_composition(&s, &configured.decode()?, &scope)?;
            roles.insert(r.definition_id.clone(), json!(agent));
            ids.push(agent);
        }
        crate::governance::reference(&s, &workflow, "workflow", &scope)?;
        fn collect(value: &Value, ids: &mut Vec<String>) {
            match value {
                Value::String(id) => {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        collect(value, ids);
                    }
                }
                Value::Object(values) => {
                    for value in values.values() {
                        collect(value, ids);
                    }
                }
                _ => {}
            }
        }
        loop {
            let before = ids.len();
            for record in &s.governance {
                if ids.contains(&record.id) || ids.contains(&record.definition_id) {
                    collect(&record.data, &mut ids);
                }
            }
            if before == ids.len() {
                break;
            }
        }

        Ok(
            json!({"workflow_ref":workflow,"task_workflow_ref":task_workflow,"role_agent_bindings":roles,"standard_ref":null,"capability_ref":capability,"model_ref":model,"harness_ref":harness,"sandbox_ref":DEFAULT_SANDBOX_ID,"permission_refs":["permission-container-full-access-v1","permission-container-full-access-v1"],"records":s.governance.iter().filter(|r|ids.contains(&r.id)||ids.contains(&r.definition_id)).collect::<Vec<_>>(),"permissions":s.permissions.iter().filter(|p|ids.contains(&p.id)).collect::<Vec<_>>(),"sandboxes":s.sandboxes.iter().filter(|b|b.id==DEFAULT_SANDBOX_ID).collect::<Vec<_>>(),"materializations":s.governance.iter().filter(|r|r.kind=="harness_materialization"&&(r.definition_id==harness||r.definition_id==DEFAULT_HARNESS_ID)).collect::<Vec<_>>() }),
        )
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;

    #[test]
    fn explicit_compaction_does_not_reintroduce_removed_compatibility_history() {
        let mut state = EngineeringState::default();
        crate::seed_configuration(&mut state);
        let original = state.governance.clone();
        let mut newest = BTreeMap::new();
        for record in original.iter().filter(|r| r.is_definition()) {
            let entry = newest.entry(record.definition_id.clone()).or_insert(record.clone());
            if record.version > entry.version { *entry = record.clone(); }
        }
        let aliases: BTreeMap<_, _> = original.iter().filter(|r| r.is_definition())
            .filter_map(|r| {let latest = &newest[&r.definition_id]; (latest.id != r.id).then(|| (r.id.clone(), latest.id.clone()))}).collect();
        let mut removed: BTreeSet<_> = aliases.keys().cloned().collect();
        let kept: BTreeSet<_> = newest.values().map(|r| r.id.clone()).collect();
        removed.extend(state.definitions.iter().filter(|r| !kept.contains(&r.id)).map(|r| r.id.clone()));
        state.governance.retain(|r| !removed.contains(&r.id));
        state.definitions.retain(|r| kept.contains(&r.id));
        for r in &mut state.governance { if r.supersedes.as_ref().is_some_and(|id| removed.contains(id)) {r.supersedes = None;} }
        for r in &mut state.definitions { if r.supersedes.as_ref().is_some_and(|id| removed.contains(id)) {r.supersedes = None;} }
        state.governance.push(record("catalog_compaction", "catalog-compaction".into(), Scope::Global,
            "显式目录整理".into(), 1, None, json!({"removed_ids":removed,"aliases":aliases}), true));
        assert!(state.definitions.iter().any(|definition| definition.id == crate::binding(&state).agent));
        let before = state.clone();
        crate::seed_configuration(&mut state);
        state.validate_governance(&before).unwrap();
        assert_eq!(json!(state.governance), json!(before.governance));
        assert_eq!(json!(state.definitions), json!(before.definitions));
        crate::seed_configuration(&mut state);
        assert_eq!(json!(state.governance), json!(before.governance));
    }
}
