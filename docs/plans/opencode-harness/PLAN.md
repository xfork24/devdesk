# opencode Harness 接入（OpenCode CLI v2.4.1）

日期：2026-11-25。状态：**待实施**。范围：接入 opencode CLI v2.4.1（MIT）为第六个 Harness，能力对齐 DSH：模型服务商、讨论会话、workflow 步骤工具。不含根编排。

前置依赖：[Harness 扩展共享地基](../harness-shared-foundation/PLAN.md) 必须先完成；**ACP 适配器由 mcode 计划（[mcode](../mcode-harness/PLAN.md)）先落地，本计划复用之**。

> **定位**：三个新接入线里**协议风险最低、许可最干净、边际成本最小**的一个——与 mcode 走同一套 ACP 标准协议，**共用 `harness_acp.rs` 适配器**，只新增自己的启动配置、模型配置、策略钩子与 MCP 注入。

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
| 全局库 | **一次全上**：24 → 25，`provider` CHECK 含 `pi,mcode,zcode,opencode`；口径以共享地基第 3 章为准 |

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
- **发行物**：`anomalyco/opencode` GitHub release v2.4.1 提供 `opencode-linux-x64.tar.gz`、`opencode-linux-arm64.tar.gz`（另有 `-musl`、`-baseline` 变体）及 `SHA256SUMS`，每个资产带 `sha256` 摘要 → **正好匹配 DevDesk 的 `HarnessInstallation.native_releases`（url/sha256/member）机制**（Codex 就是这条路）。npm `opencode-ai` latest 为 1.19.6（v1 线，MIT，optionalDependencies 含 `opencode-linux-x64/arm64` 等平台包，含 `postinstall`）——**v1 与 v2 不等价**，DevDesk 应锁 v2 发行二进制，不走 npm v1 线。
- **两个驱动面**：
  - `opencode acp`——**ACP stdio 服务端**（与 mcode 同协议族 → 可共用适配器）。
  - `opencode serve`——**v2 HTTP API + Web 服务**（`--hostname`、`--port`、`--cors`、`--service`、`--stdio`），配套 `@opencode-ai/sdk` v2 的强类型接口：`SessionCreate/Prompt/PromptAsync/Messages/Abort`、`PermissionReply/Respond/List`、`QuestionList/Answer`、`EventSubscribe`（SSE）、`ConfigGet/Update`、`ProviderList`、`ToolList`、`Pty*`。→ HTTP 路线协议风险最低，但需要给 harness 适配器新增 HTTP/SSE 传输（当前传输层是 stdio JSONL）。
- **配置面**：`opencode.json`（用户级 `~/.config/opencode/opencode.json`，项目级工作目录同名文件），schema `https://opencode.ai/config.json`；关键字段：`provider`（每项含 `npm`（AI SDK provider 插件）、`models`（`name`/`options`/`limit:{context,output}`）、`options.baseURL`、`options.headers`）、`model`、`small_model`、`mcp`（`type:"local"` 或 `type:"remote"` + `url` + `headers` + `oauth` + `timeout`）、`permission`（ruleset）。
- **MCP 远端形状**：`McpRemoteConfig = {type:"remote", url, enabled?, headers?: Record<string,string>, oauth?, timeout?}` → **宿主 MCP 桥可复用**（opencode 的配置面比 ACP 参数面更可靠，故 opencode 侧以配置文件注入为主）。
- **目录隔离**：二进制内实测路径解析为 `XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME`，并支持 `OPENCODE_CONFIG_DIR`（覆盖配置目录）与 `OPENCODE_TEST_HOME`；本机实例据此落在 `~/.local/share/opencode`（含 `opencode.db` SQLite、`auth.json`）、`~/.config/opencode`。→ 容器内按会话设置 `XDG_DATA_HOME=/state/opencode/data`、`XDG_CONFIG_HOME=/state/opencode/config`、`XDG_STATE_HOME=/state/opencode/state`、`XDG_CACHE_HOME=/state/opencode/cache`、`OPENCODE_CONFIG_DIR=/state/opencode/config/opencode` 即可获得会话级隔离。
- **后台服务风险**：opencode v2 默认连**后台服务**（本机 `~/.config/opencode/service.json` 内含服务口令），根命令有 `--standalone`（私有服务）与 `--server`；但 `acp --help` **未列出** `--standalone` → ACP 模式是否会拉起共享后台服务、能否强制私有服务，**P0 必须实测**（若会拉起，多个 DevDesk 会话将共享一个服务进程，违反「一数据集一宿主、状态按会话区分」）。
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
    - **前置改造（不可跳过，共享地基 2.5.6）**：现有安装通道只有 npm 一条——`compose_execution_recipe` 写死 `RUN npm install --global --omit=dev {package}@{version}`，且 `HarnessInstallation.package` 必填非空；`native_releases` 只被 `prepare_codex_auth` 用于宿主侧登录组件，**不是镜像安装通道**。→ 需新增「二进制发行物」安装面：`package` 可选化 + 新字段（url/sha256/member）+ Dockerfile 生成下载/校验/解包 + 可执行文件落位并入 PATH + `ExecutionRecipe::validate` 与 `recipe_digest` 覆盖。**与 mcode 的 npm 参数面扩展同批实现**（同一处配方生成代码，避免两次改动与两次摘要漂移）。
    - 备选：若 P0 证明 v2 发行包在容器内不可用，回退 npm v1 线并接受协议差异（记录在案）。
  - `installation.entrypoint: ["opencode", "acp"]`（ACP stdio 服务端）。
  - `installation.environment`：`XDG_DATA_HOME=/state/opencode/data`、`XDG_CONFIG_HOME=/state/opencode/config`、`XDG_STATE_HOME=/state/opencode/state`、`XDG_CACHE_HOME=/state/opencode/cache`、`OPENCODE_CONFIG_DIR=/state/opencode/config/opencode`、`OPENCODE_DISABLE_AUTOUPDATE=1`（若存在该开关；否则靠 `XDG_*` 隔离 + 断网构建）。
  - `installation.files`：策略插件资产（见 2.4）。
