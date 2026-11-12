# P12 · 合同版本去硬编码：让代码回到原语，合同回到数据

日期：2026-11-12。状态：**部分实施（2026-11-12；W01 按 2026-11-12 用户方向修正重做为结构识别，余量见 EXECUTION.md）**。来源：用户架构原则——"代码提供和实现原语（合并分支、提交代码、调用 Agent 等），不应针对某个 contract 的版本硬编码"；`review/docs/06-problems.md` P10 的深化。
范围：全仓库合同版本硬编码的调研结论与修正方案。**仅计划，不实施。**

---

## 0. 调研结论概览

对 crates/、web/src、resources、测试层做了六类系统排查（合同版本断言 / 供应商耦合 / 结构耦合 / 测试基线 / Web 复制 / 合理锚点）。两个关键事实：

- **好消息**：引擎内核（domain/core/adapters 的 src）**没有**对默认流程结构的任何特殊处理——TDD 门禁、独立评审、ActionPermit 全部由数据（completion 规则、action_requirements、step_grants）驱动；src 中也没有引用默认目录身份（`builtin-workflow-*`/`sol-high` 仅出现在测试与资源中）。"引擎 = 原语"这一层目前是成立的。
- **坏消息**：**数据合同版本**被以 `== N` 全等断言的方式硬编码在 **30+ 处**（domain 校验、core 查询过滤、pack/restore 形状匹配、web 前端、CLI、测试基线），另有**供应商能力以字符串比较**（`provider == "codex"`）而非能力声明实现。每次合同演进（如 v6→v7）需要同步修改约 5 个 crate 的十几个文件；出厂配置的内容哈希被硬编码进 Rust 测试，配置事实上成为"需要重编译的代码"。

严重程度：**高**（演进税 + 双重维护 + 潜在 bug 面），且与项目"配置即数据"的自我定位直接矛盾。

---

## 1. 问题清单（调研实测，file:line 为证）

### 1.1 数据合同 schema 版本的全等断言【严重度：高】

同一模式反复出现：`schema_version == N` / `renderer_version == "…/vN"`，无版本协商、无兼容窗口。

| 合同 | 断言位置（src，非测试） |
|---|---|
| Workflow v6 | `domain/workflow.rs:6`（`WORKFLOW_SCHEMA_VERSION=6`）、`:495`（validate 全等）、`core/configured_workflows.rs:285,324`（"草稿需要 Workflow v6"） |
| workflow_instance 存储形态 v6 | `core/configured_runtime.rs:845,957,1049,1463,1861`（查询过滤 `r.data["schema_version"]==6`，共 5 处）、`domain/workflow.rs:937` |
| 任务渲染 `agent-task/v6` | `core/configured_runtime.rs:565`（渲染时**写死**该字符串）、`:1101,1145`（三方一致校验）、`domain/workflow.rs:778`、**`adapters/source_views.rs:129`（pack 形状匹配器按字符串全等识别任务）**、`web/WorkflowTaskPreview.tsx:11` |
| 编排任务 `orchestration-task/v2` | `domain/orchestration.rs:5`（常量）+ `core/controllers.rs` 唤醒组装 |
| Spec v3 | `domain/workflow.rs:305`、`core/configured_workflows.rs:790`、`core/configured_runtime.rs:482` |
| Standard v4 | `domain/standards.rs:16` |
| Proposal v2 | `domain/conversation.rs:793` |
| PermissionProfile v2 | `domain/permission_profile.rs:83` |
| project_configuration v3 / work_configuration v4 | `core/configured_workflows.rs:100`、`core/governance.rs:344`（"项目配置已过时"）、`core/configured_runtime.rs:252`（写死 `schema_version:4`） |
| 外部工具定义 v2 | `domain/integrations.rs:343`、`core/configured_workflows.rs:830,866`、`core/integrations.rs:141,724`、`core/configuration.rs:96`（5 处过滤 + 校验） |
| 模板 manifest v2 | `core/templates.rs:47` |
| `--normalize-plan` | `host/main.rs`（目标版本取 `WORKFLOW_SCHEMA_VERSION` 常量） |

