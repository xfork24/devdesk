# Workflow 领域与工程工作台调整实施计划

创建日期：2026-11-08。更新日期：2026-11-09。状态：待实施。范围：Scope 与配置版本及绑定、Workflow 核心领域、Agent 实例身份与经验作用域、上下文检索和恢复、前端信息架构、数据迁移及验收。

以 Workflow 统一驱动软件开发与人和 Agent 的交互，以 Feature 和 Task 组织全部执行及交付信息。Agent 实例由稳定 ID 标识，Agent 类型与 Harness 类型固定；全局定义的 Agent 按实例 ID 跨项目积累经验，项目内定义的 Agent 按实例 ID 在所属项目积累经验。项目名称、Feature 名称和时间是经验快照的主要筛选条件。

Scope 统一表达全局、Project 和 Feature 的边界。Workflow、Standard 等配置定义通过不可变修订独立演化，Project 可动态调整默认绑定；Feature 从创建时即冻结所采用的全部配置及依赖版本。Project 后续绑定变化只作用于之后创建的 Feature，已有 Feature 的未执行步骤、新 Task、新 Attempt 和新 Session 均沿用原配置。

默认工作流依据 [架构设计第 14 章](../devdesk-v0.2/PLAN.md#14-典型-feature-开发流程)，结合现有正式交付、评审、TDD 和真实执行契约细化。本文中的目标类型、接口名称和阶段编号用于实施设计，最终协议同步更新到 `docs/domain.md`、`docs/design.md`、`docs/protocol.md` 和 `schemas/`。

## 一 已确定的产品行为

1. 左侧导航顺序为：工作总览、执行与交付、角色讨论、工作流程、规范与身份、历史记录、设置。
2. 执行与交付是主要工作页面。文档、Context、Snapshot 和 Test 纳入对应 Feature 或 Task，取消独立的文档与上下文顶层入口。
3. 工作总览的卡片和列表可以进入执行与交付，并定位 Feature、Task、执行、交付或待处理事项；多条结果进入带筛选条件的列表。
4. 文档以 Feature 组织，保留 Task 来源及引用。Context 和 Snapshot 绑定实际 Feature、Task 或已有项目讨论范围；Feature 与 Task 都能查看对应测试定义和执行结果。
5. Workflow 是具有稳定身份和不可变修订的核心领域对象，定义阶段、参与者、能力需求、交互过程、产物契约、推进条件及异常路径。
6. 每个步骤的推进方式可配置为自动推进或等待人确认；修改形成 Workflow 新修订，由 Project 调整默认绑定后用于后续 Feature。已有 Feature 的推进策略保持创建时版本。
7. Agent 表示角色定义，实例 ID 唯一标识具体承担者。一个 ID 对应的 Agent 类型、Harness 类型和经验作用域固定；相同类型组合允许存在多个独立 ID。
8. 全局 Agent 定义产生全局实例，其历史可跨项目检索；项目 Agent 定义产生项目实例，其历史限定在所属项目。历史始终按实例 ID 归属，不按角色名称或类型组合合并。
9. 人和 Agent 都能渐进式读取实例历史：先获得索引和摘要，再展开某次工作的快照和原始证据；支持时间过滤、定位和场景恢复。
10. Feature/Task worktree、TDD、独立评审、冻结输入和合入策略继续由领域内核约束。Workflow 配置通过这些约束校验。
11. Scope 是统一的核心领域值对象；定义归属、绑定目标和运行事实归属分别使用有明确职责的 Scope 字段。配置定义的作用域与其使用目标分别记录。
12. 所有可绑定的配置定义具有稳定身份和不可变版本，依赖精确引用；Project 默认绑定具有独立修订，Feature 创建时形成完整配置锁，之后禁止重新绑定。

## 二 当前实现与改造入口

| 入口 | 当前实现 | 改造目标 |
|---|---|---|
| `web/src/main.tsx` | 页面、选中 Feature、Task 过滤与详情集中在一个组件；执行页内部键为 `workflow` | 拆分页面与对象详情，建立稳定路由；执行页与工作流定义页使用不同标识 |
| 工作总览 | 部分指标来自当前 Feature 已加载的详情页 | 按明确范围独立汇总，导航目标覆盖未加载的 Feature 和 Task |
| `web/src/components/RoleConversation.tsx` | 按 owner 查询 Session，初始角色固定为需求分析师 | 选择稳定实例 ID，由当前工作流步骤推荐角色与 Harness，提供实例经验入口 |
| `crates/domain/src/agents.rs` | AgentProfile 定义角色，Definition 修订链用于身份判断 | 保留角色定义与评审身份规则，新增定义作用域和独立的 AgentInstance |
| `crates/domain/src/lib.rs` | Session 记录 Agent 修订和运行时锁，Snapshot 记录捕获来源 | 增加实例及工作流关联，保留原历史锁与来源，不把 Session ID 作为实例 ID |
| Project/Feature/Task 的 `BindingLock` 与 `binding.update` | 部分绑定可更新，执行级锁在后续阶段生成 | 新增 Scope、通用修订引用、Project 默认绑定修订与 Feature 创建时配置锁；已有 Feature/Task 配置更新须由 core 拒绝 |
| `crates/core/src/lib.rs`、`live_runs.rs` | Attempt DAG、阶段角色及前置条件主要由代码匹配 phase 决定 | Workflow 决定步骤顺序与交互，受内核契约校验后生成冻结执行计划 |
| `crates/core/src/conversations.rs`、`proposals.rs` | 讨论、提案、采纳和工程执行有明确边界 | 将交互与步骤关联，通过可追溯的产物满足工作流输入输出契约 |
| `crates/host/src/workspace.rs` | 请求固定项目 Engine，解析运行时并保持项目隔离 | 新增实例解析及受控的跨项目只读经验检索，保留工程命令的目标项目边界 |
| `sql/global/`、`sql/` | 共享 Agent 配置在全局库，Session/Snapshot/工程事实在项目库 | 同时支持全局与项目定义；全局实例及可重建经验索引放全局，项目实例放所属项目 |

当前真实 `attempt.start(planning)` 要求已采纳 Requirement、Design、TestPlan 和 TaskPlan，`reconcile` 又以同一 planning Attempt 的两类交付决定 ready。默认流程需要先完成需求讨论及评审，再形成设计和计划；实施时拆开需求与方案规划的执行契约，并将证据有效性从整 Feature 的计数比较细化为明确的依赖版本检查，具体见第五、六节。

## 三 Scope 与配置版本及绑定

### Scope 的职责

引入核心领域值对象 `Scope = Global | Project(project_id) | Feature(project_id, feature_id)`，校验 Feature 与 Project 的真实归属。对象种类声明可接受的 Scope，避免任意对象拥有无意义的全局或 Feature 作用域。Task 的工程材料继续使用其精确 MaterialOwner，并继承所属 Feature 的治理配置。

| 使用位置 | 表达的含义 | 约束 |
|---|---|---|
| `definition_scope` | 定义的所有权、维护位置与可见边界 | 全局定义可被项目引用；项目定义仅供该项目及其 Features 使用 |
| `binding_target_scope` | 配置应用于谁 | Project 默认绑定可更新；Feature 配置锁创建后不可更改 |
| `owner_scope` | WorkflowInstance、Snapshot 等具体事实的归属 | 实际运行和材料属于确定项目及 Feature，不能因引用全局定义而变成全局事实 |
| Agent 实例的 `scope` | 实例身份及经验累积边界 | 继承 Agent 定义的全局或项目范围，与当前参与的 Feature 分别记录 |

全局 Workflow 在某个 Project 中使用时保留全局定义身份，其 WorkflowInstance 和 Feature 配置锁属于实际 Project/Feature。将全局定义复制并特化为项目定义时创建新定义身份，保存 `derived_from` 的精确修订；不改变原定义的 Scope 或版本历史。

定义身份的 Scope 创建后固定。全局定义的依赖只能引用全局定义；项目定义可以引用全局或本项目定义，禁止引用其他项目定义。创建 Project 绑定及 Feature 锁时递归校验依赖闭包的作用域、内容与兼容性，拒绝循环或缺失引用。为历史读取追加复制共享修订时保留来源 Scope 和身份。

首版 Agent、Workflow、Standard、Spec、Skill、Harness、Sandbox 和 Permission 的可复用定义支持 Global/Project Scope。Feature Scope 用于创建时配置锁和具体运行事实；Feature 应用这些定义时固定精确引用，不在创建后增加可变的局部治理覆盖。Scope 与既有 `TaskScope` 的验收范围、`ExecutionScope` 的运行授权分别命名和验证。

### 稳定身份与不可变版本

所有可绑定的配置采用统一修订契约：`definition_id` 标识独立演化的定义，`revision_id` 标识不可变内容，`version` 表示该定义内的版本序号，另保存内容摘要、前序修订和 Schema 版本。业务对象自身的乐观锁 `revision` 与定义修订分别处理。

| 配置对象 | 必须固定的依赖 |
|---|---|
| Workflow | 子流程、步骤交互契约、绑定 slot 约束、默认角色/能力定义和所用模板的精确修订 |
| Standard | 所有用途 slot 的 Spec 修订及合成规则，生成可验证的 StandardLock |
| Agent 定义 | 指令、Skill 引用、工作类型及角色配置修订；实例 ID 独立于定义修订 |
| Harness/Sandbox | 能力、环境、Permission 绑定、安装配方或版本及内容约束 |
| Skill/Spec/Permission 及其他可绑定模板 | 自身不可变内容及声明的全部精确依赖 |

定义发布新版本不更新其使用方。Standard v2 引用 Spec v1 时，发布 Spec v2 后 Standard v2 仍引用 Spec v1；采用新 Spec 须形成新的 Standard 修订。所有嵌套依赖同样固定，不允许 `latest`、浮动版本范围或当前项目默认值进入 Feature 配置锁。

### Project 默认绑定与 Feature 配置锁

新增 `ProjectConfigurationRevision`，包含各配置 slot 的精确修订引用、来源和解析结果。Project 保存当前修订指针，可通过具有 expected_revision 的命令原子切换 Workflow、Standard 等默认绑定，并保留原修订。继承全局默认时，在生成 Project 配置修订时完成解析；全局默认变化不会修改现有 Project 配置修订。

`feature.create` 使用明确的 Project 配置修订，允许在创建参数中选择其他合法的精确版本；完成作用域、依赖和兼容校验后，同事务保存 Feature 与 `FeatureConfigurationLock`。锁至少包含来源 Project 配置修订、WorkflowRevision、StandardLock、角色/Skill/Harness/Sandbox/Permission 的完整依赖图及摘要。创建即冻结，包括处于 draft 的 Feature。

创建时已固定的运行定义可以在后续按相同版本配方准备实际安装，但实际能力及内容必须符合锁定约束。步骤可在冻结的角色及 Harness 类型约束内选择或创建实例 ID，选择事实另行保存；不得借新建实例、覆盖参数或读取偏好改变定义版本。

Task 从所属 Feature 的配置锁派生；即使 Task 在 Project 更新默认绑定后才创建，也使用原 Feature 的版本。Attempt、Run、Session、Review、Merge 和恢复执行引用这份治理配置，另冻结自身输入与实际运行实例。技术重试和新 Attempt 均不能采用新的 Project 默认绑定。

冻结范围是治理与能力定义的绑定。需求、设计、测试、文档和 Context 继续按各自生命周期更新修订，相关证据按既有规则失效；暂停、确认、取消、合规的人类决定和资源清理仍是运行操作。

已有 Feature 需要另一套 Workflow、Standard 或能力定义时，以 predecessor 关联创建后继 Feature，采用新的 Project 配置。经验、文档及代码基线可经显式引用继承，旧工程证据不能被重标为新版本的验收结果。

### 独立演化示例

| 时间 | Project 当前默认绑定 | Feature A | Feature B |
|---|---|---|---|
| 创建 A | Project 配置 p1：Workflow w1、Standard s1 | 创建时锁定 w1、s1 及其依赖 | 尚未创建 |
| 调整默认绑定 | Project 配置 p2：Workflow w2、Standard s2 | 保持 w1、s1，包括待执行步骤和以后创建的 Task | 尚未创建 |
| 创建 B | 仍为 p2 | 保持原锁 | 锁定 w2、s2 及其依赖 |
| 发布 Workflow w3 或 Spec 新版本 | 仍为 p2 | 保持原锁 | 保持原锁 |

并发更新 Project 配置与创建 Feature 时校验创建命令指定的配置修订，冲突时拒绝并返回当前修订；同一 command_id 重放返回原创建结果，不重新解析默认值。被引用修订及必要材料保持可读取；停用定义可阻止新绑定，不删除旧锁所需内容。

## 四 Agent 定义与实例

### 定义作用域

`AgentDefinition` 沿用现有 Definition 和 AgentProfile 修订机制，使用统一 Scope 的 Global/Project 变体。Agent 类型由稳定的定义身份根及其角色类别确定；Feature 执行采用创建时配置锁中的角色修订，Session/Run 记录实际使用的该修订。独立项目级讨论在创建时固定当前适用的 Project 配置修订。

全局定义保存在全局配置库；项目定义保存在所属项目库。查询按 ID 解析归属，同名定义均显示作用域。全局配置同步不得覆盖项目定义，也不得把同名对象视为同一个定义。

### 实例身份

`AgentInstance` 至少包含 `id`、`agent_type_ref`、`harness_type`、`scope`、显示名称、创建时间、生命周期和 revision。`scope` 继承创建它的 Agent 定义作用域，并在创建后固定。

| 定义范围 | 实例保存位置 | 经验范围 |
|---|---|---|
| 全局 | 全局配置库 | 该实例 ID 在所有参与项目中的工作 |
| 项目内 | 所属项目库 | 该实例 ID 在所属项目中的工作 |

实例 ID 采用可全局唯一定位的标识；用户可设置显示名称。相同 Agent 类型和 Harness 类型允许创建多个实例。修改角色说明形成同一类型下的新修订，由新的配置绑定采用；已有 Feature 的执行始终使用其锁定修订。同一全局实例可在不同 Feature 使用各自锁定的同类型修订，每条经验记录实际版本。修改 Agent 类型、Harness 类型或作用域须创建新实例，保留显式来源关系。

Harness 继续表示具体能力集合。实例固定 Harness 类型；Feature 配置锁固定能力定义、版本、模型及工具和权限配置，执行绑定再记录实际安装和能力证明。选择已有实例时，Harness 类型由实例确定并须匹配 Feature 锁；候选能力不足时返回原因，不自动改变该 ID 的 Harness 类型。

全局实例可以在多个项目建立独立 Session。各 Session 的上下文、代码视图、工具授权和写入范围继续属于各自项目。经验读取不共享可变 Context，也不赋予源项目工作目录的写入权限。

### 历史关联

Session、Turn、Run、Snapshot 来源及工作流参与记录增加 `agent_instance_id` 或稳定关联表。一个实例拥有多个 Session；Harness 外部会话 ID 另行保存。

Feature 汇总快照可能包含多个角色，使用参与关系及来源 Session 标记贡献。继承或导入别人的快照保留来源，分别记录“本实例产生”和“本实例采用”，不将引用材料改写为自己的工作。

评审独立性继续验证现有 Agent 定义身份根及实际作者关系。新增实例 ID 不得成为同一作者绕过评审隔离的手段。

## 五 Workflow 定义与运行实例

### 领域对象

| 对象 | 职责 |
|---|---|
| `Workflow`、`WorkflowRevision` | 稳定身份、Scope、名称、不可变定义修订、精确依赖、来源和兼容版本 |
| `WorkflowDraft` | 基于已有版本或模板编辑的可变草稿，保存独立乐观锁和校验结果 |
| `WorkflowStage`、`WorkflowStep` | 阶段、稳定步骤 key、依赖、交互类型、角色要求、能力要求、输入输出和推进策略 |
| `CompiledWorkflowPlan` | 结合完整配置闭包编译的不可变执行计划，固定动作契约版本和编译语义版本 |
| `WorkflowInstance` | 固定目标项目及 Project/Feature/Task 范围、采用的修订、实际进度与暂停状态 |
| `WorkflowStepInstance` | 实际参与实例、Session、提案、Attempt/Run、Delivery、Review、Merge 和快照引用 |
| `WorkflowConfirmation` | 对指定步骤、目标版本、产物和工作流修订的人工推进决定 |
| `ProjectConfigurationRevision` | Project 动态默认绑定的不可变修订与来源 |
| `FeatureConfigurationLock` | Feature 创建时固定的 Workflow、Standard 及完整配置依赖版本 |

默认 Workflow 定义存于全局，项目可选择精确修订或派生项目 Scope 的定义，再形成 Project 配置修订。Feature 创建时固定主流程版本，Task 从其锁定的子流程定义建立实例，父流程按所需 Tasks 的实际完成事实汇合；运行事实保存在所属项目。现有项目级需求讨论创建时固定适用配置，正式 Feature 创建后通过来源引用衔接。

WorkflowStep 使用有限的类型化交互和动作：头脑风暴、澄清、方案讨论、提案生成、提案采纳、正式执行、交付、评审、合入、快照与资源处置。输入输出指定正式对象种类及确定版本。条件使用内核可校验的规则，不接受任意脚本修改业务状态。

Workflow 的步骤与底层 Run 节点分层关联。正式执行步骤解析为冻结的 Attempt/Run 计划；核心执行节点模板描述动作语义和不变量，Workflow 决定合法的编排、参与者与交互边界。不同 Feature 所锁定的定义版本应实际驱动各自的调度和可用动作。

### 定义文档与步骤契约

WorkflowRevision 的权威正文采用有 Schema 的结构化 JSON，至少包含 `schema_version`、`process_kind`、`stages`、`binding_slots`、`steps`、`transitions`、`subworkflow_refs`、输入输出声明和版本依赖。`process_kind` 区分项目需求入口、Feature 主流程和 Task 子流程，决定合法的 owner 与入口输出契约。

Stage 负责展示分组，Step 承担实际语义；步骤使用稳定 `key`，显示名称和位置可以在新草稿中修改。Transition 连接步骤端口并声明类型化条件。首版图结构支持顺序、互斥分支、并行与汇合、按 Task 集合展开子流程；普通边不形成环，返工使用显式的范围和新轮次。

| 步骤字段 | 语义与校验 |
|---|---|
| `key / stage_key / kind / contract_version` | 定位步骤、展示阶段和动作处理器；拒绝重复 key 与未知契约版本 |
| `inputs / outputs` | 定义所需对象类型及来源端口；运行时绑定精确修订、代码和 Artifact 引用 |
| `participant_slot / capabilities` | 指定角色及能力要求；实际实例 ID 在运行时分配并固化，符合 Scope 与 Feature 配置锁 |
| `interaction` | 讨论方式、目标、必答主题、工具和历史读取策略、输出 Schema、无进展/轮次上限及完成依据 |
| `guards / completion` | 依赖完成、产物已采纳、有效 Review、测试证据、目标未变化等内置条件 |
| `advance_mode` | 完成本步且条件满足后，自动进入后续步骤或等待人确认 |
| `next / outcomes / rework_scope` | 正常分支、技术失败与业务退回路径，以及受影响步骤的重新执行范围 |
| `execution_recipe / retry_policy` | 正式执行的受约束节点配方，以及允许的技术重试类别与上限 |

绑定 slot 是有类型和能力约束的参数。例如 `architect` slot 可以设置精确默认 AgentRevision，Project 在创建配置修订时可选择满足该 slot 契约的其他可见修订；所有默认引用保持精确，参数在 Feature 创建前解析完毕。Project 可独立升级兼容角色或能力定义而不改写 Workflow 正文，生成的配置锁和编译计划体现实际差异。不可覆盖的固定引用必须通过 Workflow 新修订改变。

节点能力由动作目录提供，首版包括 `agent.interact`、`proposal.adopt`、`attempt.execute`、`delivery.review`、`merge.prepare`、`merge.apply`、`snapshot.capture`、`resource.cleanup`、条件分支、并行汇合及 `tasks.foreach`。目录描述每种动作的字段、输入输出、适用 owner、执行权限与结果条件；表单和内核消费同一契约。新动作语义需要新增处理器，已有动作的合法组合通过定义编辑完成。

`agent.interact` 建立或选用符合绑定的 Session，注入步骤目标、冻结输入、自身实例 ID 和有限经验索引，支持多轮澄清与结构化 Proposal。聊天结束或 Agent 自称完成不满足工程门禁；采纳、Review 和测试结果通过正式命令及可核验事实成立。

### 草稿与发布

创建 Workflow 时确定稳定 ID 和 Global/Project Scope，再建立空白、模板或从精确已发布版本派生的草稿。草稿有独立 `draft_id`、`base_revision_id`、文档、`edit_revision` 和保存时间；支持保存不完整草稿。编辑已有版本始终生成草稿，不写回已发布正文。跨 Scope 特化创建新 Workflow 身份。

发布在所属定义库事务中完成：核对草稿 edit_revision 和 command_id，执行结构及依赖校验，生成规范化正文、摘要、唯一版本序号和不可变 WorkflowRevision，登记发布事件与幂等结果。重复发布命令返回原版本；同一草稿内容的重复提交不生成多个版本。后续编辑从新版本建立下一份草稿。

校验覆盖 Schema、步骤和端口唯一性、连通与可达性、分支默认路径、fork/join 配对、数据流类型、依赖 Scope、精确版本、子流程递归、处理器契约、返工边界及能力要求。发布只使定义可用；Project 换绑和 Feature 创建时，再结合选定 Standard、执行模式及完整配置做兼容性校验。

### 编译与配置冻结

编译器将 WorkflowRevision、解析后的绑定 slot、StandardLock 和固定依赖转换为 CompiledWorkflowPlan。计划明确每步的处理器、输入依赖、守卫、后续分支、子流程、正式 Run DAG 配方和允许动作。Standard 和内核不变量参与校验，删除必需评审或绕过 TDD 的图不能成为可执行计划。

编译缓存 key 包括定义摘要、完整配置闭包摘要和编译语义版本。同一 Workflow 版本在不同 Project 配置下可以有不同计划，均保留准确来源。Feature 创建事务引用已核验的定义副本并保存配置锁、编译计划引用和唯一主 WorkflowInstance；不在编译、准备镜像或访问供应商期间持有项目写事务。

已创建 Feature 运行时只读取其保存的计划及版本契约。Host 升级不以新编译器重新解释旧 Feature；运行器兼容已保存的语义版本，无法兼容时返回明确阻塞。修复执行器实现不能改写旧定义和已接纳事实。

### 后端持久化

SQLite 继续是唯一可写事实源。流程正文采用 JSON 保存完整结构，Scope、ID、版本、状态、外键及查询字段使用独立列；不另建一份可独立修改的节点/边存储或配置文件。JSON Schema、Rust 类型和迁移版本共同约束序列化。

| 表或记录集合 | 位置 | 关键字段与约束 |
|---|---|---|
| `workflow_definitions` | 定义 Scope 所属库 | 稳定 ID、Scope、名称、生命周期、派生来源；Scope 不可变 |
| `workflow_drafts` | 同定义所属库 | draft_id、definition_id、base_revision_id、document JSON、edit_revision、验证摘要；只编辑草稿 |
| `workflow_revisions` | 同定义所属库 | revision_id、definition_id、version、schema_version、document JSON、content_hash、published_at；唯一 `(definition_id, version)`，发布后禁止更新/删除正文 |
| `definition_revision_imports` | 消费它的项目库 | 共享修订的只读本地副本、来源 Scope、摘要及依赖闭包；保留全局身份，不作为项目定义编辑 |
| `project_configuration_revisions` | 项目库 | 不可变绑定图、内容摘要和前序修订；Project 原子更新 current_configuration_revision 指针 |
| `feature_configuration_locks` | 项目库 | feature_id 唯一、来源 Project 配置修订、Workflow/Standard 引用、完整 lock JSON 和摘要；只插入 |
| `compiled_workflow_plans` | 项目库 | plan_id、定义/配置摘要、编译及动作语义版本、plan JSON；内容不可变，可由多个相同配置的实例引用 |
| `workflow_instances` | 项目库 | owner、parent_instance_id、workflow_revision_id、configuration_lock_id、plan_id、状态、乐观锁；每 Feature 唯一主流程 |
| `workflow_step_instances` | 项目库 | instance_id、step_key、round、状态、input_lock、output_refs、时间及乐观锁；新轮次创建新记录 |
| `workflow_step_links` | 项目库 | 步骤到实际 Session/Proposal/Attempt/Run/Delivery/Review/Merge/Snapshot 的引用及用途；验证同项目和版本归属 |
| `workflow_task_groups / members` | 项目库 | 来源 TaskPlan 修订、generation、成员摘要、确定 Task ID 与子流程 ID；一代成员集合不可原地改变 |
| `workflow_confirmations` | 项目库 | 步骤轮次、计划/输入/产物摘要、目标、决定、主体及时间；保留原决定，失效另行记录 |
| `workflow_action_intents` | 项目库 | 步骤轮次、确定 action_key、冻结命令、派发/核对状态与关联 operation；action_key 唯一 |

依赖和引用可建立规范化索引用于查询与校验，由权威 JSON 同事务派生；索引不提供独立写入口。全局定义修订在解析时只读取一次并核验摘要，项目内保存所需精确副本，随后 Project 换绑和 Feature 创建只在各自项目事务中提交，不假设跨 SQLite 原子事务。

SQL 同时保护不可变修订与锁的 update/delete 边界，不能只在 UI 禁用按钮。当前通用 `save_table` 使用 upsert，实施时对这些集合采用 insert-only 保存路径及内容一致性检查，避免正常持久化流程覆盖已发布版本。Runtime 状态的更新保留乐观锁和追加事件，接入现有 `EngineeringState`、加载/保存、迁移和备份恢复链。

发布及配置命令使用所属库的现有 operations 幂等记录；运行命令使用项目库幂等范围。工作流事件复用项目 events，不要求引入全量事件溯源。Artifact 可保存校验后的导出正文与图形，但不承担第二套可写状态。

### 推进策略与修改生效

每个步骤设置 `advance_mode = automatic | human_confirmation`，表示其输出和所有门禁满足后，是否等待人确认再向后推进。准备下一步和启动外部执行仍须检查该步骤自己的依赖、能力、额度与执行范围。

1. 自动模式：实际产物和门禁满足后，由 core 持久化推进意图，幂等创建下一步。
2. 等待人确认：满足条件后进入 `waiting_confirmation`，展示本步结果、后续动作与阻塞理由；确认绑定精确产物及 revision。
3. 头脑风暴或澄清需要人的回答时，无论推进模式如何均等待实际输入。自动模式不代替提案采纳、正式 Review、工具审批或其他必需决定。
4. 修改结构、交互或推进策略均产生新 Workflow 修订；Project 显式切换默认绑定后，之后创建的 Feature 采用新修订。仅发布定义不会改变 Project 绑定。
5. 已有 Feature 的运行实例、待执行步骤、等待确认边界和以后创建的 Task 全部保持原修订。移除将新修订应用到在途 Feature 的领域操作、API 和 UI 入口。
6. 当前步骤按原策略执行确认、暂停、恢复或取消。需要改用新流程时创建关联的后继 Feature；不得通过新建 WorkflowInstance、新 Attempt、恢复 Session 或 runtime 覆盖修改旧 Feature 的配置锁。
7. 输入或产物变化使旧确认失效。暂停期间不派发新步骤，恢复后重新验证；失败、取消、未知回执均不会因自动模式被视为成功。

`advance_mode` 作用于本步骤完成之后的推进边界。需要在合入前等待人确认时，确认设置在“候选验证和评审已完成 → 应用合入”的边界，不能在实际合入之后才询问。人工决定绑定候选 SHA、目标基线及固定交付；确认后目标变化必须重新验证。若一次提案采纳或人工 Review 同时满足本步骤的推进确认，组合命令可同事务登记两个各自类型化的决定，UI 不要求重复点击相同意思的批准。

### 状态与调度

步骤状态至少区分 pending、ready、active、waiting_input、waiting_confirmation、blocked、completed、failed 和 cancelled。状态语义区分工作未完成、推进待确认与工程门禁阻塞。

调度读取工作流实例及权威工程事实；Run/Turn 完成、提案采纳、Review 和 Merge 接纳触发重新求值。直接工程命令同样校验工作流契约；前端只能提交命令，不能按 Feature.status 自行推进。

采用确定的步骤执行 key、command_id 和事务内事件关联，避免 SSE 重复、调度重试或崩溃恢复产生重复 Session、Attempt 或 Merge。跨库操作保持先登记配置、后在目标项目采用的顺序；可重建索引不参与工程业务提交的原子性判断。

### 工作流求值与执行器

核心求值器是无 I/O 的 `evaluate(plan, instance, engineering_facts) -> decisions`，返回可执行动作、等待输入/确认、阻塞原因和状态转换。条件通过有限的类型化表达式组合，如 `all/any`、产物已采纳、交付已接受、测试有效及目标基线未变化，不执行用户脚本或依赖模型判断终态。

每轮调度按以下顺序处理：

1. 接纳并核对原生执行、交互、测试、Review、Merge 等已持久化事实及结果未知操作。
2. 对受影响流程求值，核对步骤轮次、输入版本、配置锁、暂停状态、依赖和门禁。
3. 在短事务内保存状态及事件，创建唯一 action intent 和确定的领域 command_id。
4. 事务外由处理器提交原领域命令；Session/Attempt 等创建结果关联步骤，外部执行仍通过既有持久化意图、worker 和核对协议进行。
5. 结果接纳后再次求值；单轮求值设置推进预算，避免无外部等待的控制节点长期占用调度器。

动作 intent 与原生 Run/Effect/RepositoryOperation 的职责分别保存：前者防止重复创建领域操作，后者证明供应商调用和回执。若派发后进程退出，使用原 command_id 取回已有领域操作，不新建执行；已消费且结果未知的供应商请求先核对，不能声称外部 exactly-once。

接入 `Workspace::tick` 和现有全局/Project 并发配额；修改 `EngineeringStore::has_pending_work`，包含可推进的 workflow intent、ready 步骤及新事实待求值标记。仅等待用户或确认的流程不持续热轮询；回答或确认在项目事务内设置唤醒标记。应用重启发现未完流程并恢复求值，浏览器关闭和切换项目不终止后台工作。

核心步骤状态与 Feature/Task 业务状态分别表达。Feature ready、Task accepted、Feature completed 等仍由正式交付及合入事实接纳；完成一个工作流节点不能直接设置这些终态。`allowed_actions`、推荐角色/能力和 blocked_reasons 从同一计划及求值结果产生，避免编辑器、UI 和直接 RPC 使用不同流程规则。

### Task 展开与返工

`tasks.foreach` 从已采纳的精确 TaskPlan 及其 task_instances 映射创建 TaskGroup，固定所需 Task ID、依赖和子流程版本。只要依赖满足就可并行启动 Task 子流程；同一 Session 的可变上下文不并发使用，worktree 写入和同目标 Merge 保持原互斥约束。

Join 等待该 TaskGroup 中全部必需成员的有效完成及 Task → Feature 合入事实，不能查询“当前 Feature 下所有 Task 的状态”代替。可选任务策略在定义中明确；未纳入计划的临时 Task 不改变该代汇合条件。

需求或计划被重新采纳时创建新的业务输入代和受影响执行轮次，按依赖图使证据失效；不修改 WorkflowRevision。Task 集合变化须经新 TaskPlan 采纳并创建新 TaskGroup generation，暂停旧集合的后续派发、确认旧 writer 已停止，再激活新集合。已合入 Task 保留不可变结果，只有依赖版本兼容且正式核验后才能作为新计划输入；否则使用新任务或后继 Feature。

Review 退回生成明确关联的修复步骤轮次及新 Attempt，目标分支变化只重建受影响的 Merge 候选。技术 Run 重试保持原步骤输入和配置锁。每个重做动作由 `(workflow_instance_id, step_key, round, member_id)` 及 action kind 确定去重 key；超过重试/返工自动预算时停止等待处理。

### 证据有效性与现有代码改造

新增 `StageInputLock`，明确 Requirement、Design、TaskPlan、Test、代码/候选、父交付及配置锁的实际依赖和摘要。input_version 仍可作为对象变化序号和乐观提示，但不独自决定所有阶段证据是否有效。更新 Design 不使未变化的 Requirement Review 失效；修改 Requirement 必须使引用它的 Design、Task 范围、Test 和后续交付按依赖规则失效。

W3 将现有阶段匹配拆成两部分：固定动作语义留在处理器中，编排、角色选择和推进来自 CompiledWorkflowPlan。具体替换 `core/lib.rs` 的 phase DAG/规划前置/allowed_actions、`core/live_runs.rs` 的固定角色映射，以及 `host/workspace.rs` 选择内置角色 latest 的逻辑。派发时从步骤计划读取精确版本，旧 phase 入口仅做受同一门禁约束的兼容映射。

`attempt.start`、`proposal.accept`、`review.request/submit`、`merge.request/apply` 等既有命令在关联流程时均验证对应步骤、输入锁和动作授权。独立只读浏览和不产生正式结果的补充讨论有明确上下文；一旦采纳为工程输入，就走正式关联和失效规则。不存在外部提交任意 `step.completed` 或 `set_status` 的入口。

## 六 默认 Feature 工作流

保留三大阶段，并在阶段内部定义人与 Agent 的交互。以下推进方式为初始预设，可在工作流定义中修订；人工 Review 等必需决定另按领域契约验证。

| 阶段 | 步骤与预期产出 | 默认角色 | 默认推进方式 |
|---|---|---|---|
| Feature 规划 | 建立 Feature/worktree、了解代码、建立 Context、初始快照 | 需求分析师、架构师 | 自动 |
| Feature 规划 | 头脑风暴、澄清目标和边界、形成需求草案 | 需求分析师 | 等待人确认 |
| Feature 规划 | 需求正式交付与 Review，保留明确验收条件 | 需求分析师、独立评审 | 自动，受 Review 门禁约束 |
| Feature 规划 | 方案讨论、形成并采纳 Design | 架构师 | 等待人确认 |
| Feature 规划 | Task 分解、依赖和 Test 计划讨论及采纳 | 架构师、测试 | 等待人确认 |
| Feature 规划 | 规划交付及 Review、锁定基线快照，进入 ready | 架构师、测试、独立评审 | 自动，受 Review 门禁约束 |
| Task 并行开发 | 创建各 Task worktree，继承基线快照，独立绑定实例 | 开发、测试 | 自动 |
| Task 并行开发 | test_author → Red → implement → Green/docs → verify | 测试、开发 | 自动 |
| Task 并行开发 | 交付报告与快照、Task Review、修复的新 Attempt | 开发、独立评审 | 自动，失败等待处理 |
| Task 并行开发 | Task → Feature：候选、验证、Review、rebase + squash | 测试、独立评审 | 自动，受合入门禁约束 |
| Feature 收尾 | 所需 Tasks 完成后，完整测试、Docs、验证及交付 Review | 测试、开发、独立评审 | 自动 |
| Feature 收尾 | Feature → mainline：候选、验证、Review、远端核对 | 测试、独立评审 | 等待人确认 |
| Feature 收尾 | 最终快照、报告、资源释放和按需归档 | 当前参与实例 | 自动；归档采用既有能力 |

评审失败进入显式修复路径并保留旧 Attempt；目标基线变化重建受影响候选。自动技术重试有明确次数和故障类别，耗尽后停止并显示阻塞原因。

需求与设计讨论关联正式采纳记录。重整现有真实规划前置条件时，将全量产物的一次性检查分解为步骤契约与派发检查；每个正式 Attempt 继续冻结输入，新采纳内容引起输入变化时按失效规则新建 Attempt。步骤可跨多个 Attempt 引用有效交付，避免重复规划与讨论相互等待。

## 七 角色经验检索与场景恢复

### 检索入口与筛选

角色讨论先选定实例 ID，然后展示经验范围。全局实例默认提供跨参与项目的历史；项目实例固定所属项目，并明确显示作用域。

主要筛选项在界面和 API 中均为第一等字段：项目名称、Feature 名称、时间范围。名称用于查找和展示，选择后使用稳定 Project/Feature ID 查询；同名项显示项目和 ID，改名后仍能通过捕获时名称定位旧记录。Task、阶段、产物、捕获原因和结果状态为附加筛选。

时间默认以快照捕获时间过滤，采用明确的起止边界并按客户端时区显示；同时展示实际工作起止时间。支持按工作发生时间另行筛选，不用快照时间替代实际执行时间。分页使用捕获时间和稳定 ID 排序，筛选条件进入路由并在返回时恢复。

### 渐进式披露

| 层级 | 返回内容 | 读取方式 |
|---|---|---|
| 工作索引 | 实例 ID、项目/Feature/Task、时间、步骤、简短摘要与可用性 | 默认列表，游标分页 |
| 工作摘要 | 目标、关键决定、产出、未决问题与来源引用 | 选择一项后读取 |
| 快照详情 | 冻结 Context、Session、运行绑定、文档版本与重建清单 | 按快照读取 |
| 原始证据 | 讨论轮次、工具记录、代码、测试、Delivery/Review/Merge、Artifact | 显式展开或由 Agent 按需查询 |

Agent 在新讨论中获得自身 ID、作用域和有限摘要，并能通过工具查询上述层级。Codex 与 DSH 适配器均需提供同等语义的只读经验查询能力，并在真实运行中验证；查询不允许由模型伪造当前实例身份或越过项目实例边界。

首版采用结构化筛选和可校验来源的文本检索，不依赖向量服务。摘要属于可重建投影，保存生成方式、来源和版本；查询显示摘要截断或原始记录缺失的实际边界。

### 存储与跨项目读取

1. Session、Snapshot、Artifact 正文和工程事实保留在来源项目库；项目实例的经验索引保存在该项目。
2. 全局实例在全局库拥有可重建的经验目录，记录来源项目、快照、参与关系、筛选元数据和摘要引用。来源项目事实是权威来源。
3. 通过项目内 outbox/事件序号增量更新目录，保留水位、去重 key 和重建入口。索引延迟、缺失项目、归档或磁盘离线返回明确状态和已覆盖范围。
4. Host 的经验查询协调器只读打开指定来源项目并校验 ID、作用域、引用及哈希，不切换当前活动项目、不启动该项目的 worker、不改变工程请求的 active_project 规则。
5. 来源改名、目录迁移或归档更新可重建定位；不可用材料保留摘要和来源，读取失败不从当前项目同名对象补取。

### 场景恢复

提供两个有明确目标的动作：在来源项目从快照恢复工作场景；在当前项目引用历史经验继续讨论。前者定位来源 Feature/Task 和工作流步骤，再检查代码与环境重建条件；后者导入选定经验摘要及引用，保持当前项目的代码、目标和工作流归属。

两者均创建新的 Session 或后继执行，记录来源快照、源实例、目标实例、目标步骤和已采用材料。跨项目引用使用专门的只读导入引用，不改变旧 Snapshot 的 owner，也不放宽现有 predecessor 的同项目规则。全局实例可保持同一 ID；项目实例的恢复和历史读取始终限定所属项目。

原生 resume 仅在供应商能力及版本经验证时使用。材料缺失时提供可用范围，不能声明已恢复完整环境。恢复历史上下文不回滚当前工程状态，也不复用已经失效的 Review、测试或人工确认。恢复到已有 Feature 时沿用其配置锁；历史经验引用只作为输入材料，不能导入其他 Feature 的治理配置。创建后继 Feature 则记录新锁与来源锁。

## 八 前端信息架构与交互

### 导航和地址

左侧依次为工作总览、执行与交付、角色讨论、工作流程、规范与身份、历史记录、设置。新执行页内部使用 `execution`，工作流程页使用 `workflows`；保留旧入口的兼容定位。

地址包含 project_id、feature_id、task_id、详情页签和可选的 Run/Delivery/Snapshot/实例/步骤 ID；列表筛选及分页可恢复。直接打开地址通过 ID 读取目标，不依赖其在当前 50 条分页内。跨项目跳转采用正式项目打开流程并重置过期请求；只读经验浏览保持当前工程项目。

### 工作总览

指标由项目汇总查询提供，显示统计范围和有效筛选，不使用详情当前页长度推算全量计数。Feature、Tasks、运行中、待评审、待人确认和阻塞事项都可跳转；聚合卡片打开筛选列表，具体条目直接定位对象及相关页签。

### 执行与交付

采用 Feature/Task 对象导航和当前对象详情。桌面界面保留对象列表，窄屏折叠为选择入口；详情页顶部展示对象状态、当前工作流步骤、参与实例、下一步动作和阻塞原因。Feature 配置详情只读展示锁定的 Workflow/Standard 及依赖版本、来源 Project 配置修订与当前 Project 默认差异；提供查看版本及创建后继 Feature 的入口。

| 页签 | Feature 内容 | Task 内容 |
|---|---|---|
| 总览 | 需求、阶段进度、Task 依赖与交付范围 | 冻结范围、依赖、当前步骤与继承基线 |
| 执行 | Feature 自身及 Tasks 的执行筛选、Attempt/Run、日志和交互请求 | 本 Task 的 Attempt/Run、绑定与日志 |
| 交付与评审 | 规划、Task 汇总、收尾交付及合入 | 本 Task 交付、Review、Merge 和来源证据 |
| 文档 | 需求、设计、Feature Docs、Task 产物引用和修订历史 | 本 Task 产物及实际引用的 Feature 文档 |
| 上下文 | Feature Context、快照及相关 Task 上下文入口 | 独立 Context、继承和最终快照 |
| 测试 | Feature 测试、Task 验收覆盖汇总和结果 | Task 测试定义、TestRecipe、Red/Green 和候选验证 |

文档的 Feature 归属与 Task 来源同时保留；Task 不复制一份可独立漂移的 Feature 文档。测试页区分 Test 定义与 TestResult，显示定义修订、代码版本、环境、来源、执行时间及证据有效性。

详情摘要与正文按需读取，长历史使用独立分页。高信息量页面通过页签、摘要和展开详情逐层呈现，保持待处理动作可见。

### 角色讨论

页面包括当前工作入口、实例选择、对话主体及可展开的经验面板。显示实例 ID、Agent/Harness 类型与作用域；类型与实例创建入口放在规范与身份，讨论页允许快捷选择或创建符合要求的实例。

推荐由选定 Feature/Task 的工作流步骤、交互状态和能力需求决定，并返回理由。多个步骤并行时按当前对象列出候选，允许显式选择；已运行 Session 保持原实例和绑定，不因推荐更新而切换。

Feature 创建前，按步骤显式设置、角色、项目和全局偏好解析配置，无配置时默认 Codex，并将结果纳入配置锁。Feature 内讨论及执行只读取这份锁；选定已有实例后固定其 Harness 类型，并验证所需能力。默认实例选择顺序为步骤已分配实例、对象已有合适实例、用户选择/新建，不凭名称合并。

经验面板始终突出项目名称、Feature 名称和时间筛选。选择结果可查看快照、插入有来源的经验引用、定位来源工作或恢复场景。对话显示当前步骤目标、期望产物及采纳后的下一步；正式执行产生的历史也进入同一实例履历。

### 工作流程

初版使用阶段列表、流程预览和步骤表单，可编辑顺序与依赖、角色和能力要求、交互类型、产物契约、推进方式及适用范围。页面展示定义校验结果、版本差异、采用该版本的实例和变更影响。

支持编辑内置流程的派生版本并切换 Project 默认绑定。区分保存定义修订与修改 Project 绑定两个动作，显示将采用新版本的后续 Feature 范围；已有 Feature 保持只读绑定。内置固定规范显示约束及原因。首版不要求自由画布或通用 BPMN 引擎。

### 规范与身份及历史记录

规范与身份分别管理全局/项目 Agent 定义、Agent 实例、Harness 能力、Standard/Spec、Skill、Sandbox、Permission 和运行配置，统一显示 Scope、定义身份、具体版本和引用关系。Project 配置界面支持精确版本选择、默认绑定修订与差异预览；Feature 页面仅展示所用版本。实例详情链接自己的工作履历；定义修订与实例身份分别展示。

历史记录提供工程事件及工作流推进、人工确认、定义发布、Project 绑定变更、Feature 配置冻结、实例关联、经验导入和恢复记录。角色经验是历史的特定投影，不建立第二套可修改的工程事实。

## 九 接口与模块边界

以下为目标契约分组，W0 固定最终名称和 Schema：

| 分组 | 目标能力 |
|---|---|
| Scope 与版本 | 各类定义的作用域、稳定身份、精确修订、依赖及可见性查询和校验 |
| 配置绑定 | Project 默认绑定查询与修订；创建 Feature 时解析及原子冻结；Feature 锁和依赖只读查询；拒绝创建后重新绑定 |
| 定义与实例 | 全局/项目 Agent 定义查询与修订；实例创建、查询、归档；固定类型与作用域校验 |
| Workflow | 定义查询、修订、校验及 Project 精确默认选择；实例与步骤读取；按锁定版本确认、暂停/恢复 |
| 建议与动作 | 返回当前对象的可处理步骤、推荐类型和实例、能力要求、理由及阻塞信息 |
| 经验 | 按当前实例 ID 查询项目/Feature/时间维度、索引、摘要、快照与证据；读取范围和游标分页 |
| 恢复与引用 | 材料检查、经验引用导入、来源场景恢复和新 Session 创建 |
| 工作台 | 项目汇总、对象定位与独立分页、Test 定义/结果/覆盖、上下文和文档读取 |

工程命令携带 command_id、目标项目、expected_revision 和适用的 workflow_step_instance_id。创建 Feature 的命令明确 Project 配置修订，后续命令引用 Feature 配置锁。Host 解析身份和能力，core 验证权威条件，adapter 执行外部 I/O 并回传可核验结果。全局和项目命令使用各自明确的幂等范围。

建议新增 `domain/scope.rs`、`domain/configuration.rs`、`domain/workflow.rs`、`domain/agent_instances.rs` 及 core 对应模块；经验检索抽象读端口，跨项目协调由 Host/adapter 实现。治理配置锁与需求验收 TaskScope、实际 ExecutionScope 分别验证。Web 拆出共享 RPC、路由、页面和 Feature/Task 详情组件，避免继续向 `main.tsx` 集中业务代码。

## 十 迁移与兼容

1. 顺序追加全局和项目 SQL 迁移，实施时使用最新空闲编号。保留现有数据，不修改已发布迁移。
2. 已有共享配置定义按当前全局保存位置登记 Global Scope；项目内复制的共享修订保留其全局来源，不误判为项目特化。新增项目定义与 Scope 入口后按明确归属存储和解析。
3. 新 Session 必须绑定明确实例 ID。旧 Session 缺少实例身份时保留 legacy；可按可证明的 predecessor 和相同类型绑定建立关联，否则每条独立会话链建立隔离的 legacy 实例映射。不依据相同角色/Harness 推断跨项目同一实例。
4. 迁移映射采用可追溯的附加记录；旧快照、Artifact 字节、哈希和已冻结输入不改写。无法确定时间或来源时显示未知，不用当前配置倒填。
5. 为 Project 建立默认配置修订；为已有 Feature 按其可证明的持久化绑定、历史 Spec/运行锁和实际执行语义建立不可变 legacy 配置锁及 Workflow 映射，不读取当前 Project 默认来推测原配置。明确标记为升级时保留的有效配置，不伪称已记录原创建时完整配置。缺失条目保持 `legacy_unresolved`，禁止依赖该条目的新执行；补录只采用可证明的原事实。升级不改变推进方式或触发新的自动动作，需要新治理版本时建立后继 Feature。
6. 保持真实/模拟来源、权限锁、资源清理和 Review 身份语义。旧 RPC 客户端仍通过相同 core 门禁；适配层解析 legacy 关联，不允许省略工作流引用绕过约束。
7. 全局目录建立失败可以按项目 outbox 重建；项目库不可用不阻塞其他项目本地工作。重启核对活动执行、未完成确认和推进意图，未知消费结果不得自动重发。
8. 审计 `binding.update`、`project.update`、`session.open`、正式执行和恢复入口：移除对已创建 Feature/Task 治理配置的更新及运行时重新读取最新偏好的路径。旧客户端请求明确返回配置已冻结错误；不通过兼容转换暗中重新绑定。项目默认变更本身不使旧 Feature 证据 stale。

## 十一 实施阶段与完成条件

| 阶段 | 交付范围 | 依赖 | 完成条件 |
|---|---|---|---|
| W0 Scope 及领域契约 | 固定 Scope、定义/修订/绑定/配置锁语义、身份边界、Workflow/步骤、初版流程、接口及 UI 草图 | 本计划 | 配置矩阵与依赖规则完整；需求讨论到正式执行无互相等待；保存 `CONTRACT.md` |
| W1 Scope 版本与绑定持久化 | 通用修订及精确依赖、Global/Project 定义、Project 配置修订、Feature 创建时锁、实例及历史关联、迁移 | W0 | Project 可换绑；已有 Feature 及以后创建的 Task 不漂移；同 ID 身份不变，经验范围正确；依赖可独立演化 |
| W2 Workflow 定义和实例 | 版本校验、内置种子、Feature/Task 父子流程、步骤状态、配置锁引用 | W0、W1 | 定义可按 Scope 解析并实例化；非法依赖/能力/契约被拒绝；Workflow 与 Standard 从创建起固定 |
| W3 工作流驱动交互及执行 | 讨论/提案关联、正式 DAG 解析、冻结版本下的建议、自动/人工推进、重试与恢复 | W2 | 两个版本的 Feature 各按原策略完整推进；直接 RPC 同受约束；重复事件/重启不重复执行；所有旧锁保持不变 |
| W4 经验检索和恢复 | 分作用域的历史目录、三项主要筛选、逐级读取、Agent 工具、引用导入和场景恢复 | W1，恢复绑定依赖 W3 | 全局实例跨项目按 ID 检索；项目实例仅本项目；Codex/DSH 可实际按需读取；来源与恢复语义完整 |
| W5 工程工作台 | 导航、稳定地址、总览汇总、Feature/Task 详情、只读配置版本、文档/上下文/测试整合 | W2、W3，经验入口依赖 W4 | 卡片精确定位；刷新/返回保留地址；全部执行及交付按对象查看，Feature 显示原绑定和默认差异 |
| W6 工作流及配置页面 | 定义 Scope/版本编辑、Project 默认换绑和历史、角色推荐与经验面板、身份入口 | W3、W4、W5 | 可修改定义并更新 Project 默认；新 Feature 生效，旧 Feature 无升级入口；按锁选择实例并检索经验 |
| W7 迁移与完整验收 | 历史升级、真实/模拟闭环、故障恢复、Web 验收和文档更新 | W1–W6 | 验收矩阵通过，报告明确实际平台与能力；证据保存于本目录 |

按阶段串行落实关键依赖，每阶段完成后更新执行记录和证据索引。实现只修改相关模块，保留工作区中其他在途变更。

## 十二 验收矩阵

| 编号 | 场景 | 必须观察到的结果 |
|---|---|---|
| A1 | 同 Agent/Harness 类型创建两个实例 | ID 不同，各自检索只返回自己的工作；类型/作用域变更被拒绝 |
| A2 | 全局实例在两个项目参与多个 Feature | 同 ID 的履历覆盖两项目；项目名称、Feature 名称和时间可组合过滤，正文按需读取 |
| A3 | 项目 Agent 与全局 Agent 同名 | 显示作用域与 ID，项目实例不读取或参与其他项目；名称不合并身份 |
| A4 | 项目/Feature 改名、同名及时间边界 | 稳定 ID 保持关联，历史名称可定位；筛选、游标和时区边界一致 |
| A5 | 真实 Harness 访问经验 | 先取索引再取快照和证据，记录工具调用与引用；当前实例身份及项目范围无法由模型参数伪造 |
| A6 | 新 Feature 进入需求讨论 | Workflow 推荐需求分析师及匹配 Harness，说明理由；需求、方案和计划采纳衔接正式交付 |
| A7 | 自动推进与人工推进 | 自动步骤满足全部门禁后只推进一次；人工步骤等待精确版本确认；必需 Review/回答仍实际发生 |
| A8 | 修改推进方式并更新 Project 默认 | 新 Feature 采用新版本；旧 Feature 的待执行和等待确认步骤保持原策略；试图重新绑定被 core 拒绝 |
| A9 | 输入变化、Review 退回、目标基线变化 | 旧确认/证据按规则失效；创建修复 Attempt 或新候选，无错误完成和无界自动重试 |
| A10 | Task 并行及 Feature 汇合 | 各 Task 独立上下文和 worktree；汇合只依据所需 Task 的权威完成事实，候选推进遵循既有串行约束 |
| A11 | 总览跳转与分页外对象 | 项目汇总正确，卡片可跳转筛选结果并定位目标；直接地址、刷新、返回和跨项目切换可用 |
| A12 | Feature/Task 文档、上下文及测试 | 归属和引用正确，测试定义与结果分开；显示 Red/Green、代码/环境版本及 stale 状态 |
| A13 | 来源场景恢复与跨项目引用经验 | 新 Session 有明确来源；当前代码和工作流不被历史覆盖；材料缺失显示范围，正文 hash 校验有效 |
| A14 | 重启、重复命令及索引更新中断 | 不重复外部动作，不重发已消费未知输入；目录可重建且不会丢失项目权威事实 |
| A15 | 旧数据升级与旧入口兼容 | 不合并无法证明的身份，不改 Artifact/hash，不自动启动旧工程；旧客户端无法绕过门禁 |
| A16 | 项目离线、归档或清理后的历史查询 | 摘要、来源和可用性可见；完整性边界明确，其他项目工作继续可用 |
| A17 | Scope 与定义可见性 | 全局定义可绑定不同 Project，项目定义仅限所属项目；全局引用项目定义或跨项目依赖被拒绝；定义 Scope 和目标 Scope 分别保留，同名对象不覆盖 |
| A18 | Workflow 与 Standard 默认换绑 | Project p1/p2 均可追溯，旧 Feature 锁定 w1/s1，新 Feature 锁定 w2/s2；draft Feature 也禁止修改 |
| A19 | 嵌套配置独立演化 | 发布 Spec/Skill/Permission 等新修订不改变原 Standard/Agent/Harness 修订及已有 Project/Feature 引用；完整依赖可验证 |
| A20 | 旧 Feature 后续 Task 与新执行 | Project 换绑后，新 Task、新 Attempt、新 Session、候选验证和恢复均使用旧 Feature 配置，禁止通过覆盖参数绕过 |
| A21 | 并发换绑与 Feature 创建 | 创建校验指定 Project 配置修订，冲突无半成品；幂等重放返回原结果，Feature 与配置锁同事务提交 |
| A22 | 停用版本与后继 Feature | 原修订及依赖仍能解析，已有工作不受新默认影响；后继 Feature 使用新锁并明确继承来源，旧证据不被重标 |

验证分层执行：domain/core 验证身份、状态、版本和门禁；adapter/Host 验证多库、索引、幂等和恢复；HTTP 契约验证接口和项目边界；Playwright 验证导航、筛选、编辑与对象完整流程。新增测试围绕真实业务约束，避免只复述组件实现。

实施阶段运行相关定向用例和 Web 构建，最终运行 `cargo fmt --all -- --check`、`cargo test --workspace --locked`、`npm --prefix web run build` 及 `./scripts/check.sh`。真实 Codex/DSH、Git/worktree、测试、Review 和 Merge 闭环采用既有 `scripts/check-live.sh` 体系补充案例，分别记录实际平台和外部资源结果。

## 十三 交付物与范围

本目录保存本计划、`CONTRACT.md`、`EXECUTION.md`、验收报告、截图及运行证据。同步更新领域、ADR、协议、Schema、使用说明及计划索引。阶段完成以行为及证据为准。

首版包含统一 Scope、配置独立版本与精确依赖、Project 动态默认绑定、Feature 创建时配置冻结、结构化工作流编辑及推进策略版本化、实际调度与交互约束、分作用域实例经验、跨项目只读查询、渐进式披露、场景恢复和完整工作台。跨账号经验共享、远程多租户、通用脚本/BPMN 编排与向量基础设施留待独立需求；已有 Feature 的治理配置重新绑定不属于本方案允许的操作。
