//! Read-only reverse references derived from exact source revisions.
use crate::*;
use std::collections::BTreeMap;

pub type ConfigurationUsageIndex = BTreeMap<Id, Vec<ConfigurationReferenceUsage>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationUsageKind {
    Reference,
    WorkflowInput,
    StepInput,
    StepArgument,
    StepCollection,
    StepOutput,
    StepSpec,
    StepTool,
    WorkflowOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationUsageNode {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationReferenceUsage {
    pub reference: Id,
    pub name: String,
    pub r#type: String,
    pub version: u64,
    pub scope: Scope,
    pub path: String,
    pub kind: ConfigurationUsageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<ConfigurationUsageNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<ConfigurationUsageNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
}

/// Callers select the visible revisions; this function never resolves to a newer source.
pub fn configuration_usage_index<'a>(
    records: impl IntoIterator<Item = &'a ConfigurationRecord>,
) -> Result<ConfigurationUsageIndex> {
    let records: Vec<_> = records.into_iter().collect();
    let mut index = BTreeMap::<_, Vec<_>>::new();
    for record in &records {
        if ["spec", "skill", "tool", "delivery_type"].contains(&record.r#type.as_str()) {
            index.insert(record.id.clone(), Vec::new());
        }
    }
    for record in records {
        let mut add = |target: &str,
                       path: String,
                       kind: ConfigurationUsageKind,
                       stage: Option<ConfigurationUsageNode>,
                       step: Option<ConfigurationUsageNode>,
                       port: Option<String>| {
            index
                .entry(target.to_owned())
                .or_default()
                .push(ConfigurationReferenceUsage {
                    reference: record.id.clone(),
                    name: record.name.clone(),
                    r#type: record.r#type.clone(),
                    version: record.version,
                    scope: record.scope.clone(),
                    path,
                    kind,
                    stage,
                    step,
                    port,
                });
        };
        match record.r#type.as_str() {
            "capability" | "delivery_type" => {
                for edge in configuration_edges(record)? {
                    if record.r#type == "capability"
                        && ["spec", "skill", "tool"].contains(&edge.expected_type.as_str())
                        || record.r#type == "delivery_type" && edge.expected_type == "spec"
                    {
                        add(
                            &edge.to,
                            edge.path,
                            ConfigurationUsageKind::Reference,
                            None,
                            None,
                            None,
                        );
                    }
                }
            }
            "workflow" => {
                let workflow: WorkflowDefinition = record.decode()?;
                if let Some(role) = &workflow.orchestrator_role_ref {
                    add(
                        role,
                        "/data/orchestrator_role_ref".into(),
                        ConfigurationUsageKind::Reference,
                        None,
                        None,
                        None,
                    );
                }
                let steps: BTreeMap<_, _> =
                    workflow.steps.iter().map(|s| (s.key.as_str(), s)).collect();
                let stages: BTreeMap<_, _> = workflow
                    .stages
                    .iter()
                    .map(|s| (s.key.as_str(), s))
                    .collect();
                let source_delivery = |source: &PortSource| {
                    if source.step == "$input" {
                        workflow.inputs.get(&source.port)
                    } else {
                        steps
                            .get(source.step.as_str())
                            .and_then(|s| s.outputs.get(&source.port))
                    }
                };
                for (name, input) in &workflow.inputs {
                    add(
                        &input.delivery_ref,
                        format!("/data/inputs/{name}/delivery_ref"),
                        ConfigurationUsageKind::WorkflowInput,
                        None,
                        None,
                        Some(name.clone()),
                    );
                }
                for (i, step) in workflow.steps.iter().enumerate() {
                    let stage = stages
                        .get(step.stage.as_str())
                        .map(|s| ConfigurationUsageNode {
                            key: s.key.clone(),
                            name: s.name.clone(),
                        });
                    let node = Some(ConfigurationUsageNode {
                        key: step.key.clone(),
                        name: step.name.clone(),
                    });
                    for (j, spec_ref) in step.spec_refs.iter().enumerate() {
                        add(
                            spec_ref,
                            format!("/data/steps/{i}/spec_refs/{j}"),
                            ConfigurationUsageKind::StepSpec,
                            stage.clone(),
                            node.clone(),
                            None,
                        );
                    }
                    for (name, output) in &step.outputs {
                        add(
                            &output.delivery_ref,
                            format!("/data/steps/{i}/outputs/{name}/delivery_ref"),
                            ConfigurationUsageKind::StepOutput,
                            stage.clone(),
                            node.clone(),
                            Some(name.clone()),
                        );
                    }
                    for (name, binding) in &step.inputs {
                        if let InputBinding::Source { source } = binding
                            && let Some(delivery) = source_delivery(source)
                        {
                            add(
                                &delivery.delivery_ref,
                                format!("/data/steps/{i}/inputs/{name}"),
                                ConfigurationUsageKind::StepInput,
                                stage.clone(),
                                node.clone(),
                                Some(name.clone()),
                            );
                        }
                    }
                    if let StepExecutor::Tool {
                        tool_ref,
                        arguments,
                        ..
                    } = &step.executor
                    {
                        add(
                            tool_ref,
                            format!("/data/steps/{i}/executor/tool_ref"),
                            ConfigurationUsageKind::StepTool,
                            stage.clone(),
                            node.clone(),
                            None,
                        );
                        for (name, binding) in arguments {
                            if let InputBinding::Source { source } = binding
                                && let Some(delivery) = source_delivery(source)
                            {
                                add(
                                    &delivery.delivery_ref,
                                    format!("/data/steps/{i}/executor/arguments/{name}"),
                                    ConfigurationUsageKind::StepArgument,
                                    stage.clone(),
                                    node.clone(),
                                    Some(name.clone()),
                                );
                            }
                        }
                    }
                    if let StepExecutor::Workflow {
                        foreach: Some(collection),
                        ..
                    } = &step.executor
                        && let Some(delivery) = source_delivery(&collection.source)
                    {
                        add(
                            &delivery.delivery_ref,
                            format!("/data/steps/{i}/executor/foreach/source"),
                            ConfigurationUsageKind::StepCollection,
                            stage.clone(),
                            node.clone(),
                            None,
                        );
                    }
                }
                for (name, source) in &workflow.outputs {
                    if let Some(delivery) = source_delivery(source) {
                        add(
                            &delivery.delivery_ref,
                            format!("/data/outputs/{name}"),
                            ConfigurationUsageKind::WorkflowOutput,
                            None,
                            None,
                            Some(name.clone()),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for users in index.values_mut() {
        users.sort_by(|a, b| {
            (&a.r#type, &a.reference, &a.path).cmp(&(&b.r#type, &b.reference, &b.path))
        });
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usages_follow_exact_references_and_delivery_flow_not_literal_text() {
        let record = |kind: &str, identity: &str, data: Value, refs: Value| {
            SourceDefinition {
                r#type: kind.into(),
                identity: identity.into(),
                revision: 1,
                name: identity.into(),
                scope: Scope::global(),
                data,
                refs: serde_json::from_value(refs).unwrap(),
            }
            .into_record()
            .unwrap()
        };
        let delivery = record(
            "delivery_type",
            "delivery",
            json!({"format":"text"}),
            json!({"spec_refs":["spec:1"]}),
        );
        let cap = record(
            "capability",
            "cap",
            json!({}),
            json!({"model_ref":"model:1","harness_ref":"harness:1","skills":["skill:2"],"tools":["tool:1"],"spec_refs":["spec:1"]}),
        );
        let workflow = record(
            "workflow",
            "flow",
            json!({"schema_version":6,
            "stages":[{"key":"stage","name":"Stage"}],
            "inputs":{"request":{"delivery_ref":"delivery:1"}},
            "steps":[
                {"key":"author","name":"Author","stage":"stage","executor":{"type":"control"},
                 "inputs":{"request":{"type":"source","source":{"step":"$input","port":"request"}}},
                 "outputs":{"result":{"delivery_ref":"delivery:1"}}},
                {"key":"review","name":"Review","stage":"stage","executor":{"type":"control"},"spec_refs":["spec:2"],
                 "inputs":{"work":{"type":"source","source":{"step":"author","port":"result","pointer":"/part"}},
                           "example":{"type":"constant","value":"delivery:2 skill:1"}}},
                {"key":"tool","name":"Tool","stage":"stage","executor":{"type":"tool","tool_ref":"tool:1",
                    "arguments":{"document":{"type":"source","source":{"step":"author","port":"result"}}}}},
                {"key":"children","name":"Children","stage":"stage","executor":{"type":"workflow","workflow_ref":"child:1",
                    "foreach":{"source":{"step":"author","port":"result"},"item_input":"item","max_concurrency":1,"min_items":1}}}
            ],"outputs":{"result":{"step":"author","port":"result"}}}),
            json!({}),
        );
        let index = configuration_usage_index([&delivery, &cap, &workflow]).unwrap();
        assert_eq!(index["spec:1"].len(), 2);
        let step_spec = &index["spec:2"][0];
        assert_eq!(step_spec.reference, "flow:1");
        assert_eq!(step_spec.kind, ConfigurationUsageKind::StepSpec);
        assert_eq!(step_spec.path, "/data/steps/1/spec_refs/0");
        assert_eq!(step_spec.step.as_ref().unwrap().key, "review");
        assert_eq!(index["skill:2"][0].reference, "cap:1");
        assert!(!index.contains_key("skill:1"));
        assert!(!index.contains_key("delivery:2"));
        assert_eq!(index["tool:1"].len(), 2);
        let invocation = &index["tool:1"][1];
        assert_eq!(invocation.reference, "flow:1");
        assert_eq!(invocation.kind, ConfigurationUsageKind::StepTool);
        assert_eq!(invocation.path, "/data/steps/2/executor/tool_ref");
        assert_eq!(invocation.step.as_ref().unwrap().key, "tool");
        assert_eq!(invocation.stage.as_ref().unwrap().name, "Stage");
        assert_eq!(index["delivery:1"].len(), 7);
        assert!(
            index["delivery:1"]
                .iter()
                .any(|u| u.kind == ConfigurationUsageKind::StepArgument
                    && u.port.as_deref() == Some("document"))
        );
        assert!(
            index["delivery:1"]
                .iter()
                .any(|u| u.kind == ConfigurationUsageKind::StepCollection)
        );
        let review = index["delivery:1"]
            .iter()
            .find(|u| u.step.as_ref().is_some_and(|s| s.key == "review"))
            .unwrap();
        assert_eq!(review.stage.as_ref().unwrap().name, "Stage");
        assert_eq!(review.kind, ConfigurationUsageKind::StepInput);
        assert_eq!(review.port.as_deref(), Some("work"));
    }
}
