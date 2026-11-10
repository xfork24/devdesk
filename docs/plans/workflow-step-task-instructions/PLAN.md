# Workflow Step 任务指令改造计划

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

日期：2026-11-09。状态：已实施并完成本机验收；实际结果、并行 main 整合和未覆盖环境见 [实施记录](EXECUTION.md)。

让每个实际调用 Agent 的工作流步骤都有可在 UI 编辑的明确任务指令，与冻结的 Role、Agent 指令、Skill、Spec 和业务输入一起进入模型。任务指令属于 Workflow 不可变修订；默认任务正文从 Rust 分支迁到 Markdown 资源。保留引擎对执行范围、真实证据、输出协议和状态推进的控制。

审核起点：`main` 的 `9d7051d`，已包含 Permission 运行时绑定与 DSH 多模型服务商实现。承接 [配置与工作流契约](../role-capability-project-configuration/CONTRACT.md)、[Permission 运行时绑定](../permission-runtime-bindings/PLAN.md)；与 [工具与技能接入计划](../tool-skill-integrations/PLAN.md)共用冻结上下文和权限边界。并行任务可能继续更新工作区，实施前重新核对基线和接入位置。

## 一 当前设计审核

| 当前事实 | 改造影响 | 主要入口 |
|---|---|---|
| 标准动作目录是字符串数组；中文名称、需要 Role 的动作在前端另有映射 | 不能继续靠几个不同的列表猜测某动作是否调用模型；需要共享执行能力描述 | [domain/governance.rs](../../../crates/domain/src/governance.rs)、[configuration-ui.tsx](../../../web/src/components/configuration-ui.tsx)、[WorkflowManager.tsx](../../../web/src/components/WorkflowManager.tsx) |
| `WorkflowStep` 没有通用任务指令，只有 `agent.interact` 的 `interaction.goal` | 讨论目标可编辑，其余 Agent 动作缺少同等入口 | [domain/governance.rs](../../../crates/domain/src/governance.rs) |
| 讨论首轮输入由 goal、required_topics、output_kind、材料摘要和固定提示拼接 | 任务正文、输入材料和输出协议混在一个字符串中 | [workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) 的 `evaluate_step` |
| 正式执行按 Run 节点选择大段英文提示；独立评审另有边界提示和结果要求 | 只给 Step 加字段会留下正式执行、配方和 Review 漏接 | [live_runs.rs](../../../crates/core/src/live_runs.rs) 的 `poll_live_run` |
| 一个 `attempt.execute` 包含多个配方节点，有不同 Role 和写入范围 | 除步骤公共任务外，必须能编辑每个 Agent 配方节点的专门任务 | 同上、`WorkflowRunNode`、`WorkflowManager.tsx` 的执行配方表单 |
| `red/green/verify/feature_tests/merge_verify` 直接调用真实测试执行器，不调用模型 | 不能为系统测试节点制造无效的 Agent 指令字段 | [live_runs.rs](../../../crates/core/src/live_runs.rs) 的 `poll_live_run`、`poll_live_test` |
| `merge.prepare` 当前要求 Role/Spec，但实际构造 Git 候选并产生 `merge_verify` 测试 Run | 有 Role 不等于有模型调用；应明确展示系统执行，保留其身份、Spec 和测试门禁 | [live_merges.rs](../../../crates/core/src/live_merges.rs)、[host/workspace.rs](../../../crates/host/src/workspace.rs) 的 `resolve_live_merge` |
| Feature 锁已有展开计划、Spec 审计、Role→Agent 和定义摘要；执行时另有 `execution_specification` | 可复用冻结机制，但任务文本、配方落点和渲染合同也必须进入闭包和差异 | [workflow_definition.rs](../../../crates/core/src/workflow_definition.rs)、[project_configuration.rs](../../../crates/core/src/project_configuration.rs) |
| 讨论已在接受输入时冻结 Spec，派发从 Turn 快照读取；正式 Run 还保存 run-input Artifact | 扩展每轮冻结，不退回“派发时读最新 Workflow”的做法 | [conversations.rs](../../../crates/core/src/conversations.rs)、[conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs)、[live_runs.rs](../../../crates/core/src/live_runs.rs) |
| Role/Agent 正文、Skill 在 Session 创建时进入原生指令，Spec 在 Turn 输入中携带 | 新任务组装需覆盖 Session 基础上下文与每轮任务，避免 Role 重复拼接或只在首轮生效 | [adapters/harness.rs](../../../crates/adapters/src/harness.rs) 的 worker 创建与 `dispatch_turn` |
| 同一 Agent/owner 可以复用 Session；已有回归核对复用时每轮 Spec 冻结 | 任务必须按步骤实例和轮次定位，不能按 Session 最近一条记录推断 | [conversations.rs](../../../crates/core/src/conversations.rs)、[configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs) |
| Permission 切换创建后继 Session，保存原输入，但新 Session 创建参数未通用携带步骤任务绑定 | 新任务合同必须跨后继保留来源与 Spec，不能仅依靠历史摘要中的文字恢复 | [permission_switch.rs](../../../crates/core/src/permission_switch.rs) |
| 默认讨论 goal 会拼入整段 Spec 正文；阶段 description 也引用阶段 Spec | 新默认应分清任务、阶段背景与验收规范，减少重复；旧文本不做猜测式去重 | default_catalog.rs（历史路径：`../../../crates/core/src/default_catalog.rs`）、`resources/configuration/` |

