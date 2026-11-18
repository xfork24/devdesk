//! Materialize project choices into immutable source revisions; execution never applies overlays.
use crate::governance::{record, record_with_refs, reference};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StandardOverride {
    pub source_ref: Id,
    pub replacement_ref: Id,
}

pub(crate) struct ProjectStandard {
    pub standard: ConfigurationRecord,
    pub entry: Id,
    pub snapshot: StandardSnapshot,
    pub provenance: Value,
    pub rewrites: BTreeMap<Id, Id>,
}

fn derived_source(
    state: &mut EngineeringState,
    scope: &Scope,
    original: &ConfigurationRecord,
    refs: References,
    data: Value,
) -> Result<ConfigurationRecord> {
    let project = scope.project_id().unwrap_or("global");
    let prefix = if scope.project_id().is_some() {
        "project-source"
    } else {
        "standard-source"
    };
    let identity = format!(
        "{prefix}.{}",
        configuration_hash(&(project, &original.id, &refs, &data))
    );
    let id = source_reference(&identity, 1)?;
    if let Ok(existing) = state.configuration(&id) {
        require(
            existing.r#type == original.r#type
                && existing.scope == *scope
                && existing.refs == refs
                && existing.data == data
                && existing.immutable,
            "项目派生源引用与已有定义冲突",
        )?;
        return Ok(existing.clone());
    }
    let source = record_with_refs(
        &original.r#type,
        identity,
        scope.clone(),
        format!(
            "{} · {}定制",
            original.name,
            if scope.project_id().is_some() {
                "项目"
            } else {
                "规范"
            }
        ),
        1,
        None,
        refs,
        data,
        true,
    );
    validate_source_data(&source.r#type, &source.refs, &source.data)?;
    state.governance.push(source.clone());
    Ok(source)
}

fn rewrite_source(
    state: &mut EngineeringState,
    scope: &Scope,
    original: &str,
    overrides: &BTreeMap<Id, Id>,
    active: &mut BTreeSet<Id>,
    resolved: &mut BTreeMap<Id, Id>,
) -> Result<Id> {
    if let Some(id) = resolved.get(original) {
        return Ok(id.clone());
    }
    require(active.insert(original.into()), "项目覆盖产生了循环引用")?;
    require(
        resolved.len() + active.len() <= WORKFLOW_STEP_LIMIT * 10,
        "项目规范闭包过大",
    )?;
    require(active.len() <= WORKFLOW_DEPTH_LIMIT * 4, "项目覆盖引用过深")?;
    let selected = overrides
        .get(original)
        .map(String::as_str)
        .unwrap_or(original);
    let source = state.configuration(selected)?.clone();
    let mut wire = json!(source);
    // Stable node identities survive a derived Workflow reference.
    if source.r#type == "workflow" {
        let mut doc: WorkflowDefinition = source.decode()?;
        doc.assign_node_ids(&source.id);
        wire["data"] = json!(doc);
    }
    let mut changed = false;
    for edge in configuration_edges(&source)?
        .into_iter()
        .filter(|e| ["workflow", "delivery_type", "spec"].contains(&e.expected_type.as_str()))
    {
        let replacement = rewrite_source(state, scope, &edge.to, overrides, active, resolved)?;
        if replacement != edge.to {
            *wire
                .pointer_mut(&edge.path)
                .ok_or_else(|| err("invalid_reference", "覆盖引用位置不存在"))? =
                json!(replacement);
            changed = true;
        }
    }
    let result = if changed {
        let refs = source
            .refs
            .keys()
            .map(|key| (key.clone(), wire[key].clone()))
            .collect();
        derived_source(state, scope, &source, refs, wire["data"].clone())?.id
    } else {
        source.id
    };
    active.remove(original);
    resolved.insert(original.into(), result.clone());
    Ok(result)
}

pub(crate) struct StandardReplacement {
    pub roots: Vec<Id>,
    pub snapshot: StandardSnapshot,
    pub provenance: Value,
    pub rewrites: BTreeMap<Id, Id>,
}

