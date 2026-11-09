# DevDesk v0.3 领域规范

当前工程库 schema v39，全局库 v8。v0.3 实现基线及 Workflow/工作台执行记录见 [最新基线](plans/devdesk-v0.3/PLAN.md)、[实施记录](plans/workflow-and-workbench/EXECUTION.md)。配置与分阶段默认能力以 [Role/Capability 重构](plans/role-capability-project-configuration/PLAN.md)及其[实施记录](plans/role-capability-project-configuration/EXECUTION.md)为准。原始设计、真实接入、Permission 与 Workflow 计划共同约束领域模型。新 Project 使用 live；旧原型项目保留历史来源并只读打开，不再参与调度。应用代码中的对象是类型化结构；用户创建的身份使用 UUID，内置修订和导入/幂等派生记录也可使用稳定字符串 ID。

## 归属与对象契约

| 对象 | 身份、归属与引用 | 可变性、生命周期与命令 | 事件、持久化与示例 |
|---|---|---|---|
| Project | 自身 ID，包含 Workspace 描述、默认 BindingLock | 配置可更新，revision 乐观锁；project.create/update | projects 表；project.create；示例“支付服务” |
| Workspace | Project 的仓库/路径描述值，live 工程由 RepositoryWorktree 管理真实工作树 | 随 project.update 编辑；不执行用户目录 | projects.workspace；示例 /work/payment |
| Feature | Project 下独立开发闭环，可引用 predecessor | draft→planning→ready→developing→validating→accepted→completed；输入修订增 input_version | features 表；feature.create、evidence.stale；示例“新增导出” |
| Task | Feature 下独立任务，绑定初始 Snapshot 和依赖 Tasks | ready→running→waiting_review→accepted→completed；完成后冻结 | tasks 表；task.create；示例“导出接口” |
| Requirement | Project/Feature 的不可变需求修订 | document.revise；规划正式 Delivery；新修订使旧证据 stale | documents(kind=requirement)，Artifact；示例“支持 CSV” |
| Design | 引用 Requirement 的不可变设计修订 | 规划交付，与 Task/Test 计划一起 Review | documents(kind=design)；示例“流式编码方案” |
| Test | 引用 Requirement/Design 和可选 Task 的测试定义 | 定义先于 Task 实现冻结，TestRunner 请求实际携带测试定义、候选与环境并校验响应绑定；TestResult 在 Run.result | documents(kind=test)；示例“空列表返回表头” |
| Doc | Feature/Task 的不可变说明修订 | document.revise 或原生执行产生；所属交付 Review | documents(kind=doc)；示例“使用及限制” |
| Spec | 独立规范定义 ID、修订链 | definition.revise 克隆新版本，不回写旧版本 | definitions(family=spec)；例 Internal Design Spec v1/v2 |
| Standard | 可选质量基线，按动作/阶段要求 Spec 覆盖 | 克隆修订，不注入或覆盖 Step Spec | governance_records(kind=standard)；旧 slots 仅供 v1 历史 |
| Skill | 独立定义修订 | 克隆、绑定；不支持任意脚本 | definitions(family=skill)；例 TDD v1 |
| Capability | 一个 Model、一个 Harness、多个 Tool/Skill 精确修订 | 独立命名、修订和停用；不随 Role 自动派生 | governance_records(kind=capability) |
| AgentRevision | RoleRevision + CapabilityRevision，保留执行权限和兼容投影 | 整体发布不可变组合，不允许在 AgentID 会话中拆开覆盖 | governance_records(kind=agent)，投影 legacy AgentProfile 供执行器使用 |
| AgentID | 实例身份，固定 AgentRevision 和独立 Scope | Global 跨项目经验；Project 仅本项目；同组合可有多个实例 | agent_instance / agent_session / agent_session_use |
| Model / HarnessDefinition / ToolDefinition | 模型后端/参数、运行协议与精确构建、可执行能力契约 | 版本化；不支持的协议、参数或处理器拒绝派发 | governance_records(kind=model/harness/tool) |
| Scope | Global / Project / Feature 值对象 | 定义可见性、绑定目标、实例范围和材料来源分别表达 | 类型化 kind 与 project_id / feature_id |
| Workflow | 可编辑草稿、不可变版本、有限动作/端口/角色契约 | 创建、保存、校验、预演、发布；Project 单独采用 | workflow_draft / workflow |
| ProjectConfigurationRevision / FeatureConfigurationLock | 动态默认与创建时精确闭包 | Project 追加换绑；Feature 包括 draft 禁止换绑 | project_configuration / feature_lock |
| WorkflowInstance / TaskGroup | 冻结编译计划、步骤轮次、固定 Task 成员 | intent 派发、确认、暂停、有限重试和新轮次返工 | workflow_instance / workflow_intent / workflow_task_group / workflow_step_history |
| Attempt | 属于 Feature 或 Task，固定输入、绑定、文档和 Context 内容/修订 | running→waiting_review→accepted/rejected；可 failed/cancelled/interrupted；attempt.start/pause/cancel | attempts 表；例某 Task 第一次实现 |
| Run | 仅属于一个 Attempt 或 Merge，节点与执行 ID 分离 | queued→running→succeeded/failed/cancelled/interrupted；run.retry 新 Run 带 retry_of | runs 表及分段 Artifact；run.started/finished/interrupted；例 green 节点第二次执行 |
| Session | 全局或 Project/Feature/Task 范围的角色会话，引用冻结 RuntimeBinding 和可选 Snapshot | 同范围角色连续讨论复用；Run 可关联既有 Session；不同范围隔离 | sessions 表；例评审会话 |
| Harness | 执行运行时能力值 | Codex/DSH；不允许自行批准业务门禁 | BindingLock、Session；例原生 Codex 执行器 |
| Sandbox | 执行环境能力值 | 固定 Docker 镜像、权限、启动、停止及清理 | Session、RuntimeExecution；历史 Resource 仅保留查询 |
| Resource | Run 所有的环境资源；真实容器生命周期另由 RuntimeExecution 记录 | active→released/cleanup_failed；释放失败保留 | resources 表；历史资源描述只读保留 |
| Context | Feature/Task 独立可变内容，revision | context.update；Task 从初始 Snapshot 复制，不共享可变对象 | contexts 表；例任务补充说明 |
| Snapshot | Feature/Task 下不可变上下文和绑定、文档 ID | initial/final/manual；snapshot.save、接受交付时生成 | snapshots 表；例 Task 最终交付上下文 |
| Delivery | Feature/Task 下正式冻结边界，属于 Attempt 或 Merge | 不可修改，stale 为有效性投影；delivery.submit | deliveries、Artifact；delivery.created；例 Task 实现交付 |
| Reviewer | 人类可信会话或独立 Agent 执行绑定 | 读取冻结 Delivery，不能自报 human | reviews.actor、Session；例 human:local |
| Review | 指向确定 Delivery，逐项 Check、决定、理由 | 追加记录；pass/changes_requested/inconclusive/not_reviewable | reviews、报告 Artifact；review.submit；例退回缺少错误路径 |
| Override | 指向确定 Delivery 的人类质量例外 | 只追加；review 或 test:RunID 条目；不改原事实 | overrides 表；override.accept；例接受已知断言缺陷 |
| Merge | Feature/Task 的候选接纳操作，固定输入、源、预期目标 | running→waiting_review→accepted→completed；目标变化旧候选无效 | merges、heads；merge.request/apply；例 Task→Feature |
| Artifact | 全局或 Project/Feature/Task 下封存文件 ID、相对路径、SHA256、来源 | 不可变；写临时文件→fsync→rename→数据库引用 | artifacts 表及本地文件；例执行报告 |

