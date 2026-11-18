# devdesk-core：决定什么时候可以推进，以及接纳什么事实

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

core 把领域规则组织成应用用例。它负责发布配置、固定执行选择、准备会话、派发工作、接纳真实回执、处理人工决定与恢复，不直接依赖 SQLite、Docker 或某个模型 SDK。外部能力通过 [ports.rs](engine-and-ports.md#file-src-ports-rs) 注入。

`Engine` 是单个工程状态服务，不是全局项目管理器。Host 为全局配置、各项目和全局讨论装配不同实例，再共享实际执行适配器。命令通常先在短事务内保存决定和意图；原生执行在事务外进行；观察结果后再开事务核对与接纳。`command_id` 与参数指纹处理重放，InvocationContext 处理可信来源，二者都不能由模型的自然语言声明替代。

阅读时先看 Engine/ports，再看配置编译和 configured_runtime，随后进入 orchestration、controllers 和 workflow_driver。三个模块分别负责控制命令、经理原生会话和 Rust 动作选择，不能把它们当成同一层。workflow_eligibility 为查询和命令提供共同业务资格，workflow_failure 从持久化意图/回执解析可信失败及独立重试次数；认证、CAS 和实际状态仍在提交时复核。

## 专题阅读

- [Engine、调用来源与可注入端口](engine-and-ports.md)
- [配置创作、共享目录、采用与跨库发布](configuration-and-catalog.md)
- [从源配置到冻结计划与正式任务](workflow-compilation.md)
- [步骤状态机、执行意图与可信运行事实](workflow-execution.md)
- [运行计划修改、静止等待与有界委托](workflow-changes.md)
- [控制命令、经理会话与 Rust 调度](orchestration-and-drivers.md)
- [经理知识、平台动作与 Spec 检查](manager-and-platform-tools.md)
- [会话命令、原生观察、提案与历史](conversations-and-history.md)
- [完整会话绑定、权限修订与执行安装回执](session-resources-and-permissions.md)
- [外部工具与仓库操作的持久化执行](integrations-and-repositories.md)

## 完整文件索引

每个文件只有一个主要讲解位置；相关专题通过链接引用它。

| 文件 | 职责与实现说明 |
|---|---|
| [Cargo.toml](../../../crates/core/Cargo.toml) | [应用层包与依赖注入的构建约束](overview.md#file-cargo-toml) |
| [src/agent_tasks.rs](../../../crates/core/src/agent_tasks.rs) | [正式会话与具体执行的唯一关联](workflow-compilation.md#file-src-agent-tasks-rs) |
| [src/builtin_defaults.rs](../../../crates/core/src/builtin_defaults.rs) | [从当前目录读取默认选择](configuration-and-catalog.md#file-src-builtin-defaults-rs) |
| [src/catalog_sync.rs](../../../crates/core/src/catalog_sync.rs) | [共享源副本与经验目录索引](configuration-and-catalog.md#file-src-catalog-sync-rs) |
| [src/command_catalog.rs](../../../crates/core/src/command_catalog.rs) | [哪些操作受模板代次围栏约束](engine-and-ports.md#file-src-command-catalog-rs) |
| [src/configuration.rs](../../../crates/core/src/configuration.rs) | [配置可见性、组合与引用使用方查询](configuration-and-catalog.md#file-src-configuration-rs) |
| [src/configuration_changes.rs](../../../crates/core/src/configuration_changes.rs) | [同一候选图中的预演、诊断和原子提交](configuration-and-catalog.md#file-src-configuration-changes-rs) |
| [src/configuration_transfers.rs](../../../crates/core/src/configuration_transfers.rs) | [共享发布与项目采用的跨库恢复协议](configuration-and-catalog.md#file-src-configuration-transfers-rs) |
| [src/configured_runtime.rs](../../../crates/core/src/configured_runtime.rs) | [冻结实例的主状态机与任务渲染](workflow-execution.md#file-src-configured-runtime-rs) |
| [src/configured_workflows.rs](../../../crates/core/src/configured_workflows.rs) | [发布完整配置闭包并生成内存执行计划](workflow-compilation.md#file-src-configured-workflows-rs) |
| [src/controllers.rs](../../../crates/core/src/controllers.rs) | [原生经理会话、唤醒和控制调用围栏](orchestration-and-drivers.md#file-src-controllers-rs) |
| [src/conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs) | [派发原生 Turn、批量观察与真实终态裁定](conversations-and-history.md#file-src-conversation-runtime-rs) |
| [src/conversations.rs](../../../crates/core/src/conversations.rs) | [会话打开、排队输入、交互与快照](conversations-and-history.md#file-src-conversations-rs) |
| [src/default_permissions.rs](../../../crates/core/src/default_permissions.rs) | [给当前出厂目标补齐默认绑定](session-resources-and-permissions.md#file-src-default-permissions-rs) |
| [src/driver_capabilities.rs](../../../crates/core/src/driver_capabilities.rs) | [实际驱动能力的进程级事实](session-resources-and-permissions.md#file-src-driver-capabilities-rs) |
| [src/environment.rs](../../../crates/core/src/environment.rs) | [可复现的 ID 与时间来源](engine-and-ports.md#file-src-environment-rs) |
| [src/frozen_views.rs](../../../crates/core/src/frozen_views.rs) | [从历史源选择纯重建 Plan 与任务](workflow-compilation.md#file-src-frozen-views-rs) |
| [src/governance.rs](../../../crates/core/src/governance.rs) | [单项源命令、Agent 实例和工程配置冻结](configuration-and-catalog.md#file-src-governance-rs) |
| [operational read 端口](../../../crates/core/src/ports.rs) | [先过滤分页的事实时间线](conversations-and-history.md#file-src-history-rs) |
| [src/integrations.rs](../../../crates/core/src/integrations.rs) | [外部工具冻结、调用状态和接纳回执](integrations-and-repositories.md#file-src-integrations-rs) |
| [src/invocation.rs](../../../crates/core/src/invocation.rs) | [调用者身份不能来自 RPC 自填](engine-and-ports.md#file-src-invocation-rs) |
| [src/lib.rs](../../../crates/core/src/lib.rs) | [命令事务、总分派与恢复入口](engine-and-ports.md#file-src-lib-rs) |
| [src/live_runs.rs](../../../crates/core/src/live_runs.rs) | [给会话选择代码视图并停止本进程执行者](workflow-execution.md#file-src-live-runs-rs) |
| [src/manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) | [固定项目的知识查询与配置创作](manager-and-platform-tools.md#file-src-manager-workspace-rs) |
| [src/orchestration.rs](../../../crates/core/src/orchestration.rs) | [显式控制、实际接纳、材料读取与动作凭据](orchestration-and-drivers.md#file-src-orchestration-rs) |
| [src/permission_resources.rs](../../../crates/core/src/permission_resources.rs) | [Profile 修订、导入导出与绑定 CAS](session-resources-and-permissions.md#file-src-permission-resources-rs) |
| [src/permission_switch.rs](../../../crates/core/src/permission_switch.rs) | [以可恢复后继替换会话权限](session-resources-and-permissions.md#file-src-permission-switch-rs) |
| [src/platform_tools.rs](../../../crates/core/src/platform_tools.rs) | [工作空间、任务、测试和 Git 的受管处理器](manager-and-platform-tools.md#file-src-platform-tools-rs) |
| [src/ports.rs](../../../crates/core/src/ports.rs) | [应用需要外部世界提供的完整合同](engine-and-ports.md#file-src-ports-rs) |
| [src/proposals.rs](../../../crates/core/src/proposals.rs) | [讨论提案的提取、修订与显式采纳](conversations-and-history.md#file-src-proposals-rs) |
| [src/repositories.rs](../../../crates/core/src/repositories.rs) | [仓库接入、主干刷新与依赖就绪后的工作副本](integrations-and-repositories.md#file-src-repositories-rs) |
| [src/resource_receipts.rs](../../../crates/core/src/resource_receipts.rs) | [应用侧掌握经验导入和 Harness 物化事实](session-resources-and-permissions.md#file-src-resource-receipts-rs) |
| [src/session_assembly.rs](../../../crates/core/src/session_assembly.rs) | [把完整 Agent 装配成可执行 Session](session-resources-and-permissions.md#file-src-session-assembly-rs) |
| [src/spec_lint.rs](../../../crates/core/src/spec_lint.rs) | [结构预览与实际产物接纳共用的检查链](manager-and-platform-tools.md#file-src-spec-lint-rs) |
| [src/standards.rs](../../../crates/core/src/standards.rs) | [规范快照发布与每个工作的独立采用](configuration-and-catalog.md#file-src-standards-rs) |
| [src/templates.rs](../../../crates/core/src/templates.rs) | [外层资源注入与模板目录重置](configuration-and-catalog.md#file-src-templates-rs) |
| [src/workflow_changes.rs](../../../crates/core/src/workflow_changes.rs) | [预演影响、停止执行者、切换计划与 Segment](workflow-changes.md#file-src-workflow-changes-rs) |
| [src/workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) | [Rust 根驱动的动作选择、预算与围栏](orchestration-and-drivers.md#file-src-workflow-driver-rs) |
| [src/workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs) | [查询与提交共用的业务动作资格](orchestration-and-drivers.md#file-src-workflow-eligibility-rs) |
| [src/workflow_failure.rs](../../../crates/core/src/workflow_failure.rs) | [从持久化来源恢复失败与独立尝试计数](workflow-execution.md#file-src-workflow-failure-rs) |
| [src/workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) | [执行意图的领取和派发结果登记](workflow-execution.md#file-src-workflow-runtime-rs) |
| [src/workflow_runtime_facts.rs](../../../crates/core/src/workflow_runtime_facts.rs) | [把真实外层运行身份交给后继任务](workflow-execution.md#file-src-workflow-runtime-facts-rs) |

<a id="file-cargo-toml"></a>

## Cargo.toml：应用层包与依赖注入的构建约束

源码：[crates/core/Cargo.toml](../../../crates/core/Cargo.toml)。

`devdesk-core` 只依赖 domain、serde/serde_json 和 uuid，使用 workspace 的版本与 edition。它没有 rusqlite、Docker 客户端、HTTP SDK 或出厂资源 build.rs；这些能力由外层实现并注入。

uuid 用于应用层默认身份来源，确定性测试可以通过 ApplicationEnvironment 替换。新增用例若需要访问环境，应先扩展或复用端口，而不是让 core 引入适配器具体类型。


---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
