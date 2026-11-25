# zcode Harness 接入（ZCode CLI）

日期：2026-11-25。状态：**待实施（受 P0 门控）**。范围：接入 ZCode CLI（`zcode-app-cli`，MIT 启动器 + 随包专有运行时）为第五个 Harness，能力对齐 DSH：讨论会话、workflow 步骤工具、模型服务商。不含根编排。

前置依赖：[Harness 扩展共享地基](../harness-shared-foundation/PLAN.md) 必须先完成（Provider 注册、传输层扩展、凭据泛化、宿主侧 MCP 服务端）。本计划只写 zcode 自身的运行形态、协议映射、权限资产、模型配置与工作项。

> **门控**：本计划整体以第 3 章 P0 的实跑结论为门。ZCode Protocol 未公开文档化，若 P0 证明「无法稳定驱动一轮对话并取回终态」，则**不进入 P1**，改为在 `docs/plans/` 留一份「不可行/需上游支持」的结论记录，并回退到已文档化的一次性接口路线（见第 5 章备选路线）。

## 0. 已确认决策

| 决策 | 结论 |
|---|---|
| 驱动协议 | **ZCode Protocol**（`zcode app-server`，常驻 stdio）；`zcode -p --output-format stream-json` 仅作备选路线 |
| 工具桥 | 一期打通：宿主侧 MCP 服务端承载 workflow 步骤工具，zcode 通过 `setting.json` 的 `mcp.servers` 接入；执行、回执与冻结权限核验全部在 Host 侧 |
| 凭据所有权 | `ZCODE_APP_SERVER_AUTH_MODE=host`——宿主负责账号更新与请求凭据刷新，服务端不读独立凭据文件 |
| 权限模式 | ZCode 默认 `--prompt` 下为 `yolo`，**必须显式覆盖**；审批映射到冻结审批策略 |
| 根编排 | zcode 不获得 `orchestration` 能力（与 DSH 一致） |
| Agent 提问 | ZCode 有 `interaction/requestUserInput` 通道，但**不进一期验收** |
| 许可声明 | `license` 字段必须写实际组合（启动器 MIT + 运行时 Apache-2.0 与 ZCode 声明），镜像携带 `LICENSES/` |
| 全局库 | **一次全上**：24 → 25，`provider` CHECK 含 `pi,mcode,zcode,opencode`；口径以共享地基第 3 章为准 |

## 1. 调研结论（已核实来源）

### 1.1 来源

| 证据 | 来源 |
|---|---|
| zcode 版本/命令面 | 本机 `zcode`（zcode-app-cli 3.18.2-48 / zcode-runtime 0.17.1）`--help`/`doctor` |
| zcode 实现 | 本机包 `~/.nvm/.../node_modules/zcode-app-cli/`（`bin/zcode.js`、`vendor/zcode.cjs`、`docs/{HOST_INTEGRATION,HEADLESS,CONFIGURATION}.md`、`vendor/extraction.json`、`LICENSES/`） |
| zcode 打包元数据 | `https://registry.npmjs.org/zcode-app-cli`（latest 4.0.0-49，MIT 启动器，engines `>=22.19.0`） |
| DevDesk 现状 | 共享地基第 1 章 |

### 1.2 zcode 关键事实

- **安装形态**：npm 包 `zcode-app-cli`（MIT 启动器 + pi-tui 界面），随包内置从官方发行物抽取的 ZCode 运行时（`vendor/zcode.cjs`，15.7 MB；`vendor/extraction.json` 记录来源 `cdn-zcode.z.ai ... ZCode-3.18.2-linux-x64.deb` 与 sha512）。`zcode doctor` 显示 `default artifact: node-bundle`、`sea: no (optional)` → 运行时为 Node 内联 bundle，**与平台无关**，容器只需 Node ≥ 22.19。
- **许可边界**：启动器/TUI 为 MIT；随包运行时与官方一手源码为 Apache-2.0 并附 `LICENSES/ZCode-NOTICE.md`（明确「当前共享 Agent 执行适配器不提供默认的操作系统沙箱」「权限模式不统一覆盖插件/终端」等边界）；文档/演示/表格类插件技能带非商用条款（`docs/THIRD_PARTY_CONTENT.md`）。→ DevDesk 镜像需原样携带 `LICENSES/`，且 Harness 安装声明的 `license` 字段必须写实际组合而非单一 MIT。
- **宿主契约**：`docs/HOST_INTEGRATION.md` 定义 **Host integration contract version 1**：宿主只应启动 `zcode` 可执行文件并透传参数，不得直接调用 `vendor/zcode.cjs` 等内部文件，不得解析 ANSI 屏幕输出；非交互命令只认文档化的命令输出；取消走 SIGINT/SIGTERM/SIGHUP，退出码 130/143/129。
- **两个可用入口**：
  - `zcode app-server`——**ZCode Protocol** stdio 服务端（运行时以 `input: process.stdin, output: process.stdout` 引导）。请求 `{id, method, params}` → 应答 `{id, result}` / `{id, error:{message, code, data}}`（**无** `jsonrpc` 字段）。服务端→客户端通知 `{method:"session/event", params:{type, seq, payload, deliveryKind, eventId}}`。
  - `zcode -p '<task>' --output-format stream-json`——一次性执行，文档要求「退出码 0 且存在终态 `result` 记录」才算完成。
