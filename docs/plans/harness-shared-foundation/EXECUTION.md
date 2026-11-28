# Harness 接入实施记录

2026-11-28。接续分支 `origin/harness-integration`，起点 `444ebea`。Linux 实施使用本地 `codex/harness-integration` worktree，批次 A 代码在 `a138ab0`，发布包补验在 `233f423`。架构独立性改造在独立分支推进，按计划第 8 章后期合并。

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

## macOS 接续

在 macOS 的仓库中检出远端接续分支，再记录宿主架构、Docker context 和 Docker 服务端平台。

```bash
git fetch origin
git switch harness-integration
uname -m
docker context show
docker version --format '{{.Server.Os}}/{{.Server.Arch}}'
```

1. 用本机 Docker Desktop 和隔离夹具恢复 G0 的宿主 MCP 探针。在宿主 `127.0.0.1` 启动探针，从实际运行 Harness 的容器经 `host.docker.internal` 测 `initialize`、`tools/list`、`tools/call`。分别记录连通性和真实 CLI 的工具往返，再测恢复、早到调用、上一轮迟到请求、超时与重试。Linux 的网桥监听地址只适用于原验证环境。验收以第 7 章第 9 项及第 2.4.9 节为准。[共享计划](PLAN.md)
2. 先复验 OpenCode 2.4.1。Linux 已跑通的命令是 `opencode acp`，固定版本拒绝 `--standalone`。模型配置用 `providers.<id>.package/settings`，MCP 用 `mcp.servers`，直出工具设置为 `codemode: false`、`oauth: false`、`protocol: "legacy"`。补齐目标镜像架构、私有进程与会话目录隔离、策略插件装载证明和实际阻断，再决定是否入批。[OpenCode P0](../opencode-harness/PLAN.md)
3. mcode 需要重验冻结 Hook 及仓库配置隔离。ZCode 需要重验自定义模型、权威终态、策略及再分发条件。批次 B 的共享 ACP 和宿主 MCP 桥实现待入批结果明确后开展。[mcode P0](../mcode-harness/PLAN.md)、[ZCode P0](../zcode-harness/PLAN.md)

批次 A 的 Pi 原生回归入口随分支提供，可先在 macOS 用新证据目录复验。探针用隔离数据集和受控模型。Linux 原始 G0 脚本、镜像、数据集、日志和发布档案保留在原机的 `review/harness-g0/`，远端检出只包含已跟踪的源码、夹具及本报告。

```bash
cargo build --locked -p devdesk-host
python3 tests/entrypoints/pi_harness.py --binary target/debug/devdesk-host \
  --reports /tmp/devdesk-pi-macos
```

## macOS 验证结果

2026-11-28。分支起点 `7aad459d`，macOS 26.7.3 arm64，Docker context `desktop-linux`，服务端 `linux/arm64` 29.9.3。模型、MCP 令牌、工作目录和数据集均为本轮夹具。证据在 `review/harness-g0-macos/`。

### 批次 A 与发布制品

- `scripts/release/package.sh` 生成签名的 arm64 Host、Web Core、software-development ZIP。三个制品通过 SHA-256 校验，`domain_release.mjs` 用解包后的制品通过领域激活、回滚、重启和目录恢复检查。
- Pi 原生套件在实际 Docker 容器运行。签名 Host 从既有 v24 Host 创建的独立数据集中升级到 v25，保留 Codex、DSH 安装和历史讨论，更新出厂目录并重新绑定权限。Pi 通过准备、流式讨论、上下文工具回执、只读及私有路径拦截、重启续跑、取消、步骤工具的会话及平台回执、HTTP 集成回执和未绑定工具过滤。镜像 ID 为 `sha256:9685dc73a651cf1480df7fbe216ff403013e34abd3e14930606537373f2c7ea1`。对旧版的回退拒绝由 `dataset_protocol.py` 单独复验。
- Chromium 运行配置选择、模型服务商和设置并发三组专项，13 项通过。真实系统凭据写入与商业模型调用的验收范围沿用 Linux 记录。

