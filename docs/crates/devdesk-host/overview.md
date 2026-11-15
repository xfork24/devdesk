# devdesk-host：把界面请求接到正确工程和真实执行端口

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

Host 是可执行程序，提供 HTTP/JSON-RPC、SSE、嵌入式 Web 和数据集入口。Workspace 管理一个全局配置 Engine 与多个工程 Engine，把请求固定到正确数据库，并让各项目共享受管执行适配器和总并发预算。

Host 负责连接认证、参数路由、资源准备与端口装配；业务状态、Actor 规则、配置发布和终态接纳仍调用 core。普通工程请求可携带预期 project_id，活动项目已改变时会被拒绝；选定 Engine 后固定本次范围。切换项目不终止后台其他工程，也不把它们的历史搬到新项目。全局讨论另有按需打开的独立 Engine，不与 global.sqlite3 的共享配置 Engine 混用。

本包没有独立 lib.rs。main.rs 引入 workspace.rs，后者用显式 path 挂载治理、权限、集成等文件，它们是同一个 Workspace 的扩展实现。单元测试分散在对应模块，history_tests.rs 是单独的测试模块；测试阅读入口见本包测试专题。

## 专题阅读

- [HTTP 入口、连接认证与事件流](http-and-security.md)
- [多项目装配、生命周期和 GitHub 工程](workspace-and-lifecycle.md)
- [治理、工具、模型和权限的 RPC 路由](rpc-and-configuration.md)
- [Host 测试与跨库隔离](testing.md)

## 完整文件索引

每个文件只有一个主要讲解位置；相关专题通过链接引用它。

| 文件 | 职责与实现说明 |
|---|---|
| [Cargo.toml](../../../crates/host/Cargo.toml) | [HTTP 可执行包与 Web 嵌入依赖](overview.md#file-cargo-toml) |
| [src/github_projects.rs](../../../crates/host/src/github_projects.rs) | [GitHub 项目身份与异步仓库准备](workspace-and-lifecycle.md#file-src-github-projects-rs) |
| [src/governance.rs](../../../crates/host/src/governance.rs) | [治理路由和步骤执行的事务外编排](rpc-and-configuration.md#file-src-governance-rs) |
| [src/history_tests.rs](../../../crates/host/src/history_tests.rs) | [全局治理与多个工程的组合历史隔离](testing.md#file-src-history-tests-rs) |
| [src/integrations.rs](../../../crates/host/src/integrations.rs) | [工具配置、实际调用和 OAuth callback 装配](rpc-and-configuration.md#file-src-integrations-rs) |
| [src/main.rs](../../../crates/host/src/main.rs) | [服务启动、请求围栏与固定作用域 SSE](http-and-security.md#file-src-main-rs) |
| [src/model_providers.rs](../../../crates/host/src/model_providers.rs) | [当前模型目录、凭据设置和冻结选择的区别](rpc-and-configuration.md#file-src-model-providers-rs) |
| [src/permission_resources.rs](../../../crates/host/src/permission_resources.rs) | [权限目录、精确绑定与后继会话发送](rpc-and-configuration.md#file-src-permission-resources-rs) |
| [src/runtime_setup.rs](../../../crates/host/src/runtime_setup.rs) | [执行安装端口的薄装配层](workspace-and-lifecycle.md#file-src-runtime-setup-rs) |
| [src/templates.rs](../../../crates/host/src/templates.rs) | [模板重置预演、目录代次与旧引用保留](rpc-and-configuration.md#file-src-templates-rs) |
| [src/workspace.rs](../../../crates/host/src/workspace.rs) | [全局与工程 Engine 装配、请求选择和共享配额](workspace-and-lifecycle.md#file-src-workspace-rs) |

<a id="file-cargo-toml"></a>

## Cargo.toml：HTTP 可执行包与 Web 嵌入依赖

源码：[crates/host/Cargo.toml](../../../crates/host/Cargo.toml)。

Host 直接依赖 domain、core 和 adapters：读取领域合同，调用应用用例，并装配具体端口。axum/tokio/futures-util 提供 HTTP 与异步服务，rust-embed/mime_guess 提供前端静态资源，url/base64 等处理连接参数；tempfile 用于模块测试。

这是装配根，允许同时看见抽象和具体实现，但不应把工程状态机复制进 HTTP handler。Web 构建产物由 rust-embed 编入程序，修改界面后需重建嵌入资源并重启 Host。

关联阅读：[host/src/main.rs](http-and-security.md#file-src-main-rs)、[host/src/workspace.rs](workspace-and-lifecycle.md#file-src-workspace-rs)、[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
