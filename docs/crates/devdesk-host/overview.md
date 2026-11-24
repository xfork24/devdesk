# devdesk-host：服务入口与调度

Host 将应用装配成可运行的服务：打开数据集、选择项目 Engine、提供 HTTP 接口，并驱动后台阶段。业务修改仍通过 core，用数据库或 Docker 的底层能力绕过 core 会破坏状态检查。

| 专题 | 主要入口 |
|---|---|
| [启动、装配与生命周期](workspace-and-lifecycle.md) | main.rs、workspace.rs、lifecycle.rs |
| [HTTP、RPC 与事件](http-and-security.md) | main.rs、event_hub.rs、各功能请求模块 |
| [后台调度](background-scheduling.md) | Workspace::tick、background.rs |
| [领域生命周期及客户端贡献](../../design/domain-packages.md) | domains.rs、workspace.rs |

Web 通过协议调用 Host。Host 没有所有浏览器共用的 active project；每个工程请求明确指定项目。跨项目维护和调度由 Host 负责，但单个项目的业务规则由其 Engine 执行。

源码入口：[main.rs](../../../crates/host/src/main.rs)。全量查找：[files.md](files.md)。系统位置：[总览](../../design/overview.md)。
