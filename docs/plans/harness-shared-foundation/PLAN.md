# Harness 扩展共享地基（mcode / zcode / opencode 三条接入线）

日期：2026-11-25。状态：**待实施**。范围：三条接入线共同依赖的 Provider 注册、协议传输、凭据泛化、宿主侧工具桥与全局库地基。本文件不含任何单一 Harness 的协议细节。

配套计划：[mcode 接入](../mcode-harness/PLAN.md)、[zcode 接入](../zcode-harness/PLAN.md)、[opencode 接入](../opencode-harness/PLAN.md)；既有 [pi 接入](../pi-harness/PLAN.md) 仍按原计划实施，本地基在其之上叠加三个 Provider。

## 0. 结论与范围

### 0.1 三份计划

| 计划 | Harness | 驱动协议 | 定位 |
|---|---|---|---|
| [mcode](../mcode-harness/PLAN.md) | MiniMax Code CLI 0.7.9（`@minimax-ai/code`，MIT） | **ACP 标准协议**（`mcode acp`，常驻 stdio） | 先行；Linux 原生依赖有风险，P0 判定成立性 |
| [zcode](../zcode-harness/PLAN.md) | ZCode CLI（`zcode-app-cli`，MIT 启动器 + 随包专有运行时） | **ZCode Protocol**（`zcode app-server`，常驻 stdio） | 协议未公开文档化，整体以 P0 实跑结论为门 |
| [opencode](../opencode-harness/PLAN.md) | opencode CLI 2.4.1（MIT，Bun 单体） | **ACP 标准协议**（`opencode acp`，常驻 stdio） | 与 mcode 共用同一 ACP 适配器；协议风险最低、许可最干净 |

### 0.2 结论（可复用性判定）

pi 计划的方法**结构上完全可复用**：即「编译期 provider 注册 + 声明式 Harness JSON + 镜像配方 + 权限后端资产 + 凭据链路 + Schema/全局库升级」这套骨架照搬；但**协议适配层与工具桥必须换实现**，因为四个 CLI 的扩展面互不相同：

| 维度 | pi 1.6.0 | mcode 0.7.9 | zcode 0.17.1 / 3.18.2-48 | opencode 2.4.1 |
|---|---|---|---|---|
| 打包 | npm `@earendil-works/pi-coding-agent` | npm `@minimax-ai/code`（MIT） | npm `zcode-app-cli`（MIT 启动器，随包专有运行时） | GitHub 发行二进制（MIT，Bun 单体）；npm `opencode-ai` 仅 v1 线 |
| 驱动协议 | 自有 `--mode rpc`（JSON Lines，无握手） | **ACP 标准协议**（JSON-RPC 2.0） | **ZCode Protocol**（自有 `{id,method,params}`） | **ACP 标准协议** + 备选 v2 HTTP/SSE API |
| 会话/轮次映射 | `prompt` → `agent_settled` | `session/new`→`session/prompt`→`stopReason` | `session/create`→`session/send`→`session/event` | ACP 同 mcode；HTTP 路线为 `session/create`→`session/prompt`→事件流 |
| 审批通道 | `extension_ui_request(select)` | `session/request_permission` | `interaction/requestPermission` + `interaction/respond` | ACP `session/request_permission`；HTTP 路线 `permission.asked`/`permission.reply` |
| 容器内策略钩子 | `pi.on("tool_call")` 扩展 | `PreToolUse` / `PermissionRequest` 生命周期 Hook | `PreToolUse` / `PermissionRequest` 生命周期 Hook | **插件钩子 `tool.execute.before` + `permission.ask`**（四者中最规范） |
| 步骤工具交付 | 扩展 `registerTool` + `ctx.ui.input` 隧道 | **无进程内注册通道** → 必须走 MCP | **无进程内注册通道** → 必须走 MCP | 进程内可注册插件工具，但统一走 MCP 更一致 |
| 凭据所有权 | `models.json` + provider env | provider 配置 + API key | `ZCODE_APP_SERVER_AUTH_MODE=host`（宿主供凭据） | `opencode.json` 的 `provider.options.headers`（宿主写入后即删） |

### 0.3 关键设计决策