## 核心规则

1. 技术失败仅 Run 重试；范围、输入、身份锁变化和 Review 退回后的修订必须新 Attempt。DAG 依赖按节点的最后一次执行判断，不能沿用已替代执行。
2. 全局额度默认 4，Project 默认 2；同逻辑 worktree 写入互斥。暂停不强杀在途 Run。取消确认前保留许可。
3. 新流程先执行 requirement_definition 和需求 Review，再讨论 Design/TaskPlan/TestPlan，执行 solution_planning 并评审计划；之后 Feature ready。设计变化保留未变需求评审。旧 planning 为历史兼容。
4. Task Red→implement→Green→verify；Red 是预期断言失败，不是技术失败；测试定义、候选与顺序须一致。技术失败不可作为 Red 证据，质量豁免须明确记录依据。
5. Delivery 引用确定证据；Review 不改变来源。Agent 评审为独立 Run/Session，只有 core 可以执行门禁。
6. Task accepted 后申请候选 Merge，候选验证 Delivery 经 Review 后 apply。当前 TaskPlan generation 的固定成员全部完成后才允许 Feature closure Attempt；之后相同 Merge 流程进入 mainline。
7. Override 可豁免质量，不可豁免 stale、权限、来源、引用不完整或目标竞态。旧 Review/TestResult 不覆盖。
8. 输入变化使未完成流程有关证据 stale；已完成历史保留，后续变更用关联的新 Feature/Task。定义修订不会更改旧锁。
9. Git 候选使用已观察的精确版本。目标推进先登记意图、事务外执行 CAS，再核对本地与远端回执；command_id 幂等，未核验远端事实不能声明交付完成。
10. 重启将未确认 running Run 标记 interrupted，保留原因，不透明续跑或自动成功。Queued 可继续调度，失败 Attempt 需明确重试或新 Attempt。
11. v2 的执行规范来自 Step.spec_refs；Standard 是可选覆盖基线。v1 StandardLock 的具名用途（planning/implementation/merge）Spec slots 保持历史读取与恢复；ExecutionBindingLock 包含 Agent/Skills/Harness/Sandbox 确定版本。ReconstructionManifest 是 Snapshot 中固定引用的组合，不设管理服务。
12. SQLite 为唯一工程事实源，文件 manifest/报告不是第二套可写状态。