审核结论：需要同时改造声明、动作能力描述、编译冻结、任务组装、两条派发路径、UI、默认资源和兼容读取。不能只把现有大字符串挪到文件，或在 UI 新增一个未进入派发的文本框。

## 二 范围和职责

本次不新增独立 Prompt 资源库、模板脚本或任意模型执行动作。用户在 Workflow 草稿中编辑任务，发布产生 Workflow 新修订；同一任务可通过现有确切子流程引用复用。

| 内容 | 职责及编辑位置 |
|---|---|
| Role | 长期职责与行为原则，继续在 Role 编辑 |
| Agent 附加指令 | 此 Agent 的补充行为，保留现有组合与冻结方式 |
| Skill | 执行方法、经验与工具使用方法，继续作为所选 Skill 资源 |
| Spec | 应交付内容、验收标准和适用范围，继续绑定确切修订 |
| Step 任务指令 | 本步骤具体要做什么，新增必填 Markdown 编辑器 |
| 配方节点任务指令 | 公共任务下此 Agent 节点的专门工作，在节点表单编辑 |
| 阶段意图 | 阶段背景，作为有来源的上下文展示与传递；不替代必填任务 |
| 系统执行合同 | 真实执行权限、只读/写入边界、输出结构、来源校验和状态机，由引擎与执行器控制 |

将 PRD 澄清方式、测试编写方法、实现目标、文档内容、阶段评审重点等业务正文迁到可编辑任务和默认 Markdown。测试成功判定、独立评审身份、不可变测试挂载、真实工具证据、Git 协调权、模型不能自行采纳提案等要求仍由现有代码门禁和版本化输出合同控制。

任务正文不能扩大 Permission、ExecutionScope、Capability、审批策略或交付接受权限。对于自然语言之间的矛盾，系统只能检查已实现的结构规则并呈现来源，不能宣称自动理解并证明任意任务与 Spec 一致。

## 三 Agent 执行范围

| 动作或节点 | 实际执行者 | 指令要求 |
|---|---|---|
| `agent.interact` | Agent 多轮讨论 | Step 任务必填；继续配置必答主题、输出类型与最大轮数 |
| `delivery.review`、`merge.review` | 独立 Reviewer Agent | Step 任务必填；交付范围与输出协议由系统提供 |
| `attempt.execute` | 配方协调器，展开 Agent 与测试节点 | Step 公共任务必填；每个 Agent 配方节点另有必填专门任务 |
| `requirement_validate`、`plan_validate`、`test_plan_validate` | Agent | 配方节点任务必填 |
| `test_author`、`implement`、`docs` | Agent | 配方节点任务必填；沿用节点的真实读写范围 |
| `red`、`green`、`verify`、`feature_tests`、`merge_verify` | 测试执行器 | 不接受 Agent 任务正文，显示系统行为与实际测试配方 |
| `merge.prepare` | Git 协调器和候选测试 | 不新增虚假 Agent 任务；保留当前 Role/Spec 的身份和验收用途 |
| `proposal.adopt`、`merge.apply`、`snapshot.capture`、`resource.cleanup`、`branch`、`join` | 人或系统协调器 | 不要求 Agent 任务 |
| `workflow.call`、`tasks.foreach` | 子流程/Task 集合协调器 | 子流程内部的实际 Agent 节点承担任务，不把父节点文字暗中覆盖给所有子节点 |