1. 一份计划覆盖三个 Harness，**mcode 先行、opencode 次之**；zcode 以 **P0 实跑结论为门**（协议未文档化，未跑通不进入 P1）。
2. mcode 驱动方式用 **`mcode acp`**（ACP 标准协议，常驻进程），不采用一次性 `exec --output-format stream-json`。
3. zcode 走 **`zcode app-server`** 协议路线纳入实施（若 P0 证伪则整体回退，见 zcode 计划的 P0 门控）。
4. opencode 驱动方式用 **`opencode acp`**（ACP 标准协议，常驻进程），**与 mcode 共用同一个 ACP 适配器模块**（`harness_acp.rs`），只保留各自的启动配置、模型配置、策略钩子与 MCP 注入差异；v2 HTTP/SSE API 作为文档化备选路线记录在 opencode 计划。
5. **workflow 步骤工具（dynamic tools）一期必须打通**：mcode 与 zcode 都没有「进程内注册宿主工具」的通道（opencode 虽有插件工具注册，但为保持三份计划一致并让宿主始终掌握回执，统一走宿主 MCP），因此统一改为 **宿主侧 MCP 服务端**——DevDesk Host 对容器暴露一个带会话令牌的 MCP（streamable HTTP）端点，mcode/opencode 通过 ACP `session/new.mcpServers[{type:"http"}]`、zcode 通过 `setting.json` 的 `mcp.servers` 连接它；工具执行、回执、冻结权限核验全部留在 Host 侧。
6. **宿主 MCP 桥的统一执行口径：经 core 接纳/审计链路**。桥只负责传输与寻址，**不执行工具、不自造审批、不旁路治理记录**——桥收到的 `tools/call` 由 worker 产出等价的 `Obs::ToolCall`，走 Codex/DSH 同一条接纳 → 执行 → 回执链路（详见 2.4.8）。因此「工具调用在会话记录中可见、可审计、可幂等重放」对三条线与 pi 线的语义一致。
7. **ACP 的接纳语义由宿主合成**：既有 Codex/DSH 骨架把「对请求的应答帧」当作接纳凭证，而 ACP 的 `session/prompt` 应答在**轮次结束时**才到（只有 `stopReason`）→ 共用的 `harness_acp.rs` 必须改为：宿主提交 `session/prompt` 时即合成 `Obs::Admitted{external_turn: 宿主生成的轮次 id}`（并把它写进 turn 记录以满足 `validate_provider_call_scope` 的 `turn["external_turn"]` 比对），把 `session/prompt` 应答改判为**终态**信号。详见 mcode 计划 §2.2 与本文件第 4 章 #14。

### 0.4 重要非目标

- 不引入根编排（mcode/zcode/opencode 与 pi/DSH 一致，不获得 `orchestration` 能力）。
- 不为 mcode/zcode/opencode 承诺「Agent 提问」以外的交互扩展；ACP/ZCode 都带用户提问通道，可作为 P3 的可选项，不进一期验收。
- 不改 pi 计划已定的决策（pi 仍按原计划实施），本计划只在其之上叠加三个 provider。**执行口径已统一**：pi 的步骤工具走 `ctx.ui.input` UI 隧道，新三条线走宿主 MCP 桥，两者的工具调用都经同一条 core 接纳/审计链路（`Obs::ToolCall` → core 接纳 → `ToolExecutor` 执行 → 回执），差别只在**传输**（pi 是进程内 UI 隧道，新线是容器→宿主的 HTTP MCP），语义上（会话记录、治理记录、幂等、审批）一致。
- 不做「容器内工具禁网」执行器；网络约束仍由 Docker 层承担。

### 0.5 交付形态与实施顺序

落地顺序：**本文件全部完成 → mcode（P0→P4）→ opencode（P0→P4，与 mcode 共用 ACP 适配器，边际成本最低）→ zcode（P0 门控）**。若时间不允许全量做，本文件第 2 章的 2.1–2.4（provider 注册、凭据泛化、协议通道扩展、宿主 MCP 服务端）是三个计划的最小公共前置。

三份 Harness 计划与本文件构成一组：任一 Harness 的 P0 结论变化时，先改本文件的地基判断，再改对应 Harness 计划。

### 0.6 本次工作约束

调研为只读（源码阅读 + 本机已安装包的静态分析 + npm registry / GitHub release 元数据），**未实跑任何 harness 协议**——凡未实跑验证的行为在各 Harness 计划中均标注为 P0 待验。凭据、实际运行数据、日志与本地复核材料留在版本库外。

## 1. DevDesk 现状（受影响组件）

