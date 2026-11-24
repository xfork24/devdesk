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

Host 和 Web 分别由 `scripts/release/package.sh host` 和 `scripts/release/package.sh web` 打包。解压后，传入全新数据目录、Web 制品目录和受信任的 ZIP SHA-256。Linux x86_64 上运行领域 worker 前，Docker 需要具备固定的 Node 镜像。

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

对象类型现在可以声明有类型与基数约束的关系。对象写入在同一提交中核对可达对象版本，防止两个并发写入形成环。校样提交绑定原文稿关系。Web 和 Python 客户端通过了跨项目目标、循环、未声明关系及校样归属检查。验收记录位于 `/tmp/devdesk-p3-web.hUEePO/result.json`。

讨论和经验使用确切的对象归属。相同对象及 Agent 可以复用讨论，其他对象无法继承该讨论或按自身范围读回原经验。真实 Codex 讨论 `95d8f08e-3986-8a30-1f93-4ca079de65ed` 步骤结束后生成经验 `fd3f81f3-495c-10d1-b7c1-a635287b4234`，并通过了隔离断言。

Harness 按会话、操作、所有者及版本接收工作区租约。Git 继续使用已冻结的提交和独占写入检查。只读 Artifact 取自同一流程中已接纳的步骤输出，验证摘要后挂载为 `/work/artifact`。适配器测试覆盖错误操作、写权限和内容变化。P3 联合验收再次完成真实 Codex 编排、对象讨论与经验隔离。证据位于 `/tmp/devdesk-p3-final.z3hG6K/evidence/result.json`。非 Git 工作区也经过真实 Codex 验收。Rust 驱动将同一流程前一步封存的 Artifact 只读挂载，Codex 的实际命令读取了原字节，产出的校样仍关联原文稿。执行和内容摘要见 `/tmp/devdesk-p3-full-mounted.RX94tB/evidence/result.json`。

## P4 软件开发候选包

仓库测试入口识别和工作副本分支策略已纳入软件开发 ZIP。预检使用 Git 提交里的跟踪内容，识别逻辑在 Docker 中运行。使用空仓库、Node、Rust、Go、pytest、unittest、自定义测试合同七类仓库对照旧实现，结果一致。

Feature 和 Task 的真实 Git worktree 验证了分支策略在物理调用前写入操作意图。对已写入的 Feature 操作注入回执丢失后，`repository.reconcile` 只读取原工作副本和回执，物理写入计数保持不变。源发布、PR、合入和同步的验证见本节后续记录。[候选包](../../../plugins/software-development/README.md)

两个独立发行件解压后，再次通过双端及热切换验收。软件包预检执行为 `65002759-b374-283b-0a1a-537a04e66552`，证据位于 `/tmp/devdesk-p4-release.2bfC1L/result.json`。同一 Host 发行件的真实 Codex 复验执行为 `ec056394-3eae-392e-2053-42c897e5d2bd`，证据位于 `/tmp/devdesk-p4-live-agent.RWWiq2/evidence/result.json`。

软件开发包增加了 Feature→Task 的命名空间化配置目录。包内处理器解析 GitHub 连接坐标，Host 从已绑定包的项目发出原生仓库连接意图。空领域 Host 已验证安装、激活与项目配置装配。

