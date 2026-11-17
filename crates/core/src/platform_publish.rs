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
enum PublishAction {
    Repository(Box<RepositoryOperation>),
    PullRequest(PullRequestRequest),
}
impl Engine {
    fn platform_publish_stage(
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
        let fresh = if previous.value.is_some() {
            let saved = restored(self, &previous)?;
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
            && (!inspect_only || result.is_ok())
        {
            return result;
        }
        let result = match &action {
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
        let candidate_branch = format!("devdesk/candidates/{physical}");
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
