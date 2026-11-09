# Design Doc 与架构设计阶段 Spec

## 阶段意图

把已采纳 PRD 转为可实现的系统设计。

## 所需能力

架构设计与 Design Doc。Agent 必须引用相应 Skill；执行职责与写范围仍受内核规则约束。

## 必需交付内容

- 精确 requirement_id 及验收映射。
- 组件边界、接口、数据和不变量。
- 方案取舍、安全、错误与恢复。
- 验证、迁移和演进计划。

## 完成与拒绝标准

Design Doc 能支持独立拆分 Task；不以模型口头保证代替接口、数据或失败设计，采纳设计后才进入任务拆分。

## 检查方式

由当前 Step 绑定本 Spec 精确修订，并提供给执行 Agent 与独立评审。Markdown 要求按内容和真实证据评审；现有 traceability、failure_paths、sections 只承担各自已实现的有限自动检查，不代表全文要求已自动验证。