## 闭环例子

创建 Project、Feature → 编辑 Requirement → planning Attempt → Requirement Delivery/Review → plan Delivery/Review → initial Snapshot → 两个独立 Tasks → 各自 implementation Attempt（Red/implement/Green/docs/verify）→ implementation Delivery/Review → final Snapshot → Task Merge 验证/Review/apply → closure Attempt → closure Delivery/Review → final Snapshot → Feature Merge 验证/Review/apply → GitHub 远端事实核对后完成。

## 本轮领域补强（schema v13）

- Requirement/Design/Test/Doc 继续共享不可变文档修订的存储结构，业务校验按正式阶段区分。Requirement.acceptance 为唯一 key 与可验证 description 的列表；未单独录入时将用户完整正文登记为一个显式 AC-1，用户可在 Web 拆分。Test.verifies 引用这些 key，不通过正文关键词计算追踪。
- Task.scope 固定 Requirement/Design 修订与实现的验收条目；默认继承规划全部条目，可在创建时选择子集。需求变化后，先重新规划，再通过 task.replan 采用新范围并生成新测试修订，旧 Attempt 与输入不回写。已合入 Task 仍冻结。
- Run.binding 是独立 ExecutionBindingLock。评审 Run 的重试继承原评审绑定，接收评审时再次检查作者身份隔离。Review.assignment 固定 Delivery、Spec 与人类主体或 Agent/Run/Session。
- Workspace 是仓库描述、路径与 mainline 的值对象。Harness 与 Sandbox 为独立、持久化的定义；Session.instance 为拥有稳定 ID 的运行实例，冻结两类定义和版本，并记录外部身份、起止和终态。旧数据中的 simulated 来源和实例标识仅供历史读取，不能用于新的原生执行。
- TestResult 是有结构校验的领域结果，绑定 operation_id、测试修订、阶段、候选、环境与来源；仍以 Run.result 保存其序列化形式。Attempt.baseline 固定 Red 执行的代码版本；Green/Delivery 固定实现候选，二者不要求相等。代码 revision 由 GitPort 提供。
- Snapshot.capture 固定来源 Attempt、Runs、Sessions、Resources、原因与时间；文档集合来自已接受交付。历史无法确定的捕获来源保持 legacy/空值。
- ChangeRequestRef 是 Merge 内嵌的 GitHub PR 引用，冻结仓库、外部 ID 和源/目标版本。

### 正式阶段契约

