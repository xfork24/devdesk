# pi Harness 接入（pi coding agent 1.6.0）

日期：2026-11-25。状态：**待实施**。范围：接入 pi 1.6.0（`@earendil-works/pi-coding-agent`，MIT）为第三个 Harness，能力对齐 DSH：四家模型服务商、讨论会话、workflow 步骤工具。不含根编排。

## 0. 已确认决策

| 决策 | 结论 |
|---|---|
| 工具桥 | 纳入。容器内 DevDesk 扩展经 pi `extension_ui_request` 对话框通道（`input`）把 Host 代执行工具转发给 Host，Host 回执经 `extension_ui_response` 返回 |
| 全局库升级 | 新增显式离线命令 `devdesk-host storage upgrade-global --data PATH`；**旧数据集的升级改造属于本计划范围**，验收阶段对现有数据集实际执行 |
| 模型服务商 | 智谱编程订阅、MiniMax 订阅、DeepSeek 官方 API、本机 Ollama 全部接入 |
| 根编排 | pi 不获得 `orchestration` 能力（与 DSH 一致） |
| Agent 提问 | 首期不支持（pi 无原生能力，RPC 模式无多选对话框） |

## 1. 调研结论（已核实来源）

### 1.1 pi 1.6.0

- 无头接口 `pi --mode rpc`：JSON Lines，无握手，常驻；必须显式 `--mode rpc`（否则非 TTY 落入 print 模式读空 stdin）。stdout 仅协议，stderr 仅诊断。
- 入站 33 个命令（`prompt`/`steer`/`abort`/`get_state`/`set_model`/`set_thinking_level`/`bash`/`get_session_stats`…），应答 `{id,type:"response",command,success,data|error}`，乱序。
- 出站事件：`agent_start`/`turn_start`/`message_start|update|end`/`tool_execution_start|update|end`/`turn_end`/`agent_end`/`agent_settled`；`message_update` 仅增量，`message_end` 为权威全文；`agent_settled{aborted}` 是最终静止信号，失败需看末条消息 `stopReason`（`error|aborted|length`）。
- pi→客户端请求只有 `extension_ui_request` 一族（`select|confirm|input|editor` 阻塞等待应答 + 若干 fire-and-forget）。
- npm 包 `@earendil-works/pi-coding-agent@1.6.0`，Node ≥ 22.19（沙箱基础镜像 Node 24.13 满足）。
- 扩展（`.js`，jiti 加载，显式 `-e` 在 `-ne` 下仍生效）：`pi.on("tool_call")` 可拦截/阻断；`pi.registerTool` 注册工具；工具 `execute(..., ctx)` 里可 `await ctx.ui.input(...)` 并收到 `extension_ui_response`。
- 权限/审批无内置；`grep|find` 依赖 ripgrep/fd（首期不开）；工具需 `--tools` 白名单。
- 配置：`PI_CODING_AGENT_DIR` 指向 `/state/pi`（`settings.json`、`models.json`、会话目录）；`--session-id` 按 cwd 创建或打开会话；恢复会话会还原记录的模型并可能静默回退（需 `get_state` + `set_model` 复核）。
- 智谱服务商端点/容量与 devdesk dsh 目录一致，API key 环境变量名相同；MiniMax 内置默认端点 `api.minimaxi.com`，devdesk 固定 `api.minimax.cn`，需在 `models.json` 覆盖 `baseUrl`。
- **注意**：以上来自本机安装包源码与文档（`~/.pi/agent/install/releases/1.6.0/node_modules/@earendil-works/pi-coding-agent/`），P0 实跑验证前不视为已证实的运行行为。

### 1.2 DevDesk 现状

