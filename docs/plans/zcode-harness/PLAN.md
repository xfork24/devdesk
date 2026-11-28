# zcode Harness 接入（ZCode CLI）

日期：2026-11-25。状态：**P0 延期，待补模型、策略和再分发**。范围：接入 ZCode CLI（`zcode-app-cli`，MIT 启动器 + 随包专有运行时）为新增 Harness，能力对齐 DSH：讨论会话、workflow 步骤工具、模型服务商。不含根编排。

前置依赖为 [Harness 扩展共享地基](../harness-shared-foundation/PLAN.md) 的通用管线和宿主 MCP 桥。G0 依共享地基第 7 章并行验证，协议、权限与再分发条件通过后，可以在 MCP 桥具备测试闭环时并行接入。架构独立性改造并行推进，zcode 按当前基线实施，后期归并按共享地基第 8 章执行。

> **入批门槛**：私有 `app-server` 协议须按第 3 章完成实测。维护者须在 G0 收口前确认运行时及随包第三方内容可放入 DevDesk 发布镜像，以及须携带的许可证和声明。协议或再分发路线被证伪时退回设计，证据不足时延期。第 5 章的备选接口另行评审。

## 0. 已确认决策

| 决策 | 结论 |
|---|---|
| 驱动协议 | **ZCode Protocol**（`zcode app-server`，常驻 stdio）；`zcode -p --output-format stream-json` 仅作备选路线 |
| 工具桥 | 一期打通：宿主侧 MCP 服务端承载 workflow 步骤工具，zcode 通过 `setting.json` 的 `mcp.servers` 接入；执行、回执与冻结权限核验全部在 Host 侧 |
| 凭据所有权 | `ZCODE_APP_SERVER_AUTH_MODE=host`——宿主负责账号更新与请求凭据刷新，服务端不读独立凭据文件 |
| 权限模式 | ZCode 默认 `--prompt` 下为 `yolo`，**必须显式覆盖**；审批映射到冻结审批策略 |
| 根编排 | zcode 不获得 `orchestration` 能力（与 DSH 一致） |
| Agent 提问 | ZCode 有 `interaction/requestUserInput` 通道，但**不进一期验收** |
| 许可声明 | 依据 G0 确认的再分发范围填写实际许可组合，镜像携带适用的 `LICENSES/`、第三方声明及许可证所需文件 |
| 全局库 | 版本 24 → 25 在批次 A（pi）一次性完成，CHECK 届时已含 `zcode`；**本计划不改全局库版本**（P0 未通过时该取值闲置，无害）。口径以共享地基第 3 章为准 |

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
  - `data.provider: "zcode"`、`runtime_version: "3.18.2-48"`（npm 发行版本）、`installation.package: "zcode-app-cli"`。**更正**：`compose_execution_recipe` 用 `{package}@{runtime_version}` 拼安装行（`execution_recipe.rs:219-222`），此前「`runtime_version: "0.17.1"` + `package: "zcode-app-cli@3.18.2-48"`」的写法会生成 `zcode-app-cli@3.18.2-48@0.17.1`，镜像构建必然失败。运行时自身版本 `0.17.1` 记录在本计划与验收记录中，不进入 Harness 源的 `runtime_version`。
  - 按 G0 的再分发结论填写 `license` 和 `installation.files`。核对启动器 MIT、运行时 Apache-2.0 及 ZCode 声明，确认 `LICENSES/`、`docs/THIRD_PARTY_CONTENT.md` 和受额外条款约束的内容如何随镜像分发。
  - `installation.environment`：`ZCODE_APP_SERVER_AUTH_MODE=host`、`ZCODE_CLI_SETTINGS_FILE=/state/zcode/setting.json`、`ZCODE_PERSONAL_PROVIDER_CONFIG_FILE=/state/zcode/provider_config.json`、`ZCODE_SESSION_DB_PATH=/state/zcode/db.sqlite`、`ZCODE_CLI_CREDENTIALS_FILE=/state/zcode/credentials.json`（host 模式下不使用独立凭据文件）、`ZCODE_DISABLE_UPDATE_CHECK=1`、`NO_UPDATE_NOTIFIER=1`。
  - `installation.entrypoint: ["zcode", "app-server"]`（不得带 `--browser-use`，启动器本就不向 app-server 注入 Browser Use）。