- Harness = 编译期驱动（`HarnessProvider` 枚举，`crates/domain/src/runtime.rs:9`，`HARNESS_PROVIDERS` 注册表）+ 声明式配置（`plugins/core/harnesses/*.json`，`data.provider`）。当前仅 `codex`、`dsh`。
- 协议适配集中在 `crates/adapters/src/harness.rs`（约 2325 行，单文件按 provider 分支）+ 传输层 `jsonl.rs`（JSONL 帧、`rpc2()` 开关决定是否加 `jsonrpc:"2.0"`、`Journal` 脱敏与合批）+ 权限编译 `permissions.rs`/`permission_profiles.rs`。
- 既有 provider 差异已固化在代码里（pi 计划 P1.2 已点名要改的「非 Codex 即 DSH」隐式分支）：`harness.rs`（模型连接、stop 模式名、应答形状）、`core/src/session_assembly.rs`、`domain/src/conversation.rs`、`core/src/credential_updates.rs`、`adapters/src/credentials.rs`。
- 动态工具既有两条投递路径：Codex 走 `thread/start` 的 `dynamicTools`（`harness.rs:740`），DSH 走扩展 `dynamicTools` + `contextBridge`（`harness.rs:610`）。两者都要求「宿主在协议通道上收工具调用并回结果」。
- 容器→宿主连通性已有先例：`ContainerSpec.host_gateway` → `docker.rs` 注入 `--add-host host.docker.internal:host-gateway`；集成工具执行器按 `allowed_origins` 含 `http://host.docker.internal:` 时启用（`crates/adapters/src/integrations.rs:430`）。
- DevDesk 目前只有 **MCP 客户端**（容器内 `crates/adapters/runtime/tool-worker.mjs` 实现 `initialize`/`tools/list`/`tools/call`，同时支持协议版本 `2025-11-25` 与 `2026-07-28`），**没有 MCP 服务端**。→ 本地基新增的第一个新能力面就是宿主侧 MCP 服务端。
- 全局库：`sql/global/schema.sql` 的 `runtime_installations.provider` CHECK 为 `('codex','dsh')`，`PRAGMA user_version=24`；模型服务商公开 id 必须全局唯一（`crates/core/src/configuration.rs:366` `validate_model_provider`）。
- 权限后端资产清单：`permissions/backends/manifest.json`（后端 + 资产 sha256）。新增后端会让 codex/dsh 的 `enforcement_contract` 漂移、冻结会话 `permission_asset_mismatch`（pi 计划已定性为系统既定的一次性影响）。

## 2. 共享地基工作项

### 2.1 Provider 注册与显式分支

1. `crates/domain/src/runtime.rs`：新增 `HarnessProvider::Mcode`、`HarnessProvider::Zcode`、`HarnessProvider::Opencode`，并入 `HARNESS_PROVIDERS`（provider 门禁只读该注册表，禁止散落字面量）。
2. 把「非 Codex 即 DSH」的隐式分支改成显式 match（pi 计划 P1.2 已列清单，此处扩到 4 个 provider）：`crates/adapters/src/harness.rs`、`crates/core/src/session_assembly.rs`、`crates/domain/src/conversation.rs`、`crates/core/src/credential_updates.rs`、`crates/adapters/src/credentials.rs`。`conversation_observations.rs`、`provider_tools.rs` 的 `== Dsh → external_message` 保持不动。
3. 遍历 provider 的测试改用 `HARNESS_PROVIDERS`（`harness.rs`、`permission_profiles.rs`、`tests/{permissions,schema_contracts,permission_profiles}.rs`）。

### 2.2 传输层扩展（新增 ACP 与 ZCode 两种线协议形态）

**现状更正**：`jsonl.rs` 本身不区分方向，`JsonlProcess::send` 可发任意帧；「入站请求应答」其实**已经存在**——`harness.rs` 的 `project()` 对带 `id` 且带 `method` 的帧分派（`item/tool/call` → `Obs::ToolCall`，其余 → `interaction()` 处理审批与提问），并用 `wire.send({"id":…,"result":…})` 回帧。真正缺的是**线协议形态**与**应答形状的多样化**，而不是从零造一条通道。

1. 线协议模式（新增两种，与既有 Codex 无 `jsonrpc`、DSH 带 `jsonrpc` 并列）：
   - **ACP**（**mcode 与 opencode 共用**）：`jsonrpc:"2.0"` 必填（复用现有 `rpc2()` 开关）；请求/应答按 `id` 匹配；通知按 `method` 识别。
   - **ZCode Protocol**：**不得**加 `jsonrpc` 字段；帧为 `{id, method, params}` → `{id, result|error}`；通知为 `{method:"session/event", params}`。
2. 入站请求应答的形状**按 provider 分派**（现有实现是 Codex/DSH 两分支）：ACP 审批应答为 `{"outcome":{"outcome":"selected","optionId":…}}`、取消为 `{"outcome":{"outcome":"cancelled"}}`；ZCode 为 `interaction/respond{type:"respond_permission",…}`。建议在 `jsonl.rs` 提供语义化的 `respond(id, result)` / `respond_error(id, code, message)` 封装，而不是让各 provider 直接拼帧。
3. **外部请求 id 的字符串化与还原**：`Obs::Interaction.request` 是 `String`，而 ACP/ZCode 的入站 `id` 是数字 → 适配器负责字符串化（建议 `"<provider>:<id>"` 以免不同 provider 的 id 空间碰撞），`answer_interaction` 侧再反解回原始 id 回帧。既有 `PendingInteraction.external_request` 存的就是这个字符串。
4. **等待应答期间的入站帧处置**：`JsonlProcess::request_for` 在 `capture == false` 时会把任何带 `id`+`method` 的帧**自动回 `-32601`**（`jsonl.rs:558-563`）。Harness worker 必须始终保持 `capture == true`（`journal` 存在即为 true），否则 ACP 的 `session/request_permission` 会被误答。
5. `Journal` 脱敏补强：新增「按结构脱敏」规则——凡 `headers` 数组/对象中 `name` 命中 `authorization|token|api[-_]?key|secret` 的条目，其 `value` 必须替换为 `[redacted]`（宿主 MCP 桥令牌会以 `headers` 形式出现在 `session/new` 与 settings 写入参数里，现有按键名白名单挡不住）。三种载体形状都要覆盖：ACP 的 `headers:[{name,value}]` 数组、ZCode/opencode 的 `headers:{k:v}` 对象、以及 `mcp.servers` 下的多层嵌套。

