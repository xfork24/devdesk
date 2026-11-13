//! Immutable normative source selections. Consumers and execution facts are not part of a Standard.
use crate::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const STANDARD_SNAPSHOT_VERSION: &str = "standard-snapshot/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredStandard {
    pub schema_version: u64,
}
impl ConfiguredStandard {
    pub fn validate(&self) -> Result<()> {
        // Version is provenance metadata; source references carry the entry constraints.
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkReference {
    Project { id: Id },
    Feature { id: Id },
    Task { id: Id },
}
impl WorkReference {
    pub fn id(&self) -> &str {
        match self {
            Self::Project { id } | Self::Feature { id } | Self::Task { id } => id,
        }
    }
    pub fn scope(&self, state: &EngineeringState) -> Result<Scope> {
        match self {
            Self::Project { id } => {
                require(state.projects.iter().any(|p| &p.id == id), "Project 不存在")?;
                Ok(Scope::project(id))
            }
            Self::Feature { id } => {
                let f = state.feature(id)?;
                Ok(Scope::feature(&f.project_id, &f.id))
            }
            Self::Task { id } => {
                let t = state.task(id)?;
                let f = state.feature(&t.feature_id)?;
                Ok(Scope::feature(&f.project_id, &f.id))
            }
        }
    }
}

pub fn work_reference(state: &EngineeringState, owner: &str) -> Result<WorkReference> {
    if state.projects.iter().any(|r| r.id == owner) {
        Ok(WorkReference::Project { id: owner.into() })
    } else if state.features.iter().any(|r| r.id == owner) {
        Ok(WorkReference::Feature { id: owner.into() })
    } else if state.tasks.iter().any(|r| r.id == owner) {
        Ok(WorkReference::Task { id: owner.into() })
    } else {
        Err(err("not_found", "工作对象不存在"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandardNode {
    pub reference: Id,
    pub kind: String,
    pub name: String,
    pub content: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StandardEdge {
    pub from: Id,
    pub path: String,
    pub relation: String,
    pub to: Id,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardSnapshot {
    pub digest_version: String,
    pub snapshot_digest: String,
    pub root_workflow_refs: Vec<Id>,
    pub nodes: Vec<StandardNode>,
    pub edges: Vec<StandardEdge>,
}

/// Sort only set-valued declarations. Arrays of steps and checks retain their order.
pub fn canonical_standard_content(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                let mut value = canonical_standard_content(value);
                if [
                    "spec_refs",
                    "required_skill_refs",
                    "tags",
                    "root_workflow_refs",
                ]
                .contains(&key.as_str())
                {
                    if let Some(items) = value.as_array_mut() {
                        items.sort_by_key(Value::to_string);
                    }
                }
                result.insert(key.clone(), value);
            }
            Value::Object(result)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_standard_content).collect()),
        other => other.clone(),
    }
}

pub fn resolve_standard_sources(
    state: &EngineeringState,
    roots: &[Id],
    scope: &Scope,
) -> Result<StandardSnapshot> {
    require(
        !roots.is_empty() && roots.len() <= 100,
        "Standard 需要 1—100 个 Workflow 入口",
    )?;
    struct Resolver<'a> {
        state: &'a EngineeringState,
        nodes: BTreeMap<Id, StandardNode>,
        edges: BTreeSet<StandardEdge>,
        active: BTreeSet<Id>,
    }
    impl Resolver<'_> {
        fn link(
            &mut self,
            owner: &ConfigurationRecord,
            path: String,
            relation: &str,
            target: &str,
            kind: &str,
            depth: usize,
        ) -> Result<()> {
            self.edges.insert(StandardEdge {
                from: owner.id.clone(),
                path,
                relation: relation.into(),
                to: target.into(),
            });
            self.visit(target, kind, &owner.scope, depth)
        }
        fn port(
            &mut self,
            owner: &ConfigurationRecord,
            path: String,
            port: &PortDefinition,
            depth: usize,
        ) -> Result<()> {
            require(!port.delivery_ref.is_empty(), "端口缺少 Delivery 修订")?;
            if !port.delivery_ref.is_empty() {
                self.link(
                    owner,
                    path,
                    "delivery",
                    &port.delivery_ref,
                    "delivery_type",
                    depth,
                )?;
            }
            Ok(())
        }
        fn visit(
            &mut self,
            reference: &str,
            kind: &str,
            scope: &Scope,
            depth: usize,
        ) -> Result<()> {
            parse_source_reference(reference)?;
            let source = self
                .state
                .configuration(reference)
                .map_err(|_| err("missing_source_revision", format!("缺少源修订 {reference}")))?
                .clone();
            require(
                source.r#type == kind && source.immutable,
                format!("{reference} 需要已发布的 {kind} 源修订"),
            )?;
            if !source.scope.visible_from(scope) {
                return Err(err(
                    "source_scope_violation",
                    format!("源修订不可见: {reference}"),
                ));
            }
            require(depth <= WORKFLOW_DEPTH_LIMIT, "规范子流程深度超过 8")?;
            require(!self.active.contains(reference), "规范源引用存在循环")?;
            if self.nodes.contains_key(reference) {
                return Ok(());
            }
            require(
                self.nodes.len() < WORKFLOW_STEP_LIMIT * 10,
                "规范源闭包过大",
            )?;
            self.active.insert(reference.into());
            match kind {
                "workflow" => {
                    let workflow: WorkflowDefinition = source.decode()?;
                    workflow.validate()?;
                    for (name, port) in &workflow.inputs {
                        self.port(&source, format!("inputs/{name}"), port, depth)?;
                    }
                    for step in &workflow.steps {
                        for spec in &step.spec_refs {
                            self.link(
                                &source,
                                format!("steps/{}/spec_refs", step.key),
                                "step_spec",
                                spec,
                                "spec",
                                depth,
                            )?;
                        }
                        for (name, port) in &step.outputs {
                            self.port(
                                &source,
                                format!("steps/{}/outputs/{name}", step.key),
                                port,
                                depth,
                            )?;
                        }
                        if let StepExecutor::Workflow { workflow_ref, .. } = &step.executor {
                            self.link(
                                &source,
                                format!("steps/{}/executor", step.key),
                                "workflow_call",
                                workflow_ref,
                                "workflow",
                                depth + 1,
                            )?;
                        }
                    }
                }
                "delivery_type" => {
                    let delivery: DeliveryTypeDefinition = source.decode()?;
                    let specs = reference_list(&source.refs, "spec_refs")?;
                    self.state
                        .validate_delivery_specs(&source.refs, &delivery, &source.scope)?;
                    for spec in &specs {
                        self.link(
                            &source,
                            "spec_refs".into(),
                            "delivery_spec",
                            spec,
                            "spec",
                            depth,
                        )?;
                    }
                }
                "spec" => {
                    source.decode::<ConfiguredSpecification>()?.validate()?;
                }
                _ => {
                    return Err(err(
                        "invalid_standard",
                        "Standard 只包含 Workflow、Delivery 和 Spec",
                    ));
                }
            }
            self.active.remove(reference);
            self.nodes.insert(
                reference.into(),
                StandardNode {
                    reference: reference.into(),
                    kind: kind.into(),
                    name: source.name,
                    content: canonical_standard_content(&source.data),
                },
            );
            Ok(())
        }
    }
    let mut resolver = Resolver {
        state,
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
        active: BTreeSet::new(),
    };
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();
    for root in &roots {
        resolver.visit(root, "workflow", scope, 0)?;
    }
    let nodes: Vec<_> = resolver.nodes.into_values().collect();
    let edges: Vec<_> = resolver.edges.into_iter().collect();
    let digest = configuration_hash(
        &json!({"version":STANDARD_SNAPSHOT_VERSION,"roots":roots,"nodes":nodes.iter().map(|n|json!({"reference":n.reference,"kind":n.kind,"content":n.content})).collect::<Vec<_>>(),"edges":edges}),
    );
    Ok(StandardSnapshot {
        digest_version: STANDARD_SNAPSHOT_VERSION.into(),
        snapshot_digest: digest,
        root_workflow_refs: roots,
        nodes,
        edges,
    })
}

pub fn standard_snapshot(
    state: &EngineeringState,
    reference: &str,
    scope: &Scope,
) -> Result<StandardSnapshot> {
    let source = state.configuration(reference)?;
    require(
        source.r#type == "standard" && source.immutable,
        "需要不可变 Standard 修订",
    )?;
    require(!source.scope.is_feature(), "Standard 不具有 Feature 归属")?;
    if !source.scope.visible_from(scope) {
        return Err(err("source_scope_violation", "Standard 源不可见"));
    }
    resolve_standard_sources(
        state,
        &reference_list(&source.refs, "root_workflow_refs")?,
        &source.scope,
    )
}

pub fn standard_entry(
    state: &EngineeringState,
    standard: &str,
    workflow: &str,
    scope: &Scope,
) -> Result<StandardSnapshot> {
    let snapshot = standard_snapshot(state, standard, scope)?;
    if !snapshot
        .nodes
        .iter()
        .any(|n| n.kind == "workflow" && n.reference == workflow)
    {
        return Err(err(
            "workflow_not_in_standard",
            "入口 Workflow 不属于所选 Standard",
        ));
    }
    Ok(snapshot)
}

pub fn diff_standard_snapshots(left: &StandardSnapshot, right: &StandardSnapshot) -> Value {
    fn identity(reference: &str) -> &str {
        parse_source_reference(reference)
            .map(|(id, _)| id)
            .unwrap_or(reference)
    }
    fn workflow_content(node: &StandardNode) -> Value {
        if node.kind == "workflow" {
            if let Ok(mut workflow) =
                serde_json::from_value::<WorkflowDefinition>(node.content.clone())
            {
                workflow.assign_node_ids(&node.reference);
                return canonical_standard_content(&json!(workflow));
            }
        }
        node.content.clone()
    }
    fn node_ids(value: &Value) -> BTreeSet<&str> {
        value["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|step| step["node_id"].as_str())
            .filter(|id| !id.is_empty())
            .collect()
    }
    fn fields(path: &str, a: &Value, b: &Value, out: &mut Vec<Value>) {
        if a == b {
            return;
        }
        if let (Some(a), Some(b)) = (a.as_array(), b.as_array()) {
            fn keyed(items: &[Value]) -> Option<BTreeMap<String, &Value>> {
                let pairs = items
                    .iter()
                    .map(|item| {
                        let key = item["node_id"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .or_else(|| item["key"].as_str())?;
                        Some((key.to_owned(), item))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let map = pairs.iter().cloned().collect::<BTreeMap<_, _>>();
                (map.len() == pairs.len()).then_some(map)
            }
            if (!a.is_empty() || !b.is_empty()) && a.iter().chain(b).all(Value::is_object) {
                if let (Some(left), Some(right)) = (keyed(a), keyed(b)) {
                    fn order(items: &[Value]) -> Vec<&str> {
                        items
                            .iter()
                            .map(|item| {
                                item["key"]
                                    .as_str()
                                    .or_else(|| item["node_id"].as_str())
                                    .unwrap_or("")
                            })
                            .collect::<Vec<_>>()
                    }
                    if order(a) != order(b) {
                        out.push(json!({"path":format!("{path}/$order"),"before":order(a),"after":order(b)}));
                    }
                    for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
                        let old = left.get(key).copied().unwrap_or(&Value::Null);
                        let new = right.get(key).copied().unwrap_or(&Value::Null);
                        let label = old["key"]
                            .as_str()
                            .or_else(|| new["key"].as_str())
                            .unwrap_or(key);
                        fields(
                            &format!("{path}/{}", label.replace('~', "~0").replace('/', "~1")),
                            old,
                            new,
                            out,
                        );
                    }
                    return;
                }
            }
        }
        if let (Some(a), Some(b)) = (a.as_object(), b.as_object()) {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                fields(
                    &format!("{path}/{key}"),
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                    out,
                );
            }
        } else {
            out.push(json!({"path":path,"before":a,"after":b}));
        }
    }
    let mut changes = vec![];
    let left_nodes: BTreeMap<_, _> = left
        .nodes
        .iter()
        .map(|n| (n.reference.as_str(), n))
        .collect();
    let right_nodes: BTreeMap<_, _> = right
        .nodes
        .iter()
        .map(|n| (n.reference.as_str(), n))
        .collect();
    let left_content = left
        .nodes
        .iter()
        .map(|node| (node.reference.as_str(), workflow_content(node)))
        .collect::<BTreeMap<_, _>>();
    let right_content = right
        .nodes
        .iter()
        .map(|node| (node.reference.as_str(), workflow_content(node)))
        .collect::<BTreeMap<_, _>>();
    let mut aliases = BTreeMap::new();
    let mut matched = BTreeSet::new();
    for old in &left.nodes {
        let exact = right_nodes.get(old.reference.as_str()).copied();
        let candidates: Vec<_> = right
            .nodes
            .iter()
            .filter(|n| n.kind == old.kind && identity(&n.reference) == identity(&old.reference))
            .collect();
        let own_count = left
            .nodes
            .iter()
            .filter(|n| n.kind == old.kind && identity(&n.reference) == identity(&old.reference))
            .count();
        let new = exact
            .or_else(|| (candidates.len() == 1 && own_count == 1).then(|| candidates[0]))
            .or_else(|| {
                if old.kind != "workflow" {
                    return None;
                }
                let ids = node_ids(&left_content[old.reference.as_str()]);
                let candidates = right
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.kind == "workflow"
                            && !matched.contains(node.reference.as_str())
                            && !ids.is_disjoint(&node_ids(&right_content[node.reference.as_str()]))
                    })
                    .collect::<Vec<_>>();
                if candidates.len() != 1 {
                    return None;
                }
                let ids = node_ids(&right_content[candidates[0].reference.as_str()]);
                (left
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.kind == "workflow"
                            && !ids.is_disjoint(&node_ids(&left_content[node.reference.as_str()]))
                    })
                    .count()
                    == 1)
                    .then_some(candidates[0])
            });
        if let Some(new) = new {
            matched.insert(new.reference.as_str());
            aliases.insert(identity(&old.reference), identity(&new.reference));
            if old.reference != new.reference || old.content != new.content {
                let mut diff = vec![];
                fields(
                    "",
                    &left_content[old.reference.as_str()],
                    &right_content[new.reference.as_str()],
                    &mut diff,
                );
                changes.push(json!({"kind":if diff.is_empty(){"reference_changed"}else{"definition_changed"},"source_kind":old.kind,"before_ref":old.reference,"after_ref":new.reference,"fields":diff}));
            }
        } else {
            changes
                .push(json!({"kind":"removed","source_kind":old.kind,"before_ref":old.reference}));
        }
    }
    for new in &right.nodes {
        if !matched.contains(new.reference.as_str())
            && !left_nodes.contains_key(new.reference.as_str())
        {
            changes.push(json!({"kind":"added","source_kind":new.kind,"after_ref":new.reference}));
        }
    }
    let edge_key = |e: &StandardEdge| {
        json!({"from":aliases.get(identity(&e.from)).copied().unwrap_or(identity(&e.from)),"path":e.path,"relation":e.relation,"to":aliases.get(identity(&e.to)).copied().unwrap_or(identity(&e.to))})
            .to_string()
    };
    let le: BTreeSet<_> = left.edges.iter().map(edge_key).collect();
    let re: BTreeSet<_> = right.edges.iter().map(edge_key).collect();
    for edge in le.difference(&re) {
        changes.push(json!({"kind":"relationship_removed","edge":serde_json::from_str::<Value>(edge).unwrap()}));
    }
    for edge in re.difference(&le) {
        changes.push(json!({"kind":"relationship_added","edge":serde_json::from_str::<Value>(edge).unwrap()}));
    }
    if left.root_workflow_refs != right.root_workflow_refs {
        changes.push(json!({"kind":"entries_changed","before":left.root_workflow_refs,"after":right.root_workflow_refs}));
    }
    json!({"equal":left.snapshot_digest==right.snapshot_digest,"before_digest":left.snapshot_digest,"after_digest":right.snapshot_digest,"digest_version":STANDARD_SNAPSHOT_VERSION,"changes":changes})
}

