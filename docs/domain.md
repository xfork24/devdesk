# DevDesk v0.2 领域规范

版本 1。当前计划与其第 14 节是唯一领域基线。所有项目 execution_mode=simulation。应用代码中的对象是类型化结构，下面的 ID 均为稳定 UUID；逻辑分支名除外。

## 归属与对象契约

| 对象 | 身份、归属与引用 | 可变性、生命周期与命令 | 事件、持久化与示例 |
|---|---|---|---|
| Project | 自身 ID，包含 Workspace 描述、默认 BindingLock | 配置可更新，revision 乐观锁；project.create/update | projects 表；project.create；示例“支付服务” |
| Workspace | Project 的仓库/路径描述值，不拥有真实 worktree | 随 project.update 编辑；不执行用户目录 | projects.workspace；示例 /work/payment |
| Feature | Project 下独立开发闭环，可引用 predecessor | draft→planning→ready→developing→validating→accepted→completed；输入修订增 input_version | features 表；feature.create、evidence.stale；示例“新增导出” |
| Task | Feature 下独立任务，绑定初始 Snapshot 和依赖 Tasks | ready→running→waiting_review→accepted→completed；完成后冻结 | tasks 表；task.create；示例“导出接口” |
| Requirement | Feature 的不可变需求修订 | document.revise；规划正式 Delivery；新修订使旧证据 stale | documents(kind=requirement)，Artifact；示例“支持 CSV” |
| Design | 引用 Requirement 的不可变设计修订 | 规划交付，与 Task/Test 计划一起 Review | documents(kind=design)；示例“流式编码方案” |
| Test | 引用 Requirement/Design 和可选 Task 的测试定义 | 定义先于 Task 实现冻结，TestRunner 请求实际携带测试定义、候选与环境并校验响应绑定；TestResult 在 Run.result | documents(kind=test)；示例“空列表返回表头” |
| Doc | Feature/Task 的不可变说明修订 | document.revise 或模拟执行产生；所属交付 Review | documents(kind=doc)；示例“使用及限制” |
| Spec | 独立规范定义 ID、修订链 | definition.revise 克隆新版本，不回写旧版本 | definitions(family=spec)；例 Internal Design Spec v1/v2 |
| Standard | 独立定义，slots 固定 Spec ID | 克隆修订；Attempt 锁定其 ID | definitions(family=standard)；例默认工程标准 |
| Skill | 独立定义修订 | 克隆、绑定；不支持任意脚本 | definitions(family=skill)；例 TDD v1 |
| Agent | 身份定义，非执行环境 | 克隆、绑定；工程与评审身份分开 | definitions(family=agent)；例工程 Agent/评审 Agent |
| Attempt | 属于 Feature 或 Task，固定输入、绑定、文档和 Context 内容/修订 | running→waiting_review→accepted/rejected；可 failed/cancelled/interrupted；attempt.start/pause/cancel | attempts 表；例某 Task 第一次实现 |
| Run | 仅属于一个 Attempt 或 Merge，节点与执行 ID 分离 | queued→running→succeeded/failed/cancelled/interrupted；run.retry 新 Run 带 retry_of | runs 表及分段 Artifact；run.started/finished/interrupted；例 green 节点第二次执行 |
| Session | 每 Run 独立身份，引用 Agent/Harness/Sandbox 和可选 Snapshot | 不共享可变 Session；随 Run 生命周期 | sessions 表；例评审会话 |
| Harness | 执行运行时能力值 | sim-harness-v1；仅模拟；不允许自行批准业务门禁 | BindingLock、Session；例受控模拟执行器 |
| Sandbox | 执行环境能力值 | sim-sandbox-v1；启动、取消确认、结束、清理 | Session、Resource；例模拟环境 |
| Resource | Run 所有的环境资源，无真实 PID | active→released/cleanup_failed；释放失败保留 | resources 表；例模拟环境实例 |
| Context | Feature/Task 独立可变内容，revision | context.update；Task 从初始 Snapshot 复制，不共享可变对象 | contexts 表；例任务补充说明 |
| Snapshot | Feature/Task 下不可变上下文和绑定、文档 ID | initial/final/manual；snapshot.save、接受交付时生成 | snapshots 表；例 Task 最终交付上下文 |
| Delivery | Feature/Task 下正式冻结边界，属于 Attempt 或 Merge | 不可修改，stale 为有效性投影；delivery.submit | deliveries、Artifact；delivery.created；例 Task 实现交付 |
| Reviewer | 人类可信会话或独立 Agent 执行绑定 | 读取冻结 Delivery，不能自报 human | reviews.actor、Session；例 human:local |
| Review | 指向确定 Delivery，逐项 Check、决定、理由 | 追加记录；pass/changes_requested/inconclusive/not_reviewable | reviews、报告 Artifact；review.submit；例退回缺少错误路径 |
| Override | 指向确定 Delivery 的人类质量例外 | 只追加；review 或 test:RunID 条目；不改原事实 | overrides 表；override.accept；例接受已知断言缺陷 |
| Merge | Feature/Task 的候选接纳操作，固定输入、源、预期目标 | running→waiting_review→accepted→completed；目标变化旧候选无效 | merges、heads；merge.request/apply；例 Task→Feature |
| Artifact | Feature/Task 下封存文件 ID、相对路径、SHA256、来源 | 不可变；写临时文件→fsync→rename→数据库引用 | artifacts 表及本地文件；例执行报告 |
| Archive | 延期契约 | 本轮仅交互预览，不导出、不删除、不恢复 | 不写业务成功记录；例归档范围预览 |

