# 数据格式与检查入口

Schema 约束数据形状，domain 检查结构间的关系，core 结合当前身份和状态判断能否执行。JSON 能通过 Schema，不代表已经获得执行资格。

## 格式地图

| 数据 | 格式入口 | 还要检查什么 |
|---|---|---|
| 配置源 | [source-definition](../../../schemas/source-definition.schema.json)、[source-reference](../../../schemas/source-reference.schema.json) | 类型允许的引用、依赖闭合、修订和摘要 |
| Agent 与能力 | [agent-composition](../../../schemas/agent-composition.schema.json)、[capability](../../../schemas/capability.schema.json) | Role/Capability 组合、驱动实际能力 |
| Workflow | [workflow](../../../schemas/workflow.schema.json) | DAG、端口兼容、执行器、输入和完成规则 |
| 任务输入 | [agent-task](../../../schemas/agent-task.schema.json)、[agent-input](../../../schemas/agent-input.schema.json) | 会话/Turn/步骤身份、固定源与材料来源 |
| 输出与内容检查 | [delivery-type](../../../schemas/delivery-type.schema.json)、[spec](../../../schemas/spec.schema.json) | 实际生产者、输入依据、检查报告与批准 |
| Standard | [standard](../../../schemas/standard.schema.json)、[standard-snapshot](../../../schemas/standard-snapshot.schema.json) | 固定依赖、覆盖引用和生效范围 |
| 权限与 Sandbox | [permission-profile](../../../schemas/permission-profile.schema.json)、[permission-binding](../../../schemas/permission-binding.schema.json)、[sandbox](../../../schemas/sandbox.schema.json) | 当前后端能否落实、目标是否匹配 |
| 外部工具与 Skill | [tool](../../../schemas/tool.schema.json)、[integration](../../../schemas/integration.schema.json)、[skill-package](../../../schemas/skill-package.schema.json) | 发现目录、凭据绑定、工具来源和执行权限 |
| Host 请求 | [rpc](../../../schemas/rpc.schema.json)、[host-lifecycle](../../../schemas/host-lifecycle.schema.json) | 来源、项目作用域、原版本和幂等身份 |

## 读配置时先看什么

源文件根部包含类型、identity、revision、名称、作用域及该类型允许的引用；data 保存该类型的内容。Agent 的 Role/Capability 等引用按当前源协议放在根部，不能为了显示方便重新包成另一套结构。

`identity:revision` 指向确定修订。不要把名称、文件名或目录里的最新文件当作有效引用。实际发布集合由 manifest 决定。

## 一个结果如何检查

需求分析步骤声明 `result` 端口，其 Delivery 要求 PRD 格式。分析师返回 `status/summary/outputs` 后，core 检查必需端口、声明之外的端口、大小预算、Delivery 以及来源。示例见 [PRD 交接](../agent-runtime.md)。

评审还需要说明针对哪些输入、哪些要求作出判断。仅返回 `passed:true` 无法代替有效的检查依据。工程测试结果、模型结论和 Human 决定分别保存。

## 版本号不能混用

配置修订、配置中的 schema_version、SQLite user_version、Host 生命周期 revision 和凭据助手协议版本分别解决不同问题。调整展示版本不能绕过数据库兼容检查；更新工具定义也不能证明已安装 Harness 支持新协议。

完整格式索引见 [schemas/README.md](../../../schemas/README.md)，应用层解析入口见 [配置与编译](../../crates/devdesk-core/configuration-and-catalog.md)。