动作目录增加版本化元数据，例如执行模式、是否需要任务、是否需要 Role/Spec、配方节点执行者、输出合同 ID。现有字符串目录保持兼容；增加 `action_contracts` 描述供 Core、Host、Schema 回归和 Web 使用。前端不再用 `actorActions` 同时推断 Role 和任务需求。

不开放任意新配方 key、执行器或角色职责。本次只为当前支持的节点提供指令配置；将来增加 Agent 执行路径时，必须同时登记任务与输出合同。

## 四 声明语言和版本

采用 Workflow Schema v3 与编译器 v3，保留 v1/v2 的解码和原执行合同。支持任务的动作在 v3 使用动作合同 v2；系统动作继续原合同，合法组合由动作目录校验。配置资源自身的 version 与这些合同版本分别记录。

在 `WorkflowStep` 和 `WorkflowRunNode` 增加 `task_instructions`。领域读取用兼容可选字段，省略时不改变旧序列化形状；v3 发布校验按上节范围强制非空，未知字段和显式 null 拒绝。系统节点带此字段时报不适用，不静默忽略。

示意片段（其余 DAG、Role/Spec 和交互字段沿用已有结构）：

```json
{
  "kind": "agent.interact",
  "contract_version": 2,
  "task_instructions": "## 本次任务\n围绕现有用户问题澄清需求，先列出待确认事项，再形成可采纳 PRD。",
  "interaction": {
    "required_topics": ["目标用户与场景", "范围及非目标", "失败与恢复行为"],
    "output_kind": "requirement",
    "max_turns": 8
  }
}
```

`attempt.execute` 的公共任务与配方节点任务共同生效，节点指令是补充，不替换公共任务、Role、Skill 或 Spec。UI 预览明确显示两部分。例如公共任务描述该 Task 的实现目标，`test_author` 描述独立测试工作，`implement` 描述实现工作，`docs` 描述文档工作。

新版本不以 `interaction.goal` 维护第二份正文。升级时把旧 goal 原文复制到 `task_instructions`，在新修订清空 goal；v3 只允许 goal 省略或空字符串以兼容序列化。旧版仍按原 goal 解释，两个非空任务来源在 v3 拒绝。`required_topics`、`output_kind`、`max_turns` 继续作为独立结构字段。

任务作为普通 Markdown 原文保存，保留换行、代码块、逗号和 Unicode；不引入 `{{变量}}`、shell 求值、文件 include 或运行时联网读取。数据通过命名输入单独传递。正文中出现类似占位符时按普通文字处理。

P0 固定并测试字段和总量上限，初定单段任务不超过 32 KiB UTF-8、展开任务正文总量不超过 1 MiB；与现有 HTTP 请求、1000 节点和 8 层子流程限制共同核对。空白正文拒绝，保存时不裁剪或静默截断。

## 五 统一任务组装

建立共享的任务解析与渲染模块，Core 负责从冻结来源确定任务，Harness 适配器只做原生协议编码。讨论、正式执行、Review 与预览都调用同一组装规则，v3 不再进入 `live_runs.rs` 的旧业务提示分支。

一次执行包含两个明确部分：

1. Session 基础上下文：冻结 Agent/Role 的有效指令、选定 Skill、基础执行约束。沿用当前 Role 与 Agent 附加指令的合成结果，不重复注入同一份 Role 正文。
2. 每轮任务上下文：Workflow/Step 来源、阶段背景、公共与节点任务、确切 Spec、必答主题、用户补充、输入材料、前序证据、当前授权范围和输出合同。

基础上下文继续映射到 Codex 的原生指令和 DSH 的 persona；每轮任务分别映射到 `turn/start` 与 `session/prompt`。两端接收相同的规范任务内容，供应商封套与路由 ID 可以不同。不能只在开发者指令、第一轮或默认 Agent 上接入任务。

建议冻结的规范对象 `FrozenAgentTask` 至少包含：