- **ZCode Protocol 方法面**（静态提取）：客户端→服务端 `session/create`、`session/send`、`session/subscribe`、`session/stop`、`session/cancel`、`session/compact`、`session/fork`、`session/resume`、`session/messages`、`session/usage`、`session/setModel`、`session/setThoughtLevel`、`session/setMode`、`session/goal`、`session/subagents`、`session/debug`、`session/events`、`session/lifecycle/{ensureReady,getState,restart,stop}`、`session/requestRuntimePreferences`；服务端→客户端请求 `interaction/requestPermission`、`interaction/requestUserInput`、`interaction/requestProviderRuntimeHeaders`、`interaction/requestOfficialMcpAuthHeaders`、`interaction/requestCredentialCipher`；客户端→服务端应答 `interaction/respond`，`type` 取值含 `respond_permission`（`permissionRequestId` + `optionId` + `response`）、`respond_elicitation`、`stop_generation`。
- **凭据所有权模式（关键契合点）**：`ZCODE_APP_SERVER_AUTH_MODE=host` 时，宿主负责账号更新与请求凭据刷新，服务端不再读独立凭据文件；文档明确「Host mode 保留 `provider/updateAccountConfig` 与 `interaction/requestProviderRuntimeHeaders`」——与 DevDesk「凭据由 Host 冻结持有并按需下发」的模式一致。
- **配置面**：`ZCODE_CLI_SETTINGS_FILE`（用户设置，含 `mcp.servers`、hooks、默认工具权限）、`ZCODE_PERSONAL_PROVIDER_CONFIG_FILE`（自定义 provider 与模型）、`ZCODE_SESSION_DB_PATH`、`ZCODE_CLI_CREDENTIALS_FILE`。
- **步骤工具交付面**：ZCode Protocol 中**未发现**宿主提供工具的注册方法（`zcode/toolSurface` 实为 node-repl 的响应元数据通道；`hostToolExec*` 属后台任务子系统）→ 动态工具同样只能走 MCP（`setting.json` 的 `mcp.servers`，支持 http/sse/stdio 与自定义请求头）。
- **容器内策略钩子**：官方 ZCode 声明支持 `SessionStart`、`UserPromptSubmit`、`PreToolUse`、`PermissionRequest`、`PostToolUse`、`PostToolUseFailure`、`Stop` 生命周期 Hook（Hook 可改写工具输入并参与权限决策）→ 与 mcode 同位，可承载冻结策略。
- **风险定性**：ZCode Protocol **不是公开契约**——`HOST_INTEGRATION.md` 只版本化了「进程边界」，`CONFIGURATION.md` 明说「ZCode 选择模式于启动并保留其原生协议；它不实现 Codex 的 `account/login/start`/`account/read`」。npm 启动器自身就是该协议的非公开消费者（`bin/zcode.js` 每次请求 spawn 一个 app-server 进程）。→ 依赖它是**版本绑定的私有耦合**，必须以 P0 实跑结论为门。

### 1.3 未实跑验证的部分

以上事实来自本机已安装包的静态阅读与文档，**未在容器内实跑**。P0（见第 3 章）验证前，协议细节不视为已证实的运行行为。

## 2. 方案设计

### 2.1 运行形态与协议