### 批次 B G0

`tests/entrypoints/harness_g0.py` 运行实际容器 CLI，对宿主 `127.0.0.1` 上的私有 MCP 和确定性模型留存时间对齐的协议与 HTTP 审计。

| 接入线 | 已验证结果 | 入批缺口 |
|---|---|---|
| OpenCode 2.4.1 | arm64 发布归档 SHA-256 `cb28cf94541aeb62a4b2ea5997a99b5dd6bae595a772f36fbe279299f2032a4f`。断网握手、私有 v2 策略装载及写入拦截、真实权限请求、项目插件与 MCP 隔离。三次成功、模型失败、取消、恢复与 4 次 `g0_fixture_echo` 成功回执。延迟请求超时后没有观察到自动重发。 | 真实请求可早于第一条工具协议通知。同一 MCP 会话连续使用请求 ID 2–5，`_meta` 含会话 ID 和 progress token。恢复后客户端重用数字 ID 2。复用令牌时缺少跨轮次身份，需在生产设计中验证可撤销的轮次绑定。 |
| mcode 0.7.9 | arm64 实测 npm 11.16.0 安装白名单、原生 SQLite、断网握手、三次成功、失败、取消、恢复及 4 次 `mcp__g0__fixture_echo` 回执。私有 Hook 确认 `SessionStart` 和 `PreToolUse` 并拦下测试写入。 | 默认可执行项目 MCP。只读覆盖 `.minimax` 和 `.mcp.json` 的独立测试通过，DevDesk 容器接线及跨轮次请求归属仍待验。 |
| ZCode 3.18.2-48 / 0.17.1 | Linux arm64 Node bundle 断网建会话。完整 manual model 规则及 `modelSelection.options.reasoningLevel` 使个人 BYOK 在 `host` 模式进入 Registry。`session/send` 即时返回 `accepted: true`。三次完成、模型失败、`session/stop` 取消和重启恢复均取得终态。4 次 `mcp__g0__fixture_echo` 的 HTTP `_meta.turn_id` 与协议轮次一致。 | 当时尚待验证宿主账号请求头和冻结策略。实际随包包含四个文档插件，运行时投放了对应工具。固定包许可文件的复核结果见下文。 |

OpenCode 的真实工具名和时序见 `opencode-p0-final/http-audit.json`。超时后的旧令牌重放由探针在恢复轮次中发送，收到 HTTP 401，新令牌下真实 CLI 完成往返。这验证了分代令牌的可行性。DevDesk 还需要工作轮次内的令牌生命周期、队列唤醒、core 审计及写操作超时核对。`mcode-p0-r6/` 和 `mcode-p0-masked/` 分别保留项目 MCP 执行和保护挂载的证据。

ZCode 证据在 `zcode-p0-terminal/`。取消终态是 `turn.completed` 且 `resultType: "cancelled"`。固定包的 `zcode-runtime.lock.json` 记录 appVersion `3.18.2`、SHA-512 `awE4sRAH1TPBhvLGWoFJxhn5PyL0Ji4kfMf3+YW/w1BvGWJaE4biHaBg7QR/UZNXYPtYPvsuYTdyYgYyFgyMsw==`。这些行为基于该私有协议的固定版本实测。再分发核对来源为 npm 包内 `docs/THIRD_PARTY_CONTENT.md` 及 `vendor/packages/` 的四份技能许可证。

Docker Desktop 的 16 GiB 虚拟盘在早期并行构建时耗尽。清理本轮失败构建生成的缓存后，Pi 用正常 `runtime.prepare` 完成构建。批次 B 的 arm64 实跑使用已核验的 OpenCode 二进制和容器内安装的 npm 包，由只读挂载提供。OpenCode 公共 GitHub 下载配方的完整镜像导出仍需复验。原有容器和本地镜像均保留。

### 共享地基实现

