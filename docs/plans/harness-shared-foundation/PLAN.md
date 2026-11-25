# Harness 扩展共享地基（pi / mcode / zcode / opencode 四条接入线）

日期：2026-11-25。状态：**待实施**。范围：四条接入线共同依赖的 Provider 能力表、协议传输、凭据泛化、宿主侧工具桥、配方生成面与全局库地基。本文件不含任何单一 Harness 的协议细节。

配套计划：[pi 接入](../pi-harness/PLAN.md)、[mcode 接入](../mcode-harness/PLAN.md)、[opencode 接入](../opencode-harness/PLAN.md)、[zcode 接入](../zcode-harness/PLAN.md)。

**本文件是跨计划决策的唯一权威口径**：全局库版本号与 CHECK 取值、终态映射、审批语义、接纳语义、工具桥执行链路、仓库内配置隔离、漂移轮次，一律以本文件为准；各 Harness 计划只引用，不另立口径。任一 Harness 的 P0 结论变化时，先改本文件的地基判断，再改对应 Harness 计划。

## 0. 结论与范围

### 0.1 四份计划

| 计划 | Harness | 驱动协议 | 定位 | 批次 |
|---|---|---|---|---|
| [pi](../pi-harness/PLAN.md) | pi 1.6.0（`@earendil-works/pi-coding-agent`，MIT） | 自有 `--mode rpc`（JSON Lines，无握手） | 调研最完整、风险最低；**单独先行** | A |
| [mcode](../mcode-harness/PLAN.md) | MiniMax Code CLI 0.7.9（`@minimax-ai/code`，MIT） | **ACP 标准协议**（`mcode acp`，常驻 stdio） | 批次 B 先行；Linux 原生依赖有风险，P0 判定成立性 | B |
| [opencode](../opencode-harness/PLAN.md) | opencode CLI 2.4.1（MIT，Bun 单体） | **ACP 标准协议**（`opencode acp`，常驻 stdio） | 与 mcode 共用 ACP 适配器；许可最干净；需二进制安装面 | B |
| [zcode](../zcode-harness/PLAN.md) | ZCode CLI（`zcode-app-cli`，MIT 启动器 + 随包专有运行时） | **ZCode Protocol**（`zcode app-server`，常驻 stdio） | 协议未公开文档化，整体以 P0 实跑结论为门 | B（P0 通过才进） |

### 0.2 可复用性判定

pi 计划的方法**结构上完全可复用**：即「编译期 provider 注册 + 声明式 Harness JSON + 镜像配方 + 权限后端资产 + 凭据链路 + Schema/全局库升级」这套骨架照搬；但**协议适配层与工具桥必须换实现**，因为四个 CLI 的扩展面互不相同：

| 维度 | pi 1.6.0 | mcode 0.7.9 | opencode 2.4.1 | zcode 0.17.1 / 3.18.2-48 |
|---|---|---|---|---|
| 打包 | npm `@earendil-works/pi-coding-agent` | npm `@minimax-ai/code`（MIT） | GitHub 发行二进制（MIT，Bun 单体）；npm `opencode-ai` 仅 v1 线 | npm `zcode-app-cli`（MIT 启动器，随包专有运行时） |
| 驱动协议 | 自有 `--mode rpc`（JSON Lines，无握手） | **ACP 标准协议**（JSON-RPC 2.0） | **ACP 标准协议**（JSON-RPC 2.0） | **ZCode Protocol**（自有 `{id,method,params}`，无 `jsonrpc`） |
| 会话/轮次映射 | `prompt` → `agent_settled` | `session/new`→`session/prompt`→`stopReason` | 同 mcode | `session/create`→`session/send`→`session/event` |
| 接纳凭证 | `prompt` 立即回执（`disposition`） | `session/prompt` 应答在**轮次末**才到 → 宿主按首个 provider 证据合成 | 同 mcode | 未文档化，P0 确认应答时机 |
| 恢复方式 | `--session-id <id>` | 优先 `session/resume`；`session/load` 会**重放整段历史** | 同 mcode | `session/resume` |
| 指令注入通道 | `--append-system-prompt <file>` | `mcode acp --append-system-prompt-file`，或 ACP `systemPrompt` | `acp` 子命令无系统提示开关 → ACP `systemPrompt` 或配置 `instructions`（P0 定） | P0 确认 |
| 审批通道 | `extension_ui_request(select)` | `session/request_permission` | `session/request_permission` | `interaction/requestPermission` + `interaction/respond` |
| 容器内策略钩子 | `pi.on("tool_call")` 扩展 | `PreToolUse` / `PermissionRequest` 生命周期 Hook | 插件钩子 `tool.execute.before` + `permission.ask` | `PreToolUse` / `PermissionRequest` 生命周期 Hook |
| 步骤工具交付 | 扩展 `registerTool` + `ctx.ui.input` 隧道 | **无进程内注册通道** → 宿主 MCP 桥 | 同 mcode（有插件工具，但统一走宿主 MCP） | **无进程内注册通道** → 宿主 MCP 桥 |
| 仓库内配置隔离 | 启动开关全关（已核实） | P0 确认 | 二进制内有 `OPENCODE_DISABLE_PROJECT_CONFIG` 等开关 | P0 确认 |
| 凭据所有权 | `models.json` + provider env | provider 配置 + API key | `opencode.json` 的 `provider.options`（宿主写入） | `ZCODE_APP_SERVER_AUTH_MODE=host`（宿主供凭据） |

### 0.3 关键设计决策

1. 一份计划覆盖四个 Harness 的公共地基；**mcode 先行、opencode 次之**；zcode 以 **P0 实跑结论为门**（协议未文档化，未跑通不进入 P1）。
2. mcode 驱动方式用 **`mcode acp`**（ACP 标准协议，常驻进程），不采用一次性 `exec --output-format stream-json`。
3. zcode 走 **`zcode app-server`** 协议路线纳入实施（若 P0 证伪则整体回退，见 zcode 计划的 P0 门控）。
4. opencode 驱动方式用 **`opencode acp`**，**与 mcode 共用同一个 ACP 适配器模块**（`harness_acp.rs`），差异收敛到一份「ACP 方言」配置（见 2.2.6）；v2 HTTP/SSE API 作为文档化备选路线记录在 opencode 计划。
5. **workflow 步骤工具（dynamic tools）一期必须打通**：mcode、opencode、zcode 都没有可用的「进程内注册宿主工具」通道，统一走 **宿主侧 MCP 服务端**（streamable HTTP，带会话令牌）。**pi 保持已确认的 UI 隧道方案**（用户已决策，不因本计划改变）。
6. **统一执行口径：所有宿主代执行工具调用都经 core 接纳/审计链路**。桥与隧道都只负责传输与寻址，**不执行工具、不自造审批、不旁路治理记录**；工具调用在会话记录中可见、可审计、可幂等重放。
7. **审批语义以 core 现状为准**（本轮评审更正，见 2.4.5）：`approval` 策略只作用于 **provider 自己发起的审批请求**；宿主代执行工具的准入由 core 按冻结权限判定，**适配器与桥不得为其发明审批流**。
8. **接纳语义**：既有骨架把「对宿主请求的应答帧」当作接纳凭证。ACP 与 ZCode 的轮次应答可能到轮次末才回，因此这两类协议改为 **首个 provider 证据到达时合成接纳**（见 2.2.7）。
9. **Provider 能力表替代字面量比较**（见 2.1）：现有 `== Codex` / `== Dsh` 判断多数表达的是能力而非厂商，六个 provider 并存前必须收敛成一张表。
10. **终态映射统一**（见第 6 章）：输出被截断（`max_tokens` / `length`）一律判 Failed，与 DSH 的 `DSH_MAX_TOKENS_AS_SUCCESS=false`、pi 的口径一致。
11. **策略装载证明（fail-closed）**（见 2.1.7）：冻结策略在容器内「静默不生效」是最危险失败模式，必须有可验证的装载证明。
12. **配方生成面扩展必须对既有配方字节稳定**（见 2.5.6）：不改变 codex/dsh/pi 的 `recipe_digest`。