- **Harness 源**：`plugins/core/harnesses/builtin-harness-zcode-managed-v1.json`
  - `data.provider: "zcode"`、`runtime_version: "0.17.1"`（zcode-runtime；分布版本 `3.18.2-48`）、`installation.package: "zcode-app-cli@3.18.2-48"`。
  - `license` 字段必须如实写组合：启动器 MIT + 运行时 Apache-2.0（附 ZCode 声明）；`installation.files` 需携带 `LICENSES/`（`Apache-2.0.txt`、`ZCode-NOTICE.md`、`ZCode-THIRD-PARTY-NOTICES.md`、`README.md`）与 `docs/THIRD_PARTY_CONTENT.md`。
  - `installation.environment`：`ZCODE_APP_SERVER_AUTH_MODE=host`、`ZCODE_CLI_SETTINGS_FILE=/state/zcode/setting.json`、`ZCODE_PERSONAL_PROVIDER_CONFIG_FILE=/state/zcode/provider_config.json`、`ZCODE_SESSION_DB_PATH=/state/zcode/db.sqlite`、`ZCODE_CLI_CREDENTIALS_FILE=/state/zcode/credentials.json`（host 模式下不使用独立凭据文件）、`ZCODE_DISABLE_UPDATE_CHECK=1`、`NO_UPDATE_NOTIFIER=1`。
  - `installation.entrypoint: ["zcode", "app-server"]`（不得带 `--browser-use`，启动器本就不向 app-server 注入 Browser Use）。
- **容器命令**：cwd 固定 `/work`；stdout/stdin 全部是协议流，stderr 只作诊断。
- **镜像配方**：基础镜像 Node 24.13（≥22.19 满足）；`npm install -g zcode-app-cli@3.18.2-48 --omit=dev`（该包装无原生 PTY 插件、无 postinstall 构建步骤）。
  - 体积：包约 155 MB（vendor 137 MB），镜像需留足空间。
- **会话身份**：`session/create` 返回的 session id → DevDesk `external_session`；恢复走 `session/resume`/`session/lifecycle/ensureReady`。

### 2.2 协议与事件映射（ZCode Protocol → `Obs`）

| ZCode Protocol | 观察 |
|---|---|
| `session/create {cwd, workspace...}` 应答 | 会话外部 id 建立 |
| `session/subscribe` | 订阅事件流（后续 `session/event` 通知） |
| `session/send {sessionId, message...}` | 已接纳轮次（`Admitted`，轮次 id 由 Host 生成） |
| `session/event` 通知中的增量文本 | Assistant `Delta`（合批与尾部脱敏复用 Codex 逻辑） |
| `session/event` 中的工具事件 | Tool `Full` |
| `session/stop`、`session/cancel` | 停止当前轮次（宿主→服务端） |
| `session/setMode`、`session/setModel`、`session/setThoughtLevel` | 会话内模型/模式调整（冻结绑定优先） |
| `interaction/requestPermission` | `Obs::Interaction{ToolApproval}` |
| `interaction/requestUserInput` | 一期**不产生** `Obs::Interaction`（见下方「交互的处理边界」）；仅在协议日志留痕 |
| `interaction/requestProviderRuntimeHeaders` | **宿主凭据下发点**（host auth 模式）：按冻结连接返回请求头（值来自 Provider 绑定，不落日志） |
| `interaction/respond {type:"respond_permission", permissionRequestId, optionId, response}` | 宿主对权限请求的应答 |
| 轮次终态事件 | `Terminal`（Completed / Failed / Cancelled）——**终态事件名 P0 必须实测确认**（ZCode Protocol 未文档化，不能凭猜测实现） |

- **协议形状注意**：ZCode Protocol 的帧**不带** `jsonrpc` 字段（与 ACP 相反）→ `jsonl.rs` 需要第三种线模式；错误在 `{id, error:{message, code, data}}`。
- **接纳语义待实测**：既有骨架把「对宿主请求的应答帧」当作接纳凭证（Codex/DSH 都是立即回执）。ZCode Protocol 未文档化，`session/send` 是否**立即**回执未知 → 与 ACP 同类风险（若应答在轮次结束才到，事件会被缓冲、流式退化、长轮次超缓冲上限）。**P0.2 必须同时确认「应答时机」**；若为轮次末回执，按 mcode 计划 §2.2 的口径改为宿主在提交 `session/send` 时合成 `Obs::Admitted{external_turn: 宿主生成的轮次 id}` 并写入 turn 记录（`conversation_observations.rs:338-369` 的 `turn["external_turn"]` 比对依赖它）。
- **交互的处理边界**：core 对任何 `Obs::Interaction` 都会创建 `PendingInteraction` 并把轮次置为 `WaitingInteraction`（`conversation_observations.rs:641-688`），必须经 `answer_interaction` 应答才能继续 → 「一期只记录」在现有 core 下**不成立**。一期口径：`interaction/requestUserInput` 由适配器**直接应答**（取消/拒绝），**不产生** `Obs::Interaction`，只在协议日志留痕；`interaction/requestPermission` 仍走完整审批流。
- **事件序号**：`session/event` 带 `seq`/`eventId`，可作为单调性核对依据（乱序或缺号需记录为协议异常，不得静默丢弃）。
- **失败判定**：终态事件缺失但连接断开 → `needs_reconciliation`（不得当作成功）。