### 2.3 凭据链路泛化（API key 型 Harness）

1. 把写死的 `dsh` 泛化为「API key Harness」集合：绑定键 `credentials.{harness}.{provider}`（dsh 现有键名保持不变），涉及 `crates/core/src/credential_updates.rs`、`crates/host/src/model_providers.rs`、`crates/adapters/src/{credentials,catalog,credentials_macos}.rs`。
2. macOS 钥匙串账户前缀白名单加入 mcode、zcode 与 opencode 各自的服务商前缀。
3. 服务商公开 id 全局唯一（`validate_model_provider`）→ 新建 `mcode-*`、`zcode-*`、`opencode-*` 前缀的 provider 源与 Model 源（不复用 dsh 的 id，避免跨 Harness 语义混淆）。

### 2.4 宿主侧 MCP 服务端（新增能力面，三个计划共用，一期必备）

这是「workflow 步骤工具一期必须打通」的落地物，也是本计划相对 pi 计划最大的新增面。

1. **端点**：Host 新增一个**独立监听器**（不复用主 API 的 `127.0.0.1:4317`，避免把整个宿主 API 暴露给容器），仅在会话存在动态工具时启动；路径含会话 id（如 `/internal/tool-bridge/<session>/mcp`）。
2. **鉴权**：每会话随机令牌（≥32 字节，随机源 `Sha256` 域分离），以 `Authorization: Bearer <token>` 或专用头传递；令牌只写进容器内进程可见的会话级配置，**不进仓库、不进日志**（与 2.2.5 的结构化脱敏配套）。
3. **协议**：实现 MCP `initialize`、`notifications/initialized`、`tools/list`、`tools/call`，同时支持 `2025-11-25`（`Mcp-Session-Id` 头）与 `2026-07-28`（`_meta` 携带 `io.modelcontextprotocol/protocolVersion`），语义与 `crates/adapters/runtime/tool-worker.mjs` 对齐（客户端是同一产品的服务端，两侧版本集合必须一致）。
4. **工具集合**：`tools/list` 返回该会话**冻结**的动态工具集（来源同 `harness.rs:1718 dynamic_tools(binding)`：集成绑定工具、未绑定工具、平台工具、上下文工具，且按冻结 `tool_refs` 过滤）。**工具名注册表**：MCP 侧暴露的名字必须就是 `dynamic_tools()` 结果里的 `name` 字段——集成/未绑定工具用 `definition.qualified_name`，平台工具用 `definition.call_name()`（core 侧也按这两种名字分别匹配：`conversation_observations.rs:410-418` 用 `qualified_name`，`platform_observations.rs:46-55` 用 `call_name()`）。桥侧必须保留 `name → (种类, 冻结引用)` 的反查表；`qualified_name` 与 `call_name` 撞名时以冻结引用消歧，撞名且无法消歧的条目**不暴露**。未在冻结集合内的名字一律拒绝。
   - **一期暴露范围：四类全暴露**（含平台工具）。**路由规则**：桥产出观察时 `tool` 必须按种类取值——集成/未绑定工具用 `qualified_name`，**平台工具用 `call_name()`**，上下文工具用其 `call_name()`。core 已有按名分派：`commit_conversation_batch` 用 `native_provider_observation` 判别（`conversation_runtime.rs:334-345`）——命中集成工具 `qualified_name` 的走常规接纳（`admit_provider_observations`），**其余（平台工具）自动走平台准入路径**（`admit_platform_provider_observation`）。→ 桥**不需要**新增 grant 路由或新端口。
   - **平台工具的可调用前提（必须写进 `limitations`）**：平台调用要么有 **controller 授权**（`poll.authority.controller` 存在，且 controller 活跃、租约未过期、已声明该工具——`platform_observations.rs:140-194`），要么有 **workflow 执行归属**（`poll.authority.workflow` 存在，且实例/执行仍 Running 且属本会话——`platform_observations.rs:263-290`）；**两者皆无的普通讨论会话调用平台工具会被 core 以 `tool_not_authorized` 拒绝**。桥必须把这种情况**提前判定**并返回明确错误（MCP `isError` + 原因），不能让模型反复重试。
