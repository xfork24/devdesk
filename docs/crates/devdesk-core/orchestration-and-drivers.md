# 控制命令、经理会话与 Rust 调度

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

人工、Agent 和 Rust 驱动最终调用同一组控制命令，但动作来源不同。orchestration 负责控制资格及事实接纳，controllers 负责原生经理调用与会话代次，workflow_driver 负责有界选择动作。查询提示仍须由提交时的真实状态再次核对。

## 本页文件

- [src/orchestration.rs](#file-src-orchestration-rs)：显式控制、实际接纳、材料读取与动作凭据
- [src/controllers.rs](#file-src-controllers-rs)：原生经理会话、唤醒和控制调用围栏
- [src/workflow_driver.rs](#file-src-workflow-driver-rs)：Rust 根驱动的动作选择、预算与围栏
- [src/workflow_eligibility.rs](#file-src-workflow-eligibility-rs)：查询与提交共用的业务动作资格

<a id="file-src-orchestration-rs"></a>

## src/orchestration.rs：显式控制、实际接纳、材料读取与动作凭据

源码：[crates/core/src/orchestration.rs](../../../crates/core/src/orchestration.rs)。

本文件把“观察结果”与“决定推进”分开。`freeze_plan` 保存不可变计划采用，decision/waiver 记录真实 Actor 的独立处置；workflow_control_command 核对计划、control_version、步骤/执行、权限与状态，再派发、接纳、停止、核对、完成或请求变更。

派发检查依赖已接纳、实例/祖先未暂停、无待应用变更，并在事务内检查父 foreach 的槽位；接纳绑定确切 input_digest 和输出集合，核对完成条件、人类门禁和真实 writer。`workflow_action_permit` 将当前业务测试/确认要求或明确 waiver 绑定到精确候选和目标，供 Git 动作使用。

查询提供分页 inspect、根范围事件、输出和历史 execution。大 Unicode 材料按字节边界分块并校验 digest，业务 Agent 还必须证明目标是自身显式输入。导入材料和 bypass 替代保留来源，不能凭普通 JSON 冒充平台回执。

inspect 为每步返回 actions.dispatch/accept/retry 及结构化 code/reason，实际控制命令复用 workflow_eligibility。它包括 Task 业务依赖、foreach 槽位、暂停、变更、输入、writer 停止、完成和确认门禁。查询与提交之间仍可能发生并发变化，命令继续复核授权、CAS 和确切版本，查询不是预先签发的执行许可。

代码定位：[`freeze_plan`](../../../crates/core/src/orchestration.rs#L6)、[`workflow_action_permit`](../../../crates/core/src/orchestration.rs#L282)、[`orchestration_command`](../../../crates/core/src/orchestration.rs#L375)、[`workflow_control_command`](../../../crates/core/src/orchestration.rs#L519)、[`workflow_control_query`](../../../crates/core/src/orchestration.rs#L882)、[`read_chunk`](../../../crates/core/src/orchestration.rs#L218)。

关联阅读：[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)、[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/workflow_changes.rs](workflow-changes.md#file-src-workflow-changes-rs)、[core/src/platform_tools.rs](manager-and-platform-tools.md#file-src-platform-tools-rs)、[adapters/tests/orchestration.rs](../devdesk-adapters/testing.md#file-tests-orchestration-rs)、[core/src/workflow_eligibility.rs](orchestration-and-drivers.md#file-src-workflow-eligibility-rs)。

<a id="file-src-controllers-rs"></a>

## src/controllers.rs：原生经理会话、唤醒和控制调用围栏

源码：[crates/core/src/controllers.rs](../../../crates/core/src/controllers.rs)。

`CONTROL_TOOLS` 列出 21 个 workflow 控制工具。`controller_command` 管理根编排或 workspace_only 项目经理的打开、输入、接管、恢复和后继；选择完整 Agent 与 Profile 选择，并检查实际驱动支持 orchestration。

`drive_controllers` 比较持久事件/状态、用户输入和结果摘要，生成有限唤醒和原生 Turn；没有新状态/输入时不反复调用模型。ControllerSessionBinding、epoch、lease 和真实 Session/Turn/worker 共同确定当前权限，接管或后继使旧调用失效。预算围栏与业务步骤预算分开。

原生调用先保存请求、工具引用和调用链，再由 project_controller_tool/drive_controller_calls 校验授权并执行；接纳业务回执与把工具回复交回模型是两笔事实，重启不能因为回复丢失就再次执行副作用。超大 inspect 结果采用有界投影，单元测试保护真实 completion/human 门禁不被裁掉。

经理重采用采用先停止并核对旧 writer、快照后建立新会话的路径；不修改旧 Session 中的冻结 Agent/权限。项目经理工作台可独立存在，根调度权仍按具体根 Controller/Driver 核验。

代码定位：[`CONTROL_TOOLS`](../../../crates/core/src/controllers.rs#L5)、[`controller_command`](../../../crates/core/src/controllers.rs#L145)、[`controller_agent_request`](../../../crates/core/src/controllers.rs#L554)、[`project_controller_tool`](../../../crates/core/src/controllers.rs#L604)、[`drive_controllers`](../../../crates/core/src/controllers.rs#L716)、[`drive_controller_calls`](../../../crates/core/src/controllers.rs#L791)。

关联阅读：[core/src/manager_workspace.rs](manager-and-platform-tools.md#file-src-manager-workspace-rs)、[core/src/orchestration.rs](orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)。

<a id="file-src-workflow-driver-rs"></a>

## src/workflow_driver.rs：Rust 根驱动的动作选择、预算与围栏

源码：[crates/core/src/workflow_driver.rs](../../../crates/core/src/workflow_driver.rs)。

驱动选择固定在根实例，子实例继承同一控制归属。workflow_driver_command 校验人类入口、根 revision 和精确策略；每次选择提高 Driver epoch，停止旧 Segment，切到非 agent 模式时还暂停相关 Controller、提高其 epoch、请求取消经理活动 Turn 并撤销其 prepared 意图。选择 agent 则打开或恢复相应 Controller，不一概撤销它。业务 worker 保留；authorize_driver_action 在每次提交重新核对 mode、epoch、租约与计划。

drive_rust_workflows 续租后在 max_actions_per_tick 内分页读取共同 inspect，按子实例、步骤动作资格选择 dispatch、accept、retry 或 complete。它记录本 tick 已被拒绝的 method/execution/control_version 候选；blocked、revision_conflict、change_pending 发生时继续在剩余预算内读取并寻找其他合法动作，其他错误结束本轮。这样受阻子流程不会持续抢在另一个可接纳结果前面，也不会无限忙重试。

自动技术重试要求共同 retry 资格允许、独立 retry_count 小于策略 max_retries、失败为 Technical + NotApplied、错误码在精确允许集合内，并受步骤 retry_limit、停止与权限门禁限制。已消费输入后的结果未知、业务拒绝和结构不合格不会靠重复执行刷成成功；当前出厂策略仍为 max_retries=0。

动作使用稳定幂等键与 Driver InvocationContext，在事务内保存决定及 worker 意图，外部效果沿已有管线执行。running 与 needs_reconciliation 的 Agent/Tool 占用根 worker 额度，未知执行不能偷偷释放名额。查询包含 actions 与等待原因，UI/经理和驱动读取相同业务门禁。

代码定位：[`validate_selection`](../../../crates/core/src/workflow_driver.rs#L5)、[`workflow_driver_command`](../../../crates/core/src/workflow_driver.rs#L47)、[`authorize_driver_action`](../../../crates/core/src/workflow_driver.rs#L214)、[`workflow_driver_query`](../../../crates/core/src/workflow_driver.rs#L283)、[`drive_rust_workflows`](../../../crates/core/src/workflow_driver.rs#L340)。

关联阅读：[domain/src/workflow_driver.rs](../devdesk-domain/workflows-and-checks.md#file-src-workflow-driver-rs)、[core/src/orchestration.rs](orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/workflow_runtime.rs](workflow-execution.md#file-src-workflow-runtime-rs)、[adapters/tests/orchestration.rs](../devdesk-adapters/testing.md#file-tests-orchestration-rs)、[core/src/workflow_eligibility.rs](orchestration-and-drivers.md#file-src-workflow-eligibility-rs)、[core/src/workflow_failure.rs](workflow-execution.md#file-src-workflow-failure-rs)。

<a id="file-src-workflow-eligibility-rs"></a>

## src/workflow_eligibility.rs：查询与提交共用的业务动作资格

源码：[crates/core/src/workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs)。

WorkflowActionEligibility 返回 allowed、稳定 code 与可读 reason；WorkflowStepActions 为 dispatch/accept/retry 分别给出结论。common 拒绝 Completed、Cancelled、已请求取消或存在待应用计划的实例；Failed 不被一概当成不可恢复终态。授权、当前 Actor、CAS 仍在命令接纳检查，资格查询不是一张长期有效的 permit。

派发检查 queued、祖先暂停、Task 业务依赖、步骤依赖、foreach 槽位和显式输入。needs_reconciliation 的兄弟仍占槽位；condition 为 false 时允许显式 dispatch 记录配置的 skipped，而不是先要求被跳过步骤的输入可用。接纳检查结果状态、真实 writer 停止、完成规则/确切人工覆盖和 human 确认；技术 retry 检查失败/取消状态、停止和同 round 独立次数预算。

completion_action 核对全部步骤处置、整体完成规则和最终输出。orchestration.inspect、Rust 查询/选择以及真实 dispatch/accept/retry/complete 复用这些函数，事务中仍重新计算，防止 stale 查询绕过变化。

代码定位：[`WorkflowActionEligibility`](../../../crates/core/src/workflow_eligibility.rs#L7)、[`WorkflowStepActions`](../../../crates/core/src/workflow_eligibility.rs#L43)、[`step_actions`](../../../crates/core/src/workflow_eligibility.rs#L72)、[`step_actions_with_facts`](../../../crates/core/src/workflow_eligibility.rs#L89)、[`completion_action`](../../../crates/core/src/workflow_eligibility.rs#L211)。

关联阅读：[core/src/orchestration.rs](orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/workflow_failure.rs](workflow-execution.md#file-src-workflow-failure-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