- 配方编译新增 npm 脚本白名单安装和按 `TARGETARCH` 选择校验归档。Codex、DSH 和 Pi 的原 Dockerfile 及配方摘要保持字节一致。实际容器经受信的夹具 HTTPS 测了两种归档选择和 SHA-256 错误拒绝。
- JSONL 应答复用同一帧封装。覆盖 ACP 请求头数组和嵌套 MCP 配置请求头对象的结构化日志脱敏。出厂目录本轮发布范围为批次 A。批次 B 的产品 ACP 适配器、Host MCP 收件箱、权限后端与公开入口仍待轮次绑定和发布验收。
- 接续基线的 `cargo test --workspace --locked` 为 782 项通过、39 项默认忽略。修改后 JSONL 9 项、配置与配方 9 项、Schema 23 项均通过。Clippy、格式、出厂目录、架构和文档检查通过。出厂目录检查使用应用当前 diff 的干净源码导出，排除本机 Finder 元数据。

## 批次 B 产品验收

2026-11-28，`harness-integration`。授权扩容后，Docker Desktop 虚拟盘由 16 GiB 增至 24 GiB。重启前后的三个 Kubernetes 容器名称和镜像一致。Host 为 macOS arm64，运行镜像为 Linux arm64。

- Core 每轮分配独立 RuntimeExecution。ACP 在 `session/prompt` 前将 MCP 令牌绑定到本轮。可归属的早到 HTTP 请求唤醒 worker，同样经过 core 接纳、授权、分派和回执。轮次终态及取消撤销令牌。桥的真实 HTTP 回归覆盖派发前拒绝、早到唤醒、同 ID 重放、同 ID 改参、旧令牌迟到，以及超时后同 ID 回查和新 ID 拒绝。ACP 协议日志省略工具回显的参数、结果和流式文本，core 保留授权工具的正式回执。
- 两个正式镜像均按出厂配方构建，Host 的 `runtime.prepare` 核对镜像身份、配方及断网 ACP v1 协议。OpenCode 使用校验过的 v2 二进制和私有插件，mcode 使用固定 npm 及脚本白名单和私有 Hook。

| Harness | 出厂配方 SHA-256 | Linux arm64 镜像 ID |
|---|---|---|
| OpenCode 2.4.1 | `ae5edc37d2c495e56b3928a6840edf87e21cb81c24fea97f5f1c8be10ffb7eaa` | `sha256:c7349e481c0fb164738b4ae72f794114d306a9a9597082f295abd565fb18fec3` |
| mcode 0.7.9 | `3e932ad0382858125d6e54471ba935a01910b6a7501ae3c4bfa4b87f05db3cbf` | `sha256:e6f805cd581d8b44dfaf6703189b7e0877f65ac6eead2096e6beb3ff43da5f87` |

`tests/entrypoints/pi_harness.py --provider opencode|mcode` 用签名 macOS Host 和实际 Rust 调度器发布并启动两步依赖工作流。两个 Harness 各完成两步。第二步收到第一步的产物并把其中的值交给 `materials.capture`。两步均核对模型收到的工具结果、平台治理回执和会话工具事件。额外运行验证讨论的流式输出和上下文工具、原生写入及私有路径拦截、Host 重启后的续跑、取消后的容器移除，以及项目绑定 HTTP 工具的实收回执和未绑定工具过滤。OpenCode 以冻结的上下文容量触发了一次自动压缩，夹具返回符合 v2 要求的摘要，随后在同一会话完成取消验证。

批次 A 的 v25 副本复验保留 Codex、DSH、Pi 三项安装的配方与镜像身份。批次 B 更新出厂目录并用原 Profile 重绑四个活动权限目标，三段冻结讨论的运行绑定和事件保持一致。旧资产仍要求后继会话，Pi 后继使用原镜像完成一轮。浏览器的模型服务商、设置及并发专项共 8 项通过。

