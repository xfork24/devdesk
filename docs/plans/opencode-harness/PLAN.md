# opencode Harness 接入（OpenCode CLI v2.4.1）

日期：2026-11-25。状态：**P0 Linux 部分通过，批次 B 首候选**。范围：接入 opencode CLI v2.4.1（MIT）为新增 Harness，能力对齐 DSH：模型服务商、讨论会话、workflow 步骤工具。不含根编排。

前置依赖为 [Harness 扩展共享地基](../harness-shared-foundation/PLAN.md) 的通用管线、ACP 适配器、宿主 MCP 桥和二进制安装能力。G0 依共享地基第 7 章并行验证。架构独立性改造并行推进，opencode 按当前基线实施，后期归并按共享地基第 8 章执行。

> **批次顺序**：opencode 和 mcode 均通过 P0 时，由 opencode 提供 ACP 与 MCP 桥的首个真实验收。仅一条通过时由该线先接入。opencode 负责自身的启动、模型、策略、ACP 方言与 MCP 注入。

## 0. 已确认决策

| 决策 | 结论 |
|---|---|
| 驱动协议 | **ACP 标准协议**（`opencode acp`，常驻 stdio）；v2 HTTP/SSE API 作为文档化备选路线 |
| 适配器复用 | 与 mcode 共用 `crates/adapters/src/harness_acp.rs`；`harness_opencode.rs` 只承载差异配置 |
| 安装方式 | **与本机同版本的 v2.4.1 发行二进制**（本机 `~/.opencode/bin/opencode` 为 240 MB Mach-O arm64 单体二进制，非 npm v1 线）；容器安装需**新增「二进制发行物」安装面**（共享地基 2.5.6）：现有配方只认 npm（`execution_recipe.rs:219-222`），`native_releases` 只是**宿主侧登录组件**的下载通道（`runtime_preparation.rs:216-230`），不是镜像安装通道 |
| 工具桥 | 一期打通：宿主侧 MCP 服务端承载 workflow 步骤工具；opencode 侧**优先用配置文件注入**（`opencode.json` 的 `mcp` 映射，`type:"remote"` + `url` + `headers`），ACP `session/new.mcpServers` 为备选 |
| 后台服务 | **禁止**使用 `--server` / `--service`；ACP 模式若隐式使用后台服务，必须找到强制私有服务的方式，否则改走备选路线 |
| 审批 | 走 ACP `session/request_permission`；**忽略「永久允许」选项**；**禁用 `--auto`** |
| 根编排 | opencode 不获得 `orchestration` 能力（与 DSH 一致） |
| 全局库 | 版本 24 → 25 在批次 A（pi）一次性完成，CHECK 届时已含 `opencode`；**本计划不改全局库版本**。口径以共享地基第 3 章为准 |

## 1. 调研结论（已核实来源）

### 1.1 来源

| 证据 | 来源 |
|---|---|
| opencode 版本/命令面 | 本机 `~/.opencode/bin/opencode`（v2.4.1）`--help`/`acp --help`/`serve --help`/`run --help` |
| opencode 实现 | 本机包 `~/.opencode/node_modules/@opencode-ai/{plugin,sdk}`（插件钩子类型、v1/v2 API 类型）、二进制内 `XDG_*`/`OPENCODE_CONFIG_DIR` 路径解析 |
| opencode 打包元数据 | `https://api.github.com/repos/anomalyco/opencode/releases`（v2.4.1 含 `opencode-linux-x64/arm64.tar.gz` 与 `SHA256SUMS`）；`https://registry.npmjs.org/opencode-ai/latest`（1.19.6，MIT，optionalDependencies 含 linux 平台包） |
| DevDesk 现状 | 共享地基第 1 章 |

### 1.2 opencode 关键事实