/// A read-only projection for comparing the entry actually selected by a consumer.
pub fn standard_entry_snapshot(
    snapshot: &StandardSnapshot,
    entry: &str,
) -> Result<StandardSnapshot> {
    require(
        snapshot
            .nodes
            .iter()
            .any(|n| n.kind == "workflow" && n.reference == entry),
        "Workflow 不在规范快照中",
    )?;
    let mut reachable = BTreeSet::from([entry.to_owned()]);
    loop {
        let old = reachable.len();
        for edge in &snapshot.edges {
            if reachable.contains(&edge.from) {
                reachable.insert(edge.to.clone());
            }
        }
        if old == reachable.len() {
            break;
        }
    }
    let nodes: Vec<_> = snapshot
        .nodes
        .iter()
        .filter(|n| reachable.contains(&n.reference))
        .cloned()
        .collect();
    let edges: Vec<_> = snapshot
        .edges
        .iter()
        .filter(|e| reachable.contains(&e.from))
        .cloned()
        .collect();
    let roots = vec![entry.to_owned()];
    let digest = configuration_hash(
        &json!({"version":STANDARD_SNAPSHOT_VERSION,"roots":roots,"nodes":nodes.iter().map(|n|json!({"reference":n.reference,"kind":n.kind,"content":n.content})).collect::<Vec<_>>(),"edges":edges}),
    );
    Ok(StandardSnapshot {
        digest_version: STANDARD_SNAPSHOT_VERSION.into(),
        snapshot_digest: digest,
        root_workflow_refs: roots,
        nodes,
        edges,
    })
}

