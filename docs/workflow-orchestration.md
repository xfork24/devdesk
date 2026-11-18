# 工作流与编排

本文描述工作流的**运行语义**：谁推进步骤、产物如何被接纳、什么条件下可以批准或豁免、运行中的计划如何修改。概念定义见 [领域规范](domain.md)，命令合同见 [本地协议](protocol.md)；出厂流程与默认配置清单见 [默认目录](../resources/configuration/README.md)。

## 定义与发布

Workflow 是可发布的流程文档，当前出厂定义记录 schema_version=6：`stages` 提供背景与可选默认 Role，顶层 `steps` 通过 `stage` 字段归组。Step 声明任务指令、执行方式、必需 Skill、验收 Spec、输入输出端口、工作上下文、预算与完成条件。执行方式只有四种：**Agent**（绑定 Role）、**Tool**（平台或外部工具）、**Workflow**（子流程，可按集合 foreach 展开）、**Control**（纯控制/确认节点）。合法性由实际结构、引用与业务约束决定，不以版本标签代替校验。

草稿允许不完整引用；`workflow.publish` 强制完整编译配置闭包（子流程递归深度 ≤8、展开 ≤1000 步），发布即不可变修订。步骤在发布时获得稳定 node_id，重命名不影响运行中实例。

## 三种推进模式

每个根实例明确选择一种驱动；三种驱动都通过相同的派发、接纳、完成命令推进：

| 模式 | 操作者 | 入口 |
|---|---|---|
| 人工逐步 | 用户在执行与交付页 | `workflow.step.dispatch / accept / approve / input / retry / rework` |
| Rust 驱动 | 持久化 workflow_driver 按精确策略执行有界 tick | `orchestration.mode=rust`，参数来自 Workflow.scheduling，或 `workflow.driver.select` |
| 项目经理 Agent | WorkflowController 持续会话通过原生控制工具 | `orchestration.mode=agent`，通过原生调用派发/接纳/完成 |

`workflow.segment.dispatch` 是编排中的有界委托能力，可授权 1–32 个 automatic 节点，不是第四种根驱动。它遇到人工等待、拒绝、异常、预算或计划变化时停止，不自动完成根实例。项目级 `manager.open` 工作台与根驱动独立，存在经理对话不代表根实例由 Agent 调度。

项目管理与流程编排仅允许出厂配置目录指定的项目经理角色及其精确修订；角色名称相同不构成授权。项目页统一配置编排角色、编排 Agent 与执行角色绑定，不提供独立经理工作台或启动入口。“新 Feature”保留在工作总览，只创建草稿；“执行与交付”页的“启动 Feature”按已保存的配置和输入启动流程。保存项目配置不会启动流程。

无论哪种模式，**core 都是权威终态裁判**：完成规则求值、证据核验、人工确认语义、权限门禁全部在 Rust 内核；Agent 与前端只能提议，不能裁定。

## 初始需求与人工决策

`feature.create.initial_requirement` 保存用户自然语言描述，并以 `user_request` 冻结到 Workflow 和实际 Agent 任务，子流程继承。它独立于流程声明的类型化 `workflow_inputs`；默认 Feature 无外部输入，无须 JSON。创建表单仅在流程声明了输入时在高级选项显示 JSON 及输入名称、交付类型和必填要求。

需求 Agent 依据本步骤要求的 Spec 判断是否需要澄清；信息充分即可提交，缺少必要事实时返回 needs_input，summary 说明问题。包含人工确认节点的流程启用持久的 human_governance，子流程继承。等待输入只能由人类经步骤或步骤会话回答，Controller、Rust Driver 和 Segment 均不能代答。

Agent 调度在启动 Feature 后自动唤起经理，继续派发和接纳有资格推进的工作。人工审核点以冻结 Workflow 的 confirmation 和完成规则为准，不由经理按文档名称另加确认。常规文档审核应先满足 Spec 和独立评审条件；需求澄清可发生在文档完成之前。用户可在经理会话中回答，经理仅按本轮用户原文转交步骤，不能自行补造答案。技术故障、执行结果未知及预算耗尽也会暂停调度，应明确报告故障而非包装成需求澄清。

Docker create 失败后，只有 daemon 可达且确定该操作的容器不存在，才记录 technical/not_applied：worker 未启动、模型输入未发送。无法核对容器身份或 daemon 不可达仍按结果未知处理，不重发原输入。经理的失败状态展示实际会话错误；恢复通过带上下文的后继会话重新读取当前工作状态，保留原失败轮次。