5. **权限与审批**：调用前核对冻结权限（工具是否在 `tool_refs`、网络策略）；冻结审批策略为 `on-request` 时，宿主侧工具调用走既有 `Obs::Interaction` 审批流（与 harness 自身发起的 `session/request_permission` 是两条独立通道，需在 UI 上区分来源）。
   - **实现口径**：桥不主动发起交互，而是由 **worker** 从桥收件箱取出「待审批的桥调用」后产出 `Obs::Interaction{ToolApproval, request:{"kind":"bridge",…}}` → core 建 `PendingInteraction` 并把轮次置 `WaitingInteraction` → 用户裁决 → core 调 `harness.answer_interaction` → 适配器按 `request.kind == "bridge"` 把裁决交回桥 → 桥据此执行或拒绝。冻结审批策略为 `never` 时，worker 直接产出拒绝决定（不建 `PendingInteraction`），桥返回错误结果。
6. **容器连通性**：`docker.rs` 为 Harness worker 增加独立开关（与既有 `host_gateway` 解耦：`host_gateway` 目前只服务本机模型地址），按需注入 `--add-host host.docker.internal:host-gateway`；监听地址需覆盖 macOS（Docker Desktop 的 `host.docker.internal` 可达宿主 loopback）与 Linux（需绑定网桥可达地址，宿主 loopback 不可达）两种形态——**P0 实测确定绑定策略**，见各 Harness 计划的 P0 清单。
   - **已定边界（禁网 Profile）**：冻结网络为 `Disabled` 时 `docker.rs` 使用 `--network none`（`docker.rs:888-889`），宿主桥**物理不可达**；`host_gateway` 额外要求 `spec.network`（`docker.rs:620-624`）。→ **本期明确不支持**「禁网 Profile 下的步骤工具」：桥开关只与 `host_gateway` 解耦，不与网络策略解耦；**「冻结网络为 Disabled 的 Profile 下步骤工具不可用」写入 `limitations`**，并在 Profile 校验处给出明确拒绝原因。Linux-only unix socket 桥与「受限网络」方案列为后续演进议题，本期不做（socket 桥在 macOS Docker Desktop 不可用，会让验收环境分裂）。
7. **验收底线**：容器内只能访问该桥端点与被授权的外部地址；桥令牌不可从容器外泄后仍可用（撤销即失效）；桥发起的工具调用必须与 provider 发起的工具调用**同等可审计**（第 9 条）。
8. **统一执行口径（已定）：桥调用一律经 core 接纳/审计链路**——桥只负责**传输与寻址**，不执行工具、不自造审批、不旁路治理记录。链路与 Codex/DSH 完全同构：

   1. **投递**：MCP 服务端收到 `tools/call` → 校验令牌与会话 → 生成 `call_id` → 把调用投递到该会话 worker 的**桥收件箱**（有界，与既有 `a.buffered` 同量级上限，超限报错）→ HTTP 处理器等待回执（超时 = 工具执行预算 + 余量）。
   2. **观察**：worker 的 `poll()` 在抽取协议帧的同时抽取桥收件箱 → 产出与既有形状**完全一致**的观察：
      `Obs::ToolCall{turn, request:{"kind":"bridge","call_id":"<id>"}, thread:<会话 external_session>, external_turn:<轮次的宿主生成 id>, call:<call_id>, tool:<按种类取名的工具名>, arguments:<MCP arguments>, worktree}`。
      `request` 是 `Value`（`ports.rs:220`），用 `kind` 判别传输来源：Codex/DSH 是 JSON-RPC id 形状，桥是本形状 → 回执去向可无歧义分派。`tool` 的取值规则见第 4 条（集成/未绑定 → `qualified_name`；平台/上下文 → `call_name()`），core 既有分派据此自动选择接纳路径。
   3. **接纳**：core 走既有链路——`validate_provider_call_scope`（轮次 / 线程 / 外部轮次身份比对，`conversation_observations.rs:338-369`）、冻结工具解析（`qualified_name` 或上下文工具）、`provider_tool_intent` 治理记录（持久化意图，`conversation_observations.rs:733-750`）。越权工具、跨轮次调用、已停止轮次一律在这里被拒。
   4. **执行**：core 的工具调用驱动器（`crates/core/src/integrations.rs:365-440 drive_tool_calls`）经 `tool_executor.start/poll` 执行（端口 `ports.rs:303-315`，实现是 adapters 的 `NativeTools`），产出 `ToolReceipt`；执行在事务外，回执落盘。
   5. **回执**：core 调 `harness.answer_tool_call(session, request, &receipt)`（`tool_calls.rs:628`、`ports.rs:37-47`）→ 适配器识别 `request.kind == "bridge"` → 交回桥的 pending 表 → 唤醒 HTTP 处理器 → MCP 结果（`content` + `structuredContent`，失败置 `isError`）。
   6. **幂等与审计**：意图持久化 + `replay_commit` 指纹 → 同一 `call_id` 重放不重复执行；工具调用进入会话记录（`ConversationEventKind::Tool`）与治理记录 → 可审计、可追溯。

   - **不新增 core 端口**：桥的观察由 **worker（适配器）** 产出，仍是合法的 `Obs` 生产者；回执复用既有 `Harness::answer_tool_call` 与 `Harness::answer_interaction` 两个端口，只是 `request` 的判别式不同 → core 侧无需新增「服务端发起观察」的入口。
   - **线程模型**：MCP HTTP 处理器阻塞在等待回执上；core 的轮询循环独立运行（provider 侧仍在等待 MCP 结果，轮次保持 `running`）→ 不产生死锁。前提是 worker 在 provider 请求挂起期间仍持续轮询（现有 `poll()` 循环已满足）。
   - **撤销与超时**：会话撤销或轮次取消 → 所有在途桥调用立即失败并向容器返回 `isError`；回执超时未达 → MCP 返回明确的可重试错误，**不得**在宿主侧重复执行（幂等由 `call_id` 兜底）。