- **容器命令**：cwd 固定 `/work`；stdout/stdin 全部是协议流，stderr 只作诊断。
- **指令注入通道（P0 必测）**：候选路径为 `ZCODE_CLI_SETTINGS_FILE` 中的系统提示追加项或 `session/create` 参数。至少验证一条路径能把 DevDesk 的角色指令、快照和合同送入模型上下文。
- **镜像配方**：基础镜像 Node 24.13（≥22.19 满足）；`npm install -g zcode-app-cli@3.18.2-48 --omit=dev`（该包装无原生 PTY 插件、无 postinstall 构建步骤）。
  - 体积：包约 155 MB（vendor 137 MB），镜像需留足空间。
- **会话身份**：`session/create` 返回的 session id → DevDesk `external_session`；恢复走 `session/resume`/`session/lifecycle/ensureReady`。

### 2.2 协议与事件映射（ZCode Protocol → `Obs`）

| ZCode Protocol | 观察 |
|---|---|
| `session/create {cwd, workspace...}` 应答 | 会话外部 id 建立 |
| `session/subscribe` | 订阅事件流（后续 `session/event` 通知） |
| 该轮次**首个 provider 证据**到达时（宿主侧） | 已接纳轮次（`Admitted`，轮次 id 由 Host 在派发时生成）。若 P0 证实 `session/send` 立即回执，则改按既有骨架以应答帧为接纳凭证 |
| `session/event` 通知中的增量文本 | Assistant `Delta`（合批与尾部脱敏复用 Codex 逻辑） |
| `session/event` 中的工具事件 | Tool `Full` |
| `session/stop`、`session/cancel` | 停止当前轮次（宿主→服务端） |
| `session/setMode`、`session/setModel`、`session/setThoughtLevel` | 会话内模型/模式调整（冻结绑定优先） |
| `interaction/requestPermission` | `Obs::Interaction{ToolApproval}` |
| `interaction/requestUserInput` | 一期**不产生** `Obs::Interaction`（见下方「交互的处理边界」）；仅在协议日志留痕 |
| `interaction/requestProviderRuntimeHeaders` | **宿主凭据下发点**（host auth 模式）：按冻结连接返回请求头（值来自 Provider 绑定；该方法的应答登记为**私有请求**、不落日志，并有 secrets 脱敏兜底，共享地基 2.2.10） |
| `interaction/respond {type:"respond_permission", permissionRequestId, optionId, response}` | 宿主对权限请求的应答 |
| 轮次终态事件 | `Terminal`（Completed / Failed / Cancelled）——**终态事件名 P0 必须实测确认**（ZCode Protocol 未文档化，不能凭猜测实现） |

- **协议形状注意**：ZCode Protocol 的帧**不带** `jsonrpc` 字段（与 ACP 相反）→ `jsonl.rs` 需要第三种线模式；错误在 `{id, error:{message, code, data}}`。
- **接纳语义待实测**：P0 确认 `session/send` 的应答时机。若是立即回执，验证回执是否可作接纳凭证。若是轮次末应答，按共享地基 2.2.8 用首个可归属的轮次证据接纳，验证早到审批或 MCP 请求、迟到事件和接纳前后的错误。会话级凭据请求不能单独证明轮次已接纳。
- **交互的处理边界**：core 对任何 `Obs::Interaction` 都会创建 `PendingInteraction` 并把轮次置为 `WaitingInteraction`（`conversation_observations.rs:641-688`），必须经 `answer_interaction` 应答才能继续 → 「一期只记录」在现有 core 下**不成立**。一期口径：`interaction/requestUserInput` 由适配器**直接应答**（取消/拒绝），**不产生** `Obs::Interaction`，只在协议日志留痕；`interaction/requestPermission` 仍走完整审批流。
- **事件序号**：`session/event` 带 `seq`/`eventId`，可作为单调性核对依据（乱序或缺号需记录为协议异常，不得静默丢弃）。
- **失败判定**：终态事件缺失但连接断开 → `needs_reconciliation`（不得当作成功）。
- **统一语义口径**（共享地基第 6 章）：输出截断判 Failed；工具调用失败只记 Tool 事件、不改终态；助手文本以工具调用为边界分段封存。