受保护流程的计划、范围、验收标准、关键方案和输入变更必须先 preview，再通过 decision.request 请求确切的 workflow.change.apply；人类批准后，Controller 携相同参数和 decision_ref 应用。内核独立于 Controller 策略校验实例、活动计划、完整参数与预演哈希；拒绝、旧版本或其他实例的决定不能使用。Agent 不能自行移除或绕过确认节点、豁免要求，亦不能切换后继工作配置或重启另一流程以规避已确认计划。人类仍可显式调整计划。默认 Controller 不授予豁免能力，计划应用要求人类决定；Task 范围修订也经过同一决定链。源修订显式升级后才影响后续 Feature，既有冻结流程保持原选择。

## 项目经理通过事件持续推进

agent 模式启动时只交付一次冻结目标、根引用、当前摘要与策略。编排方法来自会话冻结的 Role/Skill；后续轮次接收结构化事实、确切来源和真实用户消息，不重复注入逐步编排指令或全图快照。经理通过工具自行决定派发、接纳、返工和完成，包括所有子流程。

已接纳状态变化与 workflow.runtime 事件在同一事务保存。事件关联根实例、实际实例、源修订和原生动作来源；结果、真实停止和人类决定均可唤醒经理。SQL 根范围索引与消费位置用于待办检查，提交后通知不替代耐久事实。重启继续核对原意图、消费与回执，不重放未知副作用。

每批最多 32 条、64 KiB，以末条事件 ID 继续消费。单条通知超过 16 KiB 时，投递保留事件身份、来源修订、实例及执行引用，并标记 details_omitted；原始事件和业务事实不变，经理按引用检查确切详情。真实产物进展标志保留，摘要省略不刷新本方动作预算，也不把事件当作接纳许可。独立经理工作台没有真实用户消息时保持空闲，不因尚未 bootstrap 而持续恢复工程状态。

workflow.controller.wait 保存有界工作记录、等待对象、当前 delivery_id 和可选期限，在同一次事务中检查尚未交付的事件。结果先于等待登记到达时返回 pending_events，登记后到达则唤醒后续轮次。本方动作只有对应原生回复已经交付才能合并，不越过并发外部结果。正常租约维护、空轮询及保存工作记录不构成新的模型任务。

并发结果使等待登记的版本比较失败时，该技术操作最多尝试三次；每次重新核对原经理/worker、代次、投递、等待对象和事件头，不复用旧读集合。持续竞争仍明确拒绝，原代次撤销立即停止。该机制不重试派发、接纳、编辑或其他业务动作；追踪开启时用 controller.wait.attempt 记录尝试数与时间。

source_cursor 是已投递或已核对的业务事件位置，acknowledged_cursor 是经理等待时确认的处理位置，event_cursor 只编号耐久投递。run_state 区分 running、runnable、waiting、needs_attention；它与 active/paused/failed 等控制授权状态分开。没有新事实且经理结束正常轮次却未登记等待时展示诊断，平台不补发后继动作指令。

## 项目经理的人类交互材料

经理通过 decision.request 发起审批，通过 workflow.controller.notify 发布进展、业务问题或异常求助。每次交互包含本次事项、需要人类的操作、相关材料、经理已做工作、初审与验证、问题和建议。材料引用绑定确切 output 与执行，证据关联真实上下文和记录；审批命令绑定当前产物集合，引用校验核对执行归属、产物的 input_digest 与原执行版本，并补齐来源 Session/Turn。materials/evidence 都保留数组结构，允许其中一类为空；当前审批执行可以由两类引用联合覆盖。审阅仍须核对必要的原件和上游，观察器先保存原请求再检查材料充分性，不能用清单分类代替证据。

经理的只读工具按原生流程归属检查根范围；产物及单个执行的输入/状态读取不展开无关冻结计划，原合同仍按确切计划分块读取。发布执行的 `workflow.execution.inspect` 与活动步骤检查可以返回 `publication`：原调用、确切候选、PR 创建回执及原观察身份/时间。该信息是有界历史观察，不代表当前远端状态、外部合入授权或最终发布接纳；经理可据此呈现需要人类审查的外部操作，平台仍不代为合入。

项目经理页面直接展示这些说明和链接，审批按钮对应确切业务事项，可填写意见。审批改变材料或计划版本后，旧卡片不可签署；agent 模式中，批准只提交 Human 决定和确切步骤确认，不顺带接纳；批准或退回后由经理依据真实事件继续，无需额外发送“继续”。拒绝意见在待决定及 Human 决定两个引用下均可读取，不能以空 request 覆盖反馈。原始工具流水、参数和调度配置不在经理业务页面堆放，审计仍保存在相应执行记录。

## 步骤生命周期与产物接纳