### 0.4 重要非目标

- 不引入根编排（四条线与 DSH 一致，不获得 `orchestration` 能力）。
- 一期不交付 Agent 提问：适配器对 `elicitation/create`、`interaction/requestUserInput` **直接应答取消**，不产生 `Obs::Interaction`，并写入各 Harness 的 `limitations`。
- 不做「容器内工具禁网」执行器：四条线 network 能力均只声明 `enabled`，harness 层 `disabled_tools` 在校验阶段直接拒绝。
- 不合并 pi 的 UI 隧道与宿主 MCP 桥的实现：两者是**同一条 core 接纳/审计链路的两个传输**，差别只在传输与请求判别式（见 2.4.10）。合并列入后续演进。

### 0.5 交付形态与实施顺序

**阶段 G0：P0 批次前置（四条线并行，只读探针，不写生产代码）。** 各线的 P0 都是「手工构建镜像 + 最小协议闭环」级别的验证，先全部跑完再动生产代码，避免在 pi 全部落地之后才发现 mcode 不成立、opencode 需要另一种安装面。产出：各线 P0 结论回写计划，并修订本文件的地基判断（尤其是 mcode 是否成立、zcode 是否进门、宿主桥连通性、MCP 工具名命名空间、策略钩子发现路径、opencode 指令注入通道）。通用 P0 清单见第 7 章。

**批次 A（pi 单独落地）：pi P1–P4。** 其中**与厂商无关的通用管线由 pi 首次落地**，批次 B 只做增量：

| 通用管线 | 首次落地 | 批次 B 的增量 |
|---|---|---|
| 凭据泛化（`credentials.{harness}.{provider}`） | pi P1 | 加入 mcode/zcode/opencode 的账户前缀 |
| 全局库 v25 + `storage upgrade-global` + 启动版本检查 | pi P1 | **无**（CHECK 在批次 A 即写入全部六个 provider） |
| Schema 枚举机制（`rpc.schema.json`、`permission-backend.schema.json`） | pi P1 | 逐处追加三个取值 |
| 按 harness 渲染的服务商设置页、`model_profile_digest` 按 provider 分派 | pi P1 | 接入三个新 harness |
| Provider 能力表（2.1） | pi P1 建表并迁移现有判断 | 每个新 provider 增加一行 |

**批次 B（三条新线合并落地）：本文件剩余项 + mcode → opencode → zcode（仅 P0 通过者）。** 三条线的权限后端与资产必须**在同一次变更中登记**，合并为一次漂移。

**漂移轮次合计 = 2**（批次 A 一次、批次 B 一次）。若 zcode 的 P0 通过但实施推迟到批次 B 之后，会多出第三次漂移 → 在 G0 末尾一次性决定 zcode 的批次归属。

### 0.6 本次工作约束

调研为只读（源码阅读 + 本机已安装包的静态分析 + npm registry / GitHub release 元数据 + ACP 官方规范），**未实跑任何 harness 协议**——凡未实跑验证的行为在各 Harness 计划中均标注为 P0 待验。凭据、实际运行数据、日志与本地复核材料留在版本库外。

## 1. DevDesk 现状（受影响组件）

- Harness = 编译期驱动（`HarnessProvider` 枚举，`crates/domain/src/runtime.rs:9`，`HARNESS_PROVIDERS` 注册表）+ 声明式配置（`plugins/core/harnesses/*.json`，`data.provider`）。当前仅 `codex`、`dsh`。
- 协议适配集中在 `crates/adapters/src/harness.rs`（约 2325 行，单文件按 provider 分支）+ 传输层 `jsonl.rs`（JSONL 帧、`rpc2()` 开关决定是否加 `jsonrpc:"2.0"`、`Journal` 脱敏与合批）+ 权限编译 `permissions.rs`/`permission_profiles.rs`。
- 既有 provider 差异已固化在代码里（约 25 处 `== Codex` / `== Dsh`）：`harness.rs`（模型连接、stop 模式名、应答形状、历史复用、命令输出导出）、`core/src/session_assembly.rs`、`domain/src/conversation.rs`、`core/src/credential_updates.rs`、`adapters/src/credentials.rs`、`core/src/conversation_observations.rs`、`core/src/provider_tools.rs`。
- 动态工具既有两条投递路径：Codex 走 `thread/start` 的 `dynamicTools`（`harness.rs:740`），DSH 走扩展 `dynamicTools` + `contextBridge`（`harness.rs:610`）。两者都要求「宿主在协议通道上收工具调用并回结果」。
- **轮询是唤醒驱动的**：`JsonlProcess::managed_with_wakeup` 在每帧入队后调用 `RuntimeWorkers.wakeups.notify()`，Host 经 `subscribe_wakeup` 驱动后台调度（`crates/host/src/workspace.rs:266`）。任何**不来自协议帧**的观察来源都必须触发同一唤醒通道。
- **宿主代执行工具的审批语义**（`crates/core/src/platform_observations.rs:351-366`）：平台写操作要求 `approval == Never` 且文件系统非只读才直接执行；`on-request` 下 core 直接拒绝（「此权限不能直接执行平台写操作」）。**不存在「宿主代执行工具走人工审批」的流程**。
- **Harness 工作容器恒为联网容器**（`harness.rs:648` `network: true`）。`--network none` 只用于测试与工具容器；沙箱层 `Disabled` 被 `validate_execution_permissions` 拒绝（会阻断模型连接）。
- 容器→宿主连通性已有先例：`ContainerSpec.host_gateway` → `docker.rs` 注入 `--add-host host.docker.internal:host-gateway`，并要求 Docker context 为 unix socket（`docker.rs:620-648`）。
- DevDesk 目前只有 **MCP 客户端**（容器内 `crates/adapters/runtime/tool-worker.mjs` 实现 `initialize`/`tools/list`/`tools/call`，同时支持协议版本 `2025-11-25` 与 `2026-07-28`），**没有 MCP 服务端**。→ 本地基新增的第一个新能力面就是宿主侧 MCP 服务端。
- **crate 依赖边界**：`crates/adapters` 没有 async 与 HTTP 依赖（只有 rusqlite、sha2 等）；axum 与 tokio 只在 `crates/host`。桥的 HTTP 监听因此必须落在 host。
- **出厂目录更新是显式命令**：`configuration.templates.reset`（`preserve_custom: true` → `update_factory_catalog`，`crates/core/src/templates.rs:309`）。Host 启动时**不会**把新出厂源自动并入现有数据集。
- 全局库：`sql/global/schema.sql` 的 `runtime_installations.provider` CHECK 为 `('codex','dsh')`，`PRAGMA user_version=24`；模型服务商公开 id 必须全局唯一（`crates/core/src/configuration.rs:366` `validate_model_provider`）；凭据引用的账户必须以服务商的 `credential_account` 为前缀（`credentials.rs` `account_for_prefix`）。
- 权限后端资产清单：`permissions/backends/manifest.json`（后端 + 资产 sha256）。新增后端会让 codex/dsh 的 `enforcement_contract` 漂移、冻结会话 `permission_asset_mismatch`。
- 现有数据集事实：`data/`（v24，3 条运行安装）、`data1/`（v24，32 条源，无安装记录、无权限绑定 → 近空库，适合先演练升级）、`devdesk-recovery/20261119-large-snapshot/`（快照）。只读打开 WAL 库需要 `-shm` 文件，`data1` 直接只读打开报错 14 即此原因，并非损坏。