| 分组 | 内容 |
|---|---|
| 渲染身份 | `schema_version`、`renderer_version`、规范内容摘要、基础上下文摘要 |
| 来源 | Project/Feature/Task、配置锁、源 Workflow 修订、完整子流程调用路径、Step 实例及轮次、可选配方节点、Run/Turn |
| 执行身份 | Role、Agent、Skill、Spec 的确切修订与内容摘要；任务的原文与来源 |
| 本次任务 | 公共任务、节点任务、讨论主题、用户本轮输入、阶段背景 |
| 材料 | 命名输入、采纳文档、代码版本、依赖步骤产物和既有评审结果的受控引用及必要正文 |
| 系统合同 | 动作/输出合同 ID 和版本、冻结执行范围及权限引用 |

先按声明的输入端口和已实现的工程材料规则解析数据，再渲染任务；不能把全部项目库、任意前序记录或未经选择的 Skill 自动加入上下文。缺少必需输入、引用越权、摘要不符或来源不唯一时在模型调用前拒绝。

保留现有用户输入和正式运行输入的限额含义；新增最终组装总量检查。初定最终规范输入上限沿用正式运行的 512 KiB，并核对供应商预算；任务、Role、所选 Skill/Spec 不静默丢弃。材料摘要允许有界摘录，但必须标明截取和原始来源，预览与实际派发一致。

系统合同以固定类型、字段和简短约束表达：例如要求 ProposalContent、`summary/completed`、文档对象或逐项评审结果。当前结果解析要求的 key 不因用户编辑 Markdown 自动改变；自定义任意输出 Schema 不在本次范围。

## 六 冻结和持久化

Workflow 发布后不可变，任务原文进入 Workflow 内容摘要。编译展开时保留源修订和完整调用路径，把公共任务、节点任务、渲染合同及摘要加入编译闭包和任务审计。两个不同调用点引用同一子流程时具有不同执行身份，不能按 Step key 合并。

Project 配置审计和差异增加任务正文、配方落点、输出合同及来源变化；仅改任务也必须显示变更。Feature 创建时随当前授权闭包冻结。Project 配置的 v2 资源结构可以保留，内部按 compiler_version 区分旧锁与 v3 锁，不能只机械修改 Schema 常量。

执行时分两级冻结：

- 步骤启动时确定该实例、轮次和配方节点对应的冻结任务定义，建立可核验的执行关联。
- 每个讨论输入接受或正式 Run 输入入队时，结合当时材料和用户输入持久化最终任务快照及摘要；之后派发读取此快照，不读取当前目录的最新正文。

优先沿用项目治理记录和 Artifact：增加不可变的任务绑定/执行快照记录，按 Turn/Run 唯一关联，保存规范任务、模型实际接收的新输入及各段摘要。扩展 `SessionStart`/`TurnDispatch` 的受信任务数据；客户端只能通过 Workflow 修订编辑指令，不能向普通 `conversation.send` 注入伪造任务来源或系统合同。

`ConversationTurn.input_digest` 继续标识其原有输入，另记录最终渲染内容摘要；不能把旧字段含义改成另一种内容。摘要覆盖语义内容和渲染版本，不包含供应商生成的 thread/request ID，这些路由信息仍由协议日志关联。

任务定义摘要与单次执行载荷摘要分开。统一按规范 JSON 编码计算，Markdown 字符串保持原文、列表保留声明顺序，文件/UI/类型化对象不因对象键顺序不同产生不同身份。显式技术重试可以形成新的 Run/Turn 身份和载荷摘要，但必须关联原任务定义及冻结业务输入；它与同一个 Turn 的传输恢复、命令重放不是同一种操作。

增加必要的项目唯一索引和领域一致性校验；Workflow JSON 本身不要求为每段 Markdown 新建表。迁移编号实施时从实际最新全局/项目版本分配，不修改旧 SQL。新增可选字段必须验证旧 RuntimeBinding、Feature 锁、Snapshot、Run、清单和命令重放的原始身份不变。

## 七 会话 续轮 重试和切换

