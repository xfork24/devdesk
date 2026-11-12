# DevDesk 0.2

本地软件工程工作台原型。Rust 内核实现全部领域规则与权威终态，React Web 提供完整操作入口，SQLite 分库真实保存配置、工程事实、文档与历史。Web 统一接入 GitHub 仓库，使用 Codex / DeepSeek Harness、Git worktree、Docker 隔离测试和 GitHub PR（只创建不合入）。打开项目时自动登记并克隆仓库，执行时按需准备所选 Harness；运行路径只接受当前协议与配置，离线测试使用独立夹具。账号授权在设置中完成，操作流程见[真实工程接入](docs/real-engineering.md)。

一切配置为不可变源修订（源协议 v2，引用形如 `identity:revision`）。Workflow v6 通过显式端口传递材料：Stage 提供背景与默认 Role，Step 声明任务指令、必需 Skill、验收 Spec、工作上下文与完成条件；Agent、工具、子流程、控制是四种执行方式。执行授权独立归属 Permission（v2 Profile 与按流程步骤授权的 Context）。运行冻结确切修订，补充输入、重试、返工与人工确认都关联实际 StepExecution；测试与候选只接受真实回执，模型自述不构成证据。项目经理 Agent 通过独立编排会话与原生控制工具决定派发、接纳与完成，用户可随时人工接管，运行中的计划按"预演—等待静止—原子应用"修改。Standard 是 Workflow、Delivery、Spec 及关联的不可变快照，每个工作采用自己的规范。运行语义见[工作流与编排](docs/workflow-orchestration.md)，领域概念见[领域规范](docs/domain.md)。

## 运行

开发构建需要 Rust **1.99.1**（rustup 按 `rust-toolchain.toml` 安装）、C 编译工具链、Node **24** 和 npm。SQLite 随程序构建，无需数据库服务器。macOS 首次真实 DSH 开发构建需先按 [系统凭据说明](docs/system-credentials.md) 配置本地签名身份；Linux 使用 Secret Service。

```bash
./scripts/dev/build.sh
python3 scripts/dev/run-host.py target/release/devdesk-host
```

打开终端给出的 `http://127.0.0.1:4317`，在初始设置中选择空目录或已有 DevDesk 数据集。也可指定路径和端口：

```bash
python3 scripts/dev/run-host.py target/release/devdesk-host --data ./data --port 4317
```

Host 程序嵌入 Web 文件，默认只监听 loopback。设置、初始化页和连接失败页均提供"连接内核"，填写 `http://ip:port` 后先验证内核，再打开目标工作台。远程监听通过 `--host IP` 和 `DEVDESK_ACCESS_TOKEN` 显式开启；部署及反向代理配置见[运行包说明](docs/run-binary.md)。Host 校验连接地址对应的 Host/Origin。

生成包含可执行程序、说明和验收文档的本机运行包：

```bash
./scripts/release/package.sh
```

macOS Apple Silicon 输出 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 及 SHA-256 校验文件。解压后执行 `./devdesk-host`，无需 Rust/Node 或源码。跨平台传递当前工作区源码可执行 `./scripts/release/package-source.sh`。操作入口按开发、检查、发布和平台配置分类，见 [脚本索引](scripts/README.md)；测试夹具位于 `tests/fixtures/`。

## 使用与验证

Web 工作台按九个导航区组织：工作总览、执行与交付、角色讨论、项目管理、流程与规范、角色与 Agent、基础组件、历史记录、设置；页面状态写入 URL，操作旅程与刷新模型见 [Web 工作台指南](docs/web-workbench.md)。新项目页提供可直接采用的默认配置：入口 Feature 流程（7 阶段 22 步）与 Task 流程（7 阶段 20 步）、六个职责 Agent（需求、架构、项目经理、开发、测试、评审，各自组合独立 Role、Capability 与 Markdown Skill/Spec，分别产出 PRD、Design Doc、Task Plan、测试、实现与评审）。选择或调整后保存项目配置，再创建 Feature；[默认配置目录](resources/configuration/README.md)列出全部出厂内容。

[真实工程接入](docs/real-engineering.md)说明正常 Web 操作；[历史演示步骤](docs/plans/devdesk-prototype/demo.md)仅记录后端模拟夹具，不再对应当前 Web 入口。默认工程方法由可编辑 Workflow 定义，创建 Task、测试和 Git 接纳使用真实平台工具。

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

