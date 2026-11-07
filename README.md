# DevDesk 0.2

本地软件工程工作台原型。Rust 实现领域规则，React Web 提供完整操作入口；SQLite、文档、报告与历史真实保存。Git、Agent、项目测试和环境执行均为 **simulation**，不修改用户仓库，不调用模型服务。

## 运行

开发构建需要 Rust **1.99.1**（rustup 按 `rust-toolchain.toml` 安装）、C 编译工具链、Node **24** 和 npm。SQLite 随程序构建，无需数据库服务器。

```bash
./scripts/build.sh
./target/release/devdesk-host
```

打开终端给出的 `http://127.0.0.1:4317`，在初始设置中选择空目录或已有 DevDesk 数据集。也可指定路径和端口：

```bash
./target/release/devdesk-host --data ./data --port 4317
```

构建后的单个程序嵌入 Web 文件，运行时不需要 Node、开发服务器或源码目录。只监听 loopback。必须使用打印的 `127.0.0.1` 地址，Host 校验会拒绝其他主机名。

Linux Ubuntu 24.04 x86_64 已实际运行。macOS Apple Silicon 使用同一构建命令，需先安装 Xcode Command Line Tools；**macOS 实机验证 pending**，尚未完成全平台 F5 验收。

## 使用与验证

[演示步骤](docs/demo.md)说明从需求到模拟 mainline 的操作和异常场景。需要先完成规划的两个 Delivery/Review，再建立独立 Tasks；Task 完成后还有 Feature 自身收尾。

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check.sh
```

检查包括领域/存储/契约/集成测试、真实 host HTTP 安全和重启检查，以及 Chromium 驱动真实 Rust 后端的 Web E2E。浏览器安装只用于测试，运行产品不需要。

开发时先 `npm --prefix web run build`，再 `cargo run -p devdesk-host -- --data ./data`。Web 更改后需重新构建并重启 host，保持同源鉴权；`vite` 独立开发服务不是成品运行依赖。

## 文档

- [修订后的实施计划](plans/prototype-v0.2-plan.md)
- [领域规范](docs/domain.md)、[协议](docs/protocol.md)、[架构与 ADR](docs/design.md)
- [验收与限制](docs/acceptance.md)、[阶段报告](docs/reports/README.md)
- [协议 JSON Schema](schemas/rpc.schema.json)、[文件清单 Schema](schemas/manifest.schema.json)

数据集以 `.devdesk-dataset` 标记，使用独占写锁；非空非 DevDesk 目录不初始化。更换 data 路径需退出并重新启动，不隐式迁移。Archive 与迁移仅交互预览。所有 reset/删除均不作为正常升级或恢复手段，本原型不提供任意路径删除功能。