Requirement 交付通过 Review 后，设计 Run 才可派发；plan 必须包含同一冻结需求的 Design/Test，并覆盖全部验收条目；Task 实现必须有 TDD、Test 和 Doc；closure 必须包含完整验收覆盖与 Doc；Merge 候选必须引用有效源交付和同候选验证。报告随所属交付冻结。

需求/设计/Feature 测试定义变化使规划失效。Task 文档或测试变化只失效该 Task 交付及 Feature 收尾/主干候选，并终止旧输入的后续派发；兄弟 Task 保留。旧证据、Review、Override 不覆盖。

历史 Run 的 binding_verified=false，保留可从 Session 确定的身份，但不把无法证明的完整绑定用于技术重试；需要新建执行。新 Run 的 binding_verified=true。

TestRequest/TestResult.environment 使用 Session.instance.id，绑定具体环境实例；Sandbox 定义版本随该实例冻结，不能只用通用环境名称代替实例。

## 补强计划完成时的执行与历史契约

- Attempt.scope 冻结需求/设计与验收范围，outputs 只追加本尝试产生的修订；后续步骤使用 frozen_inputs 和 stage_outputs 的确定投影。重新采用规划会中断旧活动 Attempt、取消旧派发并保留旧 scope。
- Delivery 必须且只能归属一个 Attempt 或 Merge，归属、阶段和输入版本一致。TestResult 的 operation、阶段、环境实例、代码和定义必须与证据一致；Task 自己的测试必须覆盖其冻结范围，不能借用兄弟 Task 文档。Feature 文档修订不使规划或兄弟 Task Merge 失效。
- Context.revised 事件保存原内容、新内容和两个 revision。最终 Snapshot 的 Context 来自已接受 Attempt，独立于随后可变 Context；手动快照保存当前 Context。
- EffectRecord 保存固定操作 ID、适配器版本身份、冻结请求、原始回执与状态。pending → observed → applied/discarded；中断结果未知为 needs_reconciliation；cleanup_failed 通过新操作显式重试，原失败不覆盖。
- MergeAdvance 固定供应商、交付、候选、目标基线及回执。prepared → dispatched → observed → applied；结果未知 needs_reconciliation；供应商明确未推进则 not_applied，观察目标并重新验收。不得把旧操作交给不同供应商推定成功。
- Harness/Sandbox 的定义与 Session.instance 分开保存；能力缺失在绑定时拒绝。Reviewer/候选验证沿用源交付的锁定标准。供应商核对必须匹配原 Run/Session 或完整 TestRequest，错误回执不能解除 Resource 所有权。
- 数据库 v7—v13 增加操作记录、阶段输出、冻结范围和目标推进核对；旧信息保持空或 legacy，不从可变对象倒填执行事实。

GitContext 是 Attempt 与 MergeAdvance 中的不可变值对象，固定 Project、Workspace 和源/目标 CodeLocation。供应商候选、推进及核对显式接收它，TestRequest/ Harness 输入也携带它。v13 迁移保留历史未知定位为 null；未记录代码路由的旧执行必须新建执行，不能推定一个当前工作目录。

Run 首次调用的输入保存在 Harness EffectRecord 中；后续轮询及取消复用它。并行兄弟节点的新输出只进入后续新 Run 的输入，不能改变已在运行的请求。Agent 的身份由不可变修订链根确定，不把作者的另一修订当作独立 Reviewer。

## 真实接入新增对象（schema v14–v31）

| 对象 | 归属与契约 |
|---|---|
| AgentProfile | 不可变 Definition 的结构化角色、指令、技能、工具策略、工作类型与输出 schema；五个稳定身份根 |
| RuntimeInstallation | 全局 provider/version/OS/CPU 安装；完整内容摘要去重，项目与角色只引用 |
| RuntimeBinding | Session 冻结安装、适配器构建、模型、凭据引用、权限与写目录；偏好变化不回写 |
| ConversationTurn | Session 下独立输入、客户端幂等 ID、原生消息/turn ID、消费与终态，不要求工程 Run |
| ConversationEvent | 追加的消息、工具、状态、审批；有序分片与供应商去重键 |
| PendingInteraction | 精确绑定 Session/Turn/request，类型化问题/审批及回答；迟到与跨范围回答拒绝 |
| Proposal | 讨论来源、目标修订和依赖固定的结构化草案；采纳使用正式命令，不能直接批准交付 |
| RuntimeExecution | 每次真实 worker/测试容器的 operation、Session/Run、镜像、容器、起止、停止证明与协议 Artifact |
| TestRecipe | 固定 Node Test 文件、不可变镜像与超时；无任意宿主 shell |
| RepositoryWorktree | 同项目 owner 的真实分支、位置、基线与创建操作 |
| RepositoryOperation | Git/worktree/快照/发布/PR 的独立持久化意图、观测回执和待核对状态 |