9. **桥的验收判据（三个计划共用，缺一不可）**：
   1. **往返**：容器内一次 `tools/call` 能拿到 MCP 结果，且宿主侧产生 `ToolReceipt`。
   2. **可审计**：该次工具调用出现在**会话记录**中（`ConversationEventKind::Tool`），且存在对应的 `provider_tool_intent` 治理记录。
   3. **受冻结权限约束**：不在冻结 `tool_refs` 内的工具名被拒；只读 Profile 下写入类工具被拒。
   4. **幂等**：同一 `call_id` 重放不产生第二次执行。
   5. **撤销**：撤销会话令牌后，桥调用立即失败。
   6. **审批**：`on-request` 下需要人工裁决的桥调用，经 UI 裁决后能正确放行或拒绝（`never` 下自动拒绝且不弹窗）。
   7. **脱敏**：桥令牌与工具参数不出现在协议日志与桥日志中（共享地基 2.2.5）。
   8. **平台工具**：在有 controller 授权或 workflow 执行归属的会话里，平台工具调用被正确路由到平台准入路径并完成；在两者皆无的普通讨论会话里，平台工具调用被**明确拒绝**（`tool_not_authorized` 语义），MCP 返回可读原因，模型不反复重试。

### 2.5 Schema、全局库与出厂配置

1. `sql/global/schema.sql`：`runtime_installations.provider` CHECK 扩到 `('codex','dsh','pi','mcode','zcode','opencode')`；`PRAGMA user_version` 按第 3 章的合并决策一次性推进。
2. `crates/host/src/catalog.rs` 启动版本检查接受新版本号；新增离线命令 `devdesk-host storage upgrade-global --data PATH`（核对宿主已停止 → 重建 `runtime_installations` → 推进 `user_version` → 保留既有行）。
3. `schemas/rpc.schema.json`（六处 provider 枚举）、`schemas/permission-backend.schema.json`（两处）同步枚举。
4. 出厂配置：`plugins/core/harnesses/builtin-harness-mcode-managed-v1.json`、`builtin-harness-zcode-managed-v1.json`、`builtin-harness-opencode-managed-v1.json`，对应 `builtin-model-catalog-{mcode,zcode,opencode}-v1.json`；登记 `plugins/core/manifest.json`；`scripts/check/configuration_usage.py --write` 重生成 `reference-usage.json`；`tests/fixtures/configuration/engineering-catalog.json` 同步（`schema_contracts` 会遍历全部 provider）。
5. 目录 `recipe_digest` 取自 `examples/export_runtime_recipes.rs` 输出（该示例改用 `HARNESS_PROVIDERS`）。
6. **配方生成面扩展（两条新线的前置改造，四条线共用）**：现有 `compose_execution_recipe` 只有一条安装通道——`RUN npm install --global --omit=dev {package}@{version}`（`crates/domain/src/execution_recipe.rs:219-222`），且 `HarnessInstallation.package` 必填非空（`:119-129`、校验 `:165-171`）；`native_releases` 只服务**宿主侧登录组件**（`runtime_preparation.rs:216-230`），不是镜像安装通道。要同时支撑 mcode 的 npm 安装参数与 opencode 的发行二进制，需扩展为：
   - **npm 参数面**：`SandboxBuild`/`HarnessInstallation` 增加 npm 安装参数字段 → 配方生成按字段拼装 → 进 `ExecutionRecipe::validate` 与 `recipe_digest`（mcode 需要 `--include=optional --allow-scripts=@minimax-ai/code,better-sqlite3`）。
   - **二进制发行物安装面**：`package` 可选化 + 新字段（url/sha256/member）→ Dockerfile 生成下载 → 校验 sha256 → 解包 → 可执行文件落位并入 PATH → 进校验与 `recipe_digest`（opencode 需要 v2.4.1 的 `opencode-linux-{x64,arm64}.tar.gz`）。
   - **副作用（必须记账）**：`recipe_digest` 的输入是 files 列表 → 配方生成一变，**全部 provider 的配方摘要都变** → codex/dsh 已装运行要重新绑定（与第 3 章的权限资产漂移合并为同一轮重新绑定，不额外增加轮次）。