最终 macOS arm64 Host tarball 的 SHA-256 为 `15b552f0ac061c438e38f12122d89512fc6cef11184b62531b8e61e3e7fe8de0`。解包后通过 `codesign --verify --strict`。最终适配器单元测试 123 项、权限集成测试 14 项通过，含历史权限还原、ACP 日志脱敏、早到唤醒及重复请求回执。core、domain、Host 分组共 154 项通过。权限资产、架构、出厂目录、文档及 Web 构建检查通过。

本轮使用独立数据集、受控模型和本地 HTTP 服务。批次 B 的正式镜像在 Docker Desktop 的 Linux arm64 环境实跑。商业模型、软件开发领域的真实 Feature → Task → GitHub PR 交付、原生 Linux amd64 Host 仍需相应环境，单独验收。

### ZCode 门槛核对

- 固定 `zcode-app-cli@3.18.2-48`（Desktop 3.18.2，runtime 0.17.1）。私有 `setting.json` 的 `SessionStart` 和 `PreToolUse` 均留下执行标记，退出码 2 让实际 Bash 写入返回拒绝，工作区未出现目标文件。包内配置文档确认项目还能发现 `zcode.json`、`.zcode/config.json` 和 `.agents/mcp.json`。DevDesk 产品的项目覆盖隔离及冻结策略后端尚待接线。
- `host` 模式下，在断网容器为固定目录中的智谱订阅模型发送账号配置。用受支持的 `reasoningLevel: "high"` 派发后，收到 `interaction/requestProviderRuntimeHeaders`，含 `providerId`、`sessionId`、`turnId` 及 `reason: "model-request"`。返回 `headersApplied: false` 后，运行时结算同轮 `turn.failed` 并保留宿主错误。返回格式正确的夹具认证后，运行时进入模型请求并按网络错误结算。这是智谱内置账号回调的夹具验证。MiniMax M Plan 的 API Key 另有真实工作流回执。
- 原始 npm tarball 的 SHA-256 为 `5908564a9f1529258ff3e4bb0c444b9ee9e1e65814c452bf48fe318002b03501`。包内 `docs/THIRD_PARTY_CONTENT.md` 将文档、PDF、演示文稿和表格技能描述为非商业使用，商业使用要求书面许可。同一 tarball 中四个 `vendor/packages/*/skills/*/LICENSE.txt` 均为字节相同的 Apache-2.0 文本，SHA-256 为 `ea44298d818719ddb3da9b8d56a0cc507e5dab0f4afea57a2da619cd0644d0c5`。公开上游源码许可证也有独立适用范围。预构建镜像的再分发延期，待核对该二进制制品和四个插件的许可及声明要求。[固定版本第三方声明](https://github.com/kingsword09/zcode-cli/blob/v3.18.2-48/docs/THIRD_PARTY_CONTENT.md)

本机证据保存在 `review/harness-batch-b/`。其中 `opencode-release-accepted/` 和 `mcode-final/` 保留最终签名 Host 的工作流、模型审计与工具回执，`v25-copy-fixed/result.json` 记录旧数据副本和 Pi 后继结果，`zcode-account-verified/` 保留 G0 协议和审计。

## 发布条件复验

2026-11-28，验收证据保存在 `review/mixed-harness/release-readiness/`。在 `data` 中保留原项目，实库全局版本 25、项目版本 62，SQLite 快速检查通过。解包后的签名 Host、Web Core 和领域包通过校验、激活、回滚、重启恢复和多 Host 写入隔离检查。

每个 Harness 使用至少一个真实模型完成相关功能验收即可满足模型发布门槛。Pi、DSH、mcode、OpenCode、ZCode 均已使用 `MiniMax-M3.1-Flash` 完成正式工作流岗位。原生 Linux amd64 整链验证另行安排。

- DSH 受限任务在允许网络时也使用宿主包装器。使用固定镜像验证了允许和禁用网络两种配置下的文件读写边界与 `/proc`、`/state`、`/run/devdesk` 隔离。对照探针确认冷启动超过原限额，正式会话和安装探针现共用 DSH 握手时间。对已有的会话接纳失败，核对原 `session.open` 事实和资源版本后允许按原流程重试。
- DSH 包装器更新后，固定的 `tests.run` 和 `tests.project` 可校验升级前冻结的权限合同，并继续用原只读、离线、无凭据测试容器运行。旧版合同仍不能启动 Harness 会话。测试覆盖旧合同下的原权限验证、合同摘要篡改和冻结 Profile 摘要篡改。
- 内容评审草稿可通过 JSON 字符串传给 `spec.lint`。草稿中的行号或字符偏移也可用十进制字符串传输。正式报告仍须使用类型正确的坐标，并经原冻结来源、摘要、范围与引用内容检查。
- 签名 Host 在原项目验证了时间戳变更引起的 Git 误判。源码字节、工作树和远端 SHA 经确认一致，仓库刷新完成。已合入的 Feature 与 Task 另外通过了发布证据审计。[原五 Harness 交付](../mixed-harness-workflow/EXECUTION.md)
- 在 Docker Desktop 的 x86_64 仿真进程中，用发布脚本生成 Linux x86_64 Host 并验证归档摘要。解包二进制和最新源码的交叉编译制品均通过数据集初始化、拒绝不兼容库、v24 到 v25 升级、重跑幂等和重启检查。Pi 的 amd64 配方镜像通过断网 `get_state`，DSH 的 amd64 配方镜像通过读写、隔离和网络边界检查。mcode 的 amd64 镜像通过 MCP 和冻结策略探针，ZCode 的 amd64 镜像通过个人 API key 夹具和项目配置隔离。清理本轮临时制品后，OpenCode 按正式配方构建了 amd64 镜像，完成 ACP、权限、MCP、终态和恢复验证。原生 amd64 Docker Engine 上的完整工作流仍待验收。
- ZCode 固定镜像通过项目配置掩蔽、私有 Hook、宿主 MCP、续跑和账号回调夹具检查。MiniMax M Plan API Key 已用于真实模型验收。DevDesk 可随 Host 发布本地安装配方，预构建镜像的再分发许可待确认。[ZCode 固定制品与许可](../zcode-harness/PLAN.md)

### 现有项目的正式交付复验

首轮在原 `data` 中的 `devdesk-project-1` 跑完跨 Harness Feature `3072b9e7-cfe8-4fc7-c26f-bb9216d05c1d`。OpenCode 和 mcode 使用上述正式镜像，Pi、DSH、ZCode 也分别完成冻结岗位轮次。GPT/Codex 调度 Feature 和 Task，四份人审、实际 Red/Green、两级源提交与合入候选回归均已核对，两个 GitHub PR 已合入。[完整验收与证据](../mixed-harness-workflow/EXECUTION.md#054-与现有项目验收)

最新签名 Host 在同一项目完成 Feature `98d438b8-28fb-9dce-ff97-0d69dc54c156`。GPT/Codex 调度，Pi、DSH、OpenCode、mcode 和 ZCode 均在冻结的岗位上实际执行。Red 出现 8 个预期业务断言失败，Green 使用字节相同的独立测试文件通过 8 项。Task 与 Feature 的源提交和合入候选各通过 53 项实际回归，没有跳过。四项人工确认使用原门禁。一次 PRD 退回和一次 Red 技术恢复保留在历史中，两次计划修订均可核对原冻结版本。[Task PR #44](https://github.com/xfork24/devdesk-project-1/pull/44) [Feature PR #45](https://github.com/xfork24/devdesk-project-1/pull/45)

Feature 最终合入 `6e82c7a10cc6d48f852ab170da463358cd58c972`，等于受测候选、本地工作树和远端 `main` 的 SHA。最新 Host 用相同 SHA 重做了一次时间戳误判回归，仓库刷新成功，源码字节未变。审计记录为 `review/mixed-harness/release-readiness/project-feature/delivery-audit.json`，仓库刷新回执为同一验收目录的 `git-sync-latest-host.json`。
