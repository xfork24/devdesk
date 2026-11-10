# DevDesk 0.2

本地软件工程工作台原型。Rust 实现领域规则，React Web 提供完整操作入口；SQLite、文档、报告与历史真实保存。Web 统一接入 GitHub 仓库，使用 Codex / DeepSeek Harness、Git worktree、Docker 测试和 GitHub PR。打开项目时自动登记并克隆仓库，执行时按需准备所选 Harness；运行路径只接受当前协议与配置，离线测试使用独立夹具。账号授权在设置中完成，见[真实工程接入](docs/real-engineering.md)。

流程与规范的 Delivery 页可定义 PRD、Design Doc、Diff、Test Report 等交付物。Workflow v3 步骤可声明产出及引用依赖链上的确切交付物修订，定义与材料一起进入冻结任务。角色与 Agent 仅保留 Role、Capability、Agent 三个页签，Agent 用于组合职责与能力；讨论身份可在角色讨论中创建。同一 Task 中，同一 Agent 跨步骤和执行节点续用同一个 Session 与原生 thread，仅显式新建时切换到新会话；每轮独立冻结节点任务和执行权限。Feature 级讨论继续按步骤隔离。

## 运行

开发构建需要 Rust **1.99.1**（rustup 按 `rust-toolchain.toml` 安装）、C 编译工具链、Node **24** 和 npm。SQLite 随程序构建，无需数据库服务器。macOS 首次真实 DSH 开发构建需先按 [系统凭据说明](docs/system-credentials.md) 配置本地签名身份；Linux 使用 Secret Service。

```bash
./scripts/dev/build.sh
./target/release/devdesk-host
```

打开终端给出的 `http://127.0.0.1:4317`，在初始设置中选择空目录或已有 DevDesk 数据集。也可指定路径和端口：

```bash
./target/release/devdesk-host --data ./data --port 4317
```

Host 程序嵌入 Web 文件；macOS 的真实 DSH 凭据另使用运行包内的一份共享助手，运行时不需要 Node、开发服务器或源码目录。默认只监听 loopback。设置、初始化页和连接失败页均提供“连接内核”，填写 `http://ip:port` 后先验证内核，再打开目标工作台。远程监听通过 `--host IP` 和 `DEVDESK_ACCESS_TOKEN` 显式开启；部署及反向代理配置见[运行包说明](docs/run-binary.md)。Host 校验连接地址对应的 Host/Origin。