- Harness = 编译期驱动（`HarnessProvider` 枚举，`crates/domain/src/runtime.rs:9`）+ 声明式配置（`plugins/core/harnesses/*.json`，`data.provider`）。
- 适配器单文件协议分支：`crates/adapters/src/harness.rs`（2325 行）+ `jsonl.rs`（传输）+ `permissions.rs`/`permission_profiles.rs`（权限编译）。
- 镜像配方：`compose_execution_recipe`（`crates/domain/src/execution_recipe.rs`）从 npm 包生成 Dockerfile；pi 可直接复用，codex/dsh 配方不变。
- **一次性影响（系统既定设计）**：权限语义摘要覆盖 `permissions/backends/manifest.json` 全文与全部资产摘要，新增 pi 后端/资产后 codex/dsh 的 `enforcement_contract` 漂移、冻结会话 `permission_asset_mismatch`；全局库 `runtime_installations` CHECK 需重建表。
- 现数据集：`data/`（global 24，3 条运行安装）、`data1/`（验收前先核对可读性）、`devdesk-recovery/20261119-large-snapshot/`（快照）。`dist/data` 为构建产物目录，可重新初始化。

## 2. 方案设计

### 2.1 运行形态

- Harness 源 `plugins/core/harnesses/builtin-harness-pi-managed-v1.json`：`provider: pi`、`runtime_version: 1.6.0`、`package: @earendil-works/pi-coding-agent`，环境变量 `PI_CODING_AGENT_DIR=/state/pi`、`PI_OFFLINE=1`、`PI_TELEMETRY=0`、`PI_SKIP_VERSION_CHECK=1`。策略脚本只放 `permissions/backends/pi/` 一份（DSH 目前在 Harness JSON 和 backends 各有一份且内容不同）。
- 容器命令：`pi --mode rpc -ne -e /run/devdesk/tool-policy.js --no-skills --no-prompt-templates --no-themes --no-context-files --no-approve --no-mcp --session-id <id> --provider <route> --model <model> --append-system-prompt /run/devdesk/profile`。进程 cwd 固定 `/work`（`--session-id` 按 cwd 查找会话，必须稳定）。
- Host 打开会话时写 `/state/pi/models.json` 与 `settings.json`（`cacheWarming: off`、关闭遥测、`defaultProjectTrust: never`）。API key 沿用 DSH 的 `provider.env` + `--env-file`，创建容器后删除。
- 会话身份：新会话用 DevDesk 的 session id 作 `--session-id`；恢复时传 `external_session`，启动后 `get_state` 核对 `sessionId`，再 `set_model` 重新确认冻结模型。

### 2.2 协议与事件映射（`Obs`）

| pi 记录 | 观察 |
|---|---|
| `prompt` 响应 `disposition: started` | `Admitted{external_turn: Host 生成的 id}`（pi 不签发轮次 id，此身份只用于把工具调用绑定到已接纳的轮次） |
| `disposition: handled` | `provider_rejected`，不计为已应用 |
| `message_end` (user) | `Consumed{message: None}` |
| `message_update` + `text_delta` | Assistant `Delta`，复用 Codex 的合批与尾部脱敏；`thinking_delta` 丢弃（与 Codex 不展示 reasoning 一致） |
| `message_end` (assistant) | Assistant `Full`（权威正文） |
| `tool_execution_start` / `tool_execution_end` | Tool `Full`，正文为事件 JSON（同 DSH） |
| `agent_settled` | `Terminal`：`aborted` → Cancelled；末条 `stopReason` 为 `error`/`length` → Failed；否则 Completed |

### 2.3 工具桥与审批

- 扩展从 `/run/devdesk/dynamic-tools.json` 读取工具定义并 `pi.registerTool`；`execute` 内用 `ctx.ui.input` 发出带 `devdesk.tool_call.v1` 前缀的请求，Host 转成 `Obs::ToolCall`，`answer_tool` 回 `extension_ui_response{value: <ToolReceipt JSON>}`（载荷与 DSH 分支相同）。
- 审批：`approval !== never` 时，`tool_call` 钩子对 bash/edit/write 用 `ctx.ui.select` 发 `devdesk.approval.v1` 请求 → `Obs::Interaction{ToolApproval}`。
- `tool_call` 钩子同时执行冻结策略（读 `/run/devdesk/permission`），与 DSH 的 `tool-policy.mjs` 同构；桥接工具放行，由 Host 按冻结 Tool 引用核验。
- 无前缀或跨轮次的对话框一律回 `cancelled: true`。

