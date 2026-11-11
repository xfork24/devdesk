//! Typed changes and impact analysis; no external actions or state mutations.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowChangeOperation {
    ReplacePlan {
        document: WorkflowDefinition,
    },
    UpsertStep {
        step: StepDefinition,
    },
    RemoveStep {
        node_id: Id,
    },
    UpsertStage {
        stage: StageDefinition,
    },
    BypassStage {
        stage: String,
        #[serde(default)]
        substitutions: BTreeMap<String, InputBinding>,
    },
    BindInput {
        node_id: Id,
        input: String,
        binding: InputBinding,
    },
    SetBindings {
        bindings: BTreeMap<Id, Id>,
    },
    SetInputs {
        inputs: BTreeMap<String, serde_json::Value>,
    },
    Waive {
        node_id: Id,
        requirement: String,
    },
    ReviseTaskScope {
        task_id: Id,
        title: String,
        scope: TaskScope,
    },
    ReviseTaskDependencies {
        task_id: Id,
        dependencies: Vec<Id>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowChangeSet {
    pub plan_revision_id: Id,
    pub operations: Vec<WorkflowChangeOperation>,
    pub reason: String,
}
impl WorkflowChangeSet {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.plan_revision_id.is_empty()
                && !self.reason.trim().is_empty()
                && !self.operations.is_empty()
                && self.operations.len() <= 128,
            "变更需要计划、理由以及 1—128 项类型化操作",
        )
    }
}

impl WorkflowDefinition {
    pub fn assign_node_ids(&mut self, definition: &str) {
        for stage in &mut self.stages {
            if stage.node_id.is_empty() {
                stage.node_id = workflow_node_id(definition, "stage", &stage.key);
            }
        }
        for step in &mut self.steps {
            if step.node_id.is_empty() {
                step.node_id = workflow_node_id(definition, "step", &step.key);
            }
        }
    }

    /// Includes scheduling edges and value/rule references. Conservative invalidation is safe.
    pub fn impacted_nodes(&self, seeds: &BTreeSet<String>) -> BTreeSet<String> {
        let mut affected = seeds.clone();
        loop {
            let before = affected.len();
            for step in &self.steps {
                let dependencies = step.dependencies.iter().any(|k| affected.contains(k));
                let values = serde_json::to_value(step).unwrap_or_default();
                fn refers(v: &serde_json::Value, keys: &BTreeSet<String>) -> bool {
                    match v {
                        serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
                            (["step", "first", "second", "author", "reviewer"]
                                .contains(&k.as_str())
                                && v.as_str().is_some_and(|s| keys.contains(s)))
                                || refers(v, keys)
                        }),
                        serde_json::Value::Array(a) => a.iter().any(|v| refers(v, keys)),
                        _ => false,
                    }
                }
                if dependencies || refers(&values, &affected) {
                    affected.insert(step.key.clone());
                }
            }
            if affected.len() == before {
                return affected;
            }
        }
    }
}
