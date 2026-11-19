# 跨模块调用链

本页按实际动作追踪代码。表中的先后关系表示主要流程，异步阶段之间通过已保存记录衔接，不意味着所有函数在同一调用栈同步完成。

## 1. Host 启动与装配

| 阶段 | 入口与工作 | 交给下一阶段什么 |
|---|---|---|
| 启动 | [main.rs](../../crates/host/src/main.rs) 解析参数、建立生命周期及 HTTP 服务 | 数据集路径、监听与认证配置 |
| 打开工作区 | [Workspace::open](../../crates/host/src/workspace.rs) 打开 GlobalStore，准备项目及讨论上下文 | 各作用域的存储和运行服务 |
| 创建执行适配器 | [execution_adapters](../../crates/adapters/src/lib.rs) 创建 Harness/Git/Tests/PullRequests | core 端口的具体实现 |
| 创建 Engine | workspace.rs 的 `engine` 注入存储、FileArtifacts、NativeTools、经验读取和配置后端 | 可以执行应用用例的 Engine |
| 运行后台阶段 | Workspace::tick 与 [background.rs](../../crates/host/src/background.rs) | 工作流、会话、工具和维护阶段的执行结果 |

项目 Engine 按明确项目身份打开和缓存。HTTP 认证、数据集生命周期与工程状态属于不同层次，不共用一个“当前状态”字段。

## 2. 项目经理派发分析师

| 阶段 | 关键代码 | 检查或状态变化 |
|---|---|---|
| 模型发出工具请求 | [harness.rs](../../crates/adapters/src/harness.rs)：item/tool/call 或 devdesk/tool/call | 转换请求和调用身份，保留供应商请求 ID |
| 工具进入应用 | [provider_tools.rs](../../crates/core/src/provider_tools.rs)、[controller_calls.rs](../../crates/core/src/controller_calls.rs) | 确认当前 Session/Turn、工具和控制权 |
| 派发步骤 | [workflow_dispatch.rs](../../crates/core/src/workflow_dispatch.rs)：dispatch_step | 检查依赖、预算、权限、输入、计划及对象版本 |
| 保存后台操作 | 同文件的 runtime_intent | Agent 步骤生成 session.open，关联 configured_execution_id，要求新会话 |
| 调度操作 | [workflow_runtime.rs](../../crates/core/src/workflow_runtime.rs) 与 Workspace::tick | 读取原操作并核对其执行资格，执行后保存观察结果 |
| 准备会话 | [session_commands.rs](../../crates/core/src/session_commands.rs)、[formal_sessions.rs](../../crates/core/src/formal_sessions.rs) | 解析本次运行资源，建立会话与执行关联 |
| 启动与发送 | [conversation_runtime.rs](../../crates/core/src/conversation_runtime.rs) 经 Harness 端口进入适配器 | 创建/连接容器，发送本轮固定输入 |

派发工具返回后，分析师可能仍在安装、启动或运行。读取流程状态与执行详情才能知道当前阶段。

## 3. PRD 返回和经理继续

Harness 把供应商消息转换成 ConversationObservation。core 保存对应 Turn 的响应和终态；工作流求值读取这次真实结果，再进入 [workflow_outputs.rs](../../crates/core/src/workflow_outputs.rs) 的 `prepare_native_outputs`。

这个函数检查 status、summary、端口、Delivery、内容报告及生产来源，准备 ArtifactSeal 和 workflow_output。产物写入与 SQL 提交分阶段处理，不能在数据库长事务内执行外部 I/O。有效输出进入 awaiting_acceptance 或 waiting_confirmation。

[controller_events.rs](../../crates/core/src/controller_events.rs) 与 [controller_runtime.rs](../../crates/core/src/controller_runtime.rs) 处理经理所需的事件和后续 conversation.send。经理调用 workflow.outputs.read 后，由 [orchestration.rs](../../crates/core/src/orchestration.rs) 的 `read_workflow_outputs` 读取指定产物。下游输入从配置中的端口引用解析，不能改成读取某个任意同名文件。

## 4. 配置从保存到执行

Host 的[配置入口](../../crates/host/src/governance.rs)接收定义变更；core 的 [configuration_changes.rs](../../crates/core/src/configuration_changes.rs) 和 [configuration_publication.rs](../../crates/core/src/configuration_publication.rs) 准备、验证并提交发布。

adapters 的 [source_store.rs](../../crates/adapters/src/source_store.rs) 保存/解码源，目录缓存提供版本化读取。启动时 [configured_workflows.rs](../../crates/core/src/configured_workflows.rs) 的 `compile_workflow_sources` 解析计划，[configured_runtime.rs](../../crates/core/src/configured_runtime.rs) 形成实际任务输入。更新默认不会反向替换原执行的配置。

## 5. 人工决定、返工和计划变更

Web 提交具体对象的决定，Host 赋予可信来源，core 的[工作流评审](../../crates/core/src/workflow_review.rs)及[执行控制](../../crates/core/src/workflow_execution_controls.rs)检查原状态。经理请求决定只会建立待办，不等于批准。

评审拒绝后从 [workflow_rework.rs](../../crates/core/src/workflow_rework.rs) 阅读返工范围和额度。修改计划则从 [workflow_change_preview.rs](../../crates/core/src/workflow_change_preview.rs) 到 [workflow_change_commit.rs](../../crates/core/src/workflow_change_commit.rs)，在应用前核对停止条件、预览和原版本。

## 6. 停止与恢复

Host 的[生命周期](../../crates/host/src/lifecycle.rs)阻止新的工作进入，Workspace::stop_owned_runtime 等待和核对所属运行。core 的 [worker_reconciliation.rs](../../crates/core/src/worker_reconciliation.rs) 与 [workflow_reconciliation.rs](../../crates/core/src/workflow_reconciliation.rs) 检查原执行；adapters 读取实际容器、日志或远端状态。

排查时保留 execution_id、session_id、turn_id、operation_id 等关联身份。新建一个相似任务无法证明原任务已经安全停止。
