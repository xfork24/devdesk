# Task Plan 与任务拆分阶段 Spec

## 阶段意图

依据已采纳 PRD 和 Design 形成可执行 Task DAG。

## 所需能力

项目管理与 Task 拆分。Agent 必须引用相应 Skill；执行职责与写范围仍受内核规则约束。

## 必需交付内容

- requirement_id、design_id 与稳定 Task key。
- 每项 Task 的范围和验收映射。
- 无环依赖和汇合条件。
- 每项 Task 的有效 TestRecipe 与相对路径。

## 完成与拒绝标准

所有验收均有责任 Task，依赖完整且无循环；不能用未来测试承诺替代当前可执行的测试配方。

## 检查方式

由当前 Step 绑定本 Spec 精确修订，并提供给执行 Agent 与独立评审。Markdown 要求按内容和真实证据评审；现有 traceability、failure_paths、sections 只承担各自已实现的有限自动检查，不代表全文要求已自动验证。