StepExecution 有 10 种状态（queued → running → waiting_input / waiting_confirmation / awaiting_acceptance → completed / failed / cancelled / skipped / needs_reconciliation）。执行完不等于被接纳：

1. 执行者完成 → 统一封套（status/summary/outputs）经逐端口校验（声明外端口拒绝、Delivery content_schema 与机器断言校验；评审报告按所选 Spec 校验检查项及证据位置）。机器 lint 不代替语义质量判断。
2. 封存 Artifact（SHA-256）并写不可变 WorkflowOutput。
3. 按步骤确认策略（automatic/human）进入等待确认或等待接纳；`workflow.step.accept` 绑定确切的 input_digest 与输出 ID。

技术重试（retry）保留输入并创建新执行；业务返工（rework）沿依赖闭包重置后继步骤。模型自述永远不能替代真实测试与 Git 回执。

Workflow 的 `review_policies` 声明评审步骤、规则原语 `rejected_when`、共享 `rework_policy` 和 `allow_human_approval`；`rework_policies` 声明默认 `entry_step`、`allowed_targets` 与 `max_reworks`。出厂默认上限为 3 次额外业务返工，0 禁止自动返工；Task 的实现与候选评审共享同一策略，Feature 的需求、设计、规划和交付各有实际作者入口。旧冻结流程未声明策略时保留原行为。

经理读取真实拒绝报告与输入后，自行选择覆盖根因的最小合法范围：`producer_step` 按具体文档 output 定位责任步骤；`after_confirmed_checkpoint` 保留仍有效的 Human 确认锚点，只重做其依赖后继；`child_workflow` 保留工作身份与调用血缘，使用明确封存代码基线创建对应子实例的后继。后者只替换实际调用集合中的该成员，撤回父调用旧结果及受影响后继，其他 Task 保留。真实基线必须有平台快照及原回执，与当前受保护代码头一致；写者仍经过 Git 预检，未知修改和未知副作用先核对。

三种方式均先 `workflow.rework.preview`，再以相同 `rework`、`preview_hash` 和原始版本提交 `workflow.step.rework`。预演列出失效与保留执行/产物、需停止项、祖先结果撤回和预算；新任务冻结经理研判、失败报告引用及验收要求。仅重跑评审不能充当修复。预算记录使用根、工作身份和策略 key，随子实例及计划后继继承；成功业务提交、计数 CAS 和不可变消费记录在同一事务内，重放不重复扣费。技术 retry 不消费业务额度。计划变更的上游重置同样检查受影响工作，纯工具阶段分组不扣费；已采用的策略 key 不得通过删除或重命名重置计数。

`workflow.inspect.rework_budgets` 返回 used/limit/remaining 及历史基准状态。耗尽返回 `rework_budget_exhausted`，不自动提高额度或转为批准。调整策略须实际 Human 决定；已有流程首次挂接策略使用 ChangeSet 的 `adopt_rework_baseline{policy,used,evidence}` 明确历史覆盖范围和次数，不用 review round 推算。采用保持原执行和报告，不允许重置已有计数；返工另行预演。

经理建议例外接受时，通过 `workflow.decision.request(operation=workflow.step.approve)` 申请真实签署，列明失败项、建议、风险和后续处理。内核从本轮完整输入与报告建立 `approval_basis`，用户理由必填；`workflow.decision.resolve` 只保存当前确切结果的 manual_approval/override_completion，随后仍由经理 `step.accept`。原报告保留 `approve=false`，后继任务的 `runtime_context.human_exceptions` 携带仍有效的上游签署和范围；新执行、新候选和新问题不沿用旧批准。身份独立、执行顺序、版本一致性和真实执行结果仍校验。同一结果被拒绝后不可换命令 ID 反复申请。


带正式代码输入的 Agent 写者，在派发及 retry/rework 创建后继执行前，只读核对实际 worktree 的冻结版本和未封存修改；返工使用已验证的平台快照作为有效基线。未封存修改或版本偏移返回 `workspace_recovery_required`，保留现场、当前执行和历史，不启动新会话或消耗重试次数。运行时挂载前再次核对，防止预检后的外部改动；此类失败记为 `business/not_applied`，不可自动技术重试。须先备份并核对修改来源，再通过真实快照封存或恢复至已核对基线，不能自动丢弃或把未知修改当作成功实现。只读 Agent 仍读取冻结提交的独立副本，不受实时 worktree 修改影响。

## 完成规则与四层批准语义

完成条件是纯数据规则树（全部/任一/存在/值相等/集合覆盖/先后/不同执行身份/人工决定），只在协调者从已接受回执构建的事实表上求值。"批准"分四层，不可混淆：

