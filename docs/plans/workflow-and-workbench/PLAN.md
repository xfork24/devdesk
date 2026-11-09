# Workflow 领域与工程工作台调整实施计划

创建日期：2026-11-08。更新日期：2026-11-09。状态：已实施并完成本轮验收，实际结果及首版边界见 EXECUTION.md。范围：Scope 与配置版本及绑定、Workflow 核心领域、Agent/Model/Harness/Skill/Tool/Permission 组合与 AgentID、上下文检索和恢复、前端信息架构、数据迁移及验收。

以 Workflow 统一驱动软件开发与人和 Agent 的交互，以 Feature 和 Task 组织全部执行及交付信息。Agent 是 Model、Harness 和一组 Skills、Tools、Permission 的版本化组合，包含角色职责与行为指令；AgentID 唯一标识采用某个确定 AgentRevision 的实例。实例独立绑定 Global 或 Project Scope，分别跨项目或仅在所属项目积累经验。项目名称、Feature 名称和时间是经验快照的主要筛选条件，具体组合与实际模型信息保留在证据中。

Scope 统一表达全局、Project 和 Feature 的边界。Workflow、Standard 等配置定义通过不可变修订独立演化，Project 可动态调整默认绑定；Feature 从创建时即冻结所采用的全部配置及依赖版本。Project 后续绑定变化只作用于之后创建的 Feature，已有 Feature 的未执行步骤、新 Task、新 Attempt 和新 Session 均沿用原配置。