- **容器命令**：cwd 固定 `/work`；stdout/stdin 全部是 ACP 协议流，stderr 只作诊断。
- **会话隔离**：每个 DevDesk 会话独占一份 `XDG_*` 目录（1.2），会话库落在 `/state/opencode/data/opencode.db`；恢复时用 ACP `session/load` 携带 `sessionId`。
- **后台服务禁令**：**禁止**让 opencode 连接或拉起共享后台服务（`--server` / `--service` 一律不用）。P0 必须确认 ACP 模式是否隐式使用后台服务；若会，则必须找到强制私有服务的开关（或改用第 5 章的 `serve --standalone` 路线），否则多个 DevDesk 会话会共享一个服务进程，违反「状态按会话区分」。

### 2.2 协议与事件映射（ACP，复用 mcode 计划的表）

opencode 的 ACP 事件面与 mcode 计划 2.2 同构，**直接复用同一张映射表与同一套 `Obs` 投影**；实现上不在 `harness_opencode.rs` 复制逻辑，而是复用 `crates/adapters/src/harness_acp.rs`：

- `harness_acp.rs` 负责：ACP 握手与协议版本协商、`session/new`/`session/load`、`session/prompt` 派发、`session/update` → `Obs` 投影、`session/request_permission` → `Obs::Interaction{ToolApproval}` 与应答、`session/cancel` 停止、`stopReason` → 终态、桥接 MCP 的 `mcpServers` 注入。
- `harness_mcode.rs` / `harness_opencode.rs` 只提供**差异配置**：进程启动参数、环境变量、模型/服务商配置写入方式、容器内策略钩子的装载方式、恢复时的外部会话 id 语义。
- 差异点：
  - opencode 的工具事件类型可能更细（读/写/执行/补丁等），P0 记录实际 `tool_call` 形态后统一归一。
  - opencode 的 ACP 可能额外发 `plan_update`；一期忽略（同 mcode）。
  - opencode 的 `session/request_permission` 选项集合与 mcode 不同（opencode 侧另有 `always`（永久允许）语义）→ **DevDesk 必须忽略「永久允许」选项**，只接受单次允许/拒绝，避免冻结权限被运行时长期放宽。
- **共用同一套接纳语义**（mcode 计划 §2.2）：ACP 的 `session/prompt` 应答在**轮次结束**才到 → 宿主在提交时合成 `Obs::Admitted{external_turn: 宿主生成的轮次 id}` 并写入 turn 记录，应答帧改判终态；`Obs::Consumed` 的产生时机同 mcode 口径。opencode 若在 `session/prompt` 之外另有「立即回执」方法，需在 P0.2 记录并在 `harness_acp.rs` 里按 provider 配置化。
- **帧路径**：`sessionUpdate` 取值在 `params.update.sessionUpdate`（同 mcode 计划 §2.2）。

### 2.3 工具桥与审批（一期打通）