### 2.3 工具桥与审批（一期打通）

1. **投放**：宿主写 `/state/zcode/setting.json` 的 `mcp.servers`：
   ```json
   {"mcp":{"servers":{"devdesk":{"type":"http",
     "url":"http://host.docker.internal:<bridge-port>/internal/tool-bridge/<session>/mcp",
     "headers":{"Authorization":"Bearer <session-token>"}}}}}
   ```
   动态工具由宿主 MCP 服务端暴露（与 mcode/opencode 共用共享地基 2.4 实现）。含令牌的 `setting.json` 位于宿主侧的会话状态目录，会话结束后会残留 → 清理时覆盖或删除，并由令牌撤销兜底（共享地基 2.4.2）。
2. **执行**：复用共享地基 2.4.8 的 MCP 投递、core 按工具种类接纳、治理与回执。zcode 负责配置注入与 ZCode Protocol 接线。
3. **审批**：ZCode 自身权限模式（`build`/`edit`/`plan`/`yolo`/`guarded`）→ 冻结审批策略映射：`never` → 选择最严格且不弹窗的模式 + 宿主侧拒绝对应升级；`on-request` → 走 `interaction/requestPermission` → 用户裁决 → `interaction/respond`。
   - ZCode 的 `--mode` 默认在 `--prompt` 下是 `yolo`，**必须显式覆盖**，不能依赖默认值。
4. 桥与权限钩子使用同一份冻结权限。桥调用按共享地基 2.4.4、2.4.8 验证回执、治理记录、脱敏和撤销。
   - **宿主桥调用不设适配器层审批**（共享地基 2.4.5）：准入由 core 按冻结权限判定，`on-request` 下平台写操作被 core 直接拒绝，桥把原因原样返回；适配器不得为桥调用发明审批流。
   - **工具投放**按共享地基 2.4.4，隐藏未绑定工具，分别验收集成、上下文与平台工具。平台工具的授权要求写入 `limitations`，普通讨论中的显式调用返回 `tool_not_authorized`。
   - **网络边界（已更正）**：Harness 工作容器恒为联网容器，不存在「禁网 Profile 下宿主桥不可达」的场景。真实的边界是：不支持 harness 层工具禁网；**远程 Docker daemon 下步骤工具不可用**。两项写入 `limitations`（共享地基 2.4.6）。
   - **唤醒**：桥调用入队即 `RuntimeWorkers.wakeups.notify()`——zcode 等待 MCP 结果期间不产生协议帧，不唤醒则调用滞留到超时（共享地基 2.2.9）。
   - 注意 ZCode 的 `mcp.servers` 用的是 `headers` **对象**形式（`{"Authorization":"Bearer …"}`），与 ACP 的**数组**形式（`[{name,value}]`）不同 → 结构化脱敏要覆盖两种形态（共享地基 2.2.5）。

### 2.4 权限与策略后端