原型曾在 Linux Ubuntu 24.04 x86_64 与 macOS Apple Silicon 上完成后端/Web 验收；早期真实工程接入已在 macOS 通过，Ubuntu 24.04.5 / x86_64 也已通过双 Harness Web 工程闭环、Linux 系统凭据、安装复用、取消/崩溃恢复和独立运行包验收，AppArmor Docker 宿主使用经批准的专用策略；见 [Ubuntu 续验报告](docs/plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。macOS 构建前需安装 Xcode Command Line Tools（`xcode-select --install`）；两平台使用相同命令。平台证据与限制见[验收记录](docs/acceptance.md)。

生成包含可执行程序、说明和验收文档的本机运行包：

```bash
./scripts/release/package.sh
```

macOS Apple Silicon 输出 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 及 SHA-256 校验文件。解压后执行 `./devdesk-host`，无需 Rust/Node 或源码；详见[运行包说明](docs/run-binary.md)。

跨平台传递当前工作区源码可执行 `./scripts/release/package-source.sh`，输出 `dist/devdesk-worktree-source.tar.gz`、源码清单和校验文件。操作入口按开发、检查、发布和平台配置分类，见 [脚本索引](scripts/README.md)；测试夹具位于 `tests/fixtures/`。

## 使用与验证

配置入口分为项目管理、流程与规范、角色与 Agent、基础组件。流程与规范包含 Workflow、Delivery、Spec、Standard；基础组件依次包含 Model、Harness、MCP、Skills、Tools（HTTP API）、Harness原生工具。工具调用记录位于历史记录的 Tools调用记录范围。本轮变更、状态与验收见[改进计划](docs/plans/settings-navigation-usability/PLAN.md)及[实施报告](docs/plans/settings-navigation-usability/REPORT.md)。新项目页提供可直接采用的默认流程与六个职责 Agent；各职责引用独立 Capability 和 Markdown Skill/Spec，分别产出 PRD、Design Doc、Task Plan、测试、实现与评审。选择或调整后保存项目配置，再创建 Feature；Standard 为可选覆盖基线。[默认配置目录](resources/configuration/README.md)列出具体内容。

[真实工程接入](docs/real-engineering.md)说明正常 Web 操作；[历史演示步骤](docs/plans/devdesk-prototype/demo.md)仅记录后端模拟夹具，不再对应当前 Web 入口。需要先完成规划的两个 Delivery/Review，再建立独立 Tasks；Task 完成后还有 Feature 自身收尾。

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

检查先构建 Web，再运行领域/存储/契约/集成测试；分别验证 debug/release host 的 HTTP 安全、中文及空格路径、跨进程数据锁、分库隔离与重启，并运行 Chromium Web E2E。release HTTP 检查将程序复制到临时空目录运行。浏览器安装只用于测试，运行产品不需要。

测试目录、两个 Python 离线回归入口和真实验收探针的职责见 [测试索引](tests/README.md)。

开发时使用 `./scripts/dev/dev.sh --data ./data`，自动构建 Web、复用凭据助手并为 macOS Host 签名。Web 更改后需重新构建并重启 host，保持同源鉴权；`vite` 独立开发服务不是成品运行依赖。

## 文档

- [当前架构与实现基线 v0.4](docs/plans/devdesk-v0.4/PLAN.md)、[基线核对与验证记录](docs/plans/devdesk-v0.4/EXECUTION.md)
- [历史架构与实现基线 v0.3](docs/plans/devdesk-v0.3/PLAN.md)、[Workflow 与工作台执行记录](docs/plans/workflow-and-workbench/EXECUTION.md)
- [历史架构设计 v0.2](docs/plans/devdesk-v0.2/PLAN.md)、[原型实施计划](docs/plans/devdesk-prototype/PLAN.md)
- [领域规范](docs/domain.md)、[协议](docs/protocol.md)、[架构与 ADR](docs/design.md)
- [真实工程接入与副作用边界](docs/real-engineering.md)、[接入实施计划](docs/plans/real-engineering-integration/PLAN.md)
- [验收与限制](docs/acceptance.md)、[计划与验收索引](docs/plans/README.md)
- [协议 JSON Schema](schemas/rpc.schema.json)、[文件清单 Schema](schemas/manifest.schema.json)

数据集以 `.devdesk-dataset` 标记，使用独占写锁；非空非 DevDesk 目录不初始化。更换 data 路径需退出并重新启动，不隐式搬迁。所有 reset/删除均不作为正常升级或恢复手段，本原型不提供任意路径删除功能。

全局配置与项目工程数据分别使用 SQLite 文件：

```text
<data>/global.sqlite3                            # 并发配置、共享定义、项目索引、上次打开的项目
<data>/projects/<project-id>/engineering.sqlite3  # 当前项目的工程数据、事件与幂等记录
<data>/projects/<project-id>/artifacts/           # 当前项目的文档、报告与其他产物
```

数据库由 `sql/global/schema.sql` 与 `sql/project/schema.sql` 的当前基线直接创建；见 [结构说明](sql/README.md)。

在“当前项目”中选择项目会打开对应数据库；重启恢复上次选择。已打开项目的执行继续受全局并发额度约束。系统只支持当前分库布局，已有数据需显式迁移到当前结构后打开。

本轮领域补强的计划见 [补强计划](docs/plans/domain-hardening/PLAN.md)，验收与后续真实 Harness/GitHub/Docker 接入边界见 [补强验收](docs/plans/domain-hardening/domain-hardening.md) 和 [适配器契约](docs/real-adapter-contract.md)。

系统凭据与 macOS 稳定签名见 [凭据说明](docs/system-credentials.md)；Linux Secret Service、平台预检与真实 Web 闭环见 [Ubuntu 验收入口](docs/ubuntu-web-acceptance.md) 和 [Ubuntu 验收计划](docs/plans/ubuntu-web-acceptance/PLAN.md)。本轮聚焦 Web，macOS native 客户端后续再做。

开发 Web Host 可用 `./scripts/dev/dev.sh --data /完整路径/数据集 --port 4317`；它会在 macOS 重建后应用已配置的稳定签名。

运行权限只使用 v2 完整 profile 和版本绑定，支持执行前预览。内置 Agent 默认追随 Harness/Sandbox 的容器内全权限绑定；见 [权限说明](permissions/README.md) 和 [实施报告](docs/plans/permission-standardization/EXECUTION.md)。

2026-11-09 两项新能力的 Ubuntu x86_64 实机验证分别见 [Permission 补验](docs/plans/permission-standardization/UBUNTU.md) 和 [Workflow 与工作台补验](docs/plans/workflow-and-workbench/UBUNTU.md)，包含当前源码回归、双 Harness 原生执行、Linux 隔离与发布包证据。
