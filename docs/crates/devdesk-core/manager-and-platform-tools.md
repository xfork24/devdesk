# 项目经理与平台工具

经理 Agent 通过受控工具读取项目并推进工作流。工具请求必须来自当前授权的会话和轮次，不能把模型传入的项目 ID 或 actor 字段直接当作可信身份。

## 工具请求的处理路径

1. adapters 把供应商动态工具请求转换成统一观察消息。
2. [provider_tools.rs](../../../crates/core/src/provider_tools.rs) 与 [controller_calls.rs](../../../crates/core/src/controller_calls.rs) 关联会话、Turn、调用 ID 和运行权限。
3. [platform_tools.rs](../../../crates/core/src/platform_tools.rs) 对照处理器注册及 ToolDefinition，进入对应业务路径。
4. 业务处理保存结果或后台操作；[platform_replies.rs](../../../crates/core/src/platform_replies.rs) 等模块将结果关联到原请求，再通过 Harness 端口回复。

工具定义来自已采用的配置。存在 handler 但没有发布定义、或定义未被当前 Capability 采用，都不等于模型可以调用它。完整清单只在[工具参考](../../design/reference/tools-and-primitives.md)维护。

## 两组经理能力

| 能力组 | 定义与入口 | 作用 |
|---|---|---|
| CONTROL_TOOLS | [controllers.rs](../../../crates/core/src/controllers.rs) | 工作流检查、派发、接纳、暂停、返工和决定请求 |
| MANAGER_TOOLS | [manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) | 项目读取、配置查询/变更及内容检查 |

ManagerServices 向经理工作台提供共享配置 Engine 和 ProjectInventory。工作台能力与根调度能力分别授权；普通分析师可使用部分读取工具，并不因此成为经理。

manager_workspace 的内容读取会处理分块、摘要校验和已知凭据字段脱敏。增加新敏感字段时，需要检查这里以及调用历史的显示路径。

## 经理如何被唤醒

[controller_events.rs](../../../crates/core/src/controller_events.rs) 汇集需要处理的事件，[controller_runtime.rs](../../../crates/core/src/controller_runtime.rs) 的 `drive_controllers` 准备后续会话输入和 conversation.send。事件及待处理输入持久化，浏览器离线不影响后台经理继续。

controller 的 epoch、会话绑定和原操作关系用于拒绝旧控制者的迟到请求。`workflow.controller.wait` 表示等待后续变化；它不要求 Host 一直阻塞当前 HTTP 请求。

## 工程工具的执行阶段

[platform_preparation.rs](../../../crates/core/src/platform_preparation.rs) 准备固定请求，[platform_runtime.rs](../../../crates/core/src/platform_runtime.rs) 执行端口调用，[platform_observations.rs](../../../crates/core/src/platform_observations.rs) 和 [platform_reconciliation.rs](../../../crates/core/src/platform_reconciliation.rs) 处理实际结果及核对。平台处理器不是一张直接映射到 shell 的表。

验证入口：[manager_workspace.rs](../../../crates/adapters/tests/manager_workspace.rs)、[orchestration.rs](../../../crates/adapters/tests/orchestration.rs)、[controller_event_tests.rs](../../../crates/core/src/controller_event_tests.rs)。