### 2.3 工具桥与审批（一期打通）

1. **投放**：宿主写 `/state/zcode/setting.json` 的 `mcp.servers`：
   ```json
   {"mcp":{"servers":{"devdesk-tools":{"type":"http",
     "url":"http://host.docker.internal:<bridge-port>/internal/tool-bridge/<session>/mcp",
     "headers":{"Authorization":"Bearer <session-token>"}}}}}
   ```
   动态工具由宿主 MCP 服务端暴露（与 mcode/opencode 共用共享地基 2.4 实现）。
2. **执行（统一口径：经 core 接纳/审计链路）**：桥**不执行工具**。`tools/call` 由 MCP 服务端投递到该会话 worker 的桥收件箱 → worker 产出 `Obs::ToolCall{request:{"kind":"bridge","call_id":…}, …}` → core 走既有接纳（冻结 `tool_refs`、`qualified_name` 解析、`provider_tool_intent` 治理记录、轮次状态与 guards 版本）→ `ToolExecutor` 执行 → `harness.answer_tool_call` 回执 → 桥返回 MCP 结果。完整机制见共享地基 2.4.8。
3. **审批**：ZCode 自身权限模式（`build`/`edit`/`plan`/`yolo`/`guarded`）→ 冻结审批策略映射：`never` → 选择最严格且不弹窗的模式 + 宿主侧拒绝对应升级；`on-request` → 走 `interaction/requestPermission` → 用户裁决 → `interaction/respond`。
   - ZCode 的 `--mode` 默认在 `--prompt` 下是 `yolo`，**必须显式覆盖**，不能依赖默认值。
4. **一致性与降级**：同 mcode 计划 2.3（同一份冻结权限、同一套脱敏、同一撤销语义）；**执行口径与 mcode/opencode/pi 一致**——桥调用经 core 接纳/审计链路，工具调用必须在会话记录与治理记录中可审计。
   - 桥触发的审批同样走既有交互流：worker 产出 `Obs::Interaction{ToolApproval, request:{"kind":"bridge",…}}` → core 建 `PendingInteraction` → 用户裁决 → `harness.answer_interaction` → 按 `kind` 交回桥。
   - **暴露的工具范围（四类全暴露，含平台工具）**：桥产出观察时 `tool` 按种类取值——集成/未绑定工具用 `qualified_name`，**平台工具用 `call_name()`**；core 已按名分派（`conversation_runtime.rs:334-345`），平台工具自动走平台准入路径，**不需要新增 grant 路由或端口**。平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用，普通讨论会话被 `tool_not_authorized` 拒绝 → 写入 `limitations`。
   - **禁网边界（本期明确不支持）**：冻结网络为 Disabled 的 Profile 下容器为 `--network none`，宿主桥物理不可达 → 步骤工具不可用，写入 `limitations`（共享地基 2.4.6）。
   - 注意 ZCode 的 `mcp.servers` 用的是 `headers` **对象**形式（`{"Authorization":"Bearer …"}`），与 ACP 的**数组**形式（`[{name,value}]`）不同 → 结构化脱敏要覆盖两种形态（共享地基 2.2.5）。

### 2.4 权限与策略后端