### 2.4 权限

- `capabilities(Pi)`：filesystem 三种；network 仅 `enabled`（pi 没有工具禁网执行器）；tools `all_available` + `restricted`；`limitations` 如实列出（无原生权限模式、不支持 Agent 提问、审批为每次写操作询问、cwd 固定）。
- `validate_execution_permissions` 的 pi 分支：拒绝 `Disabled` 网络；scoped 规则沿用 DSH 的「必须由冻结挂载落实」校验（抽成共用函数）。
- 执行合同按实际执行者命名：后端 `pi-extension-v1`，`harness_backend`/`rules` 不用 `native` 字样。
- `protected_metadata` 加入 `.pi`。

### 2.5 模型与凭据

- 服务商公开 id 必须全局唯一（`validate_model_provider`，`crates/core/src/configuration.rs`），所以新建 `pi-zai-coding-cn`、`pi-minimax-cn`、`pi-deepseek`、`pi-ollama`，`route` 仍用 pi 的服务商名；新建对应 Model 源（GLM 5.3、MiniMax M3.1-Flash）和 `builtin-model-catalog-pi-v1.json`。现有 dsh 的 Model 源不动。
- MiniMax 需在 `models.json` 覆盖 `baseUrl` 为 `api.minimax.cn/anthropic`。
- 凭据链路从写死的 `dsh` 泛化为「API key Harness」：绑定键 `credentials.{harness}.{provider}`（dsh 现有键不变），macOS 钥匙串账户前缀白名单加入 pi 的三个前缀。

### 2.6 数据与契约

- `sql/global/schema.sql`：CHECK 加入 `'pi'`，`user_version=25`。
- `schemas/rpc.schema.json` 六处 provider 枚举、`schemas/permission-backend.schema.json` 两处。

## 3. 工作项

### P0 可行性验证（先于其余改动）

1. 用生成的 Dockerfile 手工构建 pi 镜像：确认 `npm install --global --omit=dev` 能干净安装，`pi --version`、`pi --list-models` 输出符合预期，容器内 `get_state` 往返成功。合成器是否需要 `--ignore-scripts` 以实测为准。
2. 用本机 pi 1.6.0 跑一个最小扩展，确认工具 `execute` 里的 `ctx.ui.input` 确实发出 `extension_ui_request` 并能被应答。
3. 本计划文件与 `docs/plans/README.md` 登记（已完成）。

### P1 注册、配置、权限、凭据与界面入口

结果：pi 可被选择，`runtime.prepare` 能构建镜像并通过协议探针。

