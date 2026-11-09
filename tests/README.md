# 测试组织与入口

仓库按验证边界组织测试，根目录 `tests/` 只承载跨进程回归、真实验收探针、报告器、测试替身和固定数据；Rust 与前端测试分别放在各自源码附近。

| 位置 | 测试内容 | 执行方式 |
|---|---|---|
| `crates/*/src/` 内的测试模块 | 领域类型、权限和配置规则、协议解析、进程与凭据边界、Host 查询等模块内回归 | `cargo test --workspace --locked` |
| `crates/adapters/tests/` | 通过适配器验证存储、工作流、恢复、会话、运行安装、治理、权限及 Git/Docker/GitHub 资源 | 同上；依赖外部资源的用例部分标记 `#[ignore]`，需显式执行 |
| `crates/adapters/tests/configuration_resources.rs` | Role/Capability/Agent 独立修订、默认阶段能力、项目绑定、逐轮冻结 Spec、基线与审计、子流程输入输出/失败返工、旧 Standard 分离迁移 | 随 Rust 默认测试执行 |
| `crates/adapters/tests/schema_contracts.rs` | 所有发布 Schema 和引用的离线编译、当前配置与 Serde 数据契约、权限和不可变产物兼容、非法输入拒绝 | 随 Rust 默认测试执行；使用 Rust 测试依赖，不依赖 Python jsonschema |
| `web/tests/` | Playwright 浏览器回归：工作台、项目接入、设置与权限、Workflow/Agent/Role 编辑和重载持久化 | Web 构建及 Host 就绪后，在 `web/` 执行 `npx playwright test` |
| `tests/entrypoints/http_contract.py` | 独立 Host 进程的 HTTP、安全、生命周期和分库隔离契约 | 默认检查入口自动执行 debug 和 release/standalone 两次 |
| `tests/entrypoints/runtime_preparation.py` | 独立 Host 进程的延迟运行环境准备及仓库接入恢复 | 默认检查入口自动执行一次 debug |
| `tests/live/` | 真实供应商、容器、仓库、凭据及浏览器专项验收；不同探针的外部依赖不同 | `scripts/check/check-live.sh` 或按需执行单个探针 |
| `tests/fixtures/protocols/` | 脱敏真实协议样本，由 Rust 协议测试嵌入回放 | 随对应 Rust 测试执行 |
| `tests/fixtures/legacy/` | 静态历史项目库及产物：迁移、完整性与只读兼容 | 随 Rust 存储测试执行 |
| `tests/reporters/` | 产品执行用户项目 Node 测试时使用的结构化报告器 | Rust 编译嵌入；报告器契约由 Rust 回归验证 |
| `tests/support/` | 只编译进 Rust 测试的原生接口替身 | 由对应测试模块引用 |

## `entrypoints/` 中的 Python 检查入口

`http_contract.py` 和 `runtime_preparation.py` 都是离线集成回归，使用 Python 标准库和临时目录，不是 pytest 用例，也不是待清理的临时脚本。默认入口为：

```bash
./scripts/check/check.sh
```

`http_contract.py` 启动真实 Host，并用不可用的 Docker/GitHub CLI 夹具阻断外部执行，验证 Cookie 会话鉴权、Host/Origin 限制、JS/CSS 资源、能力接口、中文和空格路径、跨进程写锁、命令幂等、Artifact 访问、项目切换和 SQLite 分库、SSE 项目隔离、退出与重启持久化。`--standalone` 会把二进制复制到临时目录再运行，用于证明嵌入资源不依赖源码目录。

`runtime_preparation.py` 使用真实 HTTP、SQLite 和本地 Git；Docker CLI、GitHub 响应以及远程克隆入口使用明确的本地夹具。它验证打开项目不触发 Harness 准备、中断 clone 的核对恢复、并发准备复用、默认组合的安装固化、镜像缺失、构建失败重试和 Docker 离线等情况，不调用模型或真实 Docker daemon。

