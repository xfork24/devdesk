# 文件索引

返回 [domain 概览](overview.md)。本表用于查文件，功能过程请先读概览中的专题。

由 `python3 scripts/check/check-docs.py --write` 更新；保留已有的人工专题链接。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/domain/Cargo.toml) | Cargo.toml |
| [src/agent_context.rs](../../../crates/domain/src/agent_context.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/agent_task.rs](../../../crates/domain/src/agent_task.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/agents.rs](../../../crates/domain/src/agents.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/artifact.rs](../../../crates/domain/src/artifact.rs) | [工作项与产物](work-and-materials.md) |
| [src/configuration.rs](../../../crates/domain/src/configuration.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/configuration_graph.rs](../../../crates/domain/src/configuration_graph.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/configuration_usages.rs](../../../crates/domain/src/configuration_usages.rs) | Read-only reverse references derived from exact source revisions. |
| [src/conversation.rs](../../../crates/domain/src/conversation.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/deliverable.rs](../../../crates/domain/src/deliverable.rs) | [工作项与产物](work-and-materials.md) |
| [src/execution.rs](../../../crates/domain/src/execution.rs) | Versioned runtime definitions and concrete execution instances. |
| [src/execution_recipe.rs](../../../crates/domain/src/execution_recipe.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/governance.rs](../../../crates/domain/src/governance.rs) | Versioned configuration and executable workflow contracts. No I/O. |
| [src/integrations.rs](../../../crates/domain/src/integrations.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/lib.rs](../../../crates/domain/src/lib.rs) | [工作项与产物](work-and-materials.md) |
| [src/model_provider.rs](../../../crates/domain/src/model_provider.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/orchestration.rs](../../../crates/domain/src/orchestration.rs) | Harness orchestration authority and immutable execution decisions. |
| [src/permission.rs](../../../crates/domain/src/permission.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/permission_profile.rs](../../../crates/domain/src/permission_profile.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/repository.rs](../../../crates/domain/src/repository.rs) | Repository registrations and managed worktrees are engineering objects, not runtime installations. |
| [src/runtime.rs](../../../crates/domain/src/runtime.rs) | [会话、权限和接入类型](runtime-and-permissions.md) |
| [src/source.rs](../../../crates/domain/src/source.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/spec_checks.rs](../../../crates/domain/src/spec_checks.rs) | [工作项与产物](work-and-materials.md) |
| [src/standards.rs](../../../crates/domain/src/standards.rs) | [配置结构、引用和组合](configuration-and-sources.md) |
| [src/template_catalog.rs](../../../crates/domain/src/template_catalog.rs) | Directory membership is independent of immutable configuration and execution history. |
| [src/testing.rs](../../../crates/domain/src/testing.rs) | Frozen test commands are explicit engineering input, independent of a model's success claim. |
| [src/workflow.rs](../../../crates/domain/src/workflow.rs) | [工作流结构与完成规则](workflows-and-checks.md) |
| [src/workflow_change.rs](../../../crates/domain/src/workflow_change.rs) | [工作流结构与完成规则](workflows-and-checks.md) |
| [src/workflow_driver.rs](../../../crates/domain/src/workflow_driver.rs) | Data policies for the installed workflow scheduling primitive. |
| [src/workflow_failure.rs](../../../crates/domain/src/workflow_failure.rs) | [工作流结构与完成规则](workflows-and-checks.md) |
| [src/workflow_time.rs](../../../crates/domain/src/workflow_time.rs) | [工作流结构与完成规则](workflows-and-checks.md) |

<!-- crate-files:end -->
