//! Explicit retries may adopt an already completed publication from their own
//! exact lineage. Old cancellation and physical operation identities stay intact.
use crate::{
    concurrency::*, platform_facts::PlatformAction, platform_facts::PlatformPlan,
    platform_facts::one, workflow_resources::version, *,
};
use std::collections::BTreeSet;

pub(crate) struct CompletedPublication {
    pub action: PlatformAction,
    pub reads: ReadSet,
}

fn record(
    store: &dyn EngineeringStore,
    kind: &str,
    id: &str,
    reads: &mut ReadSet,
) -> Result<ConfigurationRecord> {
    let source = one(store, kind, id)?;
    let row = ConfigurationRecord::from_storage(
        store.restore_runtime_record(
            source
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原发布核对依据缺失"))?,
        )?,
    )?;
    *reads = reads.merge([version(&source)])?;
    require(
        row.scope.project_id() == store.project_id().as_deref()
            && (kind != "governance" || row.id == id),
        "原发布核对依据跨项目",
    )?;
    Ok(row)
}

pub(crate) fn completed_publication(
    store: &dyn EngineeringStore,
    request: &PlatformToolRequest,
    execution: &StepExecution,
    root: &str,
    context: &GitContext,
) -> Result<Option<CompletedPublication>> {
    // Native calls inside an Agent are separate operations, not a direct tool
    // step's explicit retry. Rework starts a new round and clears retry_of.
    if request.caller.is_some() || execution.retry_of.is_none() {
        return Ok(None);
    }
    require(
        store.project_id().as_deref() == Some(request.project_id.as_str())
            && request.definition.qualified_name == "git.publish"
            && request.execution_id == execution.id
            && request.instance_id == execution.workflow_instance_id
            && context.project == request.project_id,
        "发布重试范围不符",
    )?;
    let mut previous = execution.retry_of.clone();
    let mut seen = BTreeSet::new();
    let mut reads = ReadSet::default();
    while let Some(id) = previous {
        require(
            seen.insert(id.clone()) && seen.len() <= 32,
            "发布重试来源存在环或超过边界",
        )?;
        let history = record(store, "workflow_history_source", &id, &mut reads)?;
        let old: StepExecution = history.decode()?;
        require(
            history.r#type == "workflow_execution_history"
                && history.immutable
                && history.definition_id == id
                && old.id == id
                && old.workflow_instance_id == request.instance_id
                && old.workflow_instance_id == execution.workflow_instance_id
                && old.node_id == execution.node_id
                && old.plan_revision_id == execution.plan_revision_id
                && old.workflow_revision_id == execution.workflow_revision_id
                && old.contract_digest == execution.contract_digest
                && old.round == execution.round
                && old.inputs == execution.inputs
                && old.input_digest == execution.input_digest,
            "发布重试改变了原节点、计划、合同或输入",
        )?;
        let failure = old
            .failure
            .as_ref()
            .ok_or_else(|| err("needs_reconciliation", "原发布缺少副作用分类"))?;
        require(
            matches!(
                old.status,
                StepExecutionStatus::Cancelled | StepExecutionStatus::Failed
            ),
            "原发布执行不属于可核对的重试历史",
        )?;
        if failure.effect == ExecutionFailureEffect::NotApplied {
            previous = old.retry_of;
            continue;
        }
        require(
            failure.kind == ExecutionFailureKind::Technical
                && failure.effect == ExecutionFailureEffect::Applied,
            "原发布副作用尚未核对，不重新发布",
        )?;
        let call_id = old
            .tool_call_id
            .as_deref()
            .ok_or_else(|| err("needs_reconciliation", "原发布缺少调用身份"))?;
        let call = record(store, "governance", call_id, &mut reads)?;
        require(
            call.r#type == "platform_call"
                && call.id == call.definition_id
                && call.id == old.id
                && call.data["handler"] == "git.publish"
                && call.data["root_instance_id"] == root
                && call.data["status"] == "succeeded"
                && call.data["remote_effect"] == "applied",
            "原发布没有已核对的完成回执",
        )?;
        let original: PlatformToolRequest = serde_json::from_value(call.data["request"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            original.caller.is_none()
                && original.execution_id == old.id
                && original.instance_id == request.instance_id
                && original.project_id == request.project_id
                && original.owner_id == request.owner_id
                && original.arguments == request.arguments
                && original.definition_ref == request.definition_ref
                && json!(original.definition) == json!(request.definition),
            "重试不能复用其他发布请求的结果",
        )?;
        let effect = record(
            store,
            "governance",
            &text(&call.data, "effect_id")?,
            &mut reads,
        )?;
        require(
            effect.r#type == "platform_effect_intent"
                && effect.immutable
                && effect.definition_id == call.id
                && effect.data["request_digest"] == configuration_hash(&original),
            "原发布失去不可变执行意图",
        )?;
        let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let (original_context, expected, candidate) = match &plan.action {
            PlatformAction::Publish {
                context,
                expected,
                candidate,
                ..
            }
            | PlatformAction::ReusedPublication {
                context,
                expected,
                candidate,
                ..
            } => (context, expected, candidate),
            _ => return Err(err("needs_reconciliation", "原操作不是发布")),
        };
        require(
            json!(original_context) == json!(context)
                && *expected == request.arguments["candidate"]["target_revision"]
                && *candidate == request.arguments["candidate"]["revision"],
            "原发布仓库、目标或候选已变化",
        )?;
        let receipt = &call.data["receipt"];
        let pr: PullRequestReceipt = serde_json::from_value(receipt["pull_request"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            receipt["revision"] == *candidate
                && pr.repository == context.workspace.repository
                && pr.base_branch == context.target.branch
                && pr.head == *candidate
                && pr.merged
                && pr.merge_commit.as_deref() == Some(candidate.as_str())
                && call.data["receipt_digest"] == configuration_hash(receipt),
            "原发布回执未证明确切候选已合入",
        )?;
        let observation_id = call.data["reconciliation_observation_id"]
            .as_str()
            .or_else(|| call.data["observation_id"].as_str())
            .ok_or_else(|| err("needs_reconciliation", "原发布缺少原始观察"))?;
        let observation = record(store, "governance", observation_id, &mut reads)?;
        require(observation.immutable, "发布观察必须不可变")?;
        match observation.r#type.as_str() {
            "platform_effect_observation" => require(
                observation.definition_id == effect.id
                    && observation.data["effect_id"] == effect.id
                    && observation.data["effect_digest"] == configuration_hash(&effect.data)
                    && observation.data["phase"] == "result"
                    && observation.data["result"]["Ok"] == *receipt,
                "原发布物理观察与回执不符",
            )?,
            "workflow_reconciliation_observation" => {
                let attempt = record(
                    store,
                    "governance",
                    &text(&observation.data, "attempt_id")?,
                    &mut reads,
                )?;
                require(
                    attempt.r#type == "workflow_reconciliation_attempt"
                        && attempt.immutable
                        && observation.definition_id == attempt.id
                        && observation.data["issuer"] == "kernel_reconciliation"
                        && attempt.data["issuer"] == "kernel_reconciliation"
                        && attempt.data["instance_id"] == request.instance_id
                        && attempt.data["execution_id"] == old.id
                        && attempt.data["call_id"] == call.id
                        && attempt.definition_id == call.data["reconciliation_id"]
                        && observation.data["value"]["result"]["Ok"]["Platform"]["receipt"]
                            == *receipt,
                    "原发布显式核对与执行或回执不符",
                )?;
            }
            _ => return Err(err("needs_reconciliation", "原发布观察类型无效")),
        }
        return Ok(Some(CompletedPublication {
            action: PlatformAction::ReusedPublication {
                context: context.clone(),
                expected: expected.clone(),
                candidate: candidate.clone(),
                receipt: receipt.clone(),
                source_call_id: call.id,
                source_observation_id: observation.id,
            },
            reads,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct Store(BTreeMap<String, ConfigurationRecord>);
    impl EngineeringStore for Store {
        fn root(&self) -> String {
            "publication-fixture".into()
        }
        fn project_id(&self) -> Option<String> {
            Some("project".into())
        }
        fn snapshot(&self) -> Result<EngineeringState> {
            panic!("retry cannot restore engineering history")
        }
        fn restore_runtime_record(&self, value: &Value) -> Result<Value> {
            Ok(value.clone())
        }
        fn read_resources(&self, keys: &[ResourceKey]) -> Result<Vec<ResourceSnapshot>> {
            keys.iter()
                .map(|key| {
                    let row = match key.kind.as_str() {
                        "governance" => self.0.get(&key.id),
                        "workflow_history_source" => self.0.values().find(|row| {
                            row.r#type == "workflow_execution_history"
                                && row.definition_id == key.id
                        }),
                        _ => panic!("unexpected resource: {key:?}"),
                    };
                    Ok(ResourceSnapshot {
                        resource: key.clone(),
                        revision: row.map_or(0, |row| row.revision),
                        value: row.map(ConfigurationRecord::to_storage),
                    })
                })
                .collect()
        }
    }
    fn row(
        id: &str,
        kind: &str,
        definition: &str,
        data: Value,
        immutable: bool,
    ) -> ConfigurationRecord {
        let mut row = crate::governance::record(
            kind,
            definition.into(),
            Scope::project("project"),
            id.into(),
            1,
            None,
            data,
            immutable,
        );
        row.id = id.into();
        row
    }
    fn fixture(reconciled: bool) -> (Store, PlatformToolRequest, StepExecution, GitContext) {
        let context = GitContext {
            project: "project".into(),
            workspace: Workspace {
                repository: "fixture/test".into(),
                path: "repository".into(),
                mainline: "main".into(),
            },
            source: CodeLocation {
                owner: "feature".into(),
                branch: "feature".into(),
                worktree: "feature-worktree".into(),
            },
            target: CodeLocation {
                owner: "project".into(),
                branch: "main".into(),
                worktree: "main-worktree".into(),
            },
            origin: Origin::External,
        };
        let candidate = "a".repeat(40);
        let expected = "b".repeat(40);
        let mut request = PlatformToolRequest {
            definition_ref:Some("git.publish:1".into()),execution_id:"old-execution".into(),instance_id:"root".into(),owner_id:"feature".into(),project_id:"project".into(),
            definition:serde_json::from_value(json!({"qualified_name":"git.publish","implementation":"platform","input_schema":{},"output_schema":{}})).unwrap(),
            arguments:json!({"candidate":{"revision":candidate,"target_revision":expected},"accepted":{"revision":candidate,"previous":expected}}),runtime:None,permissions:None,caller:None,
        };
        let old:StepExecution=serde_json::from_value(json!({"plan_revision_id":"plan","node_id":"node","contract_digest":"contract","id":"old-execution","workflow_instance_id":"root","workflow_revision_id":"workflow:1","path":"/publish","step":"publish","round":1,"sequence":1,"failure":{"code":"observed_after_stop","message":"stopped, applied result retained","source_ref":"observation","kind":"technical","effect":"applied"},"status":"cancelled","input_digest":"original-input","inputs":{},"outputs":{},"agent_revision_id":null,"session_id":null,"turn_id":null,"tool_call_id":"old-execution","started":1,"finished":2,"reason":"original cancellation retained"})).unwrap();
        let receipt = json!({"revision":candidate,"operation_id":"old-execution","pull_request":{"repository":"fixture/test","number":17,"url":"https://example.invalid/17","head_branch":"candidate","base_branch":"main","head":candidate,"base":expected,"merged":true,"merge_commit":candidate,"marker":"original-marker"}});
        let plan = PlatformPlan {
            action: PlatformAction::Publish {
                context: context.clone(),
                expected,
                candidate,
                operation: "old-execution".into(),
                operations: json!({}),
            },
            basis: ReadSet::default(),
            changes: vec![],
            head: None,
            fence: None,
            permit: None,
        };
        let effect = row(
            "effect",
            "platform_effect_intent",
            "old-execution",
            json!({"plan":plan,"request_digest":configuration_hash(&request)}),
            true,
        );
        let mut call = row(
            "old-execution",
            "platform_call",
            "old-execution",
            json!({"handler":"git.publish","root_instance_id":"root","status":"succeeded","remote_effect":"applied","request":request,"receipt":receipt,"receipt_digest":configuration_hash(&receipt),"effect_id":"effect","observation_id":"observation"}),
            false,
        );
        let observation = if reconciled {
            call.data["reconciliation_id"] = json!("reconciliation");
            call.data["reconciliation_observation_id"] = json!("observation");
            row(
                "observation",
                "workflow_reconciliation_observation",
                "attempt",
                json!({"attempt_id":"attempt","issuer":"kernel_reconciliation","value":{"result":{"Ok":{"Platform":{"receipt":receipt}}}}}),
                true,
            )
        } else {
            row(
                "observation",
                "platform_effect_observation",
                "effect",
                json!({"phase":"result","effect_id":"effect","effect_digest":configuration_hash(&effect.data),"result":{"Ok":receipt}}),
                true,
            )
        };
        let attempt = row(
            "attempt",
            "workflow_reconciliation_attempt",
            "reconciliation",
            json!({"issuer":"kernel_reconciliation","instance_id":"root","execution_id":"old-execution","call_id":"old-execution"}),
            true,
        );
        let rows = [
            row(
                "history",
                "workflow_execution_history",
                "old-execution",
                json!(old),
                true,
            ),
            call,
            effect,
            observation,
            attempt,
        ];
        let store = Store(rows.into_iter().map(|row| (row.id.clone(), row)).collect());
        let mut execution = old;
        execution.id = "retry-execution".into();
        execution.retry_of = Some("old-execution".into());
        execution.failure = None;
        execution.status = StepExecutionStatus::Running;
        execution.tool_call_id = None;
        request.execution_id = execution.id.clone();
        (store, request, execution, context)
    }

    #[test]
    fn an_explicit_retry_reuses_the_original_completed_publication_without_rewriting_cancellation()
    {
        for reconciled in [false, true] {
            let (store, request, execution, context) = fixture(reconciled);
            let completed = completed_publication(&store, &request, &execution, "root", &context)
                .unwrap()
                .unwrap();
            let PlatformAction::ReusedPublication {
                receipt,
                source_call_id,
                source_observation_id,
                ..
            } = completed.action
            else {
                panic!("must not grant another remote publish")
            };
            assert_eq!(receipt["operation_id"], "old-execution");
            assert_eq!(source_call_id, "old-execution");
            assert_eq!(source_observation_id, "observation");
            assert_eq!(store.0["history"].data["status"], "cancelled");
            for id in ["old-execution", "effect", "observation"] {
                assert!(
                    completed
                        .reads
                        .versions()
                        .iter()
                        .any(|version| version.resource == ResourceKey::new("governance", id))
                );
            }
            assert!(
                completed
                    .reads
                    .versions()
                    .iter()
                    .any(|version| version.resource.kind == "selected_record")
            );
        }
    }

    #[test]
    fn a_retry_cannot_adopt_another_scope_input_or_unproved_remote_receipt() {
        for case in [
            "root",
            "node",
            "plan",
            "round",
            "input",
            "repository",
            "candidate",
            "observation",
            "attempt",
            "unknown",
        ] {
            let (mut store, mut request, mut execution, mut context) = fixture(true);
            match case {
                "root" => {
                    store.0.get_mut("old-execution").unwrap().data["root_instance_id"] =
                        json!("foreign-root")
                }
                "node" => execution.node_id = "another-node".into(),
                "plan" => execution.plan_revision_id = "another-plan".into(),
                "round" => execution.round += 1,
                "input" => request.arguments["accepted"]["previous"] = json!("changed-target"),
                "repository" => context.workspace.repository = "foreign/repo".into(),
                "candidate" => {
                    store.0.get_mut("old-execution").unwrap().data["receipt"]["pull_request"]["merge_commit"] =
                        json!("another-candidate")
                }
                "observation" => {
                    store.0.get_mut("observation").unwrap().data["value"]["result"]["Ok"]["Platform"]
                        ["receipt"]["revision"] = json!("forged-receipt")
                }
                "attempt" => {
                    store.0.get_mut("attempt").unwrap().data["execution_id"] =
                        json!("foreign-execution")
                }
                "unknown" => {
                    store.0.get_mut("history").unwrap().data["failure"]["effect"] = json!("unknown")
                }
                _ => unreachable!(),
            }
            assert!(
                completed_publication(&store, &request, &execution, "root", &context).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn an_unapplied_intermediate_retry_can_follow_its_exact_older_applied_attempt() {
        let (mut store, request, mut execution, context) = fixture(true);
        let mut intermediate = store.0["history"].clone();
        intermediate.id = "intermediate-history".into();
        intermediate.definition_id = "intermediate".into();
        intermediate.data["id"] = json!("intermediate");
        intermediate.data["retry_of"] = json!("old-execution");
        intermediate.data["failure"]["effect"] = json!("not_applied");
        store.0.insert(intermediate.id.clone(), intermediate);
        execution.retry_of = Some("intermediate".into());
        assert!(
            completed_publication(&store, &request, &execution, "root", &context)
                .unwrap()
                .is_some()
        );
        execution.retry_of = None;
        assert!(
            completed_publication(&store, &request, &execution, "root", &context)
                .unwrap()
                .is_none()
        );
    }
}
