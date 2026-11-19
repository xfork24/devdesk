# Git、测试和 GitHub 适配器

工程适配器接收已经确定的工作区、代码版本和操作身份，执行真实命令，再返回可以核对的结果。它们不替工作流决定批准或完成。

## Git 工作区

[git.rs](../../../crates/adapters/src/git.rs) 的 CliGit 管理仓库克隆、worktree、快照、提交关系、diff 和 Git 端口操作。`prepare_worktree`、`snapshot_worktree_scoped`、`commit_parents`、`diff` 是重要阅读点。

[repository_contexts.rs](../../../crates/adapters/src/repository_contexts.rs) 处理仓库上下文，[git_workspace_continuation.rs](../../../crates/adapters/src/git_workspace_continuation.rs) 处理工作区继续执行。输入应绑定项目和原版本，不能接受模型任意选择宿主目录。

版本推进需要核对候选提交、父提交、diff 和目标是否仍为预期值。命令退出码正常，不足以证明返回的仓库就是原请求的工作区。

## 真实测试

[tests.rs](../../../crates/adapters/src/tests.rs) 是生产 CommandTests 实现。它在 Docker 中运行 TestRequest 指定的测试，提供 poll、stop、cleanup、inspect/reconcile 等能力。测试报告关联候选版本、环境和实际运行。

[git_test_configuration.rs](../../../crates/adapters/src/git_test_configuration.rs) 读取项目测试配置。测试安装、命令、代码输入和产物都要关联原请求；不能让模型返回一段“测试通过”替代运行结果。

## 远端交付

[github.rs](../../../crates/adapters/src/github.rs) 的 GitHubPullRequests 实现远端操作，[github_delivery.rs](../../../crates/adapters/src/github_delivery.rs) 处理交付状态和合入，[git_delivery_projection.rs](../../../crates/adapters/src/git_delivery_projection.rs) 处理本地交付结果。

合入要核对确切源、目标和试合并结果。无保护目标通过 beforeOid 与 force=false 限定快进；有保护目标遵守 PR 门禁。操作后读取实际提交关系与 tree，不能只因为 PR 存在就报告本次交付完成。

## 失败后的检查

网络中断时远端可能已经发生修改。inspect/reconcile 读取原操作对应的状态，帮助 core 决定如何接纳。已发生的推送不会因取消本地工作流而回滚。

验证入口：[git_resources.rs](../../../crates/adapters/tests/git_resources.rs)、[github_resources.rs](../../../crates/adapters/tests/github_resources.rs)、[command_tests.rs](../../../crates/adapters/tests/command_tests.rs)、[worktree_receipt_concurrency.rs](../../../crates/adapters/tests/worktree_receipt_concurrency.rs)。真实 GitHub 验收需专用仓库和授权，不能从协议夹具推断账号实际可用。