live Task DAG 为 test_author → Red → implement → Green → docs → verify。测试写目录与开发测试只读限制由冻结权限及 Docker 实际挂载共同执行。正式 Run 的输入封存上限为 512 KiB；普通讨论为 64 KiB，超限不会派发。

Feature closure 为 test_author → feature_tests → docs → verify；Delivery.source_deliveries 固定两个已完成 Task 的实现交付，Merge Delivery 固定其源交付。原生 Reviewer 接收递归来源、历史 Red/Green/Review/远端回执和固定 SHA diff。最终 Snapshot 收录实际 Sessions、RuntimeBindings、RuntimeExecutions、文档与报告，不以当前默认配置替代历史。

live 合入分别记录本地 CAS、远端精确 lease、PR merged 核对；只有三者一致才完成 Task/Feature。重启先遍历全局项目索引停止和核对原执行者，已消费结果未知的输入不重发。详见 [真实工程接入](real-engineering.md)。

## Permission 与执行范围

Permission v2 通过统一 profile 表达完整文件、网络、工具与审批规则，Sandbox 仅接受文件和网络维度。PermissionProfile、VersionedPermissionBinding、目标活动分配和 SandboxDefinition 作为治理配置修订保存；活动分配追加修订，以 CAS 决定目标当前绑定。Agent 命名选择 follow_target 或 pinned，旧 Agent 只追加固定兼容投影，不改写历史。新 Feature 冻结确切权限闭包；新讨论冻结绑定，已有讨论在下一轮通过后继 Session 采用新权限。切换意图、停止回执和原命令保持幂等，原 RuntimeBinding 与原生线程身份不可原地替换。详见 [权限说明](../permissions/README.md)。

Permission 是具有稳定身份及不可变修订的独立策略实体，描述文件、工具网络和工具调用权限。Harness 与 Sandbox 持有 PermissionBinding；项目、角色和本次执行的覆盖按顺序解析，省略字段继承、列表整体替换。定义修订、绑定版本和最终策略分别记录，旧运行不读取当前 latest 配置。

RuntimeBinding 与 RuntimeExecution 保存冻结的 ExecutionPermissions，包含两层解析结果、来源、ExecutionScope、审批、隔离参数、资源额度及后端摘要。正式 Harness/Sandbox 定义按配置摘要区分，并附完整的解析绑定；共享 RuntimeInstallation 和镜像不因权限不同复制。独立 Node 测试有自己的只读、禁网、无模型凭据策略。

Permission 不替代业务门禁。正式工程的所属 worktree、测试写范围、受保护测试目录和只读 Review 代码视图继续由 ExecutionScope 约束；运行时审批不能接受交付或 Review。Never 表示不询问，不把拒绝的请求变为允许。

新配置默认采用容器内全权限；旧数据迁移保留 legacy 策略，未保存标准快照的旧执行保持 unknown。全权限以整个容器为信任范围，受限配置声明的状态/凭据不可见保证必须有对应执行器和验证。具体支持范围见 [Permission 说明](../permissions/README.md)。

## Workflow 与配置的持久化补充

配置及运行控制使用 governance_records 类型化记录集合，正文由 Rust 类型解码，不单独维护可写节点/边表。Scope、definition_id、version、content_hash 和 revision 均为独立字段。SQL trigger 阻止发布内容、历史、身份及 AgentID 绑定被 update/delete/upsert 改写。草稿和运行投影可以更新。

