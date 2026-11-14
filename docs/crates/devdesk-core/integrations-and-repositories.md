# 外部工具与仓库操作的持久化执行

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

外部副作用需要原操作 ID、冻结请求和实际回执。工具调用与 Git 工作副本使用各自的执行管线；结果未知时先核对，不因为前端超时或 Host 重启就重复调用供应商。

## 本页文件

- [src/integrations.rs](#file-src-integrations-rs)：外部工具冻结、调用状态和接纳回执
- [src/repositories.rs](#file-src-repositories-rs)：仓库接入、主干刷新与依赖就绪后的工作副本

<a id="file-src-integrations-rs"></a>

## src/integrations.rs：外部工具冻结、调用状态和接纳回执

源码：[crates/core/src/integrations.rs](../../../crates/core/src/integrations.rs)。

`freeze_integrations` 沿 Agent/Skill/Tool 和项目绑定固定工具、来源、账户与包材料，确认 capability、认证和精确修订；当前默认选择改变不改旧 Feature/Session。定义删除检查所有仍被使用的修订和子资源，紧急停用另阻止后续执行。

调用管线先建立 ToolInvocation、参数摘要、权限和操作记录，再由 drive_tool_calls 在事务外 start/poll/cancel，最后 accept_tool_receipt 核验真实身份、输出 Schema、文件 Artifact、remote_effect 和状态。用户批准、原生工具调用与独立测试调用保留各自来源，人工 reconcile 不伪造成外部成功。

prepare/finish_integration_operation 为发现、导入或授权相关的外部过程保存 durable 意图；recover_tool_receipts 在重启时读取已有回执，unknown 不重发。integration_query/records 生成有界、遮蔽的 UI/历史投影，回复交付与执行成功分开。

代码定位：[`freeze_integrations`](../../../crates/core/src/integrations.rs#L27)、[`project_tool_request`](../../../crates/core/src/integrations.rs#L184)、[`drive_tool_calls`](../../../crates/core/src/integrations.rs#L524)、[`accept_tool_receipt`](../../../crates/core/src/integrations.rs#L668)、[`prepare_integration_operation`](../../../crates/core/src/integrations.rs#L450)、[`integration_records`](../../../crates/core/src/integrations.rs#L1275)。

关联阅读：[domain/src/integrations.rs](../devdesk-domain/integrations.md#file-src-integrations-rs)、[adapters/src/integrations.rs](../devdesk-adapters/external-integrations.md#file-src-integrations-rs)、[host/src/integrations.rs](../devdesk-host/rpc-and-configuration.md#file-src-integrations-rs)、[adapters/tests/tool_integrations.rs](../devdesk-adapters/testing.md#file-tests-tool-integrations-rs)。

<a id="file-src-repositories-rs"></a>

## src/repositories.rs：仓库接入、主干刷新与依赖就绪后的工作副本

源码：[crates/core/src/repositories.rs](../../../crates/core/src/repositories.rs)。

repository_command 为 connect、refresh、reconcile 保存明确 RepositoryOperation。刷新要求当前真实项目、确切观察 SHA、没有未完成仓库/Git 平台动作；接入在尚未开始工程工作的项目上建立稳定仓库意图。命令事务不执行 git clone 或网络请求。

`tick_repositories` 领取/核对操作，在事务外调用 Git 端口，接纳匹配的仓库、分支、worktree 和 head 回执。未知或重启中断保留需核对状态，不能把目录存在当作正确仓库，也不能默认重跑原 clone/push。

`prepare_task_worktrees` 等任务依赖全部完成后，才以当时 Feature head 生成确切工作副本意图；这防止在前置任务完成前固定旧基线。enqueue_worktree 用稳定 owner 和来源把意图关联起来，不按 UI 当前目录路由。

代码定位：[`repository_command`](../../../crates/core/src/repositories.rs#L4)、[`tick_repositories`](../../../crates/core/src/repositories.rs#L158)、[`prepare_task_worktrees`](../../../crates/core/src/repositories.rs#L458)、[`enqueue_worktree`](../../../crates/core/src/repositories.rs#L469)。

关联阅读：[domain/src/repository.rs](../devdesk-domain/work-and-materials.md#file-src-repository-rs)、[adapters/src/git.rs](../devdesk-adapters/engineering-resources.md#file-src-git-rs)、[host/src/github_projects.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-github-projects-rs)、[adapters/tests/git_resources.rs](../devdesk-adapters/testing.md#file-tests-git-resources-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