1. **注入方式**：opencode 的 MCP 既可走 ACP `session/new.mcpServers`，也可走配置文件 `opencode.json` 的 `mcp` 映射（`{type:"remote", url, headers}`，类型已核实支持 `headers`）。**优先用配置文件注入**（`opencode.json` 由宿主在会话启动时写入 `/state/opencode/config/opencode/opencode.json`），因为配置面是 opencode 的稳定公开契约；ACP 参数注入作为备选（若 P0 证明配置文件不生效）。
2. **端点与鉴权**：复用共享地基 2.4 的宿主 MCP 桥（独立监听器 + 每会话令牌 + 路径含会话 id）；opencode 侧配置为：
   ```json
   {"mcp":{"devdesk-tools":{"type":"remote",
     "url":"http://host.docker.internal:<bridge-port>/internal/tool-bridge/<session>/mcp",
     "headers":{"Authorization":"Bearer <session-token>"},
     "enabled":true,"timeout":120000}}}
   ```
   注意 `timeout` 需大于工具执行预算（opencode 默认 MCP 超时 5 s，远小于 DevDesk 工具超时）。
3. **执行与回执（统一口径：经 core 接纳/审计链路）**：桥**不执行工具**。`tools/call` 由 MCP 服务端投递到该会话 worker 的桥收件箱 → worker 产出 `Obs::ToolCall{request:{"kind":"bridge","call_id":…}, …}` → core 走既有接纳（冻结 `tool_refs`、`qualified_name` 解析、`provider_tool_intent` 治理记录、轮次状态与 guards 版本）→ `ToolExecutor` 执行 → `harness.answer_tool_call` 回执 → 桥返回 MCP 结果。完整机制见共享地基 2.4.8。
4. **审批**：`session/request_permission` → 宿主 `Obs::Interaction{ToolApproval}` → 用户裁决 → 以同一 `id` 应答；冻结策略为 `never` 时自动选择拒绝项。**明确禁止 `--auto`**（会绕过宿主裁决）。
   - 桥触发的审批同样走既有交互流：worker 产出 `Obs::Interaction{ToolApproval, request:{"kind":"bridge",…}}` → core 建 `PendingInteraction` → 用户裁决 → `harness.answer_interaction` → 按 `kind` 交回桥；`never` 时 worker 直接产出拒绝决定。
5. **一致性**：宿主桥与 harness 审批共用同一份冻结权限；容器内插件钩子（2.4）同样读同一份 `/run/devdesk/permission`。**执行语义与 mcode/zcode/pi 完全一致**——差别只在传输与触发来源，桥调用同样进入会话记录与治理记录。
   - **暴露的工具范围（四类全暴露，含平台工具）**：桥产出观察时 `tool` 按种类取值——集成/未绑定工具用 `qualified_name`，**平台工具用 `call_name()`**；core 已按名分派（`conversation_runtime.rs:334-345`），平台工具自动走平台准入路径，**不需要新增 grant 路由或端口**。平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用，普通讨论会话被 `tool_not_authorized` 拒绝 → 写入 `limitations`。
   - **禁网边界（本期明确不支持）**：冻结网络为 Disabled 的 Profile 下容器为 `--network none`，宿主桥物理不可达 → 步骤工具不可用，写入 `limitations`（共享地基 2.4.6）。

### 2.4 权限与策略后端

