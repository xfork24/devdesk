# devdesk-adapters：存储与外部执行

adapters 把 core 的能力请求转换成数据库、文件、进程或远端协议操作，再返回实际结果。它负责执行与技术约束；工作流是否完成、谁可批准等应用判断由 core 负责。

## 按边界阅读

| 专题 | 主要实现 |
|---|---|
| [工厂与配置装载](factory-and-configuration.md) | execution_adapters、factory_bundle、GlobalStore、配置缓存 |
| [存储与产物](storage-and-source-codecs.md) | SqliteStore、资源提交、专用查询、FileArtifacts |
| [Harness 与 Docker](harness-and-protocols.md) | RoutingHarness、JsonlProcess、DockerSandbox |
| [运行环境准备](runtime-preparation.md) | 配方、安装、镜像、权限和协议检查 |
| [工程资源](engineering-resources.md) | CliGit、CommandTests、GitHubPullRequests |
| [凭据与权限](credentials-and-permissions.md) | Keychain/Secret Service、权限编译、平台资产 |
| [外部接入](external-integrations.md) | NativeTools、HTTP/MCP worker、OAuth 和 Skill 导入 |

领域包的处理器在沙箱里运行的是内核的引导模块 [runtime/domain-worker.mjs](../../../crates/adapters/runtime/domain-worker.mjs)。它加载包的入口模块、按名称调用处理器、把处理器的拒绝交回 Host，并回答激活时的握手。协议见[领域包设计](../../design/domain-packages.md#运行端与前端)。

`src/tests.rs` 是生产测试执行器 CommandTests，不能因为文件名而把它当作单元测试文件。跨包行为回归主要在本包 tests 目录，其中许多测试同时覆盖 core。

入口：[lib.rs](../../../crates/adapters/src/lib.rs)。完整文件表：[files.md](files.md)。验证选择见[测试指南](../testing-and-extension.md)。