### 2.6 前端与测试基建

1. 前端：`RuntimeSettings.tsx`（类型与下拉）、`AgentCatalog.tsx`、`RuntimePreparationStatus.tsx`、`Integrations.tsx`、`RoleConversation.tsx`、`App.tsx`（凭据键前缀）；`DshProviderSettings.tsx` 改为按 harness 渲染，供 dsh/pi/mcode/zcode/opencode 共用。
2. 测试支撑：`crates/adapters/tests/support/mod.rs`（`DEVDESK_TEST_MCODE_IMAGE`、`DEVDESK_TEST_ZCODE_IMAGE`、`DEVDESK_TEST_OPENCODE_IMAGE`）、`tests/support/runtime.rs`、`tests/entrypoints/native_acceptance.py`、`tests/entrypoints/doctor.py`。

## 3. 全局库版本与旧数据集升级

1. **版本合并决策（已定）**：**一次全上**——pi 计划与三个新 provider 同期落地，`user_version 24 → 25` 一次推进，`runtime_installations.provider` CHECK 直接写成 `('codex','dsh','pi','mcode','zcode','opencode')`，`storage upgrade-global` 一次完成重建与推进。理由：避免连续多次重建表与多次权限合同漂移；代价是单个变更集较大、回归面最宽，且 zcode 线的落地以其 P0 门控结论为前提（若 zcode 被证伪，CHECK 仍为六个 provider 但 zcode 线不实施）。本文件第 3 章是版本号与 CHECK 取值的**唯一权威口径**，pi 与三份 Harness 计划只引用。
2. 离线升级命令 `devdesk-host storage upgrade-global --data PATH`：核对宿主进程已停止 → 重建 `runtime_installations`（新 CHECK）→ 推进 `user_version` → **保留全部既有行**（现有 `data/` 的 3 条运行安装）。启动版本检查同步接受 `0|24|25`（或含 26/27/28，依 1 决策）。
3. `tests/entrypoints/dataset_protocol.py` 扩展覆盖升级路径。
4. 升级后 codex/dsh（及 pi）权限绑定合同漂移：通过设置页重新应用绑定（RPC `permission.binding.preflight` + `permission.bindings.apply`）；既有冻结会话需创建后继会话。
   - **漂移轮次口径**：每新增一个权限后端都会改动 `asset_digests()`（`crates/adapters/src/permissions.rs:42-45`，覆盖 `permissions/backends/manifest.json` 全文与全部资产摘要）→ codex/dsh 的 `enforcement_contract` 漂移、冻结会话 `permission_asset_mismatch`。**推荐把所有新后端资产一次性登记、合并为一次升级**；若必须逐级落地，则**每一级**都要重跑「重新应用绑定 → 重建冻结会话 → codex/dsh 各跑一轮」，并在验收记录里写明漂移轮次。
5. 验收阶段对 `data/`、`data1/` 实际执行升级并保留原库备份；`devdesk-recovery/20261119-large-snapshot/` 快照按需另行执行同一命令。
6. **唯一权威口径**：全局库版本号与 `runtime_installations.provider` CHECK 的目标值**只以本章为准**；pi 计划与三份 Harness 计划的「全局库」决策行只引用本章，不各自写死版本号或 CHECK 取值，避免出现两个互相矛盾的 25。

## 4. 未决问题与后续关注点