检查先构建 Web，再运行领域/存储/契约/集成测试；分别验证 debug/release host 的 HTTP 安全、中文及空格路径、跨进程数据锁、分库隔离与重启，并运行 Chromium Web E2E。release HTTP 检查将程序复制到临时空目录运行。浏览器安装只用于测试，运行产品不需要。真实验收探针见 [测试索引](tests/README.md) 与 `scripts/check/check-live.sh`。

开发时使用 `./scripts/dev/dev.sh --data ./data`，自动构建 Web、复用凭据助手并为 macOS Host 签名；Web 更改后需重新构建并重启 host，保持同源鉴权，`vite` 独立开发服务不是成品运行依赖。

## 文档

- 现行系统说明：[领域规范](docs/domain.md)、[协议](docs/protocol.md)、[架构与设计](docs/design.md)、[工作流与编排](docs/workflow-orchestration.md)、[运行时权限](docs/permissions.md)、[存储与数据布局](docs/storage.md)、[讨论与经验](docs/conversations.md)、[Web 工作台指南](docs/web-workbench.md)、[源配置与执行解析](docs/source-configuration.md)、[真实工程接入](docs/real-engineering.md)、[适配器契约](docs/real-adapter-contract.md)、[运行包说明](docs/run-binary.md)、[系统凭据](docs/system-credentials.md)、[工具与技能接入](docs/tool-skill-integrations.md)、[模型接入](docs/model-providers.md)。
- 结构定义：[Schema 索引](schemas/README.md)（含 [RPC 协议](schemas/rpc.schema.json)、[会话快照清单](schemas/manifest.schema.json)）。
- 历史计划与验收：[计划与验收索引](docs/plans/README.md)。历史验收记录归档于对应计划目录，例如 [权限改造验收](docs/plans/permission-standardization/acceptance-20261110.md)、[0.2 原型验收](docs/plans/devdesk-prototype/acceptance.md)、[Ubuntu Web 验收](docs/plans/ubuntu-web-acceptance/ACCEPTANCE.md)。

## 数据与存储

数据集以 `.devdesk-dataset` 标记（内容 `devdesk-source-v2-standard-v1`）并施加独占写锁；非空非 DevDesk 目录不初始化，更换 data 路径需退出重启，不隐式搬迁。全局配置、每项目工程数据与全局讨论分别使用独立 SQLite 文件：

```text
<data>/global.sqlite3                             # 并发配置、共享定义、项目索引、运行时安装、权限预设
<data>/conversations/global/engineering.sqlite3   # 全局讨论（独立命名空间，不创建项目）
<data>/projects/<project-id>/engineering.sqlite3  # 当前项目的工程数据、事件与幂等记录
<data>/projects/<project-id>/artifacts/           # 当前项目的封存产物（SHA-256）
<data>/projects/<project-id>/execution/           # 会话、测试、Git 意图与工具执行目录
```

数据库由 `sql/global/schema.sql` 与 `sql/project/schema.sql` 的当前基线直接创建（global 16 / project 47）；只支持新数据集，不提供旧库升级。选择项目即打开对应数据库，重启恢复上次选择；已打开项目的执行继续受全局并发额度约束。表结构与幂等/恢复模型见 [存储与数据布局](docs/storage.md) 和 [结构说明](sql/README.md)。

## 关键边界

- 执行权限只使用 v2 完整 Profile 与版本化绑定，支持执行前预览；内置 Agent 默认追随 Harness/Sandbox 的容器内全权限绑定，步骤级读写与工具授权由 Permission Context 声明，见 [运行时权限](docs/permissions.md) 与 [权限说明](permissions/README.md)。
- 编排（项目经理）当前支持 Codex 原生工具桥；DSH 在补齐工具桥前不能承担编排会话。
- 模型经 Codex（OpenAI）或 DeepSeek Harness 接入 DeepSeek、智谱国内编程订阅、MiniMax 国内订阅与本机 Ollama，见 [模型接入](docs/model-providers.md)。
- macOS 稳定签名与凭据助手见 [系统凭据](docs/system-credentials.md)；Linux 使用 Secret Service。macOS 构建前需安装 Xcode Command Line Tools（`xcode-select --install`），两平台使用相同命令。
- 平台实机验证（Ubuntu x86_64 与 macOS Apple Silicon）证据见 [计划与验收索引](docs/plans/README.md)；macOS native 客户端不在当前范围。