| 层 | 机制 | 谁批准 |
|---|---|---|
| 步骤确认 | ConfirmationPolicy + `workflow.step.confirm` | human；automatic 步骤无需签署 |
| 完成规则不满足时的人工例外 | `workflow.step.approve`，绑定 waiting_confirmation / awaiting_acceptance 的确切执行、输入输出与理由 | 仅 human；与普通 confirm 的覆盖许可分开校验 |
| 人工决定 | `workflow.decision.resolve`（Controller 的 decision.request） | 仅 human |
| 豁免 | ChangeSet 的 Waive（完成/确认/测试/独立性/覆盖） | human；Controller 受控制策略限制 |

`Independent` 规则按执行身份根判定：评审者与作者必须是不同身份，Agent 换新修订不构成独立评审。豁免从不伪造回执或改写事实，保留原值并单独记录理由。

## 运行中修改计划

ChangeSet 当前提供 13 类类型化操作：adopt_standard、revert_change、replace_plan、upsert_step、remove_step、upsert_stage、bypass_stage、bind_input、set_bindings、set_inputs、waive、revise_task_scope、revise_task_dependencies。操作与字段以 `WorkflowChangeOperation` 为准。协议固定为：**预演 → 等待受影响执行者静止 → 原子应用**。

- `workflow.change.preview` 生成不可变预演快照（受影响节点、must_stop、标准差异、preview_hash）。
- `workflow.change.apply` 校验哈希后：请求受影响执行者停止 → 全部静止 → 受影响执行写入不可变 workflow_execution_history（原始合同与迟到观察保留）→ 后代子实例取消 → 未受影响步骤按 node_id 保留 → 冻结新计划修订。
- 回退（revert）受限：仅限最近一次应用的纯 UpsertStep 变更，且受影响执行从未启动。

## 执行详情的阅读与管理

执行与交付按不同主对象组织四个页签：

| 页签 | 主对象与用途 |
|---|---|
| 工作流程 | 按实例、阶段和步骤管理进展、阻塞、执行者及必要操作；必要的步骤控制与人工操作按当前资格展示。 |
| 材料 | 按冻结交付类型分类的目录及单份材料正文；来源工作、步骤、轮次用于筛选和定位。 |
| 上下文 | 按实际 Agent 实例选择参与的任务和会话轮次，读取当时冻结的任务、输入来源及要求。 |
| 执行记录 | 选择一次步骤执行，核对实际执行者、调度/接纳决定、人工理由和真实工具回执。 |

材料页汇总当前 Feature、所属 Task 及嵌套流程的产物，搜索和分页只读取索引，选中后按需读取冻结正文及生产计划的 Delivery/Spec。Markdown、Diff、评审、测试和任务规划使用对应的可读布局，未知内容按字段展示；手工材料与工作流交付物区分来源。步骤页只保留材料、上下文及执行记录的定位入口，不嵌入正文和完整任务。

上下文按实际 `agent_instance_id` 区分，同名/同 Role 不合并实例；缺少实例身份的旧会话独立标识。输入材料链接到材料页，前轮和继承会话可显式选择；手工共享背景标明不代表当轮实际发送内容；展开背景列表只展示各项入口，正文在逐项展开后读取。关闭或切换会取消对应读取，版本变化后重新读取正文，编辑继续使用原始版本。经理编排参与者与步骤执行者分别标识，平台/人工步骤不冒充 Agent。

阅读链接使用项目、实例、执行、output/session/turn 的确切 ID，支持刷新和前进后退；指定材料可独立于目录分页读取。历史执行按原冻结计划追溯，历史材料不可读时明确提示，不替换为同名当前内容。原始 JSON 在独立只读窗口按来源 ID 重读；原始任务、会话事件与公开平台回执分别标识，平台调用的内部运行时/权限请求不通过回执查看入口暴露。

展示重组不复制材料，不更改接纳事实、人工门禁和授权，也不产生新的模型轮次或工程操作。

工具步骤仅改变 `stage` 归属时，运行计划变更保留原执行身份、冻结合同和平台回执，不触发返工；参数、权限、依赖等业务字段改变仍重置受影响执行。Agent 的阶段背景及默认 Role 继续参与业务影响分析。

## TDD 证据链与平台动作门禁

测试与候选的事实只能来自真实回执：`tests.run` 按冻结配方（固定镜像 SHA、文件清单、超时）在无网只读容器执行，区分断言失败与环境失败；`git.candidate` 产出单父提交与确切 diff；`git.advance` 必须持有 WorkflowActionPermit——按计划的 action_requirements 核对与候选匹配的成功测试回执；`git.publish` 用精确租约推送并创建 GitHub PR（平台不合入）。

