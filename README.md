# DevDesk v0.5

DevDesk 是一个可加载领域包的工作台。Rust Host 管理配置、领域对象、工作流、Agent 会话、权限和历史。Web Core 提供操作界面，其他客户端也可以通过同一套 HTTP、JSON-RPC 和 SSE 协议连接。软件开发是首个独立领域包，提供从 Feature、Task 到 PR 合入和本地同步的完整流程。

## 安装和启动

从发行渠道取得所需制品及其 SHA-256 校验文件。

| 制品 | 内容 |
|---|---|
| `devdesk-core-host-0.6.0-<target>.tar.gz` | Rust Host、核心配置及运行资产。支持空领域和纯 API 运行 |
| `devdesk-core-web-0.6.0.tar.gz` | Web Core、共享 React 布局与 UI SDK、领域包 Schema |
| `software-development-0.6.0.zip` | 软件开发配置、Node.js 运行时扩展和 Web 页面 |

解压 Host 和 Web Core，将路径替换为实际目录。启动空领域仅需前两个制品。

```bash
./devdesk-host --data ./data --web-dir ./devdesk-core-web-0.6.0
```

打开终端显示的地址，默认是 `http://127.0.0.1:4317`。空领域工作台提供基础组件、全局历史、设置和领域包管理。省略 `--web-dir` 时，Host 提供客户端协议。

接入软件开发时，在已经启动的工作台打开「领域管理」，选择 `software-development-0.6.0.zip` 导入，再点击「激活」进入软件开发工作台。领域包可在 Host 运行时加载、切换和卸载。浏览器在同一工作台内渲染领域页面，共用导航、布局和会话。安装、数据目录和远程连接设置见[运行指南](docs/design/run-binary.md)。

### 开始软件开发工作流

1. 激活软件开发包，在工程设置中配置 GitHub 认证并接入仓库。
2. 配置模型认证、工作流、Standard、Role 到 Agent 的绑定及测试环境。准备 Harness/Sandbox 时，Host 会验证实际 Docker 镜像和权限资产。
3. 创建 Feature，在执行页选择人工、Rust 或 Agent 调度。三个模式共用步骤输入、产物、审批与接纳规则。
4. 检查 PRD、设计、Task 列表、独立 Red/Green 和评审产物，在需要人工确认的步骤核对确切版本并签署。
5. 在交付页跟踪源发布、候选、回归测试、PR 合入及本地同步回执。

运行软件开发领域需要 Docker、Git、GitHub CLI，以及实际使用的 GitHub 和模型服务认证。见[工程交付](docs/design/engineering-delivery.md)和[工作流](docs/design/workflow.md)。

## 从源码开发

构建使用仓库固定的 Rust **1.99.1**、Node **24**、Python **3.10+** 和 C 工具链。macOS 的签名和 Keychain 设置见[签名指南](docs/crates/devdesk-adapters/signing-and-keychain.md)。

```bash
./scripts/dev/dev.sh --data ./data --port 4317
```

独立生成发行制品及校验文件：

```bash
./scripts/release/package.sh host
./scripts/release/package.sh web
./scripts/release/package.sh software-development
```

默认输出在 `dist/`。Host 可独立于 Node、Web 构建产物和领域源码构建。Web Core 构建只需要前端工具链。领域 ZIP 的运行机器需要 Docker，安装时使用已打包的 JavaScript 与静态资产。

Host 默认使用体积优先的 `size` 配置。使用 `host --profile speed` 启用常规速度优化，或用 `host --profile symbols` 保留符号供诊断。见[打包选项](scripts/README.md#host-打包选项)。

```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install -r scripts/check/requirements.txt
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

测试范围、领域制品验收与真实外部执行入口见[测试索引](tests/README.md)。

## 仓库与文档

| 目录 | 内容 |
|---|---|
| `crates/{domain,core,adapters,host}` | Rust 领域模型、应用层、执行与持久化适配器、Host |
| `web/` | Web Core、共享 SDK、软件领域使用的 UI 组件 |
| `kernel/core/` | 随 Host 发布的领域无关默认配置 |
| `plugins/software-development/` | 软件开发目录、运行端、Web 贡献及 ZIP 构建入口 |
| `schemas/` · `sql/` | 结构合同和 SQLite 基线 |
| `docs/` · `scripts/` · `tests/` | 设计、代码手册、构建、发布和验收 |

[文档索引](docs/README.md) · [领域包架构](docs/design/domain-packages.md) · [客户端协议](docs/design/reference/rpc.md) · [仓库指南](AGENT.md)

仓库目前未提供根级 `LICENSE`。第三方组件的许可随依赖或权限资产保留。