首版编译器使用五个规范职责 slot 和有限动作目录，Feature 可引用精确 Task 子流程；不支持任意职责别名或无限子流程递归。配置发布、Project 默认换绑、Feature 创建为独立操作。Feature 配置闭包及 compiler_version 随创建一起冻结，新 Task/Attempt/Session 沿用该锁。

AgentID 经验索引以项目名、Feature 名和工作时间为主要筛选，保留捕获名称和稳定 ID。跨库读取只读且不切换工程；原生工具挂载自身实例的只读经验目录。恢复创建带来源的后继上下文及新 Session，不能覆盖当前代码、正式材料或配置。

旧 live Feature 启动升级时建立 legacy_feature_lock，仅保存可证明的 Binding 和已记录 Session，不生成虚构 AgentID。治理配置禁止换绑，新执行需使用完整 Project 配置建立后继 Feature；原冻结执行及回执仍可读取和核对。

## Role、Capability 与默认配置

Role 定义名称、Markdown 职责及 Global/多个项目可见范围，执行职责类型与用户定义身份分开。全局目录索引共享 Role 的一条修订历史，数据库归属不限制声明的多项目可见性。编辑 Role 只创建 Role 修订；被 Agent、Workflow 或项目引用时必须先显式换绑才可删除。停用保留历史，不生成替代资源。

Capability 固定 Model、Harness、Tools、Skills。命名 Agent 固定 RoleRevision 和 CapabilityRevision；项目以 Role 身份绑定单个 Agent。Workflow v2 Step 只声明 Role 和精确 Spec，配方内部节点也声明 Role；项目和 Feature 冻结完整依赖摘要。角色身份不相同的 Agent 不能仅凭执行职责相同替换。可选 Standard 校验规范落点、版本及最低匹配数量，不修改 Step 行为；审计快照按路径、动作、角色、规范与输出契约比较。

默认目录提供分阶段 Feature/Task 工作流、六个职责 Agent、各自 Capability、Markdown Skill/Spec、GPT-6.1 Sol High、固定 Codex Harness 和 Docker 默认权限。架构师产出 Design Doc，独立项目经理 Role 负责 Task Plan；其底层执行职责复用 architect 协议。新项目页预填默认配置，用户保存后才用于新 Feature。默认资源是普通独立定义，不自动追随修订。完整目录见 [默认配置](../resources/configuration/README.md)。

v1 配置及旧 defaults_bundle 只保留历史语义。升级按旧 Workflow/Standard 精确组合派生等价 v2 Spec 绑定，不覆盖原 Workflow；无法证明的映射留待配置。harness_materialization 保存固定配方到 RuntimeInstallation 的不可变关联，Sandbox 定义与实际 Docker 执行实例分开，默认条目不声称已经运行。


## Workflow 任务冻结

Workflow v3 为每个实际 Agent Step 声明 Markdown task_instructions；attempt.execute 同时声明公共任务与 Agent 配方节点任务。系统节点没有任务编辑入口。具体合同、限制与查询见 [任务协议](protocol.md#workflow-step-任务合同)。

WorkflowTaskDefinition 随路径进入 Feature 编译闭包，AgentTaskBase 固定有效 AgentRevision、声明顺序的 Skill 和 Role 来源。execution_task_binding 关联步骤/轮次/节点，execution_task 按 Turn 唯一、同时引用 agent-task Artifact。FrozenAgentTask 使用 agent-task/v1 渲染器，分别保存定义、基础上下文、规范输入及总摘要；Markdown 原文不变，摘要用规范 JSON 对象键序。领域层核对正文、输出合同、Spec 顺序、来源、Artifact 哈希与不可变历史，不以最新 Workflow 重新解释排队输入。

项目迁移 038 追加任务唯一索引与步骤索引，全局库为 v8。旧 Workflow/Feature/RuntimeBinding/清单不补写任务字段；旧兼容执行提示继续保留。新默认任务在编译时嵌入，运行时读写已导入 Workflow 修订。

全局讨论独立于 Project/Feature/Task，使用全局 AgentID 和独立会话库。讨论范围与 AgentID 的经验范围分别表达会话归属和身份可见性：选择 AgentID 后不再重复选择经验范围；全局身份可积累跨项目工作，项目身份不能用于全局讨论。全局材料采用 `MaterialOwner::Global`，不挂靠虚构项目。
