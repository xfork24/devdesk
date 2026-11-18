//! Protected PR merges of single-parent delivery snapshots. The server's strict
//! required check closes the base race; a client-side GET is never the fence.
use super::*;
use crate::github::GitHubPullRequests;
use std::time::Duration;
const CHECK: &str = "devdesk/merge-regression";
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
        let mut command = std::process::Command::new("gh");
        command.args(["api", "--method", method, endpoint]);
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
                if value.as_ref().ok().is_some_and(|v| {
                    ["405", "409", "422", "403"].contains(&v["status"].as_str().unwrap_or(""))
                }) {
                    err("remote_gate_blocked", message).technical_not_applied()
                } else {
                    err("needs_reconciliation", message).technical_unknown()
                },
            );
        }
        value.map_err(storage)
    }
    fn protected_delivery_branch(&self, repository: &str, branch: &str) -> Result<Value> {
        // diagnose validates repository shape and checks complete inherited rules.
        let policy = self.diagnose(repository, branch)?;
        require(
            policy
                .blocked_reasons
                .iter()
                .all(|r| r == "仓库存在传统分支保护；本轮本地 CAS 策略不支持，不会修改或绕过规则")
                && policy.active_branch_rulesets.is_empty(),
            "当前精简合入合同不支持规则集，不能绕过",
        )?;
        let rule = self.delivery_api(
            "GET",
            &format!("repos/{repository}/branches/{}/protection", segment(branch)),
            None,
        )?;
        Self::validate_delivery_protection(&rule)?;
        Ok(rule)
    }
    pub(crate) fn validate_delivery_protection(rule: &Value) -> Result<()> {
        require(
            rule["enforce_admins"]["enabled"] == true
                && rule["required_status_checks"]["strict"] == true
                && rule["required_status_checks"]["contexts"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == CHECK))
                && rule["allow_force_pushes"]["enabled"] == false
                && rule["required_linear_history"]["enabled"] == false,
            "需要包含管理员的严格 merge-regression 检查和禁止强推；不支持线性历史替换合入",
        )
    }
    pub fn diagnose_connection(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<GitHubRepositoryPolicy> {
        let mut policy = self.diagnose(repository, branch)?;
        if !policy.compatible
            && ["ADMIN", "MAINTAIN", "WRITE"].contains(&policy.viewer_permission.as_str())
            && self.protected_delivery_branch(repository, branch).is_ok()
        {
            policy.compatible = true;
            policy.blocked_reasons.clear();
        }
        Ok(policy)
    }
    fn delivery_request(&self, r: &PullRequestRequest) -> Result<()> {
        require(
            uuid::Uuid::parse_str(&r.project).is_ok()
                && uuid::Uuid::parse_str(&r.merge).is_ok()
                && r.head_branch == format!("devdesk/candidates/{}", r.merge)
                && [r.candidate.as_str(), r.expected_target.as_str()]
                    .iter()
                    .all(|v| v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit())),
            "精简 PR 归属或版本无效",
        )?;
        self.protected_delivery_branch(&r.repository, &r.base_branch)?;
        let commit = self.delivery_api(
            "GET",
            &format!("repos/{}/git/commits/{}", r.repository, r.candidate),
            None,
        )?;
        require(
            commit["parents"]
                .as_array()
                .is_some_and(|p| p.len() == 1 && p[0]["sha"] == r.expected_target),
            "远端原候选必须以已验目标为唯一直接父提交",
        )
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
    pub(crate) fn protected_target(&self, c: &GitContext, create: bool) -> Result<Value> {
        if let Ok(rule) = self.protected_delivery_branch(&c.workspace.repository, &c.target.branch)
        {
            return Ok(rule);
        }
        require(
            create && c.target.owner != c.project && c.target.branch != c.workspace.mainline,
            "项目主分支必须预先配置支持的严格保护，平台不能降低或绕过规则",
        )?;
        let policy = self.diagnose(&c.workspace.repository, &c.target.branch)?;
        require(
            policy.active_branch_rulesets.is_empty(),
            "目标存在未支持的规则集",
        )?;
        // Only initialize a completely unprotected, managed parent branch. Never
        // overwrite an existing protection to make our request pass.
        let branch = self.delivery_api(
            "GET",
            &format!(
                "repos/{}/branches/{}",
                c.workspace.repository,
                segment(&c.target.branch)
            ),
            None,
        )?;
        require(
            branch["protected"] == false,
            "保留既有分支保护，需配置支持的交付检查",
        )?;
        self.delivery_api("PUT",&format!("repos/{}/branches/{}/protection",c.workspace.repository,segment(&c.target.branch)),
            Some(json!({"required_status_checks":{"strict":true,"contexts":[CHECK]},"enforce_admins":true,
            "required_pull_request_reviews":null,"restrictions":null,"allow_force_pushes":false,"allow_deletions":false})))?;
        self.protected_delivery_branch(&c.workspace.repository, &c.target.branch)
    }
    pub(crate) fn delivery_ensure(
        &self,
        r: &PullRequestRequest,
        inspect: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        self.delivery_request(r)?;
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
        require(rows.len() <= 1, "交付 PR 不唯一，不推测归属")?;
        let row = if let Some(row) = rows.first() {
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
            self.delivery_api("POST",&format!("repos/{}/pulls",r.repository),Some(json!({"title":format!("DevDesk delivery {}",r.merge),
                "head":r.head_branch,"base":r.base_branch,"body":format!("{}\n\nDelivery snapshot: `{}`\nValidated target: `{}`\n\nOriginal source, reviews, test receipts and final Human decision are retained in the owning DevDesk workflow. This PR cannot advance the target until exact merge regression and protected server checks pass.",r.marker(),r.candidate,r.expected_target)})))?
        };
        self.receipt(r, &row).map(Some)
    }
    pub(crate) fn delivery_report_status(
        &self,
        r: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<Value>> {
        self.delivery_request(&r.request)?;
        let pr = self.delivery_pr(&r.request)?;
        self.receipt(&r.request, &pr)?;
        if !pr["merged"].as_bool().unwrap_or(false)
            && pr["base"]["sha"] != r.request.expected_target
        {
            return Err(err("stale_target", "目标已变化，旧回归和批准失效").technical_not_applied());
        }
        let description = format!("DevDesk tested tree {}", r.tree);
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
        let row = self.delivery_pr(&r.request)?;
        let receipt = self.receipt(&r.request, &row)?;
        if receipt.merged {
            return Ok(true);
        }
        if receipt.base != r.request.expected_target {
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
        let mut row = self.delivery_pr(&r.request)?;
        let mut receipt = self.receipt(&r.request, &row)?;
        if !receipt.merged {
            if inspect {
                return Ok(None);
            }
            if receipt.base != r.request.expected_target {
                return Err(err("stale_target", "目标已变化，拒绝陈旧合入").technical_not_applied());
            }
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
                        && !observed.merged
                        && observed.base != r.request.expected_target
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