| 场景 | 使用的任务 |
|---|---|
| 同一讨论 Step 的后续用户输入 | 继续该步骤实例冻结的任务，附本轮补充和该轮材料快照 |
| 不同 Step 或返工新轮次 | 明确新任务关联；v3 默认建立独立步骤 Session，可通过带来源的 Snapshot 选择继承材料 |
| 同一 Session 的兼容复用 | 每轮必须有自己的任务快照；禁止按“最近绑定”覆盖已排队的输入，保留已有逐轮 Spec 冻结回归 |
| 显式技术重试 | 沿用原任务定义、渲染合同和冻结业务输入，新执行身份另记并关联 retry_of；不读取最新 Workflow |
| 原命令重放、Host 重启、输入派发结果未知 | 继续原执行载荷及摘要；沿用消费与核对协议，不重发未知结果 |
| 业务返工 | 新步骤轮次、新材料快照；任务正文仍来自该 Feature 的冻结 Workflow |
| 修改并发布 Workflow | 新配置冻结采用新正文，已有 Feature/Attempt/Turn 不热更新 |
| Permission 后继 Session | 保留原步骤与任务来源、Spec 和用户消息身份，只重新冻结被授权改变的权限部分 |

现有 Session 复用不能成为任务权威来源。v3 关联需能验证 workflow_instance、step_instance、round、可选 recipe_node 与 Session；多个可能步骤时返回明确诊断，不选择第一项或最近一项。原生历史对话不等于当前有效任务，继承内容按历史材料标注。

权限切换必须复制或重新关联同一冻结任务定义和 Spec，并对新权限形成新执行快照；原 Turn 记录不改写。恢复持久化切换意图时继续当时选定的任务与权限，不能因目录或渲染器更新重新解释原命令。此交叉路径纳入专门回归。

本次不增加活动 Feature 在线换 Workflow 的能力。要更改已授权工程的任务正文，需要现有明确支持的新配置冻结流程；第一版 UI 不把“保存任务新版本”描述成对所有运行立即生效。

## 八 UI 和审计预览

Workflow 编辑器在需要任务的 Step 下显示必填的“任务指令（Markdown）”，支持源码/预览并保留原文。讨论用此字段替代当前“讨论目标”；必答主题、输出类型与轮数保留。阶段背景、Role、Skill、Spec 分开显示其来源和职责。

正式执行显示“公共任务指令”，配方中的 Agent 节点显示“节点任务指令”；系统测试节点展示执行者、真实测试配方和依赖，不出现不会被使用的输入框。切换动作或执行阶段时对已有任务正文做明确保留/清除处理，避免用旧节点任务冒充新动作任务。

新增只读任务预览查询，暂定 `query.workflow_task_preview`，支持草稿/确切 Workflow、完整步骤路径和可选配方节点，并在有项目配置时解析实际 Agent/Role/Skill/Spec。缺少绑定或材料时展示具体缺口，不自动选择默认 Agent、准备运行环境或调用模型。

预览分为两种：

- 配置预览：查看任务原文、可解析的上下文、输出合同和缺失输入；明确尚未形成运行授权。
- 执行记录：按 Turn/Run 读取当时持久化的任务快照、正文、摘要和实际采用修订。区分本轮新增输入与供应商已有对话历史，不宣称还原模型内部完整上下文。

发布门禁校验所有展开 Agent 任务，错误定位到源子流程、Step 和配方节点。任务编辑、草稿切换、刷新、子流程跳转、版本差异、导入/导出均保留全文。Spec 审计可复用入口，但需显示任务维度，不把任务差异藏在大段 JSON 中。

## 九 默认任务与硬编码清理

在 `resources/configuration/tasks/` 保存默认任务 Markdown，按动作、阶段和 Agent 配方节点组织。默认 Workflow 播种时将正文写进具体 Step/节点；发布包嵌入资源，UI 从已导入修订读写，不依赖运行时源码路径。

建立现有提示词迁移清单，至少包含：讨论任务、三个规划验证节点、Task 测试编写、Feature 收尾测试编写、实现、文档、交付评审、候选评审。每段明确拆分任务正文、Skill 方法、Spec 判准和系统输出合同，逐项核对调用点；不能删掉旧字符串后用一句笼统默认任务补空。

新默认不再把完整 Spec 复制进任务或讨论目标。任务引用由正文解释的业务目标，Spec/Skill 通过确切资源单独传入；共同内容只按已知默认资源人工整理，旧自定义正文不自动去重。

追加默认 Workflow/子流程新修订，保留旧修订和项目引用。默认任务必须在 UI 可见可编辑，与用户自己编写的任务走同一发布、冻结、预览和派发路径。

## 十 旧配置和渲染合同兼容

