# P14 · DSH 工具执行桥：让 dsh harness 获得编排能力

日期：2026-11-12。状态：**提案（未实施）**。来源：用户需求——"修改代码以支持 dsh harness 来进行编排"；P13 已完成门禁能力化（`dynamic-tools` 能力标记）与 dsh 栈配置，本计划补齐缺失的工具执行桥本体。

---

## 0. 调研结论概览

- **引擎侧已是 provider 无关**：`Obs::ToolCall` 投影（`crates/core/src/integrations.rs:186`）、平台工具分发/回执/应答（`platform_tools.rs`）、controller 调用执行与回执（`controllers.rs:427-530, 600-799`）均不检查 provider。平台工具权限执行（`platform_tools.rs:234-268`、`controllers.rs:473-481`）作用于冻结的 `ExecutionPermissions`，同样与厂商无关。
- **缺失在 DSH SDK 协议**（0.2.2-rc.1，`/Users/hengliu/workspace/deepseek-harness/packages/sdk/protocol/src/types.ts`）：
  - `InitializeParams`（types.ts:18-33）无 tools 字段；`SessionPromptParams`（:71-76）仅 sessionId+contentBlocks；
  - server→client 请求只有 `approval/request`（:240-242）与 `user-question/ask`（:60-62），**无工具执行请求**；
  - devdesk 侧因此从不给 DSH 传工具 schema（`crates/adapters/src/harness.rs:662` 初始化仅 cwd/provider/model/maxTokens/两个 capability），DSH 工具面只来自容器内 profile 插件（`tool/call`、`tool/result` 仅作单向通知投影为 Obs::Message，`harness.rs:1093-1107`，DSH 从不产生 `Obs::ToolCall`）。
- **codex 桥的形态**（对齐目标）：`thread/start`/`thread/resume` 传 `dynamicTools` schema（`harness.rs:627-634`，`workflow.start` 重写为 `devdesk_workflow_start`）；模型调用经 JSON-RPC 请求 `item/tool/call` 被拦截（`harness.rs:916-939`，仅 Codex 分支）→ 平台执行 → `answer_tool` 以 codex 形状回包（`harness.rs:1239-1260`）。
- **传输层已支持 host 应答请求**：`approval/request`/`user-question/ask` 已有注册与应答先例（`harness.rs:1134-1238, 1261-1320`），缺的不是传输而是 SDK 侧工具定义/转发协议。
- **插件 API 可注册进程内工具**：`deepseek-harness/packages/experimental/tool-worktree`（`ctx.tools.register(defineTool(...))`），是无需改 wire 协议的扩展点。

## 1. 方案设计

### 方案 A · 上游 SDK 协议扩展（对齐 codex，推荐）

1. **deepseek-harness 侧**（跨仓库）：
   - `InitializeParams` 增加 `tools`（JSON Schema 数组）或新增 `session/tools` 请求按会话注册；
   - 新增 server→client 工具执行请求（如 `tools/execute`，形状对齐 `approval/request` 的请求/应答模式）；
   - 发布新版（如 `0.2.2` 或 `0.3.0`）。
2. **devdesk 侧**：
   - `crates/domain/src/defaults.rs` `DEFAULT_DSH_VERSION` 升级；`dsh-model-catalog.json` 的 `runtime_version` 同步（`model_provider.rs:103-106` 有全等断言）；
   - dsh 容器镜像（`containers/dsh/Dockerfile`）与冻结 recipe digest 重建（`runtime_setup.rs:111-123` 拒绝静默换版，需显式推进）；
   - `harness.rs`：初始化/会话注册时序列化平台+外部工具 schema（复用 `ToolDefinition::call_name()`）；在 `harness.rs:916` 旁增加 DSH 分支拦截工具执行请求 → `Obs::ToolCall`；`answer_tool` 增加 DSH 形状应答；
   - `harnesses/builtin-harness-dsh-multi-provider` 升修订声明 `dynamic-tools` 能力；dsh PM capability 升修订补 21 个控制工具（P13 已铺好的开通路径）。