1. **容器内冻结策略**：`permissions/backends/opencode/plugin.js`（opencode 插件，`tool.execute.before` + `permission.ask` 钩子）——读 `/run/devdesk/permission`，对不在冻结 allow/deny 集合内的工具与升级请求**抛错阻断**（opencode 插件钩子支持阻断，`tool.execute.before` 抛错即拒绝执行）。
2. **能力声明** `capabilities(Opencode)`：filesystem 三档；network 仅 `enabled`；tools `all_available` + `restricted`；`limitations` 如实列出：**无进程内宿主工具注册通道（步骤工具走宿主 MCP）**、审批为逐次询问、cwd 固定 `/work`、**不支持「永久允许」语义（已冻结权限不允许被运行时放宽）**、ACP 模式不得使用后台服务、**冻结网络为 Disabled 的 Profile 下步骤工具不可用（宿主桥在 `--network none` 下不可达）**、**平台工具仅在具备 controller 授权或 workflow 执行归属的会话可调用**。
3. **校验分支**：`validate_execution_permissions` 的 opencode 分支（拒绝 `Disabled` 网络；scoped 规则必须由冻结挂载落实）。
4. **执行合同命名**：后端 `opencode-plugin-v1`，不使用 `native` 字样。
5. **资产登记**：`permissions/backends/manifest.json` 新增后端与资产 sha256（条目需写全 `id`（`opencode-plugin-v1`）、`provider`（`opencode`）、`version`、`assets`；schema 的 `provider` 枚举当前只有 `docker|codex|dsh`）；`schemas/permission-backend.schema.json` 同步。
6. `protected_metadata` 是否加入 `.opencode`：**先核实再定**——opencode 的状态目录实际是 `~/.config/opencode` 与 `~/.local/share/opencode`（容器内 `/state/opencode/...`），**不在 `/work` 内**，而 `protected_metadata` 只作用于 `/work` 下的叶子目录（`docker.rs:1008`）；若 opencode 不在项目目录写 `.opencode/`，则无需加入，避免引入无效条目。若确定要加，**三处必须同步**：`crates/domain/src/permission.rs:255`、`crates/adapters/src/permissions.rs:97`、`crates/adapters/src/docker.rs:1004`（**硬编码白名单** `require([".git",".codex",".agents"].contains(&leaf))`）。

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
3. **凭据链路**：按共享地基 2.3 泛化后的键 `credentials.opencode.{provider}`；macOS 钥匙串账户前缀白名单加入 opencode 的服务商前缀。
4. **服务商公开 id 全局唯一**（`validate_model_provider`）——与 dsh/pi/mcode/zcode 的 provider id 不冲突。

## 3. 工作项

### P0 可行性验证

1. 用发行 tarball 构建 opencode 镜像：`opencode-linux-x64.tar.gz` / `opencode-linux-arm64.tar.gz` 解包后二进制是否可直接运行（是否需要 glibc 版本、是否有额外运行时依赖）；`opencode --version` 输出是否符合预期；镜像体积与启动时间是否可接受（约 240 MB 单体二进制）。**同时验证「二进制发行物」安装面**（新增字段 → Dockerfile 下载/校验 sha256/解包 → 可执行文件就位）是否可行；若不可行则按第 5 章回退。
2. **ACP 最小闭环**：容器内 `opencode acp` → `initialize` → `session/new` → `session/prompt` → 观察 `session/update` 与 `stopReason`；确认协议版本与 `session/new` 是否接受 `mcpServers`；**确认 `session/prompt` 应答的时机（轮次末 vs 立即）**（决定 §2.2 的宿主合成接纳口径与流式是否成立）。
3. **后台服务隔离**：确认 ACP 模式是否使用后台服务；若使用，找出强制私有服务的方式（`--standalone` 是否被 `acp` 接受 / 是否有等价环境变量）；否则改走第 5 章。
4. **会话目录隔离**：确认 `XDG_*` + `OPENCODE_CONFIG_DIR` 组合能让两个容器实例互不共享会话库与配置。
5. **策略钩子**：确认插件 `tool.execute.before` 抛错能真正阻断工具调用，`permission.ask` 能参与权限决策；**并确认 opencode 从哪里发现插件**（`OPENCODE_CONFIG_DIR` 下的 `plugin/`？项目级目录？`XDG_CONFIG_HOME` 下的用户级插件目录？）——发现路径不定，冻结策略会**静默不生效**（本计划已把「策略静默不生效」列为最危险风险）。
6. **MCP 注入**：确认 `opencode.json` 的 `mcp`（remote + headers）能被运行时加载，并能从容器内连到宿主 MCP 桥端点（与 mcode 计划 P0.3 共用同一验证）。
7. **模型与凭据**：确认 `provider.options.headers` 形式的 API key provider 可用，`limit` 字段被接受，`model` 选择生效。

### P1

与 mcode 计划同构：provider 注册与显式分支（含 Opencode）；`harness_acp.rs` 从 `harness_mcode.rs` 抽出为共用模块（mcode 计划的 P2 需同步重构）；`permissions/backends/opencode/` 资产；`opencode_model_patch`；`runtime_verification.rs` 的 opencode 探针（`initialize` + `session/new`，无密钥无网络）；`default_opencode_recipe_digest`；出厂配置与模型目录；凭据泛化；Schema 枚举；前端（按 harness 渲染的 provider 设置页复用）。

**本计划独有的前置改造**：

