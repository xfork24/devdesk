# DevDesk 0.2

本地软件工程工作台原型。Rust 实现领域规则，React Web 提供完整操作入口；SQLite、文档、报告与历史真实保存。Web 统一接入 GitHub 仓库，使用 Codex / DeepSeek Harness、Git worktree、Docker 测试和 GitHub PR。打开项目时自动登记并克隆仓库，执行时按需准备所选 Harness；后端保留模拟数据和离线夹具。账号授权在设置中完成；见 [真实工程接入](docs/real-engineering.md)。

## 运行

开发构建需要 Rust **1.99.1**（rustup 按 `rust-toolchain.toml` 安装）、C 编译工具链、Node **24** 和 npm。SQLite 随程序构建，无需数据库服务器。macOS 首次真实 DSH 开发构建需先按 [系统凭据说明](docs/system-credentials.md) 配置本地签名身份；Linux 使用 Secret Service。

```bash
./scripts/build.sh
./target/release/devdesk-host
```

打开终端给出的 `http://127.0.0.1:4317`，在初始设置中选择空目录或已有 DevDesk 数据集。也可指定路径和端口：

```bash
./target/release/devdesk-host --data ./data --port 4317
```

Host 程序嵌入 Web 文件；macOS 的真实 DSH 凭据另使用运行包内的一份共享助手，运行时不需要 Node、开发服务器或源码目录。只监听 loopback。必须使用打印的 `127.0.0.1` 地址，Host 校验会拒绝其他主机名。

原型曾在 Linux Ubuntu 24.04 x86_64 与 macOS Apple Silicon 上完成后端/Web 验收；本轮真实工程接入已在 macOS 通过，Ubuntu 24.04.5 / x86_64 也已通过双 Harness Web 工程闭环、Linux 系统凭据、安装复用、取消/崩溃恢复和独立运行包验收，AppArmor Docker 宿主使用经批准的专用策略；见 [Ubuntu 续验报告](docs/plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。macOS 构建前需安装 Xcode Command Line Tools（`xcode-select --install`）；两平台使用相同命令。平台证据与限制见[验收记录](docs/acceptance.md)。

生成包含可执行程序、说明和验收文档的本机运行包：

```bash
./scripts/package.sh
```

macOS Apple Silicon 输出 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 及 SHA-256 校验文件。解压后执行 `./devdesk-host`，无需 Rust/Node 或源码；详见[运行包说明](docs/run-binary.md)。

跨平台传递当前工作区源码可执行 `./scripts/package-source.sh`，输出 `dist/devdesk-worktree-source.tar.gz`、源码清单和校验文件。操作入口统一位于 `scripts/*.sh`，Python 辅助实现位于 `scripts/py/`；测试夹具位于 `tests/fixtures/`。

## 使用与验证

[真实工程接入](docs/real-engineering.md)说明正常 Web 操作；[历史演示步骤](docs/demo.md)仅记录后端模拟夹具，不再对应当前 Web 入口。需要先完成规划的两个 Delivery/Review，再建立独立 Tasks；Task 完成后还有 Feature 自身收尾。

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check.sh
```

检查先构建 Web，再运行领域/存储/契约/集成测试；分别验证 debug/release host 的 HTTP 安全、中文及空格路径、跨进程数据锁、分库隔离与重启，并运行 Chromium Web E2E。release HTTP 检查将程序复制到临时空目录运行。浏览器安装只用于测试，运行产品不需要。

开发时使用 `./scripts/dev.sh --data ./data`，自动构建 Web、复用凭据助手并为 macOS Host 签名。Web 更改后需重新构建并重启 host，保持同源鉴权；`vite` 独立开发服务不是成品运行依赖。

## 文档

- [架构设计 v0.2](docs/plans/devdesk-v0.2/PLAN.md)、[修订后的实施计划](docs/plans/devdesk-prototype/PLAN.md)
- [领域规范](docs/domain.md)、[协议](docs/protocol.md)、[架构与 ADR](docs/design.md)
- [真实工程接入与副作用边界](docs/real-engineering.md)、[接入实施计划](docs/plans/real-engineering-integration/PLAN.md)
- [验收与限制](docs/acceptance.md)、[计划与验收索引](docs/plans/README.md)
- [协议 JSON Schema](schemas/rpc.schema.json)、[文件清单 Schema](schemas/manifest.schema.json)

数据集以 `.devdesk-dataset` 标记，使用独占写锁；非空非 DevDesk 目录不初始化。更换 data 路径需退出并重新启动，不隐式搬迁。Archive 导出与 data 目录搬迁仅交互预览。所有 reset/删除均不作为正常升级或恢复手段，本原型不提供任意路径删除功能。

全局配置与项目工程数据分别使用 SQLite 文件：

```text
<data>/global.sqlite3                            # 并发配置、共享定义、项目索引、上次打开的项目
<data>/projects/<project-id>/engineering.sqlite3  # 当前项目的工程数据、事件与幂等记录
<data>/projects/<project-id>/artifacts/           # 当前项目的文档、报告与其他产物
```

数据库初始化与结构升级脚本存放于 `sql/`，其中 `sql/global/` 对应全局配置库；脚本在编译时嵌入程序。

在“当前项目”中选择项目会打开对应数据库；重启恢复上次选择。已打开项目的执行继续受全局并发额度约束。旧版根目录 `engineering.sqlite3` 首次打开时自动拆分，校验产物并保留历史和幂等记录；旧库、旧产物及 `runtime.json` 保留不变，后续只使用新布局。迁移失败可修复原因后重试，不发布不完整的项目目录。详细语义见[架构说明](docs/design.md#adr-010每项目数据库与全局配置库)。

本轮领域补强的计划见 [补强计划](docs/plans/domain-hardening/PLAN.md)，验收与后续真实 Harness/GitHub/Docker 接入边界见 [补强验收](docs/plans/domain-hardening/domain-hardening.md) 和 [适配器契约](docs/real-adapter-contract.md)。

系统凭据与 macOS 稳定签名见 [凭据说明](docs/system-credentials.md)；Linux Secret Service、平台预检与真实 Web 闭环见 [Ubuntu 验收入口](docs/ubuntu-web-acceptance.md) 和 [Ubuntu 验收计划](docs/plans/ubuntu-web-acceptance/PLAN.md)。本轮聚焦 Web，macOS native 客户端后续再做。

开发 Web Host 可用 `./scripts/dev.sh --data /完整路径/数据集 --port 4317`；它会在 macOS 重建后应用已配置的稳定签名。

运行权限采用独立的 Permission 修订，支持 Harness/Sandbox 绑定、覆盖和执行前预览。新配置默认容器内全权限，旧数据保留原绑定；见 [权限说明](permissions/README.md) 和 [实施报告](docs/plans/permission-standardization/EXECUTION.md)。
