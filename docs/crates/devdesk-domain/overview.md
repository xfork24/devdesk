# devdesk-domain：把工程规则表达成可验证的数据

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

这个包回答“哪些状态和材料是合法的”。它定义项目、任务、会话、源修订、流程、权限、测试和仓库事实，并提供不访问外部系统的检查函数。它不创建目录、不启动模型或容器，也不替调用者取得当前时间和随机身份。

调用方通常先把 JSON 解析成这里的类型，再检查引用、作用域、DAG 和不可变约束。[core 的 Engine](../devdesk-core/engine-and-ports.md#file-src-lib-rs) 决定何时应用这些规则，[SQLite 事务](../devdesk-adapters/storage-and-source-codecs.md#file-src-lib-rs) 在提交状态时再次保护它们。一次字段反序列化成功，只说明形状可读，不表示它已经有执行权限或真实外部证据。

阅读顺序建议是共同工作模型 → 源修订 → Workflow 与内容检查 → 会话 → 执行安装/权限。`lib.rs` 汇出子模块，外部通常通过 `devdesk_domain::*` 使用这些类型。包内有纯规则单元测试，更完整的持久化和流程组合回归放在 adapters 的集成测试中。

## 专题阅读

- [工作对象、材料与真实工程事实](work-and-materials.md)
- [源修订、配置依赖与规范快照](configuration-and-sources.md)
- [Workflow、完成规则与可定位的内容检查](workflows-and-checks.md)
- [Agent 身份、会话与轮次](agents-and-conversations.md)
- [执行安装、模型连接与权限](runtime-and-permissions.md)
- [外部工具、账户绑定与 Skill 包](integrations.md)

## 完整文件索引

由 `python3 scripts/py/check-docs.py --write` 更新。专题解释主要合同，表格覆盖当前全部文件。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/domain/Cargo.toml) | [纯领域包的依赖边界](overview.md#file-cargo-toml) |
| [src/agent_context.rs](../../../crates/domain/src/agent_context.rs) | Versioned, read-only context access shared by every Agent identity. |
| [src/agent_task.rs](../../../crates/domain/src/agent_task.rs) | [任务正文及大输入协议的技术边界](workflows-and-checks.md#file-src-agent-task-rs) |
| [src/agents.rs](../../../crates/domain/src/agents.rs) | [工作类型与工具能力上限](agents-and-conversations.md#file-src-agents-rs) |
| [src/artifact.rs](../../../crates/domain/src/artifact.rs) | Exact identity and immutable bytes for a prepared local file seal. |
| [src/configuration.rs](../../../crates/domain/src/configuration.rs) | [Role 定义的额外可见范围](configuration-and-sources.md#file-src-configuration-rs) |
| [src/configuration_graph.rs](../../../crates/domain/src/configuration_graph.rs) | [只从有类型字段提取依赖](configuration-and-sources.md#file-src-configuration-graph-rs) |
| [src/configuration_usages.rs](../../../crates/domain/src/configuration_usages.rs) | Read-only reverse references derived from exact source revisions. |
| [src/conversation.rs](../../../crates/domain/src/conversation.rs) | [冻结绑定、输入消费、交互及真实停止](agents-and-conversations.md#file-src-conversation-rs) |
| [src/deliverable.rs](../../../crates/domain/src/deliverable.rs) | [交付类型的格式、结构与产出来源](work-and-materials.md#file-src-deliverable-rs) |
| [src/execution.rs](../../../crates/domain/src/execution.rs) | [执行环境与精确 Git 路由](work-and-materials.md#file-src-execution-rs) |
| [src/execution_recipe.rs](../../../crates/domain/src/execution_recipe.rs) | [配置决定运行配方，摘要决定实际来源](runtime-and-permissions.md#file-src-execution-recipe-rs) |
| [src/governance.rs](../../../crates/domain/src/governance.rs) | [源记录信封、Scope 与执行组合类型](configuration-and-sources.md#file-src-governance-rs) |
| [src/integrations.rs](../../../crates/domain/src/integrations.rs) | [外部来源、工具合同、账户锁与执行回执](integrations.md#file-src-integrations-rs) |
| [src/lib.rs](../../../crates/domain/src/lib.rs) | [项目、任务、材料与聚合状态](work-and-materials.md#file-src-lib-rs) |
| [src/model_provider.rs](../../../crates/domain/src/model_provider.rs) | [服务商事实、容量及冻结连接的纯规则](runtime-and-permissions.md#file-src-model-provider-rs) |
| [src/orchestration.rs](../../../crates/domain/src/orchestration.rs) | [控制授权、租约和不可混淆的决定](workflows-and-checks.md#file-src-orchestration-rs) |
| [src/permission.rs](../../../crates/domain/src/permission.rs) | [可移植策略与实际执行计划](runtime-and-permissions.md#file-src-permission-rs) |
| [src/permission_profile.rs](../../../crates/domain/src/permission_profile.rs) | [Profile、目标绑定和双端权限锁](runtime-and-permissions.md#file-src-permission-profile-rs) |
| [src/repository.rs](../../../crates/domain/src/repository.rs) | [仓库操作、工作副本与 PR 回执](work-and-materials.md#file-src-repository-rs) |
| [src/runtime.rs](../../../crates/domain/src/runtime.rs) | [驱动、安装产物和完整性结果](runtime-and-permissions.md#file-src-runtime-rs) |
| [src/source.rs](../../../crates/domain/src/source.rs) | [规范源引用与纯解析入口](configuration-and-sources.md#file-src-source-rs) |
| [src/spec_checks.rs](../../../crates/domain/src/spec_checks.rs) | [机器断言与冻结输入位置证明](workflows-and-checks.md#file-src-spec-checks-rs) |
| [src/standards.rs](../../../crates/domain/src/standards.rs) | [规范闭包、稳定摘要与结构差异](configuration-and-sources.md#file-src-standards-rs) |
| [src/template_catalog.rs](../../../crates/domain/src/template_catalog.rs) | [目录代次与不可变历史分离](configuration-and-sources.md#file-src-template-catalog-rs) |
| [src/testing.rs](../../../crates/domain/src/testing.rs) | [可重复执行的 Node 测试配方](work-and-materials.md#file-src-testing-rs) |
| [src/workflow.rs](../../../crates/domain/src/workflow.rs) | [流程图、执行记录与完成事实的共同合同](workflows-and-checks.md#file-src-workflow-rs) |
| [src/workflow_change.rs](../../../crates/domain/src/workflow_change.rs) | [类型化变更与保守影响图](workflows-and-checks.md#file-src-workflow-change-rs) |
| [src/workflow_driver.rs](../../../crates/domain/src/workflow_driver.rs) | [有界自动调度的可配置参数](workflows-and-checks.md#file-src-workflow-driver-rs) |
| [src/workflow_failure.rs](../../../crates/domain/src/workflow_failure.rs) | [失败类别、副作用结论与安全重试前提](workflows-and-checks.md#file-src-workflow-failure-rs) |
| [src/workflow_time.rs](../../../crates/domain/src/workflow_time.rs) | Explicit v1 limits for logical execution wall time and Human waiting. Legacy activity time remains a separate budget. No clock or environment I/O. |

<!-- crate-files:end -->

<a id="file-cargo-toml"></a>

## Cargo.toml：纯领域包的依赖边界

源码：[crates/domain/Cargo.toml](../../../crates/domain/Cargo.toml)。

包名为 `devdesk-domain`，使用 workspace 的版本和 Rust edition。依赖 serde/serde_json 表达合同，thiserror 表达错误，sha2 计算内容身份，url 解析连接地址；没有对 core、adapters、host 的依赖。

这些库允许解析和计算，不表示允许在领域代码中访问文件、网络或进程。新增领域规则应接收已经取得的事实作为参数；涉及时间、随机 ID 或外部观察时，由上层端口提供，再交给纯规则验证。


---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
