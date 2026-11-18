# 实施合同与基线

2026-11-18，实施基线 `e3957f3`。独立 worktree 为 `/home/hengliu/.codex/worktrees/default-workflow-simplification/devdesk`，分支 `codex/default-workflow-simplification`。原方案保持原文。

精简版使用项目配置中的 `test_configuration`，包含语言、结构化独立测试执行器及固定镜像的项目回归配方。Node 测试保持兼容，新增 Python unittest 与明确 argv/cwd/environment 的通用项目命令，后者不能仅凭非零退出证明有效 Red。准备入口与执行共享同一隔离容器、冻结版本和总超时，源码只读，构建输出使用 `/state` 或 `/tmp`，没有凭据挂载。项目选择复制到工作配置与运行输入，后续配置不改变既有运行。

精简版的 PR 发布采用单父交付快照策略：原开发分支照常提交并推送；交付 ref 保存源与目标试合并的完整 tree，唯一父提交为确切目标基线。PR 使用该受平台管理的交付 ref，原源 SHA、分支、工作归属及完整差异继续进入冻结交付材料。交付 ref 与开发分支分别记录，不能以可见分支名称推导权限。

正式目标只通过 GitHub PR merge 推进。支持的远端合同要求严格 required status check `devdesk/merge-regression`，包含管理员的强制保护，不允许 force-push。平台在确切交付候选实际回归通过后报告该候选状态，以 PR head SHA 前置条件调用 merge；最终核对父提交和 tree，再同步本地正式目标。由于候选是目标的直接子提交，目标被无关提交推进后，服务端严格检查会阻止旧候选合入。未知或不支持的保护组合阻塞，不降低规则。详细版维持旧发布/本地 CAS/外部合入合同。

原始竞争试验在专用测试仓库 `xfork24/devdesk-integration-test` 的隔离分支 `devdesk/simplification-p0-0eac3eb1d3-target` 执行。PR #47 合入到 `46732d1871c2043a45430c8a1fa239eb423bd464`，同一基线的 PR #48 被服务端 405 拒绝；并未推进该仓库 main。最初尝试仅给 test-merge SHA 设置成功状态仍被拒绝，保留原失败。原始请求、响应和版本在 `/tmp/devdesk-default-workflow-simplification-20261118/remote-contract/`。这仅证明远端策略，不代表产品端到端已通过。

用户已授权新建 `xfork24/devdesk-default-workflow-test-20261118`，后续真实验收使用该仓库及专用空数据集。业务 PRD/Design/Tasks 及最终合入仍须应用内 Human 决定。用户于本次会话批准 PRD，并明确授权外层助手代为批准后续项目经理建议批准的阶段；助手经可信 Human 入口提交决定，记录授权来源、确切材料版本及推荐依据。经理 Agent 与观察探针不能冒充 Human，也不能豁免硬性测试与版本门禁。

依据：[GitHub 严格状态检查](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)、[状态检查与 test-merge 选择](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)。