## 核心规则

1. 技术失败仅 Run 重试；范围、输入、身份锁变化和 Review 退回后的修订必须新 Attempt。DAG 依赖按节点的最后一次执行判断，不能沿用已替代执行。
2. 全局额度默认 4，Project 默认 2；同逻辑 worktree 写入互斥。暂停不强杀在途 Run。取消确认前保留许可。
3. Requirement 和 Design/Task/Test 计划分别正式交付，两个 Review 接受后保存 initial Snapshot，Feature ready。Task 独立 Context、绑定与 Attempts。
4. Task Red→implement→Green→verify；Red 是预期断言失败，不是技术失败；测试定义、候选与顺序须一致。模拟断言质量失败可经显式 Override 接受。
5. Delivery 引用确定证据；Review 不改变来源。Agent 评审为独立 Run/Session，只有 core 可以执行门禁。
6. Task accepted 后申请候选 Merge，候选验证 Delivery 经 Review 后 apply。所有 Task 完成后才允许 Feature closure Attempt；之后相同 Merge 流程进入 mainline。
7. Override 可豁免质量，不可豁免 stale、权限、来源、引用不完整或目标竞态。旧 Review/TestResult 不覆盖。
8. 输入变化使未完成流程有关证据 stale；已完成历史保留，后续变更用关联的新 Feature/Task。定义修订不会更改旧锁。
9. 模拟 Git 候选使用 sim-rev-*。目标检查、推进、Merge 完成及事件必须单事务；command_id 幂等。不得产生真实代码已合入声明。
10. 重启将未确认 running Run 标记 interrupted，保留原因，不透明续跑或自动成功。Queued 可继续调度，失败 Attempt 需明确重试或新 Attempt。
11. StandardLock 由固定 Standard 及其具名用途（planning/implementation/merge）Spec slots 表达；ExecutionBindingLock 包含 Agent/Skills/Harness/Sandbox 确定版本。ReconstructionManifest 是 Snapshot 中固定引用的组合，不设管理服务。
12. SQLite 为唯一工程事实源，文件 manifest/报告不是第二套可写状态。

## 闭环例子

创建 Project、Feature → 编辑 Requirement → planning Attempt → Requirement Delivery/Review → plan Delivery/Review → initial Snapshot → 两个独立 Tasks → 各自 implementation Attempt（Red/implement/Green/docs/verify）→ implementation Delivery/Review → final Snapshot → Task Merge 验证/Review/apply → closure Attempt → closure Delivery/Review → final Snapshot → Feature Merge 验证/Review/apply → 模拟流程已完成。

## 本轮领域补强（schema v13）

- Requirement/Design/Test/Doc 继续共享不可变文档修订的存储结构，业务校验按正式阶段区分。Requirement.acceptance 为唯一 key 与可验证 description 的列表；未单独录入时将用户完整正文登记为一个显式 AC-1，用户可在 Web 拆分。Test.verifies 引用这些 key，不通过正文关键词计算追踪。
- Task.scope 固定 Requirement/Design 修订与实现的验收条目；默认继承规划全部条目，可在创建时选择子集。需求变化后，先重新规划，再通过 task.replan 采用新范围并生成新测试修订，旧 Attempt 与输入不回写。已合入 Task 仍冻结。
- Run.binding 是独立 ExecutionBindingLock。评审 Run 的重试继承原评审绑定，接收评审时再次检查作者身份隔离。Review.assignment 固定 Delivery、Spec 与人类主体或 Agent/Run/Session。
- Workspace 是仓库描述、路径与 mainline 的值对象。Harness 与 Sandbox 为独立、持久化的定义；Session.instance 为拥有稳定 ID 的运行实例，冻结两类定义和版本，并记录外部身份、起止和终态。模拟执行使用 sim-session / sim-environment 标识；来源始终为 simulated，不伪造真实供应商实例。
- TestResult 是有结构校验的领域结果，绑定 operation_id、测试修订、阶段、候选、环境与来源；仍以 Run.result 保存其序列化形式。Attempt.baseline 固定 Red 执行的代码版本；Green/Delivery 固定实现候选，二者不要求相等。代码 revision 由 GitPort 提供。
- Snapshot.capture 固定来源 Attempt、Runs、Sessions、Resources、原因与时间；文档集合来自已接受交付。历史无法确定的捕获来源保持 legacy/空值。
- ChangeRequestRef 是 Merge 内嵌的变更请求引用，当前建立明确标记 simulation 的本地实例。ArchivePreview 是有范围和阻塞理由的只读预览，executable=false；它不是已经归档的 Archive。

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
