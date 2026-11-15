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
| Rust 驱动 | 持久化 workflow_driver 按精确策略执行有界 tick | `orchestration.mode=rust` + `policy_ref`，或 `workflow.driver.select` |
| 项目经理 Agent | WorkflowController 编排会话经 21 个控制工具 | `orchestration.mode=agent`，通过原生调用派发/接纳/完成 |

`workflow.segment.dispatch` 是编排中的有界委托能力，可授权 1–32 个 automatic 节点，不是第四种根驱动。它遇到人工等待、拒绝、异常、预算或计划变化时停止，不自动完成根实例。项目级 `manager.open` 工作台与根驱动独立，存在经理对话不代表根实例由 Agent 调度。

无论哪种模式，**core 都是权威终态裁判**：完成规则求值、证据核验、人工确认语义、权限门禁全部在 Rust 内核；Agent 与前端只能提议，不能裁定。

## 步骤生命周期与产物接纳

StepExecution 有 10 种状态（queued → running → waiting_input / waiting_confirmation / awaiting_acceptance → completed / failed / cancelled / skipped / needs_reconciliation）。执行完不等于被接纳：

1. 执行者完成 → 统一封套（status/summary/outputs）经逐端口校验（声明外端口拒绝、Delivery content_schema 与机器断言校验；评审报告按所选 Spec 校验检查项及证据位置）。机器 lint 不代替语义质量判断。
2. 封存 Artifact（SHA-256）并写不可变 WorkflowOutput。
3. 按步骤确认策略（automatic/human）进入等待确认或等待接纳；`workflow.step.accept` 绑定确切的 input_digest 与输出 ID。

技术重试（retry）保留输入并创建新执行；业务返工（rework）沿依赖闭包重置后继步骤。模型自述永远不能替代真实测试与 Git 回执。

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
| 工作流程 | 按实例、阶段和步骤管理进展、阻塞、执行者及必要操作；调度和经理配置可展开管理。 |
| 材料 | 按冻结交付类型分类的目录及单份材料正文；来源工作、步骤、轮次用于筛选和定位。 |
| 上下文 | 按实际 Agent 实例选择参与的任务和会话轮次，读取当时冻结的任务、输入来源及要求。 |
| 执行记录 | 选择一次步骤执行，核对实际执行者、调度/接纳决定、人工理由和真实工具回执。 |

材料页汇总当前 Feature、所属 Task 及嵌套流程的产物，搜索和分页只读取索引，选中后按需读取冻结正文及生产计划的 Delivery/Spec。Markdown、Diff、评审、测试和任务规划使用对应的可读布局，未知内容按字段展示；手工材料与工作流交付物区分来源。步骤页只保留材料、上下文及执行记录的定位入口，不嵌入正文和完整任务。

上下文按实际 `agent_instance_id` 区分，同名/同 Role 不合并实例；缺少实例身份的旧会话独立标识。输入材料链接到材料页，前轮和继承会话可显式选择；手工共享背景标明不代表当轮实际发送内容。经理编排参与者与步骤执行者分别标识，平台/人工步骤不冒充 Agent。

阅读链接使用项目、实例、执行、output/session/turn 的确切 ID，支持刷新和前进后退；指定材料可独立于目录分页读取。历史执行按原冻结计划追溯，历史材料不可读时明确提示，不替换为同名当前内容。原始 JSON 在独立只读窗口按来源 ID 重读；原始任务、会话事件与公开平台回执分别标识，平台调用的内部运行时/权限请求不通过回执查看入口暴露。

展示重组不复制材料，不更改接纳事实、人工门禁和授权，也不产生新的模型轮次或工程操作。

## TDD 证据链与平台动作门禁

测试与候选的事实只能来自真实回执：`tests.run` 按冻结配方（固定镜像 SHA、文件清单、超时）在无网只读容器执行，区分断言失败与环境失败；`git.candidate` 产出单父提交与确切 diff；`git.advance` 必须持有 WorkflowActionPermit——按计划的 action_requirements 核对与候选匹配的成功测试回执；`git.publish` 用精确租约推送并创建 GitHub PR（平台不合入）。

## 默认流程概览

默认入口为 `builtin-workflow-feature-sol-high:6`（7 阶段 22 步）与 `builtin-workflow-task-sol-high:6`（7 阶段 20 步）：

- **Feature 流程**：requirements（PRD + 独立评审）→ design（基线快照、工程约束、Design Doc + 评审）→ planning（Task Plan + 评审 + tasks.create）→ implementation（foreach 对每个 Task 运行 Task 子流程）→ verification（回归测试配方、版本冻结、真实执行）→ delivery（Feature 报告 + 评审）→ integration（候选、候选测试、候选评审、接纳、发布）。
- **Task 流程**：tests（读取工作空间、编写独立测试）→ red（测试快照、最小可调用基线、Red 快照、真实 Red）→ implementation（实现、实现快照、Green）→ documentation（只读交付说明、复验）→ review（独立实现评审）→ integration（目标、候选、候选测试、候选评审、接纳、发布）→ handoff（证据封存、完成）。Red 必须是预期失败的真实断言，模块加载或环境错误不能包装为 Red。规则强制 Red 先于实现，测试/实现/评审身份独立，并核对确切代码版本。

两条默认流程的步骤确认均为 automatic。模型产出、独立评审、真实测试、步骤接纳和 Git 门禁仍分别执行；automatic 不代表跳过完成规则。显式选用 human 策略或人工例外时，才产生对应人类决定。