#[cfg(test)]
mod diff_tests {
    use super::*;
    fn snapshot(reference: &str, content: Value) -> StandardSnapshot {
        StandardSnapshot {
            digest_version: STANDARD_SNAPSHOT_VERSION.into(),
            snapshot_digest: configuration_hash(&json!([reference, content])),
            root_workflow_refs: vec![reference.into()],
            nodes: vec![StandardNode {
                reference: reference.into(),
                kind: "workflow".into(),
                name: "Work".into(),
                content,
            }],
            edges: vec![StandardEdge {
                from: reference.into(),
                path: "steps/one/spec_refs".into(),
                relation: "step_spec".into(),
                to: "spec:1".into(),
            }],
        }
    }
    #[test]
    fn a_derived_workflow_diff_locates_the_changed_step_and_preserves_relationship_identity() {
        let document = json!({"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"one","name":"Before","stage":"work","executor":{"type":"control"}}]});
        let left = snapshot("published:1", document.clone());
        let mut workflow: WorkflowDefinition = serde_json::from_value(document).unwrap();
        workflow.assign_node_ids("published:1");
        workflow.steps[0].name = "After".into();
        let right = snapshot("execution-derived:1", json!(workflow));
        let diff = diff_standard_snapshots(&left, &right);
        let changes = diff["changes"].as_array().unwrap();
        let changed = changes
            .iter()
            .find(|change| change["kind"] == "definition_changed")
            .unwrap();
        assert_eq!(
            changed["fields"],
            json!([{"path":"/steps/one/name","before":"Before","after":"After"}])
        );
        assert!(!changes.iter().any(|change| {
            [
                "added",
                "removed",
                "relationship_added",
                "relationship_removed",
            ]
            .contains(&change["kind"].as_str().unwrap())
        }));
    }
    #[test]
    fn ambiguous_copied_node_identities_are_not_merged_into_one_workflow() {
        let mut workflow:WorkflowDefinition=serde_json::from_value(json!({"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"one","name":"Work","stage":"work","executor":{"type":"control"}}]})).unwrap();
        workflow.assign_node_ids("original:1");
        let mut left = snapshot("copy-a:1", json!(workflow));
        let mut copy = left.nodes[0].clone();
        copy.reference = "copy-b:1".into();
        left.nodes.push(copy);
        workflow.steps[0].name = "Changed".into();
        let right = snapshot("derived:1", json!(workflow));
        let diff = diff_standard_snapshots(&left, &right);
        assert_eq!(
            diff["changes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|change| change["kind"] == "definition_changed")
                .count(),
            0
        );
    }
}