构建 Host 后可单独执行：

```bash
python3 tests/entrypoints/http_contract.py
python3 tests/entrypoints/runtime_preparation.py
python3 tests/entrypoints/http_contract.py target/release/devdesk-host --standalone
```

## `live/` 中的真实验收

`check_live.py` 是编排入口，`client.py` 是共享 HTTP/RPC 客户端。默认组为 `git,docker,harness,github,web,workflow,recovery`，`workbench` 需显式选择。`--mode full` 生成新执行，`--mode verify-existing` 复核已有事实；后者不能算作一次新的模型或浏览器验收。

| 探针 | 验证内容 | 是否由编排入口直接调用 |
|---|---|---|
| `probe_application_conversations.py` | 应用到原生供应商的真实对话、轮次消费和持久化证据 | `harness` 组 |
| `browser_conversation.mjs` | 浏览器讨论、结构化提案和正式需求采纳 | `web` / `workflow` 组 |
| `probe_repository.py` | 仓库登记、接入和 Feature worktree | `web` / `workflow` 组 |
| `probe_planning.py` | 角色讨论、规划采纳、原生规划 Run/Review 和任务 worktree | `workflow` 组 |
| `probe_implementation.py` | Task DAG、Docker Red/Green、Delivery 和独立 Review | `workflow` 组 |
| `probe_merges.py` | PR 候选、过期目标拒绝、合并推进、Feature 收尾与主线交付 | `workflow` 组 |
| `browser_accept.mjs` | 已评审主线候选的浏览器最终验收 | 由 `probe_merges.py --browser-final` 间接调用 |
| `probe_recovery.py` | 原生取消、交互问题、输入已消费后的崩溃恢复 | `recovery` 组 |
| `probe_workflow_workbench.py` | v2 分阶段默认能力、PRD 可复用子流程、显式 Spec、Task TDD 与独立评审的原生闭环 | 显式 `workbench` 组 |
| `probe_agent_composition.py` | 不可变 Model/Harness/Tool/Agent 组合的原生轮次 | 单独执行 |
| `probe_agent_experience.py` | AgentID 经验隔离、跨项目查询与场景恢复 | 单独执行 |
| `probe_installations.py` | 并发安装登记、Docker 镜像层复用 | 单独执行 |
| `probe_protocols.py` | 不带凭据的供应商协议握手 | 单独执行 |
| `probe_turns.py` | 原生供应商两轮输入和 RPC 兼容性 | 单独执行 |
| `probe_boundaries.py` | Docker/GitHub 边界实验与专用仓库操作 | 单独执行 |
| `probe_managed_tools.py` | 原生工具可用性与私有状态隔离 | 单独执行 |
| `probe_nested_sandbox.py` | 使用无害凭据标记验证真实 Codex 子沙箱，不调用模型 | 单独执行 |
| `probe_permissions.py` | 双供应商 Permission 的文件和网络限制 | 单独执行 |
| `probe_permission_package.py` | 解包运行包的 Permission 资产、API 和持久化，不调用模型 | 单独执行 |
| `probe_permission_runtime.py` | 临时独立数据集中的真实 Codex v2 改绑、后继会话、工具写入及受控 HTTP 收据；显式提供登录文件，测试后清理副本 | 单独执行 |
| `probe_linux_credentials.mjs` | Linux Secret Service、浏览器凭据设置和 Host 重启 | 单独执行 |
| `probe_oauth_recovery.mjs` | 原生 OAuth 开始/取消与本地响应丢失恢复，拦截授权导航 | 单独执行 |

编排入口的 `git` / `docker` 组调用 Rust 资源集成测试，不另设同名 Python 探针。默认离线检查不会自动执行 `live/`；该目录下的专项探针也不会全部随 `check-live.sh` 执行。真实验收需专用数据集和测试仓库，具体前提和参数见 [工程接入](../docs/real-engineering.md) 及各探针的 `--help`。日志、JSON 和截图写到临时报告目录，计划目录只保留 Markdown 执行说明。