直接发布步骤的显式技术重试沿 `retry_of` 核对最多 32 个原执行。若同一项目、根、节点、计划、轮次、合同和输入的原发布已由不可变物理观察或内核核对证明确切候选合入，则新执行在当前权限与接纳条件下复用原回执，不再次推送或创建 PR。原取消、原物理 operation 身份及观察保留；中间明确未生效的尝试可以继续追溯，未知效果仍须先核对。返工、新候选、不同发布参数和 Agent 内部独立调用不复用该结果。

## 默认流程概览

默认入口为 `builtin-workflow-feature-sol-high:10`（8 阶段 22 步）与 `builtin-workflow-task-sol-high:9`（8 阶段 20 步），由 `builtin-standard-engineering:8` 引用。本地配置目录仅发布最新修订，数据集中的既有执行按原冻结修订追溯：

- **Feature 流程**：requirements（PRD + 独立评审）→ design（基线快照、工程约束、Design Doc + 评审）→ planning（Task Plan + 评审 + tasks.create）→ implementation（foreach 对每个 Task 运行 Task 子流程）→ verification（回归测试配方、版本冻结、真实执行）→ candidate_preparation（目标工作空间、候选、候选测试）→ delivery（Feature 报告 + 评审）→ integration（候选评审、接纳、发布）。
- **Task 流程**：tests（读取工作空间、编写独立测试）→ red（测试快照、最小可调用基线、Red 快照、真实 Red）→ implementation（实现、实现快照、Green）→ documentation（只读交付说明、复验）→ candidate_preparation（目标工作空间、候选、候选测试）→ review（独立实现评审）→ integration（候选评审、接纳、发布）→ handoff（证据封存、完成）。Red 必须是预期失败的真实断言，模块加载或环境错误不能包装为 Red。规则强制 Red 先于实现，测试/实现/评审身份独立，并核对确切代码版本。

默认 Feature 的 requirements_review、design_review、plan_review 使用 human 确认：独立 Agent 按 Spec 验收通过后，人类确认本轮确切输入和产物，才能接纳并派发后继步骤。其他 Feature 步骤及默认 Task 步骤保持 automatic。返工会重置受影响执行，原确认不得用于新产物；模型评审不能代替人类确认。

步骤直接引用 Profile，文件与工具规则只保存在 Profile 中（见 [运行时权限](permissions.md)）；Feature/Task 各自可采用的规范见 Standard 快照与工作采用（见 [领域规范](domain.md)）。

## 预算与并发

步骤预算含轮次/时长/重试。创作默认值为 8 轮、300 秒、3 次重试；出厂业务 Agent 步骤大多为 30 轮、1800 秒，两个 candidate_review 步骤为 8 轮、1800 秒，重试上限均为 3。foreach 的出厂并发为 1，可配置至 32，写者按 worktree 互斥；会话同时受全局和项目额度约束。默认 Workflow 调度配置每次唤醒 8 轮、每轮最多 64 次工具调用、600 秒。人工等待与祖先暂停不消耗执行预算。

步骤可在不可变 Workflow 修订的 `budget.time_limits` 显式选择 `{"version":1,"wall_seconds":3600,"human_wait_seconds":600}`。两个值均为有限正秒数，总墙钟最多 30 天，Human 期限不得超过总墙钟；缺省不启用，旧预算的序列化和出厂选择不变。v1 总墙钟从首次派发开始，覆盖当前逻辑节点的等待、技术重试、返工、保留 node_id 的计划替换和 Host 停机时间；它不是整个工作根或经理 Session 的总预算。

v1 Human 期限覆盖步骤的 waiting_input/waiting_confirmation 阶段，从该阶段持久化时起算；重复观察和通知不重置起点。实际补充输入使步骤继续；确切人类签署或决定与等待关闭原子提交。拒绝后再次发起实际决定请求可建立新 Human 等待，总墙钟继续使用原起点。Agent Turn 内的其他交互、外部事实等待、平台操作 deadline 和无进展 watchdog 尚未纳入此 v1 政策。

同一 Host 进程使用单调计数计算期限，持久化进程身份、UTC 锚点及原逻辑执行身份。重启后用 UTC 恢复，包含停机时段；早于最后持久化锚点的时钟不能取得新额度。到期边界为 `>=`，派发、补充输入、签署和接纳重新检查原权威版本及期限；后台请求停止并进入 needs_reconciliation，保留原执行、产物及失败，不将超时视作外部效果未生效。停止/核对后的状态继续由实际证据决定。