默认工作流依据 [架构设计第 14 章](../devdesk-v0.2/PLAN.md#14-典型-feature-开发流程)，结合现有正式交付、评审、TDD 和真实执行契约细化。本文中的目标类型、接口名称和阶段编号用于实施设计，最终协议同步更新到 `docs/domain.md`、`docs/design.md`、`docs/protocol.md` 和 `schemas/`。

## 一 已确定的产品行为

1. 左侧导航顺序为：工作总览、执行与交付、角色讨论、工作流程、规范与身份、历史记录、设置。
2. 执行与交付是主要工作页面。文档、Context、Snapshot 和 Test 纳入对应 Feature 或 Task，取消独立的文档与上下文顶层入口。
3. 工作总览的卡片和列表可以进入执行与交付，并定位 Feature、Task、执行、交付或待处理事项；多条结果进入带筛选条件的列表。
4. 文档以 Feature 组织，保留 Task 来源及引用。Context 和 Snapshot 绑定实际 Feature、Task 或已有项目讨论范围；Feature 与 Task 都能查看对应测试定义和执行结果。
5. Workflow 是具有稳定身份和不可变修订的核心领域对象，定义阶段、参与者、能力需求、交互过程、产物契约、推进条件及异常路径。
6. 每个步骤的推进方式可配置为自动推进或等待人确认；修改形成 Workflow 新修订，由 Project 调整默认绑定后用于后续 Feature。已有 Feature 的推进策略保持创建时版本。
7. Agent = Model + Harness + 一组定义好的 Skills/Tools/Permission，角色职责和行为指令属于 Agent 的配置。AgentID 唯一标识具体实例，固定到一个不可变 AgentRevision；相同组合允许有多个独立 AgentID。
8. AgentID 的 Global/Project 绑定决定经验累积范围，独立于定义的保存位置。全局 Agent 定义可创建全局实例或项目实例；项目定义仅在其可见范围内创建项目实例。历史始终按 AgentID 归属，不按角色、Harness 或 Model 名称合并。
9. 人和 Agent 都能渐进式读取实例历史：先获得索引和摘要，再展开某次工作的快照和原始证据；支持时间过滤、定位和场景恢复。
10. Feature/Task worktree、TDD、独立评审、冻结输入和合入策略继续由领域内核约束。Workflow 配置通过这些约束校验。
11. Scope 是统一的核心领域值对象；定义归属、绑定目标和运行事实归属分别使用有明确职责的 Scope 字段。配置定义的作用域与其使用目标分别记录。
12. 所有可绑定的配置定义具有稳定身份和不可变版本，依赖精确引用；Project 默认绑定具有独立修订，Feature 创建时形成完整配置锁，之后禁止重新绑定。

## 二 实施前基线与改造入口

| 入口 | 当前实现 | 改造目标 |
|---|---|---|
| `web/src/main.tsx` | 页面、选中 Feature、Task 过滤与详情集中在一个组件；执行页内部键为 `workflow` | 拆分页面与对象详情，建立稳定路由；执行页与工作流定义页使用不同标识 |
| 工作总览 | 部分指标来自当前 Feature 已加载的详情页 | 按明确范围独立汇总，导航目标覆盖未加载的 Feature 和 Task |
| `web/src/components/RoleConversation.tsx` | 按 owner 查询 Session，角色、Harness 与模型分开选择 | 按步骤选择完整 AgentRevision 与兼容 AgentID，展示 Model/Harness 组合和实例经验，取消同 ID 下临时换模型 |
| `crates/domain/src/agents.rs` | AgentProfile 定义角色，Definition 修订链用于身份判断 | 将角色/指令作为 Agent 组合属性，增加 Model/Tool 定义及 AgentRevision/AgentInstance，保留作者与评审来源关系 |
| `crates/domain/src/lib.rs` | Session 记录 Agent 修订和运行时锁，Snapshot 记录捕获来源 | 增加实例及工作流关联，保留原历史锁与来源，不把 Session ID 作为实例 ID |
| `RuntimeBinding.model`、运行偏好及 `HarnessProvider` | 模型为运行绑定中的字符串，provider 主要标识 Codex/DSH Harness | 提升 Model 为独立版本化定义，区分模型提供方与 Harness 类型；Agent 组合整体绑定，运行时记录声明及实际 Model |
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
| Agent 实例的 `scope` | AgentID 的归属及经验累积边界 | 创建实例时独立选择 Global/Project，满足定义及完整依赖的可见范围；与当前参与的 Feature 分别记录 |

全局 Workflow 在某个 Project 中使用时保留全局定义身份，其 WorkflowInstance 和 Feature 配置锁属于实际 Project/Feature。将全局定义复制并特化为项目定义时创建新定义身份，保存 `derived_from` 的精确修订；不改变原定义的 Scope 或版本历史。

定义身份的 Scope 创建后固定。全局定义的依赖只能引用全局定义；项目定义可以引用全局或本项目定义，禁止引用其他项目定义。创建 Project 绑定及 Feature 锁时递归校验依赖闭包的作用域、内容与兼容性，拒绝循环或缺失引用。为历史读取追加复制共享修订时保留来源 Scope 和身份。

首版 Agent、Model、Workflow、Standard、Spec、Skill、Tool、Harness、Sandbox 和 Permission 的可复用定义支持 Global/Project Scope。Feature Scope 用于创建时配置锁和具体运行事实；Feature 应用这些定义时固定精确引用，不在创建后增加可变的局部治理覆盖。Scope 与既有 `TaskScope` 的验收范围、`ExecutionScope` 的运行授权分别命名和验证。

### 稳定身份与不可变版本

所有可绑定的配置采用统一修订契约：`definition_id` 标识独立演化的定义，`revision_id` 标识不可变内容，`version` 表示该定义内的版本序号，另保存内容摘要、前序修订和 Schema 版本。业务对象自身的乐观锁 `revision` 与定义修订分别处理。

| 配置对象 | 必须固定的依赖 |
|---|---|
| Workflow | 子流程、步骤交互契约、角色/能力约束、默认完整 AgentRevision 和所用模板的精确修订 |
| Standard | 所有用途 slot 的 Spec 修订及合成规则，生成可验证的 StandardLock |
| Agent 定义 | ModelRevision、HarnessRevision、Skill/Tool/Permission 精确引用、角色与指令；AgentID 固定该组合修订 |
| Model | 模型后端/提供方、连接定义版本、模型标识及可用版本证明、推理参数和能力声明 |
| Harness/Sandbox | 模型接入及工具编排能力、环境、执行器权限边界、安装配方或版本及内容约束 |
| Skill/Tool/Spec/Permission 及其他可绑定模板 | 自身不可变内容、工具调用契约、执行约束及声明的全部精确依赖 |

定义发布新版本不更新其使用方。Standard v2 引用 Spec v1 时，发布 Spec v2 后 Standard v2 仍引用 Spec v1；采用新 Spec 须形成新的 Standard 修订。所有嵌套依赖同样固定，不允许 `latest`、浮动版本范围或当前项目默认值进入 Feature 配置锁。

### Project 默认绑定与 Feature 配置锁

新增 `ProjectConfigurationRevision`，包含各配置 slot 的精确修订引用、来源和解析结果。Project 保存当前修订指针，可通过具有 expected_revision 的命令原子切换 Workflow、Standard 等默认绑定，并保留原修订。继承全局默认时，在生成 Project 配置修订时完成解析；全局默认变化不会修改现有 Project 配置修订。

`feature.create` 使用明确的 Project 配置修订，允许在创建参数中选择其他合法的精确版本；完成作用域、依赖和兼容校验后，同事务保存 Feature 与 `FeatureConfigurationLock`。锁至少包含来源 Project 配置修订、WorkflowRevision、StandardLock、各参与 slot 的 AgentRevision、其 Model/Harness/Skill/Tool/Permission 组合闭包和 Sandbox 引用及摘要。创建即冻结，包括处于 draft 的 Feature。

创建时已固定的运行定义可以在后续按相同版本配方准备实际安装，但实际能力及内容必须符合锁定约束。步骤只选择或创建绑定该 slot 所锁定 AgentRevision 的 AgentID，选择事实另行保存；不能只验证角色/Harness 类型相同。需要多个组合参与时，在创建 Feature 前将各候选精确修订纳入定义和配置锁；不得借新实例、单独的 Model/Harness 覆盖或读取偏好改变组合。

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

## 四 Agent 组合与 AgentID

### 概念及职责

Agent 是可复用的执行者配置，AgentRevision 固定其全部组成部分，AgentID 是该组合的具体实例身份。角色描述职责，Workflow 描述协作过程，二者分别进入 Agent 属性和步骤参与要求；Standard 管理工程标准，Sandbox 管理实际执行环境，不并入 Agent 身份。

| 概念 | 定义及主要内容 |
|---|---|
| Model | 推理后端定义：提供方、连接版本、模型标识/版本、推理参数及能力；独立于 Harness 和其安装 |
| Harness | 承载 Agent 的执行及编排运行时：模型协议接入、工具调用、会话、交互和恢复能力；同一 Harness 可支持多个 Model |
| Skill | 可复用的方法、操作说明及领域知识，声明所需 Tool/能力；内容及依赖版本固定 |
| Tool | 可调用操作的契约及实现绑定，具有名称空间、输入输出 Schema、版本和适用执行范围 |
| Permission | 对文件、网络和工具调用等操作的授权策略；由相应 Harness/Sandbox 执行器落实 |
| Agent | ModelRevision + HarnessRevision + Skill 集合 + Tool 绑定集合 + Permission 绑定，加上职责、指令和允许工作类型 |
| AgentID | AgentInstance 的唯一 ID，固定采用一个 AgentRevision，并绑定 Global 或某个 Project Scope |

Agent 与 Model/Harness/Skill/Tool/Permission 都使用稳定定义身份及不可变修订。替换组成部分或改变其参数均生成新的 AgentRevision，已有实例继续绑定原修订。即使同一个 Harness、同一种角色，只要 Model 或其他组成部分不同，就具有可区分的组合身份。

### Model 与 Harness 的边界

ModelRevision 至少记录 `model_definition_id`、`revision_id`、`provider_kind`、`endpoint_revision_ref`、`provider_model_id`、供应商可提供的模型版本证明、推理参数和能力声明。连接定义不保存密钥正文，实际认证使用 credential_ref；凭据轮换不自动改变模型配置或授权范围。

同名模型经过不同提供方/路由、不同模型发布版本或推理参数时，采用可区分的 ModelRevision。供应商只给出浮动别名时记录这一事实，实际回执中可观测的模型版本另行保存；配置版本冻结不能被描述为远端模型行为逐字可复现。

HarnessRevision 保存运行时类型和版本、适配器/协议契约、支持的模型接入及工具和会话能力，RuntimeInstallation 只描述对应实际二进制或镜像。同一安装可以服务不同 AgentRevision，不因更换 Model 重复安装 Harness。代码中的 HarnessProvider 与模型提供方字段分开命名和迁移，不能用 Codex/DSH Harness 名称推断实际模型后端。

派发前验证 Model 与 Harness 的协议、工具调用、结构化输出及交互需求兼容性。实际 RuntimeBinding 固定 AgentRevision、ModelRevision、HarnessRevision、模型参数和连接引用、安装摘要、适配器、已暴露工具及最终权限。选择新的 Model 必须采用新的 AgentRevision 和 AgentID，不允许在同一 ID 下通过会话参数或静默 fallback 换模型；模型不可用时显示实际阻塞。

### Skill Tool 与 Permission 的组合

Skill 的方法说明不安装 Tool，也不授予 Permission。Agent 明确声明所用 Skill 精确修订、Tool 集合及各工具的实现绑定、Permission 精确修订及解析规则。工具名称相同而提供方、Schema 或实现版本不同时保持不同的 ToolRevision，使用限定名称定位，避免调用重名工具。

AgentRevision 保存组合内容、引用顺序和 `composition_hash`。指令或有序 Skill 装配的变化也形成新修订。发布时递归校验依赖 Scope、工具 Schema、所需能力和权限可执行性；缺少实现或执行器无法落实的规则返回错误，不以删除约束的方式通过校验。

实际可用能力由 Agent 声明的 Tool 集合、Model/Harness 支持、工具已安装状态、冻结 Permission、ExecutionScope 和 Sandbox 约束共同决定。界面展示配置及收窄来源；Skill 或模型的工具请求不能扩大授权。Project/全局配置只在构建 AgentRevision 时提供默认值，不能在实例运行时重新覆盖组合。具体 Feature/worktree 路径及一次性运行审批属于冻结策略下的执行事实，按执行保存，不改写 AgentRevision。

### Agent 定义与实例作用域

Agent 定义的 definition_scope 控制维护位置和可见性，AgentID 的 scope 控制具体实例归属及经验范围，两者分别选择并验证：

| Agent 定义范围 | 可创建的实例 Scope | 实例保存位置及经验范围 |
|---|---|---|
| Global，且组合依赖均全局可见 | Global | 全局实例登记；按该 ID 跨参与项目积累经验 |
| Global，且组合依赖均全局可见 | Project P | P 的实例登记；该 ID 仅在 P 内参与工作和积累经验 |
| Project P | Project P | P 的实例登记；该 ID 仅在 P 内参与工作和积累经验 |
| Project P | Global 或其他 Project | 拒绝，实例范围不能扩大定义及依赖的可见性 |

定义和修订保存在其所属库，引用共享版本时保留只读副本和来源。创建实例时明确选择 Scope，不再由定义保存位置自动决定经验范围。实例 Scope 创建后固定；需要改变范围时创建新 AgentID，并保留明确来源。

### AgentRevision 与实例数据

`AgentRevision` 至少包含稳定 Agent 定义 ID、不可变修订 ID、角色/指令、ModelRevision、HarnessRevision、有序 Skill 引用、ToolBindings、PermissionBindings、允许工作类型、组合摘要和 Schema 版本。

`AgentInstance` 至少包含 `agent_instance_id`、`agent_revision_id`、`scope`、显示名称、创建时间、生命周期、可选来源实例及乐观锁。AgentID 指该记录的唯一身份，不等于 Agent 定义 ID、AgentRevision ID、Session ID 或 Harness 原生会话 ID。

一个 AgentRevision 可以创建多个 AgentID，并分别指定 Global/Project 范围。同一个全局 AgentID 可以在多个 Feature/Project 工作，前提是它们的配置锁均允许其绑定的精确 AgentRevision；不能在不同 Feature 中用同一 ID 运行不同组合版本。组合升级需要新 AgentID，可以通过显式的快照引用继承旧经验，但不合并或重标原 ID 的历史。

同一实例可以拥有多个独立 Session。各 Session 的上下文、代码视图、工具授权和写入范围继续属于实际项目。全局经验读取不会共享可变 Context，也不赋予来源项目写入权。

### 存储 历史与表现归属

现有 Definition 修订存储扩展为类型化 AgentRevision、ModelRevision 和 ToolRevision 等配置记录；Agent 配置只保存精确引用和组合内容，不再在 Session 创建时临时拼接 Model。Global/Project 的 `agent_instances` 分别保存相应 Scope 的实例，SQL 和 core 保证 agent_revision_id/scope 不变，项目内维护共享实例的只读引用。

Session、Turn、Run、Snapshot 来源及参与记录保存 agent_instance_id、agent_revision_id、composition_hash 和实际 RuntimeBinding。经验按 AgentID 检索，项目名称、Feature 名称和时间仍为主要筛选。展示或比较工作表现时可按 AgentRevision、ModelRevision、HarnessRevision 和实际环境进一步区分，不能仅按角色或 Harness 汇总后宣称模型表现相同。

Feature 汇总快照可以包含多个实例，通过参与关系及来源 Session 标记贡献。采用他人快照保留“来源实例产生/当前实例引用”的区别；Model 的声明版本、可观测实际版本及未知边界分别展示。

评审独立性结合角色职责、定义来源链及实际作者/评审参与关系验证。更换 Model、新建 Agent 定义或 AgentID 不自动构成合格的独立评审者；迁移继续保留原有可证明的身份隔离事实。

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
| `participant_slot / capabilities` | 指定角色及能力要求，绑定完整 AgentRevision；实际 AgentID 必须精确匹配该组合及允许的实例 Scope |
| `interaction` | 讨论方式、目标、必答主题、工具和历史读取策略、输出 Schema、无进展/轮次上限及完成依据 |
| `guards / completion` | 依赖完成、产物已采纳、有效 Review、测试证据、目标未变化等内置条件 |
| `advance_mode` | 完成本步且条件满足后，自动进入后续步骤或等待人确认 |
| `next / outcomes / rework_scope` | 正常分支、技术失败与业务退回路径，以及受影响步骤的重新执行范围 |
| `execution_recipe / retry_policy` | 正式执行的受约束节点配方，以及允许的技术重试类别与上限 |

绑定 slot 是有职责和能力约束的参数。例如 `architect` slot 可以设置精确默认 AgentRevision，Project 在创建配置修订时可选择满足该契约的其他完整 Agent 组合；所有引用保持精确，参数在 Feature 创建前解析完毕。Project 可通过选择新 AgentRevision 独立采用另一 Model/Harness/Skill/Tool/Permission 组合，而不改写兼容的 Workflow 正文；不得在 slot 内单独覆盖 Agent 的组成部分。不可覆盖的固定引用必须通过 Workflow 新修订改变。

节点能力由动作目录提供，首版包括 `agent.interact`、`proposal.adopt`、`attempt.execute`、`delivery.review`、`merge.prepare`、`merge.apply`、`snapshot.capture`、`resource.cleanup`、条件分支、并行汇合及 `tasks.foreach`。目录描述每种动作的字段、输入输出、适用 owner、执行权限与结果条件；表单和内核消费同一契约。新动作语义需要新增处理器，已有动作的合法组合通过定义编辑完成。

`agent.interact` 根据 AgentID 固定的完整 AgentRevision 建立或选用 Session，注入步骤目标、冻结输入、实例 ID 和有限经验索引，支持多轮澄清与结构化 Proposal。调用工具和经验读取均符合其 Tool/Permission 及实例 Scope。聊天结束或 Agent 自称完成不满足工程门禁；采纳、Review 和测试结果通过正式命令及可核验事实成立。

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
| `workflow_drafts` | 同定义所属库 | draft_id、definition_id、base_revision_id、document JSON、edit_revision、验证摘要、published_revision_id；发布后从新草稿继续编辑 |
| `workflow_revisions` | 同定义所属库 | revision_id、definition_id、version、schema_version、document JSON、content_hash、published_at；唯一 `(definition_id, version)`，发布后禁止更新/删除正文 |
| `definition_revision_imports` | 消费它的项目库 | 导入的本地修订引用、来源 Scope、摘要及依赖闭包；保留全局身份，不作为项目定义编辑 |
| 配置定义/修订的 Model/Tool/Agent 类型记录 | 定义 Scope 所属库 | 共用版本记录的 kind 和类型化 configuration JSON，分别保存模型后端配置、工具契约、Agent 组合引用与 composition_hash；不复制为另一套可写正文 |
| `agent_instances` | 实例绑定的 Scope 所属库 | agent_instance_id、agent_revision_id、scope、来源与生命周期；AgentRevision 和 Scope 不可变；全局定义可对应全局或项目实例 |
| `project_configuration_revisions` | 项目库 | 不可变绑定图、内容摘要和前序修订；Project 原子更新 current_configuration_revision 指针 |
| `feature_configuration_locks` | 项目库 | feature_id 唯一、来源 Project 配置修订、Workflow/Standard 引用、完整 lock JSON 和摘要；只插入 |
| `compiled_workflow_plans` | 项目库 | plan_id、定义/配置摘要、编译及动作语义版本、plan JSON；内容不可变，可由多个相同配置的实例引用 |
| `workflow_instances` | 项目库 | owner、parent_instance_id、workflow_revision_id、configuration_lock_id、plan_id、状态、乐观锁；每 Feature 唯一主流程 |
| `workflow_step_instances` | 项目库 | instance_id、step_key、round、状态、input_lock、output_refs、时间及乐观锁；唯一 `(instance_id, step_key, round)`，新轮次创建新记录 |
| `workflow_step_links` | 项目库 | 步骤到实际 Session/Proposal/Attempt/Run/Delivery/Review/Merge/Snapshot 的引用及用途；验证同项目和版本归属 |
| `workflow_task_groups / members` | 项目库 | 来源 TaskPlan 修订、generation、成员摘要、确定 Task ID 与子流程 ID；一代成员集合不可原地改变 |
| `workflow_confirmations` | 项目库 | 步骤轮次、计划/输入/产物摘要、目标、决定、主体及时间；保留原决定，失效另行记录 |
| `workflow_action_intents` | 项目库 | 步骤轮次、确定 action_key、冻结命令、派发/核对状态与关联 operation；action_key 唯一 |

依赖和引用可建立规范化索引用于查询与校验，由权威 JSON 同事务派生；索引不提供独立写入口。全局定义修订在解析时只读取一次并核验摘要，项目内保存所需精确副本，随后 Project 换绑和 Feature 创建只在各自项目事务中提交，不假设跨 SQLite 原子事务。

导入时将共享 Workflow 定义和修订写入项目内同型表的只读副本，保留原 ID、Scope、正文和 hash，imports 表只登记来源关系。运行表的外键引用本地精确修订，避免跨数据库外键；项目编辑 API 不允许修改这些全局副本。其他配置依赖采用同样的本地镜像规则，副本可从来源重建且不能独立发布。

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

步骤状态至少区分 pending、ready、active、waiting_input、waiting_confirmation、blocked、completed、skipped、failed 和 cancelled。状态语义区分工作未完成、推进待确认、未选择分支与工程门禁阻塞。互斥分支必须具有确定且无歧义的匹配规则；未选择分支标记 skipped，汇合只等待实际激活分支。空 Task 集合是否允许跳过必须由流程用途和 Standard 明确决定，不能将空集自动当成有效交付。

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

`attempt.start`、`proposal.adopt`、`review.request/submit`、`merge.request/apply` 等既有命令在关联流程时均验证对应步骤、输入锁和动作授权。独立只读浏览和不产生正式结果的补充讨论有明确上下文；一旦采纳为工程输入，就走正式关联和失效规则。不存在外部提交任意 `step.completed` 或 `set_status` 的入口。

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
| Feature 收尾 | Feature → mainline：准备候选、验证、Review，固定待合入交付 | 测试、独立评审 | 等待人确认后才能进入应用合入 |
| Feature 收尾 | 应用已确认候选，核对本地/远端推进和 PR 状态 | 内核合入处理器 | 自动；再次校验候选与目标基线 |
| Feature 收尾 | 最终快照、报告、资源释放和按需归档 | 当前参与实例 | 自动；归档采用既有能力 |

评审失败进入显式修复路径并保留旧 Attempt；目标基线变化重建受影响候选。自动技术重试有明确次数和故障类别，耗尽后停止并显示阻塞原因。

### 需求到交付的实际驱动路径

1. Project 选择 Feature 工作流及 Standard 的精确版本，并保存配置修订。可选的项目需求入口流程对每个需求建立独立实例，固定入口配置；产生 Feature 时显式解析当时的 Project 配置并校验版本。
2. 创建 Feature，同事务固定配置锁、编译计划和唯一主流程，登记初始动作。worktree 的实际创建依照既有 RepositoryOperation 执行，不把定义发布视为工程启动。
3. 需求步骤要求需求分析职责并推荐满足条件的完整 Agent 组合，明确显示其 Model 和 Harness。选择绑定该 AgentRevision 的 AgentID，打开步骤讨论；通过澄清形成 Proposal，人采纳后生成正式 Requirement 修订。
4. 执行 `requirement_definition` 契约的 Attempt，只依赖当前需求和必要代码认识，形成需求 Delivery；独立 Review 接受后才开放方案设计步骤。此阶段不要求 Design、TaskPlan 或 TestPlan 已存在。
5. 架构师与测试角色在后续步骤中讨论并采纳 Design、TaskPlan 和 TestPlan。执行 `solution_planning` 契约的 Attempt，固定已接受需求、上述方案产物及验收映射，形成 plan Delivery 并进行 Review。
6. core 依据兼容且仍有效的需求与规划交付，加上基线 Snapshot，将 Feature 接纳为 ready。需求与方案两个阶段可以属于不同 Attempt，引用同一明确 Requirement 修订，不要求在一个 planning Attempt 中反复生成。
7. 从该版 TaskPlan 的确定成员创建 TaskGroup、Task worktree 和子 WorkflowInstance。依赖满足的任务可并行，各自执行 test_author、Red、implement、Green/docs、verify，按具体证据生成 Delivery。
8. Task Review 通过后准备 Task → Feature 候选，验证及 Review 后应用；只有原生 Merge 已正式接纳，成员才满足该 TaskGroup 的合入条件。
9. TaskGroup 汇合成功后创建 Feature closure Attempt，固定成员的已接受交付，执行完整测试、Docs、verify 及独立 Review。新输入或退回按依赖图创建修复轮次。
10. 准备 Feature → mainline 候选，验证并评审，通过后等待配置要求的人工确认。确认引用精确候选、目标基线和 Delivery；自动推进策略也必须满足相同工程门禁。
11. 应用已确认或按锁定自动策略获准的候选，核对本地 CAS、远端 lease 和 PR 事实；core 接纳后才声明 Feature 完成。保存最终快照及报告，执行资源释放。

新的 requirement_definition/solution_planning 是正式阶段执行契约，W3 同步调整 Attempt/Delivery 校验、规划接纳和节点配方。旧 `planning` 行为作为 legacy 契约读取和核验，不改写历史阶段。新 Feature 只采用新编译路径；独立需求交付及评审、方案交付及评审分别留证，保证交互先后关系可以实际执行。

Project 不只有一个可变的“当前流程步骤”。它聚合多个 Feature、Task 及可选需求入口实例，并协调额度和资源；每个实例有独立进度及固定配置。项目默认换绑后，不影响任何已有 Feature 或入口实例，汇总查询按实例及版本显示真实进度。

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

页面包括当前工作入口、Agent 组合及实例选择、对话主体和可展开的经验面板。显示 AgentID、Agent 名称及修订、角色职责、Model 后端/版本、Harness/版本和实例 Scope；Skill/Tool/Permission 在详情中渐进展开。定义与实例创建入口放在规范与身份，讨论页允许快捷创建绑定已有 AgentRevision 的实例并选择 Global/当前 Project。

推荐由选定 Feature/Task 的工作流步骤、交互状态和能力需求决定，并返回理由。多个步骤并行时按当前对象列出候选，允许显式选择；已运行 Session 保持原实例和绑定，不因推荐更新而切换。

Feature 创建前，按 Workflow slot、Project 和全局默认选择完整 AgentRevision，并核对职责与组合能力；内置默认 Agent 采用 Codex Harness 和明确发布的 ModelRevision，缺失模型配置则显示待配置。Feature 内只从锁定组合选择精确匹配的 AgentID，不提供运行时独立 Model/Harness/Permission 覆盖。默认实例选择顺序为步骤已分配实例、对象已有兼容实例、用户选择/新建；同一组合的 Global 与 Project 实例可分别显示，经验不自动混合。

经验面板始终突出项目名称、Feature 名称和时间筛选。选择结果可查看快照、插入有来源的经验引用、定位来源工作或恢复场景。对话显示当前步骤目标、期望产物及采纳后的下一步；正式执行产生的历史也进入同一实例履历。

### 工作流程

流程管理入口位于左侧“工作流程”，负责定义、草稿、版本和 Project 采用关系；实际执行进度位于“执行与交付”的 Feature/Task 中。两个入口使用相同步骤 key 和显示组件，管理定义与操作执行各有明确地址。

#### 流程列表与版本详情

列表展示流程名称、Scope、用途、最新已发布版本、草稿状态及被采用情况；支持按 Global/当前 Project 和用途定位。全局定义可以查看并派生为项目版本，项目定义不出现在其他项目的可选列表。最新已发布版本与当前 Project 实际绑定版本分别显示。

每项提供查看已发布版本、继续草稿、基于此版本编辑、复制为新流程等动作。已发布详情包含流程预览、步骤契约、精确依赖、版本记录和采用它的 Project/Feature；采用清单分页读取。停用阻止新绑定，已有引用保留可读。

#### 创建与编辑草稿

“新建 Workflow”先选择空白、内置 Feature 开发模板或某个已发布版本，填写名称、Scope 及用途。选择 Scope 后建立 Workflow 身份和草稿；后续跨 Scope 复制使用派生操作。空白草稿可以保存不完整状态，模板默认包含原始第 14 章的三段开发闭环。

编辑器使用三个区域，窄屏改为顺序页签：

```text
┌ 阶段与子流程目录 ┬ 流程预览及当前选中步骤 ┬ 步骤配置 ┐
│ Feature 规划    │ 需求讨论 → 需求评审    │ 角色与能力│
│ Task 子流程     │       ↓               │ 交互与产物│
│ Feature 收尾    │ 方案规划 → Task 并行   │ 条件与推进│
└────────────────┴───────────────────────┴──────────┘
          草稿保存状态       校验结果       发布版本
```

左侧目录切换阶段与子流程，支持添加步骤、移动、复制、删除及设置并行组。中央预览展示顺序、分支、汇合和子流程，可点击节点定位表单；展开 Task 子流程时保留父路径。首版通过结构化操作和依赖选择器建立合法关系，支持键盘移动，不依赖自由画布拖线。

右侧表单按以下分组渐进展开，字段随动作目录动态选择：

| 分组 | 用户编辑的内容 |
|---|---|
| 基本信息 | 名称、动作类型、所属阶段、前置步骤、分支与子流程；稳定 key 由创建动作生成并保留 |
| 参与者与能力 | 职责 slot、完整 AgentRevision 及 Model/Harness/Skill/Tool/Permission 预览；显示定义和实例的不同 Scope，解释 Project 可以替换整个 Agent 组合 |
| 交互 | 头脑风暴/澄清/方案讨论方式、目标、必答主题、输出要求、允许工具、历史读取范围及无进展处理 |
| 输入与产物 | 从上游端口选择输入，声明 Proposal/文档/交付等输出，显示验收条件映射 |
| 条件与推进 | 类型化条件选择器，自动或待人确认，以及确认发生的具体边界 |
| 失败与返工 | 技术重试范围及次数、Review 退回后回到的业务范围、需要停止等待处理的情况 |

定义编辑职责要求、默认 Agent 组合和实例分配策略；实际 AgentID 在运行时选择并记录，必须匹配锁定的精确 AgentRevision。讨论表单不要求用户写原始 JSON，也不把业务条件表达为任意代码。高级只读定义视图可用于检查和导出；可编辑内容始终经过同一 Schema。

引用独立 Task 子流程版本时，点击“编辑子流程”建立其草稿；发布新子流程后，父流程草稿显式更新引用并发布，旧父流程版本不变。保存一次表单改变只修改同一份草稿文档，目录和预览从它派生，不独立维护节点或连线事实。

草稿可自动保存，显示未保存/保存中/已保存/冲突。每次保存携带 edit_revision，冲突保留本地未提交内容并要求重读合并；不得后写覆盖其他编辑。发布前先完成保存，并确认校验结果匹配当前文档摘要。

#### 校验与流程预演

“校验流程”显示可定位到具体节点或字段的错误，例如分支无出口、输入缺失、角色能力不足、跨 Scope 依赖或不符合固定标准；固定节点和条件显示不可删除原因。

“预演”使用显式的样本事实，逐步展示某分支为何可执行、何时等人、哪些角色参与、Task 如何展开和汇合，以及退回后生成哪些轮次。预演标记为演练，不调用模型、创建 worktree、运行真实测试或合入代码；其通过不代表真实执行证据已成立。

#### 发布与 Project 绑定

发布页展示相对基线的结构、角色/能力、交互和推进策略差异，以及新版本号和变更说明。发布命令成功后进入只读版本详情，提供单独的“用于当前 Project”动作。

Project 绑定表单选择精确 Workflow 与 Standard 版本，展示 Scope、依赖解析及兼容性、当前与新配置差异，并明确生效对象为之后创建的 Feature。提交生成 ProjectConfigurationRevision；发布失败不会改变绑定，绑定失败不撤销已发布版本。已有 Feature 只展示自己的版本、与当前默认的差异和创建后继 Feature 入口。

#### 运行进度展现

Feature/Task 详情展示实际阶段、当前步骤、分支、Task 汇合、等待输入/确认和阻塞原因。节点展开读取该轮的实例 ID、输入、对话、产物、测试、Review 和 Merge；历史轮次保留并可切换。运行画面不出现步骤编辑、版本切换或拖动节点动作。

“继续讨论”“处理问题”“采纳提案”“确认后继续”“暂停/恢复”“重试/返工”等按钮来自 core 的 allowed_actions。重复 SSE、重连及刷新只触发查询；过期请求不能推进新轮次。实际进度按阶段和有效证据呈现，避免用节点数量百分比替代工程完成状态。

新增页面组件建议为 WorkflowList、WorkflowVersionDetail、WorkflowDraftEditor、WorkflowStepForm、WorkflowPreview、WorkflowValidationPanel、ProjectWorkflowBinding 和 WorkflowInstanceView；复用 typed RPC、路由和图展示层。定义管理可在尚未连接 GitHub/Docker 或没有活动 Feature 时使用，项目相关绑定操作仅要求已存在的目标 Project。

### 规范与身份及历史记录

规范与身份分别管理全局/项目 Agent、Model、Harness、Skill、Tool、Permission、Sandbox 和 Standard/Spec。Agent 编辑器依次选择职责/指令、Model、Harness、Skills、Tools 和 Permission，展示版本、兼容性、组合摘要及有效授权；保存产生新的 AgentRevision，不修改已有实例。

实例创建选择精确 AgentRevision，再选择 Global 或当前 Project。界面显示允许的实例范围及经验累积边界；实例详情只读展示组成部分，修改定义提供创建新版本和新实例入口。模型列表显示提供方、模型/发布标识、推理参数与兼容 Harness；同 Harness 的不同模型 Agent 并列可辨，不只显示角色名称。

Project 配置界面支持完整 AgentRevision 和其他配置的精确版本选择、默认绑定修订与差异预览；Feature 页面仅展示已锁定组合。实例详情链接自己的工作履历及实际 Model/环境信息，定义可见范围、实例范围与材料来源分别展示。

历史记录提供工程事件及工作流推进、人工确认、定义发布、Project 绑定变更、Feature 配置冻结、实例关联、经验导入和恢复记录。角色经验是历史的特定投影，不建立第二套可修改的工程事实。

## 九 接口与模块边界

以下为目标契约分组，W0 固定最终名称和 Schema：

| 分组 | 目标能力 |
|---|---|
| Scope 与版本 | 各类定义的作用域、稳定身份、精确修订、依赖及可见性查询和校验 |
| 配置绑定 | Project 默认绑定查询与修订；创建 Feature 时解析及原子冻结；Feature 锁和依赖只读查询；拒绝创建后重新绑定 |
| 定义与实例 | Agent/Model/Harness/Skill/Tool/Permission 定义查询、修订及组合校验；AgentID 创建、查询、归档；固定 AgentRevision 与实例 Scope |
| Workflow | 定义查询、修订、校验及 Project 精确默认选择；实例与步骤读取；按锁定版本确认、暂停/恢复 |
| 建议与动作 | 返回当前对象的可处理步骤、推荐类型和实例、能力要求、理由及阻塞信息 |
| 经验 | 按当前实例 ID 查询项目/Feature/时间维度、索引、摘要、快照与证据；读取范围和游标分页 |
| 恢复与引用 | 材料检查、经验引用导入、来源场景恢复和新 Session 创建 |
| 工作台 | 项目汇总、对象定位与独立分页、Test 定义/结果/覆盖、上下文和文档读取 |

工程命令携带 command_id、目标项目、expected_revision 和适用的 workflow_step_instance_id。创建 Feature 的命令明确 Project 配置修订，后续命令引用 Feature 配置锁。Host 解析身份和能力，core 验证权威条件，adapter 执行外部 I/O 并回传可核验结果。全局和项目命令使用各自明确的幂等范围。

### Workflow 操作契约

| 目标接口 | 输入与返回要点 |
|---|---|
| `model.create/revise`、`tool.create/revise` | 明确 Scope、提供方/实现及配置、契约和精确依赖；返回独立修订，不改变已有 Agent |
| `agent.create/revise` | 角色/指令及 Model/Harness/Skills/Tools/Permission 精确引用；返回完整 AgentRevision、composition_hash 和兼容性结果 |
| `agent.instance.create` | 精确 AgentRevision 和独立选择的 Global/Project Scope；返回 AgentID，范围受定义及依赖可见性限制 |
| `query.agent_instances / query.agent_revision` | 按实例 Scope、Agent 及角色定位；返回不可变组合与实际实例信息，区分同 Harness 不同 Model |
| `workflow.create` | 显式 Global/Project Scope、名称、用途和可选模板修订；同事务返回 Workflow 及首份 draft |
| `workflow.draft.create` | 已发布基线修订和目标定义；同 Scope 建新草稿，跨 Scope 走派生定义 |
| `workflow.draft.update` | 目标 draft、expected_revision、结构化文档；返回新 edit_revision 和 document_hash |
| `query.workflow_validation / query.workflow_preview` | 指定草稿 edit_revision/hash、目标配置和可选样本事实；返回节点级错误或不执行外部动作的预演结果 |
| `workflow.publish` | 目标 draft、expected_revision 和变更说明；重新校验后返回不可变 revision_id/version/hash；重复命令返回原发布结果 |
| `project.configuration.update` | Project 的 expected_revision、来源配置修订、精确 Workflow/Standard 及 slot 绑定；返回新的 Project 配置修订 |
| `feature.create` | 当前 Project 配置修订及创建时显式绑定选择；返回 Feature、配置锁、计划及主流程实例 ID |
| `query.workflows / query.workflow / query.workflow_draft` | Scope 与游标；读取列表、精确版本、草稿及采用关系，正文按需读取 |
| `query.workflow_version_diff` | 两个精确版本或草稿 hash；返回结构、参与者、交互和推进条件差异 |
| `query.workflow_instance / query.workflow_actions` | 固定 owner 和实例/轮次；返回图、状态、子流程、引用、可用动作和阻塞理由 |
| `workflow.step.confirm` | 目标步骤轮次、expected_revision、计划与输入/输出摘要；绑定可信人类主体并核对陈旧确认 |
| `workflow.pause/resume` | 目标实例及 revision；修改运行控制状态，不改变定义和配置锁 |
| `workflow.step.retry/rework` | 技术重试或业务修复的明确目标、原因、版本；只允许计划和领域契约支持的路径 |
| `query.project_workflow_summary` | 项目、Feature 和状态筛选；汇总不同版本的实例，返回可定位的待处理工作 |

现有 `conversation.send`、`interaction.answer`、`proposal.adopt`、Review 和 Merge 命令保留各自语义；关联 Workflow 时携带或由目标对象解析步骤引用。人类专属动作处理器只能产生待办和等待状态，自动推进不允许冒充人提交回答或批准。组合采纳/确认命令在同项目事务内验证并写入各自决定。

建议新增 `domain/scope.rs`、`domain/configuration.rs`、`domain/workflow.rs`、`domain/agent_instances.rs`；core 分别实现 WorkflowCompiler、WorkflowEvaluator、WorkflowActionDispatcher、版本依赖及有效性校验。adapter 提供版本/草稿的 insert-only 存储及动作读取，Host 负责 Scope 路由、精确共享定义导入、现有执行协调和 SSE；不让 HTTP 层直接设置步骤或业务终态。

新增 `schemas/workflow.schema.json`、Agent 组合及 Model/Tool 定义 Schema 和对应动作目录/契约 Schema，补齐 `schemas/rpc.schema.json`。Schema、Rust 类型、UI 动作字段和契约版本有一致性校验；治理配置锁、AgentRevision、StageInputLock、验收 TaskScope 和实际 ExecutionScope 分别验证。经验检索通过读端口由 Host/adapter 跨项目协调，Web 拆出共享 typed RPC、路由、组合编辑器和 Feature/Task 详情组件。

## 十 迁移与兼容

1. 顺序追加全局和项目 SQL 迁移，实施时使用最新空闲编号。保留现有数据，不修改已发布迁移。
2. 已有共享配置定义按当前全局保存位置登记 Global Scope；项目内复制的共享修订保留其全局来源，不误判为项目特化。新增项目定义与 Scope 入口后按明确归属存储和解析。
3. 新 Session 必须绑定明确 AgentID 和完整 AgentRevision。旧 Session 按已冻结的实际 Model/Harness/Skill/Tool/Permission 建立可证明的 legacy 组合与实例关联；只有 predecessor 链、组合和作用域均可证明一致时才共用 AgentID，组合变化拆为不同实例并保存来源链。缺失 Model 提供方、工具或权限信息保持 legacy/unknown，不能从 Harness 名称或当前默认推断。无法证明的同角色/同 Harness 会话不合并为同一实例，旧 Session 和 Artifact 正文不改写。
4. 迁移映射采用可追溯的附加记录；旧快照、Artifact 字节、哈希和已冻结输入不改写。无法确定时间或来源时显示未知，不用当前配置倒填。
5. 为 Project 建立默认配置修订；为已有 Feature 按其可证明的持久化绑定、历史 Spec/运行锁和实际执行语义建立不可变 legacy 配置锁及 Workflow 映射，不读取当前 Project 默认来推测原配置。明确标记为升级时保留的有效配置，不伪称已记录原创建时完整配置。缺失条目保持 `legacy_unresolved`，禁止依赖该条目的新执行；补录只采用可证明的原事实。升级不改变推进方式或触发新的自动动作，需要新治理版本时建立后继 Feature。
6. 保持真实/模拟来源、权限锁、资源清理和 Review 身份语义。旧 RPC 客户端仍通过相同 core 门禁；适配层解析 legacy 关联，不允许省略工作流引用绕过约束。
7. 全局目录建立失败可以按项目 outbox 重建；项目库不可用不阻塞其他项目本地工作。重启核对活动执行、未完成确认和推进意图，未知消费结果不得自动重发。
8. 审计 `binding.update`、`project.update`、`session.open`、正式执行和恢复入口：移除对已创建 Feature/Task 治理配置的更新及运行时重新读取最新偏好的路径。旧客户端请求明确返回配置已冻结错误；不通过兼容转换暗中重新绑定。项目默认变更本身不使旧 Feature 证据 stale。
9. 迁移保留 legacy planning 与原 input_version 判断的历史语义，为旧事实明确登记原契约；新 StageInputLock 不从后来的可变对象反推。索引、共享定义副本和编译计划按固定摘要验证，旧运行不会因 Host 更新而重新编译为新语义。
10. 保留旧权限和 runtime.model 字符串的原始含义，追加类型化关联而不重写历史。Project/Harness/Model 偏好改为后续 AgentRevision 的创建建议或完整 Agent 选择；旧会话入口单独传 model/provider/Permission 覆盖时，不得在 AgentID 下暗中生成新组合。

## 十一 实施阶段与完成条件

| 阶段 | 交付范围 | 依赖 | 完成条件 |
|---|---|---|---|
| W0 Scope 及领域契约 | 固定 Agent 组合、Model/Tool、AgentID/Scope、定义/草稿/修订/锁、步骤动作、存储和 UI/API 契约 | 本计划 | 组合与实例范围分离；Model/Harness 及 Skill/Tool/Permission 职责明确；需求先于设计，确认先于合入；保存 `CONTRACT.md` |
| W1 Scope 版本与绑定持久化 | Model/Tool/Agent 类型化修订、实例注册、全局/项目定义、insert-only 版本、Project 修订、Feature 锁及迁移 | W0 | 同 ID 组合与 Scope 不变；全局定义可建两种范围实例；发布幂等、数据库不可变约束、Project 换绑及历史归属正确 |
| W2 Workflow 定义编译和实例 | 动作契约/Schema、内置种子、完整 Agent slot 解析及组合兼容性、编译计划、父子实例及 CRUD/API | W0、W1 | Model/Harness/Tool/Permission 与流程契约兼容；非法组合/图/Scope 被拒绝；旧 Feature 计划不重新解释 |
| W3 工作流驱动交互及执行 | 求值器与 intent 派发、唤醒/调度、两段规划、StageInputLock、TaskGroup、Review/Merge、确认和返工恢复 | W2 | 需求到主干真实驱动；Task 汇合成员固定，人工确认先于合入；RPC/UI 同规则，重复/重启不重发未知输入 |
| W4 经验检索和恢复 | 分实例 Scope 的历史目录、主要筛选、组合及实际 Model 记录、逐级读取、经验 Tool 和场景恢复 | W1，恢复绑定依赖 W3 | 同 Harness 不同 Model 的工作可区分；历史按 AgentID 隔离；全局/项目范围正确，Tool/Permission 实际生效 |
| W5 工程工作台 | 导航、稳定地址、项目多流程汇总、Feature/Task 流程进度及证据、只读配置、文档/上下文/测试整合 | W2、W3，经验入口依赖 W4 | 卡片及步骤精确定位；刷新恢复地址；运行可处理待办且无法编辑绑定；不同版本 Feature 显示正确 |
| W6 工作流及配置页面 | 流程编辑/预演/发布与 Project 绑定、Model/Tool 定义、Agent 组合编辑、实例 Scope 选择及经验页 | W3、W4、W5 | 完整组合可创建和修订；同 Harness 不同 Model 可辨；AgentID 绑定后不可拆开覆盖；流程发布/换绑及旧 Feature 冻结成立 |
| W7 迁移与完整验收 | 历史升级、真实/模拟闭环、故障恢复、Web 验收和文档更新 | W1–W6 | 验收矩阵通过，报告明确实际平台与能力；证据保存于本目录 |

按阶段串行落实关键依赖，每阶段完成后更新执行记录和证据索引。实现只修改相关模块，保留工作区中其他在途变更。

W3 分三个可验收增量：先贯通需求讨论、采纳、需求交付/评审与方案规划；再贯通固定 TaskGroup、TDD 和 Task 合入；最后接入 Feature 收尾、合入前确认及恢复。每个增量先用直接领域命令与 HTTP 验证真实状态变化，再接同契约的页面，避免只完成编辑器而流程仍由旧硬编码推进。

## 十二 验收矩阵

| 编号 | 场景 | 必须观察到的结果 |
|---|---|---|
| A1 | 同一完整 AgentRevision 创建两个实例 | AgentID 不同，经验独立；组成部分、AgentRevision 和实例 Scope 的原地变更被拒绝 |
| A2 | 全局实例在两个项目参与多个 Feature | 同 ID 的履历覆盖两项目；项目名称、Feature 名称和时间可组合过滤，正文按需读取 |
| A3 | 同名 Agent 定义和不同范围实例 | 分别显示定义 Scope、实例 Scope 和 AgentID；项目实例不读取或参与其他项目，名称不合并身份 |
| A4 | 项目/Feature 改名、同名及时间边界 | 稳定 ID 保持关联，历史名称可定位；筛选、游标和时区边界一致 |
| A5 | 真实 Harness 访问经验 | 先取索引再取快照和证据，记录工具调用与引用；当前实例身份及项目范围无法由模型参数伪造 |
| A6 | 新 Feature 进入需求讨论 | Workflow 推荐有需求分析职责的完整 Agent 及匹配 AgentID，显示 Model/Harness 并说明理由；采纳衔接正式交付 |
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
| A19 | 嵌套配置独立演化 | 发布 Model/Tool/Spec/Skill/Permission 新修订不改变原 Agent/Standard/Harness 组合及旧 AgentID/Feature；完整依赖可验证 |
| A20 | 旧 Feature 后续 Task 与新执行 | Project 换绑后，新 Task、新 Attempt、新 Session、候选验证和恢复均使用旧 Feature 配置，禁止通过覆盖参数绕过 |
| A21 | 并发换绑与 Feature 创建 | 创建校验指定 Project 配置修订，冲突无半成品；幂等重放返回原结果，Feature 与配置锁同事务提交 |
| A22 | 停用版本与后继 Feature | 原修订及依赖仍能解析，已有工作不受新默认影响；后继 Feature 使用新锁并明确继承来源，旧证据不被重标 |
| A23 | 前端创建及草稿编辑 | 从模板或空白创建；阶段、预览及表单保持一致；保存后刷新恢复，旧 edit_revision 更新不覆盖新内容 |
| A24 | 发布与绑定分离 | 发布幂等且正文不可改写；单独换绑才影响新 Feature；发布/换绑任一失败不造成另一项的隐式改变 |
| A25 | 子流程独立编辑 | Task 子流程新版本发布后，旧父流程引用不变；父流程显式采用新子流程才生成新定义及计划 |
| A26 | 图与契约校验 | 缺失端口、无出口分支、循环/递归、错误汇合、不兼容角色/能力和绕过 TDD/Review 均被准确定位并拒绝发布/编译 |
| A27 | 校验与预演 | 样本分支、等待与返工路径可解释；不调用模型、Git、Docker、测试或 Merge，不产生正式完成事实 |
| A28 | SQLite 持久化与不可变性 | 草稿、版本、锁和步骤重读一致；update/delete/upsert 不能改写已发布内容；共享修订副本保持原 Scope/hash |
| A29 | 编译计划及版本语义 | 同版本不同合法 slot 配置形成各自计划；Host 升级保留旧计划语义；未知动作版本阻塞而非静默切换 |
| A30 | 分阶段规划与证据依赖 | 只有需求时可完成需求交付及 Review，再开始设计；Design 改变不重做未变需求 Review，Requirement 改变准确失效下游 |
| A31 | Task 集合变更与汇合 | 汇合固定已采纳 TaskPlan 成员；临时任务不改变条件；重新规划产生新 generation，旧 writer 停止后激活新集合 |
| A32 | 合入前确认 | 等待确认时目标分支尚未推进；确认绑定候选及基线；目标变化拒绝旧确认并重新验证，不能通过直接 merge.apply 绕过 |
| A33 | 无浏览器时的唤醒与恢复 | 关闭页面或切换项目后自动步骤继续；等待输入不热轮询；回答后唤醒；重启从 intent 和原 operation 恢复 |
| A34 | 重试与业务返工 | 技术 Run 重试与新步骤轮次分别记录；去重不重复创建对象；未知消费结果先核对，历史结果不覆盖 |
| A35 | 运行 UI 与直接命令一致 | UI 所示动作和 RPC 实际门禁相同；运行图不可编辑；人类动作不会被自动执行或由模型伪造 |
| A36 | 编辑后的不同版本完整闭环 | 同项目两个 Feature 使用不同讨论/推进定义，分别从需求到交付；新配置产生实际行为差异，旧 Feature 全程保留原配置 |
| A37 | 同 Harness 的不同 Model | 形成可区分的 AgentRevision、composition_hash 和 AgentID；记录实际模型，不能通过会话覆盖或静默 fallback 混用 |
| A38 | 定义 Scope 与实例 Scope 分离 | 全局 Agent 定义可分别创建 Global/Project 实例；项目定义创建 Global/其他项目实例被拒绝；历史按实例范围读取 |
| A39 | Agent 组合升级 | Model、Harness、Skill、Tool、Permission 或指令变化形成新 AgentRevision；采用新版本创建新 AgentID，旧实例和 Feature 继续使用原组合 |
| A40 | Model 与 Harness 兼容性 | 不支持的模型协议、工具调用或输出能力在派发前阻塞；已有安装可复用，实际执行仍记录各自组合 |
| A41 | Skill Tool Permission 边界 | Skill 不自动安装/授权 Tool；工具集合、冻结策略、Sandbox 及执行范围共同约束；界面和执行记录显示收窄来源 |
| A42 | 换模型与评审隔离 | 换 Model、新 Agent 或 AgentID 不自动满足独立 Review 要求；作者来源及实际参与关系继续校验 |
| A43 | 旧组合迁移 | 只根据历史冻结值建立关联，组合变化的会话链拆分；缺失模型/权限保持 unknown，不猜测后端，不改原快照或哈希 |
| A44 | 组合 UI 与历史追溯 | 可创建 Model/Tool/Agent 及选择实例 Scope；同 Harness 不同 Model 清晰可辨，历史显示实际版本；新默认不回写旧实例 |

验证分层执行：domain/core 验证身份、状态、版本和门禁；adapter/Host 验证多库、索引、幂等和恢复；HTTP 契约验证接口和项目边界；Playwright 验证导航、筛选、编辑与对象完整流程。新增测试围绕真实业务约束，避免只复述组件实现。

实施阶段运行相关定向用例和 Web 构建，最终运行 `cargo fmt --all -- --check`、`cargo test --workspace --locked`、`npm --prefix web run build` 及 `./scripts/check.sh`。真实 Codex/DSH、Git/worktree、测试、Review 和 Merge 闭环采用既有 `scripts/check-live.sh` 体系补充案例，分别记录实际平台和外部资源结果。

## 十三 交付物与范围

本目录只保存本计划、`CONTRACT.md`、`EXECUTION.md` 及其他 Markdown 验收记录。日志、JSON、截图和临时测试文件写入临时目录，不作为计划交付物保存；源码回归测试保留在 crates、tests、web/tests。CONTRACT 包含 Agent/Model/Harness/Skill/Tool/Permission 组合与实例样例、动作目录和流程 JSON、SQL 约束、草稿/发布与运行状态机、API 字段和页面线框；实现时同步更新领域、ADR、协议、Schema、使用说明及计划索引。阶段完成以行为及证据为准。

首版包含统一 Scope、配置独立版本与精确依赖、Project 动态默认绑定、Feature 创建时配置冻结、结构化工作流编辑及推进策略版本化、实际调度与交互约束、分作用域实例经验、跨项目只读查询、渐进式披露、场景恢复和完整工作台。跨账号经验共享、远程多租户、通用脚本/BPMN 编排与向量基础设施留待独立需求；已有 Feature 的治理配置重新绑定不属于本方案允许的操作。


## 实现落点与首版约束

实际实现采用 governance_records 类型化记录集合，步骤与边从同一 WorkflowDocument 派生，未另建可写图表。编译器 v1 使用五种规范职责 slot、有限动作目录及 Feature→Task 精确子流程引用；任意职责别名、任意深度子流程和自定义模型 endpoint 不属于首版执行能力，发布/派发时明确拒绝。经验工具采用实例专属只读目录，先索引后相关快照文件；不是任意数据库调用接口。

旧 live Feature 升级时只锁定可以证明的原 Binding 和运行记录，不推断 Model/AgentID，不自动套入新 Workflow。新组合不完整的后续工作要求以当前 Project 配置创建后继 Feature。模拟兼容接口仍供源代码测试使用，Web 不显示模拟工程。

版本化配置、编辑/预演/发布、绑定、运行、回执恢复及平台的最终验收边界统一以 EXECUTION.md 为准；当前真实实现基线另见 [devdesk-v0.3/PLAN.md](../devdesk-v0.3/PLAN.md)。
