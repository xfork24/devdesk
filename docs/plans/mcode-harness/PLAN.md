# mcode Harness 接入（MiniMax Code CLI 0.7.9）

日期：2026-11-25。状态：**已接入批次 B，Linux arm64 正式镜像通过讨论、权限及两步依赖工作流验收。** 范围：接入 mcode 0.7.9（`@minimax-ai/code`，MIT），提供四家模型服务商、讨论会话和 workflow 步骤工具。

前置依赖为 [Harness 扩展共享地基](../harness-shared-foundation/PLAN.md) 的通用管线、ACP 适配器、宿主 MCP 桥和 npm 安装能力。G0 依共享地基第 7 章并行验证，首个 ACP 接入者按 0.5 确定。架构独立性改造并行推进，mcode 按当前基线实施，后期归并按共享地基第 8 章执行。

## 0. 已确认决策

| 决策 | 结论 |
|---|---|
| 驱动协议 | **ACP 标准协议**（`mcode acp`，常驻 stdio，JSON-RPC 2.0）；不采用一次性 `mcode exec --output-format stream-json` |
| 工具桥 | 一期打通：宿主侧 MCP 服务端（streamable HTTP）承载 workflow 步骤工具，mcode 通过 ACP `session/new.mcpServers[{type:"http"}]` 接入；执行、回执与冻结权限核验全部在 Host 侧 |
| 模型服务商 | 智谱编程订阅、MiniMax 订阅、DeepSeek 官方 API、本机 Ollama 全部接入 |
| 根编排 | mcode 不获得 `orchestration` 能力（与 DSH 一致） |
| Agent 提问 | ACP 的 `elicitation/create` 可映射为交互询问，但**不进一期验收** |
| 镜像构建 | 用 P0 验证过的固定 npm 版本，以 `--include=optional` 和严格的安装脚本白名单构建，见 2.1 及共享地基 2.5.6 |
| 全局库 | 版本 24 → 25 在批次 A（pi）一次性完成，CHECK 届时已含 `mcode`；**本计划不改全局库版本**。口径以共享地基第 3 章为准 |

## 1. 调研结论（已核实来源）

### 1.1 来源

| 证据 | 来源 |
|---|---|
| mcode 版本/命令面 | 本机 `/Users/hengliu/.minimax-code/bin/mcode`（0.7.9）`--help`/`exec --help`/`acp --help` |
| mcode 实现 | 本机包 `~/.minimax-code/releases/0.7.9/lib/node_modules/@minimax-ai/code/`（`chunks/run-acp-command-*.js`、`chunks/chunk-ZD2UOSPY.js`、`README.md`、`verify-native-install.mjs`、`native/`） |
| mcode 打包元数据 | `https://registry.npmjs.org/@minimax-ai/code`（latest 0.7.9，MIT，engines `>=22.19 <23 \|\| >=24 <27`，`postinstall: node ./verify-native-install.mjs`，optionalDependencies `better-sqlite3@12.11.1`） |
| DevDesk 现状 | 共享地基第 1 章 |

### 1.2 mcode 关键事实

- **安装形态**：npm 包 `@minimax-ai/code`，MIT；本机为 npm-prefix 布局（`~/.minimax-code/releases/<version>/`，`bin/mcode` 是转发到 `lib/node_modules/@minimax-ai/code/cli.js` 的 sh 脚本）。engines 要求 Node `>=22.19 <23 || >=24 <27`（沙箱基础镜像 Node 24.13 满足）。
- **两个可用入口**：
  - `mcode acp`——ACP（Agent Client Protocol）stdio 服务端；方法表实测含 `initialize`、`authenticate`、`session/new`、`session/load`、`session/resume`、`session/list`、`session/prompt`、`session/cancel`、`session/close`、`session/fork`、`session/set_mode`、`session/set_config_option`、`session/activate`、`session/steer`，以及客户端侧方法 `session/request_permission`、`session/update`、`fs/read_text_file`、`fs/write_text_file`、`terminal/{create,output,release,wait_for_exit,kill}`、`mcp/{connect,message,disconnect}`、`elicitation/create`。
  - `mcode exec --output-format text|json|stream-json`——一次性执行（本次不采用）。
