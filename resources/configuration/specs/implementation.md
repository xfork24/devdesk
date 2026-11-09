# Task TDD 实现与交付阶段 Spec

## 阶段意图

在冻结 Task 范围内以独立测试驱动实现。

## 所需能力

测试设计、TDD 实现与工程说明。Agent 必须引用相应 Skill；执行职责与写范围仍受内核规则约束。

## 必需交付内容

- 独立测试与真实 assertion Red。
- Red→implement→Green→verify 依赖。
- 代码、文档和验收映射。
- 实际测试结果、失败、限制和独立评审。

## 完成与拒绝标准

缺少断言 Red、修改独立测试、越界写入或没有真实 Green/verify 都不能通过；Task 不等待未来 Feature 收尾证据。

## 检查方式

由当前 Step 绑定本 Spec 精确修订，并提供给执行 Agent 与独立评审。Markdown 要求按内容和真实证据评审；现有 traceability、failure_paths、sections 只承担各自已实现的有限自动检查，不代表全文要求已自动验证。
