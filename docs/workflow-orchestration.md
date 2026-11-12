# 工作流与编排

本文描述工作流的**运行语义**：谁推进步骤、产物如何被接纳、什么条件下可以批准或豁免、运行中的计划如何修改。概念定义见 [领域规范](domain.md)，命令合同见 [本地协议](protocol.md)；出厂流程与默认配置清单见 [默认目录](../resources/configuration/README.md)。

## 定义与发布

Workflow 是可发布的流程文档（当前 schema v6）：Stage 提供背景与可选默认 Role；Step 声明任务指令、执行方式、必需 Skill、验收 Spec、输入输出端口、工作上下文、预算与完成条件。Step 执行方式只有四种：**Agent**（绑定 Role）、**Tool**（平台或外部工具）、**Workflow**（子流程，可按集合 foreach 展开）、**Control**（纯控制/确认节点）。

草稿允许不完整引用；`workflow.publish` 强制完整编译配置闭包（子流程递归深度 ≤8、展开 ≤1000 步），发布即不可变修订。步骤在发布时获得稳定 node_id，重命名不影响运行中实例。

## 三种推进模式

系统没有"自动遍历就绪节点"的隐式推进；每一步执行都需要显式派发：

| 模式 | 操作者 | 入口 |
|---|---|---|
| 人工逐步 | 用户在执行与交付页 | `workflow.step.dispatch / accept / approve / input / retry / rework` |
| 项目经理 Agent | WorkflowController 编排会话经 21 个控制工具 | `orchestration.open` 后自主派发/接纳/完成 |
| 有界段委托 | 把 1–32 个 automatic 节点批量委托给 Controller | `workflow.segment.dispatch` |

无论哪种模式，**core 都是权威终态裁判**：完成规则求值、证据核验、人工确认语义、权限门禁全部在 Rust 内核；Agent 与前端只能提议，不能裁定。

## 步骤生命周期与产物接纳

StepExecution 有 10 种状态（queued → running → waiting_input / waiting_confirmation / awaiting_acceptance → completed / failed / cancelled / skipped / needs_reconciliation）。执行完不等于被接纳：

1. 执行者完成 → 统一封套（status/summary/outputs）经逐端口校验（声明的端口之外拒绝、交付类型 content_schema、Spec 内容检查逐项带证据）。
2. 封存 Artifact（SHA-256）并写不可变 WorkflowOutput。
3. 按步骤确认策略（automatic/human）进入等待确认或等待接纳；`workflow.step.accept` 绑定确切的 input_digest 与输出 ID。

技术重试（retry）保留输入并创建新执行；业务返工（rework）沿依赖闭包重置后继步骤。模型自述永远不能替代真实测试与 Git 回执。

## 完成规则与四层批准语义

完成条件是纯数据规则树（全部/任一/存在/值相等/集合覆盖/先后/不同执行身份/人工决定），只在协调者从已接受回执构建的事实表上求值。"批准"分四层，不可混淆：

| 层 | 机制 | 谁批准 |
|---|---|---|
| 步骤确认 | ConfirmationPolicy + `workflow.step.confirm` | human（automatic 步骤免签署） |
| 完成规则不满足时的人工例外 | `workflow.step.approve`（需理由，步骤须允许覆盖） | 仅 human |
| 人工决定 | `workflow.decision.resolve`（Controller 的 decision.request） | 仅 human |
| 豁免 | ChangeSet 的 Waive（完成/确认/测试/独立性/覆盖） | human；Controller 受控制策略限制 |

`Independent` 规则按执行身份根判定：评审者与作者必须是不同身份，Agent 换新修订不构成独立评审。豁免从不伪造回执或改写事实，保留原值并单独记录理由。

## 运行中修改计划

ChangeSet 提供 12 类类型化操作（增删改步骤、绕过阶段、改依赖、改输入、豁免、回退等）。协议固定为：**预演 → 等待受影响执行者静止 → 原子应用**。

- `workflow.change.preview` 生成不可变预演快照（受影响节点、must_stop、标准差异、preview_hash）。
- `workflow.change.apply` 校验哈希后：请求受影响执行者停止 → 全部静止 → 受影响执行写入不可变 workflow_execution_history（原始合同与迟到观察保留）→ 后代子实例取消 → 未受影响步骤按 node_id 保留 → 冻结新计划修订。
- 回退（revert）受限：仅限最近一次应用的纯 UpsertStep 变更，且受影响执行从未启动。

## TDD 证据链与平台动作门禁

测试与候选的事实只能来自真实回执：`tests.run` 按冻结配方（固定镜像 SHA、文件清单、超时）在无网只读容器执行，区分断言失败与环境失败；`git.candidate` 产出单父提交与确切 diff；`git.advance` 必须持有 WorkflowActionPermit——按计划的 action_requirements 核对与候选匹配的成功测试回执；`git.publish` 用精确租约推送并创建 GitHub PR（平台不合入）。

## 默认流程概览

默认入口为 `builtin-workflow-feature-sol-high:6`（7 阶段 22 步）与 `builtin-workflow-task-sol-high:6`（7 阶段 20 步）：

- **Feature 流程**：requirements（PRD + 独立评审）→ design（基线快照、工程约束、Design Doc + 评审）→ planning（Task Plan + 评审 + tasks.create）→ implementation（foreach 对每个 Task 运行 Task 子流程）→ verification（回归测试配方、版本冻结、真实执行）→ delivery（Feature 报告 + 评审）→ integration（候选、候选测试、候选评审、接纳、发布）。
- **Task 流程**：tests（独立测试，只可写 tests 目录）→ red（版本冻结、Red 基线、真实 Red——必须预期失败的真断言）→ implementation（实现、版本冻结、Green、复验）→ documentation（只读补充交付说明）→ integration（候选、候选测试、评审、接纳、发布）→ handoff（证据封存、完成）。流程级规则强制 Red 先于实现、测试/实现/评审三方身份独立、Green 跑在与快照一致的确切代码上。

步骤的读写与工具授权不在 Workflow 中，由 Permission Context 的步骤授权叠加（见 [运行时权限](permissions.md)）；Feature/Task 各自可采用的规范见 Standard 快照与工作采用（见 [领域规范](domain.md)）。

## 预算与并发

步骤预算含轮次/时长/重试（默认 authoring 预算 8 轮/300 秒/3 次；默认流程 Agent 步骤实际为 30 轮/1800 秒）。foreach 并发默认 1，可配置至 32（写者按 worktree 互斥）。会话并发 = min(全局额度, 项目额度) − 在跑轮次。编排唤醒有独立预算（轮次/工具调用/超时），人工等待与祖先暂停不消耗执行预算。
