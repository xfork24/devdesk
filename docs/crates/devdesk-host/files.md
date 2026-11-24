# 文件索引

返回 [host 概览](overview.md)。本表用于查文件，功能过程请先读概览中的专题。

由 `python3 scripts/check/check-docs.py --write` 更新；保留已有的人工专题链接。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/host/Cargo.toml) | Cargo.toml |
| [src/active_frontend_performance.rs](../../../crates/host/src/active_frontend_performance.rs) | Opt-in local HTTP regression fixture, with real resource commits and scheduler. Control steps have no provider I/O. This is not the full browser/W2/P6 workload. |
| [src/background.rs](../../../crates/host/src/background.rs) | [后台阶段与跨项目调度](background-scheduling.md) |
| [src/configuration_read.rs](../../../crates/host/src/configuration_read.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/domains.rs](../../../crates/host/src/domains.rs) | Client-neutral domain directory, commands, and update notifications. |
| [src/event_hub.rs](../../../crates/host/src/event_hub.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/github_projects.rs](../../../crates/host/src/github_projects.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/governance.rs](../../../crates/host/src/governance.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/history_tests.rs](../../../crates/host/src/history_tests.rs) | `add_history`、`combined_history_keeps_global_and_each_project_in_their_own_database`、`operational_pages_do_not_restore_aggregate_history_and_preserve_exact_calls` |
| [src/integrations.rs](../../../crates/host/src/integrations.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/lifecycle.rs](../../../crates/host/src/lifecycle.rs) | [启动、装配与生命周期](workspace-and-lifecycle.md) |
| [src/loading_performance.rs](../../../crates/host/src/loading_performance.rs) | Explicit, offline performance entrypoints. Only a copied, compatible dataset is accepted. |
| [src/main.rs](../../../crates/host/src/main.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/model_providers.rs](../../../crates/host/src/model_providers.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/performance_history.rs](../../../crates/host/src/performance_history.rs) | Explicit, typed history for opt-in performance fixtures. No external effects or model completions are fabricated: discussion Turns were cancelled before dispatch, and their messages are labelled fixture inputs. |
| [src/performance_repository.rs](../../../crates/host/src/performance_repository.rs) | Test-only seed of a private local repository from actual Git receipts. |
| [src/permission_resources.rs](../../../crates/host/src/permission_resources.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/runtime_reads.rs](../../../crates/host/src/runtime_reads.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/runtime_setup.rs](../../../crates/host/src/runtime_setup.rs) | [HTTP、RPC、认证与事件](http-and-security.md) |
| [src/templates.rs](../../../crates/host/src/templates.rs) | Reset only the current definition directory; immutable project bindings retain their history. |
| [src/workflow_live.rs](../../../crates/host/src/workflow_live.rs) | [后台阶段与跨项目调度](background-scheduling.md) |
| [src/workspace.rs](../../../crates/host/src/workspace.rs) | [后台阶段与跨项目调度](background-scheduling.md) |

<!-- crate-files:end -->
