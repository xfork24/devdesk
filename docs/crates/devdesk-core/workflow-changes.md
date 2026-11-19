# 运行计划修改、静止等待与有界委托

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

修改运行中的图不能直接替换 JSON。预演先计算影响、合同与材料缺口，再停止受影响执行者，保留原执行历史并建立新计划。未受影响节点可携带旧回执继续；已完成根实例保持终态。

## 当前实现

<a id="file-src-workflow-changes-rs"></a>

- [workflow_change_preview.rs](../../../crates/core/src/workflow_change_preview.rs) 读取完整配置目录与明确选择的运行事实，生成 `NativePreview`、影响集合、停止集合及预演摘要。
- [workflow_change_commit.rs](../../../crates/core/src/workflow_change_commit.rs) 核对原预演与资源版本，持久化变更和静止意图，接纳停止证明并提交新计划。
- [workflow_segments.rs](../../../crates/core/src/workflow_segments.rs) 保存有限 automatic 节点的委托，逐项调用现有控制命令。
- [workflow_rework.rs](../../../crates/core/src/workflow_rework.rs) 与 [workflow_retries.rs](../../../crates/core/src/workflow_retries.rs) 分别处理返工影响和独立技术重试。

运行计划修改使用 preview → quiesce → apply。预演固定原计划、源修订、输入、权限、Standard 及实际执行，不允许用另一份图沿用原摘要。停止确认后封存旧执行，受影响子实例按明确规则取消，未受影响回执显式沿用到新计划。

回退仍产生新计划，保留原拒绝、执行历史和迟到回执；已完成根实例不重新打开。修改 Task 范围或依赖受控制策略限制。Segment 遇到拒绝、人工门禁、计划变化或预算边界即停止，不能自动签署人工决定或完成根流程。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
