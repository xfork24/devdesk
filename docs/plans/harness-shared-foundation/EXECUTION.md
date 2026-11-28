# Harness 接入实施记录

2026-11-28。工作分支 `codex/harness-integration`，起点 `444ebea`。五份计划共用 `/home/hengliu/workspace/devdesk-harness-integration`。架构独立性改造在独立分支推进，按计划第 8 章后期合并。

## G0 Linux 首轮证据

环境是 Linux amd64、Docker Engine 29.10.0、默认本地 Socket。镜像使用出厂 Sandbox 固定的 Node 24.13.0 bookworm-slim。四个 CLI 均通过 Linux amd64 镜像构建。真实 CLI 向 Docker 网桥上的受控模型及 MCP 夹具发请求。断网探针使用 `--network none --read-only --cap-drop ALL`。探针、详细协议样本和镜像构建日志在工作树忽略目录 `review/harness-g0/`。

| 接入线 | Linux amd64 执行结果 | 待核对 |
|---|---|---|
| pi 1.6.0 | `get_state` 零凭据断网往返成功。三次步骤工具 `input` 隧道往返成功。收到了 `devdesk.policy.loaded.v1`，冻结 Hook 阻止了测试工具写入，项目扩展与 MCP 未装载，仓库上下文标记未进入模型。分段增量、`error` 失败、`aborted` 取消及重启后的原会话与模型均已验证。 | 已进入批次 A。 |
| mcode 0.7.9 | npm 11.16.0 的实际构建运行了两个允许的脚本，`better-sqlite3` 可打开内存库。ACP `initialize` 零凭据断网成功，`session/new` 在无模型配置时明确要求认证。受控 BYOK 配置成功，三次成功、一次模型错误、一次取消、一次重启恢复。真实 MCP 往返 4 次，模型可见名 `mcp__g0__g0_echo`。追加只读探针中，尝试的用户级 `hooks.json` 未触发阻断，测试 Shell 写入成功，模型也收到了仓库 `AGENTS.md` 标记。 | 重新确认 ACP 的 Hook 装载和项目发现隔离。桥早到请求及 macOS 仍待验。 |
| OpenCode 2.4.1 | BuildKit 按发布 SHA-256 构建成功。`opencode --standalone acp` 被 CLI 拒绝，`opencode acp` 零凭据断网握手成功。无模型目录时 `session/new` 返回 `CatalogNotReady`。使用 v2 私有配置 `providers.*.package/settings`、`instructions` 与 `mcp.servers` 的直出工具模式后，三次成功、错误、取消、重启恢复和 4 次 MCP 往返成功。追加探针使用私有 `permissions` 禁止 Shell，模型工具列表省略 `shell`，仓库 `opencode.json` 的允许规则与 `AGENTS.md` 标记未覆盖私有配置。 | 策略插件装载、项目插件隔离、早到请求、macOS 桥、Linux arm64 运行。 |
| ZCode 3.18.2-48 / 0.17.1 | `doctor` 确认 Node bundle。零凭据断网 `session/create` 成功，参数使用 `workspace.workspacePath` 和 `workspace.workspaceKey`。在应答前必须处理 `session/requestRuntimePreferences`，本探针应答 `nativeSearchEnhancementsEnabled: false`。Linux 上用 `deliveryKind: web-remote-replayable` 订阅成功。宿主模式下测试自定义模型未进入 Registry，明确指定模型的 `session/send` 被拒绝。随包声明还列出有额外使用条款的文档类技能。 | 自定义模型与宿主凭据的装配、权威终态、权限、MCP、macOS 及发布镜像的再分发条件。 |

OpenCode v2 配置与进程边界已对照固定版本文档，见 [ACP](https://github.com/anomalyco/opencode/blob/v2.4.1/services/site/src/docs/content/cli/acp.mdx)、[MCP 配置](https://github.com/anomalyco/opencode/blob/v2.4.1/services/site/src/docs/content/mcp-servers.mdx) 和[权限规则](https://github.com/anomalyco/opencode/blob/v2.4.1/services/site/src/docs/content/permissions.mdx)。

## G0 收口与批次安排

Pi 通过 P0 后进入批次 A。OpenCode 保留为批次 B 的首个候选，等待剩余策略与平台证据。mcode 的冻结策略和项目发现隔离退回设计。ZCode 延期，继续验证模型装配、权限与再分发条件。本轮按用户确认完成 Linux amd64 验证与批次 A。macOS Docker Desktop 和 Linux arm64 尚无可运行环境。

## 批次 A 验收

- 在旧版 Host 创建的隔离 v24 数据集上登记 Codex、DSH 安装并保留历史讨论。停机升级到 v25，验证备份、锁、幂等和旧版回退拒绝。新 Host 显式更新出厂目录并按原 Profile 重绑。历史权限、会话和安装仍可读取，过期资产会要求后继会话。DSH 后继会话及在升级后目录中创建的 Pi 会话通过实际 Docker 和受控模型验证。
- Pi 1.6.0 通过 Host `runtime.prepare` 构建并核验零凭据协议。在普通讨论中记录输入消费、分段输出、终稿与上下文工具回执，实际阻断只读写入。重启 Host 后按原模型与会话 ID 续跑。取消用例确认写权限撤销及容器移除。
- 工作流步骤通过 UI 隧道实际调用 `materials.capture`，并核对模型、治理记录和会话事件。项目讨论中的绑定 HTTP 工具收到真正的本地服务回执，未绑定工具未投放。协议夹具验证重复对话帧只接纳一次、未知对话及意外审批自动取消。
- Chromium 用例验证 Pi Harness 选项、DSH/Pi 服务商凭据隔离、免密 Ollama 和过期凭据编辑恢复。Linux release 制品的 160 个 Chromium 用例全部通过。真实系统凭据写入和付费模型可用性留待授权环境。
- v24 升级恢复代码保留已知的旧权限合同和原资产摘要。后续批次更换当前资产时需要验证其自身的历史恢复和运行拦截。生产数据切换安排独立维护窗口。

Pi 的出厂镜像 ID 为 `sha256:56d6bfe136775f34dc577cf56520ee378a4970fee6cf7589d424dbf31c2e6131`。本地证据目录 `review/harness-g0/native-batch-a/` 保留原生验收报告。24 个 Docker 回归用例与 Pi Host 套件通过。`review/harness-g0/pi/final-legacy/` 保留旧版数据库迁移及升级后端到端证据。

`cargo test --workspace --locked` 在 Linux 上以 4 个测试线程复验，共 781 项通过。样式、Schema、权限资产、出厂目录、模块边界及文档检查通过。Release Host 的数据集、模板、运行准备与 Chromium 入口检查通过。

## 提交与 Linux 发布包补验

2026-11-28，批次 A 提交为 `a138ab0`，保留在 `codex/harness-integration`。用该提交的 `scripts/release/package.sh` 在本地生成 Linux x86_64 Host、Web Core 和 software-development 领域包，三个包均通过 SHA-256 校验。解包后的 Host 在隔离数据集重跑 v24 升级、旧讨论恢复与 Pi 原生套件，全部通过。Chromium 使用解包后的 Web Core，安装并激活独立领域包，核对 Pi Harness、四个模型服务商和免密 Ollama。发布包及日志在 `review/harness-g0/batch-a-package/`。

批次 B 暂挂，macOS Docker Desktop 验证环境就绪后从共享宿主 MCP 桥探针恢复。现有 Harness 计划仅定义 A、B 两批，后期归并仍按共享计划第 8 章执行。
