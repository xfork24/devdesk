//! PR publication observes existing repository rules without installing policy.
//! Unprotected targets use an exact remote CAS; protected targets use PR gates.
use super::*;
use crate::github::GitHubPullRequests;
use std::time::Duration;
const CHECK: &str = "devdesk/merge-regression";
fn rejected_by_remote_gate(value: &Value) -> bool {
    let code = value["status"]
        .as_u64()
        .or_else(|| value["status"].as_str()?.parse().ok());
    matches!(code, Some(403 | 405 | 409 | 422))
}
fn segment(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
impl GitHubPullRequests {
    fn delivery_api(&self, method: &str, endpoint: &str, input: Option<Value>) -> Result<Value> {
        let mut command = self.api_command()?;
        command.args(["--method", method, endpoint]);
        if input.is_some() {
            command.args(["--input", "-"]);
        }
        let output = crate::process::capture_command(
            command,
            Duration::from_secs(25),
            input.map(|v| serde_json::to_vec(&v).unwrap()),
        )?;
        let value = serde_json::from_slice::<Value>(&output.stdout);
        if output.code != 0 || output.truncated {
            let message = "GitHub 精简交付请求未成功；保留原请求及远端事实";
            return Err(
                if value.as_ref().ok().is_some_and(rejected_by_remote_gate) {
                    err("remote_gate_blocked", message).technical_not_applied()
                } else {
                    err("needs_reconciliation", message).technical_unknown()
                }.with_context("GitHub delivery API", json!({"method":method,"endpoint":endpoint,"exit_code":output.code,"truncated":output.truncated,"response":value.as_ref().ok(),"response_text":value.as_ref().err().map(|_|String::from_utf8_lossy(&output.stdout).chars().take(8192).collect::<String>())})),
            );
        }
        value.map_err(storage)
    }
    pub fn diagnose_connection(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<GitHubRepositoryPolicy> {
        let mut policy = self.diagnose(repository, branch)?;
        // These are restrictions of the legacy direct-push path, not reasons
        // to reject a normal PR. Existing rules stay on GitHub and are enforced there.
        policy.blocked_reasons.retain(|reason| {
            !matches!(
                reason.as_str(),
                "仓库存在传统分支保护；本轮本地 CAS 策略不支持，不会修改或绕过规则"
                    | "存在分支/推送规则集；本轮策略保守阻塞，不使用管理员 bypass 或平台合入替代"
            )
        });
        policy.compatible = policy.blocked_reasons.is_empty();
        Ok(policy)
    }
    pub(crate) fn observe_delivery_target(&self, context: &GitContext) -> Result<Value> {
        self.with_delivery(|client| {
            client.delivery_target(&context.workspace.repository, &context.target.branch)
        })
    }
    pub(crate) fn delivery_ensure(
        &self,
        request: &PullRequestRequest,
        inspect: bool,
        creation: Option<&PullRequestCreation>,
    ) -> Result<Option<PullRequestReceipt>> {
        self.with_delivery(|client| client.delivery_ensure(request, inspect, creation))
    }
    pub(crate) fn observe_delivery_candidate(
        &self,
        request: &PullRequestRequest,
    ) -> Result<Option<PullRequestMergeCandidate>> {
        self.with_delivery(|client| client.delivery_candidate(request))
    }
    pub(crate) fn delivery_report_status(
        &self,
        request: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<Value>> {
        self.with_delivery(|client| client.delivery_report_status(request, inspect))
    }
    pub(crate) fn delivery_ready(&self, request: &PullRequestMergeRequest) -> Result<bool> {
        self.with_delivery(|client| client.delivery_ready(request))
    }
    pub(crate) fn delivery_merge(
        &self,
        request: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        self.with_delivery(|client| client.delivery_merge(request, inspect))
    }
    fn with_delivery<T>(
        &self,
        operation: impl FnOnce(&DeliveryClient<'_>) -> Result<T>,
    ) -> Result<T> {
        operation(&DeliveryClient {
            api: &|method, endpoint, input| self.delivery_api(method, endpoint, input),
        })
    }
}

type DeliveryApi<'a> = dyn Fn(&str, &str, Option<Value>) -> Result<Value> + 'a;
struct DeliveryClient<'a> {
    api: &'a DeliveryApi<'a>,
}
impl DeliveryClient<'_> {
    fn delivery_api(&self, method: &str, endpoint: &str, input: Option<Value>) -> Result<Value> {
        (self.api)(method, endpoint, input)
    }
    fn receipt(&self, request: &PullRequestRequest, value: &Value) -> Result<PullRequestReceipt> {
        GitHubPullRequests::receipt(request, value)
    }
    fn delivery_target(&self, repository: &str, branch: &str) -> Result<Value> {
        GitHubPullRequests::validate_repository_branch(repository, branch)?;
        self.delivery_api(
            "GET",
            &format!("repos/{repository}/branches/{}", segment(branch)),
            None,
        )
    }

    fn advance_unprotected_target(
        &self,
        request: &PullRequestRequest,
        revision: &str,
    ) -> Result<()> {
        let repository =
            self.delivery_api("GET", &format!("repos/{}", request.repository), None)?;
        let repository_id = repository["node_id"].as_str().ok_or_else(|| {
            err("adapter_contract", "GitHub 仓库缺少 node_id").technical_not_applied()
        })?;
        // updateRefs compares beforeOid atomically; force=false permits only a
        // fast-forward and GitHub still enforces any rules added concurrently.
        let result = self.delivery_api("POST","graphql",Some(json!({
            "query":"mutation($input:UpdateRefsInput!){updateRefs(input:$input){clientMutationId}}",
            "variables":{"input":{"repositoryId":repository_id,"clientMutationId":request.merge,
                "refUpdates":[{"name":format!("refs/heads/{}",request.base_branch),"beforeOid":request.expected_target,"afterOid":revision,"force":false}]}}
        })))?;
        if let Some(errors) = result["errors"]
            .as_array()
            .filter(|errors| !errors.is_empty())
        {
            let rejected = errors.iter().all(|error| {
                matches!(
                    error["type"].as_str(),
                    Some(
                        "FORBIDDEN"
                            | "UNPROCESSABLE"
                            | "STALE_DATA"
                            | "NOT_FOUND"
                            | "INSUFFICIENT_SCOPES"
                    )
                )
            });
            let error = err(if rejected {"remote_gate_blocked"} else {"needs_reconciliation"},"GitHub 未确认目标分支的原子推进")
                .with_context("GitHub updateRefs",json!({"repository":request.repository,"branch":request.base_branch,"expected_target":request.expected_target,"candidate":request.candidate,"response":result}));
            return Err(if rejected {
                error.technical_not_applied()
            } else {
                error.technical_unknown()
            });
        }
        require(
            !result["data"]["updateRefs"].is_null(),
            "GitHub 缺少目标推进回执",
        )?;
        Ok(())
    }
    fn delivery_request(&self, r: &PullRequestRequest) -> Result<()> {
        require(
            uuid::Uuid::parse_str(&r.project).is_ok()
                && uuid::Uuid::parse_str(&r.merge).is_ok()
                && (r.head_branch == format!("devdesk/candidates/{}", r.merge)
                    || ["devdesk/tasks/", "devdesk/features/"]
                        .iter()
                        .any(|prefix| {
                            r.head_branch
                                .strip_prefix(prefix)
                                .is_some_and(|owner| uuid::Uuid::parse_str(owner).is_ok())
                        }))
                && [r.candidate.as_str(), r.expected_target.as_str()]
                    .iter()
                    .all(|v| v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit())),
            "精简 PR 归属或版本无效",
        )?;
        GitHubPullRequests::validate_repository_branch(&r.repository, &r.base_branch)?;
        if let Some(projection) = &r.projection {
            projection.validate()?;
            require(
                r.head_branch == format!("devdesk/candidates/{}", r.merge)
                    && projection.target_revision == r.expected_target,
                "投影 PR 的候选分支或冻结目标不符",
            )?;
        }
        let commit = self.delivery_api(
            "GET",
            &format!("repos/{}/git/commits/{}", r.repository, r.candidate),
            None,
        )?;
        if r.head_branch.starts_with("devdesk/candidates/") {
            require(
                commit["parents"]
                    .as_array()
                    .is_some_and(|p| p.len() == 1 && p[0]["sha"] == r.expected_target),
                "历史候选父提交与冻结目标不符",
            )?;
        }
        Ok(())
    }
    fn delivery_candidate(
        &self,
        request: &PullRequestRequest,
    ) -> Result<Option<PullRequestMergeCandidate>> {
        self.delivery_request(request)?;
        let row = self.delivery_pr(request)?;
        self.receipt(request, &row)?;
        if row["mergeable"].is_null() {
            return Ok(None);
        }
        require(
            row["mergeable"] == true,
            "PR 存在合并冲突，需修复源分支后重新验证",
        )
        .map_err(|error| error.technical_not_applied())?;
        let actual = self.delivery_target_head(request)?;
        if actual != request.expected_target {
            return Err(err("stale_target","PR 目标基线已变化，需要重新生成合并候选")
                .technical_not_applied().with_context("PR candidate target",json!({"expected":request.expected_target,"actual":actual,"number":request.number})));
        }
        let Some(revision) = row["merge_commit_sha"].as_str() else {
            return Ok(None);
        };
        let commit = self.delivery_api(
            "GET",
            &format!("repos/{}/git/commits/{revision}", request.repository),
            None,
        )?;
        require(
            commit["parents"].as_array().is_some_and(|parents| {
                parents.len() == 2
                    && parents[0]["sha"] == request.expected_target
                    && parents[1]["sha"] == request.candidate
            }),
            "PR 试合并的源或目标版本不符",
        )
        .map_err(|error| error.technical_not_applied())?;
        let tree = commit["tree"]["sha"]
            .as_str()
            .ok_or_else(|| err("adapter_contract", "PR 试合并缺少 tree"))?;
        require(
            [revision, tree]
                .iter()
                .all(|sha| sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit())),
            "PR 试合并对象无效",
        )?;
        Ok(Some(PullRequestMergeCandidate {
            revision: revision.into(),
            tree: tree.into(),
            source_revision: request.candidate.clone(),
            target_revision: request.expected_target.clone(),
        }))
    }
    fn validate_merge_candidate(&self, request: &PullRequestMergeRequest) -> Result<()> {
        if let Some(revision) = &request.merge_revision {
            require(
                revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()),
                "缺少冻结试合并版本",
            )?;
            let commit = self.delivery_api(
                "GET",
                &format!(
                    "repos/{}/git/commits/{revision}",
                    request.request.repository
                ),
                None,
            )?;
            require(
                commit["tree"]["sha"] == request.tree
                    && commit["parents"].as_array().is_some_and(|parents| {
                        parents.len() == 2
                            && parents[0]["sha"] == request.request.expected_target
                            && parents[1]["sha"] == request.request.candidate
                    }),
                "冻结试合并与已测试源、目标或 tree 不符",
            )
            .map_err(|error| error.technical_not_applied())?;
        }
        Ok(())
    }
    fn delivery_pr(&self, r: &PullRequestRequest) -> Result<Value> {
        let number = r
            .number
            .ok_or_else(|| err("blocked", "交付 PR 缺少 number"))?;
        self.delivery_api(
            "GET",
            &format!("repos/{}/pulls/{number}", r.repository),
            None,
        )
    }
    fn delivery_target_head(&self, r: &PullRequestRequest) -> Result<String> {
        // PR.base.sha can remain the creation snapshot while the branch moves.
        // Read the actual branch. Unprotected targets additionally compare
        // beforeOid on the server; protected targets retain their merge rules.
        let branch = self.delivery_api(
            "GET",
            &format!(
                "repos/{}/branches/{}",
                r.repository,
                segment(&r.base_branch)
            ),
            None,
        )?;
        let head = branch["commit"]["sha"]
            .as_str()
            .ok_or_else(|| err("adapter_contract", "目标分支缺少实际提交"))?;
        require(
            head.len() == 40 && head.bytes().all(|v| v.is_ascii_hexdigit()),
            "目标分支实际提交无效",
        )?;
        Ok(head.into())
    }
    pub(crate) fn delivery_ensure(
        &self,
        r: &PullRequestRequest,
        inspect: bool,
        creation: Option<&PullRequestCreation>,
    ) -> Result<Option<PullRequestReceipt>> {
        self.delivery_request(r)?;
        if let Some(creation) = creation {
            creation.validate(r)?;
        }
        if r.number.is_some() {
            return self.receipt(r, &self.delivery_pr(r)?).map(Some);
        }
        let rows = self.delivery_api(
            "GET",
            &format!(
                "repos/{}/pulls?state=all&head={}:{}&base={}&per_page=100",
                r.repository,
                r.repository.split('/').next().unwrap(),
                segment(&r.head_branch),
                segment(&r.base_branch)
            ),
            None,
        )?;
        let rows = rows
            .as_array()
            .ok_or_else(|| err("adapter_contract", "PR 列表无效"))?;
        require(rows.len() < 100, "交付 PR 列表未完整读取，不推测归属")?;
        let matching = rows
            .iter()
            .filter(|row| {
                row["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(&r.marker()))
            })
            .collect::<Vec<_>>();
        require(matching.len() <= 1, "当前交付的 PR 不唯一，不推测归属")?;
        let row = if let Some(row) = matching.first() {
            self.delivery_api(
                "GET",
                &format!(
                    "repos/{}/pulls/{}",
                    r.repository,
                    row["number"]
                        .as_u64()
                        .ok_or_else(|| err("adapter_contract", "PR number 缺失"))?
                ),
                None,
            )?
        } else if inspect {
            return Ok(None);
        } else {
            require(
                !r.head_branch.starts_with("devdesk/candidates/") || r.projection.is_some(),
                "历史 candidates 发布不能新建 PR；请以原开发分支重新发布",
            )
            .map_err(|error| error.technical_not_applied())?;
            require(
                rows.iter().all(|row| row["state"] != "open"),
                "此源分支已有其他交付的未关闭 PR，请先核对归属",
            )
            .map_err(|error| error.technical_not_applied())?;
            let branch = self.delivery_api(
                "GET",
                &format!(
                    "repos/{}/branches/{}",
                    r.repository,
                    segment(&r.head_branch)
                ),
                None,
            )?;
            require(
                branch["commit"]["sha"] == r.candidate,
                "源分支已变化，不能用旧版本创建 PR",
            )
            .map_err(|error| error.technical_not_applied())?;
            let body = if let Some(creation) = creation {
                json!(creation)
            } else {
                let source = r
                    .projection
                    .as_ref()
                    .map_or(r.candidate.as_str(), |p| p.source_revision.as_str());
                let publication = r.projection.as_ref().map_or_else(
                || "The Task source branch is used directly; its exact PR test-merge commit is retained in the owning DevDesk workflow.".to_owned(),
                |p| format!("A code-only Feature delivery is used. Workflow validation changes are excluded at: {}. The original Feature and Task branches retain their materials.", p.validation_paths.join(", ")));
                json!({"title":format!("DevDesk delivery {}",r.merge),
                "head":r.head_branch,"base":r.base_branch,"body":format!("{}\n\nSource revision: `{source}`\nDelivery revision: `{}`\nValidated target: `{}`\n\n{publication}\n\nReviews, exact test receipts and the final Human decision remain bound to these versions. Existing repository merge requirements apply.",r.marker(),r.candidate,r.expected_target)})
            };
            self.delivery_api("POST", &format!("repos/{}/pulls", r.repository), Some(body))?
        };
        self.receipt(r, &row).map(Some)
    }
    pub(crate) fn delivery_report_status(
        &self,
        r: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<Value>> {
        self.delivery_request(&r.request)?;
        self.validate_merge_candidate(r)?;
        let pr = self.delivery_pr(&r.request)?;
        self.receipt(&r.request, &pr)?;
        if !pr["merged"].as_bool().unwrap_or(false)
            && self.delivery_target_head(&r.request)? != r.request.expected_target
        {
            return Err(err("stale_target", "目标已变化，旧回归和批准失效").technical_not_applied());
        }
        let description = r.merge_revision.as_ref().map_or_else(
            || format!("DevDesk tested tree {}", r.tree),
            |revision| format!("DevDesk tested merge {revision} tree {}", r.tree),
        );
        if inspect {
            let rows = self.delivery_api(
                "GET",
                &format!(
                    "repos/{}/commits/{}/statuses?per_page=100",
                    r.request.repository, r.request.candidate
                ),
                None,
            )?;
            let rows = rows
                .as_array()
                .ok_or_else(|| err("adapter_contract", "状态列表无效"))?;
            let found = rows.iter().find(|v| v["context"] == CHECK);
            return Ok(found
                .filter(|v| v["state"] == "success" && v["description"] == description)
                .cloned());
        }
        self.delivery_api(
            "POST",
            &format!(
                "repos/{}/statuses/{}",
                r.request.repository, r.request.candidate
            ),
            Some(json!({"state":"success","context":CHECK,"description":description})),
        )
        .map(Some)
    }
    pub(crate) fn delivery_ready(&self, r: &PullRequestMergeRequest) -> Result<bool> {
        self.delivery_request(&r.request)?;
        self.validate_merge_candidate(r)?;
        let row = self.delivery_pr(&r.request)?;
        let receipt = self.receipt(&r.request, &row)?;
        if receipt.merged {
            return Ok(true);
        }
        if self.delivery_target_head(&r.request)? != r.request.expected_target {
            return Err(
                err("stale_target", "目标已推进，必须重新合并回归及审核").technical_not_applied()
            );
        }
        Ok(row["mergeable"] == true
            && ["clean", "unstable"].contains(&row["mergeable_state"].as_str().unwrap_or("")))
    }
    pub(crate) fn delivery_merge(
        &self,
        r: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        self.delivery_request(&r.request)?;
        self.validate_merge_candidate(r)?;
        let mut row = self.delivery_pr(&r.request)?;
        let mut receipt = self.receipt(&r.request, &row)?;
        if !receipt.merged {
            let target = self.delivery_target_head(&r.request)?;
            if target == *r.merge_revision.as_ref().unwrap_or(&r.request.candidate) {
                // The exact CAS may already be committed while GitHub is still
                // updating the PR's indirect-merge observation. Never write again.
                return Ok(None);
            }
            if target != r.request.expected_target {
                return Err(err(
                    "stale_target",
                    format!(
                        "PR {} 未合入，候选 {} 的目标已从 {} 变为 {}",
                        receipt.number, receipt.head, r.request.expected_target, target
                    ),
                )
                .technical_not_applied());
            }
            if inspect {
                return Ok(None);
            }
            require(row["mergeable"] == true && ["clean","unstable"].contains(&row["mergeable_state"].as_str().unwrap_or("")),"GitHub 现有合并规则尚未满足；等待 required checks、评审或其他仓库门禁")
                .map_err(|error|error.technical_not_applied().with_context("GitHub PR merge gates",json!({"repository":r.request.repository,"number":receipt.number,"mergeable":row["mergeable"],"mergeable_state":row["mergeable_state"]})))?;
            let branch = self.delivery_target(&r.request.repository, &r.request.base_branch)?;
            if branch["protected"] == false {
                self.advance_unprotected_target(&r.request,r.merge_revision.as_deref().unwrap_or(&r.request.candidate)).map_err(|error| {
                    if error.code == "remote_gate_blocked"
                        && let Ok(target) = self.delivery_target_head(&r.request)
                        && target != r.request.expected_target && target != *r.merge_revision.as_ref().unwrap_or(&r.request.candidate)
                    {
                        return err("stale_target", "目标分支在原子推进前已变化，需要刷新合并回归及审核")
                            .technical_not_applied().with_context("GitHub target CAS rejection",json!({"original_error":error,"expected_target":r.request.expected_target,"actual_target":target,"candidate":r.request.candidate}));
                    }
                    error
                })?;
                row = self.delivery_pr(&r.request)?;
                receipt = self.receipt(&r.request, &row)?;
                if !receipt.merged {
                    return Ok(None);
                }
            } else {
                require(branch["protected"] == true, "不能确认目标分支的保护状态")
                    .map_err(|error| error.technical_not_applied())?;
                let reply = self
                    .delivery_api(
                        "PUT",
                        &format!(
                            "repos/{}/pulls/{}/merge",
                            r.request.repository, receipt.number
                        ),
                        Some(json!({"sha":r.request.candidate,"merge_method":"merge"})),
                    )
                    .map_err(|error| {
                        if error.code == "remote_gate_blocked"
                            && let Ok(current) = self.delivery_pr(&r.request)
                            && let Ok(observed) = self.receipt(&r.request, &current)
                            && let Ok(target) = self.delivery_target_head(&r.request)
                            && !observed.merged
                            && target != r.request.expected_target
                        {
                            return err("stale_target", "服务端拒绝了目标竞争后的陈旧合入")
                                .technical_not_applied();
                        }
                        error
                    })?;
                require(reply["merged"] == true, "GitHub 没有确认合入")?;
                row = self.delivery_pr(&r.request)?;
                receipt = self.receipt(&r.request, &row)?;
            }
        }
        require(receipt.merged, "PR 合入观察未确认")?;
        let revision = receipt
            .merge_commit
            .as_ref()
            .ok_or_else(|| err("needs_reconciliation", "合入缺少实际提交"))?;
        let commit = self.delivery_api(
            "GET",
            &format!("repos/{}/git/commits/{revision}", r.request.repository),
            None,
        )?;
        let parents = commit["parents"]
            .as_array()
            .ok_or_else(|| err("adapter_contract", "合入父提交缺失"))?;
        require(
            commit["tree"]["sha"] == r.tree
                && ((parents.len() == 2
                    && parents[0]["sha"] == r.request.expected_target
                    && parents[1]["sha"] == r.request.candidate)
                    || (revision == &r.request.candidate
                        && parents.len() == 1
                        && parents[0]["sha"] == r.request.expected_target)),
            "远端合入的父提交/tree 与已验候选不一致，保留原始事实",
        )?;
        Ok(Some(receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn request() -> PullRequestRequest {
        let merge = uuid::Uuid::new_v4().to_string();
        PullRequestRequest {
            repository: "owner/repo".into(),
            project: uuid::Uuid::new_v4().to_string(),
            head_branch: format!("devdesk/candidates/{merge}"),
            merge,
            base_branch: "main".into(),
            candidate: "a".repeat(40),
            expected_target: "b".repeat(40),
            number: Some(7),
            projection: None,
        }
    }
    fn pr(request: &PullRequestRequest, merged: bool, state: &str) -> Value {
        json!({"number":7,"html_url":"https://github.com/owner/repo/pull/7","body":request.marker(),"head":{"repo":{"full_name":request.repository},"ref":request.head_branch,"sha":request.candidate},"base":{"repo":{"full_name":request.repository},"ref":request.base_branch,"sha":request.expected_target},"merged":merged,"merge_commit_sha":if merged {Some(request.candidate.clone())} else {None},"mergeable":true,"mergeable_state":state})
    }
    fn candidate(request: &PullRequestRequest) -> Value {
        json!({"parents":[{"sha":request.expected_target}],"tree":{"sha":"tested-tree"}})
    }
    #[test]
    fn pr_creation_and_lost_receipt_recovery_never_require_or_write_branch_protection() {
        for protected in [false, true] {
            let mut request = request();
            request.number = None;
            request.head_branch = format!("devdesk/features/{}", uuid::Uuid::new_v4());
            let exists = Cell::new(false);
            let writes = RefCell::new(Vec::new());
            let creation = PullRequestCreation {
                title: "Packaged delivery".into(),
                body: format!("{}\n\nFrozen candidate", request.marker()),
                head: request.head_branch.clone(),
                base: request.base_branch.clone(),
            };
            let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint)
            {
                ("GET", "repos/owner/repo/branches/main") => {
                    Ok(json!({"protected":protected,"commit":{"sha":request.expected_target}}))
                }
                ("GET", endpoint) if endpoint.contains("/branches/devdesk%2Ffeatures%2F") => {
                    Ok(json!({"commit":{"sha":request.candidate}}))
                }
                ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
                ("GET", endpoint) if endpoint.contains("pulls?state=all") => Ok(if exists.get() {
                    json!([pr(&request, false, "blocked")])
                } else {
                    json!([])
                }),
                ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "blocked")),
                ("POST", "repos/owner/repo/pulls") => {
                    assert_eq!(input.unwrap(), json!(creation));
                    writes.borrow_mut().push(endpoint.to_owned());
                    exists.set(true);
                    Ok(pr(&request, false, "blocked"))
                }
                _ => panic!("unexpected request {method} {endpoint}"),
            };
            let client = DeliveryClient { api: &api };
            assert_eq!(
                client.delivery_target("owner/repo", "main").unwrap()["protected"],
                protected
            );
            assert!(
                client
                    .delivery_ensure(&request, true, Some(&creation))
                    .unwrap()
                    .is_none()
            );
            let created = client
                .delivery_ensure(&request, false, Some(&creation))
                .unwrap()
                .unwrap();
            let recovered = client
                .delivery_ensure(&request, true, Some(&creation))
                .unwrap()
                .unwrap();
            let repeated = client
                .delivery_ensure(&request, false, Some(&creation))
                .unwrap()
                .unwrap();
            assert_eq!(created.number, recovered.number);
            assert_eq!(created.number, repeated.number);
            assert_eq!(writes.borrow().as_slice(), &["repos/owner/repo/pulls"]);
        }
    }
    #[test]
    fn reused_source_branch_recovers_only_its_own_delivery_pr() {
        let mut request = request();
        request.number = None;
        request.head_branch = format!("devdesk/features/{}", uuid::Uuid::new_v4());
        let mut unrelated = pr(&request, true, "clean");
        unrelated["number"] = json!(6);
        unrelated["body"] = json!("A historical delivery with a different marker");
        unrelated["state"] = json!("closed");
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", endpoint) if endpoint.contains("pulls?state=all") => {
                Ok(json!([unrelated, pr(&request, false, "clean")]))
            }
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "clean")),
            _ => panic!("recovery must not write: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        assert_eq!(
            client
                .delivery_ensure(&request, true, None)
                .unwrap()
                .unwrap()
                .number,
            7
        );
    }
    #[test]
    fn an_existing_required_gate_blocks_merge_without_changing_rules_or_using_a_bypass() {
        let request = request();
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "blocked")),
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":true,"commit":{"sha":request.expected_target}}))
            }
            _ => panic!("gate must prevent {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        assert!(!client.delivery_ready(&merge).unwrap());
        let error = client.delivery_merge(&merge, false).unwrap_err();
        assert_eq!(
            error.execution_failure.unwrap().effect,
            ExecutionFailureEffect::NotApplied
        );
    }
    #[test]
    fn unprotected_merge_is_an_atomic_fast_forward_and_recovers_without_a_second_write() {
        let request = request();
        let merged = Cell::new(false);
        let writes = Cell::new(0);
        let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, merged.get(), "clean")),
            ("GET", "repos/owner/repo/branches/main") => Ok(
                json!({"protected":false,"commit":{"sha":if merged.get(){&request.candidate}else{&request.expected_target}}}),
            ),
            ("GET", "repos/owner/repo") => Ok(json!({"node_id":"repository-id"})),
            ("POST", "graphql") => {
                let input = input.unwrap();
                let update = &input["variables"]["input"]["refUpdates"][0];
                assert_eq!(update["beforeOid"], request.expected_target);
                assert_eq!(update["afterOid"], request.candidate);
                assert_eq!(update["force"], false);
                assert_eq!(update["name"], "refs/heads/main");
                writes.set(writes.get() + 1);
                merged.set(true);
                Ok(json!({"data":{"updateRefs":{"clientMutationId":request.merge}}}))
            }
            _ => panic!("unexpected request {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        assert!(
            client
                .delivery_merge(&merge, false)
                .unwrap()
                .unwrap()
                .merged
        );
        assert!(client.delivery_merge(&merge, true).unwrap().unwrap().merged);
        assert_eq!(writes.get(), 1);
    }
    #[test]
    fn protected_merge_uses_existing_pr_rules_and_preserves_the_remote_rejection() {
        let request = request();
        let writes = Cell::new(0);
        let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "clean")),
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":true,"commit":{"sha":request.expected_target}}))
            }
            ("PUT", "repos/owner/repo/pulls/7/merge") => {
                let input = input.unwrap();
                assert_eq!(
                    input,
                    json!({"sha":request.candidate,"merge_method":"merge"})
                );
                writes.set(writes.get() + 1);
                Err(err(
                    "remote_gate_blocked",
                    "A repository rule requires another review",
                )
                .technical_not_applied())
            }
            _ => panic!("must not bypass a protected merge using {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        let error = client.delivery_merge(&merge, false).unwrap_err();
        assert_eq!(error.code, "remote_gate_blocked");
        assert!(error.message.contains("another review"));
        assert_eq!(writes.get(), 1);
    }
    #[test]
    fn a_target_race_rejects_the_original_cas_without_a_force_push() {
        let request = request();
        let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "clean")),
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":false,"commit":{"sha":request.expected_target}}))
            }
            ("GET", "repos/owner/repo") => Ok(json!({"node_id":"repository-id"})),
            ("POST", "graphql") => {
                let input = input.unwrap();
                assert_eq!(
                    input["variables"]["input"]["refUpdates"][0]["beforeOid"],
                    request.expected_target
                );
                Ok(
                    json!({"data":{"updateRefs":null},"errors":[{"type":"UNPROCESSABLE","message":"beforeOid no longer matches"}]}),
                )
            }
            _ => panic!("unexpected request {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        let error = client.delivery_merge(&merge, false).unwrap_err();
        assert_eq!(error.code, "remote_gate_blocked");
        assert_eq!(
            error.execution_failure.as_ref().unwrap().effect,
            ExecutionFailureEffect::NotApplied
        );
        assert!(!error.context.is_empty());
    }
    #[test]
    fn an_already_applied_fast_forward_waits_for_the_pr_receipt_without_writing_again() {
        let request = request();
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, false, "clean")),
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":false,"commit":{"sha":request.candidate}}))
            }
            _ => panic!("lost receipt must not write: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        for inspect in [true, false] {
            assert!(client.delivery_merge(&merge, inspect).unwrap().is_none());
        }
    }
    #[test]
    fn protected_merge_can_succeed_under_existing_rules_without_the_custom_protection_template() {
        let request = request();
        let merged = Cell::new(false);
        let writes = Cell::new(0);
        let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint) {
            ("GET", endpoint)
                if endpoint.ends_with(&format!("/git/commits/{}", request.candidate)) =>
            {
                Ok(candidate(&request))
            }
            ("GET", endpoint)
                if endpoint.ends_with(&format!("/git/commits/{}", "c".repeat(40))) =>
            {
                Ok(
                    json!({"tree":{"sha":"tested-tree"},"parents":[{"sha":request.expected_target},{"sha":request.candidate}]}),
                )
            }
            ("GET", "repos/owner/repo/pulls/7") => {
                let mut row = pr(&request, merged.get(), "clean");
                if merged.get() {
                    row["merge_commit_sha"] = json!("c".repeat(40));
                }
                Ok(row)
            }
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":true,"commit":{"sha":request.expected_target}}))
            }
            ("PUT", "repos/owner/repo/pulls/7/merge") => {
                assert_eq!(
                    input.unwrap(),
                    json!({"sha":request.candidate,"merge_method":"merge"})
                );
                writes.set(writes.get() + 1);
                merged.set(true);
                Ok(json!({"merged":true}))
            }
            _ => panic!("must preserve repository policy: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: "tested-tree".into(),
            merge_revision: None,
        };
        assert!(
            client
                .delivery_merge(&merge, false)
                .unwrap()
                .unwrap()
                .merged
        );
        assert_eq!(writes.get(), 1);
    }
    #[test]
    fn graphql_internal_failures_keep_the_unknown_effect_and_complete_error() {
        let request = request();
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", "repos/owner/repo") => Ok(json!({"node_id":"repository-id"})),
            ("POST", "graphql") => Ok(
                json!({"data":{"updateRefs":null},"errors":[{"type":"INTERNAL","message":"result unknown"}]}),
            ),
            _ => panic!("unexpected request {method} {endpoint}"),
        };
        let error = DeliveryClient { api: &api }
            .advance_unprotected_target(&request, &request.candidate)
            .unwrap_err();
        assert_eq!(
            error.execution_failure.unwrap().effect,
            ExecutionFailureEffect::Unknown
        );
        assert!(
            error
                .context
                .iter()
                .any(|context| context.details.to_string().contains("result unknown"))
        );
    }
    #[test]
    fn source_branch_candidate_waits_for_github_and_freezes_both_parents_and_tree() {
        let mut request = request();
        request.head_branch = format!("devdesk/features/{}", uuid::Uuid::new_v4());
        let ready = Cell::new(false);
        let candidate_sha = "c".repeat(40);
        let tree = "d".repeat(40);
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", endpoint)
                if endpoint.ends_with(&format!("/git/commits/{}", request.candidate)) =>
            {
                Ok(candidate(&request))
            }
            ("GET", endpoint) if endpoint.ends_with(&format!("/git/commits/{candidate_sha}")) => {
                Ok(
                    json!({"parents":[{"sha":request.expected_target},{"sha":request.candidate}],"tree":{"sha":tree}}),
                )
            }
            ("GET", "repos/owner/repo/pulls/7") => {
                let mut row = pr(&request, false, "clean");
                row["mergeable"] = if ready.get() {
                    json!(true)
                } else {
                    Value::Null
                };
                row["merge_commit_sha"] = json!(candidate_sha);
                Ok(row)
            }
            ("GET", "repos/owner/repo/branches/main") => {
                Ok(json!({"protected":false,"commit":{"sha":request.expected_target}}))
            }
            _ => panic!("candidate observation cannot publish refs: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        assert!(client.delivery_candidate(&request).unwrap().is_none());
        ready.set(true);
        let frozen = client.delivery_candidate(&request).unwrap().unwrap();
        assert_eq!(frozen.revision, candidate_sha);
        assert_eq!(frozen.tree, tree);
        assert_eq!(frozen.source_revision, request.candidate);
        assert_eq!(frozen.target_revision, request.expected_target);
    }
    #[test]
    fn direct_source_merge_advances_only_the_frozen_test_merge_and_recovers_once() {
        let mut request = request();
        request.head_branch = format!("devdesk/tasks/{}", uuid::Uuid::new_v4());
        let candidate_sha = "c".repeat(40);
        let tree = "d".repeat(40);
        let merged = Cell::new(false);
        let writes = Cell::new(0);
        let api = |method: &str, endpoint: &str, input: Option<Value>| match (method, endpoint) {
            ("GET", endpoint)
                if endpoint.ends_with(&format!("/git/commits/{}", request.candidate)) =>
            {
                Ok(candidate(&request))
            }
            ("GET", endpoint) if endpoint.ends_with(&format!("/git/commits/{candidate_sha}")) => {
                Ok(
                    json!({"parents":[{"sha":request.expected_target},{"sha":request.candidate}],"tree":{"sha":tree}}),
                )
            }
            ("GET", "repos/owner/repo/pulls/7") => {
                let mut row = pr(&request, merged.get(), "clean");
                row["merge_commit_sha"] = json!(candidate_sha);
                Ok(row)
            }
            ("GET", "repos/owner/repo/branches/main") => Ok(
                json!({"protected":false,"commit":{"sha":if merged.get(){&candidate_sha}else{&request.expected_target}}}),
            ),
            ("GET", "repos/owner/repo") => Ok(json!({"node_id":"repository-id"})),
            ("POST", "graphql") => {
                let update = &input.as_ref().unwrap()["variables"]["input"]["refUpdates"][0];
                assert_eq!(update["beforeOid"], request.expected_target);
                assert_eq!(update["afterOid"], candidate_sha);
                assert_eq!(update["force"], false);
                assert_eq!(update["name"], "refs/heads/main");
                writes.set(writes.get() + 1);
                merged.set(true);
                Ok(json!({"data":{"updateRefs":{"clientMutationId":request.merge}}}))
            }
            _ => panic!(
                "direct source delivery cannot create candidates branches: {method} {endpoint}"
            ),
        };
        let client = DeliveryClient { api: &api };
        let merge = PullRequestMergeRequest {
            request: request.clone(),
            tree: tree.clone(),
            merge_revision: Some(candidate_sha.clone()),
        };
        let receipt = client.delivery_merge(&merge, false).unwrap().unwrap();
        assert_eq!(receipt.head, request.candidate);
        assert_eq!(
            receipt.merge_commit.as_deref(),
            Some(candidate_sha.as_str())
        );
        assert!(client.delivery_merge(&merge, true).unwrap().unwrap().merged);
        assert_eq!(writes.get(), 1);
    }
    #[test]
    fn legacy_candidate_pr_is_readable_but_cannot_be_newly_created() {
        let mut request = request();
        request.number = None;
        let exists = Cell::new(true);
        let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", endpoint) if endpoint.contains("pulls?state=all") => Ok(if exists.get() {
                json!([pr(&request, true, "clean")])
            } else {
                json!([])
            }),
            ("GET", "repos/owner/repo/pulls/7") => Ok(pr(&request, true, "clean")),
            _ => panic!("legacy recovery is read-only: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        assert!(
            client
                .delivery_ensure(&request, false, None)
                .unwrap()
                .unwrap()
                .merged
        );
        exists.set(false);
        let error = client.delivery_ensure(&request, false, None).unwrap_err();
        assert_eq!(
            error.execution_failure.unwrap().effect,
            ExecutionFailureEffect::NotApplied
        );
    }
    #[test]
    fn reviewed_feature_projection_can_create_a_mainline_pr_with_its_original_source() {
        let mut request = request();
        request.number = None;
        request.projection = Some(DeliveryProjection {
            source_revision: "c".repeat(40),
            target_revision: request.expected_target.clone(),
            validation_paths: vec!["tests/.devdesk".into(), "evidence/.devdesk".into()],
        });
        let writes = Cell::new(0);
        let api = |method: &str, endpoint: &str, body: Option<Value>| match (method, endpoint) {
            ("GET", endpoint) if endpoint.contains("/git/commits/") => Ok(candidate(&request)),
            ("GET", endpoint) if endpoint.contains("pulls?state=all") => Ok(json!([])),
            ("GET", endpoint) if endpoint.contains("/branches/devdesk%2Fcandidates%2F") => {
                Ok(json!({"commit":{"sha":request.candidate}}))
            }
            ("POST", "repos/owner/repo/pulls") => {
                let body = body.unwrap();
                assert_eq!(body["head"], request.head_branch);
                assert_eq!(body["base"], "main");
                let text = body["body"].as_str().unwrap();
                assert!(text.contains(&"c".repeat(40)));
                assert!(text.contains(&request.candidate));
                assert!(text.contains("code-only Feature delivery"));
                assert!(text.contains("tests/.devdesk"));
                writes.set(writes.get() + 1);
                Ok(pr(&request, false, "clean"))
            }
            _ => panic!("unexpected projection action: {method} {endpoint}"),
        };
        let client = DeliveryClient { api: &api };
        assert_eq!(
            client
                .delivery_ensure(&request, false, None)
                .unwrap()
                .unwrap()
                .head,
            request.candidate
        );
        assert_eq!(writes.get(), 1);
        let mut invalid = request.clone();
        invalid.projection.as_mut().unwrap().target_revision = "d".repeat(40);
        assert!(client.delivery_ensure(&invalid, false, None).is_err());
        assert_eq!(writes.get(), 1);
    }
    #[test]
    fn changed_source_or_tested_tree_is_rejected_before_direct_merge() {
        let mut request = request();
        request.head_branch = format!("devdesk/features/{}", uuid::Uuid::new_v4());
        let revision = "c".repeat(40);
        for changed_source in [false, true] {
            let api = |method: &str, endpoint: &str, _: Option<Value>| match (method, endpoint) {
                ("GET", endpoint)
                    if endpoint.ends_with(&format!("/git/commits/{}", request.candidate)) =>
                {
                    Ok(candidate(&request))
                }
                ("GET", endpoint) if endpoint.ends_with(&format!("/git/commits/{revision}")) => Ok(
                    json!({"parents":[{"sha":request.expected_target},{"sha":if changed_source{"e".repeat(40)}else{request.candidate.clone()}}],"tree":{"sha":if changed_source{"d".repeat(40)}else{"f".repeat(40)}}}),
                ),
                _ => panic!("changed evidence cannot authorize {method} {endpoint}"),
            };
            let merge = PullRequestMergeRequest {
                request: request.clone(),
                tree: "d".repeat(40),
                merge_revision: Some(revision.clone()),
            };
            let error = DeliveryClient { api: &api }
                .delivery_merge(&merge, false)
                .unwrap_err();
            assert_eq!(
                error.execution_failure.unwrap().effect,
                ExecutionFailureEffect::NotApplied
            );
        }
    }
    #[test]
    fn gate_rejection_accepts_numeric_and_string_status_without_hiding_unknowns() {
        for code in [403, 405, 409, 422] {
            assert!(rejected_by_remote_gate(&json!({"status":code})));
            assert!(rejected_by_remote_gate(&json!({"status":code.to_string()})));
        }
        for value in [
            json!({}),
            json!({"status":500}),
            json!({"status":"timeout"}),
            json!({"status":null}),
        ] {
            assert!(!rejected_by_remote_gate(&value));
        }
    }
}
