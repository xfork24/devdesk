//! Each remote write has one durable grant. PR polling is read only; publishing
//! a candidate never advances or merges the remote target branch.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    workflow_resources::version,
    *,
};

#[derive(Serialize, Deserialize)]
pub(crate) enum PublishAction {
    DeliveryTargetHead {
        context: GitContext,
        expected: String,
    },
    Repository(Box<RepositoryOperation>),
    PullRequest(PullRequestRequest),
    DeliveryTarget {
        context: GitContext,
    },
    DeliveryPullRequest(PullRequestRequest),
    PlannedDeliveryPullRequest {
        request: PullRequestRequest,
        creation: PullRequestCreation,
    },
    DeliveryStatus(PullRequestMergeRequest),
    DeliveryCandidate(PullRequestRequest),
    DeliveryMerge(PullRequestMergeRequest),
    DeliverySync {
        context: GitContext,
        expected: String,
        revision: String,
        operation: Id,
    },
}
impl Engine {
    pub(crate) fn delivery_candidate_branch(
        &self,
        effect: &ConfigurationRecord,
        phase: &str,
        project: &str,
        candidate: &str,
        inspect_only: bool,
    ) -> Result<String> {
        let previous = one(
            self.store.as_ref(),
            "governance",
            &format!("platform-publish-stage:{}:{phase}", effect.id),
        )?;
        if previous.value.is_some() {
            let saved = restored(self, &previous)?;
            require(
                saved.r#type == "platform_publish_stage"
                    && saved.definition_id == effect.id
                    && saved.data["parent_effect"] == effect.id,
                "候选分支缺少原发布意图",
            )?;
            let action: PublishAction = serde_json::from_value(saved.data["action"].clone())
                .map_err(|error| err("storage", error.to_string()))?;
            if let PublishAction::Repository(operation) = action
                && operation.project_id == project
                && let RepositoryAction::PublishRef { branch, merge, .. } = operation.action
                && merge == candidate
            {
                return Ok(branch);
            }
            return Err(err("storage", "候选分支与原发布身份不符"));
        }
        if !inspect_only
            && let Some(domain) = &self.domain_reader
            && let Some(branch) =
                extensions::worktree_branch(domain, project, candidate, WorktreeKind::Candidate)?
        {
            return Ok(branch);
        }
        Ok(format!("devdesk/candidates/{candidate}"))
    }