v1 在核心命令准备时检查截止，其后提交与停止投影按原资源版本竞争；它不承诺 SQL COMMIT 返回必须早于截止点。尚未提供提交瞬间硬期限护栏，不能将逻辑期限当作外部 writer 已物理停止的证明。

活动计时仍按已有等待区间并集计算，与新总墙钟/Human 期限同时约束。稳定观察不写时钟心跳；有期限的等待通过既有调度空闲退避做低频复核。已开始节点不能在计划采用时替换或移除所选 time_limits；相同政策下的保留节点继承原墙钟起点。项目库基线为 60，旧项目库明确拒绝，不自动迁移。

## Rust / Agent 根实例调度

项目/工作选择的 `orchestration` 只含 `mode: manual | rust | agent`。Workflow.scheduling 保存每轮动作、根/实例并发、租约、技术重试与 Agent 唤醒预算，随 Workflow 和 Standard 修订发布并冻结。缺少可信错误码、未知副作用、业务拒绝或人工门禁时等待；默认不自动技术重试。所有自动动作还须满足 Harness Profile 的平台操作权限、步骤预算和共同执行内核。

根实例的 `workflow_driver` 保存策略快照、来源和 epoch；子实例继承该控制归属。`workflow.driver.select` 是人类入口，必须同时核对原根实例 revision 与 payload.expected_driver_epoch（不存在驱动时为 0）；驱动变化未必改变根实例 revision，二者不能互相替代。界面保留开始编辑时的两个值，冲突后不在保存前刷新版本。切换撤销原经理控制代次、未派发控制意图和委托，不停止业务 worker。已有运行 worker/结果继续通过实际回执接纳。Rust tick 从分页的共同 inspect/ready 投影稳定选择动作，事务内保存决定及外部 worker 意图；外部效果由现有 intent/receipt 管线执行。每项逻辑动作有幂等键，计划和代次变化后旧动作不能生效。

Web 经理面板用 `query.workflow_driver({view:"summary"})` 展示模式、epoch 和有界等待节选，`query.orchestration` 展示根范围有界交互与决定；均按显式项目作用域直接只读查询。节选只表达观察，不提供动作资格；完整 `query.workflow_driver` 和 workflow.inspect 保留原控制查询语义。人工决定前按需读取确切请求与修订，不用节选代替签署依据，协议见 [本地协议](protocol.md)。项目管理可选择未来默认模式，流程的经理协作面可切换当前根实例。没有选择驱动的已有实例维持明确的人工控制行为。经理工作台查询/配置创作与根实例调度授权分别处理。

Agent 的实际任务包含冻结的 `runtime_context`：`runtime_context.binding` 是本次 Session 选择的执行安装身份；`runtime_context.inputs.<input>.runtime_executions` 来自对应上游 Turn 的真实 Harness 执行和停止证明，包含镜像、operation 与执行身份，并以 source_output_id 关联产物。同一运行事实也出现在 `input_sources.<input>.runtime_executions`。前者是选择，后者是已观察到的外层执行事实，均不能证明任意嵌套或远程程序的环境。该投影排除凭据、模型连接、模型参数与宿主可执行路径；不会从模型自述推导环境事实。旧任务仍按实际发送的历史合同读取。

完成规则为 false 的 `awaiting_acceptance` 步骤同样出现在驱动等待事项中，标明 `completion` 或 `human_confirmation` 门禁及父子实例路径。诊断不会批准报告、改写完成规则或自动补入缺失证据。

实现依据：[业务返工与共享预算](../crates/core/src/workflow_rework.rs)、[确切人工例外](../crates/core/src/workflow_review.rs)、[领域原语](../crates/domain/src/workflow.rs)、[变更操作](../crates/domain/src/workflow_change.rs)、[Rust 驱动](../crates/core/src/workflow_driver.rs)、[控制工具](../crates/core/src/controllers.rs)、[公开运行事实](../crates/core/src/workflow_runtime_facts.rs)、[出厂流程](../resources/configuration/workflows/)。

## 共同动作资格

`workflow.inspect` 的每步 `actions.dispatch/accept/retry` 返回 allowed、稳定的阻塞 code 和 reason；dispatchable 与同一派发资格一致。foreach 槽位不足、暂停、输入缺失、完成/人工门禁和真实执行者尚未停止分别表达。Rust、经理和 UI 使用相同事实；提交仍在事务内核对计划、控制身份和当前资格。Rust 在有界 tick 内跳过受阻或陈旧候选，继续处理其他合法工作，不能通过增加额度或绕过门禁推进。