1. `permissions/backends/zcode/hooks/hooks.json` + `tool-policy.mjs`：读 `/run/devdesk/permission`，在 `PreToolUse`/`PermissionRequest` 上落实冻结策略（契约 **P0 实测**；ZCode 声明 Hook 可改写工具输入并参与权限决策）。
2. `capabilities(Zcode)`：filesystem 三档；network 仅 `enabled`；tools `all_available` + `restricted`；`limitations` 如实列出：**无进程内宿主工具注册通道（步骤工具走宿主 MCP）**、审批为逐次询问、cwd 固定 `/work`、默认权限模式需显式覆盖、**冻结网络为 Disabled 的 Profile 下步骤工具不可用（宿主桥在 `--network none` 下不可达）**、**平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用**。
3. 校验分支：`validate_execution_permissions` 的 zcode 分支（拒绝 `Disabled` 网络；scoped 规则必须由冻结挂载落实）。
4. 执行合同命名：后端 `zcode-hooks-v1`，不使用 `native` 字样。
5. 资产登记与 `protected_metadata` 加入 `.zcode`。**三处必须同步**：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`（**硬编码白名单** `require([".git",".codex",".agents"].contains(&leaf))`）；manifest 条目需写全 `id`（`zcode-hooks-v1`）、`provider`（`zcode`）、`version`、`assets`（sha256）。`.zcode` 是否真的需要加入 `protected_metadata` 取决于 zcode 是否在项目目录内写该目录（`protected_metadata` 只作用于 `/work` 下的叶子目录），须先核实。

### 2.5 模型与凭据

1. 新建 provider 源：`zcode-minimax-cn`、`zcode-zai-coding-cn`、`zcode-deepseek`、`zcode-ollama`（与 mcode 同样保持公开 id 全局唯一）；新建 Model 源与 `builtin-model-catalog-zcode-v1.json`。
2. 宿主写 `/state/zcode/provider_config.json`（`ZCODE_PERSONAL_PROVIDER_CONFIG_FILE`）：自定义 provider（`base_url` + API key + 模型规则）。字段语义按包内 `docs/PROVIDER_CONFIG.md` 写（P1 读取该文档定稿）。
3. 凭据：按共享地基 2.3 泛化后的键 `credentials.zcode.{provider}`；host auth 模式下由宿主在 `interaction/requestProviderRuntimeHeaders` 时下发，容器内不留长期凭据文件。

## 3. 工作项

### P0 可行性验证（门控；任一硬性项失败则本计划停止）

1. 手工构建 zcode 镜像：`npm install -g zcode-app-cli@3.18.2-48` 是否干净；容器内 `zcode --version`、`zcode doctor`（`default artifact: node-bundle`）是否正常；运行时是否真的平台无关。
2. 容器内跑一次 ZCode Protocol 最小闭环：`session/create` → `session/subscribe` → `session/send` → 收到 `session/event` 增量文本 → **确认权威终态事件** → `session/stop`。
   - **同时确认「`session/send` 的应答时机」**：立即回执还是轮次末回执（决定接纳语义与流式是否成立，见 §2.2）。
3. 确认凭据模式：`ZCODE_APP_SERVER_AUTH_MODE=host` 下 `interaction/requestProviderRuntimeHeaders` 的请求/应答形状，以及自定义 provider（API key）是否被接受。
4. 确认权限通道：`interaction/requestPermission` 的选项结构与 `interaction/respond{respond_permission}` 的应答形状；`--mode` 各档对工具调用的实际影响。
5. 确认 `setting.json` 的 `mcp.servers`（http + headers）能被运行时加载并从容器内连到宿主 MCP 桥端点。
6. 确认 Hook 契约：`PreToolUse` 能否阻断工具调用；**并确认 zcode 运行时从哪里发现钩子**（`ZCODE_CLI_SETTINGS_FILE` 指向的设置目录？项目级目录？）——发现路径不定，冻结策略会**静默不生效**。
7. **硬性判定**：以上 2/3/4 任一无法在 1 小时内稳定复现 → 停止本计划并出结论（不得用「大概能跑」推进）。

### P1–P4

与 mcode 计划同构（provider 注册 / 显式分支 → 协议驱动 → 工具桥与审批 → 验收与文档），差异点：

- **P1**：`jsonl.rs` 新增 ZCode 线模式（无 `jsonrpc` 字段）+ `interaction/*` 入站请求应答；`permissions/backends/zcode/` 资产；`zcode_model_patch`（写 `provider_config.json` 与 `setting.json`）；`runtime_verification.rs` 的 zcode 探针（`session/create` + `session/lifecycle/getState`，无密钥无网络）；`default_zcode_recipe_digest`；出厂配置与模型目录；凭据泛化；前端复用按 harness 渲染的 provider 设置页。
- **P2**：`crates/adapters/src/harness_zcode.rs`（启动输入、事件投影、停止模式、凭据注入），`harness.rs` 委托分支；协议测试用例（接纳、消费、增量与终稿封存、工具事件、三种终态、权限往返）配合脱敏样本 `tests/fixtures/protocols/zcode-session-turn.json`。
- **P3**：宿主 MCP 桥（复用共享地基 2.4 实现，此处只做 zcode 侧注入与分账）；`setting.json` 写入；权限应答。
  - 桥收件箱、观察产出（`Obs::ToolCall{request:{"kind":"bridge",…}}`）、回执按 `request.kind` 路由、桥审批经 `PendingInteraction` 往返——**全部复用 mcode 计划 P3 的实现**，zcode 侧只提供差异配置；执行语义与 mcode/opencode/pi 完全一致（共享地基 2.4.8）。
  - zcode 特有的确认点：`interaction/respond` 的应答形状与桥侧回执形状互不影响（桥走 MCP，provider 审批走 ZCode 协议线），需分别测试。
  - 验收须覆盖共享地基 2.4.9 的 8 条桥判据（往返、会话记录 + 治理记录可审计、越权拒绝、幂等、撤销、审批、脱敏、**平台工具的路由与无授权时的明确拒绝**）。
- **P4**：`native_acceptance.py` 的 zcode 套件、probe provider 选项、浏览器枚举 spec、文档更新（并在本计划中记录 ZCode Protocol 的未文档化事实与版本绑定风险）。

## 4. 验证

- 自动检查同 mcode 计划第 4 章（同一套命令）。
- 真实运行（`./scripts/dev/dev.sh --data ./data-zcode --port 4319`）：
  1. 镜像准备与协议探针通过；provider API key 可用。
  2. 讨论轮次：流式输出、终稿、终态（Completed/Failed/Cancelled 三种都要能触发并正确分类）。
  3. 工具事件入账；只读 Profile 下写入被拦截且原因明确。
  4. 步骤工具（宿主 MCP 桥）往返 + 宿主回执，且该调用**出现在会话记录与治理记录中（可审计）**；同一 `call_id` 重放不重复执行。
  5. 取消轮次 → 停止模式正确、容器移除。
  6. 宿主重启后 `session/resume` 恢复成功。
- **验收附加**：由于协议未文档化，验收记录必须写明所用 zcode 版本、运行时版本与锁文件摘要（`zcode-runtime.lock.json` 的 appVersion/sha512），并声明「该行为未由上游公开契约保证」。

## 5. 不在本期范围与备选路线

- 根编排（zcode 不获得 `orchestration`，与 DSH 一致）。
- Agent 提问：`interaction/requestUserInput` 一期只记录，不进验收。

**备选路线（仅当 P0 判定失败时启用，需再次评审）**：`zcode -p '<task>' --output-format stream-json`（包内 `docs/HEADLESS.md` 明确文档化，要求退出码 0 且存在终态 `result` 记录）。代价：turn 与进程一一对应，DevDesk 需要「每轮一个容器」的会话模型，`external_session` 语义、停止语义（SIGINT→130）、恢复语义（`--resume`）都要重做设计与验收。

## 6. 风险与兼容

| 风险 | 影响 | 应对 |
|---|---|---|
| ZCode Protocol 无公开契约 | 上游改版即失效；无法追责 | P0 门控；版本钉死 + `zcode-runtime.lock.json` 摘要入档；接受其性质为「版本绑定私有耦合」并在文档明写 |
| npm 包装为**非官方**第三方客户端且随包携带专有运行时 | 供应链与许可风险 | 钉版本 + 记录 sha512；镜像携带 `LICENSES/`；`license` 字段如实声明；不引入官方以外的额外分发 |
| `zcode` 官方运行时默认 `yolo`（`--prompt` 下） | 权限放大 | 容器命令显式 `--mode`/对应会话模式 + 冻结策略 Hook + 宿主侧复核 |
| 终态事件与权限应答形状未知 | 无法正确判定终态/审批 | P0.2/P0.4 必测；未确认前不实现终态映射 |
| 宿主 MCP 桥（与其它计划共用） | 同 mcode 计划第 6 章 | 同 mcode 计划第 6 章应对 |
| `--prompt` 一次性路线（备选） | 会话模型偏离：每轮新进程、容器生命周期变化、恢复语义弱 | 仅在 app-server 路线被证伪时启用，且需单独设计容器/会话生命周期并重新评估验收 |