专用 GitHub 仓库的 `devdesk/domain-p4-20261124` 分支从确切的 Node 测试基线启动。0.2.0 的 Task 和 Feature 均通过源发布、候选、两类真实 Docker 回归、PR 合入及本地同步。Task 的 Red 有 5 个预期断言失败，Green 有 6 个通过。签署前派发 Feature 合入被 `dependencies` 拒绝，原 GitHub 目标引用仍指向基线。签署、合入后，候选 tree、远端目标和干净本地工作树通过逐项核对。原 Task PR 使用相同的标记做实仓回读，重复请求返回同一 PR。完整核对记录位于 `/tmp/devdesk-p4-domain-live.p9xrZP/run-core/delivery-audit.json`。[Task PR #35](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/35) · [Feature PR #36](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/36)

0.3.0 在软件包内提供测试报告器、候选分支策略和 GitHub PR 创建请求生成。Host 保存冻结报告器及 PR 请求，在通用 Docker、Git 和 GitHub 端口执行。GitHub 适配器继续处理远端保护、目标 CAS、合入观察及同步。独立解包的 Host 和 Web 通过了空领域、安装、加载、卸载及同一协议的 Web 和 Python 验收。

软件开发 0.3.0 的完整验收使用同一专用仓库的 `devdesk/domain-p4-package-20261124` 分支。Task Red 出现 4 个预期断言失败，Green 通过相同的 4 个用例。两个流程共执行六次 Docker 测试，回执保留 ZIP 内报告器的摘要。核对了两份远端 PR 的标题和正文、源发布修订、候选 tree、目标父提交、合入和本地同步。Feature 合入在人工签署前被依赖门禁拒绝。对 PR #37 回读并重放原标记得到同一个 PR。实仓使用普通测试分支，保护分支拒绝及目标变化由适配器回归覆盖。[Task PR #37](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/37) · [Feature PR #38](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/38)

正式包位于 `dist/software-development-0.3.0.zip`，SHA-256 为 `dacee6dca95abaefab62af7194c86c7089726e3299a039e6afc8dd9f2fc4cf34`。重建后的 ZIP 与实仓加载的 ZIP 字节一致。端到端证据位于 `/tmp/devdesk-p4-domain-pkg.VQIVQc/run/delivery-audit.json`。下列命令可从验收数据再次核对冻结记录、实仓 PR 与本地 Git 状态。

```bash
python3 tests/live/audit_domain_delivery.py /tmp/devdesk-p4-domain-pkg.VQIVQc/run \
  --github-repo xfork24/devdesk-default-workflow-test-20261118 --require-packaged-handlers
```

## P5 软件开发页面

0.4.0 将软件工作台页面、分支队列、候选与 PR、交付 Diff、角色讨论、项目配置和历史装入领域 ZIP。Web Core 使用一个 React 根和共用的布局、导航及表单控件挂载活动页面。领域 ESM 入口、样式、项目 RPC 及事件流绑定活动代次。卸载会移除页面、导航及样式，过期资源与请求由 Host 拒绝。

从独立发行件和空数据目录运行 Chromium 双标签页与 Python HTTP 客户端。测试通过两个 Web 接入入口连接专用 GitHub 仓库并复用项目，采用并恢复项目的领域默认配置草稿。在 Web 创建 Feature `9708885e-a3ce-6e28-35d8-81dcb60c3fcf`，比对 Web 和 Python 读到的分支事实，并检查项目历史及浏览器返回路径。Docker 软件预检提交测试配置的执行为 `dcab8c30-9d51-6466-b7e1-404c5ffa1f69`。软件开发与文稿来回切换并卸载，Host 使用同一进程，切回后可读取原执行与产物。验收数据位于 `/tmp/devdesk-p56-release.KskR1C/dual-client.json`。

## P6 独立发行

Linux x86_64 的 Core Host、Web Core 和软件开发 0.4.0 分别打包并生成 SHA-256。Web Core 提供 API 1 和 API 2 的运行接口，并附 Web SDK 类型与领域 Schema。Core Web 构建会拒绝纳入软件工作台入口。软件 ZIP 的 SHA-256 为 `88eb8e50e744bdba5d2eaa6b7340634c14bcd8a2873033ea27f9ab8d645e82a0`。

从制品解压目录验收仅升级 Web、仅升级 Host、仅升级领域 ZIP、不兼容版本拒绝、回切旧摘要、强制停止后恢复以及 100 个并发领域目录读。0.4.0 的运行端及配置与 0.3.0 逐文件相同，实仓 PR #37、#38 在同一轮再次审计并核对同步证据。发行矩阵位于 `/tmp/devdesk-p56-release.KskR1C/matrix.json`，实仓审计位于 `/tmp/devdesk-p56-p4-delivery-audit.log`。[运行和兼容性](../../design/run-binary.md#领域工作台独立发行)

CI 包含 Linux 和 macOS 的独立构建以及 Linux 双端、回切与重启验收。本轮在 Linux 本地执行。原 Feature/Task 的 `delivery_policy` 仍参与冻结门禁和恢复。正式 100 个活跃前端的持续负载矩阵需要独立负载机，本轮覆盖并发目录读。

## 0.5.0 交付证据迁移与三种调度验收

软件开发 ZIP 的 Feature Development 第 6 修订和 Task Development 第 5 修订使用显式 `operation_evidence`。Host 在冻结平台操作意图中记录证据来源、步骤执行、摘要及回执版本，Docker 中的领域验证器检查对应操作。完成条件要求已接纳的真实合入和同步回执。0.3.0、0.4.0 及旧版内置流程保留原冻结合同。

同一 0.5.0 ZIP 在三个独立 Linux Core Host 数据目录和专用 GitHub 测试分支运行。每条是具有真实 Codex 功能角色、一个 Task、源发布、候选回归和远端合入的 Feature 开发工作流。人工模式用已认证的验收客户端调用公开派发和接纳接口。审批在核对冻结材料、精确回执及最终 GitHub Diff 后，以独立操作者请求提交。

| 调度 | Task PR | Feature PR | Task 独立 Red / Green |
| --- | --- | --- | --- |
| 人工 | [#39](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/39) | [#41](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/41) | 4 个预期断言失败 / 5 个用例通过 |
| Rust | [#42](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/42) | [#44](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/44) | 5 个预期断言失败 / 5 个用例通过 |
| Agent | [#40](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/40) | [#43](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/43) | 5 个预期断言失败 / 6 个用例通过 |

三条流程各实际执行六次 Docker 测试。审计核对新包的八次持久化交付授权、两份 GitHub PR 的创建计划、源和候选提交、GitHub 合入 tree、目标父提交及干净的本地同步。三个 Feature 在签署前的合入派发均被拒绝，实际远端目标仍为原提交。另用该包处理真实 Task 回执的修改副本，确认过期候选、写租约未撤销及错误 Green 生产者均被拒绝。

Rust 线路保留同一执行做了一次真实的独立评审返工。已补充 Host 的返工预演公开路由。Agent 线路在重启后核对并停止旧的待核对轮次，通过原工作流恢复 Agent 调度。审计从持久化派发记录读到人工 33 次、Rust driver 35 次和 Agent 授权 segment 33 次。Agent 根流程完成且经理关闭。

实仓运行使用调试 Host。解包后的 Linux 发行 Host 通过目录与热切换双端回归，并在三个原数据集重新读出完整执行及合入同步回执。Rust 全量回归 766 个通过，39 个忽略。0.5.0 发行 ZIP 与三个活动 ZIP 字节一致，SHA-256 为 `3bed78151ba2feabee787ce053816cf0c9f170de80d341824b8c2161359997c3`。合并验收记录位于 `/tmp/devdesk-domain-evidence.gD22zd/three-mode-acceptance.json`。

## 基线和后续范围

原基线 `2458a06` 的 `actual_git_docker_workflow_handoffs` 在启动 Docker 前停于 `builtin-tool-workspace-snapshot:2` 的配置读取。探针把项目级源用于全局草稿，并引用了退役 Harness 的测试工具修订。只修正探针的草稿作用域和工具引用后，真实 Git/Docker 流程在 18.210 秒内完成。证据存于 `/tmp/devdesk-domain-p0.O6iSOB/evidence`。

首批 ZIP 使用 Host 启动参数提供的可信摘要。当前每次只读调用使用临时容器。带副作用处理器的停止和核对随 P4–P6 推进。[完整计划](PLAN.md)
