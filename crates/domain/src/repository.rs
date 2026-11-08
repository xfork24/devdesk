//! Repository registrations and managed worktrees are engineering objects, not runtime installations.
use crate::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeKind {
    Mainline,
    Feature,
    Task,
    Candidate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepositoryAction {
    Candidate {
        merge: Id,
        context: GitContext,
        source: String,
        target: String,
        session: Id,
    },
    PublishRef {
        merge: Id,
        context: GitContext,
        branch: String,
        revision: String,
        expected: Option<String>,
        stage: PublishStage,
    },
    PullRequest {
        merge: Id,
        request: PullRequestRequest,
        observe_only: bool,
    },
    Snapshot {
        run: Id,
        context: GitContext,
        expected: String,
        allowed_paths: Vec<String>,
        #[serde(default)]
        protected_paths: Vec<String>,
    },
    Connect {
        repository: String,
        mainline: String,
    },
    CreateWorktree {
        owner: Id,
        worktree_kind: WorktreeKind,
        base: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepositoryPolicy {
    pub repository: String,
    pub repository_id: String,
    pub branch: String,
    pub head: Option<String>,
    pub viewer_permission: String,
    pub compatible: bool,
    pub blocked_reasons: Vec<String>,
    pub active_branch_rulesets: Vec<String>,
    pub classic_protection_count: u64,
    pub observed_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryReceipt {
    Candidate {
        revision: String,
    },
    PublishedRef {
        revision: String,
        branch: String,
    },
    PullRequest {
        pull_request: PullRequestReceipt,
    },
    Snapshot {
        revision: String,
    },
    Connected {
        policy: GitHubRepositoryPolicy,
        workspace: Workspace,
        head: String,
        worktree: RepositoryWorktree,
    },
    Worktree {
        worktree: RepositoryWorktree,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryWorktree {
    pub id: Id,
    pub project_id: Id,
    pub owner_id: Id,
    pub kind: WorktreeKind,
    pub branch: String,
    pub path: String,
    pub base: String,
    pub operation_id: Id,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryOperationStatus {
    Prepared,
    Dispatched,
    Observed,
    Completed,
    NeedsReconciliation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryOperation {
    pub id: Id,
    pub project_id: Id,
    pub action: RepositoryAction,
    pub status: RepositoryOperationStatus,
    pub receipt: Option<RepositoryReceipt>,
    pub reason: String,
    pub revision: u64,
}
impl EngineeringState {
    pub fn validate_repositories(&self, before: &Self) -> Result<()> {
        for op in &self.repository_operations {
            require(
                self.projects.iter().any(|p| p.id == op.project_id),
                "仓库操作跨项目",
            )?;
            require(
                op.status != RepositoryOperationStatus::Completed || op.receipt.is_some(),
                "仓库操作完成必须有实际回执",
            )?;
            if let Some(old) = before.repository_operations.iter().find(|v| v.id == op.id) {
                require(
                    old.project_id == op.project_id
                        && serde_json::to_value(&old.action).unwrap()
                            == serde_json::to_value(&op.action).unwrap(),
                    "仓库操作意图不可替换",
                )?;
                require(
                    old.status != RepositoryOperationStatus::Completed
                        || serde_json::to_value(old).unwrap() == serde_json::to_value(op).unwrap(),
                    "仓库操作回执不可覆盖",
                )?;
            }
        }
        let mut owners = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        for work in &self.repository_worktrees {
            require(
                owners.insert((&work.project_id, &work.owner_id)) && paths.insert(&work.path),
                "worktree 归属或路径重复",
            )?;
            let valid = match work.kind {
                WorktreeKind::Mainline => work.owner_id == work.project_id,
                WorktreeKind::Feature => self
                    .features
                    .iter()
                    .any(|f| f.id == work.owner_id && f.project_id == work.project_id),
                WorktreeKind::Task => self.tasks.iter().any(|t| {
                    t.id == work.owner_id
                        && self
                            .feature(&t.feature_id)
                            .is_ok_and(|f| f.project_id == work.project_id)
                }),
                WorktreeKind::Candidate => self.merges.iter().any(|m| {
                    m.id == work.owner_id
                        && self
                            .feature(&m.feature_id)
                            .is_ok_and(|f| f.project_id == work.project_id)
                }),
            };
            require(valid, "worktree 归属不存在或跨项目")?;
            require(
                self.repository_operations.iter().any(|op| {
                    op.id == work.operation_id
                        && op.project_id == work.project_id
                        && op.status == RepositoryOperationStatus::Completed
                }),
                "worktree 缺少已完成的创建回执",
            )?;
            if let Some(old) = before.repository_worktrees.iter().find(|v| v.id == work.id) {
                require(
                    serde_json::to_value(old).unwrap() == serde_json::to_value(work).unwrap(),
                    "worktree 创建身份不可变更",
                )?;
            }
        }
        for old in before
            .projects
            .iter()
            .filter(|p| p.execution_mode == "live")
        {
            if let Some(new) = self.projects.iter().find(|p| p.id == old.id) {
                require(
                    new.execution_mode == "live"
                        && serde_json::to_value(&old.workspace_definition).unwrap()
                            == serde_json::to_value(&new.workspace_definition).unwrap()
                        && old.workspace == new.workspace,
                    "真实项目的仓库绑定不能原地切换",
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishStage {
    TargetBase,
    Candidate,
    RemoteAdvance,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestRequest {
    pub repository: String,
    pub project: Id,
    pub merge: Id,
    pub head_branch: String,
    pub base_branch: String,
    pub candidate: String,
    pub expected_target: String,
    pub number: Option<u64>,
}
impl PullRequestRequest {
    pub fn marker(&self) -> String {
        format!(
            "<!-- devdesk:project={}:merge={} -->",
            self.project, self.merge
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestReceipt {
    pub repository: String,
    pub number: u64,
    pub url: String,
    pub head_branch: String,
    pub base_branch: String,
    pub head: String,
    pub base: String,
    pub merged: bool,
    pub merge_commit: Option<String>,
    pub marker: String,
}