1. `permissions/backends/zcode/hooks/hooks.json` + `tool-policy.mjs`：读 `/run/devdesk/permission`，在 `PreToolUse`/`PermissionRequest` 上落实冻结策略（契约 **P0 实测**；ZCode 声明 Hook 可改写工具输入并参与权限决策）。装载证明见共享地基 2.1.7：没有会话级信号时，至少做到配置独占并在镜像探针里验证一次「策略生效」。
2. `capabilities(Zcode)`：filesystem 三档；network 仅 `enabled`；tools `all_available` + `restricted`；`limitations` 如实列出：**无进程内宿主工具注册通道（步骤工具走宿主 MCP）**、审批为逐次询问、cwd 固定 `/work`、默认权限模式需显式覆盖、**不支持 harness 层工具禁网**、**不支持 Agent 提问（`interaction/requestUserInput` 被自动取消）**、**远程 Docker daemon 下步骤工具不可用**、**ZCode Protocol 无公开契约、行为随版本绑定**、**平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用**。
3. 校验分支：`validate_execution_permissions` 的 zcode 分支（拒绝 `Disabled` 网络；scoped 规则必须由冻结挂载落实）。
4. 执行合同命名：后端 `zcode-hooks-v1`，不使用 `native` 字样。
5. 资产登记：manifest 条目需写全 `id`（`zcode-hooks-v1`）、`provider`（`zcode`）、`version`、`assets`（sha256）。**仓库内可执行配置隔离**（共享地基 2.1.8）：数据目录在 `/state/zcode`，`.zcode` 无需因为「写入」而屏蔽；需要处理的是 zcode **从项目目录读取**的可执行配置（项目级 hooks 等）——P0.6 确认发现路径，关不掉的路径加入 `protected_metadata`，三处白名单同步：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`（**硬编码白名单**，只改前两处会在容器创建时被第三处拒绝）。

### 2.5 模型与凭据

1. 新建 provider 源：`zcode-minimax-cn`、`zcode-zai-coding-cn`、`zcode-deepseek`、`zcode-ollama`（与 mcode 同样保持公开 id 全局唯一）；新建 Model 源与 `builtin-model-catalog-zcode-v1.json`。
2. 宿主写 `/state/zcode/provider_config.json`（`ZCODE_PERSONAL_PROVIDER_CONFIG_FILE`）：自定义 provider（`base_url` + API key + 模型规则）。字段语义按包内 `docs/PROVIDER_CONFIG.md` 写（P1 读取该文档定稿）。
3. 凭据：按共享地基 2.3 泛化后的键 `credentials.zcode.{provider}`；host auth 模式下由宿主在 `interaction/requestProviderRuntimeHeaders` 时下发，容器内不留长期凭据文件。
4. 凭据按 `(Harness, 服务商)` 隔离：用户需要为 zcode 单独保存同厂商的 API key（共享地基 2.3.4）。

## 3. 工作项

### P0 可行性验证（G0 并行，时间盒 1 个工作日）

本轮 G0 结论为 **延期（Linux 断网建会话与订阅已跑通，宿主模式的自定义模型未进入 Registry，策略、macOS 和再分发待验）**。见 [共享实施记录](../harness-shared-foundation/EXECUTION.md)。

1. 手工构建锁定版本的 zcode 镜像，记录 `zcode --version`、`zcode doctor`、运行时与制品摘要。独立使用零凭据、断网探针验证 `app-server` 的最小协议往返。
2. 用真实 CLI 和获授权模型或受控模型夹具跑 `session/create` → `session/subscribe` → `session/send` → 收到流式 `session/event` 和权威终态，并验证 `session/stop`。
   - 确认 `session/send` 的应答时机和终态事件名。覆盖首条通知前的审批或 MCP 请求、上一轮的迟到事件，以及接纳前后的错误。
3. 确认凭据模式：`ZCODE_APP_SERVER_AUTH_MODE=host` 下 `interaction/requestProviderRuntimeHeaders` 的请求/应答形状，以及自定义 provider（API key）是否被接受。
4. 确认权限通道：`interaction/requestPermission` 的选项结构与 `interaction/respond{respond_permission}` 的应答形状；`--mode` 各档对工具调用的实际影响。
5. 用 `setting.json` 的 HTTP MCP 配置连接 G0 共用探针，在 macOS/Linux 验证步骤工具往返。记录模型可见名、超时、请求身份与重试行为。
6. 确认 Hook 契约：`PreToolUse` 能否阻断工具调用；**并确认 zcode 运行时从哪里发现钩子**（`ZCODE_CLI_SETTINGS_FILE` 指向的设置目录？项目级目录？）——发现路径不定，冻结策略会**静默不生效**。
7. **协议硬性判定**：最小闭环连续 3 次取得权威终态，分类正确。宿主凭据往返和审批形状有可复现证据。冻结策略装载、阻断和仓库配置隔离须通过共享地基第 7 章。
8. 通用项（共享地基第 7 章）：启动期外联、指令注入和恢复后模型、指令与 MCP 配置的复核。
9. **再分发硬性判定**：维护者核对固定版本及实际镜像内容的再分发条件，记录许可证、声明及对受额外条款约束内容的处置，必要时由法务确认。
10. 按共享地基第 7 章记录「通过」「退回设计」「延期」。所有适用必验项和再分发条件确认后才进入 P1。

### P1–P4

工作项依次为内部制品注册和准备、协议驱动、工具桥接线、验收与发布。

- **P1**：在内部制品接入 ZCode 线协议与 `interaction/*` 应答，`requestProviderRuntimeHeaders` 登记为私有请求。接入策略资产、模型 patch、凭据、`default_zcode_recipe_digest`、出厂配置和设置页，新增零凭据断网探针。全局库沿用批次 A 的 v25。
- **P2**：`crates/adapters/src/harness_zcode.rs`（启动输入、事件投影、停止模式、凭据注入），`harness.rs` 委托分支；协议测试用例（接纳、消费、增量与终稿封存、工具事件、三种终态、权限往返）配合脱敏样本 `tests/fixtures/protocols/zcode-session-turn.json`。
- **P3**：宿主 MCP 桥（复用共享地基 2.4 实现，此处只做 zcode 侧注入与分账）；`setting.json` 写入；权限应答。
  - 复用共享地基 2.4.8 的收件箱、唤醒、按工具种类的接纳和回执路由。zcode 负责自身配置及协议投影。
  - zcode 特有的确认点：`interaction/respond` 的应答形状与桥侧回执形状互不影响（桥走 MCP，provider 审批走 ZCode 协议线），需分别测试。
  - 验收须覆盖共享地基 2.4.9 的 9 条桥判据。
- **P4**：增加 `native_acceptance.py` 的 zcode 套件、probe 选项、浏览器枚举和当前文档。讨论、步骤工具、权限和许可验收通过后，用批次 A 的 v25 副本演练发布。公开入口、出厂源与权限资产随批次 B 集中发布。

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
- Agent 提问：`interaction/requestUserInput` 一期由适配器直接应答取消，写入 `limitations`，不进验收。

**备选路线（P0 证伪后另行评审）**：`zcode -p '<task>' --output-format stream-json`（包内 `docs/HEADLESS.md` 要求退出码 0 且存在终态 `result` 记录）。该路线需要重新设计每轮进程与容器生命周期、`external_session`、停止和恢复语义。

## 6. 风险与兼容

| 风险 | 影响 | 应对 |
|---|---|---|
| ZCode Protocol 无公开契约 | 上游改版即失效；无法追责 | P0 门控；版本钉死 + `zcode-runtime.lock.json` 摘要入档；接受其性质为「版本绑定私有耦合」并在文档明写 |
| 第三方启动器随包带运行时及受额外条款约束的内容 | 影响发布镜像可再分发范围 | G0 确认实际许可，固定版本和摘要，按核对结果保留声明及处理镜像内容 |
| `zcode` 官方运行时默认 `yolo`（`--prompt` 下） | 权限放大 | 容器命令显式 `--mode`/对应会话模式 + 冻结策略 Hook + 宿主侧复核 |
| 终态事件与权限应答形状未知 | 无法正确判定终态/审批 | P0.2/P0.4 必测；未确认前不实现终态映射 |
| 宿主 MCP 桥（与其它计划共用） | 同 mcode 计划第 6 章 | 同 mcode 计划第 6 章应对 |
| `--prompt` 一次性路线（备选） | 会话模型偏离：每轮新进程、容器生命周期变化、恢复语义弱 | 仅在 app-server 路线被证伪时启用，且需单独设计容器/会话生命周期并重新评估验收 |
| 指令注入通道未确定 | DevDesk 的角色指令与冻结合同送不到模型 | P0 必测（§2.1）；不成立则不进入 P1 |
| 桥调用入队后 worker 未被调度 | 桥调用滞留到超时、步骤工具不通 | 入队即 `wakeups.notify()`（共享地基 2.2.9）；列入桥验收判据第 9 条 |
| 默认权限模式（`--prompt` 下为 `yolo`）叠加未生效的 Hook | 冻结策略无法落实 | 显式设置模式，实测 Hook 阻断与装载证明。任一项被证伪时退回设计 |
