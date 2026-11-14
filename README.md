# DevDesk

DevDesk 是本地软件工程工作台：Rust 内核管理配置、工作流和权威执行事实，React Web 提供操作入口，SQLite 按项目保存工程数据与历史。它通过 GitHub 仓库、Git worktree、Linux Docker worker 和 Codex / DeepSeek Harness 执行实际工作；平台发布只创建 GitHub PR，不自动合入。

工作方法由不可变配置源组合：Role 定义职责，Skill 定义方法，Step 定义任务，Spec 定义检查，Delivery 定义交付合同，Standard 固定规范闭包。项目和具体工作显式采用确切修订，运行冻结输入、权限、资源与接入选择；修改默认配置不会覆写已有运行。

## 启动

源码构建需要 Rust **1.99.1**（由 `rust-toolchain.toml` 指定）、Node **24**、npm、Python 3 与 C 编译工具链。macOS 首次构建先按 [系统凭据](docs/system-credentials.md) 配置签名身份。运行工程还需已启动的 Docker、Git 和已登录的 GitHub CLI。

```bash
./scripts/dev/build.sh
python3 scripts/dev/run-host.py target/release/devdesk-host --data ./data --port 4317
```

在浏览器打开终端显示的地址，默认 `http://127.0.0.1:4317`。省略 `--data` 时由初始化页选择数据目录。开发入口会构建 Web 与 debug Host，并在 macOS 使用独立签名副本运行：

```bash
./scripts/dev/dev.sh --data ./data --port 4317
```

Web 和出厂资源嵌入 Host，界面更新后重新构建并重启。发行包由以下命令生成，解压后运行 `./devdesk-host`，无需 Rust、宿主 Node 或源码：

```bash
./scripts/release/package.sh
```

输出为 `dist/devdesk-0.2.0-<rust-host-target>.tar.gz` 及 SHA-256 文件。远程内核连接、访问密钥、Linux 凭据与 AppArmor 设置见 [运行包说明](docs/run-binary.md)。源码分发入口为 `./scripts/release/package-source.sh`。

## 工程流程

1. 在 Web “打开 GitHub 项目”，填写仓库和主分支，等待克隆及工作空间登记完成。
2. 在设置中完成模型认证；在项目管理采用 Workflow、Standard、Role→Agent、Permission Context 与驱动选择。运行资源按确切配置准备，准备探针不调用模型。
3. 创建 Feature，选择人工逐步、Rust 有界调度或项目经理 Agent 调度。三者使用相同的派发、接纳、完成及证据规则；Rust 模式必须选择精确调度策略。
4. 在“执行与交付”查看步骤、原始产物、真实测试、独立评审及候选。运行计划变更经过预演、静止等待和原子应用；未知副作用先核对。
5. 核对候选、测试、评审、业务接纳与远端发布回执，取得 PR。模型自述不能代替 Docker 测试、Git 事实或人类签署。

默认 Feature 为 7 阶段 22 步，Task 为 7 阶段 20 步，覆盖需求、设计、任务规划、独立测试、Red/Green、实现、文档、评审、候选与发布。出厂步骤确认均为 automatic，仍须满足业务门禁；显式 human 策略及人工例外单独记录。完整流程、预算和授权见 [工作流与编排](docs/workflow-orchestration.md) 与 [默认配置](resources/configuration/README.md)。

项目经理独立工作台可在创建 Feature 前查询资料、预演和创作配置；它与根实例调度权分开。当前 Codex 提供平台/编排原生工具桥，DSH 不提供该桥。DSH 可按源配置连接 DeepSeek、智谱国内编程订阅、MiniMax 国内订阅及本机 Ollama；目录结果不代表真实推理已验证。

## 数据与执行边界

```text
<data>/global.sqlite3                              # 共享源配置、项目目录、运行安装与准备记录
<data>/conversations/global/engineering.sqlite3    # 全局讨论，独立命名空间
<data>/projects/<id>/engineering.sqlite3           # 单项目工程事实与执行历史
<data>/projects/<id>/artifacts/                    # 摘要封存产物
<data>/projects/<id>/repository/                   # 受管理仓库克隆
<data>/projects/<id>/execution/                    # 会话、测试、工具、Git 意图与工作副本
```

数据集标记为 `devdesk-source-flat-v1`，当前 SQLite 基线为 **global 18 / project 48**，同一数据集只有一个可写 Host。不兼容旧数据集直接拒绝，不自动转换、删库或重建；兼容数据集可重开，验证新代码使用独立空目录。

配置唯一存储在 `source_definitions`，执行视图按原源引用恢复，不存第二份展开闭包。权限与能力分开：Agent 只引用 Role/Capability，Permission Context 提供步骤授权，Profile 落实 Harness/Sandbox 策略。真实 worker 在 Linux 容器运行，宿主登录组件不作为工程执行环境。

## 开发与验证

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

检查入口执行权限资产、出厂目录、架构、Web 构建、Rust、HTTP/数据集合同及 debug/release Chromium。默认回归不调用真实付费模型或 GitHub。真实 Git/Docker、模型工程和 Standard 验收使用各自专用入口、独立数据集及明确指定的仓库，见 [测试索引](tests/README.md) 和 [真实工程接入](docs/real-engineering.md)。

仓库职责与改动约束见 [AGENT.md](AGENT.md)，构建/检查/打包命令见 [脚本索引](scripts/README.md)。

## 当前文档

| 主题 | 文档 |
|---|---|
| 概念与结构 | [领域规范](docs/domain.md)、[结构合同](docs/contracts.md)、[架构设计](docs/design.md) |
| 流程与规范 | [工作流与编排](docs/workflow-orchestration.md)、[源配置与执行解析](docs/source-configuration.md) |
| 操作与会话 | [Web 工作台](docs/web-workbench.md)、[讨论与经验](docs/conversations.md)、[真实工程接入](docs/real-engineering.md) |
| 执行与接入 | [适配器契约](docs/real-adapter-contract.md)、[工具与技能](docs/tool-skill-integrations.md)、[模型服务商](docs/model-providers.md) |
| 权限与部署 | [运行时权限](docs/permissions.md)、[系统凭据](docs/system-credentials.md)、[运行包说明](docs/run-binary.md) |
| 协议与存储 | [本地协议](docs/protocol.md)、[存储布局](docs/storage.md)、[Schema](schemas/README.md)、[SQL](sql/README.md) |

`docs/` 根目录 Markdown 描述当前实现；[docs/plans/](docs/plans/README.md) 保存历史计划、合同和验收记录，原始路径、SHA、测试结果只属于当时执行，不保证临时产物仍存在。
