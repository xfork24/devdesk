# 合同版本去硬编码 · 执行与验收

日期：2026-11-12。基线：P13 完成后工作区。验收：`cargo test --workspace --locked` 26 套件全绿；`npm --prefix web run build` 通过；注册表单元测试 4/4。

## 已实施（W01–W09 对照）

| 工作项 | 结果 |
|---|---|
| W01 合同注册表 | 新增 `crates/domain/src/contracts.rs`：12 个合同（Workflow/WorkflowInstance/AgentTask/OrchestrationTask/Spec/Standard/Proposal/PermissionProfile/ProjectConfiguration/WorkConfiguration/ExternalTool/TemplateManifest），`current()` 写入侧、`accepts()` 读取侧谓词、`renderer()` 渲染身份、`require_current()` 统一错误（含兼容窗口）。迁移全部 src 断言：domain/workflow.rs（4 处）、standards.rs、conversation.rs、integrations.rs、permission_profile.rs、core/governance.rs、core/configuration.rs、core/templates.rs、core/configured_workflows.rs（8 处）、core/configured_runtime.rs（9 处）、core/integrations.rs（2 处）。**src 中版本全等断言清零**（grep 验证） |
| W02 结构识别 | `adapters/source_views.rs` 任务打包匹配器改为结构指纹（`source.workflow_instance_id` + `inputs` + `input_sources` + `agent.id` + 实例存在），renderer 字符串不再作判据，restore 全等回验保留；orchestration policy 匹配器改 `controller_id` 判据 |
| W03 查询去版本化 | workflow_instance 三处 find-filter 去掉 schema 条件（kind + steps 结构 + decode 兜底）；两处列表 filter 走 `Contract::WorkflowInstance.accepts()` |
| W04 能力声明（部分） | governance 3 处 `["codex","dsh"]` 白名单 → `HARNESS_PROVIDERS` 枚举注册表（domain/runtime.rs `known_harness_provider`），host 状态端点 providers 数组同步枚举驱动。**追记**：P13 曾落地的 4 处门禁能力化（`dynamic-tools`）已按用户决定回滚为 `provider == "codex"` 原判断（仅 codex 可作为调度者），本计划 W04 的门禁部分随 p14 挂起 |
| W05 读取机制外置 | `domain/agent_task.rs` 新增 `OUTPUT_READ_QUALIFIED_NAME/CALL_NAME/PAGE_BYTES` 常量，任务合同渲染不再内嵌字面量 |
| W06 测试解耦（部分） | 新增 `scripts/py/check-defaults.py`（出厂领域对象集合完整性校验：manifest ↔ 文件 ↔ defaults 引用 ↔ 文件归属），入口 `scripts/check/check-defaults.sh` 接入 `check.sh`；删除 `factory_workflow_business_contracts…` 的 3 个十六进制 hash 常量断言（P13 的重锚方案按计划被取代） |
| W07 Web 去断言（部分） | WorkflowManager 导入校验改调后端 `query.workflow_requirements`（本地仅 JSON parse + 数组形状检查）；WorkflowTaskPreview 删除 `agent-task/v6` renderer 过滤（后端仅返回已校验合同） |
| W08 D1 执行 | 删除 `--normalize-plan` CLI、`normalize_plan_document` 及其 3 个测试；usage 文本同步 |
| W09 文档 | 新增 `docs/contracts.md`：数据合同（注册表）/ 实现身份 / 外部运行时锚 / 概念词汇四层登记 + 演进流程 |

## 决策项执行

- D1：删除 `--normalize-plan`（按计划推荐）✓
- D2：读取窗口机制已实现（`supported` 范围 + 窗口演练回归测试）；**本批所有合同窗口为单版本**（N..=N）——开窗到 N-1 需逐合同核实读取路径可结构重建，未盲开（行为保持与迁移前完全一致）
- D3：实现放 `scripts/py/check-defaults.py`、入口 `scripts/check/check-defaults.sh`（按 scripts/py 目录约定，2026-11-12 用户调整）并接入 `check.sh` ✓

## 未尽事项（后续批次）