1. **「二进制发行物」安装面**（共享地基 2.5.6，**与 mcode 的 npm 参数面扩展同批实现**）：`HarnessInstallation.package` 可选化 + 新字段（url/sha256/member）+ `compose_execution_recipe` 生成下载/校验/解包/落位步骤 + `ExecutionRecipe::validate` 与 `recipe_digest` 覆盖 + `execution_recipe.rs:165-171` 的 package 校验放宽。**不做这一步，opencode 就无法在容器内使用与本机同版本的 v2.4.1 发行二进制**（现有配方只认 npm；`native_releases` 不是镜像安装通道）。摘要取自发行页 `SHA256SUMS`，按 `linux/x86_64` 与 `linux/aarch64` 分别钉住。
2. `model_profile_digest` 的 provider 分派：`crates/adapters/src/session_resources.rs:207-209` 目前**写死** `dsh_model_patch` → 需按 provider 选 patch（这是四条新线共用的同一处改动，应与共享地基一起做，避免重复与遗漏）。

### P2 协议驱动

`crates/adapters/src/harness_opencode.rs` 只提供差异配置与启动/停止接线；事件投影复用 `harness_acp.rs`；协议测试用例复用 ACP 夹具 + opencode 特有差异（工具类型归一、永久允许选项被忽略）配合脱敏样本 `tests/fixtures/protocols/opencode-acp-turn.json`。

### P3 工具桥与审批

复用共享地基 2.4 的宿主 MCP 桥；opencode 侧写 `opencode.json` 的 `mcp` 配置并注入令牌；权限应答与「永久允许」拒绝测试。

桥收件箱、观察产出（`Obs::ToolCall{request:{"kind":"bridge",…}}`，`tool` 按种类取值：集成/未绑定 → `qualified_name`，平台 → `call_name()`）、回执按 `request.kind` 路由、桥审批经 `PendingInteraction` 往返——**全部复用 mcode 计划 P3 的实现**，opencode 侧只提供差异配置；执行语义与其余三条线一致（共享地基 2.4.8）。验收须覆盖共享地基 2.4.9 的 8 条桥判据（往返、会话记录 + 治理记录可审计、越权拒绝、幂等、撤销、审批、脱敏、**平台工具的路由与无授权时的明确拒绝**）。

### P4 验收与文档

`native_acceptance.py` 的 opencode 套件、probe provider 选项、浏览器枚举 spec；文档更新（并记录「ACP 标准协议 + MIT + 发行二进制摘要」这一相对低风险的接入面）。

## 4. 验证

- 自动检查同 mcode 计划第 4 章。
- 真实运行（`./scripts/dev/dev.sh --data ./data-opencode --port 4320`）：
  1. 镜像准备与协议探针通过；API key provider 可用。
  2. 讨论轮次：流式输出、终稿、终态（Completed/Failed/Cancelled）。
  3. 工具事件入账；只读 Profile 下写入被拦截且原因明确（插件钩子生效）。
  4. 步骤工具（宿主 MCP 桥）往返 + 宿主回执，且该调用**出现在会话记录与治理记录中（可审计）**；同一 `call_id` 重放不重复执行。
  5. 取消轮次 → 停止模式正确、容器移除。
  6. 重启 Host 后 `session/load` 恢复成功，且恢复的是本会话的 opencode 会话库。
  7. 两个并行 opencode 会话互不共享配置/会话库（后台服务隔离验证）。

## 5. 备选路线：v2 HTTP/SSE API

若 ACP 路线在 P0 出现阻塞（例如必须依赖后台服务、或 ACP 事件形态不足），可切换到 opencode 的 v2 HTTP API：`opencode serve --hostname 127.0.0.1 --port <port>` 在容器内起服务，宿主通过**发布端口**（`docker create -p 127.0.0.1:<ephemeral>:<port>`）访问，用 `@opencode-ai/sdk` v2 的强类型接口驱动（`SessionCreate/Prompt/Messages/Abort`、`PermissionReply`、`EventSubscribe` SSE、`ConfigUpdate`）。

代价与影响（需在切换时重新评审）：

- 传输层要新增 HTTP + SSE 客户端（当前 `jsonl.rs` 只做 stdio JSONL），且要把容器端口发布纳入 `docker.rs` 的安全审查（仅绑定宿主 loopback）。
- 会话与容器生命周期的映射要重做（HTTP 会话在服务进程内，容器重启后会话库仍在 `/state`，需确认恢复语义）。
- 好处：协议是**有类型、有版本**的公开 API，风险低于 ACP 细节与 zcode 私有协议。

## 6. 不在本期范围

- 根编排（opencode 不获得 `orchestration`，与 DSH 一致）。
- Agent 提问：ACP `elicitation/create` 通道不作为一期验收项。
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
