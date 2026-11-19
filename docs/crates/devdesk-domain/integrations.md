# 外部工具、账户绑定与 Skill 包

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

工具来源、单个动作、项目账户和一次调用是不同对象。领域层把这些对象及其冻结关系表达清楚，执行器才能校验“这个项目中的这次调用，是否允许使用这个工具和账户”。静态 Skill 材料不等于可以自动运行其中的脚本。

## 本页文件

- [src/integrations.rs](#file-src-integrations-rs)：外部来源、工具合同、账户锁与执行回执

<a id="file-src-integrations-rs"></a>

## src/integrations.rs：外部来源、工具合同、账户锁与执行回执

源码：[crates/domain/src/integrations.rs](../../../crates/domain/src/integrations.rs)。

`IntegrationSource` 区分 HTTP 和 MCP，定义源端点/传输以及明确的调用限制。`ExternalTool` 保存某个动作的输入/输出 Schema、实现映射、来源摘要与认证需求；`IntegrationBinding` 为实际工作选择账户、允许 origin、工具和执行环境。`ToolAuthRequirement.satisfied_by` 防止工具需求与绑定认证不一致。

OAuth 配置、认证引用、静态 Skill 文件/包及 BoundTool、IntegrationLock 都在这里表达。包校验约束路径和静态材料，不把脚本安装或执行作为导入副作用。`ToolInvocation` 保存冻结参数、调用身份和状态，`ToolReceipt` 保存输出、文件结果、错误及远端副作用判断。

调用失败或取消不表示远端回滚。core 需要在 unknown 时核对，HTTP/MCP 真正运行及认证注入则由 adapters 实现；本文件不发请求、不读取凭据。

代码定位：[`IntegrationSource`](../../../crates/domain/src/integrations.rs#L16)、[`IntegrationBinding`](../../../crates/domain/src/integrations.rs#L172)、[`ExternalTool`](../../../crates/domain/src/integrations.rs#L218)、[`SkillPackage`](../../../crates/domain/src/integrations.rs#L274)、[`IntegrationLock`](../../../crates/domain/src/integrations.rs#L298)、[`ToolReceipt`](../../../crates/domain/src/integrations.rs#L330)。

关联阅读：[core/src/integrations.rs](../devdesk-core/integrations-and-repositories.md#file-src-integrations-rs)、[adapters/src/integrations.rs](../devdesk-adapters/external-integrations.md#file-src-integrations-rs)、[adapters/src/integration_oauth.rs](../devdesk-adapters/external-integrations.md#file-src-integration-oauth-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