/// Apply substitutions once across the entire immutable normative graph.
pub(crate) fn replace_standard_sources(
    state: &mut EngineeringState,
    base: &str,
    scope: &Scope,
    raw_overrides: &Value,
) -> Result<StandardReplacement> {
    reference(state, base, "standard", scope)?;
    let before = standard_snapshot(state, base, scope)?;
    let mut overrides: Vec<StandardOverride> = serde_json::from_value(raw_overrides.clone())
        .map_err(|e| err("invalid_params", e.to_string()))?;
    require(overrides.len() <= 1000, "覆盖项超过上限")?;
    overrides.sort_by(|a, b| a.source_ref.cmp(&b.source_ref));
    let mut substitutions = BTreeMap::new();
    for item in &overrides {
        let old = before
            .nodes
            .iter()
            .find(|n| n.reference == item.source_ref)
            .ok_or_else(|| err("invalid_override", "覆盖项不属于基础 Standard"))?;
        let replacement = reference(state, &item.replacement_ref, &old.kind, scope)?;
        require(
            replacement.immutable && item.source_ref != item.replacement_ref,
            "请选择不同的已发布修订",
        )?;
        require(
            substitutions
                .insert(item.source_ref.clone(), item.replacement_ref.clone())
                .is_none(),
            "同一规范项不能重复覆盖",
        )?;
    }
    let mut resolved = BTreeMap::new();
    let mut roots = Vec::new();
    for root in &before.root_workflow_refs {
        roots.push(rewrite_source(
            state,
            scope,
            root,
            &substitutions,
            &mut BTreeSet::new(),
            &mut resolved,
        )?);
    }
    roots.sort();
    roots.dedup();
    let after = resolve_standard_sources(state, &roots, scope)?;
    // An ancestor override must not silently discard another requested override.
    for item in &overrides {
        let applied = resolved.get(&item.source_ref).ok_or_else(|| {
            err(
                "invalid_override",
                "某个覆盖项被其他覆盖移除，请调整覆盖范围",
            )
        })?;
        require(
            after.nodes.iter().any(|n| &n.reference == applied),
            "某个覆盖项被其他覆盖移除，请调整覆盖范围",
        )?;
    }
    let rewrites: BTreeMap<_, _> = resolved
        .into_iter()
        .filter(|(old, new)| old != new)
        .collect();
    let diff = diff_standard_snapshots(&before, &after);
    let override_sources = overrides
        .iter()
        .map(|item| {
            let source = state.configuration(&item.replacement_ref)?;
            Ok(json!({"kind":source.r#type,"reference":source.id}))
        })
        .collect::<Result<Vec<_>>>()?;
    let provenance = json!({"base_standard_ref":base,"overrides":overrides,"override_sources":override_sources,"source_rewrites":rewrites,"diff":diff,"base_digest":before.snapshot_digest,"snapshot_digest":after.snapshot_digest});
    Ok(StandardReplacement {
        roots,
        snapshot: after,
        provenance,
        rewrites,
    })
}

pub(crate) fn materialize_project_standard(
    state: &mut EngineeringState,
    project: &str,
    base: &str,
    raw_overrides: &Value,
) -> Result<ProjectStandard> {
    require(state.projects.iter().any(|p| p.id == project), "项目不存在")?;
    let scope = Scope::project(project);
    let source = reference(state, base, "standard", &scope)?.clone();
    let before = standard_snapshot(state, base, &scope)?;
    let default = standard_default_workflow(state, base, &scope).or_else(|error| {
        // Preserve an explicitly recorded legacy entry without guessing from root order.
        let previous = state
            .governance
            .iter()
            .filter(|r| {
                r.r#type == "project_configuration"
                    && r.definition_id == project
                    && r.data["standard_ref"] == base
            })
            .max_by_key(|r| r.version)
            .and_then(|r| r.data["workflow_ref"].as_str().map(str::to_owned));
        let defaults = current_template_defaults(state);
        let recorded = previous.or_else(|| {
            defaults
                .as_ref()
                .filter(|d| d["standard_ref"] == base)
                .and_then(|d| d["workflow_ref"].as_str().map(str::to_owned))
        });
        recorded
            .filter(|id| before.root_workflow_refs.contains(id))
            .ok_or(error)
    })?;
    let result = replace_standard_sources(state, base, &scope, raw_overrides)?;
    let entry = result.rewrites.get(&default).cloned().unwrap_or(default);
    let overrides = &result.provenance["overrides"];
    if overrides.as_array().is_some_and(Vec::is_empty) {
        return Ok(ProjectStandard {
            standard: source,
            entry,
            snapshot: result.snapshot,
            provenance: result.provenance,
            rewrites: result.rewrites,
        });
    }
    let roots = &result.roots;
    let refs = References::from([
        ("root_workflow_refs".into(), json!(roots)),
        ("default_workflow_ref".into(), json!(entry)),
        ("base_standard_ref".into(), json!(base)),
    ]);
    let identity = format!("project-standard.{project}.{}", source.definition_id);
    let existing = state
        .governance
        .iter()
        .find(|r| {
            r.r#type == "standard"
                && r.definition_id == identity
                && r.refs == refs
                && state.governance.iter().any(|meta| {
                    meta.r#type == "standard_derivation"
                        && meta.definition_id == r.id
                        && meta.data["overrides"] == json!(overrides)
                })
        })
        .cloned();
    let standard = if let Some(existing) = existing {
        existing
    } else {
        let previous = state
            .governance
            .iter()
            .filter(|r| r.r#type == "standard" && r.definition_id == identity)
            .max_by_key(|r| r.version)
            .cloned();
        let standard = record_with_refs(
            "standard",
            identity,
            scope.clone(),
            format!("{} · 项目快照", source.name),
            previous.as_ref().map_or(1, |r| r.version + 1),
            previous.map(|r| r.id),
            refs,
            json!({"schema_version":4}),
            true,
        );
        state.governance.push(standard.clone());
        standard
    };
    let provenance = result.provenance;
    if !state
        .governance
        .iter()
        .any(|r| r.r#type == "standard_derivation" && r.definition_id == standard.id)
    {
        state.governance.push(record(
            "standard_derivation",
            standard.id.clone(),
            scope,
            "项目规范来源与差异".into(),
            1,
            None,
            provenance.clone(),
            true,
        ));
    }
    Ok(ProjectStandard {
        standard,
        entry,
        snapshot: result.snapshot,
        provenance,
        rewrites: result.rewrites,
    })
}

pub(crate) fn project_standard_permissions(
    state: &mut EngineeringState,
    project: &str,
    profile: &str,
    _rewrites: &BTreeMap<Id, Id>,
) -> Result<Id> {
    reference(
        state,
        profile,
        "permission_profile",
        &Scope::project(project),
    )?;
    Ok(profile.into())
}

impl Engine {
    pub(crate) fn project_standard_preview(
        &self,
        mut state: EngineeringState,
        p: &Value,
    ) -> Result<Value> {
        let project = text(p, "project_id")?;
        require(
            self.store
                .project_id()
                .as_deref()
                .is_none_or(|id| id == project),
            "不能预览其他项目的快照",
        )?;
        let base = text(p, "standard_ref")?;
        let base_snapshot = standard_snapshot(&state, &base, &Scope::project(&project))?;
        let baseline = json!({"nodes":base_snapshot.nodes.iter().map(|n|json!({"reference":n.reference,"kind":n.kind,"name":n.name})).collect::<Vec<_>>(),"default_workflow_ref":standard_default_workflow(&state,&base,&Scope::project(&project)).ok()});
        let result = materialize_project_standard(
            &mut state,
            &project,
            &base,
            p.get("standard_overrides").unwrap_or(&json!([])),
        )?;
        let source_context = p["harness_permission_ref"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| selected_harness_profile(&state).map(|r| r.id.clone()))
            .ok_or_else(|| err("permission_unconfigured", "请选择权限 Profile"))?;
        let context =
            project_standard_permissions(&mut state, &project, &source_context, &result.rewrites)?;
        let mut selection = json!({"harness_permission_ref":context});
        selection["standard_ref"] = json!(result.standard.id);
        let selected = crate::standards::selection_state(&state, &selection);
        let plan = self.compile_configured_workflow(
            &selected,
            selected.configuration(&result.entry)?,
            &Scope::project(&project),
            None,
        )?;
        let hash = configuration_hash(&(&result.entry, &result.provenance, &context));
        Ok(
            json!({"standard_ref":result.standard.id,"baseline":baseline,"default_workflow_ref":result.entry,"snapshot":result.snapshot,"provenance":result.provenance,"preview_hash":hash,"harness_permission_ref":context,
            "requirements":{"roles":plan.roles.values().collect::<Vec<_>>(),"specification_view":plan.specification_view}}),
        )
    }
}