1. **W06 余量**：`crates/adapters/tests/configured_workflows.rs`（约 2800 行、279 个断言）的"内联夹具 + 编译级校验"重构未做——按计划纪律（先建夹具层、逐段迁移、全程全量回归）需独立批次。
2. **W07 余量**：展示性过滤仍在前端（WorkflowManager `:183/:332` 当前草稿过滤、ProjectManager `:152-153`、StandardManager `:297`、AgentCatalog `:612`、PermissionResources 编辑器 schema 2）——正确做法是后端 `query.*` 返回可用性标记后移除，需 Playwright 双档回归，未在本批盲改。
3. `docker-worker/v1`（校验侧）与 `docker-worker/v2:{}`（生成侧）的 enforcement 常量统一未做。
4. `workflow.outputs.read` 的 qualified_name 在 `platform_tools.rs`/`controllers.rs` 的多处全等匹配仍为字面量（已登记 docs/contracts.md §2 说明）。

## 验收对照

- A01（演进演练）：以 `read_windows_cover_exactly_one_prior_version_when_opened` 常驻回归替代一次性 bump 演练（注册表窗口机制 + 不变量测试）
- A02/A03：结构识别与能力声明均已落地（A03 的 dsh 侧由 P13 `tool_integrations` 回归覆盖）
- A04：`check-defaults.py` 替代 hash 门禁；修改出厂 skill 正文不再需要改 Rust 测试（materialize 校验仍全量在 cargo test 内）
- A06：`grep -rnE 'schema_version.*== *[0-9]|== *"agent-task/v|== *"orchestration-task/v' crates/*/src web/src` 零命中（renderer 定义仅存 contracts.rs）
- A07：26 套件全绿；`check.sh` 全量通过（Playwright debug 50/50 + release 50/50、HTTP 合同、双档签名构建），见配置精简计划执行报告 §5


## 追记二（2026-11-12 · W01 按用户方向修正重做：注册表 → 结构识别）

用户审阅 `contracts.rs` 后指出：合同注册表仍是版本思维，未消除"数据演进需要动代码"的根本问题。正确方向是**代码只定义原语与领域对象，数据合法性由结构识别决定**，使 workflow 可完全独立演化（将来由项目管理 agent 定义与改进）。重做内容：

- **删除** `crates/domain/src/contracts.rs`（Contract 枚举、current/supported 窗口、require_current、4 个注册表单测）及 lib.rs 接线；D2"读取窗口"决策随之作废。
- 迁移为结构识别（src 中版本断言仍为零）：
  - 领域校验去版本门禁：Workflow/Spec/Standard/Proposal/PermissionProfile/ExternalTool 的 validate 不再比较 `schema_version`，保留业务规则（DAG/端口/非空正文/引用）。
  - 类型分派按结构字段：外部工具 = `integration_ref` 存在（core/configuration.rs、configured_workflows.rs、core/integrations.rs 共 5 处）；Spec = decode `ConfiguredSpecification`；草稿当前性 = 文档可 parse 为 `WorkflowDefinition`（`parses` 闭包）。
  - 存储与合同自洽：workflow 实例判定 = `steps` 数组存在（含完整性检查与全部查询过滤）；`ConfiguredAgentTask::validate` 改为记录自身一致性（外层/内层 renderer 一致 + 摘要 + 来源）。
  - `manifest` 不再断言协议版本（Manifest 结构反序列化即边界）；项目配置过时检查移除显式版本门禁（下游 decode 兜底）。
  - 元数据写入常量：`WORKFLOW_INSTANCE_SCHEMA`（domain/workflow.rs）、`AGENT_TASK_RENDERER`（domain/agent_task.rs）、`ORCHESTRATION_RENDERER`（domain/orchestration.rs 恢复）——只写不读。
- `docs/contracts.md` 重写为"原语、领域对象与结构识别"（领域对象/原语/结构规则五条/实现身份/外部锚/出厂自洽）。
- 验证：`cargo test --workspace --locked` 26 套件全绿；`grep` 确认无 `Contract::` 残留、无版本全等断言、`schema_version` 仅剩测试夹具/字段透传/元数据写入。