步骤的读写与工具授权不在 Workflow 中，由 Permission Context 的步骤授权叠加（见 [运行时权限](permissions.md)）；Feature/Task 各自可采用的规范见 Standard 快照与工作采用（见 [领域规范](domain.md)）。

## 预算与并发

步骤预算含轮次/时长/重试。创作默认值为 8 轮、300 秒、3 次重试；出厂业务 Agent 步骤大多为 30 轮、1800 秒，两个 candidate_review 步骤为 8 轮、1800 秒，重试上限均为 3。foreach 的出厂并发为 1，可配置至 32，写者按 worktree 互斥；会话同时受全局和项目额度约束。默认控制策略每次唤醒 8 轮、每轮最多 64 次工具调用、600 秒。人工等待与祖先暂停不消耗执行预算。

## Rust / Agent 根实例调度

项目/工作选择的 `orchestration` 包含 `mode: manual | rust | agent`，Rust 模式须明确引用 `orchestration_policy` 源修订。策略规定每轮动作上限、根实例 worker 并发上限、租约时间和可重试的结构化技术错误码/次数；缺少可信错误码、未知副作用、业务拒绝或人工门禁时等待。默认策略不开启技术重试。所有自动动作仍受冻结 Permission Context 的 allowed/human operations、步骤预算及同一 dispatch/accept/complete 内核约束。

根实例的 `workflow_driver` 保存策略快照、来源和 epoch；子实例继承该控制归属。`workflow.driver.select` 是人类入口，使用根实例 revision CAS；切换撤销原经理控制代次、未派发控制意图和委托，不停止业务 worker。已有运行 worker/结果继续通过实际回执接纳。Rust tick 从分页的共同 inspect/ready 投影稳定选择动作，事务内保存决定及外部 worker 意图；外部效果由现有 intent/receipt 管线执行。每项逻辑动作有幂等键，计划和代次变化后旧动作不能生效。

`query.workflow_driver` 展示模式、精确策略、epoch 和等待事项。项目管理可选择未来默认模式，流程的经理协作面可切换当前根实例。没有选择驱动的已有实例维持明确的人工控制行为。经理工作台查询/配置创作与根实例调度授权分别处理。

Agent 的实际任务包含冻结的 `runtime_context`：`runtime_context.binding` 是本次 Session 选择的运行资源身份；`runtime_context.inputs.<input>.runtime_executions` 来自对应上游 Turn 的真实 Harness 执行和停止证明，包含镜像、operation 与执行身份，并以 source_output_id 关联产物。同一运行事实也出现在 `input_sources.<input>.runtime_executions`。前者是选择，后者是已观察到的外层执行事实，均不能证明任意嵌套或远程程序的环境。该投影排除凭据、模型连接、模型参数与宿主可执行路径；不会从模型自述推导环境事实。旧任务仍按实际发送的历史合同读取。

完成规则为 false 的 `awaiting_acceptance` 步骤同样出现在驱动等待事项中，标明 `completion` 或 `human_confirmation` 门禁及父子实例路径。诊断不会批准报告、改写完成规则或自动补入缺失证据。

实现依据：[领域原语](../crates/domain/src/workflow.rs)、[变更操作](../crates/domain/src/workflow_change.rs)、[Rust 驱动](../crates/core/src/workflow_driver.rs)、[控制工具](../crates/core/src/controllers.rs)、[公开运行事实](../crates/core/src/workflow_runtime_facts.rs)、[出厂流程](../resources/configuration/workflows/)。

## 共同动作资格

`workflow.inspect` 的每步 `actions.dispatch/accept/retry` 返回 allowed、稳定的阻塞 code 和 reason；dispatchable 与同一派发资格一致。foreach 槽位不足、暂停、输入缺失、完成/人工门禁和真实执行者尚未停止分别表达。Rust、经理和 UI 使用相同事实；提交仍在事务内核对计划、控制身份和当前资格。Rust 在有界 tick 内跳过受阻或陈旧候选，继续处理其他合法工作，不能通过增加额度或绕过门禁推进。

技术重试按当前业务轮次中的执行历史计算次数，与补充输入的 sequence 分开；每次新执行以 sequence=1 开始，retry_of 关联旧执行，原冻结输入和最近补充指令继续传给后继任务。步骤 retry_limit 与 Rust 策略 max_retries 共同限制重试，重启不重置次数，业务返工另起 round。自动重试只消费可信端口明确给出的 technical/not_applied 失败及所选错误码；模型文字、未知副作用、规范拒绝和未停止 writer 都不能触发自动重试。失败保存结构化 code、kind、effect 和实际来源引用，原意图终态不接受迟到结果改写。

结果未知的执行继续保留根 worker 和 foreach 槽位，核对前不能把它视为空闲容量。

默认 Codex 业务角色获得只读 spec.lint，通过所选 Permission Context 的逐步授权使用。Agent 从 outputs.<port>.definition.id 取得精确 Delivery 引用，在提交前检查草稿；关键评审的定位、摘要和引文仍由同一接纳检查器复算。默认 PRD/Design 后继 Spec 已声明必要章节，关键评审后继 Spec 要求真实冻结输入位置；既有工作不会随出厂选择改变而升级。自检报告明确区分结构、报告证据和语义判断，不替代独立评审或真实工具结果。

Task 所属流程派发还要求该 Task 的业务依赖全部 completed。该条件与步骤依赖分别核对，通过共同资格入口向 inspect、Rust 及命令返回 task_dependencies；依赖等待不占用已派发工作额度，也不阻止其他合法 Task 推进。工作空间仍只在依赖完成后从当时父工作实际 HEAD 准备，完成时再次核对业务依赖。
