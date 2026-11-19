# DevDesk 代码手册

这套文档帮助开发者从功能找到代码，并理解修改会经过哪些层。系统设计和设计原因见[架构文档](../design/README.md)；这里重点说明入口、调用关系、数据变化和测试。

## 推荐阅读顺序

1. [仓库地图](overview.md)：了解各目录和包的责任。
2. [跨模块调用链](execution-flows.md)：跟踪“派发分析师并取得 PRD”等完整过程。
3. 按功能进入下面的包手册；需要查单个文件时再看该包的 files 索引。
4. 修改前查[测试与扩展指南](testing-and-extension.md)和[开发规范](standards/rust.md)。

| 手册 | 主要问题 |
|---|---|
| [domain](devdesk-domain/overview.md) | 对象、配置、规则如何表达？ |
| [core](devdesk-core/overview.md) | 应用流程怎样检查、执行并提交状态？ |
| [adapters](devdesk-adapters/overview.md) | 外部系统和存储怎样实现？ |
| [host](devdesk-host/overview.md) | 服务如何启动、装配、接收请求和调度？ |
| [tools](devdesk-tools/overview.md) | 独立辅助程序如何运行和构建？ |
| [Web 与 Host](web-integration.md) | 页面怎样查询、发送命令并更新？ |

源码链接配合符号名使用。源码移动后更新链接，函数变化后重查调用路径，不把文档中的行号当作稳定接口。