- **许可与形态**：MIT；v2 是 **Bun 编译的单体二进制**（二进制内可见 `/$bunfs/root/...` 打包），本机 `~/.opencode/bin/opencode` 约 240 MB，`~/.opencode/bin/opencode2` 为转发 shim。**四个候选里许可最干净**（无随包专有运行时、无第三方插件非商用条款）。
- **发行物**：v2.4.1 提供 `opencode-linux-x64.tar.gz`、`opencode-linux-arm64.tar.gz` 及 `SHA256SUMS`，用于共享地基的镜像二进制安装路径。宿主侧 `native_releases` 用于登录组件。npm 包 `opencode-ai` 对应 v1 线，本方案锁定 v2 发行物。见 [v2.4.1 发布页](https://github.com/anomalyco/opencode/releases/tag/v2.4.1)。
- **两个驱动面**：
  - `opencode acp`——**ACP stdio 服务端**（与 mcode 同协议族 → 可共用适配器）。
  - `opencode serve`——**v2 HTTP API + Web 服务**（`--hostname`、`--port`、`--cors`、`--service`、`--stdio`），配套 `@opencode-ai/sdk` v2 的强类型接口：`SessionCreate/Prompt/PromptAsync/Messages/Abort`、`PermissionReply/Respond/List`、`QuestionList/Answer`、`EventSubscribe`（SSE）、`ConfigGet/Update`、`ProviderList`、`ToolList`、`Pty*`。→ HTTP 路线协议风险最低，但需要给 harness 适配器新增 HTTP/SSE 传输（当前传输层是 stdio JSONL）。
- **配置面**：`opencode.json`（用户级 `~/.config/opencode/opencode.json`，项目级工作目录同名文件），schema `https://opencode.ai/config.json`；关键字段：`provider`（每项含 `npm`（AI SDK provider 插件）、`models`（`name`/`options`/`limit:{context,output}`）、`options.baseURL`、`options.headers`）、`model`、`small_model`、`mcp`（`type:"local"` 或 `type:"remote"` + `url` + `headers` + `oauth` + `timeout`）、`permission`（ruleset）。
- **MCP 远端形状**：`McpRemoteConfig = {type:"remote", url, enabled?, headers?: Record<string,string>, oauth?, timeout?}` → **宿主 MCP 桥可复用**（opencode 的配置面比 ACP 参数面更可靠，故 opencode 侧以配置文件注入为主）。
- **目录隔离**：二进制内实测路径解析为 `XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME`，并支持 `OPENCODE_CONFIG_DIR`（覆盖配置目录）与 `OPENCODE_TEST_HOME`；本机实例据此落在 `~/.local/share/opencode`（含 `opencode.db` SQLite、`auth.json`）、`~/.config/opencode`。→ 容器内按会话设置 `XDG_DATA_HOME=/state/opencode/data`、`XDG_CONFIG_HOME=/state/opencode/config`、`XDG_STATE_HOME=/state/opencode/state`、`XDG_CACHE_HOME=/state/opencode/cache`、`OPENCODE_CONFIG_DIR=/state/opencode/config/opencode` 即可获得会话级隔离。
- **隔离开关（已核实存在于 2.4.1 二进制）**：`OPENCODE_DISABLE_PROJECT_CONFIG`、`OPENCODE_DISABLE_MODELS_FETCH`、`OPENCODE_DISABLE_AUTOUPDATE`、`OPENCODE_DISABLE_CLAUDE_CODE`（及其 `_PROMPT`、`_SKILLS`）、`OPENCODE_DISABLE_FREE_MODELS`、`OPENCODE_DISABLE_FILEWATCHER`、`OPENCODE_DISABLE_CHANNEL_DB`；另有 `OPENCODE_CONFIG_CONTENT`。开关的存在已核实，**各开关的确切语义由 P0 逐项验证**。二进制内 `models.dev` 出现 13 处 → 启动期有模型目录外联，必须关闭或在探针中证伪。
- **后台服务风险**：opencode v2 默认连**后台服务**（本机 `~/.config/opencode/service.json` 内含服务口令），根命令有 `--standalone`（私有服务）与 `--server`；但 `acp --help` **未列出** `--standalone`（本机 `opencode --help` 已核实根命令有 `--standalone`，`acp --help` 只有 `--login`）→ ACP 模式是否会拉起共享后台服务、能否强制私有服务，**P0 必须实测**（先试 `opencode --standalone acp`）（若会拉起，多个 DevDesk 会话将共享一个服务进程，违反「一数据集一宿主、状态按会话区分」）。
- **插件钩子（策略落点，四者中最规范）**：`@opencode-ai/plugin` 暴露 `tool.execute.before`、`tool.execute.after`、`permission.ask`、`chat.message`、`experimental.chat.messages.transform` 与事件总线 → 可直接承载 DevDesk 冻结策略（读 `/run/devdesk/permission`），等价于 pi 的 `tool_call` 钩子、DSH 的 `tool-policy.mjs`、mcode/zcode 的 `PreToolUse`。
- **审批面**：既有 `permission.ask` 插件钩子，也有配置 `permission` ruleset 与 CLI `--auto`（自动批准未被显式拒绝的权限）。**DevDesk 绝不可使用 `--auto`**：审批必须由宿主按冻结策略与用户裁决驱动。
- **插件与工具注册**：opencode 插件可注册自定义工具（进程内），但按统一决策仍以宿主 MCP 承载 DevDesk 步骤工具。

### 1.3 未实跑验证的部分

以上事实来自本机已安装包与发行元数据的静态阅读，**未在容器内实跑**。P0（见第 3 章）验证前，协议细节不视为已证实的运行行为。

## 2. 方案设计

### 2.1 运行形态

- **Harness 源**：`plugins/core/harnesses/builtin-harness-opencode-managed-v1.json`
  - `data.provider: "opencode"`、`runtime_version: "2.4.1"`、`license: "MIT"`。
  - **安装走 GitHub 发行二进制，版本与本机一致（v2.4.1）**：本机 `~/.opencode/bin/opencode` 是 240 MB 的 Mach-O arm64 单体二进制（`~/.opencode/package.json` 只装了 `@opencode-ai/plugin`）→ 即发行二进制，不是 npm v1 线。容器取**同版本**的 linux 发行件：`opencode-linux-x64.tar.gz` / `opencode-linux-arm64.tar.gz`（摘要取自发行页 `SHA256SUMS`）；**不使用 npm `opencode-ai`**（该包停留在 v1 线 1.19.6，与 v2 不等价）。
    - 采用共享地基 2.5.6 的二进制安装路径，按 `TARGETARCH` 选择资产并在构建期校验摘要。该安装路径可独立落地。
    - v2 发行包在容器内不可用时，本线退回设计。换发行变体或改用 v1 须重新评审和验证。
  - `installation.entrypoint: ["opencode", "acp"]`（ACP stdio 服务端）。
  - `installation.environment`：`XDG_DATA_HOME=/state/opencode/data`、`XDG_CONFIG_HOME=/state/opencode/config`、`XDG_STATE_HOME=/state/opencode/state`、`XDG_CACHE_HOME=/state/opencode/cache`、`OPENCODE_CONFIG_DIR=/state/opencode/config/opencode`，以及隔离开关 `OPENCODE_DISABLE_AUTOUPDATE=1`、`OPENCODE_DISABLE_MODELS_FETCH=1`、`OPENCODE_DISABLE_PROJECT_CONFIG=1`、`OPENCODE_DISABLE_CLAUDE_CODE=1`、`OPENCODE_DISABLE_FREE_MODELS=1`（均见于二进制，语义由 P0 逐项验证）。
  - `installation.files`：策略插件资产（见 2.4）。
- **容器命令**：cwd 固定 `/work`；stdout/stdin 全部是 ACP 协议流，stderr 只作诊断。
- **指令注入通道（P0 必测）**：优先使用 agent 已声明的 ACP `systemPrompt`，也验证私有 `opencode.json` 的 `instructions`。至少一个路径必须能把 DevDesk 的角色指令、快照和合同送入模型上下文。
- **会话隔离**：每个 DevDesk 会话独占一份 `XDG_*` 目录，会话库落在 `/state/opencode/data/opencode.db`。恢复按共享地基 2.2.7 协商 `resume` 或 `loadSession`，复核冻结模型、指令和步骤工具。
- **后台服务隔离**：每个会话独占服务及状态目录。P0 确认 ACP 是否隐式启动或连接后台服务，并验证强制私有服务的配置。若 ACP 路径无法隔离，退回设计并按第 5 章评审备选路线。

### 2.2 协议与事件映射（ACP）

两条 ACP 线以共享地基 2.2 和第 6 章为契约。共用 `harness_acp.rs` 完成事件投影，本节记录 opencode 方言需要验证的差异。

- `harness_acp.rs` 负责：ACP 握手与协议版本协商、`session/new`/`session/resume`（不重放历史；见共享地基 2.2.7）、`session/prompt` 派发、`session/update` → `Obs` 投影、`session/request_permission` → `Obs::Interaction{ToolApproval}` 与应答、`session/cancel` 停止、`stopReason` → 终态、桥接 MCP 的 `mcpServers` 注入。
- `harness_mcode.rs` / `harness_opencode.rs` 只提供**差异配置**：进程启动参数、环境变量、模型/服务商配置写入方式、容器内策略钩子的装载方式、恢复时的外部会话 id 语义。
- 差异点：
  - opencode 的工具事件类型可能更细（读/写/执行/补丁等），P0 记录实际 `tool_call` 形态后统一归一。
  - opencode 的 ACP 可能额外发 `plan_update`；一期忽略（同 mcode）。
  - opencode 的 `session/request_permission` 选项集合与 mcode 不同（opencode 侧另有 `always`（永久允许）语义）→ **DevDesk 必须忽略「永久允许」选项**，只接受单次允许/拒绝，避免冻结权限被运行时长期放宽。
- **接纳与消费**按共享地基 2.2.8 和第 6 章。P0.2 记录实际应答时机，并验证早到审批、早到 MCP 调用及迟到请求的轮次归属。
- **帧路径**：`sessionUpdate` 取值在 `params.update.sessionUpdate`（同 mcode 计划 §2.2）。
- **终态与终稿**：与 mcode 同口径（共享地基第 6 章）——`max_tokens` 判 Failed；工具调用失败只记 Tool 事件、不改终态；助手文本以工具调用为边界分段封存。

### 2.3 工具桥与审批（一期打通）

1. **注入方式**：opencode 的 MCP 既可走 ACP `session/new.mcpServers`，也可走配置文件 `opencode.json` 的 `mcp` 映射（`{type:"remote", url, headers}`，类型已核实支持 `headers`）。**优先用配置文件注入**（`opencode.json` 由宿主在会话启动时以 0600 写入 `/state/opencode/config/opencode/opencode.json`），因为配置面是 opencode 的稳定公开契约；ACP 参数注入作为备选（若 P0 证明配置文件不生效）。该文件位于宿主侧的会话状态目录，会话结束后会残留 → `cleanup_worker` 时覆盖或删除，并由令牌撤销兜底（共享地基 2.4.2）。
2. **端点与鉴权**：复用共享地基 2.4 的宿主 MCP 桥（独立监听器 + 每会话令牌 + 路径含会话 id）；opencode 侧配置为：
   ```json
   {"mcp":{"devdesk":{"type":"remote",
     "url":"http://host.docker.internal:<bridge-port>/internal/tool-bridge/<session>/mcp",
     "headers":{"Authorization":"Bearer <session-token>"},
     "enabled":true,"timeout":120000}}}
   ```
   注意 `timeout` 需大于工具执行预算（opencode 默认 MCP 超时 5 s，远小于 DevDesk 工具超时）。
3. **执行与回执**复用共享地基 2.4.8，由 core 按冻结工具种类接纳、执行、记账并回传。
4. **审批（口径见共享地基 2.4.5）**：`session/request_permission` → 宿主 `Obs::Interaction{ToolApproval}` → 用户裁决 → 以同一 `id` 应答；冻结策略为 `never` 时自动选择拒绝项；允许时只选 `kind == allow_once`，**永不选「永久允许」**。**明确禁止 `--auto`**（会绕过宿主裁决）。
   - **宿主桥调用不设适配器层审批**：准入完全由 core 按冻结权限判定（`approval == never` 且文件系统非只读时平台写操作直接执行；`on-request` 下 core 拒绝），桥把拒绝原样返回。此前版本写的「桥调用经 `PendingInteraction` 往返、`never` 时直接拒绝」是 core 里不存在的流程，且会让默认 Profile 下的步骤工具被全部拒绝，**不得实现**。
5. **一致性**：宿主桥与 harness 审批共用同一份冻结权限；容器内插件钩子（2.4）同样读同一份 `/run/devdesk/permission`。**执行语义与 mcode/zcode/pi 完全一致**——差别只在传输与触发来源，桥调用同样进入会话记录与治理记录。
   - **工具投放**按共享地基 2.4.4，隐藏未绑定工具，按工具种类验证回执和治理。平台工具的授权要求写入 `limitations`，普通讨论中的显式调用返回 `tool_not_authorized`。
   - **网络边界（已更正）**：Harness 工作容器恒为联网容器，不存在「禁网 Profile 下宿主桥不可达」的场景。真实的边界是：不支持 harness 层工具禁网（`disabled_tools`）；**远程 Docker daemon 下步骤工具不可用**。两项写入 `limitations`（共享地基 2.4.6）。
   - **唤醒与 MCP 工具名**（共享地基 2.2.9、2.4.4）：桥调用入队即 `RuntimeWorkers.wakeups.notify()`——opencode 等待 MCP 结果期间不产生协议帧，不唤醒则调用滞留到超时；P0 记录 opencode 给 MCP 工具加的服务器前缀与模型可见名长度。

### 2.4 权限与策略后端

1. **容器内冻结策略**：`permissions/backends/opencode/plugin.js`（opencode 插件，`tool.execute.before` + `permission.ask` 钩子）——读 `/run/devdesk/permission`，对不在冻结 allow/deny 集合内的工具与升级请求**抛错阻断**（opencode 插件钩子支持阻断，`tool.execute.before` 抛错即拒绝执行）。插件装载后必须发出一个**装载信号**，供 Host 在首轮输入前核对（共享地基 2.1.7）；拿不到会话级信号时退回「配置独占 + 镜像探针」。
2. **能力声明** `capabilities(Opencode)`：filesystem 三档；network 仅 `enabled`；tools `all_available` + `restricted`；`limitations` 如实列出：**无进程内宿主工具注册通道（步骤工具走宿主 MCP）**、审批为逐次询问、cwd 固定 `/work`、**不支持「永久允许」语义（已冻结权限不允许被运行时放宽）**、ACP 模式不得使用后台服务、**不支持 harness 层工具禁网（`disabled_tools`）**、**不支持 Agent 提问（`elicitation/create` 被自动取消）**、**远程 Docker daemon 下步骤工具不可用**、**平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用**。
3. **校验分支**：`validate_execution_permissions` 的 opencode 分支（拒绝 `Disabled` 网络；scoped 规则必须由冻结挂载落实）。
4. **执行合同命名**：后端 `opencode-plugin-v1`，不使用 `native` 字样。
5. **资产登记**：`permissions/backends/manifest.json` 新增后端与资产 sha256（条目需写全 `id`（`opencode-plugin-v1`）、`provider`（`opencode`）、`version`、`assets`；schema 的 `provider` 枚举当前只有 `docker|codex|dsh`）；`schemas/permission-backend.schema.json` 同步。
6. **仓库内可执行配置隔离（共享地基 2.1.8）**：opencode 的状态目录在 `/state/opencode/...`，**但会从工作目录读取项目级配置与插件**（`.opencode/`、`opencode.json`、`AGENTS.md`、Claude Code 配置）——仓库内容是不可信数据，其中的插件与 MCP 服务声明不能被加载。一期用 `OPENCODE_DISABLE_PROJECT_CONFIG=1` 与 `OPENCODE_DISABLE_CLAUDE_CODE=1` 关闭（P0 验证生效）；关不掉的路径加入 `protected_metadata`（`opencode.json` 是文件型条目，需确认白名单与挂载逻辑支持）。三处白名单必须同步：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`（**硬编码白名单**，只改前两处会在容器创建时被第三处拒绝）。

### 2.5 模型与凭据

1. **provider 源**：新建 `opencode-minimax-cn`、`opencode-zai-coding-cn`、`opencode-deepseek`、`opencode-ollama`；新建 Model 源与 `builtin-model-catalog-opencode-v1.json`。
2. **配置写入**：`crates/adapters/src/model_providers.rs` 新增 `opencode_model_patch(connection, model)`，生成 `/state/opencode/config/opencode/opencode.json`：
   ```json
   {"provider":{"<provider-id>":{"name":"<name>","npm":"@ai-sdk/openai-compatible",
     "options":{"baseURL":"<base>","headers":{"Authorization":"Bearer <key>"}},
     "models":{"<model-id>":{"name":"<model-id>","limit":{"context":<ctx>,"output":<out>}}}}},
    "model":"<provider-id>/<model-id>"}
   ```
   - `limit.context/output` 必须由 DevDesk 的模型目录供给（opencode 配置类型要求该结构），不能留空。
   - `options.headers` 中的 API key 由宿主在会话启动时写入，写完即从临时文件删除（沿用 `harness.rs` 现有凭据文件生命周期），且**不得进入协议日志**（共享地基 2.2.5 的结构化脱敏必须覆盖 `options.headers` 这类嵌套结构）。
   - **provider `npm` 包的安装路径（P0 必测）**：`@ai-sdk/openai-compatible`、`@ai-sdk/anthropic` 等包若由 opencode 在**运行时**从 npm 拉取，每次会话启动都会有未固定版本的外联下载，不可接受。必须确认可预装并钉版本进入镜像，或改用其内置协议；如确有运行时安装，其可写目录必须落在会话私有的 `XDG_CACHE_HOME` 内。
3. **凭据链路**：按共享地基 2.3 泛化后的键 `credentials.opencode.{provider}`；macOS 钥匙串账户前缀白名单加入 opencode 的服务商前缀。
4. **服务商公开 id 全局唯一**（`validate_model_provider`）——与 dsh/pi/mcode/zcode 的 provider id 不冲突。

## 3. 工作项

### P0 可行性验证（G0 并行）

本轮 G0 结论为 **延期（Linux 协议、MCP 与私有 Shell 禁用已验证，策略插件、macOS 和 arm64 待验）**。见 [共享实施记录](../harness-shared-foundation/EXECUTION.md)。

1. 用两个目标 Linux 架构的固定发行物手工构建镜像，验证构建期摘要、解包、`opencode --version`、依赖、体积和启动耗时。独立使用零凭据、断网容器探针验证 ACP 启动与最小握手。
2. **ACP 最小闭环**：用真实 CLI 和获授权模型或受控模型夹具验证建会话、流式、终态、取消。记录 `initialize` 声明的 `mcpCapabilities.http`、`sessionCapabilities.resume`、`agentCapabilities.loadSession` 和 `sessionCapabilities.systemPrompt`，实测恢复。确认轮次应答时机、早到审批和 MCP 调用及上一轮迟到事件。
3. **后台服务隔离**：确认 ACP 模式是否使用后台服务。若使用，实测 `opencode --standalone acp` 或等价设置，验证两个会话各自使用私有服务。无法确认隔离时，按 G0 判据收口并另行评审备选路线。
4. **会话目录隔离**：确认 `XDG_*` + `OPENCODE_CONFIG_DIR` 组合能让两个容器实例互不共享会话库与配置。
5. **策略钩子**：确认插件 `tool.execute.before` 抛错能真正阻断工具调用，`permission.ask` 能参与权限决策；**并确认 opencode 从哪里发现插件**（`OPENCODE_CONFIG_DIR` 下的 `plugin/`？项目级目录？`XDG_CONFIG_HOME` 下的用户级插件目录？）——发现路径不定，冻结策略会**静默不生效**（本计划已把「策略静默不生效」列为最危险风险）；同时确定可供 Host 在首轮输入前核对的**装载信号**（共享地基 2.1.7）。
6. **MCP 注入**：用 `opencode.json` 或经能力协商的 ACP 参数连接 G0 共用探针，验证 macOS/Linux 上的工具往返及恢复后仍生效，记录模型可见名、超时、请求身份与重试行为。
7. **模型与凭据**：确认 `provider.options.headers` 形式的 API key provider 可用，`limit` 字段被接受，`model` 选择生效；确认 provider 的 `npm` 包是预装还是运行时拉取（§2.5）。
8. **指令注入通道**（§2.1）：验证 ACP `systemPrompt` 或配置 `instructions` 能把 DevDesk 指令送进模型上下文；两者皆不成立则本计划不进入 P1。
9. **隔离与外联**：逐项验证 `OPENCODE_DISABLE_PROJECT_CONFIG`、`OPENCODE_DISABLE_CLAUDE_CODE`、`OPENCODE_DISABLE_MODELS_FETCH`、`OPENCODE_DISABLE_AUTOUPDATE` 的语义，确认仓库内可执行配置和未固定的运行时下载均被排除。
10. 按共享地基第 7 章记录「通过」「退回设计」「延期」。隔离、冻结策略或指令注入被证伪时退回设计，备选路线另行评审。

### P1

在内部制品接入 `Opencode` 的注册和路由、策略插件、`opencode_model_patch`、零凭据断网探针、出厂配置和模型目录。复用批次 A 的凭据、Schema 和设置页机制。公开可选入口、出厂源与权限资产随批次 B 发布。

**安装及模型配置**

1. 接入共享地基 2.5.6 的二进制安装能力，固定两个目标架构的发行资产及摘要，核对 `default_opencode_recipe_digest`。
2. 在批次 A 的 `model_profile_digest` 分派中接入 opencode patch。

### P2 协议驱动

`crates/adapters/src/harness_opencode.rs` 只提供差异配置与启动/停止接线；事件投影复用 `harness_acp.rs`；协议测试用例复用 ACP 夹具 + opencode 特有差异（工具类型归一、永久允许选项被忽略）配合脱敏样本 `tests/fixtures/protocols/opencode-acp-turn.json`。

### P3 工具桥与审批

复用共享地基 2.4 的宿主 MCP 桥；opencode 侧写 `opencode.json` 的 `mcp` 配置并注入令牌；会话清理时覆盖或删除含令牌的配置文件；权限应答与「永久允许」拒绝测试。

按共享地基 2.4.8 复用收件箱、唤醒和回执路由。按 2.4.9 验收九项判据，覆盖早到调用、独立审批、按工具种类审计及写调用超时核对。

### P4 验收与文档

`native_acceptance.py` 的 opencode 套件、probe provider 选项、浏览器枚举 spec 与当前文档同步。讨论、步骤工具和权限验收通过后，使用批次 A 的 v25 副本演练发布。公开入口、出厂源和权限资产随批次 B 集中发布。

## 4. 验证

- 自动检查同 mcode 计划第 4 章。
- 真实运行（`./scripts/dev/dev.sh --data ./data-opencode --port 4320`）：
  1. 镜像准备与协议探针通过；API key provider 可用。
  2. 讨论轮次：流式输出、终稿、终态（Completed/Failed/Cancelled）。
  3. 工具事件入账；只读 Profile 下写入被拦截且原因明确（插件钩子生效）。
  4. 步骤工具（宿主 MCP 桥）往返 + 宿主回执，且该调用**出现在会话记录与治理记录中（可审计）**；同一 `call_id` 重放不重复执行；默认 `never` Profile 下步骤工具正常执行。
  5. 取消轮次 → 停止模式正确、容器移除。
  6. 重启 Host 后 `session/resume` 恢复成功，且恢复的是本会话的 opencode 会话库；长会话（多轮带工具调用）恢复不触发初始化缓冲上限。
  7. 仓库内放置含插件与 MCP 服务声明的 `.opencode/` 与 `opencode.json`，确认它们**不被加载**（配置隔离验证）。
  8. 两个并行 opencode 会话互不共享配置/会话库（后台服务隔离验证）；会话清理后宿主侧不再残留可用的桥令牌。

## 5. 备选路线：v2 HTTP/SSE API

ACP 路线被 P0 证伪时，可另行评审 opencode v2 HTTP API。候选方案是每个容器独占 `opencode serve`，宿主通过仅绑定宿主 loopback 的发布端口访问。服务需监听容器中可被 Docker 转发的地址，并在备选方案验证中核对实际连通性与隔离。驱动用 `@opencode-ai/sdk` v2 的会话、审批和 SSE 接口。

代价与影响（需在切换时重新评审）：

- 传输层要新增 HTTP + SSE 客户端（当前 `jsonl.rs` 只做 stdio JSONL），且要把容器端口发布纳入 `docker.rs` 的安全审查（仅绑定宿主 loopback）。
- 会话与容器生命周期的映射要重做（HTTP 会话在服务进程内，容器重启后会话库仍在 `/state`，需确认恢复语义）。
- 好处：协议是**有类型、有版本**的公开 API，风险低于 ACP 细节与 zcode 私有协议。

## 6. 不在本期范围

- 根编排（opencode 不获得 `orchestration`，与 DSH 一致）。
- Agent 提问：ACP `elicitation/create` 通道一期由适配器直接应答取消，写入 `limitations`。
- 容器内工具禁网；opencode 的进程内插件工具注册（统一走宿主 MCP）。

## 7. 风险与兼容

| 风险 | 影响 | 应对 |
|---|---|---|
| ACP 模式隐式使用后台服务 | 多会话共享服务进程，破坏会话隔离与冻结状态 | P0.3 必测；必要时 `--standalone` 或第 5 章的 v2 HTTP/SSE 路线 |
| 单体二进制体积（~240 MB） | 镜像变大、构建变慢 | 固定发行摘要；评估是否裁剪 Desktop 资产（发行包已分离 CLI 与 Desktop 资产） |
| `opencode.json` 的 provider 结构随版本变动 | 模型连接失效 | 版本钉死 + `model_profile_digest` 随 provider patch 变化（共享地基 2.5） |
| 插件钩子契约变动 | 冻结策略失效（最危险：策略静默不生效） | P0.5 必测；策略插件必须有「未加载即拒绝执行关键工具」的失败模式（fail-closed） |
| 「永久允许」语义 | 冻结权限被运行时长期放宽 | 宿主侧忽略该选项并记入事件流；2.2/2.3 已有约束 |
| 密钥出现在 `options.headers` | 泄漏进日志或配置残留 | 共享地基 2.2.5 结构化脱敏 + 会话启动后删除凭据文件 + 写配置时 0600 权限 |
| 与 mcode 共享 ACP 适配器的耦合 | mcode 的改动影响 opencode | 适配器按 provider 配置注入，不在共用层写 provider 分支；两套 provider 各自的协议测试夹具独立 |
| `opencode acp` 没有系统提示开关 | DevDesk 的角色指令与冻结合同送不到模型 | P0.8 必测：ACP `systemPrompt` → 配置 `instructions`；皆不成立则不进入 P1 |
| 仓库内 `.opencode/`、`opencode.json` 被当作项目级配置加载 | 不可信的仓库内容可注入插件与 MCP 服务 | `OPENCODE_DISABLE_PROJECT_CONFIG` + `OPENCODE_DISABLE_CLAUDE_CODE`（P0.9 验证）；不足再屏蔽 |
| provider 的 `npm` 包在运行时从 npm 拉取 | 未固定版本的外联下载；断网构建或离线环境失败 | P0.7 必测：预装并钉版本，或使用内置协议 |
| 桥调用入队后 worker 未被调度 | 桥调用滞留到超时、步骤工具不通 | 入队即 `wakeups.notify()`（共享地基 2.2.9）；列入桥验收判据第 9 条 |
| `session/load` 重放整段历史 | 长会话恢复撞上初始化缓冲上限 | 按共享地基 2.2.7 协商恢复并有界丢弃重放 |
