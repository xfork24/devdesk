# 会话、运行资源与结果处理

会话代码负责把应用输入发送给一个确定的运行环境，并把供应商消息变成可追踪的应用记录。它同时服务普通讨论和工作流 Agent，但两者拥有不同的执行权限。

## 准备一轮输入

| 阶段 | 关键文件 | 结果 |
|---|---|---|
| 接收会话操作 | [session_commands.rs](../../../crates/core/src/session_commands.rs)、[conversation_submission.rs](../../../crates/core/src/conversation_submission.rs) | 带身份和原版本的命令 |
| 选择运行资源 | [session_assembly.rs](../../../crates/core/src/session_assembly.rs)、[formal_sessions.rs](../../../crates/core/src/formal_sessions.rs) | 本次采用的 Agent、安装、权限、连接和凭据引用 |
| 形成任务输入 | [formal_inputs.rs](../../../crates/core/src/formal_inputs.rs)、[conversation_admission.rs](../../../crates/core/src/conversation_admission.rs) | Session/Turn 对应的固定输入 |
| 开始运行 | [conversation_start.rs](../../../crates/core/src/conversation_start.rs) | 原操作身份及启动条件 |
| 推进运行 | [conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs)：tick_conversations | 调用 Harness、处理消息并推进阶段 |

SessionResources 端口检查安装和权限是否可以落实。准备失败时不能把运行状态标为已启动；新配置也不能替换原会话所用资源。

## 消息、工具与人工交互

[conversation_observations.rs](../../../crates/core/src/conversation_observations.rs) 处理供应商观察结果，[conversation_captures.rs](../../../crates/core/src/conversation_captures.rs) 保存捕获内容。供应商事件、模型输出、工具请求和终态各有含义，不能仅凭 stdout 结束推断工作流已完成。

[provider_tools.rs](../../../crates/core/src/provider_tools.rs) 处理平台和外部工具桥；[native_context_calls.rs](../../../crates/core/src/native_context_calls.rs) 与 [agent_context.rs](../../../crates/core/src/agent_context.rs) 处理经验上下文读取；[interaction_delivery.rs](../../../crates/core/src/interaction_delivery.rs) 把允许的用户回答返回原交互。

用户输入、模型输出和平台回复保持不同来源。模型不能把自己生成的“用户已批准”当作真实 Human 决定。

## 停止与历史

[conversation_controls.rs](../../../crates/core/src/conversation_controls.rs)、[conversation_workers.rs](../../../crates/core/src/conversation_workers.rs) 和 [worker_reconciliation.rs](../../../crates/core/src/worker_reconciliation.rs) 处理取消、worker 停止和清理。结果不明时保留核对状态，直到获得足够的实际证据。

历史查询通过专用存储读取，不在每次打开对话列表时恢复整个项目。普通讨论可以产生经验和材料，但不会自动变成工作流输出或工程批准。

[proposal_commands.rs](../../../crates/core/src/proposal_commands.rs) 的 `native_proposal_command` 处理提案创建、提取、修订、拒绝和采用。它要求真实完成的讨论轮次、原目标版本和 Human 来源，拒绝把正式工作流会话绕道变成提案。采用提案不产生平台执行证明。

验证入口：[conversations.rs](../../../crates/adapters/tests/conversations.rs)、[agent_capabilities.rs](../../../crates/adapters/tests/agent_capabilities.rs)、[experience_directory.rs](../../../crates/adapters/tests/experience_directory.rs)。协议实现见 [Harness 手册](../devdesk-adapters/harness-and-protocols.md)。