最脆弱点：**`source_views.rs:129` 的持久化 pack 匹配器**——它靠 `renderer_version == "agent-task/v6"` 字符串全等来识别"这是任务形状、可压缩为源引用"。渲染器升级后旧记录仍能 restore（restore 走结构），但新记录会静默落入泛化打包路径，压缩率与一致性依赖巧合而非设计。

### 1.2 供应商/后端能力用字符串比较，而非能力声明【严重度：高】

| 硬编码 | 位置 | 语义 |
|---|---|---|
| `harness.provider == "codex"` | `core/controllers.rs:214`（编排必须 codex）、`core/configuration.rs:100,105`（外部工具/平台工具桥必须 codex）、`core/configured_workflows.rs:199` | "谁能做某事"写死为厂商名 |
| `["codex","dsh"].contains(...)` | `core/governance.rs:498,505,515` | 供应商白名单 |
| `sandbox.enforcement == "docker-worker/v1"` | `core/governance.rs:415`、`adapters/permission_profiles.rs:41` | **同一文件内 v1 比较（:41）与 v2 生成（:46 `docker-worker/v2:{}`）并存**——版本字符串不一致的现成隐患 |
| `runtime_version == DEFAULT_DSH_VERSION` | `domain/model_provider.rs:104` | 内嵌目录与版本强绑（新 DSH 版本必须改代码换目录） |

新增一个 Harness 供应商或一个支持外部工具的执行器，当前需要修改 core 的多处字符串比较——这正是"原语缺失"。

### 1.3 机制细节被冻结进任务合同【严重度：中】

- `core/configured_runtime.rs:532-539`：大产物引用渲染把 `read_tool: "devdesk_workflow_outputs_read"`、`arguments{max_bytes:32768}` 等具体工具调用参数写死在 `agent-task` 合同正文中——"如何读取产物"这一机制成为合同版本的一部分；`workflow.outputs.read` 的 qualified_name 在 core 三处全等匹配（`controllers.rs:9,34,50`、`platform_tools.rs:9,249,604,663`）。
- `core/configured_runtime.rs:494-500`：`approve/summary/checks` 报告结构与 `check_report_contract` 指令文本在渲染器中拼装（这是 Spec 检查的合同，属于合理原语，但同样应登记为可版本化合同而非散落字符串）。

### 1.4 测试把出厂配置内容当代码基线【严重度：中】

- `crates/adapters/tests/configuration_resources.rs:230-249`：默认 Workflow 与 PermissionContext 的 `configuration_hash` 以 3 个 64 位十六进制常量硬编码比对——**改默认目录任何字节都必须同步改 Rust 测试**。
- `crates/adapters/tests/configured_workflows.rs`（279 个 assert）：大量对默认流程图结构（步骤键、TDD 链、端口）的直接断言；3 处引用 `builtin-workflow-*-sol-high` 身份。
- `crates/host/src` 内联测试引用 `DEFAULT_HARNESS_ID`、`builtin-harness-dsh-multi-provider:1` 等（同为配置身份耦合）。

### 1.5 Web 前端复制版本断言【严重度：中】

- `WorkflowManager.tsx`：schema 6 断言 5 处（含导入校验 `:434`）、工具 schema 2 过滤 `:923`。
- `WorkflowTaskPreview.tsx:11`：`renderer_version === "agent-task/v6"` 过滤。
- `PermissionResources.tsx:40,188`：profile schema 2。
- 后端本已有完整校验入口（`query.workflow_validation/requirements`），前端断言是纯粹的重复维护面。

### 1.6 合理锚点（应保留，明确边界即可）

- **外部运行时版本**：`DEFAULT_DSH_VERSION`/`DEFAULT_CODEX_VERSION`、recipe digest、每平台 SHA256——锚定真实外部供应商，属必要事实。
- **协议/存储边界**：SQL `user_version`、数据集标记、JSON-RPC 版本、适配器内容寻址身份（`managed_adapter_identity`、`github-pr/v1:indirect-merge-only`、`permission-compiler/v2`、`system-isolated-tests-v2`）——这些是**实现/协议身份**而非数据合同版本；命名中的 `vN` 易与数据合同混淆，需登记分层，不建议改语义。
- **概念词汇**：17 种定义 kind、StepExecutor 四类、CompletionRule 算子、`action_requirements` 类别（`"tests"`/`"confirmation"`）、21 控制工具与 31 平台处理器名（协议注册表）——这是类型系统本身，属原语，保留。