技术重试按当前业务轮次中的执行历史计算次数，与补充输入的 sequence 分开；每次新执行以 sequence=1 开始，retry_of 关联旧执行，原冻结输入和最近补充指令继续传给后继任务。步骤 retry_limit 与 Rust 策略 max_retries 共同限制重试，重启不重置次数，业务返工另起 round。自动重试只消费可信端口明确给出的 technical/not_applied 失败及所选错误码；模型文字、未知副作用、规范拒绝和未停止 writer 都不能触发自动重试。失败保存结构化 code、kind、effect 和实际来源引用，原意图终态不接受迟到结果改写。

结果未知的执行继续保留根 worker 和 foreach 槽位，核对前不能把它视为空闲容量。

默认 Codex 业务角色获得只读 spec.lint，通过步骤直接引用的 Profile 使用。Agent 从 outputs.<port>.definition.id 取得精确 Delivery 引用，在提交前检查草稿；关键评审的定位、摘要和引文仍由同一接纳检查器复算。默认 PRD/Design 后继 Spec 已声明必要章节，关键评审后继 Spec 要求真实冻结输入位置；既有工作不会随出厂选择改变而升级。自检报告明确区分结构、报告证据和语义判断，不替代独立评审或真实工具结果。

Task 所属流程派发还要求该 Task 的业务依赖全部 completed。该条件与步骤依赖分别核对，通过共同资格入口向 inspect、Rust 及命令返回 task_dependencies；依赖等待不占用已派发工作额度，也不阻止其他合法 Task 推进。工作空间仍只在依赖完成后从当时父工作实际 HEAD 准备，完成时再次核对业务依赖。

工作流评估在确认步骤需要推进后才创建回滚检查点；未派发、等待接纳/确认和已结束步骤不复制完整工程状态。外层 Host 不再次驱动 Engine 已处理的仓库及会话阶段；传入额度只扣除其他项目占用，本项目的占用由会话驱动自己核对。

经理原生动作先持久化结果，再在同一次驱动中回复。Host 在动作提交与回执登记之间中断时，按原调用身份、参数和旧 Actor 代次恢复耐久回执，即使经理随后撤权也不重做动作。取消经理 Turn 同事务暂停绑定控制器并提升 epoch；尚未提交的旧调用不能继续派发，已提交的动作保留其真实成功事实。回复传输失败保留待交付状态，已停止 worker 记录为 unavailable，不把未知消费输入重发给后继会话。

同轮回复所需的调用、Session 和精确 worker 存活信息，从结果事务的完整权威状态中提取，只有事务成功提交后才交付；不再为此恢复一次完整历史。失败传输仍由持久化结果恢复。经理只读工具不等待配置写门，单记录读取只编码目标记录，overview 直接统计目标集合；读取继续核对项目可见性、当前经理授权和敏感字段脱敏。

原生会话先持久化 Dispatching Turn 和 Prepared execution、占用原有并发额度，再由适配器后台执行 Docker 创建与 SDK 初始化。后台线程不写工程事实、不发送 Turn 输入；调度轮询实际启动回执，核对原操作和冻结镜像后提交 Running，随后才发送输入。没有原进程启动任务时返回待核对，不重新启动。取消标志与 worker 锁协调，取得实际停止证明后才能撤权和释放额度；其他会话不必等待新 worker 的 SDK 启动完成。这不改变公共跨项目调度和慢 Git/工具 I/O 的策略。

经理超时原因分列总墙钟、已登记平台请求等待及其余区间；重叠工具等待只计一次，其余区间不等同于模型推理时间。当前冻结预算仍按原墙钟规则执行。`DEVDESK_PERF_LOG=1` 增加 workflow tick、运行读取/复制、原生交付及 Docker 阶段计量，不记录工具参数和业务正文。

平台工具等待外部结果时，prepared 首次推进为 running；后续无结果或等待受管理工作空间不更新调用行。纯 Workflow 工具运行时没有原生待回复项，不恢复会话历史去扫描回复。`workflow.inspect` 只计算当前页的步骤说明，并在序列化响应前裁掉页外步骤及不公开的冻结定义；业务校验仍使用完整权威快照，普通查询不写事实。

原生 Agent 的本机 materials.capture、spec.lint、workflow.outputs.read，在耐久意图回执登记后可定向推进已准备的原调用，避免额外等待整个 tick。它们复用相同权限、取消、计划及 schema 校验与结果登记路径；普通工作流调用和外部 Git/Docker 工具不进入该定向路径。已交付的历史原生请求在解码前过滤。

