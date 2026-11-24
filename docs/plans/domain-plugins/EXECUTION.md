# 领域插件化实施与验证

实现工作位于 `codex/domain-plugins`。计划基线为 `2458a06`，P0–P2 首条发行和双端路径为 `8c5f011`。P3 已接入工作流编排 Role 与领域项目的活动代次。

## 已接通的路径

- 核心 Host 的无默认 feature 构建以独立清单加载 Model、Provider、Catalog、Harness、Sandbox、权限预设和通用工具。核心数据集使用独立布局标记。独立 Web 入口和构建脚本可从核心 Host 的 `--web-dir` 提供静态文件。
- ZIP 清单、内容摘要、路径边界、源闭包和呈现绑定在安装时检查。激活以全局代次切换，SSE 驱动 Web 与无浏览器客户端重新发现。已激活的 React 组件动态进入同一个 React 根。
- 领域读处理器在固定 Node 镜像的临时 Docker 容器中通过 JSON-RPC 执行。Git 读取由平台适配器执行。文稿 Workflow 的工具步骤走既有 Rust 调度、持久化调用和回执接纳，Web 可按无浏览器端返回的执行 ID 读回状态和输出。
- 切换及卸载核对包绑定项目中的活动根流程。正在工作的包需要先结束在途流程。包停用期间，其绑定项目不领取后台调度。重装相同摘要后可读取原有执行。
- 包内 Workflow 声明编排 Role，配合包默认的 Role→Agent 精确绑定，经活动目录向各端发布 Agent 调度选项。工作流控制器和全局项目管理使用各自的 Role 资格。领域项目的 RPC、Artifact 下载和事件流检查活动代次，切包时结束原领域项目事件流。
- ZIP 可为带版本和 JSON Schema 的领域对象类型声明根工作流和交付端口。项目库按预期修订冻结文稿，完成事务从已接纳的端口提交校样，并保存原 Artifact、源执行和签署引用。

## 验收入口

Host 和 Web 分别由 `scripts/release/package-core.sh host` 和 `scripts/release/package-core.sh web` 打包。解压后，传入全新数据目录、Web 制品目录和受信任的 ZIP SHA-256。Linux x86_64 上运行领域 worker 前，Docker 需要具备固定的 Node 镜像。

```bash
docker pull node@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f
zip_sha=$(sha256sum /path/to/domain.zip | cut -d ' ' -f 1)
devdesk-host --data /path/to/new-dataset --web-dir /path/to/devdesk-core-web --trust-domain "$zip_sha"
```

体验夹具可执行 `python3 tests/fixtures/domain-packages/build.py /path/to/zip-directory` 生成。按仓库源码复跑验收时使用以下命令。

```bash
cargo build --locked -p devdesk-host --no-default-features
npm --prefix web ci
npm --prefix web run build:core
node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core
```

验收使用独立数据目录、自建 Git 仓库、固定 Linux Docker 镜像、两个 Chromium 标签页和 Python 标准库 HTTP 客户端。记录真实提交和执行 ID，检查空领域中新增的模型、真实 Codex Harness/Docker 准备回执、核心 Git 读取、同一提交的双端读取、文稿 Workflow 持久化输出、未结束流程阻塞卸载、取消后卸载、SSE 代次、旧代次拒绝以及重装后的原运行。容器内的文稿处理和本地 Git 查询为实际执行。现有 Agent 定义用于配置发现，模型调用和 GitHub 写入属于后续验收。

独立发行件解压后的完整脚本通过。该次记录的镜像为 `sha256:328e96db04cc428bec67d1e8ef06837041c3c147ce5940c7e86c251f9809b84b`，流程执行为 `d3eab70c-c437-db47-e175-329274fc50d2`。证据位于 `/tmp/devdesk-domain-release-verification.B1Vzff/result.json`。

P3 扩展验收使用同一个文稿包。Web 启动人工流程，Python 派发步骤、读取其 Artifact、接纳确切产物并结束流程。Python 也检查命令重放、损坏 ZIP、重复导入、旧代次写入、卸载后的项目目录，以及两类旧项目事件订阅结束。独立发行件复验的 Rust 执行为 `7d889430-9d55-2c00-15e8-f2fd4ba0c941`，人工执行为 `7fd78589-e7f2-7bec-d4e2-0d0ffe2260a7`。证据位于 `/tmp/devdesk-domain-p3-release.3h1AAp/result.json`。

脚本化 Harness 通过原生工具派发编辑角色负责的流程，并记录来自 Human 的步骤批准。该 Role 无法开启项目管理控制器。测试使用脚本化 Agent 行为，真实模型调用单独验收。

对象合同验收直接通过 Python 客户端发现类型，拒绝类型错误、跨项目读取及过期修订写入，验证新建重放和重装原 ZIP 后回读修订。测试将实际工作流输出保存为校样对象，保留工作流自己的 Artifact 和来源记录。最新发行件复验记录位于 `/tmp/devdesk-domain-p3-terminal.rx19mJ/result.json`。单独解压 Host、以空领域启动并通过 HTTP 回读核心配置的检查也通过。

对象归属的后续复验覆盖浏览器人工启动、Rust 自动派发以及真实 Codex 编排。三种方式使用同一文稿 ZIP 和 Docker 处理器。Codex 发起人工决定请求，验收客户端通过已认证的 Human RPC 提交签署，随后 Codex 接纳原结果并完成工作流。真实执行 `5e43b242-9996-6e29-9210-ac07a572675c` 的证据位于 `/tmp/devdesk-domain-live-p3-final.ZYgXjv/evidence/result.json`，校样记录包含非空输入摘要及签署引用。正式会话沿用项目数据库会话分区，冻结的正式会话来源记录对象归属。

## P4 软件开发候选包

仓库测试入口识别和工作副本分支策略已纳入软件开发 ZIP。预检使用 Git 提交里的跟踪内容，识别逻辑在 Docker 中运行。使用空仓库、Node、Rust、Go、pytest、unittest、自定义测试合同七类仓库对照旧实现，结果一致。

Feature 和 Task 的真实 Git worktree 验证了分支策略在物理调用前写入操作意图。对已写入的 Feature 操作注入回执丢失后，`repository.reconcile` 只读取原工作副本和回执，物理写入计数保持不变。本轮远端 PR、合入、保护分支和同步仍待迁移及验收。[候选包](../../../configurations/software-development/README.md)

两个独立发行件解压后，再次通过双端及热切换验收。软件包预检执行为 `65002759-b374-283b-0a1a-537a04e66552`，证据位于 `/tmp/devdesk-p4-release.2bfC1L/result.json`。同一 Host 发行件的真实 Codex 复验执行为 `ec056394-3eae-392e-2053-42c897e5d2bd`，证据位于 `/tmp/devdesk-p4-live-agent.RWWiq2/evidence/result.json`。

## 基线和后续范围

原基线 `2458a06` 的 `actual_git_docker_workflow_handoffs` 在启动 Docker 前停于 `builtin-tool-workspace-snapshot:2` 的配置读取。探针把项目级源用于全局草稿，并引用了退役 Harness 的测试工具修订。只修正探针的草稿作用域和工具引用后，真实 Git/Docker 流程在 18.210 秒内完成。证据存于 `/tmp/devdesk-domain-p0.O6iSOB/evidence`。

首批 ZIP 使用 Host 启动参数提供的可信摘要。当前每次只读调用使用临时容器。对象关系、讨论与经验的对象范围、冻结资源句柄，以及带副作用处理器的停止和核对仍按 P3–P6 推进。[完整计划](PLAN.md)