3. **权限侧**：`devdesk-tool-boundary` 插件按工具名过滤 `tools/execute`，桥接工具需放行；`capabilities(dsh)` 若变动则 `permission_semantics_digest` 变化，冻结会话需重新冻结（既有语义）。

### 方案 B · 容器内插件 + 旁路通道（不动上游）

- 新增 dsh 插件资产（模式同 `permissions/backends/dsh/tool-policy.mjs`，登记进 manifest + `asset_digests`）：经 profile.yaml 注入工具 schema，`ctx.tools.register` 注册平台工具，经挂载的 FIFO/socket 转发执行到 host；
- adapter `Worker` 起旁路服务应答，审计/日志需对齐 `JsonlProcess` journal 语义；
- 全部资产哈希进 `managed_adapter_identity`（`harness.rs:14-57`），适配器身份变化使既有冻结会话失效（by design）。
- 优点：不依赖上游发版；缺点：引入第二信任通道，需复制 JSONL 审计保障，长期维护成本高于方案 A。

## 2. 工作项（方案 A）

| 编号 | 工作项 | 涉及 |
|---|---|---|
| W01 | SDK 协议扩展（tools 注册 + `tools/execute` 请求）与新版本发布 | deepseek-harness（跨仓库） |
| W02 | `DEFAULT_DSH_VERSION`/catalog runtime_version/Dockerfile/recipe digest 联动升级 | domain、adapters、containers/dsh |
| W03 | harness.rs DSH 工具拦截分支 + schema 传递 + DSH 形状应答 | crates/adapters/src/harness.rs |
| W04 | dsh harness 修订声明 `dynamic-tools`；dsh PM capability 修订补控制工具；权限边界放行桥接工具 | resources/configuration、permissions/backends/dsh |
| W05 | 回归：dsh 栈端到端编排验收（controller 绑定 dsh PM → workflow 全流程） | scripts/check/check-live.sh |

## 3. 风险与兼容

- 冻结配方换版不可静默：`runtime_setup.rs` 会拒绝不一致的 harness 定义，需按流程显式推进；所有冻结会话随适配器身份/digest 变化失效（系统设计语义）。
- 上游协议演进需与 deepseek-harness 发布节奏协同（版本常量、目录断言 `model_provider.rs:104` 同步）。
- controller 的取消语义差异：dsh 无 per-session cancel（stop 杀容器，`harness.rs:751-753`），编排长回合的取消体验需验收确认。

## 4. 依赖与顺序

依赖 P13（门禁能力化 + dsh 栈配置已就位，`dynamic-tools` 开关只剩 dsh harness 修订声明）。建议在 P13 合并后启动；W01 是关键路径（跨仓库发版）。

## 5. 上游现状核实（2026-11-12）

- `/Users/hengliu/workspace/deepseek-harness` @ master（HEAD 5aeed699a0），`package.json` version `0.2.2-rc.1`，工作区无本地修改——与 devdesk 冻结版本一致。
- `packages/sdk/protocol/src/types.ts` 的 `InitializeParams` 仅有 `cwd/approvalRequests/provider/model/reasoningEffort/maxTokens/capabilities.userQuestions`，确认无 tools 注册入口；server→client 请求仅 `approval/request`、`user-question/ask`。
- 实施前置：SDK 协议扩展属上游设计决策（tools 挂 InitializeParams 还是 per-session、执行请求形状），需要在该仓库内按其测试体系（vitest）单独开发与发版；devdesk 侧 W02-W05 均依赖 W01 的新版本号，无法先行独立落地。

## 6. 状态（2026-11-12 用户决定）

用户决定**暂不做 DSH tools 注册入口，维持仅 codex 可作为调度者的现状**。P13 曾落地的"门禁能力化"（`dynamic-tools` 标记 + `supports_dynamic_tools()` + codex harness rev2 级联）已整体回滚，4 处门禁恢复 `provider == "codex"` 字符串判断。因此本计划 W04 需重新包含门禁能力化（或届时直接沿用 p12 方案 §3.3 的能力字段设计）；P13 保留的 dsh 栈配置（含 PM 普通 agent 形态）不变，仍可作为本计划的开通基础。本计划整体挂起，等上游发版意愿恢复时重启。