1. `crates/domain/src/runtime.rs`：`HarnessProvider::Pi`、`HARNESS_PROVIDERS`、`as_str`。
2. 把「非 Codex 即 DSH」的隐式分支改成显式判断：`harness.rs`（模型连接、stop 模式名、应答形状）、`core/src/session_assembly.rs`、`domain/src/conversation.rs`、`core/src/credential_updates.rs`、`adapters/src/credentials.rs`。`conversation_observations.rs` 与 `provider_tools.rs` 的 `== Dsh → external_message` 不动（pi 走 `external_turn` 分支是对的）。
3. `crates/adapters/src/jsonl.rs`：新增 pi 线协议模式（发送 `{"id","type",…}`，按 `type=="response"` 匹配应答，取 `data`，`success:false` 转错误）；日志对 pi 的 `message_update` 增量按 Codex 规则省略并做尾部脱敏。
4. `crates/adapters/src/permissions.rs`、`permission_profiles.rs`：pi 能力、编译分支、校验分支、`asset_digests()`、语义摘要。
5. 权限资产：`permissions/backends/pi/tool-policy.js`（P1 先只做策略拦截）、`manifest.json` 新增后端与 sha256、`permission-backend.schema.json`。
6. `crates/adapters/src/docker.rs`：按 provider 选择内嵌策略（pi 挂到 `/run/devdesk/tool-policy.js`，codex/dsh 行为不变）；`ContainerSpec` 新增 `bridge_file` → `/run/devdesk/dynamic-tools.json`。
7. `runtime_verification.rs`（pi 探针：`get_state`，无密钥无网络）、`runtime_registry.rs`、`runtime_preparation.rs`（`default_pi_recipe_digest`）、`session_resources.rs`（独立准备锁、凭据分支）、`examples/export_runtime_recipes.rs`（改用 `HARNESS_PROVIDERS`）、`lib.rs` 导出。
8. `harness.rs` 此阶段只加让枚举穷尽的占位分支（`managed_adapter_contract` 为 `pi-rpc/v1`、`provider!(PiHarness, Pi)`、`RoutingHarness` 路由，命令分支返回 `unsupported`），P2 替换。
9. 出厂配置：Harness/服务商/Model/目录/原生工具源，登记 `plugins/core/manifest.json`，用 `scripts/check/configuration_usage.py --write` 重生成 `reference-usage.json`；目录的 `recipe_digest` 取自 `export_runtime_recipes` 输出。`tests/fixtures/configuration/engineering-catalog.json` 同步加入同样的源（`schema_contracts` 会遍历全部 provider）。
10. 凭据泛化：`core/src/credential_updates.rs`、`host/src/model_providers.rs`、`adapters/src/{credentials,catalog,credentials_macos}.rs`。
11. 全局库：schema、`catalog.rs` 版本检查、升级命令（`host/src/main.rs`）。
12. Schema 枚举更新。
13. 前端：`RuntimeSettings.tsx`（类型与下拉）、`AgentCatalog.tsx`、`RuntimePreparationStatus.tsx`、`Integrations.tsx`、`RoleConversation.tsx`、`App.tsx`（凭据键前缀）；`DshProviderSettings.tsx` 改为按 harness 渲染，供 dsh 与 pi 共用。
14. 测试支撑：`crates/adapters/tests/support/mod.rs`（`DEVDESK_TEST_PI_IMAGE`）、`tests/support/runtime.rs`、`native_acceptance.py`、`doctor.py`。

### P2 协议驱动

结果：pi 会话端到端可用（讨论轮次、恢复、停止）。

1. 新文件 `crates/adapters/src/harness_pi.rs`（在 `harness.rs` 里按 `harness_startup.rs` 的方式挂模块）：启动输入、事件投影、停止模式。`harness.rs` 的 command/init/dispatch/project/stop/answer 换成委托给它的 `Pi` 分支；`managed_adapter_identity` 纳入新文件与 pi 资产。复用 `provider_turn_text`、`prompt_snapshot`、`write_private`。
2. `crates/adapters/src/model_providers.rs`：新增 `pi_model_patch`（生成 `models.json`，参照 `dsh_model_patch`），`binding_model_connection` 放宽到 Dsh|Pi；`session_resources.rs` 与 `source_views.rs` 的 `model_profile_digest` 按 provider 选 patch。
3. `permissions/backends/pi/tool-policy.js`：`tool_call` 策略拦截。
4. 测试：`harness.rs::protocol_tests` 的 pi 用例（接纳、消费、增量与终稿、工具事件、三种终态、`handled` 拒绝）配合新增脱敏样本 `tests/fixtures/protocols/pi-turn.json`；`jsonl.rs` 线协议与拆分密钥脱敏；`permissions.rs`；`model_providers.rs`。遍历 provider 的测试改用 `HARNESS_PROVIDERS`（`harness.rs`、`permission_profiles.rs`、`tests/{permissions,schema_contracts,permission_profiles}.rs`）。