    pub(crate) fn platform_publish_stage(
        &self,
        effect: &ConfigurationRecord,
        name: &str,
        action: PublishAction,
        inspect_only: bool,
    ) -> Result<Option<Value>> {
        let key = ResourceKey::new(
            "governance",
            format!("platform-publish-stage:{}:{name}", effect.id),
        );
        let previous = one(self.store.as_ref(), "governance", &key.id)?;
        let original = previous
            .value
            .as_ref()
            .map(|_| restored(self, &previous))
            .transpose()?;
        let action = if let PublishAction::DeliveryPullRequest(request) = action {
            let creation = if let Some(saved) = &original {
                match serde_json::from_value::<PublishAction>(saved.data["action"].clone())
                    .map_err(|error| err("storage", error.to_string()))?
                {
                    PublishAction::PlannedDeliveryPullRequest { creation, .. } => Some(creation),
                    _ => None,
                }
            } else if !inspect_only
                && request.number.is_none()
                && let Some(domain) = &self.domain_reader
            {
                extensions::pull_request_creation(domain, &request)?
            } else {
                None
            };
            if let Some(creation) = creation {
                creation.validate(&request)?;
                PublishAction::PlannedDeliveryPullRequest { request, creation }
            } else {
                PublishAction::DeliveryPullRequest(request)
            }
        } else {
            action
        };
        let mut stage = record(
            "platform_publish_stage",
            effect.id.clone(),
            effect.scope.clone(),
            name.into(),
            1,
            None,
            json!({"status":"granted","parent_effect":effect.id,"action":action,"issuer":"kernel_platform"}),
            true,
        );
        stage.id = key.id.clone();
        let fresh = if let Some(saved) = original {
            require(
                saved.r#type == stage.r#type
                    && saved.definition_id == effect.id
                    && saved.data == stage.data,
                "发布阶段改变了原操作",
            )?;
            stage = saved;
            false
        } else if inspect_only {
            return Ok(None);
        } else {
            let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let call: ResourceVersion = serde_json::from_value(effect.data["call_version"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            self.store
                .submit_changes(&PreparedChangeSet {
                    command_id: key.id.clone(),
                    fingerprint: configuration_hash(&stage.data),
                    read_set: plan.basis.merge([
                        call,
                        version(&previous),
                        ResourceVersion {
                            resource: ResourceKey::new("governance", &effect.id),
                            revision: effect.revision,
                        },
                    ])?,
                    changes: vec![ResourceChange {
                        resource: key,
                        value: stage.to_storage(),
                    }],
                    result: json!({"id":stage.id}),
                    time: now() as i64,
                })?
                .applied
        };
        if let Some((_, result)) = self.platform_observed(&stage, "result")?
            && (matches!(&result, Ok(Some(_)))
                || (result.is_err()
                    && !inspect_only
                    && !matches!(action, PublishAction::DeliverySync { .. })))
        {
            return result;
        }
        let result = match &action {
            PublishAction::DeliveryTargetHead { context, expected } => {
                let revision = self.git.observe_delivery_target(context)?;
                require(
                    revision.is_some() || context.target.owner != context.project,
                    "项目主分支缺失",
                )?;
                Ok(Some(
                    json!({"revision":revision.as_deref().unwrap_or(expected),"remote_exists":revision.is_some()}),
                ))
            }
            PublishAction::DeliveryTarget { context } => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
                .delivery_target(context)
                .map(Some),
            PublishAction::DeliveryPullRequest(request) => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
                .ensure_delivery(request, !fresh)
                .map(|v| v.map(|v| json!(v))),
            PublishAction::PlannedDeliveryPullRequest { request, creation } => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
                .ensure_planned_delivery(request, creation, !fresh)
                .map(|value| value.map(|receipt| json!(receipt))),
            PublishAction::DeliveryCandidate(request) => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "缺少 PR 适配器"))?
                .delivery_candidate(request)
                .map(|value| value.map(|candidate| json!(candidate))),
            PublishAction::DeliveryStatus(request) => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
                .delivery_status(request, !fresh),
            PublishAction::DeliveryMerge(request) => self
                .pull_requests
                .as_ref()
                .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
                .merge_delivery(request, !fresh)
                .map(|v| v.map(|v| json!(v))),
            PublishAction::DeliverySync {
                context,
                expected,
                revision,
                operation,
            } => self
                .git
                .sync_delivery(context, expected, revision, operation, !fresh),
            PublishAction::Repository(op) => {
                if fresh {
                    self.git.repository_operation(op).map(|r| Some(json!(r)))
                } else {
                    self.git.inspect_repository_operation(op).and_then(|r| {
                        r.map(|r| Some(json!(r))).ok_or_else(|| {
                            err("needs_reconciliation", "原发布阶段结果未知，不重新推送")
                        })
                    })
                }
            }
            PublishAction::PullRequest(request) => {
                let adapter = self
                    .pull_requests
                    .as_ref()
                    .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?;
                if fresh {
                    adapter.ensure(request).map(|r| Some(json!(r)))
                } else {
                    adapter.inspect(request).and_then(|r| {
                        r.map(|r| Some(json!(r))).ok_or_else(|| {
                            err("needs_reconciliation", "原 PR 创建结果未知，不重新提交")
                        })
                    })
                }
            }
        };
        if !inspect_only {
            self.platform_observe(&stage, "result", &result)?;
        }
        result
    }
    pub(crate) fn drive_platform_publish(
        &self,
        effect: &ConfigurationRecord,
        action: &PlatformAction,
    ) -> Result<Option<Value>> {
        self.read_or_drive_platform_publication(effect, action, false)
    }
    pub(crate) fn inspect_platform_publication(
        &self,
        effect: &ConfigurationRecord,
        action: &PlatformAction,
    ) -> Result<Option<Value>> {
        self.read_or_drive_platform_publication(effect, action, true)
    }
    fn read_or_drive_platform_publication(
        &self,
        effect: &ConfigurationRecord,
        action: &PlatformAction,
        inspect_only: bool,
    ) -> Result<Option<Value>> {
        let PlatformAction::Publish {
            context,
            expected,
            candidate,
            operation,
            operations,
        } = action
        else {
            return Err(err("storage", "原发布意图类型不符"));
        };
        let physical = crate::material_commands::stable_object_id(
            self.store.as_ref(),
            operation,
            "publication",
        )?;
        let candidate_branch = self.delivery_candidate_branch(
            effect,
            "candidate",
            &context.project,
            &physical,
            inspect_only,
        )?;
        for (name, branch, revision, stage) in [
            (
                "base",
                context.target.branch.clone(),
                expected.clone(),
                PublishStage::TargetBase,
            ),
            (
                "candidate",
                candidate_branch.clone(),
                candidate.clone(),
                PublishStage::Candidate,
            ),
        ] {
            let op = RepositoryOperation {
                id: text(operations, name)?,
                project_id: context.project.clone(),
                action: RepositoryAction::PublishRef {
                    merge: physical.clone(),
                    context: context.clone(),
                    branch: branch.clone(),
                    revision: revision.clone(),
                    expected: None,
                    stage,
                },
                status: RepositoryOperationStatus::Dispatched,
                receipt: None,
                reason: String::new(),
                revision: 1,
            };
            let Some(value) = self.platform_publish_stage(
                effect,
                name,
                PublishAction::Repository(Box::new(op)),
                inspect_only,
            )?
            else {
                return Ok(None);
            };
            let receipt: RepositoryReceipt = serde_json::from_value(value)
                .map_err(|e| err("adapter_contract", e.to_string()))?;
            require(
                matches!(receipt,RepositoryReceipt::PublishedRef{revision:r,branch:b} if r==revision && b==branch),
                "发布 ref 回执改变了冻结分支或提交",
            )?;
        }
        let request = PullRequestRequest {
            repository: context.workspace.repository.clone(),
            project: context.project.clone(),
            merge: physical,
            head_branch: candidate_branch,
            base_branch: context.target.branch.clone(),
            candidate: candidate.clone(),
            expected_target: expected.clone(),
            number: None,
            projection: None,
        };
        let Some(value) = self.platform_publish_stage(
            effect,
            "pull_request",
            PublishAction::PullRequest(request.clone()),
            inspect_only,
        )?
        else {
            return Ok(None);
        };
        let original: PullRequestReceipt =
            serde_json::from_value(value).map_err(|e| err("adapter_contract", e.to_string()))?;
        require(
            original.repository == request.repository
                && original.marker == request.marker()
                && original.head == *candidate
                && original.head_branch == request.head_branch
                && original.base_branch == request.base_branch,
            "PR 回执不属于原冻结发布请求",
        )?;
        let current = self
            .pull_requests
            .as_ref()
            .ok_or_else(|| err("unsupported", "未配置 PR 适配器"))?
            .read(&PullRequestRequest {
                number: Some(original.number),
                ..request.clone()
            })?;
        require(
            current.number == original.number
                && current.repository == request.repository
                && current.marker == request.marker()
                && current.head == *candidate
                && current.head_branch == request.head_branch
                && current.base_branch == request.base_branch,
            "PR 当前事实改变了原冻结请求",
        )?;
        self.platform_observe(
            effect,
            &format!("pr-state:{}", configuration_hash(&current)),
            &Ok(Some(json!(current))),
        )?;
        if !current.merged {
            return Ok(None);
        }
        require(
            current.merge_commit.as_deref() == Some(candidate.as_str()),
            "PR 合入结果不是原确切候选，保留核对",
        )?;
        Ok(Some(
            json!({"revision":candidate,"pull_request":current,"operation_id":operation}),
        ))
    }
}
