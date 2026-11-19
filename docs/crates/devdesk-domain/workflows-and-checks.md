# 工作流结构与完成规则

[workflow.rs](../../../crates/domain/src/workflow.rs) 定义 WorkflowDefinition、StepDefinition、StepExecutor、StepExecution 和 CompletionRule。它说明什么是一个合法的流程，以及给定事实后怎样判断条件成立。

## 从定义到判断

StepDefinition 声明输入输出、执行器、依赖、确认方式及访问范围。结构检查需要确认依赖和端口引用有效，图中没有不允许的循环。步骤 key 用于引用，不负责选择隐藏的业务实现。

StepExecutor 区分 Agent、平台工具或子工作流等执行方式。具体派发交给 core；domain 不知道容器是否正在运行。

`CompletionRule::evaluate(&WorkflowFacts)` 接收已经整理好的事实，求值 All/Any、相等、覆盖、先后、独立性、结果和批准等条件。它不会自行查找数据库中的“最新测试”。core 必须提供正确范围和来源的 WorkflowFacts。

## 输出检查如何避免空泛结论

[spec_checks.rs](../../../crates/domain/src/spec_checks.rs) 的检查报告将结论与输入、要求和证据关联。core 在输出接纳时调用相应校验。评审者声称“全部通过”不能替代缺失的输入依据，也不能绕过独立性约束。

## 运行状态的含义

StepExecutionStatus 区分 queued、running、waiting_input、waiting_confirmation、awaiting_acceptance、completed、failed、cancelled、skipped 和 needs_reconciliation。声明 enum 不会自动完成状态迁移；允许的转换在 core 的用例中检查。

[workflow_change.rs](../../../crates/domain/src/workflow_change.rs) 表达计划变更，[workflow_failure.rs](../../../crates/domain/src/workflow_failure.rs) 区分失败类别和副作用状态，[workflow_time.rs](../../../crates/domain/src/workflow_time.rs) 支持时间与预算计算。调用方提供时间，domain 不直接读取时钟。

相关验证：本文件内纯规则测试，以及 [configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs)、[workflows.rs](../../../crates/adapters/tests/workflows.rs)。执行逻辑见 [core 工作流](../devdesk-core/workflow-execution.md)。