- **ACP 语义**：`session/update` 的 `sessionUpdate` 取值实测含 `agent_message_chunk`、`agent_thought_chunk`、`user_message_chunk`、`tool_call`、`tool_call_update`、`plan_update`、`plan_removed`、`usage_update`、`current_mode_update`、`config_option_update`、`session_info_update`、`available_commands_update`。`session/prompt` 返回 `stopReason`，取值 `end_turn` / `max_tokens` / `max_turn_requests` / `refusal` / `cancelled`。
- **客户端能力**：`initialize` 的 `clientCapabilities` 结构为 `{fs:{readTextFile,writeTextFile}, terminal, auth:{terminal}, elicitation?}`；实测该版本 agent 仅回调 `session/request_permission` 与 `elicitation/create`，**未**回调 `fs/*`/`terminal/*`。
- **步骤工具交付面**：`session/new` 参数结构为 `{cwd, additionalDirectories?, mcpServers?}`，`mcpServers` 元素为 `{type:"http",name,url,headers:[{name,value}]}` / `{type:"sse",...}` / `{type:"acp",name,serverId}` / `{type:"stdio",name,command,args,env}`。→ **动态工具只能以 MCP 服务器形式投放**。包内 ACP schema 虽含 `{type:"acp",name,serverId}` 与 `mcp/connect`、`mcp/message`、`mcp/disconnect`，但 **mcode 0.7.9 未实现**：`configureSessionMcpServers` 只映射 stdio 与 URL 两类，包内没有 `type:"acp"` 分支，也没有 `mcp/connect` 的调用点 → 不能借协议通道转发工具调用，步骤工具只能走宿主 HTTP MCP 桥。
- **容器内策略钩子**：ACP 实测读取 `$MINIMAX_DATA_DIR/hooks.json`。顶层 `enabled: true`，事件组下配置 `type: "command"`。`SessionStart` 和 `PreToolUse` 均执行，工具调用收到带拒绝原因的 `hookSpecificOutput`。
- **数据目录**：运行时读 `MINIMAX_DATA_DIR`（实测存在于包内）→ 可指向容器内 `/state/mcode`；`config.yaml` 位于该目录下。
- **Linux 原生依赖风险（重要）**：包的 `native/` 目录只含 `darwin/{darwin-arm64,darwin-x64}` 与 `win32/{win32-arm64,win32-x64}` 预编译，**无 linux 预编译**；`postinstall` 脚本 `verify-native-install.mjs` 会在 Linux 上校验 `better-sqlite3` 是否可加载并按 Node ABI 要求 glibc（ABI 137/141/147 → glibc ≥ 2.29；127 → ≥ 2.29），失败即 `process.exitCode=1`。→ 镜像构建**不可**用 `--ignore-scripts`，且必须 `--include=optional --allow-scripts=@minimax-ai/code,better-sqlite3`。
- **凭据面**：`mcode provider add --name <id> --base-url <url> --model <model-id>`；官方订阅走 OAuth 登录，DevDesk 场景只应使用 API key 型自定义 provider。

### 1.3 未实跑验证的部分

以上事实来自本机已安装包的源码与文档阅读，**未在容器内实跑**。P0（见第 3 章）验证前，协议细节不视为已证实的运行行为。

## 2. 方案设计

### 2.1 运行形态

- **Harness 源**：`plugins/core/harnesses/builtin-harness-mcode-managed-v1.json`
  - `data.provider: "mcode"`、`runtime_version: "0.7.9"`、`installation.package: "@minimax-ai/code"`、`license: "MIT"`。
  - `installation.entrypoint: ["mcode", "acp", "--append-system-prompt-file", "/run/devdesk/profile"]`（ACP 子命令支持 `--append-system-prompt-file`，见 `mcode acp --help`）。
  - `installation.environment`：`MINIMAX_DATA_DIR=/state/mcode`、`NODE_ENV=production`、`NO_UPDATE_NOTIFIER=1`、`CI=1`、`MAVIS_BUILD_ENV=production`（禁用内部 lane 旁路）。
  - `installation.files`：权限后端资产（见 2.4）。