---

## 2. 目标与非目标

**目标**

1. 建立**合同注册表**（Contract Registry）作为版本唯一事实源：`kind → { current, supported: [N-1, N], normalize: Option<fn> }`；全部 `== N` 断言改为注册表谓词 `accepts(kind, v)`。
2. 渲染器合同（agent-task/orchestration-task）去字符串化：常量出自注册表；`source_views` pack 匹配器改为**结构识别**（存在 `source.workflow_instance_id` + `inputs` + `agent` 即视为任务形状，restore 全等回验兜底），不再比较 renderer 字符串。
3. 实例/草稿/工具等查询过滤去版本化：按 kind + 结构字段（如 `steps` 数组存在）判定，或经注册表谓词。
4. 供应商能力声明化：Harness/Sandbox 定义增加能力字段（`orchestration_bridge` / `external_tools` / `platform_tool_bridge`），替换全部 `provider == "codex"` 与 `["codex","dsh"]` 白名单；`docker-worker/v1|v2` 统一为单一 enforcement 合同常量并消除并存。
5. 任务合同中的读取机制外置：`read_tool`/`max_bytes` 等改为从注册表/工具引用解析，合同正文只保留"引用 + 按 next_offset 读取"语义。
6. 测试与出厂配置解耦：hash 防漂移门禁改为**资源自洽校验**（manifest bundle_hash 与目录内容一致，脚本化，不进 Rust 常量）；Rust 测试改用内联夹具流程断言引擎语义，不引用 `builtin-*` 身份。
7. Web 去断言：版本判断全部走后端校验查询；前端仅保留展示性默认值。
8. `--normalize-plan` 注册表驱动（升级目标 = `registry.current`）；或按"版本内容移除"方向直接删除该一次性工具（决策项 D1）。

**非目标**

- 不做旧数据迁移（沿用"不迁移"哲学；兼容窗口靠 `supported` 范围表达，过期即拒绝）。
- 不改外部运行时锚（DSH/Codex 版本、recipe digest）与存储 user_version 机制。
- 不动概念词汇白名单与协议注册表（工具/处理器名）。
- 不在本计划内重设计 17 kind 或权限五层（那是 P2/P9 的范围）。

## 3. 方案设计

> **方向修正（2026-11-12，用户指示）**：初版 3.1 采用"合同注册表"（集中声明各对象 current/supported 版本）。用户指出这仍是版本思维——把散落的硬编码集中化，没有消除"数据演进需要动代码"的根本问题。正确方向：**代码只定义原语与领域对象，数据合法性由结构识别决定**（详见 [docs/contracts.md](../../contracts.md)）。3.1 已按此重写；注册表实现已删除。

### 3.1 结构识别（替代合同注册表）

- **接受判据 = 领域对象反序列化 + 业务规则**：workflow/spec/standard/proposal/profile 等的校验不再比较 `schema_version`；serde `deny_unknown_fields` 反序列化本身就是结构边界，业务规则（DAG、端口、引用、非空正文）在其后校验。
- **类型分派按结构字段**：外部工具 = `integration_ref` 存在；Spec 记录 = 能 decode `ConfiguredSpecification`；草稿"当前性" = 文档能否 parse 为 `WorkflowDefinition`。
- **存储与合同自洽**：workflow 实例 = `steps` 数组存在；任务合同 = 外层/内层 `renderer_version` 一致 + 摘要一致（记录自身一致性，不与代码常量比对）。
- **`schema_version`/`renderer_version` 降级为记录性元数据**：写入时用简单常量（`WORKFLOW_INSTANCE_SCHEMA`、`AGENT_TASK_RENDERER`、`ORCHESTRATION_RENDERER`）记录身份，任何读取/校验路径不比较版本号。原 3.1 的 ContractSpec/Contract/accepts/require_current 设计作废。