## 2. 共享地基工作项

### 2.1 Provider 注册、能力表与显式分支

1. `crates/domain/src/runtime.rs`：新增 `HarnessProvider::Pi`、`Mcode`、`Zcode`、`Opencode`，并入 `HARNESS_PROVIDERS`（provider 门禁只读该注册表，禁止散落字面量）。
2. 遍历 provider 的测试改用 `HARNESS_PROVIDERS`（`harness.rs`、`permission_profiles.rs`、`tests/{permissions,schema_contracts,permission_profiles}.rs`）。
3. **能力表**：在 `crates/domain/src/runtime.rs` 增加 `HarnessProvider::traits(self) -> HarnessTraits`（纯数据、`const` 可求值），把现有 `== Codex` / `== Dsh` 判断逐处替换为能力判断：

   | 能力项 | Codex | DSH | pi | mcode | opencode | zcode | 取代的现有判断 |
   |---|---|---|---|---|---|---|---|
   | `identity` 输入身份 | ProviderTurn | ProviderMessage | HostTurn | HostTurn | HostTurn | HostTurn（P0） | `conversation_observations.rs:358,395`、`provider_tools.rs:712` |
   | `admission` 接纳凭证 | OnResponse | OnResponse | OnResponse | OnFirstEvidence | OnFirstEvidence | OnFirstEvidence（P0） | `harness.rs:1033-1058` |
   | `owns_history` 恢复时 provider 自持历史 | 是 | 否 | 否 | 否 | 否 | 否 | `harness.rs:401` |
   | `streams` 输出流式增量 | 是 | 否 | 是 | 是 | 是 | 是（P0） | `harness.rs:1148` 起的投影分支 |
   | `interrupt` 协议中断 | Protocol | ContainerOnly | Protocol | Protocol | Protocol | Protocol（P0） | `harness.rs:882-911` |
   | `host_model_route` 宿主下发模型路由 | 否 | 是 | 是 | 是 | 是 | 是 | `harness.rs:448`、`session_assembly.rs:431`、`conversation.rs:474`、凭据链路 |
   | `tool_transport` 步骤工具传输 | ProtocolRequest | ProtocolRequest | UiDialog | HostMcp | HostMcp | HostMcp | `harness.rs:1088`、`:1444` |
   | `command_output` 原生命令输出导出 | 是 | 否 | 否 | 否 | 否 | 否 | `harness.rs:426` |
   | `orchestration` 根编排 | 是 | 否 | 否 | 否 | 否 | 否 | `harness.rs:2017` |

   说明：`IdentityStyle::ProviderMessage` 仅 DSH 使用，替换后行为不变；`HostTurn` 表示「宿主生成轮次 id 并在接纳时写入 turn 记录」。zcode 的四项取值待其 P0 确认。
4. **模块布局**：`harness.rs` 只保留共享的 worker、容器创建、停止围栏与路由；新增协议代码一律进各自模块，不再向 `harness.rs` 增加协议细节：

   ```
   crates/adapters/src/
     harness.rs          共享骨架 + 按 traits 的分派（既有 Codex/DSH 分支本期不强制迁出）
     harness_pi.rs       pi RPC
     harness_zcode.rs    ZCode Protocol
     harness_acp.rs      ACP 共用适配器（无 provider 分支）
     harness_mcode.rs / harness_opencode.rs   ACP 方言配置 + 各自启动/模型/策略接线
     harness_bridge.rs   宿主 MCP 桥的 adapters 侧：每会话收件箱、Obs::ToolCall 产出、answer 路由
   ```

   全部新文件纳入 `managed_adapter_identity` 的哈希输入。
5. **协议合同命名**：`managed_adapter_contract` 分别为 `pi-rpc/v1`、`mcode-acp/v1`、`opencode-acp/v1`、`zcode-protocol/v1`（ACP 适配器共用，但合同按 provider 命名，便于单线升级）。
6. **能力登记随 P0 结论而定**：`driver_capability_registry` 只在该 provider 的传输被 P0 证实后才登记 `dynamic-tools`、`external-tools`、`platform-tools`；未证实者不登记，其 Harness 源也不得声明对应能力（`validate_harness` 会拒绝）。
7. **策略装载证明（fail-closed）**：每次打开会话，Host 在发送首轮输入前必须确信冻结策略已在容器内生效，拿不到即终止会话并报 `policy_not_enforced`，**不得降级运行**。证明分两级：
   - **会话级信号**（能做到的优先做）：pi 的 DevDesk 扩展在装载时发一条 `extension_ui_request{method:"notify", message:"devdesk.policy.loaded.v1"}`，Host 在初始化阶段必须观察到它。
   - **配置独占 + 镜像探针**（会话级信号不可得时的最低要求）：Harness 的全部配置来源由 Host 独占生成（会话私有的 `/state/<harness>`、仓库内配置发现已关闭或屏蔽、环境变量钉死），并在 `verify_runtime_protocol` 的镜像探针中加入一次「策略生效」验证，保证同一镜像每次准备都复核。
