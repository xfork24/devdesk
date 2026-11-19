# 控制授权、经理会话与 Rust 驱动

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

manual、rust 和 agent 共用实际业务控制规则。驱动只选择动作；当前计划、Actor、版本和真实回执决定动作能否提交。

<a id="file-src-orchestration-rs"></a>

## 冻结计划与有限控制

[orchestration.rs](../../../crates/core/src/orchestration.rs) 发布冻结计划记录并提供有界控制读取。业务控制分别由 [workflow_controls.rs](../../../crates/core/src/workflow_controls.rs)、[workflow_authorization.rs](../../../crates/core/src/workflow_authorization.rs)、[workflow_control_reads.rs](../../../crates/core/src/workflow_control_reads.rs) 和 [workflow_rules.rs](../../../crates/core/src/workflow_rules.rs) 执行。

派发、接纳、人工决定及完成核对原实例/节点/execution、输入输出摘要、控制版本和可信调用者。暂停、Task 依赖、子流程容量、计划修改和未知效果参与资格判断，不能凭旧查询结果绕过提交时校验。

<a id="file-src-controllers-rs"></a>

## 经理会话与控制调用

[controllers.rs](../../../crates/core/src/controllers.rs) 声明控制工具和共享合同；[controller_runtime.rs](../../../crates/core/src/controller_runtime.rs) 协调有界实际工作切片，保存原 Session/输入意图后才交由 Host 物化。

[controller_calls.rs](../../../crates/core/src/controller_calls.rs)、[controller_interactions.rs](../../../crates/core/src/controller_interactions.rs) 与 [controller_events.rs](../../../crates/core/src/controller_events.rs) 接纳原生调用、交互和事件。控制授权绑定当前 root、Session、Turn/worker 及 epoch；后继会话保留旧依据，旧执行者不能继续控制新计划。经理独立工作台不自动获得根调度权。

<a id="file-src-workflow-driver-rs"></a>

## 驱动选择与运行

[workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) 校验驱动选择及冻结调度合同，[workflow_driving.rs](../../../crates/core/src/workflow_driving.rs) 推进 Rust 根调度。调度预算来自所选 Workflow，实际动作仍经业务用例和资源级提交。

[driver_capabilities.rs](../../../crates/core/src/driver_capabilities.rs) 保存实际驱动注册。当前 Codex/DSH 都支持动态工具及平台工具，根 orchestration 仅由 Codex 声明；配置字符串不能添加实现能力。

<a id="file-src-workflow-eligibility-rs"></a>

## 资格展示与提交校验

[workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs) 定义 allowed/code/reason 展示值。[workflow_readiness.rs](../../../crates/core/src/workflow_readiness.rs) 与 [workflow_control_reads.rs](../../../crates/core/src/workflow_control_reads.rs) 读取当前有限事实，计算可用动作与缺口。

显示允许最终一致，提交必须重新校验权威版本和授权。冲突保留原草稿与原请求，不自动更新版本或静默重发。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
