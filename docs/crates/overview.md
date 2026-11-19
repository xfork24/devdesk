# 仓库地图

应用从 `crates/host/src/main.rs` 启动。Host 创建具体适配器，将它们注入 core 的 Engine，再提供 HTTP 服务和后台调度。domain 提供各层共享的业务对象和纯规则。

## Rust 包

| 包 | 入口 | 读代码时关注什么 |
|---|---|---|
| devdesk-domain | [lib.rs](../../crates/domain/src/lib.rs) | 类型关系、校验和规则求值；不执行 I/O |
| devdesk-core | [lib.rs](../../crates/core/src/lib.rs)、[ports.rs](../../crates/core/src/ports.rs) | Engine、命令/查询、状态变化和外部能力接口 |
| devdesk-adapters | [lib.rs](../../crates/adapters/src/lib.rs) | SqliteStore、FileArtifacts、execution_adapters 及外部协议 |
| devdesk-host | [main.rs](../../crates/host/src/main.rs)、[workspace.rs](../../crates/host/src/workspace.rs) | HTTP、生命周期、项目路由、装配和调度 |
| devdesk-tools | [Cargo.toml](../../crates/tools/Cargo.toml) | 独立 bin；当前是 macOS 凭据助手 |

编译依赖是 host 使用 adapters/core/domain，adapters 使用 core/domain，core 只依赖 domain。运行时 Engine 通过 trait 调用具体适配器，详细解释见[分层设计](../design/layers.md)。

## Rust 以外的目录

| 目录 | 与运行的关系 |
|---|---|
| [web](../../web) | React/TypeScript 工作台；构建产物嵌入 Host |
| [configurations](../../configurations/README.md) | manifest 发布的源定义、正文、安装资源和默认选择 |
| [crates/adapters/runtime](../../crates/adapters/runtime) | 适配器使用的工具 worker；当前 HTTP/MCP 实现在 tool-worker.mjs |
| [schemas](../../schemas/README.md) | 数据形状及协议合同 |
| [sql](../../sql/README.md) | 当前数据库基线和结构 |
| [permissions](../../permissions/README.md) | 实际权限后端资产 |
| [containers](../../containers/README.md) | 测试等容器输入；Harness 执行配方由配置组合生成 |
| [tests](../../tests/README.md) | 跨进程合同、原生协议探针和共享支持 |
| [scripts](../../scripts/README.md) | 开发、检查、平台安装与发布入口 |

## 按问题定位

“谁有权执行”先看 core；“Docker 实际执行了什么”看 adapters；“请求进入了哪个项目”看 host；“这个配置结构是否合法”看 domain 和 Schema。不要从文件名的相似性推断责任。

先跟踪[完整调用链](execution-flows.md)，再展开相关专题。全量文件表在每个包的 files.md，概览不承担逐文件复述。