`tests/reporters/node-reporter.mjs` 是产品执行测试时使用的 Node 报告器，属于运行时资产：Rust `CommandTests` 将其嵌入并写入测试容器，解析结构化失败分类和汇总，并将内容摘要纳入执行器身份。它是产品运行时需要的测试支持资产，与测试用例分开放在 `reporters/`。

原型脚本化执行用例已移除；当前 Rust 回归覆盖原生会话、Run 结构化结果、取消、停止证明、重启后的旧测试核对和历史只读迁移。Workflow 样本演练仅计算流程条件，不创建外部事实。

DSH 多服务商离线验证：设置 DEVDESK_DSH_MOCK_IMAGE 为固定镜像 ID，运行 `cargo test -p devdesk-adapters --test model_providers -- --ignored --nocapture`。DSH 流式/工具/恢复由禁外网容器内的模拟端点提供；另一测试只查询模拟 Ollama 目录，不推理。`tests/live/probe_model_providers.py` 验证完整 Host 连接与冻结；仅传入 --codex-auth-source 时允许 Codex 真实模型验证，始终不发 DSH 真实推理请求。

macOS 源码启动兼容回归：在已配置同证书签名助手的开发机执行 `python3 tests/entrypoints/credential_helper_startup.py --launch`。兼容/拒绝检查使用隔离的助手副本，实际启动只打开空工作空间，不读取凭据、不调用模型，也不替换共享助手。


Workflow 任务专项：`cargo test -p devdesk-adapters --test agent_tasks --locked` 覆盖所有 Agent 类别的记录端口派发、v3 校验、Markdown 原文、冻结恢复、正式 Run/重试、权限后继、子流程身份和伪造拒绝。原基线夹具显式选择 v2，专项夹具选择 v3。

`DEVDESK_DSH_MOCK_IMAGE=sha256:… cargo test -p devdesk-adapters --test agent_tasks frozen_task_reaches_native --locked -- --ignored --nocapture` 使用固定 DSH SDK、Docker --network=none 和本地模型响应，核对原生请求中的完整任务/Spec 与基础 Role/Skill、工具结果及恢复；不声称真实 DSH 模型推理。`probe_workflow_tasks.py --binary … --auth-file … --output …` 在临时私有数据集中用真实 Codex 执行两个任务，核对实际工具输出和受控 HTTP 回执，只复制显式提供的登录文件，不读取原项目库。探针输出不含凭据。

`web/tests/workflow-tasks.spec.ts` 验证浏览器编辑、保存、发布、上下文预览及公共/配方任务持久化。独立发布包探针 probe_permission_package.py 同时验证嵌入的默认 v3 任务、编辑发布、只读预览和项目 schema v38，不调用模型。
工具与技能接入：`crates/adapters/tests/tool_integrations.rs` 覆盖修订、作用域、配置锁、依赖、离线 Schema、审批/取消/核对；其 `docker_` 忽略用例使用真实 Docker HTTP/MCP/OAuth 夹具，不调用模型。`web/tests/integrations.spec.ts` 使用实际 Host 完成配置，MCP 测试通过真正的 Docker stdio 服务发现及调用，需可用 Docker 和固定 Codex 镜像。

`tests/live/probe_tool_integrations.py --codex-auth-source /绝对路径/专用/auth.json --host /绝对路径/devdesk-host --report /tmp/tools-native.json` 创建独立数据集，配置 HTTP/MCP/Skill、Capability、Agent 和项目绑定，真实 Codex 执行两轮并重启 Host，核验文件只读和外部调用回执。始终不调用 DSH；只复制明确提供的 Codex 登录文件，结束后删除测试中的 auth.json 副本，保留无凭据运行证据。
