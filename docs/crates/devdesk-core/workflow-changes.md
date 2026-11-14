# 运行计划修改、静止等待与有界委托

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

修改运行中的图不能直接替换 JSON。预演先计算影响、合同与材料缺口，再停止受影响执行者，保留原执行历史并建立新计划。未受影响节点可携带旧回执继续；已完成根实例保持终态。

## 本页文件

- [src/workflow_changes.rs](#file-src-workflow-changes-rs)：预演影响、停止执行者、切换计划与 Segment

<a id="file-src-workflow-changes-rs"></a>

## src/workflow_changes.rs：预演影响、停止执行者、切换计划与 Segment

源码：[crates/core/src/workflow_changes.rs](../../../crates/core/src/workflow_changes.rs)。

`change_preview_in` 在当前冻结计划上应用类型化操作，重新编译候选，计算受影响 node_id、输入/权限/角色与 Standard 变化、must_stop 和 carry-forward。预演哈希固定基准和候选，调用方不能提交另一份图沿用旧哈希。

`apply_workflow_change` 保存变更与静止意图，`drive_workflow_changes/advance_workflow_change` 等待旧执行者停止，封存 workflow_execution_history，取消受影响子实例，再建立新 Workflow 源/计划修订。未受影响的合同和回执显式携带到新计划，原拒绝与晚到观察继续保留。子流程仅保留它实际使用的资源闭包，不因父级多余绑定而失效兄弟工作。

回退不是任意撤销历史：revert_preview 仅接受最近一次纯 UpsertStep 变更、替代执行尚未开始且无不允许的子流程/混合变化。它创建新计划恢复原合同，不删除旧变更。AdoptStandard 同样走预演、授权、静止和新计划路径。

Segment 记录对有限 automatic 节点的明确授权，逐个使用现有控制命令；遇到拒绝、人工门禁、计划变化或预算边界即停，不自动完成根流程。修改 Task 范围/依赖还需 control_policy 明确许可。

代码定位：[`Preview`](../../../crates/core/src/workflow_changes.rs#L9)、[`change_preview_in`](../../../crates/core/src/workflow_changes.rs#L161)、[`revert_preview`](../../../crates/core/src/workflow_changes.rs#L50)、[`apply_workflow_change`](../../../crates/core/src/workflow_changes.rs#L684)、[`drive_workflow_changes`](../../../crates/core/src/workflow_changes.rs#L1127)、[`dispatch_workflow_segment`](../../../crates/core/src/workflow_changes.rs#L1155)。

关联阅读：[domain/src/workflow_change.rs](../devdesk-domain/workflows-and-checks.md#file-src-workflow-change-rs)、[core/src/orchestration.rs](orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[adapters/tests/orchestration.rs](../devdesk-adapters/testing.md#file-tests-orchestration-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