- **容器命令**：cwd 固定 `/work`。ACP 规范规定 `cwd` 与 agent 进程的 cwd 无关，但恢复时请求里的 `cwd` 必须与创建会话时一致 → DevDesk 每次打开都传同一个 `/work`。
- **指令注入**：一期用 `mcode acp --append-system-prompt-file /run/devdesk/profile`；若 P0 确认 mcode 宣告 ACP `sessionCapabilities.systemPrompt`，可改用 `session/new.systemPrompt{mode:"append"}`（标准路径，便于与 opencode 共用适配器）。P0 必须验证注入内容确实进入模型上下文。
- **镜像配方**：用共享地基 2.5.6 的 npm 安装路径表达可选依赖和受限脚本参数，校验字段并计入 `recipe_digest`。npm 安装能力可独立落地。
  - 基础镜像 Node 24.13.0 自带 npm 11.6.2，该版缺少 `--allow-scripts`。在 mcode 的构建路径固定一个经验证支持该选项和严格模式的 npm 版本，记录其版本与安装来源。参考 [Node 随附 npm 版本](https://raw.githubusercontent.com/nodejs/node/v24.13.0/deps/npm/package.json)、[npm 11.16.0 发布记录](https://github.com/npm/cli/releases/tag/v11.16.0)和 [npm install 选项](https://docs.npmjs.com/cli/v11/commands/npm-install/#allow-scripts)。
  - P0 验证 `--include=optional`、`--allow-scripts=@minimax-ai/code,better-sqlite3` 和 `--strict-allow-scripts`，用构建输出确认所需脚本执行，并验证未授权脚本被拒。`--ignore-scripts` 会覆盖白名单并跳过所需 `postinstall`。
  - 基础镜像 glibc 必须 ≥ 2.29（Debian bookworm 为 2.36 ✓）。
  - **P0 必须实测**：npm tarball 内 `native/` 无 linux 预编译（仅 darwin/win32），需确认 Linux 容器内 mcode 是否仍可启动（可能依赖 WASM 回退 `emscripten-module.wasm`/`quickjs-worker.js`）。若不可启动，方案二：改用「容器内 `npm install` + 运行时从 `native/` 补 linux 预编译」或退回「以官方 Linux 发行物为准」，**该结论决定本计划是否成立**。
- **会话身份**：ACP `session/new` 的 `sessionId` 存为 `external_session`。恢复按共享地基 2.2.7 协商 `resume` 或 `loadSession`，复核冻结模型、指令与 MCP 服务器列表。
- **模型与 provider**：Host 写 `MINIMAX_DATA_DIR/config.yaml`，用 `defaultModel: custom_provider:<provider>/<model>` 选择 `custom_provider` 中的模型。固定版本已实测 `api: openai-completions` 及 `options.baseURL/apiKey`。

### 2.2 协议与事件映射（ACP → `Obs`）

| ACP 事件/应答 | 观察 |
|---|---|
| `initialize` 应答 | 协议握手完成，记录协商的 `protocolVersion` 与 `agentCapabilities`；客户端能力声明 `{fs:{readTextFile:false,writeTextFile:false},terminal:false,auth:{terminal:false}}`。**逐项核对**：`mcpCapabilities.http` 为假则步骤工具不可用（给出明确原因）；`sessionCapabilities.resume` 决定恢复方式 |
| `session/new` 应答 `sessionId` | 会话外部 id 建立（绑定为已接纳会话） |
| `session/resume`（或 `session/load`）应答 | 恢复完成；恢复后复核模型与 MCP 列表 |
| 该轮次**首个 provider 证据**到达时（宿主侧） | 已接纳轮次（`Admitted`，`external_turn` = 派发时生成并存入 Activity 的轮次 id，接纳时写入 turn 记录）——**不是**等应答帧，也不是提交即接纳 |
| `session/update` / `agent_message_chunk` | Assistant `Delta`（复用 Codex 的合批与尾部脱敏） |
| `session/update` / `agent_thought_chunk` | 丢弃（与 Codex/pi 一致，不展示 reasoning） |
| `session/update` / `tool_call`、`tool_call_update` | Tool `Full`（正文为事件 JSON，含 `status` pending/in_progress/completed/failed） |
| `session/update` / `plan_update`、`plan_removed` | 一期忽略（可选映射为结构化观察，不入账） |
| `session/update` / `usage_update`、`current_mode_update`、`session_info_update`、`available_commands_update`、`config_option_update` | 元数据，不产生观察 |
| `session/prompt` 应答 `stopReason=end_turn` | `Terminal` = Completed |
| `stopReason=max_tokens` | **Failed**（原因 `max_tokens`；截断的交付物不能当作完整结果，与 pi、DSH 口径一致，见共享地基第 6 章） |
| `stopReason=max_turn_requests` | Failed |
| `stopReason=refusal` | Failed |
| `stopReason=cancelled` | Cancelled |
| 服务端请求 `session/request_permission` | `Obs::Interaction{ToolApproval}`（选项 `allow_once`/`allow_always`/`reject_once`/`reject_always`） |
| 服务端请求 `elicitation/create` | 一期**不产生** `Obs::Interaction`（见下方「交互的处理边界」）；仅在协议日志留痕 |
| `session/cancel`（宿主→服务端） | 取消当前轮次（停止模式，参考 DSH/Codex 现有 stop 分支） |

**接纳语义（与 Codex/DSH 不同，必须显式实现）**：既有骨架把「对宿主请求的应答帧」当作接纳凭证（`harness.rs:1033-1058`：`id == a.request` 且无 `method` → `Obs::Admitted`），Codex `turn/start` 与 DSH `session/prompt` 都是**立即**回执。ACP 的 `session/prompt` 只在**轮次结束**才回 `{stopReason}`，沿用该骨架会导致：所有 `session/update` 通知在接纳前被塞进 `a.buffered`（上限 256，`harness.rs:1141-1144` 触发「原生输入回执前事件过多」硬失败）→ 长轮次必然失败；流式输出退化为轮次结束后一次性回放；接纳与终态落在同一帧，与终态边界逻辑冲突。
**实现口径**见共享地基 2.2.8。P0 记录能归属当前轮次的首个证据，包含早于首条 `session/update` 的审批或 MCP 工具请求。验证先接纳再处理、迟到请求及时拒绝，以及接纳后的错误保留已接纳事实。

**「消息已消费」标记**：ACP 没有「用户消息已消费」事件（Codex 有 `item/started userMessage` → `Obs::Consumed`）→ 需明确 `Obs::Consumed` 的产生时机。默认口径：与 `Admitted` 同时产生 `Obs::Consumed{message: None}`（即首个证据到达时）；P0.2 若发现 provider 会回显用户消息（`user_message_chunk`），则改为在回显到达时产生。

**交互的处理边界**：core 对任何 `Obs::Interaction` 都会创建 `PendingInteraction` 并把轮次置为 `WaitingInteraction`（`conversation_observations.rs:641-688`），必须经 `answer_interaction` 应答才能继续 → 「一期只记录不阻塞」在现有 core 下**不成立**。一期口径：`elicitation/create` 由适配器**直接应答**（取消/拒绝），**不产生** `Obs::Interaction`，只在协议日志留痕；`session/request_permission` 仍走完整审批流。

- **帧路径**：`sessionUpdate` 的取值位于 `params.update.sessionUpdate`（不是 `params.sessionUpdate`）；`tool_call`/`tool_call_update` 的内容在 `params.update` 内。
- `stopReason` 的取值：ACP 规范与 mcode 包内 schema 均为 snake_case（`end_turn`、`max_tokens`、`max_turn_requests`、`refusal`、`cancelled`；包内 `"end_turn"` 出现 12 处）。此前版本称「ACP 规范常见 camelCase」不成立 → 按 snake_case 实现，P0.2 实跑确认取值集合即可。
- ACP 无「消息终稿」事件（不同于 pi 的 `message_end`）：**正文权威值 = 累积的 `agent_message_chunk` 文本**。多步轮次里工具调用前后的文本属于不同消息，因此**以工具调用为边界分段封存**：每次 `tool_call` 之前封存当前段为 Assistant `Full`，`session/prompt` 应答到达时封存最后一段。不能把整轮拼成一条。空文本段不产出 `Full`，空轮次按空正文处理。
- 工具调用失败（`tool_call_update` 的 `status=failed`）只记为 Tool 事件，**不改变轮次终态**——agent 可以从失败的工具调用中恢复，与 Codex/DSH 一致；轮次终态只看 `stopReason`（共享地基第 6 章）。

### 2.3 工具桥与审批（一期打通）

1. **投放**：宿主在 `session/new`/`session/resume` 的 `mcpServers` 中注入（服务名取短名 `devdesk`，见共享地基 2.4.4）：
   ```json
   [{"type":"http","name":"devdesk",
     "url":"http://host.docker.internal:<bridge-port>/mcp",
     "headers":[{"name":"Authorization","value":"Bearer <worker-token>"}]}]
   ```
   动态工具（`dynamic_tools(binding)` 结果）由宿主 MCP 服务端以 `tools/list` 暴露，模型看到的是 MCP 工具。
2. **执行**：复用共享地基 2.4.8 的投递、core 接纳、按工具类型执行及回执链路。mcode 负责方言接线和宿主桥注入。
3. **harness 自身审批**：`session/request_permission` → 宿主 `Obs::Interaction{ToolApproval}` → 用户裁决 → 以同一 `id` 应答 `{"outcome":{"outcome":"selected","optionId":"<id>"}}`；取消时 `{"outcome":{"outcome":"cancelled"}}`。
   - 冻结审批策略为 `never` 时：不弹窗，自动选择拒绝类选项（`reject_once`），并记入事件流。
   - 允许时**只选 `kind == allow_once` 的选项，永不选 `allow_always`**；请求里没有单次允许选项时按拒绝处理（冻结权限不能被运行时长期放宽）。
4. **一致性要求（审批口径以共享地基 2.4.5 为准）**：两条通道落在同一份冻结权限上，但**审批只对 provider 自己发起的请求**（第 3 条）。**宿主桥调用不设适配器层审批**：准入完全由 core 按冻结权限判定——平台写操作在 `approval == never` 且文件系统非只读时直接执行，`on-request` 下 core 直接拒绝（`platform_observations.rs:361-365`），桥把拒绝原样返回。此前版本写的「桥调用经 `PendingInteraction` 往返、`never` 时 worker 直接拒绝」是 core 里不存在的流程，按字面实现会让默认 `never` Profile 下的步骤工具被全部拒绝，**不得实现**。
5. **工具投放**按共享地基 2.4.4，验收集成、上下文和平台工具各自的治理路径。未绑定工具在列表中隐藏。平台工具的授权要求写入 `limitations`，普通讨论中的显式调用返回 `tool_not_authorized`。
6. **网络边界（已更正）**：Harness 工作容器恒为联网容器（`harness.rs:648`），`--network none` 只用于测试与工具容器，因此不存在「禁网 Profile 下宿主桥不可达」的场景。真实的边界是：mcode 不支持 harness 层工具禁网（`disabled_tools`），校验阶段直接拒绝；**远程 Docker daemon**（`DOCKER_HOST` 非 unix socket）下容器到不了 DevDesk 宿主，存在需要宿主桥的动态工具时在打开阶段直接拒绝。两项写入 `limitations`（共享地基 2.4.6）。
7. HTTP MCP 路径被 P0 证伪时退回设计。可能的替代传输包括容器内 stdio MCP 代理和宿主 Unix socket，或 ACP 客户端方法承载受限工具集。替代路线须另行评审并重跑步骤工具与权限验收。
8. **桥的落点与唤醒**（共享地基 2.4.1、2.2.9）：adapters 的独立监听器将请求投递至当前 worker 的收件箱，并通知 `RuntimeWorkers.wakeups`。mcode 等待 MCP 结果期间，调度无需新的 ACP 帧。
9. **MCP 工具名**（共享地基 2.4.4）：暴露名满足 `^[A-Za-z0-9_-]{1,64}$`，原名含其他字符时确定性映射并在桥侧反查；P0 记录 mcode 给 MCP 工具加的服务器前缀与模型可见名长度，据此调整指令文本对工具名的引用。

### 2.4 权限与策略后端

1. **容器内冻结策略**：由 Host 写入私有 `$MINIMAX_DATA_DIR/hooks.json`，通过固定 `command` 加载 `permissions/backends/mcode/tool-policy.mjs` 对应资产。脚本读取 `/run/devdesk/permission`，在 `PreToolUse` 上对禁止的调用返回 `hookSpecificOutput.permissionDecision: "deny"`。初始 `SessionStart` 装载标记须绑定到本次运行。
2. **能力声明** `capabilities(Mcode)`：filesystem 三档；network 仅 `enabled`（无工具级禁网执行器）；tools `all_available` + `restricted`；`limitations` 如实列出：容器内 Hook 非原生权限模式、审批为逐次询问、cwd 固定 `/work`、**不支持 harness 层工具禁网（`disabled_tools`）**、**不支持 Agent 提问（`elicitation/create` 被自动取消）**、**远程 Docker daemon 下步骤工具不可用**、**不支持进程内注册宿主工具（步骤工具走宿主 MCP）**、**平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用**。
3. **校验分支** `validate_execution_permissions` 的 mcode 分支：拒绝 `Disabled` 网络；scoped 规则沿用 DSH 的「必须由冻结挂载落实」校验（抽成共用函数）。
4. **执行合同命名**：按实际执行者命名，后端 `mcode-hooks-v1`，`harness_backend`/`rules` 不使用 `native` 字样。
5. **资产登记**：`permissions/backends/manifest.json` 新增后端与资产 sha256；`schemas/permission-backend.schema.json` 同步。
6. **仓库内可执行配置隔离（共享地基 2.1.8）**：mcode 的数据目录在 `/state/mcode`，不在 `/work` 内，`.minimax` 无需加入 `protected_metadata`。需要处理的是 mcode **从项目目录读取**的可执行配置（项目级 `hooks/` 等）：P0.4 记录其发现路径；能用开关关闭就关闭，关不掉的路径加入 `protected_metadata`（目录型条目需要新增挂载能力）。三处白名单必须同步：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`（**硬编码白名单**，只改前两处会在容器创建时被第三处拒绝）。

### 2.5 模型与凭据

1. 新建 provider 源：`mcode-minimax-cn`、`mcode-zai-coding-cn`、`mcode-deepseek`、`mcode-ollama`；新建 Model 源与 `builtin-model-catalog-mcode-v1.json`（如 GLM 5.3、MiniMax M3 系列）。
2. 端点：MiniMax 需覆盖 `baseUrl` 为 `api.minimax.cn`（官方默认 `api.minimaxi.com`）；智谱编程订阅端点与 dsh 目录一致。
3. 凭据：按共享地基 2.3 泛化后的键 `credentials.mcode.{provider}`；容器内以 provider 配置 + 环境变量下发，容器创建后删除临时文件（沿用 `harness.rs` 现有 `credential_env_file` 生命周期）。
4. provider 公开 id 全局唯一校验（`validate_model_provider`）不得与 dsh/pi/其他新 provider 的同名 provider 冲突。
5. 凭据按 `(Harness, 服务商)` 隔离：用户需要为 mcode 单独保存同厂商的 API key（共享地基 2.3.4），设置页如实提示。

## 3. 工作项

### P0 可行性验证（G0 并行）

G0 在 Linux arm64 上实测项目 MCP 默认可启动。只读覆盖 `.minimax` 和 `.mcp.json` 后，项目 MCP 与项目 Hook 的探针均未运行，私有 Hook、用户指令和宿主 MCP 往返继续生效。正式 worker 采用项目配置保护挂载和独立的轮次桥。`AGENTS.md` 仍作为不可信上下文进入模型。[批次 B 产品验收](../harness-shared-foundation/EXECUTION.md#批次-b-产品验收)

1. 手工构建固定版本镜像。验证 2.1 的 npm 脚本白名单实际生效，验证 `better-sqlite3` 在目标 Linux 架构可加载。记录 `mcode --version`，用独立零凭据、断网探针验证 `mcode acp` 启动及最小握手。
2. 用真实 CLI 和获授权模型或受控模型夹具验证 `initialize` → `session/new` → `session/prompt` → 观察和权威终态。确认 ACP 协商、流式、审批应答形状及取消。
   - **必须同时确认「应答时机」**：`session/prompt` 的应答是**轮次结束**才到（只有 `stopReason`）还是**立即**回执；并观察 `session/update` 是否在应答之前到达（决定 §2.2 的宿主合成接纳口径与流式实现是否成立）。
   - 确认 `sessionUpdate` 取值的实际路径（`params.update.sessionUpdate`）与 `stopReason` 的实际大小写。
   - 确认 provider 是否回显用户消息（决定 `Obs::Consumed` 的产生时机）。
   - 覆盖审批或 MCP 调用早于首条通知、上一轮迟到调用，以及接纳前后的错误应答（共享地基 2.2.8）。
3. 确认 ACP 宣告 HTTP MCP 能力，`session/new.mcpServers` 可经 HTTP 和 headers 连上 G0 共用探针。验证 macOS/Linux 两种宿主的步骤工具往返，记录模型可见名、超时、请求身份与重试行为。
4. 确认 `hooks/hooks.json` 的 `PreToolUse` 模块契约：导出签名、入参（工具名/参数）、**能否阻断工具调用**（只读或拒绝）；**并确认 mcode 运行时从哪里发现钩子**（项目级目录？`MINIMAX_DATA_DIR` 下？全局配置目录？）——「发现路径」不定，冻结策略就会**静默不生效**。现有 `docker.rs` 只有 `/run/devdesk/permission` 与 `/run/devdesk/tool-policy.mjs` 两个固定挂载目标，目录型目标（如 `/run/devdesk/mcode-hooks/`）需要新增挂载能力与目标白名单（参照 `profile_target` 的 `require([...].contains(...))` 校验）。
5. 确认模型/provider 的权威配置位置（`config.yaml` 字段 vs 启动参数）与 API key 自定义 provider 的最小配置；恢复路径上用什么手段复核冻结模型（ACP 没有 pi 的 `get_state`，需找等价手段）。
6. 记录 `sessionCapabilities.resume` 与 `agentCapabilities.loadSession`，验证支持的恢复路径。使用 `session/load` 时记录重放规模与耗时。
7. 策略钩子的**装载信号**：有无办法在会话启动时确认钩子已装载（共享地基 2.1.7）；没有会话级信号时，确认「配置独占 + 镜像探针」的最低证明路径。
8. 启动期外联与仓库内配置：逐项确认 mcode 启动时的网络请求（更新检查、遥测、模型目录）并关闭；确认是否读取项目目录内的可执行配置。
9. 按共享地基第 7 章记录「通过」「退回设计」「延期」。钩子装载证明或阻断被证伪时退回设计。

### P1 注册、配置、权限、凭据与界面入口

结果：内部制品可注册 mcode，`runtime.prepare` 能构建镜像并通过协议探针。公开入口随批次 B 发布。

1. 共享地基 2.1 的 provider 注册与显式分支（含 mcode）。
2. `crates/adapters/src/jsonl.rs`：ACP 线协议模式 + 入站请求应答（共享地基 2.2）。
3. `crates/adapters/src/permissions.rs`、`permission_profiles.rs`：mcode 能力、编译分支、校验分支、`asset_digests()`、语义摘要。
4. 权限资产：`permissions/backends/mcode/`（hooks + 策略）+ `manifest.json` + `permission-backend.schema.json`。manifest 条目需同时写 `id`（`mcode-hooks-v1`）、`provider`（`mcode`）、`version`（mcode 版本）、`assets`（sha256 登记）——schema 必填 `id`/`provider`/`assets`，`provider` 枚举当前只有 `docker|codex|dsh`，需扩。
5. `crates/adapters/src/docker.rs`：接入 mcode 目录型冻结策略挂载和目标白名单，路径以 P0 发现结果为准。宿主桥开关复用共享地基 2.4.6。
6. `crates/adapters/src/model_providers.rs`：接入 `mcode_model_patch` 生成 provider 配置和计算 `model_profile_digest`，复用批次 A 的分派机制。
7. `crates/adapters/src/runtime_verification.rs`：mcode 探针（`initialize` + `session/new`，无密钥无网络）；`runtime_registry.rs`、`runtime_preparation.rs`（`default_mcode_recipe_digest`）、`session_resources.rs`（独立准备锁、凭据分支）。
8. `crates/adapters/src/harness.rs`：在内部制品接入 `mcode-acp/v1` 和 `RoutingHarness`，协议驱动在 P2 通过 `harness_mcode.rs` 接线。
9. 出厂配置：`plugins/core/harnesses/builtin-harness-mcode-managed-v1.json`、服务商/Model 源、目录、`plugins/core/manifest.json`、`configuration_usage.py --write`、`tests/fixtures/configuration/engineering-catalog.json`。路径、命令与夹具按共享地基第 8 章。
10. 凭据泛化的增量部分（共享地基 2.3：mcode 的账户前缀与测试表）；全局库 v25 与升级命令已在批次 A 落地，**此处不改全局库版本**，只核对 CHECK 已含 `mcode`。
11. Schema 枚举更新。
12. 前端（共享地基 2.6.1）。
13. 采用共享地基 2.5.6 的 npm 安装能力，按 P0 验证结果填入固定 npm 版本及允许参数，并验证 `recipe_digest`。

### P2 协议驱动

结果：mcode 会话端到端可用——讨论轮次、恢复、停止。

1. 新增 `harness_mcode.rs` 提供启动参数、环境变量、模型配置、策略钩子和 `AcpDialect`，将会话驱动委托给共享地基的 `harness_acp.rs`。`managed_adapter_identity` 纳入新模块和策略资产。若 G0 选定 mcode 先接，由本线提供共用模块的第一条真实验收链路。
2. 复用 `provider_turn_text`、`prompt_snapshot`、`write_private`。
3. `crates/adapters/src/model_providers.rs`：mcode provider 配置写入 + 凭据下发。
4. `permissions/backends/mcode/tool-policy.mjs`：`PreToolUse` 策略拦截。
5. 测试：`harness.rs::protocol_tests` 的 mcode 用例（接纳、消费、增量与终稿封存、工具事件、五种 `stopReason`、`session/request_permission` 往返）配合新增脱敏样本 `tests/fixtures/protocols/mcode-acp-turn.json`；`jsonl.rs` 线协议与结构化脱敏（桥令牌不得出现在日志）；`permissions.rs`；`model_providers.rs`。

### P3 工具桥与审批

结果：workflow 步骤工具一期打通。

1. `harness_acp_setup.rs` 提供 `session/new` 与恢复所需的 `mcpServers`。Host MCP 监听、worker 收件箱、唤醒、回执与撤销复用共享地基 2.4。
2. 接入 mcode 的审批选项映射，按 2.3 回答 provider 审批。桥调用沿 core 的工具准入链路。
3. 按共享地基 2.4.9 在真实镜像验收九条桥判据，同步检查早到调用、写调用超时核对及策略阻断。

### P4 验收与文档

1. `tests/entrypoints/native_acceptance.py` 增加 mcode 套件；`tests/live/probe_*.py` 的 provider 选项；浏览器用例里枚举 Harness 选项的 spec。
2. 文档：`docs/design/agent-runtime.md`、`docs/design/reference/integrations.md`、`docs/crates/devdesk-adapters/{harness-and-protocols,runtime-preparation,credentials-and-permissions}.md`、`permissions/README.md`、`sql/README.md`、`AGENT.md`（global 版本号）、`docs/design/run-binary.md`、`tests/README.md`；`scripts/check/check-docs.py --write` 刷新文件索引。
3. 本线通过讨论、步骤工具和权限验收后，按共享地基第 3 章在批次 A 的 v25 副本演练发布。公开入口、出厂源和权限资产随批次 B 集中发布。

## 4. 验证

```bash
./scripts/check/check-style.sh
cargo test --workspace --locked
./scripts/check/check-permissions.sh
python3 scripts/check/check-defaults.py
python3 scripts/check/check-docs.py
python3 tests/entrypoints/dataset_protocol.py          # 覆盖 v24→v25 升级
python3 tests/entrypoints/native_acceptance.py --docker-only --prepare
./scripts/check/check.sh
```

真实运行：`./scripts/dev/dev.sh --data ./data-mcode --port 4318`（新空数据目录）。

1. 设置里准备 mcode 镜像，确认协议探针通过；为 mcode 保存智谱或 MiniMax 的 API key。
2. 新建 Agent，选 mcode 与目标模型，开讨论发消息：确认流式输出、最终正文与终态（`end_turn` → Completed）。
3. 让 Agent 读文件、改文件，确认工具事件入账；只读 Profile 下确认写入被拦截且原因明确。
4. 跑一个带步骤工具的 workflow 步骤，确认宿主 MCP 桥调用往返、宿主侧产生回执，**且该工具调用出现在会话记录与治理记录中（可审计）**；同一 `call_id` 重放不产生第二次执行；取消运行中的轮次，确认停止模式与容器移除。
5. 重启 Host 后继续该会话，确认 `sessionId` 与模型被正确恢复（`session/resume`）。
6. 审批策略 `on-request` 下触发一次 provider 自己发起的工具审批，确认用户裁决可回传并被 harness 接受，且「永久允许」类选项从未被选中；`never` 下 provider 审批被自动拒绝、不弹窗。

**验收判据**：上列 6 项全部有实测记录（含命令、制品版本、实际结果、未验证范围），且第 4 项同时满足共享地基 2.4.9 的 9 条桥判据——只验「宿主侧产生回执」不成立。

## 5. 不在本期范围

- 根编排（mcode 不获得 `orchestration`，与 DSH 一致）。
- Agent 提问：ACP 有 `elicitation/create` 通道，一期由适配器直接应答取消，并写入 `limitations`，不进验收。
- 容器内工具禁网；mcode 的原生 `grep`/`find` 工具（依赖 ripgrep/fd，与 pi 计划同样会改动共享沙箱配方）。
- 思考等级参数。

## 6. 风险与兼容

| 风险 | 影响 | 应对 |
|---|---|---|
| npm tarball 无 linux 原生预编译 | 容器内 mcode 可能无法启动 → 本计划不成立 | P0.1 必测；备选：补 linux 预编译 / 改用官方 Linux 发行物 / 换基础镜像 |
| `better-sqlite3` 需要 glibc ≥ 2.29 且安装脚本不可跳过 | 镜像构建或启动失败 | 固定 Debian bookworm 基础镜像；构建命令固定 `--allow-scripts`、`--include=optional` |
| ACP 客户端能力未被 agent 使用（实测未见 `fs/*`/`terminal/*`） | 无法用 ACP 客户端方法承载宿主工具 | 一期改走宿主 MCP 桥（2.3），P0.3/P0.4 验证 |
| `PreToolUse` Hook 契约未知（能否阻断） | 冻结策略无法在容器内落实 | P0.4 实测阻断，按共享地基 2.1.7 证明装载。任一失败退回设计 |
| 宿主 MCP 桥是新增网络面 | 令牌泄漏 / 宿主暴露面扩大 | 独立监听器 + 每会话令牌 + 撤销 + 结构化日志脱敏 + 只暴露冻结工具集 |
| 桥调用入队后 worker 未被调度 | provider 等待 MCP 结果时调用滞留 | 入队即唤醒，按共享地基 2.4.9 验证早到调用与调度 |
| `session/load` 重放整段历史 | 长会话恢复撞上初始化缓冲上限 | 按共享地基 2.2.7 协商恢复并有界丢弃重放 |
| 桥等待超时或轮次撤销 | 已接纳写调用的效果可能仍待核对 | 按共享地基 2.4.8 保留原调用身份与意图，核对结果 |
| 容器→宿主连通性在 macOS/Linux 行为不同 | 桥不可达 → 步骤工具不通 | P0.3 双形态实测；必要时提供 Linux-only socket 桥作为备选 |
| ACP 协议版本随 mcode 升级变化 | 会话恢复兼容性破坏 | 固定 `@minimax-ai/code` 版本；`adapter_version` 记 `mcode-acp/v1`；恢复时复核协议版本 |