1. v1/v2 Workflow、旧编译锁和已接受 Turn 保留原行为、内容摘要及输入记录。不能通过补字段把历史数据假装升级成带任务合同的配置。
2. 旧定义可继续明确标记为兼容模式；新建与升级后的编辑默认使用 v3。UI 提供“创建任务指令版草稿”，原 Workflow、Project 默认和 Feature 引用不改写。
3. 讨论升级复制旧 goal 和主题原文；其他旧 Agent 路径按确切动作、phase、节点和历史合同映射到完整任务。无法证明映射的自定义节点列为待补，不自动生成“请完成任务”后通过发布。
4. v3 引用含 Agent 的旧版子流程时要求先形成其 v3 修订并显式重新选择；不在展开过程中静默借用最新版默认任务。纯系统子流程的兼容规则在 P0 明确并回归。
5. 渲染器与输出合同也要版本化。旧任务不能由新版组装器重新渲染后冒充相同输入；优先使用已冻结内容。未保存完整输入时，只能按已证明的旧程序构建与提示合同映射，不能仅凭 Workflow v2 推定所有历史提示相同；未知构建明确待核对。
6. 旧代码提示保留在独立兼容路径供旧冻结运行使用；v3 缺任务时报错，不能落回隐藏的旧字符串。
7. 更新默认目录不会自动换绑已有项目；旧锁重试不追随新任务、Skill 或 Spec。迁移可重跑，来源和待处理项可审计。

适配器构建校验与任务渲染版本分别核验。新增协议字段可能改变当前 Harness 的构建身份；旧历史可读不等于旧 worker 可直接由新二进制续用。保留已有漂移门禁，需要后继或兼容执行器时明确处理，不能为了让任务迁移通过而绕过安装、适配器、权限和模型连接的冻结校验。

## 十一 实施阶段

### P0 契约和现有行为回归

- [x] 固定动作/配方执行者目录、v3 字段、动作合同版本、必填与不适用规则、输入上限及输出合同。
- [x] 建立全部 Agent 提示来源清单，区分业务任务与真实执行门禁；覆盖候选准备等有 Role 但无模型的路径。
- [x] 固定任务身份、逐轮快照、会话复用与权限后继关联规则，并保存旧序列化/摘要回归。

完成条件：每个实际模型调用有唯一任务来源，每个系统节点不承担虚假 Agent 任务。

### P1 声明 编译和冻结

- [x] 实现 v3 解析与发布校验、任务原文/摘要、配方和子流程完整路径。
- [x] 扩展编译闭包、配置差异、Feature 冻结与执行关联，追加必要迁移。
- [x] 实现明确的旧版升级草稿与来源记录，不改历史默认或锁。

完成条件：任务修改可审计；同名子流程落点不串用；新锁采用新文本，旧锁保持固定。

### P2 统一任务组装和派发

- [x] 实现共享组装器、Session 基础上下文、每轮任务、渲染合同、总量检查和最终摘要。
- [x] 接通讨论首轮/续轮、正式配方和两类 Review；Codex/DSH 只编码相同任务内容。
- [x] 在接受/入队时持久化任务，派发与重试只读冻结快照；补齐权限后继和恢复关联。

完成条件：全部 v3 Agent 执行可证明收到 UI 配置的任务、Role/Skill/Spec；无遗留隐藏业务任务分支。

### P3 默认资源和兼容路径

- [x] 提取默认 Markdown 任务，整理业务提示、Skill、Spec 与系统合同的落点。
- [x] 发布新的默认 Workflow/子流程修订，完整填写所有 Agent 节点任务。
- [x] 隔离旧渲染与执行合同，处理不支持升级及缺失任务的明确诊断。

完成条件：新默认与自定义任务使用同一路径；旧配置继续可读和按原身份恢复。

### P4 UI 预览和协议

- [x] 接入公共与节点 Markdown 编辑、必填诊断、草稿保留及不同执行者展示。
- [x] 实现配置预览和运行记录的实际任务查询，展示精确来源、正文与差异。
- [x] 同步 RPC/JSON/manifest Schema、协议、领域、默认资源与迁移文档。

完成条件：用户可完成编辑、保存、发布、选择新配置、执行及核对实际任务，提示准确反映生效范围。

### P5 完整验证

- [x] 运行下列矩阵中的领域、存储、协议、HTTP、浏览器和发布包回归。
- [x] 在可用环境进行真实 Codex 任务执行；DSH 至少完成原生协议任务载荷一致性，真实模型按可用凭据明确记录。
- [x] 写 EXECUTION.md，记录实际平台、模型/执行器版本、验证方式与未覆盖项，更新索引状态。