`DEVDESK_PERF_LOG=1` 另输出原生协议 request_observed/reply_written：完整帧读入后、解析和日志持久化前取起点，协议写入收到成功确认后取终点。按 operation、序号及请求 ID 摘要关联，只保留有界未回复计时项；不记录参数、正文或原始请求 ID。此区间包含等待 Host 观察、登记、执行、提交及回复写入，不能把回复登记或模型后续处理当作协议交付终点。

同一开关另记录 native_phase（call_id、阶段、耗时），区分经理门等待、授权快照、目标读取、动作、结果事务和回复；native_startup 单列 Docker/协议初始化。嵌套阶段不能直接相加。数据库中的 finished 时间字段在提交前写入，不是精确的事务提交终点。权限编译只共享二进制内固定资产的期望摘要与固定语义摘要，动态源、绑定、作用域和实际安装校验不跨快照缓存。

Docker 测试的本轮 inspect 已证明容器停止时，结果采集复用该证明，并按固定容器 ID 导出日志；确实需要 stop 的容器仍在 stop 后重新取得停止证明。不得从仍运行的容器读取成功终态。

Host 在已提交变化、原生协议收包和异步启动完成时唤醒调度，活跃工作保留 75 ms 补扫、空闲使用 1 s 补扫，目录维护单独限频。通知可以合并或丢失，实际资格仍从耐久待办及原授权读取；唤醒不派发下一业务步骤。

平台外部尝试仍先持久化授权，原始结果观察先于接纳保存。步骤完整版本只参与授权当下的 CAS；长期回执保持原执行、计划、契约、输入及祖先运行资格，Host 填入 tool_call_id 不撤销该授权。原业务、源、上游及写入目标版本继续精确校验；取消、契约改变或原写入目标变化仍拒收且保留观察，不自动重发外部效果。

页面按项目/实例的事件失效共享读取，合并同键在途请求并施加背压。普通日志追加不刷新整个工作区，隐藏页暂停读取；运行时准备的空闲状态每 30 秒补扫，正在准备时保持短检查。背景、原始事件和工具调用正文在展开时读取，刷新不替换未保存草稿的原版本。

同一进程内，平台授权提交到原结果观察保存之间有按调用区分的技术在途登记。并发扫描跳过仍在执行的原尝试，不将它当作重启后遗留的未知效果；登记不跨 I/O 持锁、不等待其他业务调用，不授权外部执行，进程退出后不伪造完成证明。

Codex Harness 将本次原生 operation 的已脱敏 commandExecution 完成原件另存为不可覆盖 JSON；执行上下文给出本次 command_output_directory，Docker 只读挂载到 /run/devdesk/command-output。模型输出展示截断时可分块读取这些原件，目录不包含协调器其他状态或凭据，最多 512 文件/64 MiB。原件是原生观察，不是平台 tests.run 回执；执行者仍须提交符合冻结 Delivery 的完整测试证据，不能只交文件路径或把展示截断称为测试日志丢失。DSH 不发布该 Codex 协议专用目录。

Codex 恢复已有原生线程时使用 `thread/resume` 的 `excludeTurns: true`，由原生运行时恢复自身历史，Host 只接收线程元数据；不在每次经理唤醒时回传全部旧 Turn。原始协议、会话和业务记录继续保留，正常帧及恢复响应的有界限制不变；恢复未得到确切成功回执时不发送轮次输入或盲目重发。

材料文件封存和平台调用均按确切意图保持技术在途登记；恢复扫描跳过当前进程的活跃尝试，登记不持有跨 I/O 业务锁、不改变原授权/接纳校验。重启后没有在途证明的 prepared 意图仍按未知结果核对。写入工作树的启动前校验拒绝属于已知未执行；它不允许把脏工作树当作新基线。

Web 命令成功后使相应项目的显示缓存失效；显式刷新绕过标签页短缓存并更新经理进度及协作状态。自动事件刷新仍按根范围合并、串行读取并有背压，不以每次 token/tool 日志追加刷新整个执行页。显示没有变化时保留原组件和编辑草稿，自动刷新不替换用户编辑时的版本。

Rust 驱动与已授权委托的纯控制阶段若一轮未产生提交，Host 用该项目已提交事件游标暂记空闲；游标变化立即使提示过期，另以一秒补扫核对租约及遗漏。记录最多 128 项，不保存或授予业务资格；外部轮询、取消停止及原生回执交付不受此提示限制。实际命令仍重新读取权威状态并验证原资源读集合。

运行中的流程使用 ChangeSet `set_review_policies{rework_policies,review_policies}` 只调整后续拒绝处置规则，保留原步骤合同、执行和报告；首次采用时同批提交 adopt_rework_baseline 明确历史额度。
