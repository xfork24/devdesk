# 配置结构、引用和组合

配置类型回答“可以怎样组合”，core 再根据实际目录决定“本次采用哪一组”。domain 不从文件系统加载配置。

## 主要入口

| 文件 | 在组合过程中的作用 |
|---|---|
| [source.rs](../../../crates/domain/src/source.rs) | 源定义与确定修订引用 |
| [configuration_graph.rs](../../../crates/domain/src/configuration_graph.rs) | 引用图和依赖关系 |
| [configuration.rs](../../../crates/domain/src/configuration.rs) | 配置记录与工作配置类型 |
| [agents.rs](../../../crates/domain/src/agents.rs) | Agent、Role、Capability 的组合 |
| [standards.rs](../../../crates/domain/src/standards.rs) | 工程规范及固定快照 |
| [execution_recipe.rs](../../../crates/domain/src/execution_recipe.rs) | Harness/Sandbox 组合后的执行配方 |
| [model_provider.rs](../../../crates/domain/src/model_provider.rs) | 模型服务连接、路由及容量规则 |

## 一次组合的输入和结果

输入是一组源定义及其明确引用。检查包括类型允许的引用、依赖是否存在、是否形成循环、组合是否满足结构要求。结果是可供编译和运行准备使用的类型与关系，尚未创建会话或执行镜像。

Agent 只组合 Role 和 Capability；步骤负责具体任务；Skill 提供方法；Profile 提供权限。将这些信息塞进 Agent 名称或某个业务 key 会绕过显式结构，后续无法可靠追踪版本。

## 修改时需要保持的关系

根引用与 data 的内容都参与配置身份和摘要。新增引用字段时同时更新源类型、允许关系、Schema、解析和发布资源。不要只更新 UI 表单。

Standard 的覆盖需要明确原引用与替代引用，并检查依赖闭合。恢复执行要使用原来的源集合，而不是重新读取当前默认。

相关验证：[configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs)、[standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs)、[project_standards.rs](../../../crates/adapters/tests/project_standards.rs)。应用过程见 [core 配置](../devdesk-core/configuration-and-catalog.md)。
