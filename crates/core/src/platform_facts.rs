//! A platform action's actual, finite authorization facts and original updates.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    work_owners::WorkOwnerFacts,
    workflow_resources::{WorkflowResources, version},
    *,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum PlatformAction {
    Delivery(Box<crate::platform_delivery::DeliveryAction>),
    Value(Value),
    DomainRead {
        project: Id,
        handler: String,
        input: Value,
    },
    WorkspaceCurrent {
        context: GitContext,
        workspace: RepositoryWorktree,
    },
    Snapshot {
        operation: RepositoryOperation,
        workspace: RepositoryWorktree,
        include_diff: bool,
        scope_base: Option<String>,
    },
    Test(ports::TestRequest),
    Candidate {
        context: GitContext,
        source: String,
        target: String,
        operation: Id,
        workspace: Id,
    },
    Advance {
        context: GitContext,
        expected: String,
        candidate: String,
        operation: Id,
    },
    Publish {
        context: GitContext,
        expected: String,
        candidate: String,
        operation: Id,
        operations: Value,
    },
    ReusedPublication {
        context: GitContext,
        expected: String,
        candidate: String,
        receipt: Value,
        source_call_id: Id,
        source_observation_id: Id,
    },
}
/// A fact the kernel seals for a verifier: the accepted result of one step, bound to that
/// step's execution and, when there is one, to the platform call or the Human decision behind
/// it. A package describes the same shape in the input Schema of its verifier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceFact {
    pub step: String,
    pub execution_id: Id,
    pub input_digest: String,
    pub output_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_revision: Option<String>,
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_confirmation: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DomainEvidenceCheck {
    pub handler: Id,
    pub input: Value,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PlatformPlan {
    /// One verifier call per contract that guards the operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_checks: Vec<DomainEvidenceCheck>,
    pub action: PlatformAction,
    pub basis: ReadSet,
    pub changes: Vec<ResourceChange>,
    pub head: Option<ResourceSnapshot>,
    pub fence: Option<ConfigurationRecord>,
    pub permit: Option<WorkflowActionPermit>,
}
pub(crate) fn one(store: &dyn EngineeringStore, kind: &str, id: &str) -> Result<ResourceSnapshot> {
    let key = ResourceKey::new(kind, id);
    let mut rows = store.read_resources(std::slice::from_ref(&key))?;
    require(
        rows.len() == 1 && rows[0].resource == key,
        "平台资源读取身份不符",
    )?;
    Ok(rows.remove(0))
}
pub(crate) fn restored(engine: &Engine, row: &ResourceSnapshot) -> Result<ConfigurationRecord> {
    serde_json::from_value(
        engine.store.restore_runtime_record(
            row.value
                .as_ref()
                .ok_or_else(|| err("not_found", "原平台事实缺失"))?,
        )?,
    )
    .map_err(|e| err("storage", e.to_string()))
}
impl Engine {
    fn platform_native_context(
        &self,
        request: &PlatformToolRequest,
        owner: Option<&str>,
    ) -> Result<(GitContext, RepositoryWorktree, ReadSet)> {
        let owner = owner.unwrap_or(&request.owner_id);
        let owner = if owner == "$project" {
            request.project_id.as_str()
        } else {
            owner
        };
        let original = WorkOwnerFacts::read(self.store.as_ref(), &request.owner_id)?;
        let selected = WorkOwnerFacts::read(self.store.as_ref(), owner)?;
        require(
            original.project == request.project_id && selected.project == request.project_id,
            "平台工作空间跨项目",
        )?;
        let within = owner == request.owner_id
            || owner == request.project_id
            || selected.owner.value.as_ref().is_some_and(|t| {
                selected.owner.resource.kind == "task" && t["feature_id"] == request.owner_id
            })
            || (request.definition.qualified_name == "workspace.current"
                && original.owner.value.as_ref().is_some_and(|t| {
                    original.owner.resource.kind == "task" && t["feature_id"] == owner
                }));
        require(within, "工作空间不属于实际执行范围")?;
        let project = one(self.store.as_ref(), "project", &request.project_id)?;
        let project_value: Project = serde_json::from_value(
            project
                .value
                .clone()
                .ok_or_else(|| err("not_found", "项目缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let target_owner = selected
            .owner
            .value
            .as_ref()
            .and_then(|t| {
                (selected.owner.resource.kind == "task")
                    .then(|| t["feature_id"].as_str())
                    .flatten()
            })
            .unwrap_or(&request.project_id);
        let source = one(
            self.store.as_ref(),
            "repository_worktree_owner",
            &json!([request.project_id, owner]).to_string(),
        )?;
        let target = one(
            self.store.as_ref(),
            "repository_worktree_owner",
            &json!([request.project_id, target_owner]).to_string(),
        )?;
        let source_value: RepositoryWorktree = serde_json::from_value(
            source
                .value
                .clone()
                .ok_or_else(|| err("resource_pending", "工作空间尚未就绪"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let target_value: RepositoryWorktree = serde_json::from_value(
            target
                .value
                .clone()
                .ok_or_else(|| err("resource_pending", "目标工作空间尚未就绪"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let reads = original
            .read_set
            .merge(selected.read_set.versions().iter().cloned())?
            .merge([version(&project), version(&source), version(&target)])?;
        Ok((
            GitContext {
                project: request.project_id.clone(),
                workspace: project_value
                    .workspace_definition
                    .ok_or_else(|| err("configuration_required", "项目未绑定仓库"))?,
                source: CodeLocation {
                    owner: owner.into(),
                    branch: source_value.branch.clone(),
                    worktree: source_value.path.clone(),
                },
                target: CodeLocation {
                    owner: target_owner.into(),
                    branch: target_value.branch,
                    worktree: target_value.path,
                },
                origin: Origin::External,
            },
            source_value,
            reads,
        ))
    }
    pub(crate) fn platform_receipt_proof(
        &self,
        root: &str,
        handler: &str,
        receipt: &Value,
    ) -> Result<ResourceVersion> {
        let proof = one(
            self.store.as_ref(),
            "platform_receipt",
            &json!([root, handler, configuration_hash(receipt)]).to_string(),
        )?;
        let row = proof
            .value
            .as_ref()
            .ok_or_else(|| err("scope_violation", "参数缺少该流程根的真实平台回执"))?;
        require(
            row["type"] == "platform_call"
                && row["data"]["receipt"] == *receipt
                && row["data"]["status"] == "succeeded"
                && row["data"]["root_instance_id"] == root
                && row["data"]["handler"] == handler,
            "平台回执字面事实不符",
        )?;
        Ok(version(&proof))
    }
    pub(crate) fn platform_native_grant(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
    ) -> Result<(GitContext, RepositoryWorktree, ReadSet)> {
        let grant = &request.arguments["workspace"];
        let id = text(grant, "workspace_id")?;
        let workspace = one(self.store.as_ref(), "repository_worktree", &id)?;
        let worktree: RepositoryWorktree = serde_json::from_value(
            workspace
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原工作空间缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let (context, actual, mut reads) =
            self.platform_native_context(request, Some(&worktree.owner_id))?;
        require(
            actual.id == id && worktree.project_id == request.project_id,
            "原工作空间身份或归属不符",
        )?;
        let mut accepted = None;
        for handler in [
            "workspace.current",
            "workspace.snapshot",
            "workspace.checkpoint",
        ]
        .into_iter()
        // A synthetic Git candidate is a real immutable revision issued
        // by this root. It authorizes testing that exact commit, without
        // granting it as a writable snapshot/head for other operations.
        .chain(
            matches!(
                request.definition.qualified_name.as_str(),
                "tests.run" | "tests.project"
            )
            .then_some("git.candidate"),
        )
        .chain(
            matches!(
                request.definition.qualified_name.as_str(),
                "tests.run" | "tests.project"
            )
            .then_some("git.delivery.open"),
        ) {
            // Workspace grants may include optional diff metadata. The exact
            // issued receipt is immutable; the projection predicate below
            // selects by workspace/revision rather than inventing a receipt.
            let key = json!([
                resources.instance.root_instance_id,
                id,
                text(grant, "revision")?,
                handler
            ])
            .to_string();
            let row = one(self.store.as_ref(), "platform_workspace_grant", &key)?;
            if let Some(raw) = &row.value {
                require(
                    raw["type"] == "platform_call"
                        && raw["data"]["status"] == "succeeded"
                        && raw["data"]["root_instance_id"] == resources.instance.root_instance_id
                        && raw["data"]["handler"] == handler
                        && raw["data"]["receipt"]["workspace_id"] == id
                        && raw["data"]["receipt"]["revision"] == grant["revision"],
                    "工作空间真实回执不符",
                )?;
                accepted = Some(version(&row));
                break;
            }
        }
        reads = reads.merge([
            version(&workspace),
            accepted.ok_or_else(|| {
                err(
                    "scope_violation",
                    "工作空间引用缺少当前流程根的真实平台来源",
                )
            })?,
        ])?;
        Ok((context, worktree, reads))
    }
    fn platform_native_permit(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
    ) -> Result<(WorkflowActionPermit, ReadSet)> {
        let candidate = &request.arguments["candidate"];
        let revision = text(candidate, "revision")?;
        let mut requirements = std::collections::BTreeMap::new();
        let mut decisions = Vec::new();
        let mut reads = resources.reads.clone();
        for requirement in resources
            .instance
            .plan
            .document
            .action_requirements
            .get(&request.definition.qualified_name)
            .into_iter()
            .flatten()
        {
            let satisfied = match requirement.as_str() {
                "tests" => {
                    let test = &request.arguments["test_receipt"];
                    if test["candidate"] == revision
                        && test["status"] == "succeeded"
                        && test["expected_failure"] == false
                    {
                        reads = reads.merge([self.platform_receipt_proof(
                            &resources.instance.root_instance_id,
                            "tests.run",
                            test,
                        )?])?;
                        true
                    } else {
                        false
                    }
                }
                "confirmation" => {
                    let mut satisfied = false;
                    for execution in resources
                        .instance
                        .steps
                        .iter()
                        .filter(|e| e.inputs.values().any(|v| v == candidate))
                    {
                        let step = one(
                            self.store.as_ref(),
                            "workflow_step",
                            &json!([resources.record.id, execution.id]).to_string(),
                        )?;
                        let actual = self.store.restore_runtime_value(
                            step.value
                                .as_ref()
                                .ok_or_else(|| err("not_found", "确认的原执行缺失"))?,
                        )?;
                        require(actual == json!(execution), "确认原执行在准备期间已变化")?;
                        let confirmations =
                            one(self.store.as_ref(), "workflow_confirmations", &execution.id)?;
                        reads = reads.merge([version(&step), version(&confirmations)])?;
                        satisfied |= confirmations
                            .value
                            .as_ref()
                            .and_then(Value::as_array)
                            .is_some_and(|rows| {
                                rows.iter().any(|r| {
                                    r["data"]["input_digest"] == execution.input_digest
                                        && r["data"]["outputs"] == json!(execution.outputs)
                                })
                            });
                    }
                    satisfied
                }
                _ => {
                    return Err(err(
                        "unsupported",
                        format!("未知平台业务要求 {requirement}"),
                    ));
                }
            };
            if satisfied {
                requirements.insert(requirement.clone(), "satisfied".into());
                continue;
            }
            let waiver = one(
                self.store.as_ref(),
                "workflow_waiver",
                &json!([
                    resources.instance.active_plan_revision_id,
                    resources.execution.node_id,
                    requirement
                ])
                .to_string(),
            )?;
            let row = waiver.value.as_ref().ok_or_else(|| {
                err(
                    "business_requirement_missing",
                    format!("操作要求未满足：{requirement}"),
                )
            })?;
            reads = reads.merge([version(&waiver)])?;
            decisions.push(text(row, "id")?);
            requirements.insert(requirement.clone(), "waived".into());
        }
        Ok((
            WorkflowActionPermit {
                instance_id: request.instance_id.clone(),
                plan_revision_id: resources.instance.active_plan_revision_id.clone(),
                candidate_digest: configuration_hash(candidate),
                target_revision: text(candidate, "target_revision")?,
                requirements,
                decision_refs: decisions,
            },
            reads,
        ))
    }
    pub(crate) fn prepare_platform_plan(
        &self,
        request: &PlatformToolRequest,
    ) -> Result<PlatformPlan> {
        let resources = WorkflowResources::read(
            self.store.as_ref(),
            &request.instance_id,
            request.step_execution_id(),
        )?;
        require(
            resources.instance.owner_id == request.owner_id
                && resources.instance.project_id == request.project_id,
            "平台原工作归属不符",
        )?;
        let mut plan = PlatformPlan {
            domain_checks: Vec::new(),
            action: PlatformAction::Value(Value::Null),
            basis: resources.reads.clone(),
            changes: vec![],
            head: None,
            fence: None,
            permit: None,
        };
        let p = &request.arguments;
        let operation = request
            .runtime
            .as_ref()
            .map(|r| r.operation_id.clone())
            .unwrap_or(stable_object_id(
                self.store.as_ref(),
                &request.execution_id,
                "platform-effect",
            )?);
        // A domain tool is one handler of the active package; the arguments are its input.
        if request.definition.implementation == "domain" {
            plan.action = PlatformAction::DomainRead {
                project: request.project_id.clone(),
                handler: request
                    .definition
                    .handler
                    .clone()
                    .ok_or_else(|| err("invalid_configuration", "领域工具缺少处理器"))?,
                input: p.clone(),
            };
        } else {
            plan.action = match request.definition.qualified_name.as_str() {
                "materials.capture" => PlatformAction::Value(
                    json!({"inputs":p,"captured_at":now(),"operation_id":request.execution_id}),
                ),
                "spec.lint" => {
                    let (value, reads) = self.native_platform_lint(request, &resources)?;
                    plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                    PlatformAction::Value(value)
                }
                "workflow.outputs.read" => {
                    let (value, reads) = self.native_platform_outputs(request, &resources)?;
                    plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                    PlatformAction::Value(value)
                }
                "tasks.create" => {
                    let (value, reads, changes) =
                        self.native_platform_tasks(request, &resources)?;
                    plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                    plan.changes = changes;
                    PlatformAction::Value(value)
                }
                "tasks.complete" => {
                    let tid = text(p, "task_id")?;
                    let owner = WorkOwnerFacts::read(self.store.as_ref(), &tid)?;
                    owner.require_editable()?;
                    let raw = owner.owner.value.as_ref().unwrap();
                    require(
                        owner.owner.resource.kind == "task"
                            && owner.project == request.project_id
                            && (tid == request.owner_id || raw["feature_id"] == request.owner_id),
                        "Task 不属于当前执行范围",
                    )?;
                    let dependencies: Vec<Id> = serde_json::from_value(raw["dependencies"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                    require(
                        dependencies.len() <= WORKFLOW_STEP_LIMIT,
                        "Task 依赖超出预算",
                    )?;
                    plan.basis = plan
                        .basis
                        .merge(owner.read_set.versions().iter().cloned())?;
                    for id in dependencies {
                        let row = one(self.store.as_ref(), "task", &id)?;
                        require(
                            row.value.as_ref().is_some_and(|t| {
                                t["feature_id"] == raw["feature_id"] && t["status"] == "completed"
                            }),
                            "Task 依赖尚未完成",
                        )?;
                        plan.basis = plan.basis.merge([version(&row)])?;
                    }
                    let project = one(self.store.as_ref(), "project", &request.project_id)?;
                    let configured_repository = project
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "Task 项目缺失"))?["workspace_definition"]
                        ["repository"]
                        .as_str()
                        .is_some_and(|repository| !repository.is_empty());
                    plan.basis = plan.basis.merge([version(&project)])?;
                    if configured_repository
                        || raw["worktree"].as_str().is_some_and(|p| !p.is_empty())
                    {
                        let (context, _, reads) =
                            self.platform_native_context(request, Some(&tid))?;
                        plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                        self.platform_plan_fence(&mut plan, request, &context.source.worktree)?;
                    }
                    let mut next = raw.clone();
                    next["status"] = json!(Status::Completed);
                    next["revision"] = json!(owner.owner.revision + 1);
                    plan.changes.push(ResourceChange {
                        resource: owner.owner.resource,
                        value: next,
                    });
                    PlatformAction::Value(
                        json!({"task_id":tid,"status":"completed","operation_id":request.execution_id}),
                    )
                }
                "workspace.current" => {
                    let (context, workspace, reads) =
                        self.platform_native_context(request, p["owner_id"].as_str())?;
                    plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                    PlatformAction::WorkspaceCurrent { context, workspace }
                }
                "workspace.target" | "git.source.publish" | "git.delivery.open"
                | "git.delivery.merge" => {
                    self.prepare_delivery_action(request, &resources, &mut plan)?
                }
                "workspace.snapshot"
                | "workspace.checkpoint"
                | "tests.run"
                | "tests.project"
                | "git.candidate"
                | "git.advance"
                | "git.publish" => {
                    let (context, workspace, reads) =
                        self.platform_native_grant(request, &resources)?;
                    plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                    match request.definition.qualified_name.as_str() {
                        "workspace.snapshot" | "workspace.checkpoint" => {
                            self.platform_plan_fence(&mut plan, request, &context.source.worktree)?;
                            plan.head = Some(one(
                                self.store.as_ref(),
                                "head",
                                &if context.source.owner == request.project_id {
                                    format!("main:{}", request.project_id)
                                } else {
                                    context.source.owner.clone()
                                },
                            )?);
                            let allowed_paths: Vec<String> =
                                serde_json::from_value(p["allowed_paths"].clone())
                                    .map_err(|e| err("invalid_params", e.to_string()))?;
                            let protected_paths: Vec<String> = serde_json::from_value(
                                p.get("protected_paths").cloned().unwrap_or(json!([])),
                            )
                            .map_err(|e| err("invalid_params", e.to_string()))?;
                            require(
                                !allowed_paths.is_empty()
                                    && allowed_paths.len() <= WORKFLOW_STEP_LIMIT
                                    && protected_paths.len() <= WORKFLOW_STEP_LIMIT,
                                "快照原路径集合超出预算",
                            )?;
                            let scope_base = if let Some(base) = p.get("diff_base") {
                                require(
                                    base["workspace_id"] == p["workspace"]["workspace_id"],
                                    "范围 Diff 必须使用同一工作空间",
                                )?;
                                let mut original = request.clone();
                                original.arguments["workspace"] = base.clone();
                                let (_, _, reads) =
                                    self.platform_native_grant(&original, &resources)?;
                                plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                                Some(text(base, "revision")?)
                            } else {
                                None
                            };
                            PlatformAction::Snapshot {
                                operation: RepositoryOperation {
                                    id: operation.clone(),
                                    project_id: request.project_id.clone(),
                                    action: if request.definition.qualified_name
                                        == "workspace.checkpoint"
                                    {
                                        let interfaces = p
                                            .get("interfaces")
                                            .map(TestInterface::parse_list)
                                            .transpose()?
                                            .unwrap_or_default();
                                        RepositoryAction::Checkpoint {
                                            context,
                                            expected: text(&p["workspace"], "revision")?,
                                            allowed_paths,
                                            protected_paths,
                                            interfaces,
                                        }
                                    } else {
                                        RepositoryAction::Snapshot {
                                            run: operation,
                                            context,
                                            expected: text(&p["workspace"], "revision")?,
                                            allowed_paths,
                                            protected_paths,
                                        }
                                    },
                                    status: RepositoryOperationStatus::Dispatched,
                                    receipt: None,
                                    reason: String::new(),
                                    revision: 1,
                                },
                                workspace,
                                include_diff: p["include_diff"] == true,
                                scope_base,
                            }
                        }
                        "tests.run" | "tests.project" => {
                            let configuration = resources.instance.inputs.get("test_configuration");
                            if request.definition.qualified_name == "tests.project"
                                && configuration
                                    .and_then(|c| c.get("regression"))
                                    .is_some_and(Value::is_null)
                            {
                                PlatformAction::Value(
                                    json!({"operation_id":operation,"candidate":text(&p["workspace"], "revision")?,"status":"succeeded","skipped":true,"reason":"no_regression_configured","summary":"未配置项目回归测试，按项目配置直接通过","expected_failure":false,"assertion_failed":false,"writer_revoked":true,"exit_code":0,"counts":{"tests":0,"passed":0,"failed":0},"execution":null,"environment":null}),
                                )
                            } else {
                                let runtime = request.runtime.clone().ok_or_else(|| {
                                    err("configuration_required", "测试工具缺少冻结环境")
                                })?;
                                let mut recipe =
                                    if request.definition.qualified_name == "tests.project" {
                                        configuration.ok_or_else(|| {
                                            err("configuration_required", "项目回归缺少冻结配置")
                                        })?["regression"]
                                            .clone()
                                    } else {
                                        p["recipe"].clone()
                                    };
                                if let Some(configuration) = configuration {
                                    if request.definition.qualified_name == "tests.run" {
                                        require(
                                            recipe["runner"] == configuration["independent_runner"],
                                            "独立测试执行器必须匹配冻结项目配置",
                                        )?;
                                    }
                                    if let Some(image) =
                                        recipe.get("image").and_then(Value::as_str).filter(|_| {
                                            request.definition.qualified_name == "tests.run"
                                        })
                                    {
                                        require(
                                            runtime.sandbox.image.as_deref() == Some(image),
                                            "测试配方镜像与冻结运行环境不同",
                                        )?;
                                    }
                                }
                                recipe["image"] =
                                    json!(runtime.sandbox.image.as_ref().ok_or_else(|| err(
                                        "configuration_required",
                                        "测试镜像未冻结"
                                    ))?);
                                let recipe: TestRecipe = serde_json::from_value(recipe)
                                    .map_err(|e| err("invalid_params", e.to_string()))?;
                                recipe.validate()?;
                                let reporter = self
                                    .domain_reader
                                    .as_ref()
                                    .map(|domain| {
                                        extensions::test_reporter(
                                            domain,
                                            &context.project,
                                            &recipe.runner,
                                        )
                                    })
                                    .transpose()?
                                    .flatten();
                                PlatformAction::Test(ports::TestRequest {
                                    reporter,
                                    operation_id: runtime.operation_id.clone(),
                                    session_id: None,
                                    candidate: text(&p["workspace"], "revision")?,
                                    environment: runtime.id.clone(),
                                    origin: Origin::External,
                                    code: Some(context),
                                    runtime: Some(runtime),
                                    recipe: Some(recipe),
                                    expect_assertion_failure: p["expect_assertion_failure"] == true,
                                })
                            }
                        }
                        "git.candidate" => PlatformAction::Candidate {
                            context,
                            source: text(&p["workspace"], "revision")?,
                            target: text(p, "target_revision")?,
                            operation,
                            workspace: workspace.id,
                        },
                        "git.advance" => {
                            let candidate = &p["candidate"];
                            plan.basis = plan.basis.merge([self.platform_receipt_proof(
                                &resources.instance.root_instance_id,
                                "git.candidate",
                                candidate,
                            )?])?;
                            let (permit, reads) =
                                self.platform_native_permit(request, &resources)?;
                            plan.permit = Some(permit);
                            plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
                            self.platform_plan_fence(&mut plan, request, &context.target.worktree)?;
                            plan.head = Some(one(
                                self.store.as_ref(),
                                "head",
                                &if context.target.owner == request.project_id {
                                    format!("main:{}", request.project_id)
                                } else {
                                    context.target.owner.clone()
                                },
                            )?);
                            PlatformAction::Advance {
                                context,
                                expected: text(candidate, "target_revision")?,
                                candidate: text(candidate, "revision")?,
                                operation,
                            }
                        }
                        _ => {
                            let candidate = &p["candidate"];
                            let accepted = &p["accepted"];
                            require(
                                accepted["revision"] == candidate["revision"]
                                    && accepted["previous"] == candidate["target_revision"],
                                "发布需要确切候选的本地接纳",
                            )?;
                            plan.basis = plan.basis.merge([self.platform_receipt_proof(
                                &resources.instance.root_instance_id,
                                "git.advance",
                                accepted,
                            )?])?;
                            let operations =
                                one(self.store.as_ref(), "governance", &request.execution_id)?;
                            plan.basis = plan.basis.merge([version(&operations)])?;
                            if let Some(reused) = crate::publication_retry::completed_publication(
                                self.store.as_ref(),
                                request,
                                &resources.execution,
                                &resources.instance.root_instance_id,
                                &context,
                            )? {
                                plan.basis =
                                    plan.basis.merge(reused.reads.versions().iter().cloned())?;
                                reused.action
                            } else {
                                PlatformAction::Publish {
                                    context,
                                    expected: text(candidate, "target_revision")?,
                                    candidate: text(candidate, "revision")?,
                                    operation: request.execution_id.clone(),
                                    operations:
                                        operations.value.as_ref().unwrap()["data"]["operations"]
                                            .clone(),
                                }
                            }
                        }
                    }
                }
                _ => return Err(err("unsupported", "平台执行工具未声明该处理器")),
            };
        }
        // Evidence the workflow declares for this operation besides the repository delivery
        // contract, which the engineering part prepares with what it knows about the repository.
        let operation = request.definition.qualified_name.as_str();
        for (contract, binding) in &resources.instance.plan.document.operation_evidence {
            if contract == REPOSITORY_DELIVERY_EVIDENCE || !binding.checks.contains_key(operation) {
                continue;
            }
            let check =
                self.prepare_operation_evidence(contract, binding, request, &resources, &mut plan)?;
            plan.domain_checks.push(check);
        }
        if let Some(head) = &plan.head {
            require(head.value.is_some(), "代码头尚未登记")?;
            plan.basis = plan.basis.merge([version(head)])?;
        }
        Ok(plan)
    }
    pub(crate) fn platform_plan_fence(
        &self,
        plan: &mut PlatformPlan,
        request: &PlatformToolRequest,
        worktree: &str,
    ) -> Result<()> {
        let writers = one(
            self.store.as_ref(),
            "conversation_writers",
            &request.project_id,
        )?;
        if !writers
            .value
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.iter().any(|w| w["worktree"] == worktree))
        {
            return Err(err(
                "resource_pending",
                "目标工作空间仍有活动写者或未核对的外部操作",
            ));
        }
        plan.basis = plan.basis.merge([version(&writers)])?;
        let mut fence = record(
            "platform_workspace_fence",
            request.execution_id.clone(),
            Scope::project(&request.project_id),
            "平台工作空间操作资格".into(),
            1,
            None,
            json!({"status":"active","worktree":worktree,"call_id":request.execution_id,"origin":"kernel_platform"}),
            false,
        );
        fence.id = format!("platform-workspace-fence:{}", request.execution_id);
        plan.fence = Some(fence);
        Ok(())
    }
    fn native_platform_tasks(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
    ) -> Result<(Value, ReadSet, Vec<ResourceChange>)> {
        let owner = WorkOwnerFacts::read(self.store.as_ref(), &request.owner_id)?;
        owner.require_editable()?;
        require(
            owner.owner.resource.kind == "feature" && owner.project == request.project_id,
            "创建 Task 需要实际 Feature 所有者",
        )?;
        let items = request.arguments["tasks"]
            .as_array()
            .ok_or_else(|| err("invalid_params", "tasks 应为数组"))?;
        require(
            !items.is_empty() && items.len() <= WORKFLOW_STEP_LIMIT,
            "Task 集合为空或超过预算",
        )?;
        let nodes = items
            .iter()
            .map(|v| {
                Ok((
                    text(v, "key")?,
                    serde_json::from_value::<Vec<String>>(
                        v.get("dependencies").cloned().unwrap_or(json!([])),
                    )
                    .map_err(|e| err("invalid_params", e.to_string()))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let order = topological(&nodes)?;
        for item in items {
            if let Some(reasons) = item.get("dependency_reasons") {
                let reasons = reasons
                    .as_array()
                    .ok_or_else(|| err("invalid_params", "依赖依据必须为数组"))?;
                let deps = item["dependencies"]
                    .as_array()
                    .ok_or_else(|| err("invalid_params", "依赖必须为数组"))?;
                require(
                    reasons.len() == deps.len()
                        && deps.iter().all(|dep| {
                            reasons
                                .iter()
                                .filter(|r| {
                                    r["dependency"] == *dep
                                        && r["artifact"]
                                            .as_str()
                                            .is_some_and(|s| !s.trim().is_empty())
                                        && r["reason"]
                                            .as_str()
                                            .is_some_and(|s| !s.trim().is_empty())
                                })
                                .count()
                                == 1
                        }),
                    "每条直接依赖必须且只能有一份明确产物及原因",
                )?;
            }
        }
        let ids = order
            .iter()
            .map(|key| {
                Ok((
                    key.clone(),
                    stable_object_id(
                        self.store.as_ref(),
                        &request.execution_id,
                        &format!("task:{key}"),
                    )?,
                ))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
        let mut reads = owner.read_set;
        let mut changes = vec![];
        let mut members = vec![];
        let parent = one(
            self.store.as_ref(),
            "work_configuration_source",
            &request.owner_id,
        )?;
        reads = reads.merge([version(&parent)])?;
        let original_catalog = self.store.configuration_snapshot()?;
        reads = reads.merge([ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: original_catalog.revision,
        }])?;
        let parent = parent
            .value
            .as_ref()
            .map(|_| restored(self, &parent))
            .transpose()?;
        let mut catalog = original_catalog.state.as_ref().clone();
        for key in order {
            let item = items
                .iter()
                .find(|v| v["key"] == key)
                .ok_or_else(|| err("storage", "排序后的 Task 缺失"))?;
            let tid = ids[&key].clone();
            let dependencies = nodes
                .iter()
                .find(|(k, _)| k == &key)
                .unwrap()
                .1
                .iter()
                .map(|k| ids[k].clone())
                .collect::<Vec<_>>();
            let task = Task {
                id: tid.clone(),
                feature_id: request.owner_id.clone(),
                title: text(item, "title")?,
                status: Status::Draft,
                revision: 1,
                inherited_snapshot: None,
                dependencies: dependencies.clone(),
                worktree: String::new(),
                scope: TaskScope::default(),
            };
            let context = Context {
                id: stable_object_id(
                    self.store.as_ref(),
                    &request.execution_id,
                    &format!("task-context:{key}"),
                )?,
                owner_id: tid.clone(),
                content: item.to_string(),
                revision: 1,
            };
            changes.push(ResourceChange {
                resource: ResourceKey::new("task", &tid),
                value: json!(task),
            });
            changes.push(ResourceChange {
                resource: ResourceKey::new("context", &context.id),
                value: json!(context),
            });
            if let Some(parent) = &parent {
                let standard = resources
                    .instance
                    .plan
                    .standard_ref
                    .as_deref()
                    .or_else(|| parent.data["standard_ref"].as_str());
                if let Some(standard) = standard {
                    let definition = standard_snapshot(
                        &catalog,
                        standard,
                        &Scope::project(&request.project_id),
                    )?;
                    if let Some(entry) =
                        resources.instance.plan.document.steps.iter().find_map(|s| {
                            match &s.executor {
                                StepExecutor::Workflow {
                                    workflow_ref,
                                    work_binding: Some(_),
                                    ..
                                } if definition.nodes.iter().any(|n| {
                                    n.kind == "workflow" && &n.reference == workflow_ref
                                }) =>
                                {
                                    Some(workflow_ref.clone())
                                }
                                _ => None,
                            }
                        })
                    {
                        let before = catalog.governance.len();
                        crate::standards::select_work_configuration_for(
                            &mut catalog,
                            &WorkReference::Task { id: tid.clone() },
                            &owner.scope,
                            &json!({"standard_ref":standard,"workflow_ref":entry,"role_agent_bindings":resources.instance.role_agent_bindings,"harness_permission_ref":resources.instance.plan.harness_permission_ref,"test_configuration":resources.instance.inputs.get("test_configuration").cloned().unwrap_or_else(||parent.data["test_configuration"].clone())}),
                            Some(parent),
                        )?;
                        for row in &catalog.governance[before..] {
                            changes.push(ResourceChange {
                                resource: ResourceKey::new("governance", &row.id),
                                value: self.store.encode_runtime_record(row)?,
                            });
                        }
                    }
                }
            }
            let mut member = item.clone();
            member["id"] = json!(tid);
            member["work_ref"] = json!({"kind":"task","id":tid});
            member["feature_id"] = json!(request.owner_id);
            member["dependency_keys"] = item.get("dependencies").cloned().unwrap_or(json!([]));
            member["dependencies"] = json!(dependencies);
            members.push(member);
        }
        reads = reads.merge(changes.iter().map(|c| ResourceVersion {
            resource: c.resource.clone(),
            revision: 0,
        }))?;
        Ok((json!({"tasks":members}), reads, changes))
    }
}
