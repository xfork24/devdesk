# Test Plan 与测试设计阶段 Spec

## 阶段意图

确定验收如何由独立测试验证。

## 所需能力

测试设计与独立 TDD 验证。Agent 必须引用相应 Skill；执行职责与写范围仍受内核规则约束。

## 必需交付内容

- 验收到正常、错误及边界用例的映射。
- Task 测试与 Feature 测试的边界。
- 固定镜像、命令、文件和超时。
- 有效 assertion Red 与环境失败的区分。

## 完成与拒绝标准

测试方案可执行、可追溯，输出 test_plan 提案和明确 TestRecipe；不得伪造执行证据。

## 检查方式

由当前 Step 绑定本 Spec 精确修订，并提供给执行 Agent 与独立评审。Markdown 要求按内容和真实证据评审；现有 traceability、failure_paths、sections 只承担各自已实现的有限自动检查，不代表全文要求已自动验证。
