# Host 测试与跨库隔离

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

`cargo test -p devdesk-host --locked` 运行连接参数、项目路由、治理、权限和模板等模块内测试。它们使用独立临时数据集并可注入执行端口，不启动付费模型。

连接 origin/token 校验见 [HTTP 入口](http-and-security.md#file-src-main-rs)；工程切换、inactive 调度与停机见 [Workspace](workspace-and-lifecycle.md#file-src-workspace-rs)；源修订与实例历史见 [治理路由](rpc-and-configuration.md#file-src-governance-rs)；权限后继会话与单目录恢复见 [权限 API](rpc-and-configuration.md#file-src-permission-resources-rs)；模板代次和回滚见 [模板路由](rpc-and-configuration.md#file-src-templates-rs)。本页独立文件专门检验组合历史的数据库归属。

真实 HTTP 进程、数据集入口和浏览器交互另由 [跨进程入口测试](../../../tests/entrypoints) 与 [浏览器测试](../../../web/tests) 覆盖；模块测试通过不能替代这两层，也不表示外部 Docker/GitHub 已验收。

## 本页文件

- [src/history_tests.rs](#file-src-history-tests-rs)：全局讨论与多个工程的组合历史隔离

<a id="file-src-history-tests-rs"></a>

## src/history_tests.rs：全局讨论与多个工程的组合历史隔离

源码：[crates/host/src/history_tests.rs](../../../crates/host/src/history_tests.rs)。

此文件仅在 cfg(test) 下挂载。add_history 在指定 Engine 建立会话/快照事实，主场景为 global、项目 A 和项目 B 分别写入记录，再从 Workspace 路由组合历史查询。

断言每个查询只读取其正确数据库，即使切换当前项目也不泄露其他工程事实。它防止为实现“统一历史界面”而把所有记录塞进全局库，也检查底层源 API 使用固定项目上下文。

代码定位：[`add_history`](../../../crates/host/src/history_tests.rs#L3)、[`combined_history_keeps_global_and_each_project_in_their_own_database`](../../../crates/host/src/history_tests.rs#L25)。

关联阅读：[core/src/history.rs](../devdesk-core/conversations-and-history.md#file-src-history-rs)、[host/src/workspace.rs](workspace-and-lifecycle.md#file-src-workspace-rs)、[adapters/tests/conversations.rs](../devdesk-adapters/testing.md#file-tests-conversations-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
