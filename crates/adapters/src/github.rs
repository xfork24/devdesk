//! GitHub PR publication and observed merges; existing rules are never changed or bypassed.
use super::*;
use crate::process::{capture_command, capture_command_split};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;
#[path = "github_auth.rs"]
mod auth;
pub use auth::GitHubAuthStatus;
pub type RepositoryPolicy = GitHubRepositoryPolicy;
#[derive(Default)]
pub struct GitHubPullRequests {
    vault: Option<credentials::CredentialVault>,
    credential_update: Mutex<()>,
}
impl GitHubPullRequests {
    pub(crate) fn validate_repository_branch(repository: &str, branch: &str) -> Result<()> {
        let pieces: Vec<_> = repository.split('/').collect();
        require(
            pieces.len() == 2
                && pieces.iter().all(|p| {
                    !p.is_empty()
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                }),
            "GitHub 仓库必须是 owner/repo",
        )?;
        require(
            !branch.is_empty() && branch.len() <= 200 && !branch.contains(['\n', '\r']),
            "分支无效",
        )?;
        Ok(())
    }
    pub(crate) fn exact_branch_head(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<Option<String>> {
        Self::validate_repository_branch(repository, branch)?;
        let (owner, name) = repository.split_once('/').unwrap();
        let query = "query($owner:String!,$name:String!,$ref:String!){repository(owner:$owner,name:$name){nameWithOwner ref(qualifiedName:$ref){name prefix target{oid}}}}";
        let mut command = self.api_command()?;
        command.args([
            "graphql",
            "-f",
            &format!("query={query}"),
            "-f",
            &format!("owner={owner}"),
            "-f",
            &format!("name={name}"),
            "-f",
            &format!("ref=refs/heads/{branch}"),
        ]);
        let output =
            capture_command_split(command, Duration::from_secs(10), None).map_err(|error| {
                if error.code == "needs_reconciliation" {
                    err("github_ref_timeout", "GitHub API 查询分支超过 10 秒")
                } else {
                    error
                }
            })?;
        if output.code != 0 || output.truncated {
            return Err(auth::api_error(
                &output,
                "GitHub API 未能确认分支，请检查仓库访问和网络",
            ));
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| err("github_ref_response", "GitHub API 返回了无效的分支数据"))?;
        Self::parse_exact_branch_head(repository, branch, &value)
    }

    fn parse_exact_branch_head(
        repository: &str,
        branch: &str,
        value: &Value,
    ) -> Result<Option<String>> {
        let row = &value["data"]["repository"];
        if value.get("errors").is_some()
            || !row["nameWithOwner"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(repository))
        {
            return Err(err(
                "github_ref_response",
                "GitHub API 未确认原仓库身份和查询结果",
            ));
        }
        match row.get("ref") {
            Some(Value::Null) => Ok(None),
            Some(reference)
                if reference["name"] == branch && reference["prefix"] == "refs/heads/" =>
            {
                let sha = reference["target"]["oid"]
                    .as_str()
                    .filter(|sha| {
                        sha.len() == 40
                            && sha
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                    .ok_or_else(|| err("github_ref_response", "GitHub API 缺少有效的原分支提交"))?;
                Ok(Some(sha.to_owned()))
            }
            _ => Err(err(
                "github_ref_response",
                "GitHub API 未返回原分支的确切查询结果",
            )),
        }
    }
    pub fn diagnose(&self, repository: &str, branch: &str) -> Result<RepositoryPolicy> {
        Self::validate_repository_branch(repository, branch)?;
        let pieces: Vec<_> = repository.split('/').collect();
        let query = r#"query($owner:String!,$name:String!,$branch:String!){repository(owner:$owner,name:$name){id nameWithOwner viewerPermission ref(qualifiedName:$branch){target{oid}} branchProtectionRules(first:100){totalCount pageInfo{hasNextPage}} rulesets(first:100,includeParents:true){nodes{id name enforcement target} pageInfo{hasNextPage}}}}"#;
        let args = [
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
            "-f".into(),
            format!("owner={}", pieces[0]),
            "-f".into(),
            format!("name={}", pieces[1]),
            "-f".into(),
            format!("branch=refs/heads/{branch}"),
        ];
        let mut command = self.api_command()?;
        command.args(args);
        let output = capture_command_split(command, Duration::from_secs(20), None)?;
        if output.code != 0 || output.truncated {
            return Err(auth::api_error(
                &output,
                "无法读取 GitHub 仓库和分支规则，请检查仓库权限及网络连接",
            ));
        }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(storage)?;
        require(value.get("errors").is_none(), "GitHub 规则查询不完整")?;
        Self::policy(repository, branch, &value["data"]["repository"])
    }
    fn policy(repository: &str, branch: &str, row: &Value) -> Result<RepositoryPolicy> {
        let id = row["id"]
            .as_str()
            .ok_or_else(|| err("github_unavailable", "仓库身份不可核对"))?;
        require(
            row["nameWithOwner"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(repository)),
            "GitHub 仓库身份漂移",
        )?;
        let permission = row["viewerPermission"]
            .as_str()
            .unwrap_or("UNKNOWN")
            .to_string();
        let classic = row["branchProtectionRules"]["totalCount"]
            .as_u64()
            .ok_or_else(|| err("github_policy_unknown", "不能读取传统保护规则"))?;
        let rules = row["rulesets"]["nodes"]
            .as_array()
            .ok_or_else(|| err("github_policy_unknown", "不能读取仓库/上级规则集"))?;
        let mut reasons = vec![];
        let mut active = vec![];
        if !["ADMIN", "MAINTAIN", "WRITE"].contains(&permission.as_str()) {
            reasons.push("账号没有可核对的仓库写权限".into());
        }
        if classic > 0 {
            reasons
                .push("仓库存在传统分支保护；本轮本地 CAS 策略不支持，不会修改或绕过规则".into());
        }
        if row["branchProtectionRules"]["pageInfo"]["hasNextPage"] != false
            || row["rulesets"]["pageInfo"]["hasNextPage"] != false
        {
            reasons.push("规则分页不完整，不能确认策略兼容".into());
        }
        for rule in rules {
            let enforcement = rule["enforcement"].as_str().unwrap_or("UNKNOWN");
            let target = rule["target"].as_str().unwrap_or("UNKNOWN");
            if target != "TAG" && enforcement != "DISABLED" {
                active.push(rule["name"].as_str().unwrap_or("unknown rule").into());
            }
        }
        if !active.is_empty() {
            reasons.push(
                "存在分支/推送规则集；本轮策略保守阻塞，不使用管理员 bypass 或平台合入替代".into(),
            );
        }
        Ok(RepositoryPolicy {
            observed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            repository: row["nameWithOwner"].as_str().unwrap().into(),
            repository_id: id.into(),
            branch: branch.into(),
            head: row["ref"]["target"]["oid"].as_str().map(str::to_owned),
            viewer_permission: permission,
            compatible: reasons.is_empty(),
            blocked_reasons: reasons,
            active_branch_rulesets: active,
            classic_protection_count: classic,
        })
    }
}

impl GitHubPullRequests {
    fn api(&self, endpoint: &str, input: Option<Value>) -> Result<Value> {
        let mut command = self.api_command()?;
        command.arg(endpoint);
        if input.is_some() {
            command.args(["--method", "POST", "--input", "-"]);
        }
        let output = capture_command(
            command,
            Duration::from_secs(25),
            input.map(|v| serde_json::to_vec(&v).unwrap()),
        )?;
        require(
            output.code == 0 && !output.truncated,
            "GitHub API 结果未确认，请核对原 PR，不会调用平台 merge 或 bypass",
        )?;
        serde_json::from_slice(&output.stdout).map_err(storage)
    }
    fn validate_request(&self, r: &PullRequestRequest) -> Result<()> {
        require(
            uuid::Uuid::parse_str(&r.project).is_ok() && uuid::Uuid::parse_str(&r.merge).is_ok(),
            "PR 归属无效",
        )?;
        require(
            (r.head_branch == format!("devdesk/candidates/{}", r.merge)
                || ["devdesk/tasks/", "devdesk/features/"]
                    .iter()
                    .any(|prefix| {
                        r.head_branch
                            .strip_prefix(prefix)
                            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
                    }))
                && r.candidate.len() == 40
                && r.candidate.bytes().all(|v| v.is_ascii_hexdigit())
                && !r.base_branch.is_empty(),
            "PR 分支或候选无效",
        )?;
        // Opening or reading a PR never advances its base. Merge protection
        // belongs to the merge path, not to publication validation.
        Self::validate_repository_branch(&r.repository, &r.base_branch)
    }
    pub(crate) fn receipt(r: &PullRequestRequest, row: &Value) -> Result<PullRequestReceipt> {
        require(
            row["head"]["repo"]["full_name"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(&r.repository))
                && row["base"]["repo"]["full_name"]
                    .as_str()
                    .is_some_and(|v| v.eq_ignore_ascii_case(&r.repository)),
            "PR 仓库不符",
        )?;
        require(
            row["head"]["ref"] == r.head_branch
                && row["base"]["ref"] == r.base_branch
                && row["head"]["sha"] == r.candidate
                && row["body"]
                    .as_str()
                    .is_some_and(|v| v.contains(&r.marker())),
            "PR 头、目标或归属 marker 不符",
        )?;
        let number = row["number"]
            .as_u64()
            .ok_or_else(|| err("adapter_contract", "PR number 缺失"))?;
        require(r.number.is_none_or(|v| v == number), "PR number 不符")?;
        Ok(PullRequestReceipt {
            repository: r.repository.clone(),
            number,
            url: row["html_url"]
                .as_str()
                .ok_or_else(|| err("adapter_contract", "PR URL 缺失"))?
                .into(),
            head_branch: r.head_branch.clone(),
            base_branch: r.base_branch.clone(),
            head: r.candidate.clone(),
            base: row["base"]["sha"]
                .as_str()
                .ok_or_else(|| err("adapter_contract", "PR base 缺失"))?
                .into(),
            merged: row["merged"].as_bool().unwrap_or(false),
            merge_commit: row["merge_commit_sha"].as_str().map(str::to_string),
            marker: r.marker(),
        })
    }
}
impl ports::PullRequests for GitHubPullRequests {
    fn delivery_target(&self, c: &GitContext) -> Result<Value> {
        self.observe_delivery_target(c)
    }
    fn ensure_delivery(
        &self,
        r: &PullRequestRequest,
        inspect: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        self.delivery_ensure(r, inspect)
    }
    fn delivery_candidate(
        &self,
        request: &PullRequestRequest,
    ) -> Result<Option<PullRequestMergeCandidate>> {
        self.observe_delivery_candidate(request)
    }
    fn delivery_status(&self, r: &PullRequestMergeRequest, inspect: bool) -> Result<Option<Value>> {
        self.delivery_report_status(r, inspect)
    }
    fn merge_delivery(
        &self,
        r: &PullRequestMergeRequest,
        inspect: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        self.delivery_merge(r, inspect)
    }
    fn delivery_merge_ready(&self, r: &PullRequestMergeRequest) -> Result<bool> {
        self.delivery_ready(r)
    }
    fn identity(&self) -> String {
        "github-pr/v1:indirect-merge-only".into()
    }
    fn ensure(&self, r: &PullRequestRequest) -> Result<PullRequestReceipt> {
        self.validate_request(r)?;
        if r.number.is_some() {
            return self.read(r);
        }
        let owner = r.repository.split('/').next().unwrap();
        // GET before every create resolves a successful POST whose response was lost.
        let rows = self.api(
            &format!(
                "repos/{}/pulls?state=all&head={}:{}&base={}&per_page=100",
                r.repository, owner, r.head_branch, r.base_branch
            ),
            None,
        )?;
        let rows = rows
            .as_array()
            .ok_or_else(|| err("github_unavailable", "PR 列表结构不完整"))?;
        require(
            rows.len() < 100 && rows.len() <= 1,
            "PR 列表不唯一或分页不完整",
        )?;
        let row = if let Some(row) = rows.first() {
            let number = row["number"]
                .as_u64()
                .ok_or_else(|| err("github_unavailable", "PR number 缺失"))?;
            self.api(&format!("repos/{}/pulls/{number}", r.repository), None)?
        } else {
            require(
                !r.head_branch.starts_with("devdesk/candidates/"),
                "历史 candidates PR 仅支持核对，请以开发分支重新发布",
            )
            .map_err(|error| error.technical_not_applied())?;
            self.api(&format!("repos/{}/pulls",r.repository),Some(json!({"title":format!("DevDesk delivery {}",r.merge),"head":r.head_branch,"base":r.base_branch,"body":format!("{}\n\nFrozen candidate: `{}`\nExpected target: `{}`\n\nValidated and reviewed by DevDesk before local CAS and exact remote lease. GitHub records indirect merge; no platform merge endpoint is used.",r.marker(),r.candidate,r.expected_target)})))?
        };
        Self::receipt(r, &row)
    }
    fn inspect(&self, r: &PullRequestRequest) -> Result<Option<PullRequestReceipt>> {
        self.validate_request(r)?;
        if r.number.is_some() {
            return self.read(r).map(Some);
        }
        let owner = r.repository.split('/').next().unwrap();
        let rows = self.api(
            &format!(
                "repos/{}/pulls?state=all&head={}:{}&base={}&per_page=100",
                r.repository, owner, r.head_branch, r.base_branch
            ),
            None,
        )?;
        let rows = rows
            .as_array()
            .ok_or_else(|| err("github_unavailable", "PR 列表结构不完整"))?;
        require(rows.len() <= 1, "PR 列表不唯一或分页不完整")?;
        rows.first()
            .map(|row| {
                let number = row["number"]
                    .as_u64()
                    .ok_or_else(|| err("github_unavailable", "PR number 缺失"))?;
                Self::receipt(
                    r,
                    &self.api(&format!("repos/{}/pulls/{number}", r.repository), None)?,
                )
            })
            .transpose()
    }
    fn read(&self, r: &PullRequestRequest) -> Result<PullRequestReceipt> {
        self.validate_request(r)?;
        let number = r
            .number
            .ok_or_else(|| err("blocked", "核对 PR 需要 number"))?;
        Self::receipt(
            r,
            &self.api(&format!("repos/{}/pulls/{number}", r.repository), None)?,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"id":"fixture-repo","nameWithOwner":"owner/repo","viewerPermission":"ADMIN","ref":{"target":{"oid":"a".repeat(40)}},"branchProtectionRules":{"totalCount":0,"pageInfo":{"hasNextPage":false}},"rulesets":{"nodes":[],"pageInfo":{"hasNextPage":false}}})
    }
    #[test]
    fn exact_branch_fallback_distinguishes_absence_from_unconfirmed_reads() {
        let branch = "devdesk/tasks/original";
        let oid = "a".repeat(40);
        let response = |reference: Value| json!({"data":{"repository":{"nameWithOwner":"Owner/Repo","ref":reference}}});
        let observed = response(json!({"name":branch,"prefix":"refs/heads/","target":{"oid":oid}}));
        assert_eq!(
            GitHubPullRequests::parse_exact_branch_head("owner/repo", branch, &observed).unwrap(),
            Some(oid)
        );
        let mut absent = response(Value::Null);
        assert_eq!(
            GitHubPullRequests::parse_exact_branch_head("owner/repo", branch, &absent).unwrap(),
            None
        );
        absent["errors"] = json!([{"message":"partial result"}]);
        for result in [
            absent,
            response(json!({"name":branch,"prefix":"refs/tags/","target":{"oid":"a".repeat(40)}})),
            json!({"data":{"repository":{"nameWithOwner":"Owner/Repo"}}}),
            json!({"data":{"repository":{"nameWithOwner":"different/repo","ref":null}}}),
        ] {
            assert_eq!(
                GitHubPullRequests::parse_exact_branch_head("owner/repo", branch, &result)
                    .unwrap_err()
                    .code,
                "github_ref_response"
            );
        }
    }
    #[test]
    fn pr_request_validation_does_not_query_or_require_branch_protection() {
        let merge = uuid::Uuid::new_v4().to_string();
        let request = PullRequestRequest {
            repository: "owner/repo".into(),
            project: uuid::Uuid::new_v4().to_string(),
            head_branch: format!("devdesk/candidates/{merge}"),
            merge,
            base_branch: "main".into(),
            candidate: "a".repeat(40),
            expected_target: "b".repeat(40),
            number: None,
            projection: None,
        };
        GitHubPullRequests::default()
            .validate_request(&request)
            .unwrap();
    }
    #[test]
    fn never_treat_admin_or_incomplete_rules_as_permission_to_bypass() {
        let mut value = fixture();
        assert!(
            GitHubPullRequests::policy("owner/repo", "main", &value)
                .unwrap()
                .compatible
        );
        value["rulesets"]["nodes"] =
            json!([{"name":"required CI","enforcement":"ACTIVE","target":"BRANCH"}]);
        assert!(
            !GitHubPullRequests::policy("owner/repo", "main", &value)
                .unwrap()
                .compatible
        );
        value = fixture();
        value["branchProtectionRules"]["totalCount"] = json!(1);
        assert!(
            !GitHubPullRequests::policy("owner/repo", "main", &value)
                .unwrap()
                .compatible
        );
        value = fixture();
        value["rulesets"]["pageInfo"]["hasNextPage"] = json!(true);
        assert!(
            !GitHubPullRequests::policy("owner/repo", "main", &value)
                .unwrap()
                .compatible
        );
    }
}