```rust
pub struct ContractSpec {
    pub current: u64,           // 写入/升级目标
    pub supported: RangeInclusive<u64>,  // 接受读取的范围（建议 N-1..=N）
    pub renderer: Option<&'static str>,  // agent-task / orchestration-task
    pub normalize: Option<fn(&mut Value) -> Result<()>>,  // 旧→新规范形钩子
}
pub enum Contract { Workflow, WorkflowInstance, AgentTask, OrchestrationTask,
                    Spec, Standard, Proposal, PermissionProfile,
                    ProjectConfiguration, WorkConfiguration, ExternalTool, TemplateManifest }
impl Contract { pub fn accepts(self, v: u64) -> bool; pub fn current(self) -> u64; }
```

- 所有领域校验与 core 过滤改走 `Contract::X.accepts(...)`；错误消息统一由注册表生成（含 supported 范围与升级指引）。
- `supported` 允许 `[N-1, N]` 并存：读取侧接受两版（restore 路径本来就按结构重建），写入侧永远写 `current`——这在不迁移前提下给出有限的演进缓冲。

### 3.2 结构识别替代字符串匹配

- `source_views.rs` 的任务形状识别：以"字段指纹"（`source.workflow_instance_id` + `inputs` + `input_sources` + `agent`）判定候选，尝试打包→restore 全等回验（现有机制保留为正确性证明），renderer 字符串仅作记录不再作判据。
- `configured_runtime` 的实例查询过滤：`kind == "workflow_instance" && data.steps.is_array()`，schema 交给校验层。

### 3.3 能力声明

- `HarnessDefinition.data` 增加 `capabilities: { orchestration_bridge, external_tools, platform_tool_bridge }`（默认 false）；默认目录的两个 harness 按现状填 codex=true。
- core 的 4 处 `provider == "codex"`、governance 的 3 处数组白名单改为读能力字段/适配器能力表；适配器身份已含能力矩阵，可作为运行时事实来源。
- `enforcement` 合同：`adapters/permission_profiles.rs` 定义 `pub const DOCKER_WORKER_ENFORCEMENT: &str`，消除 `:41` 的 v1 与 `:46` 的 v2 并存（统一到一个版本常量，修正比较处）。

### 3.4 读取机制外置与测试解耦

- 渲染合同中 `read_tool/arguments` 从注册表常量解析（工具名仍是协议常量，但参数形态集中一处并可随合同版本演进）；32KiB 阈值移入常量区。
- 新增 `scripts/check/check-defaults.py`（或并入现有 check-permissions 思路）：按 manifest 计算 bundle_hash 与目录内容比对——资源完整性校验替代 Rust 内 hash 常量；`configuration_resources.rs` 删除 3 个十六进制常量断言，保留"源协议/引用语义"断言。
- `configured_workflows.rs` 中对默认流程的结构断言：迁移为"内联夹具流程（测试自建 workflow 文档）断言引擎语义" + "默认目录经 `compile_workflow_sources` 全量编译通过 + requirements 数量与 manifest 一致"两级，不再逐步骤硬断言。

### 3.5 Web 与 CLI

- WorkflowManager 导入校验改调 `query.workflow_validation`（本地仅做 JSON parse）；schema 过滤改为后端 `query.workflows` 已返回的可用性标记。
- WorkflowTaskPreview 的 renderer 过滤删除（后端返回即已校验）。
- D1（决策项）：`--normalize-plan` 保留（注册表驱动）或删除（一次性历史工具，随版本常量清理）。建议：**删除**，理由是 v5→v6 已完成历史使命，未来演进走注册表 normalize 钩子在线内完成。

## 4. 工作项