| # | 问题 | 影响 | 处置 |
|---|---|---|---|
| 1 | mcode 在 Linux 容器内能否启动（无 linux 原生预编译） | 决定 mcode 计划是否成立 | mcode 计划 P0.1；失败则回到用户决策（换发行物 / 换基础镜像 / 放弃 mcode） |
| 2 | `PreToolUse` Hook 能否真正阻断工具调用（mcode 与 zcode） | 决定冻结策略能否在容器内落实 | mcode 计划 P0.4 / zcode 计划 P0.6；不能则如实降级并写入 `limitations` |
| 3 | ACP `session/new` 的 `mcpServers` 在容器内能否连通宿主端点 | 决定步骤工具一期能否打通 | mcode 计划 P0.3；备选见 mcode 计划 §2.3 第 5 条；opencode 侧优先改用配置文件注入 |
| 4 | 宿主 MCP 桥在 macOS/Linux 的监听地址策略 | 决定桥的可用性与暴露面 | mcode 计划 P0.3 双形态实测；独立监听器 + 每会话令牌 |
| 5 | ZCode Protocol 的权威终态事件与权限应答形状 | 决定 zcode 计划能否进入 P1 | zcode 计划 P0.2/P0.4；未确认即停止 |
| 6 | zcode 随包专有运行时的再分发条款是否允许进入 DevDesk 镜像 | 合规风险 | zcode 计划 P1 前由维护者/法务确认；至少完整携带 `LICENSES/` 与声明 |
| 7 | 与 pi 计划的全局库版本合并 vs 逐级升级 | 影响一次性影响面与验收工作量 | **已定：一次全上**（24→25，CHECK 含六个 provider）；见第 3.1 条 |
| 8 | pi 的 UI 隧道与新宿主 MCP 桥是否合并为同一套桥接实现 | 长期一致性 | **执行口径已统一**（都经 core 接纳/审计链路）；差异只在传输，合并为同一实现列入后续演进议题，本期不做 |
| 9 | opencode ACP 模式是否隐式使用后台服务 | 多会话共享服务进程，破坏会话隔离 | opencode 计划 P0.3；必要时 `--standalone` 或改走其 v2 HTTP/SSE 备选路线 |
| 10 | opencode 发行二进制在容器内是否可直接运行（glibc/依赖） | 决定 opencode 计划是否成立 | opencode 计划 P0.1；注意 `native_releases` 只是**宿主侧登录组件**的下载通道（`runtime_preparation.rs:216-230`），**不是镜像安装通道**——容器安装仍由 `compose_execution_recipe` 的 npm 行决定，见 #13 |
| 11 | 三条接入线的实施顺序与人力分配（mcode/opencode 优先，zcode 门控） | 影响交付节奏 | 建议 mcode → opencode → zcode；版本口径已定「一次全上」（第 3.1 条），实施顺序仍可按此建议推进 |
| 12 | 宿主 MCP 桥的调用执行路由：(a) 转发到 core 既有接纳链路，还是 (b) 桥作为受控执行器并自行写入会话事件 | 决定步骤工具是否可审计、可幂等重放 | **已定 (a)**：经 core 接纳/审计链路（共享地基 2.4.8）；桥只做传输与寻址，不直接执行工具 |
| 13 | 是否新增「二进制发行物」安装面（`HarnessInstallation.package` 可选 + 新字段 + Dockerfile 生成下载/校验/解包 + 摘要校验） | 决定 opencode 能否用 v2 发行二进制 | **已定：新增**（共享地基 2.5.6）。opencode 用与本机同版本的 v2.4.1 发行二进制（本地为 240 MB Mach-O arm64 单体，非 npm v1 线），容器取同版本 linux 发行件并钉 sha256 |
| 14 | ACP 的接纳（`Obs::Admitted`）语义 | 决定流式输出与长轮次是否可用 | **已定**：由宿主在提交 `session/prompt` 时合成 `Admitted{external_turn: 宿主生成的轮次 id}`，把 `session/prompt` 应答改判为终态（见 mcode 计划 §2.2「接纳语义」条）；ACP 的 `session/prompt` 应答在轮次**结束时**才到，不能沿用 Codex/DSH「立即回执即接纳」的骨架 |
| 15 | 冻结网络为 Disabled 的 Profile 下步骤工具是否可用 | 决定「一期必须打通步骤工具」的适用边界 | **已定：本期明确不支持**（共享地基 2.4.6）——写进 `limitations` 并在 Profile 校验处给出明确拒绝原因；socket 桥与受限网络方案列为后续演进议题 |
| 16 | mcode/zcode 的容器内策略钩子**真实发现路径** | 决定冻结策略能否在容器内落实（静默失效最危险） | mcode/zcode/opencode 计划均写「挂到 `/run/devdesk/<x>-hooks/`」，但运行时是否从该路径发现钩子**未确认**；P0 必须先确定发现路径，再决定挂载目标（现有 `docker.rs` 只有 `/run/devdesk/permission` 与 `/run/devdesk/tool-policy.mjs` 两个固定挂载目标，目录型挂载需新增能力与目标白名单） |

## 5. 实施阶段的通用约束

1. 先读 `AGENT.md` 与 `docs/design/`，保留既有改动；改动行为时同步更新合同与当前文档。
2. 不把凭据、运行数据、日志写入版本库；桥令牌、API key 只在容器内短生命周期存在，日志一律脱敏。
3. 镜像构建固定版本与摘要；`compose_execution_recipe` 产出的 Dockerfile 与锁文件摘要进入出厂配置。
4. 涉及全局库变更时，先在副本数据集演练升级，再对真实数据集执行并保留备份。
5. 每次改动后运行受影响的最小测试集，最后跑 `./scripts/check/check.sh` 全量；报告写明命令、制品版本、实际结果与未验证范围。
