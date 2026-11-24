# 返工、计划变更与恢复

这组模块处理已经存在的执行。重点是保留原输入、拒绝和外部操作身份，避免把新尝试与旧执行混在一起。

## 先选择正确的操作

| 情况 | 主要模块 | 处理原则 |
|---|---|---|
| 技术失败后再次尝试 | [workflow_retries.rs](../../../crates/core/src/workflow_retries.rs) | 保留原条件，检查允许重试与预算 |
| 评审拒绝后修改结果 | [workflow_rework.rs](../../../crates/core/src/workflow_rework.rs) | 明确返工对象、范围、原因和前后关系 |
| 变更后续计划 | [workflow_change_preview.rs](../../../crates/core/src/workflow_change_preview.rs)、[workflow_change_commit.rs](../../../crates/core/src/workflow_change_commit.rs) | 先预览影响和核对执行停止，再应用 |
| 外部结果未知 | [workflow_reconciliation.rs](../../../crates/core/src/workflow_reconciliation.rs) | 先核对原操作，不创建盲目重复动作 |
| 用户停止或继续 | [workflow_execution_controls.rs](../../../crates/core/src/workflow_execution_controls.rs) | 检查执行身份、当前状态与实际 worker 条件 |

## 计划变更为何要分阶段

预览计算受影响的步骤和输入，提供调用者能够审阅的结果。应用前需要使相关执行停止或失去写权限，再检查原计划和控制版本。否则旧容器可能在新计划启用后继续写入，迟到结果也可能错误覆盖新执行。

应用成功后仍保留旧计划和历史结果。旧拒绝不能被改写成成功；后续执行通过关联解释它如何修正问题。

## 恢复从哪里读

[bootstrap_recovery.rs](../../../crates/core/src/bootstrap_recovery.rs) 处理启动后的恢复条件，[worker_reconciliation.rs](../../../crates/core/src/worker_reconciliation.rs) 检查 worker，[platform_reconciliation.rs](../../../crates/core/src/platform_reconciliation.rs) 检查平台操作，[delivery_recovery.rs](../../../crates/core/src/delivery_recovery.rs) 处理工程交付恢复。

外部观察结果可能先于业务提交保存。再次处理时应该利用原结果和操作身份核对，不能因为业务状态尚未更新就重新发出副作用。

## 修改与测试

修改返工规则时检查：同一拒绝是否可重复消费、额度是否共享、执行范围是否扩大、人工决定是否匹配、旧回执是否被保留。修改恢复时覆盖“动作已发生但提交未完成”，不能只测动作尚未开始的情况。

测试入口：[workflow_resource_concurrency.rs](../../../crates/adapters/tests/workflow_resource_concurrency.rs)、[publication_retry.rs](../../../crates/adapters/tests/publication_retry.rs)、[delivery_migration.rs](../../../crates/adapters/tests/delivery_migration.rs)，界面用例见 [review-rework.spec.ts](../../../tests/browser/review-rework.spec.ts)。
