# DevDesk

**把需求、AI 执行、真实测试和代码交付放进同一个本地工程工作台。**

DevDesk 使用 Rust 内核管理配置与工程事实，React 提供 Web 工作台，SQLite 按项目保存执行历史。它通过 Git worktree、Linux Docker worker、Codex / DeepSeek Harness 和 GitHub 执行实际工程工作。

[快速开始](#快速开始) · [工作方式](#创建第一项工作) · [开发与验证](#开发与验证) · [文档](#文档) · [贡献](#贡献)

## 能做什么

- **配置工程方法**：组合 Role、Skill、Workflow、Spec、Delivery 和 Standard，发布不可变修订，预览后明确采用。
- **执行并跟踪工作**：选择人工逐步、Rust 有界调度或项目经理 Agent；查看步骤、原始产物、实际测试与独立评审。
- **核对代码交付**：绑定确切源版本、目标和测试候选，检查 PR、合入结果与本地同步。默认 Task 自动合入所属 Feature，Feature 合入主分支需要 Human 决定。
- **保留执行依据**：会话冻结模型、权限、输入和环境；历史执行保留原引用，未知副作用先核对，运行计划变更经过预演与停止确认。
- **按项目协作**：每个浏览器标签页独立选择项目，读取并发执行，写入通过资源版本校验防止覆盖。

当前支持 macOS 和 Linux Host，工程 worker 在 Linux Docker 容器执行。Codex 与 DSH 都提供动态工具和经验访问桥；当前根编排驱动支持 Codex。100 个活跃前端是持续容量目标，正式联合验收状态见[并发与性能约束](docs/design.md#单-host-并发与性能设计约束)。

## 快速开始

### 环境要求

| 用途 | 依赖 |
|---|---|
| 源码构建 | Rust **1.99.1**（`rust-toolchain.toml`）、Node **24**、npm、Python **3.10+**、C 编译工具链 |
| 工程执行 | 运行中的 Docker、Git、已认证的 GitHub CLI，以及选定模型服务商的认证 |
| 本机凭据 | macOS 固定签名 Keychain 助手；Linux Secret Service 和可访问的用户 D-Bus |
| 完整回归 | Python Schema 校验依赖、Chromium、Docker 与镜像/包源网络 |

macOS 首次构建先按[系统凭据说明](docs/system-credentials.md)设置固定签名身份。Python 依赖建议安装到虚拟环境：

```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install -r scripts/check/requirements.txt
./scripts/dev/build.sh
python3 scripts/dev/run-host.py target/release/devdesk-host --data ./data --port 4317
```

打开终端显示的地址，默认是 `http://127.0.0.1:4317`。省略 `--data` 时，由初始化页选择数据目录。构建脚本会安装前端依赖、构建 Web 和 release Host。

### 创建第一项工作

1. 在 Web 中“打开 GitHub 项目”，选择专用仓库与主分支，等待克隆和工作空间登记完成。
2. 在设置中完成模型认证；为项目选择 Workflow、Standard、Role→Agent、Sandbox、权限 Profile 和驱动，并保存项目测试配置。
3. 准备实际执行环境。Host 根据所选 Harness 与 Sandbox 组合配方，核验镜像、驱动与权限资产；准备探针不调用模型。
4. 创建 Feature 并填写需求。在执行页查看当前输入、步骤产物、测试和评审；需要人工决定时，核对确切版本后签署。
5. 确认 PR、合入结果和本地同步回执，再判断工程交付是否完成。

默认出厂目录发布精简版 Feature/Task 流程，保留独立 Red/Green、代码评审、源版本与合并结果两类项目回归。PRD、Design、Tasks 和最终 Feature 合入要求 Human 决定。配置组成及流程详见[默认配置](resources/configuration/README.md)与[工作流和编排](docs/workflow-orchestration.md)。

### 运行包

```bash
./scripts/release/package.sh
```

输出为 `dist/devdesk-0.2.0-<rust-host-target>.tar.gz` 及 SHA-256 文件。解压后运行 `./devdesk-host`，无需 Rust、宿主 Node 或源码；实际工程仍需 Docker、Git、GitHub CLI 及认证。远程连接、访问密钥和平台设置见[运行包说明](docs/run-binary.md)。

## 开发与验证

开发入口构建 Web 和 debug Host，然后启动服务：

```bash
./scripts/dev/dev.sh --data ./data --port 4317
```

Web 与出厂资源嵌入 Host；修改后需要重新构建并重启。macOS 启动器使用独立签名副本，避免后续编译覆盖正在运行的程序。

```bash
# 在已安装上述 Python 依赖的环境中执行。
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

完整检查覆盖配置和权限资产、架构、文档、RPC 清单、代码风格、类型、Web 构建、Rust 测试，以及 debug/release 的 HTTP、数据集和 Chromium 回归。默认回归不调用付费模型或真实 GitHub；Docker 测试与模型工程验收有显式入口，见[测试索引](tests/README.md)。

常用专项检查：

```bash
./scripts/check/check-style.sh
cargo test --workspace --locked
python3 scripts/py/check-docs.py
```

## 项目结构

| 路径 | 职责 |
|---|---|
| `crates/domain` | 领域类型、纯规则与有界校验 |
| `crates/core` | 配置编译、命令/查询、会话与工作流、权威回执接纳 |
| `crates/adapters` | SQLite、产物存储、Git/GitHub、Docker、Harness 与凭据 |
| `crates/host` | HTTP/SSE、认证、项目路由、调度与 Web 嵌入 |
| `crates/credential-helper` | macOS 独立签名 Keychain 助手 |
| `web` / `tests` | Web 工作台、浏览器回归及跨进程/原生测试入口 |
| `resources` / `permissions` / `containers` | 出厂配置、权限后端资产与项目测试容器 |
| `schemas` / `sql` | JSON 结构合同与 SQLite 初始化基线 |
| `docs` / `scripts` | 当前说明、实施计划与构建/检查/打包工具 |

依赖方向为 `host → adapters → core → domain`，外部能力通过 core 端口注入。详见[代码手册](docs/crates/overview.md)。

## 数据与权限

数据目录包括共享的 `global.sqlite3`、独立全局讨论库，以及 `projects/<id>/` 下的工程库、产物、仓库和执行目录。一个数据集由一个 Host 管理。

当前数据标记为 `devdesk-profile-runtime-v2`，布局为 `single-host-resource-v2`，SQLite 基线为 **global 24 / project 60**。不兼容数据集会被拒绝，不自动迁移、清空或重建；验证新代码使用独立空目录。

配置唯一保存于不可变的 `source_definitions`。Agent 的能力与 Profile 的授权分别校验，Harness/Sandbox 两层权限共同约束实际执行；无法落实的策略明确拒绝。具体边界见[存储布局](docs/storage.md)和[权限说明](docs/permissions.md)。

## 文档

| 主题 | 入口 |
|---|---|
| 使用与部署 | [Web 工作台](docs/web-workbench.md)、[运行包](docs/run-binary.md)、[系统凭据](docs/system-credentials.md) |
| 工程方法 | [工作流与编排](docs/workflow-orchestration.md)、[源配置](docs/source-configuration.md)、[真实工程接入](docs/real-engineering.md) |
| 代码与架构 | [crates 索引](docs/crates/overview.md)、[执行链](docs/crates/execution-flows.md)、[架构](docs/design.md)、[领域](docs/domain.md) |
| 合同与接入 | [结构合同](docs/contracts.md)、[协议](docs/protocol.md)、[适配器](docs/real-adapter-contract.md)、[工具与技能](docs/tool-skill-integrations.md)、[模型服务商](docs/model-providers.md) |
| 开发参考 | [Rust 规范](docs/code-standards-rust.md)、[Web 规范](docs/code-standards-web.md)、[测试](tests/README.md)、[脚本](scripts/README.md)、[Schema](schemas/README.md) |

`docs/` 根目录和 `docs/crates/` 描述当前实现；[docs/plans/](docs/plans/README.md)保存方案与历史验收，其结果只对应当时的代码、环境和输入。

## 贡献

修改前阅读 [AGENT.md](AGENT.md)，保持领域、应用、适配器与 Host 的边界。行为变更同步更新合同、必要的 Schema 和当前文档；提交前运行相关检查，说明验证结果与尚未验证的范围。日志、凭据、运行数据和构建产物不进入版本库。

## 许可证

仓库目前未提供根级 `LICENSE` 文件。第三方组件及权限资产的许可证分别随依赖、安装元数据和 `permissions/backends/docker/upstream/` 保留。