| 编号 | 工作项 | 涉及 | 验收要点 |
|---|---|---|---|
| W01 | 全部 `== N` 断言迁移为结构识别（约 25 处；初版注册表方案已按用户修正删除） | domain、core | src 中版本断言零残留；数据合法性由结构决定 |
| W02 | source_views 任务形状结构识别 + renderer 去判据 | adapters/source_views.rs | 伪造 renderer 字符串不影响 pack/restore 正确性 |
| W03 | 实例/草稿查询过滤去版本化（6 处） | core/configured_runtime.rs、configured_workflows.rs | 按 kind+结构判定，行为等价 |
| W04 | Harness 能力字段 + 替换 provider 字符串比较（7 处）+ enforcement 常量统一 | domain、core、adapters/permission_profiles.rs、resources/configuration/harnesses | 假想供应商（测试替身）声明能力即可通过组合校验 |
| W05 | 读取机制外置（read_tool/32KiB 集中为注册表常量） | core/configured_runtime.rs | 合同正文不再内嵌散落参数 |
| W06 | 测试解耦：hash 门禁→manifest 自洽脚本；默认流程断言→内联夹具 + 编译级校验 | crates/adapters/tests、scripts/check | 修改默认 Skill 正文后 `cargo test` 全绿 |
| W07 | Web 去断言（8 处）改后端校验 | web/src 三个组件 | 前端 grep 不到 `schema_version ===` / `agent-task/v` |
| W08 | D1 决策执行：删除或注册表化 `--normalize-plan` | crates/host/main.rs | 若删除：help 与文档同步 |
| W09 | 文档登记：数据合同（注册表管）vs 实现身份（vN 命名但非合同）vs 运行时锚（保留）三层清单 | docs/domain.md 或新节 | 三层边界成文 |

## 5. 验收标准

- A01 **演进演练（按方向修正重写）**：给 `WorkflowDefinition` 增加一个可选字段并发布含该字段的新 workflow——不改任何版本断言/注册表（已无此物），存量 v6 文档与新文档同时可读可发布；带未知字段的文档被 serde 结构边界拒绝。
- A02 演进演练后 `source_views` pack：v7 任务记录正确识别为任务形状（结构识别），restore 全等。
- A03 测试替身 harness 声明 `external_tools=true` → Capability 引用外部工具的编译与冻结通过，core 无厂商字符串可改。
- A04 修改 `resources/configuration/skills/*.md` 任一正文 → `cargo test --workspace --locked` 全绿（当前会被 hash 门禁打破）；`check-defaults` 脚本捕获真正的目录篡改。
- A05 web 导入 v7 草稿 → 后端返回结构化错误；前端代码中无版本常量。
- A06 `grep -rnE 'schema_version.*== *[0-9]|== *"agent-task/v' crates/*/src web/src` 零命中（既无散落断言，也无注册表）。
- A07 全量既有回归（cargo workspace + Playwright 双档）不回归。

## 6. 风险与兼容

- **注册表本身成为新的硬编码点**：必须支持 range + normalize 钩子，禁止退化回单值；A01 演练作为长期回归保留。
- supported 窗口与"不迁移"哲学的张力：窗口只承诺**读取兼容**，不承诺升级路径；文档明确（W09）。
- 能力字段是配置新字段：默认目录两个 harness 需随 bundle 推进补填；旧 harness 修订无该字段 → 按全部 false 处理（保守）。
- 测试重构（W06）触碰 2800 行主回归文件，纪律：先建夹具层、逐段迁移、全程跑全量。

## 7. 依赖与顺序

无外部依赖。建议顺序：W01（注册表，其余的地基）→ W02/W03（存储与查询）→ W04（能力声明）→ W05 → W06（测试解耦，工作量最大）→ W07 → W08/W09。
与 P10（生态扩展）高度协同：W04 完成后 P10 的 openai-compatible 与新 Harness 接入成本大幅下降，建议 P10 在本计划 W04 之后实施。

## 8. 决策项

- **D1**：`--normalize-plan` 删除（推荐，理由见 3.5）还是注册表化保留？
- **D2**：~~supported 窗口~~——随方向修正作废：结构识别下不存在版本窗口概念。
- **D3**：`check-defaults` 校验脚本放 `scripts/py/`（实现）+ `scripts/check/check-defaults.sh`（入口）——2026-11-12 用户按目录约定定位；备选保留为 Rust 测试已否决。