### P3 工具桥与审批

1. 扩展：注册动态工具、`input` 隧道、`select` 审批。
2. Rust：`project` 识别隧道请求；`answer_tool`、`answer` 的 pi 形状；`approval == Never` 时自动拒绝；打开会话时用现有的 `dynamic_tools(binding)` 写出桥接文件。
3. 测试：隧道往返、无前缀对话框取消、审批自动拒绝；真实镜像上的 `#[ignore]` Docker 用例（被策略拦截的工具、一次桥接调用）。

### P4 验收与文档

1. `native_acceptance.py` 增加 pi 套件；`tests/live/probe_*.py` 的 provider 选项；浏览器用例里枚举 Harness 选项的 spec。
2. 文档：`docs/design/agent-runtime.md`、`docs/design/reference/integrations.md`、`docs/crates/devdesk-adapters/{harness-and-protocols,runtime-preparation,credentials-and-permissions}.md`、`permissions/README.md`、`sql/README.md`、`AGENT.md`（global 25）、`docs/design/run-binary.md`、`tests/README.md`；`check-docs.py --write` 刷新文件索引。本计划取代 dsh-model-providers 计划里「不引入 Pi CLI」的决定，历史计划原文不改。

## 4. 旧数据集升级（本计划范围）

1. 新增离线命令 `devdesk-host storage upgrade-global --data PATH`：核对宿主进程已停止后重建 `runtime_installations`（CHECK 加入 `'pi'`），`user_version` 24→25，保留全部既有行（含 `data/` 现有 3 条安装）。启动版本检查同步接受 0|24|25（`catalog.rs:41`）。
2. `tests/entrypoints/dataset_protocol.py` 扩展覆盖 v24→v25 升级路径。
3. 升级后 codex/dsh 权限绑定合同漂移：通过设置页重新应用绑定（RPC `permission.binding.preflight` + `permission.bindings.apply`；`permission.bind` 已退役）。既有冻结会话需创建后继会话。
4. 验收阶段对 `data/`、`data1/` 实际执行升级并保留原库备份；`devdesk-recovery` 快照按需另行执行同一命令。

## 5. 验证

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

真实运行：`./scripts/dev/dev.sh --data ./data-pi --port 4318`（新空数据目录）；升级验证在现有数据集（或其副本）上执行。

1. 设置里准备 Pi 镜像，确认协议探针通过；为 pi 保存智谱或 MiniMax 的 API key。
2. 新建 Agent，选 Pi 与 GLM 5.3，开讨论发消息：确认流式输出、最终正文和终态。
3. 让 Agent 读文件、改文件，确认工具事件入账；只读 Profile 下确认写入被拦截且原因明确。
4. 跑一个带步骤工具的 workflow 步骤，确认桥接调用往返；取消运行中的轮次，确认停止模式与容器移除。
5. 重启 Host 后继续该会话，确认 `sessionId` 与模型被正确恢复。
6. 对旧数据集执行 `storage upgrade-global`，重新应用 codex/dsh 权限绑定，codex 与 dsh 各跑一轮。

## 6. 不在本期范围

- 根编排（pi 不获得 `orchestration`，与 DSH 一致）。
- Agent 提问：pi 无原生能力，RPC 模式也不支持多选对话框。
- 工具禁网；pi 的 `grep`/`find`/`ls` 工具（需要 ripgrep/fd，会改动共享沙箱配方）；思考等级参数。

## 7. 风险与兼容

- **pi 行为尚未实跑**：协议细节来自源码阅读，P0 先验证；若 P0 第 2 项不成立，P3 需要改方案。
- **工具桥借用 UI 通道**：属于有节制的用法变通。pi 版本固定在镜像里不会意外失效，升级 pi 时需重新验证隧道。
- **`harness.rs` 继续膨胀**：pi 逻辑放独立模块；该文件任何改动都会改变全部 provider 的构建摘要（既有行为）。
