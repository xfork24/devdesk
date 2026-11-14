# Agent 身份、会话与轮次

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

一个 Agent 修订描述职责和能力组合，一个 Agent 实例标识工作身份，一次 Session 固定运行选择，一次 Turn 表示输入的实际消费。它们的生命周期不同，不能因为仍是“同一个角色”就复用过期权限，也不能因为界面收到结束文字就提前释放 writer。

## 本页文件

- [src/agents.rs](#file-src-agents-rs)：工作类型与工具能力上限
- [src/conversation.rs](#file-src-conversation-rs)：冻结绑定、输入消费、交互及真实停止

<a id="file-src-agents-rs"></a>

## src/agents.rs：工作类型与工具能力上限

源码：[crates/domain/src/agents.rs](../../../crates/domain/src/agents.rs)。

`AgentRole` 是自由字符串身份，代码没有开发/测试/评审角色枚举。`AgentProfile` 保存 Role 与 Skill 修订的派生组合，要求 Role 非空；当前源 Agent 的 Role/Capability 引用在 source 模块解析。

`WorkType` 区分业务 Workflow、Discussion 和 Orchestration。`ToolPolicy` 表达只读、工作区写入和局部写入的技术上限，`permits` 判断是否容纳请求。它不是完整权限策略，实际可写路径、工具白名单与原生后端仍由 Permission 共同限制。

代码定位：[`AgentRole`](../../../crates/domain/src/agents.rs#L5)、[`AgentProfile`](../../../crates/domain/src/agents.rs#L32)、[`WorkType`](../../../crates/domain/src/agents.rs#L9)、[`ToolPolicy`](../../../crates/domain/src/agents.rs#L17)。

关联阅读：[domain/src/source.rs](configuration-and-sources.md#file-src-source-rs)、[domain/src/permission.rs](runtime-and-permissions.md#file-src-permission-rs)、[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)。

<a id="file-src-conversation-rs"></a>

## src/conversation.rs：冻结绑定、输入消费、交互及真实停止

源码：[crates/domain/src/conversation.rs](../../../crates/domain/src/conversation.rs)。

`RuntimeBinding` 固定模型、参数、实际安装、适配器身份、权限、接入锁与不透明凭据引用；`effective_model_parameters` 优先采用完整 Agent 组合中的冻结参数。SessionScope 表达 Global/Project/Feature/Task，SessionLifecycle 表达准备、活动、空闲、需核对、需后继与关闭。

`ConversationTurn` 独立保存 client_message_id、输入摘要、冻结代码视图、外部消息/轮次和 consumed 标记。事件有 sequence、幂等键及 Full/Delta 模式；工具审批与问题回答由类型化交互记录保存。Proposal 是讨论产生的通用材料，不等于正式 WorkflowOutput。

`RuntimeExecution` 记录 operation、容器、镜像、输入接纳/消费、原生终态、日志和 writer 撤销等不可由配置重建的事实。`validate_conversations` 对比前后状态，禁止原地改 Session 范围/绑定、重写已提交输入或终态证据，并检查单活跃轮次、事件和执行归属。材料 owner 校验阻止跨项目混用。

“模型说完成”“收到原生终态”“实际 writer 已停止”是三个观察；core 需要按端口结果组合它们，不能只看最后一句回复。快照恢复只提供历史背景，不重新执行旧 Turn。

代码定位：[`RuntimeBinding`](../../../crates/domain/src/conversation.rs#L25)、[`ConversationTurn`](../../../crates/domain/src/conversation.rs#L85)、[`ConversationEvent`](../../../crates/domain/src/conversation.rs#L127)、[`PendingInteraction`](../../../crates/domain/src/conversation.rs#L194)、[`RuntimeExecution`](../../../crates/domain/src/conversation.rs#L253)、[`validate_conversations`](../../../crates/domain/src/conversation.rs#L285)。

关联阅读：[core/src/conversations.rs](../devdesk-core/conversations-and-history.md#file-src-conversations-rs)、[core/src/conversation_runtime.rs](../devdesk-core/conversations-and-history.md#file-src-conversation-runtime-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)、[core/src/permission_switch.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-switch-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