完成条件：配置原文、执行证据与冻结身份一致；模拟协议、真实工具和真实模型的验证结论分别记录。

## 十二 验证矩阵

| 场景 | 必须证明的结果 |
|---|---|
| 全路径覆盖 | 讨论、规划验证、测试编写、实现、文档、交付评审、候选评审均收到任务；系统测试和候选准备不额外调用模型 |
| 数据合法性 | 空白/null、未知字段、错误动作版本、系统节点带指令、超长字段、展开总量超限、缺配方任务均被拒绝 |
| Markdown 往返 | 多语言、换行、代码块、逗号、看似模板的文本在编辑、保存、导入导出和派发中保持原文 |
| 组装一致 | 同一冻结输入的预览与正式任务内容/摘要一致；Codex/DSH 语义内容一致；Role 不重复拼接、Skill/Spec 不漏传 |
| 上下文与输出 | 只传授权材料与所选资源；缺输入和越权引用失败；过长不静默丢任务，输出仍按真实合同解析 |
| 多 Agent 配方 | 公共任务与节点任务同时存在；Tester/Developer 的任务、角色、Spec 和写范围正确对应，不使用同一笼统任务替代 |
| 子流程与任务集合 | 同一子流程的两个调用点、两个 Task 及不同 round 的任务身份不串用；父节点不覆盖子节点任务 |
| 修改与冻结 | 只改任务可见差异；发布新 Workflow 或更新资源后，已排队 Turn、已有 Feature/Run、技术重试仍使用原快照 |
| 会话复用 | 保留现有逐轮 Spec 测试；在同 Agent 的不同步骤和后续用户输入中核对任务来源，不读取最近错误绑定 |
| 权限后继 | 下一轮切换权限后仍携带相同冻结任务和 Spec；新权限摘要另记，旧输入/结果不变；命令与消息不重复 |
| 失败与重启 | 输入准备后崩溃、派发回执丢失、停止结果未知和进程重启不重新渲染已接受输入或重复调用 |
| 输出和门禁 | 编辑任务不能把测试自述当真实 Red/Green、绕过独立 Review、修改只读目录或自行接受 Delivery/合入 |
| 兼容升级 | v1/v2 原始数据、清单和摘要不变；升级草稿可重跑；缺少映射明确待补；旧合同不被 v3 默认正文替换 |
| 浏览器完整链路 | 编辑任务和节点任务→刷新/切换→发布→配置差异→预览→查询实际执行快照，无 API 表单替身 |
| 原生与真实模型 | 原生请求捕获证明每段原文进入正确协议字段；真实 Codex 在专用资源上按两份不同任务产生可观察工具结果，不能只依赖“我收到指令”的模型回答 |
| 独立发布包 | 无源码目录也可加载默认任务、编辑、发布和渲染；运行任务快照可查询，旧历史正常读取 |

## 十三 预计改动范围

| 范围 | 主要文件或模块 |
|---|---|
| 声明和动作合同 | `domain/governance.rs`、`configuration.rs`、兼容解码与动作描述目录 |
| 编译与冻结 | `core/workflow_definition.rs`、`project_configuration.rs`、`governance.rs` |
| 任务组装与运行 | 新共享任务模块；`workflow_runtime.rs`、`live_runs.rs`、`conversations.rs`、`conversation_runtime.rs`、`ports.rs` |
| 后继与恢复 | `permission_switch.rs`、Host 权限发送入口、Session 关联与历史校验 |
| 原生协议 | `adapters/harness.rs`、DSH persona/Codex 原生指令与输入编码；保留新模型路由合同 |
| 默认与 UI | `resources/configuration/tasks/`、`default_catalog.rs`、`WorkflowManager.tsx`、`configuration-ui.tsx`、审计与执行记录 |
| 数据和契约 | 按需追加项目 SQL；RPC、Workflow、manifest Schema 与协议文档 |
| 回归 | 配置/工作流/权限切换 Rust 测试、原生协议夹具、HTTP、浏览器、真实验收探针及独立包 |

按 P0 → P1 → P2 → P3 → P4 → P5 实施。任务必须从 UI 编辑一路可追溯到实际模型输入和执行记录后，才可宣称本改造完成。
