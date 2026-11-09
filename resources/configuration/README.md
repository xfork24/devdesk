# 默认配置目录

默认条目是普通、独立版本化定义。程序首次初始化时嵌入 Markdown 正文并建立明确引用；项目页预填默认配置，保存后用于后续 Feature。编辑资源不回写其他定义或已有执行锁。

| Role / Agent | Capability 中的专用 Skill | 主要交付与阶段 Spec |
|---|---|---|
| 需求分析师 | [PRD](skills/prd.md) | PRD / [需求规范](specs/prd.md) |
| 架构师 | [架构设计](skills/architecture.md) | Design Doc / [设计规范](specs/architecture.md)、[方案交付](specs/solution.md) |
| 项目经理 | [项目管理](skills/project-planning.md) | Task Plan、依赖及 TestRecipe / [拆分规范](specs/project-planning.md) |
| 开发 | [TDD 实现与文档](skills/tdd-implementation.md) | 代码、Markdown 说明 / [Task 实现](specs/implementation.md) |
| 测试 | [独立测试](skills/test-design.md)、[Feature/候选验证](skills/release-verification.md) | Test Plan、真实 Red/Green/verify / [测试设计](specs/test-design.md)、[完整验收](specs/closure.md)、[候选合入](specs/release.md) |
| 独立评审 | [分阶段评审](skills/independent-review.md) | 当前交付边界的逐项 Review / 对应 Step 的阶段 Spec |

各 Capability 使用 GPT-6.1 Sol High、固定 Codex Harness、工程上下文 Skill、对应的专用 Skill，以及已有 `harness.native` 工具。AgentID 经验属于检索、定位 Feature/Task 问题与恢复现场的历史上下文，不是 Tool；应用保留经验查询与恢复入口。工具实际由原生 Harness 提供，工具名称不构成授权。Agent 固定默认 Harness/Sandbox Permissions，实际运行使用 Docker；默认镜像配方不是执行证据。

Feature 默认分为八个阶段；Task 默认包含 TDD 实现、独立评审、候选汇合和快照归档。阶段描述取自对应 Spec Markdown。讨论正文是 Markdown，requirement/design/task_plan/test_plan 仍通过已有结构化提案协议保存 ID、验收及测试配方。标准基线可选，Step 的显式 Spec 是实际执行规范来源。


## 默认 Step 任务

`tasks/` 的 22 份 Markdown 在编译时嵌入，播种为新的 Feature/Task Workflow v3 修订。用户在 Workflow UI 编辑任务，发布后正文属于该修订；修改源码资源不会改写已有修订或 Feature 锁。任务没有模板插值；Role 是职责，Skill 是方法，Spec 是判准，阶段描述是背景，均独立保留来源。旧 v2 默认仍可明确采用。

| 原业务提示入口 | 新任务文件 | 保留的系统合同 |
|---|---|---|
| workflow_runtime 讨论 goal | prd / design / planning / discussion-tests / feature | ProposalContent、主题、轮数、显式采纳 |
| requirement_validate / plan_validate / test_plan_validate | validate-requirement / validate-plan / validate-tests；对应 attempt 公共任务 | evidence-checks/v1、只读范围 |
| Task / Feature 测试编写 | author-task-tests / author-feature-tests | 冻结测试写范围、真实测试执行与封存 |
| 实现与文档 | implement / docs；attempt-implementation / attempt-closure | work-summary/v1 / implementation-document/v1、代码写范围与协调器 Git |
| 阶段交付评审 | review-requirement / review-plan / review-task / review-feature | 独立评审身份、四类 checks 与真实证据 |
| 候选评审 | review-candidate-task / review-candidate-feature | 候选来源、只读代码与合入门禁 |

新 v3 路径只传这些已冻结正文和受控材料，不再从 live_runs 的旧分支追加业务提示。旧字符串继续服务旧编译锁；系统测试和 merge.prepare 不调用 Agent。

工程上下文、V1 工作方法、Role 职责、Spec 与 Standard 正文分别维护在 `skills/`、`roles/`、`specs/`、`standards/`。升级时仅为未被用户自定义的内置正文追加修订；原冻结引用、已发表内容与摘要保持不变。DSH 原生工具组通过明确的 Harness 标识限制适用后端，旧通用原生工具绑定继续用于历史组合。
