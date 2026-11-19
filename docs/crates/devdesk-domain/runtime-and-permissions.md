# 会话、权限和接入类型

本专题定义跨模块交换的运行数据。实际进程和认证仍由 adapters 实现。

## 会话和任务

[conversation.rs](../../../crates/domain/src/conversation.rs) 表达 Session/Turn 及交互状态；[runtime.rs](../../../crates/domain/src/runtime.rs) 表达运行安装和绑定；[agent_task.rs](../../../crates/domain/src/agent_task.rs) 表达发给 Agent 的本轮任务。

同一个可复用 Agent 定义可以用于多次会话。一轮任务必须能追溯到原会话、执行和采用的配置，不能仅靠模型名称或工作目录关联。

[agent_context.rs](../../../crates/domain/src/agent_context.rs) 描述 Agent 可引用的上下文与经验。一个经验引用是读取入口，不能被解释为任意读取源项目的权限。

## 权限类型如何使用

[permission_profile.rs](../../../crates/domain/src/permission_profile.rs) 表达权限规则，[permission.rs](../../../crates/domain/src/permission.rs) 表达相关绑定和执行要求。domain 检查结构及规则关系；core 选择本次绑定；adapters 检查当前平台能否落实并生成实际限制。

Harness 与 Sandbox 是两层不同约束。替换一层默认绑定，不会自动取消另一层限制；无法表达或执行的权限也不能默认为允许。

## 外部接入的身份

[integrations.rs](../../../crates/domain/src/integrations.rs) 包含来源、项目绑定、工具定义、调用和 Skill 包相关类型。来源定义描述服务，项目绑定选择账户与授权范围，工具定义说明可调用方法和 Schema。

新增接入协议需要同步更新这些类型、Schema、core 的调用检查与 adapters 的实现。只增加 enum 变体不足以支持生产运行。

相关验证：[agent_capabilities.rs](../../../crates/adapters/tests/agent_capabilities.rs)、[permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs)、[tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs)。实际执行见[运行适配器](../devdesk-adapters/harness-and-protocols.md)。
