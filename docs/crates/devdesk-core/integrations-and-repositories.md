# 外部工具与仓库操作的持久化执行

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

外部副作用需要原操作 ID、冻结请求和实际回执。工具调用与 Git 工作副本使用各自的执行管线；结果未知时先核对，不因为前端超时或 Host 重启就重复调用供应商。

## 本页文件

- [src/integrations.rs](#file-src-integrations-rs)：外部工具冻结、调用状态和接纳回执
- [src/repository_runtime.rs](#file-src-repositories-rs)：仓库接入、主干刷新与依赖就绪后的工作副本

<a id="file-src-integrations-rs"></a>

## src/integrations.rs：外部工具冻结、调用状态和接纳回执

源码：[crates/core/src/integrations.rs](../../../crates/core/src/integrations.rs)。

`freeze_integrations` 沿 Agent/Skill/Tool 和项目绑定固定工具、来源、账户与包材料，确认 capability、认证和精确修订；当前默认选择改变不改旧 Feature/Session。定义删除检查所有仍被使用的修订和子资源，紧急停用另阻止后续执行。

调用管线先建立 ToolInvocation、参数摘要、权限和操作记录；insert_tool_invocation 核对工具/来源/绑定启用、认证、输入 Schema 与当前授权，再由 drive_tool_calls 在事务外 start/poll/cancel。accept_tool_receipt 只核对 invocation_id 和 External origin，按 success/remote_effect 保存回执与状态；它不重新校验远端输出 Schema，也不读取结果文件。

生产 NativeTools 在返回回执前校验 output_schema，将文件转换为带摘要/大小的专用资源链接；文件读取由其 read_artifact 再核对。正式 Workflow 输出还要通过 configured_runtime 的 Delivery/Spec/provenance 接纳。这些检查位于不同边界，不能把替身 ToolExecutor 返回一个成功回执当作已跑过生产输出校验。用户批准、原生工具调用与独立工具试用保留各自来源，人工 reconcile 不伪造成外部成功。

prepare/finish_integration_operation 为发现、导入或授权相关的外部过程保存 durable 意图；recover_tool_receipts 在重启时读取已有回执，unknown 不重发。integration_query/records 生成有界、遮蔽的 UI/历史投影，回复交付与执行成功分开。

代码定位：[`freeze_integrations`](../../../crates/core/src/integrations.rs#L27)[`drive_tool_calls`](../../../crates/core/src/integrations.rs#L365)[`prepare_integration_operation`](../../../crates/core/src/integration_operations.rs#L242)、[`integration_records`](../../../crates/core/src/integrations.rs#L934)。

关联阅读：[domain/src/integrations.rs](../devdesk-domain/integrations.md#file-src-integrations-rs)、[adapters/src/integrations.rs](../devdesk-adapters/external-integrations.md#file-src-integrations-rs)、[host/src/integrations.rs](../devdesk-host/rpc-and-configuration.md#file-src-integrations-rs)、[adapters/tests/tool_integrations.rs](../devdesk-adapters/testing.md#file-tests-tool-integrations-rs)。

<a id="file-src-repositories-rs"></a>

## src/repository_runtime.rs：仓库接入、主干刷新与依赖就绪后的工作副本

源码：[crates/core/src/repository_runtime.rs](../../../crates/core/src/repository_runtime.rs)。

[repository_updates.rs](../../../crates/core/src/repository_updates.rs) 准备有限的仓库更新；`native_repository_command` 将 connect、refresh、reconcile 路由到资源级提交。命令核对真实项目、目标版本和未完成操作，事务内只保存意图。

`tick_repositories` 在事务外调用 Git 端口，再接纳与原操作匹配的分支、worktree 和 head。未知或重启中断保持待核对，不因目录存在就假定 clone/push 成功。

`prepare_task_worktrees` 在 Task 依赖完成后才准备工作副本，防止提前固定旧 Feature 基线。[repository_dispatch.rs](../../../crates/core/src/repository_dispatch.rs) 负责有限派发合同，[repository_projection.rs](../../../crates/core/src/repository_projection.rs) 接纳外部事实。公开业务命令、实际执行和事实投影各有来源校验。

关联阅读：[domain/src/repository.rs](../devdesk-domain/work-and-materials.md#file-src-repository-rs)、[adapters/src/git.rs](../devdesk-adapters/engineering-resources.md#file-src-git-rs)、[host/src/github_projects.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-github-projects-rs)、[adapters/tests/git_resources.rs](../devdesk-adapters/testing.md#file-tests-git-resources-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