8. **仓库内运行时配置隔离**：指令类文件（`AGENTS.md`）与 Codex 现状同等对待（作为不可信输入读取）；**可执行配置**（项目级插件、钩子、MCP 服务、权限规则）必须用启动开关关闭，关不掉的路径加入 `protected_metadata`。`protected_metadata` 是 `/work` 下的叶子名：目录挂空目录、文件挂空文件（`docker.rs:996-1025`），白名单硬编码在三处，新增条目三处同步：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`。

   | Harness | 仓库内会被读取的可执行配置 | 一期处置 |
   |---|---|---|
   | pi | `.pi/{settings.json,mcp.json,extensions,skills,prompts,themes}`、`.agents/skills` | 已由启动开关全关（`-na -ne -ns -np --no-mcp`），**无需**加入 `protected_metadata`（pi 1.6.0 也不向项目目录写入） |
   | mcode | 项目级 `hooks/` 等（发现路径未确认） | P0 确认；关不掉则加入 `protected_metadata` |
   | opencode | `.opencode/`、`opencode.json` | 优先 `OPENCODE_DISABLE_PROJECT_CONFIG=1`（二进制内可见），P0 验证语义；不足再屏蔽 |
   | zcode | 项目级 hooks、`.zcode/` | P0 确认；关不掉则加入 `protected_metadata` |

### 2.2 传输层扩展（新增 ACP 与 ZCode 两种线协议形态）

**现状更正**：`jsonl.rs` 本身不区分方向，`JsonlProcess::send` 可发任意帧；「入站请求应答」其实**已经存在**——`harness.rs` 的 `project()` 对带 `id` 且带 `method` 的帧分派（`item/tool/call` → `Obs::ToolCall`，其余 → `interaction()` 处理审批与提问），并用 `wire.send({"id":…,"result":…})` 回帧。真正缺的是**线协议形态**与**应答形状的多样化**，而不是从零造一条通道。

1. 线协议模式（新增两种，与既有 Codex 无 `jsonrpc`、DSH 带 `jsonrpc` 并列）：
   - **ACP**（mcode 与 opencode 共用）：`jsonrpc:"2.0"` 必填（复用现有 `rpc2()` 开关）；请求/应答按 `id` 匹配；通知按 `method` 识别。
   - **ZCode Protocol**：**不得**加 `jsonrpc` 字段；帧为 `{id, method, params}` → `{id, result|error}`；通知为 `{method:"session/event", params}`。
2. 入站请求应答的形状**按 provider 分派**：ACP 审批应答为 `{"outcome":{"outcome":"selected","optionId":…}}`、取消为 `{"outcome":{"outcome":"cancelled"}}`；ZCode 为 `interaction/respond{type:"respond_permission",…}`。在 `jsonl.rs` 提供语义化的 `respond(id, result)` / `respond_error(id, code, message)` 封装，而不是让各 provider 直接拼帧。
3. **外部请求 id 的字符串化与还原**：`Obs::Interaction.request` 是 `String`，而 ACP/ZCode 的入站 `id` 是数字 → 适配器负责字符串化（`"<provider>:<id>"` 以免不同 provider 的 id 空间碰撞），`answer_interaction` 侧再反解回原始 id 回帧。
4. **等待应答期间的入站帧处置**：`JsonlProcess::request_for` 在 `capture == false` 时会把任何带 `id`+`method` 的帧**自动回 `-32601`**（`jsonl.rs:558-563`）。Harness worker 必须始终保持 `capture == true`（`journal` 存在即为 true），否则 ACP 的 `session/request_permission` 会被误答。
5. `Journal` 脱敏补强——新增「按结构脱敏」规则：凡 `headers` 数组/对象中 `name`/键命中 `authorization|token|api[-_]?key|secret` 的条目，其值必须替换为 `[redacted]`（宿主 MCP 桥令牌会以 `headers` 形式出现在 `session/new` 与 settings 写入参数里，现有按键名白名单挡不住）。三种载体形状都要覆盖：ACP 的 `headers:[{name,value}]` 数组、ZCode/opencode 的 `headers:{k:v}` 对象、以及 `mcp.servers` 下的多层嵌套。
6. **ACP 方言**：`harness_acp.rs` 不含 provider 分支，差异全部来自一份 `AcpDialect` 配置：恢复方式（`session/resume` 优先）、MCP 注入方式（`session/new.mcpServers` 或配置文件）、指令注入方式（CLI 开关或 `session/new.systemPrompt`）、权限选项映射、工具事件归一。`initialize` 应答的 `agentCapabilities` 必须逐项核对并据此选择路径：`mcpCapabilities.http` 为假则步骤工具不可用（直接给出明确原因）；`sessionCapabilities.resume` 缺失才退回 `session/load`。
7. **ACP 恢复与历史重放**：ACP 规范要求 `session/load` 在应答前把整段对话以 `session/update` **重放**给客户端（`agentclientprotocol.com/protocol/v1/session-setup`）。DevDesk 自持会话记录，不需要重放；而现有初始化缓冲上限 256 帧（「初始化事件超出限制」），长会话恢复必然失败。→ **优先使用 `session/resume`（不重放）**；不得不使用 `session/load` 时，必须在等待其应答期间**有界地丢弃**重放的 `session/update`（不进入观察、不占初始化缓冲）。
8. **宿主合成接纳**：`admission == OnFirstEvidence` 的 provider 在派发时生成轮次 id 存入 Activity，并在**首个 provider 证据**到达的那次 `poll()` 里先产出 `Obs::Admitted{external_turn: 该 id}` 再处理证据。「首个证据」= 该轮次的第一条通知，或轮次应答本身（空轮次）。若在证据之前先收到轮次请求的**错误应答**，判 `provider_rejected`，不计为已应用。详见 mcode 计划 §2.2。
9. **唤醒**：任何非协议帧来源的观察（桥收件箱）入队时必须调用 `RuntimeWorkers.wakeups.notify()`，否则 provider 在等待结果期间不产生协议帧，worker 不会被轮询。
10. **私有请求**：zcode 的 `interaction/requestProviderRuntimeHeaders` 应答含 API key → 与 Codex 的 `account/`、`auth/` 同等登记为私有请求（不落日志）；桥令牌与 API key 一律进入 `JsonlProcess` 的 secrets 列表。

### 2.3 凭据链路泛化（API key 型 Harness）

1. 把写死的 `dsh` 泛化为「API key Harness」集合：绑定键 `credentials.{harness}.{provider}`（dsh 现有键名保持不变），涉及 `crates/core/src/credential_updates.rs`、`crates/host/src/model_providers.rs`、`crates/adapters/src/{credentials,catalog,credentials_macos}.rs`。
2. macOS 钥匙串账户前缀白名单（`credentials_macos.rs:47-52`）加入 pi、mcode、zcode、opencode 各自的服务商前缀；`credentials.rs` 的单元测试表同步。
3. 服务商公开 id 全局唯一（`validate_model_provider`）→ 新建 `pi-*`、`mcode-*`、`zcode-*`、`opencode-*` 前缀的 provider 源与 Model 源（不复用 dsh 的 id，避免跨 Harness 语义混淆）。
4. **已知使用成本**：同一厂商的 API key 需要为每个 Harness 各保存一次（凭据引用按 `(Harness, 服务商)` 隔离，与 dsh 现状一致）。一期在设置页如实提示；跨 Harness 一键复用需要扩展 `credentials.configure` 合同与钥匙串账户命名空间，列入后续演进（第 4 章 #20）。

### 2.4 宿主侧 MCP 服务端（新增能力面，mcode / opencode / zcode 共用，一期必备）

这是「workflow 步骤工具一期必须打通」的落地物，也是本计划相对 pi 计划最大的新增面。

1. **端点**：Host 新增一个**独立监听器**（不注册到主 API 路由器，避免把整个宿主 API 暴露给容器），仅在存在需要宿主桥的会话时启动；路径含会话 id（如 `/internal/tool-bridge/<session>/mcp`）。监听器落在 host（axum + tokio），经 `ports::ToolBridge`（新增的**传输接口**，不是 core 接纳入口）把调用交给 adapters 的 worker 收件箱。
2. **鉴权**：每会话随机令牌（≥32 字节），以 `Authorization: Bearer <token>` 传递；worker 只保存令牌摘要并做常数时间比较。
   - **令牌生命周期 = worker 操作生命周期**：worker 停止或 `cleanup_worker` 时撤销；Host 关闭时全部撤销。令牌只写进容器内进程可见的会话级配置（`write_private`，0600），**不进仓库、不进日志**。
   - opencode 与 zcode 的含令牌配置文件位于宿主侧的会话状态目录（`/state/<harness>` 的绑定挂载），会话结束后残留在磁盘 → 清理时必须覆盖或删除，并由撤销保证残留令牌不可用。
3. **协议**：实现 MCP `initialize`、`notifications/initialized`、`ping`、`tools/list`（含 cursor 分页）、`tools/call`，并处理 `notifications/cancelled` 与 `DELETE`；响应一律 JSON（不开 SSE 流）。协议版本集合与 `crates/adapters/runtime/tool-worker.mjs` 保持一致（`2025-11-25` 的 `Mcp-Session-Id` 头与 `2026-07-28` 的 `_meta` 版本协商）。其余方法与未带令牌的请求一律拒绝。校验 `Host` 头；请求体上限与协议帧一致（1 MiB）。
4. **工具集合**：`tools/list` 返回该会话**冻结**的动态工具集（来源同 `harness.rs:1718 dynamic_tools(binding)`：集成绑定工具、未绑定工具、平台工具、上下文工具，且按冻结 `tool_refs` 过滤）。**工具名注册表**：集成/未绑定工具用 `definition.qualified_name`，平台工具与上下文工具用 `definition.call_name()`（core 侧也按这两种名字分别匹配：`conversation_observations.rs:410-418` 用 `qualified_name`，`platform_observations.rs:46-55` 用 `call_name()`）。桥侧保留 `MCP 暴露名 → (种类, 冻结引用, 原名)` 的反查表；`qualified_name` 与 `call_name` 撞名且无法用冻结引用消歧的条目**不暴露**。未在冻结集合内的名字一律拒绝。
   - **MCP 工具名约束**：暴露名必须满足 `^[A-Za-z0-9_-]{1,64}$`（多数客户端与模型提供方的共同约束）；原名含其他字符时做确定性映射（`.` → `_` 等），`tools/call.name` 经反查表还原为原名后再产出观察。MCP 服务名取短名（建议 `devdesk`）。
   - **客户端命名空间化**（P0 必测）：各 harness 会给 MCP 工具加服务器前缀后再暴露给模型（pi 文档即有 `mcp__<server>__<tool>` 形态），模型可见名与 DevDesk 指令文本里的工具名可能不一致。P0 记录每条线的模型可见名与长度；指令文本对工具名的引用改为「按前缀识别」的措辞，或在 `tools/list` 的 description 中说明。
   - **一期暴露范围：四类全暴露**（含平台工具）。桥产出观察时 `tool` 取**原名**，core 已有按名分派：命中集成工具 `qualified_name` 的走常规接纳（`admit_provider_observations`），其余（平台工具）自动走平台准入路径（`admit_platform_provider_observation`，`conversation_runtime.rs:334-345`）→ 桥**不需要**新增 grant 路由或新端口。
   - **平台工具的可调用前提（必须写进 `limitations`）**：平台调用要么有 **controller 授权**（`poll.authority.controller` 存在且有效，`platform_observations.rs:140-194`），要么有 **workflow 执行归属**（`poll.authority.workflow` 存在，`platform_observations.rs:263-290`）；两者皆无的普通讨论会话调用平台工具会被 core 以 `tool_not_authorized` 拒绝。桥把 core 的拒绝原因原样返回（MCP `isError` + 可读原因），不能让模型反复重试。
5. **权限与审批（更正后的口径）**：
   - **provider 自己发起的审批请求**（`session/request_permission`、`interaction/requestPermission`、pi 的 `devdesk.approval.v1`）走既有 `Obs::Interaction{ToolApproval}` 流；冻结审批策略为 `never` 时适配器**自动拒绝、不弹窗**（与 Codex `interaction()` 的自动 decline 同构）。允许时一律选 `kind == allow_once` 的选项，**永不选 `allow_always`**。
   - **宿主代执行工具**（桥调用与 pi 隧道调用）**不设适配器层审批**：准入完全由 core 按冻结权限判定。平台写操作在 `approval == never` 且文件系统非只读时直接执行；`on-request` 下 core 拒绝（`platform_observations.rs:361-365`）。桥把拒绝原样返回。
   - 两条通道落在同一份冻结权限上，UI 上区分来源即可。
6. **容器连通性**：`docker.rs` 为 Harness worker 增加独立开关（与既有 `host_gateway` 解耦，但共用同一条 `--add-host host.docker.internal:host-gateway` 注入与同一条 Docker context 检查）。
   - **更正**：此前版本写「冻结网络为 Disabled 的 Profile 下容器为 `--network none`，宿主桥物理不可达」——该前提**不成立**。Harness 工作容器恒为 `network: true`；沙箱层禁网本就被现有校验拒绝；harness 层 `disabled_tools` 四条线均不支持并在校验阶段直接拒绝。因此不存在「禁网 Profile 下的步骤工具」这一场景，相关 `limitations` 文案改为「不支持 harness 层工具禁网」。
   - 监听地址需覆盖两种宿主形态，**P0 实测确定**：macOS（Docker Desktop 的 `host.docker.internal` 可达宿主 loopback → 绑定 `127.0.0.1`）与 Linux（`host-gateway` 指向网桥网关 → 必须绑定该网桥地址而非 loopback，且不得绑定 `0.0.0.0`）。
   - **远程 Docker daemon 不支持宿主桥**：`DOCKER_HOST` 非 unix socket 时容器无法到达 DevDesk 宿主，与 `host_gateway` 的既有检查一致。会话存在需要宿主桥的动态工具时在打开阶段**直接拒绝并给出明确原因**。
7. **验收底线**：容器内只能访问该桥端点与被授权的外部地址；桥令牌泄漏后撤销即失效；桥发起的工具调用必须与 provider 发起的工具调用**同等可审计**（第 9 条）。
8. **统一执行口径：桥调用一律经 core 接纳/审计链路**——桥只负责**传输与寻址**，不执行工具、不自造审批、不旁路治理记录。链路与 Codex/DSH 同构：

   1. **投递**：MCP 服务端校验令牌与会话 → 生成 `call_id` → 经 `ports::ToolBridge` 把调用投递到该会话 worker 的**桥收件箱**（有界，超限报错）→ **触发 `wakeups.notify()`** → HTTP 处理器等待回执（超时 = 工具执行预算 + 余量）。
   2. **观察**：worker 的 `poll()` 在抽取协议帧的同时抽取桥收件箱 → 产出与既有形状**完全一致**的观察：
      `Obs::ToolCall{turn, request:{"kind":"bridge","call_id":"<id>"}, thread:<会话 external_session>, external_turn:<轮次的宿主生成 id>, call:<call_id>, tool:<原名>, arguments:<MCP arguments>, worktree}`。
      `request` 是 `Value`（`ports.rs:220`），用 `kind` 判别传输来源。
   3. **接纳**：core 走既有链路——`validate_provider_call_scope`（`conversation_observations.rs:338-369`）、冻结工具解析、`provider_tool_intent` 治理记录。越权工具、跨轮次调用、已停止轮次一律在这里被拒。
   4. **执行**：core 的工具调用驱动器（`crates/core/src/integrations.rs:365-440`）经 `tool_executor.start/poll` 执行（`ports.rs:303-315`，实现是 adapters 的 `NativeTools`），产出 `ToolReceipt`。
   5. **回执**：core 调 `harness.answer_tool_call(session, request, &receipt)` → 适配器识别 `request.kind == "bridge"` → 交回桥的 pending 表 → 唤醒 HTTP 处理器 → MCP 结果：`content` 中 text/image 直接映射、其余类型序列化为 text，`structuredContent` = `structured_content`，`isError = !success` 且把 `error` 文本放入 content。
   6. **幂等与审计**：意图持久化 + `replay_commit` 指纹 → 同一 `(session, turn, worker, call_id)` 重放不重复执行；工具调用进入会话记录（`ConversationEventKind::Tool`）与治理记录。**幂等范围需如实表述**：core 层对同一 `call_id` 的重放不重复执行；HTTP 层的客户端重试是新的 `call_id`，桥只对「同会话、同参数指纹、仍在途」的调用做合并等待，不承诺跨重试的恰好一次。

   - **不新增 core 端口**：桥的观察由 **worker（适配器）** 产出，仍是合法的 `Obs` 生产者；回执复用既有 `Harness::answer_tool_call`，只是 `request` 的判别式不同。
   - **线程模型**：MCP HTTP 处理器阻塞在等待回执上；core 的轮询循环独立运行（provider 侧仍在等待 MCP 结果，轮次保持 `running`）→ 不产生死锁。前提是入队即唤醒（第 1 步）。
   - **撤销与超时**：会话撤销、轮次取消或观察到终态 → 所有在途桥调用立即失败并向容器返回 `isError`；回执超时未达 → MCP 返回明确的可重试错误，**不得**在宿主侧重复执行。
   - **资源上限**：每会话在途桥调用上限（建议 8）、全局并发上限、HTTP 读超时与空闲超时均为显式常量。
9. **桥的验收判据（三个计划共用，缺一不可）**：
   1. **往返**：容器内一次 `tools/call` 能拿到 MCP 结果，且宿主侧产生 `ToolReceipt`。
   2. **可审计**：该次工具调用出现在**会话记录**中，且存在对应的 `provider_tool_intent` 治理记录。
   3. **受冻结权限约束**：不在冻结 `tool_refs` 内的工具名被拒；只读 Profile 下写入类工具被拒。
   4. **幂等**：同一 `call_id` 重放不产生第二次执行。
   5. **撤销**：撤销会话令牌后，桥调用立即失败。
   6. **审批口径正确**：`approval == never` 的默认 Profile 下步骤工具**正常执行**（不得被拒）；`on-request` 下平台写操作被 core 拒绝且原因可读、不弹审批窗。
   7. **脱敏**：桥令牌与工具参数不出现在协议日志与桥日志中（2.2.5）。
   8. **平台工具**：在有 controller 授权或 workflow 执行归属的会话里，平台工具调用被正确路由到平台准入路径并完成；在两者皆无的普通讨论会话里被**明确拒绝**（`tool_not_authorized` 语义），模型不反复重试。
   9. **唤醒**：provider 在等待 MCP 结果期间不产生任何协议帧时，桥调用仍在一个调度周期内被处理（不得依赖任何协议帧的到来）。
10. **传输对照与演进**：

   | | Codex / DSH | pi | mcode / opencode / zcode |
   |---|---|---|---|
   | 请求到达 | 协议线上的 JSON-RPC 请求 | 协议线上的 `extension_ui_request` 对话框 | HTTP MCP `tools/call` → 桥收件箱 |
   | `Obs::ToolCall.request` | JSON-RPC id | `{"kind":"dialog","id":"<对话框 id>"}` | `{"kind":"bridge","call_id":"<id>"}` |
   | 回执 | `wire.send({"id","result"})` | `extension_ui_response{value}` | 唤醒 HTTP 处理器 |
   | 新增宿主监听 | 无 | 无 | 有（会话令牌保护） |

   - **上游能力观察（已核实不可用）**：ACP 的 SDK schema 定义了「客户端承载的 MCP 服务」（`mcpServers` 的 `{type:"acp", name, serverId}` 与 `mcp/connect`、`mcp/message`、`mcp/disconnect`）。若 agent 实现它，步骤工具就能直接走 stdio 协议通道，与 Codex/DSH/pi 完全同构，宿主不再需要监听器与令牌。**本机 mcode 0.7.9 未实现**（`configureSessionMcpServers` 只处理 stdio 与 URL 两类，包内无 `type:"acp"` 分支与 `mcp/connect` 调用点），**opencode 2.4.1 二进制内也没有 `mcp/connect`**。因此本期保留宿主 HTTP 桥；上游实现后可切换并移除监听面。

### 2.5 Schema、全局库与出厂配置

1. `sql/global/schema.sql`：`runtime_installations.provider` CHECK 扩到 `('codex','dsh','pi','mcode','zcode','opencode')`；`PRAGMA user_version` 按第 3 章的合并决策一次性推进。
2. 全局库升级命令与启动版本检查见第 3 章。**启动只接受 `0` 与 `25`**；v24 数据集启动时给出指向升级命令的明确错误，不得静默放行。
3. `schemas/rpc.schema.json`（六处 provider 枚举）、`schemas/permission-backend.schema.json`（两处）同步枚举。
4. 出厂配置：`plugins/core/harnesses/builtin-harness-{mcode,zcode,opencode}-managed-v1.json`，对应 `builtin-model-catalog-{mcode,zcode,opencode}-v1.json`；登记 `plugins/core/manifest.json`；`scripts/check/configuration_usage.py --write` 重生成 `reference-usage.json`；`tests/fixtures/configuration/engineering-catalog.json` 同步（`schema_contracts` 会遍历全部 provider）。
5. 目录 `recipe_digest` 取自 `examples/export_runtime_recipes.rs` 输出（该示例改用 `HARNESS_PROVIDERS`）。
6. **配方生成面扩展（两条新线的前置改造）**：现有 `compose_execution_recipe` 只有一条安装通道——`RUN npm install --global --omit=dev {package}@{version}`（`crates/domain/src/execution_recipe.rs:219-222`），且 `HarnessInstallation.package` 必填非空（`:119-129`、校验 `:165-171`）；`native_releases` 只服务**宿主侧登录组件**（`runtime_preparation.rs:216-230`），不是镜像安装通道。扩展需满足四条约束：
   - **字节稳定**：未使用新字段的 Harness（codex/dsh/pi/zcode）生成的 Dockerfile 必须与改造前**逐字节相同**，`recipe_digest` 不变 → 已登记的安装继续有效、Harness 与 Model 目录的 `recipe_digest` 钉定不受影响、无需重新准备镜像。`installation_identity`（`execution_recipe.rs:183`）的哈希输入只在字段存在时追加。回归测试：`export_runtime_recipes` 对 codex/dsh/pi 的 `recipe_digest` 在改造前后一致（写成常量断言）。此前版本称「配方生成一变，全部 provider 的配方摘要都变」——按本约束**不成立**，权限资产漂移也不因此多出轮次。
   - **注入防护**：进入 Dockerfile `RUN` 行的字段一律白名单校验。npm 参数字段只接受枚举开关与受限取值（如 `--include=optional`、`--allow-scripts=<包名列表>`），字符集受限；二进制字段 url 仅 https、无内嵌凭据/查询/片段，sha256 为 64 位十六进制，member 为不含 `..`、非绝对的相对路径；所有插值经 `serde_json::to_string` 转义（与现有 COPY 行一致）。出厂 Harness 源是数据，可经 `harness.create`/`revise` 编辑，不能假定可信。
   - **构建期完整性**：二进制发行物在 `docker build` 内完成 sha256 校验后才落位（`ADD --checksum` 或镜像内 Node 脚本 fetch + verify），不依赖宿主预下载；按 `TARGETARCH` 选择资产；**一份配方覆盖 linux/x86_64 与 linux/aarch64**（摘要覆盖全部平台资产），以维持「一个 Harness 一个 `recipe_digest`」的现有语义。
   - **Schema 与校验同步**：`HarnessInstallation` 是 `deny_unknown_fields`，出厂源在 `crates/adapters/build.rs` 构建期按 Schema 校验 → 新字段同步 JSON Schema 与 `ExecutionRecipe::validate`。
   - P0 验证：支持的最老 Docker 版本上 BuildKit 的 `TARGETARCH` 与 `ADD --checksum` 可用。不可用时的退路是宿主侧下载 + 校验后放入构建上下文（复用 `prepare_codex_auth` 的 curl + sha256 流程），此时按目标平台生成不同配方并让 `recipe_digest` 含平台，需要同步调整单值假设——代价更高，仅在 P0 证明必要时采用。

### 2.6 前端与测试基建

1. 前端：`RuntimeSettings.tsx`（类型与下拉）、`AgentCatalog.tsx`、`RuntimePreparationStatus.tsx`、`Integrations.tsx`、`RoleConversation.tsx`、`App.tsx`（凭据键前缀）；`DshProviderSettings.tsx` 改为按 harness 渲染，供 dsh/pi/mcode/zcode/opencode 共用。
2. **能力与限制可见**：Agent 编辑器与权限页展示所选 Harness 的 `limitations`（例如「不支持 harness 层工具禁网」「不支持 Agent 提问」「远程 Docker daemon 下步骤工具不可用」）；组合不可用时在保存前拦截，而不是运行期才失败。
3. 测试支撑：`crates/adapters/tests/support/mod.rs`（`DEVDESK_TEST_PI_IMAGE`、`DEVDESK_TEST_MCODE_IMAGE`、`DEVDESK_TEST_ZCODE_IMAGE`、`DEVDESK_TEST_OPENCODE_IMAGE`）、`tests/support/runtime.rs`、`tests/entrypoints/native_acceptance.py`、`scripts/check/doctor.py`。

## 3. 全局库版本与旧数据集升级

1. **版本合并决策（已定）**：**全局库只在批次 A 变更一次**——`user_version 24 → 25`，`runtime_installations.provider` CHECK 直接写成 `('codex','dsh','pi','mcode','zcode','opencode')`。批次 B 不再发生全局库版本变更；zcode 即使最终不实施，CHECK 里保留该取值也无害。本节是版本号与 CHECK 取值的**唯一权威口径**，各 Harness 计划只引用。
   - 维护性提示：字面量 CHECK 意味着每接入一个新 Harness 都要重建表。升级命令因此设计成**可复用的步进框架**（第 2 条），后续版本只需追加条目。
2. **离线升级命令** `devdesk-host storage upgrade-global --data PATH`（实现在 `crates/adapters/src/global_migration.rs`，与 `project_migration.rs` 同级；CLI 在 `host/src/main.rs` 的 `storage` 子命令组）：
   1. 经 `lock_dataset` 取得独占锁——Host 正在运行则直接失败，不依赖人工核对。
   2. 读取 `user_version`：0 → 无需升级；24 → 继续；25 → 幂等返回；其他 → 拒绝。
   3. 对 `global.sqlite3` 做 WAL checkpoint，保证备份完整。
   4. 备份到数据集目录之外（沿用仓库现有的 `.devdesk-storage-backups` 约定），文件名含时间戳与源版本。
   5. 单事务内重建 `runtime_installations`（新 CHECK）并重建其索引，推进 `user_version`，**保留全部既有行**（现有 `data/` 的 3 条运行安装）。
   6. 校验：行数一致、`sqlite_master` 中该表与索引的定义与 v25 基线一致、`PRAGMA integrity_check` 通过。
   7. 输出 JSON 报告（源版本、目标版本、行数、备份路径）。
   - 启动版本检查（`catalog.rs:41`）同步改为只接受 `0 | 25`。
3. `tests/entrypoints/dataset_protocol.py` 扩展：覆盖 v24→v25 升级路径、幂等重跑、Host 运行中拒绝升级、升级后旧二进制被拒绝。
4. **旧数据集的完整升级流程**（重建表只是第一步，以下缺一不可）：
   1. 停止 Host，执行 `storage upgrade-global`。**先在 `data1/` 的副本上演练，再对 `data1/`、最后对 `data/` 执行**，每次保留备份。
   2. 启动新 Host。
   3. **更新出厂目录**：`configuration.templates.reset`（`preserve_custom: true`）。新增的 Harness/服务商/Model/原生工具源不会自动进入现有数据集，不做这一步旧数据集里根本选不到新 Harness。核对 `query.template_catalog` 的计数。
   4. 重新应用权限绑定：新增后端资产让 codex/dsh 的 `enforcement_contract` 漂移。逐目标执行 `permission.binding.preflight` + `permission.bindings.apply`（`permission.bind` 已退役）。**待核实**：出厂目录更新随后的 `template_permission_defaults` 是否会因合同变化而重新绑定出厂目标；若会，这一步只需覆盖自定义目标与 Workflow 步骤的显式 Profile。
   5. 新 Harness 目标的默认权限绑定：出厂 Harness 修订由 `template_permission_defaults` 自动套用默认 Profile；自定义目标必须显式应用绑定，否则会话打开失败。
   6. 既有冻结会话需创建带上下文的后继会话；进行中的执行按恢复流程核对后重跑。
   7. codex/dsh 的配方摘要不变（2.5.6 字节稳定），已登记安装仍有效，仅按新驱动身份重新通过协议探针（自动）。
5. **漂移轮次口径**：每新增一批权限后端都会改动 `asset_digests()`（`crates/adapters/src/permissions.rs:42-45`，覆盖 `permissions/backends/manifest.json` 全文与全部资产摘要）→ codex/dsh 的 `enforcement_contract` 漂移、冻结会话 `permission_asset_mismatch`。按 0.5 的两批次安排，**每批次之后**都要重跑第 4 条的流程，并在验收记录里写明这是第几轮漂移。
6. `devdesk-recovery/20261119-large-snapshot/` 是恢复快照，**不升级**；确有需要时在同一命令下另行处理。

## 4. 未决问题与后续关注点

| # | 问题 | 影响 | 处置 |
|---|---|---|---|
| 1 | mcode 在 Linux 容器内能否启动（无 linux 原生预编译） | 决定 mcode 计划是否成立 | mcode 计划 P0.1；失败则回到用户决策（换发行物 / 换基础镜像 / 放弃 mcode） |
| 2 | `PreToolUse` Hook 能否真正阻断工具调用（mcode 与 zcode） | 决定冻结策略能否在容器内落实 | mcode 计划 P0.4 / zcode 计划 P0.6；不能则如实降级并写入 `limitations` |
| 3 | ACP `session/new` 的 `mcpServers`（http + headers）在容器内能否连通宿主端点；agent 是否宣告 `mcpCapabilities.http` | 决定步骤工具一期能否打通 | mcode 计划 P0.3；备选见 mcode 计划 §2.3 第 7 条；opencode 侧优先改用配置文件注入 |
| 4 | 宿主 MCP 桥在 macOS/Linux 的监听地址策略 | 决定桥的可用性与暴露面 | mcode 计划 P0.3 双形态实测；独立监听器 + 每会话令牌 |
| 5 | ZCode Protocol 的权威终态事件与权限应答形状 | 决定 zcode 计划能否进入 P1 | zcode 计划 P0.2/P0.4；未确认即停止 |
| 6 | zcode 随包专有运行时的再分发条款是否允许进入 DevDesk 镜像 | 合规风险 | zcode 计划 P1 前由维护者/法务确认；至少完整携带 `LICENSES/` 与声明 |
| 7 | 全局库版本合并 vs 逐级升级 | 影响一次性影响面与验收工作量 | **已定：两批次、全局库只升一次**（批次 A 把 CHECK 写成六个 provider）；漂移轮次 2；见第 3.1 条与 0.5 |
| 8 | pi 的 UI 隧道与新宿主 MCP 桥是否合并为同一套桥接实现 | 长期一致性 | **执行口径已统一**（都经 core 接纳/审计链路，请求判别式见 2.4.10）；合并实现列入后续演进。上游 ACP 若实现「客户端承载的 MCP」，三条 ACP/ZCode 以外的线可改走协议通道（2.4.10） |
| 9 | opencode ACP 模式是否隐式使用后台服务 | 多会话共享服务进程，破坏会话隔离 | opencode 计划 P0.3；必要时 `--standalone` 或改走其 v2 HTTP/SSE 备选路线 |
| 10 | opencode 发行二进制在容器内是否可直接运行（glibc/依赖） | 决定 opencode 计划是否成立 | opencode 计划 P0.1；`native_releases` 不是镜像安装通道，容器安装走 2.5.6 |
| 11 | 三条接入线的实施顺序 | 影响交付节奏 | **已定：P0 批次前置（G0），随后批次 A（pi）、批次 B（mcode → opencode → zcode）**；见 0.5 |
| 12 | 宿主 MCP 桥的调用执行路由：(a) 转发到 core 既有接纳链路，(b) 桥自行执行并写入会话事件 | 决定步骤工具是否可审计、可幂等重放 | **已定 (a)**（2.4.8）；桥只做传输与寻址 |
| 13 | 是否新增「二进制发行物」安装面 | 决定 opencode 能否用 v2 发行二进制 | **已定：新增**（2.5.6），并对既有配方字节稳定 |
| 14 | ACP 的接纳（`Obs::Admitted`）语义 | 决定流式输出与长轮次是否可用 | **已定**：按首个 provider 证据合成（2.2.8）；轮次应答改判终态 |
| 15 | 冻结网络为 Disabled 的 Profile 下步骤工具是否可用 | 「一期必须打通步骤工具」的适用边界 | **已更正**：原前提不成立——Harness 容器恒联网，相关场景不存在（2.4.6） |
| 16 | mcode/zcode 的容器内策略钩子**真实发现路径** | 决定冻结策略能否在容器内落实（静默失效最危险） | P0 必须先确定发现路径，再决定挂载目标；并落实 2.1.7 的装载证明。目录型挂载目标需新增能力与白名单 |
| 17 | MCP 客户端对工具名的命名空间化与长度限制 | 模型可见名与 DevDesk 指令文本不一致；超长名被截断或拒绝 | **P0 必测**（2.4.4）：记录每条线的模型可见名；暴露名约束与确定性映射 |
| 18 | 桥调用入队后 worker 是否被及时轮询 | provider 等待 MCP 结果期间不产生协议帧 → 桥调用滞留到超时 | **已定：入队即唤醒**（2.2.9、2.4.8），并列入桥验收判据第 9 条 |
| 19 | opencode provider 的 `npm` 包（如 `@ai-sdk/openai-compatible`）在容器内的安装路径 | 运行时从未固定的 npm 拉取包不可接受 | **P0 必测**：预装并钉版本，或使用其内置协议；见 opencode 计划 P0.7 |
| 20 | 同厂商 API key 跨 Harness 复用 | 用户需为每个 Harness 重复保存同一把 key | 一期保持按 `(Harness, 服务商)` 隔离并在 UI 提示；一键复用列入后续演进（2.3.4） |
| 21 | opencode 的指令注入通道 | DevDesk 的角色指令、快照与合同无法送达 | **P0 必测**：`acp` 无系统提示开关 → ACP `sessionCapabilities.systemPrompt`，退路为配置 `instructions` |
| 22 | ACP 恢复使用 `session/load` 会重放整段历史 | 长会话恢复触发初始化缓冲上限而失败 | **已定口径**：优先 `session/resume`；退回 `session/load` 时有界丢弃重放（2.2.7） |
| 23 | 宿主代执行工具是否走人工审批 | 按此前描述实现会让默认 Profile 下的步骤工具被全部拒绝 | **已更正**：适配器与桥不设审批，准入由 core 按冻结权限判定（2.4.5） |

## 5. 实施阶段的通用约束

1. 先读 `AGENT.md` 与 `docs/design/`，保留既有改动；改动行为时同步更新合同与当前文档。
2. 不把凭据、运行数据、日志写入版本库；桥令牌、API key 只在容器内短生命周期存在，日志一律脱敏。
3. 镜像构建固定版本与摘要；`compose_execution_recipe` 产出的 Dockerfile 与锁文件摘要进入出厂配置。
4. 涉及全局库变更时，先在副本数据集演练升级，再对真实数据集执行并保留备份。
5. 每次改动后运行受影响的最小测试集，最后跑 `./scripts/check/check.sh` 全量；报告写明命令、制品版本、实际结果与未验证范围。
6. **P0 批次全部完成之前不写生产代码**；任一 P0 结论变化时先修订本文件，再改对应 Harness 计划。
7. 进入 Dockerfile 的任何配置字段都按不可信数据处理（2.5.6 注入防护）。

## 6. 统一语义口径（四条线一致）

| 语义 | 口径 | 依据 |
|---|---|---|
| 正常结束 | Completed | — |
| 输出被截断（`max_tokens`、`length`、`stopReason: length`） | **Failed**（原因写 `max_tokens`） | DSH 的 `DSH_MAX_TOKENS_AS_SUCCESS=false`；截断的交付物不能被当作完整结果接纳 |
| `max_turn_requests`、`refusal`、`error` | Failed | — |
| 用户取消、`aborted`、`cancelled` | Cancelled | — |
| 工具调用失败 | 只记为 Tool 事件，**不改变轮次终态** | 与 Codex/DSH 一致：agent 可以从失败的工具调用中恢复 |
| 「消息已消费」 | 用户消息证据到达时产出 `Obs::Consumed`；provider 不回显用户消息时（ACP）与接纳同时产出 | mcode 计划 P0.2 确认 |
| 助手终稿 | 以工具调用为边界**分段**封存，每次工具调用前封存当前段；不能把整轮拼成一条 | 多步轮次里工具调用前后的文本属于不同消息 |
| Agent 提问 | 适配器直接应答取消，不产生 `Obs::Interaction` | core 对任何 `Obs::Interaction` 都会把轮次置为 `WaitingInteraction` |
| 推理内容 | 不展示、不入账 | 与 Codex 不展示 reasoning 一致 |
| 审批 | 只对 provider 自己发起的请求；`never` 自动拒绝；允许时只选单次允许 | 2.4.5 |

## 7. 通用 P0 清单（各线 P0 必含，结论回写对应计划）

| # | 验证项 | 判定 |
|---|---|---|
| 1 | 镜像可构建、`--version` 正确、协议进程可在 `--network none`、只读根文件系统、`--cap-drop ALL` 下启动并完成最小握手 | 启动期若有强制外联，探针会在这里失败 |
| 2 | 最小闭环：建会话 → 发一轮 → 收到流式增量与权威终态 → 取消一轮 | 连续 3 次结果一致 |
| 3 | 轮次请求的**应答时机**（立即回执 vs 轮次末）与 `stopReason` 的实际取值和大小写 | 决定 `admission` 取值与终态映射 |
| 4 | 冻结策略钩子：发现路径、装载信号、能否阻断工具调用 | 三者齐备才登记 `tools: restricted` |
| 5 | 仓库内可执行配置是否被读取；关闭开关是否生效 | 见 2.1.8 |
| 6 | 启动期外联（模型目录、更新检查、遥测）逐项确认并关闭 | 不得有未固定的运行时下载 |
| 7 | 指令注入通道可用，且注入内容确实进入模型上下文 | — |
| 8 | 恢复：会话 id 稳定、不重放历史或重放可被有界丢弃、模型与 MCP 配置在恢复后仍然生效 | 见 2.2.7 |
| 9 | 宿主桥连通性（仅 HostMcp 线）：agent 宣告对应 MCP 传输能力；容器内能连到宿主端点；macOS 与 Linux 两种绑定地址；工具名经客户端后的模型可见名与长度 | 见 2.4.4、2.4.6 |
| 10 | 模型路由与凭据：API key 型自定义 provider 的最小配置；零凭据启动的探针配置 | 探针必须零凭据、零网络 |
