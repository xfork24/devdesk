# 步骤状态机、执行意图与可信运行事实

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

这组文件处理已经冻结的流程：输入如何被解析，Agent/Tool/子流程如何启动，产物如何保存，取消与恢复如何判断。观察外部完成与业务接纳分开；queued 节点要经过显式派发，等待确认/接纳的产物也不能仅因“执行结束”自动成为业务成功。

## 本页文件

- [src/configured_runtime.rs](#file-src-configured-runtime-rs)：冻结实例的主状态机与任务渲染
- [src/workflow_runtime.rs](#file-src-workflow-runtime-rs)：执行意图的领取和派发结果登记
- [src/live_runs.rs](#file-src-live-runs-rs)：给会话选择代码视图并停止本进程执行者
- [src/workflow_runtime_facts.rs](#file-src-workflow-runtime-facts-rs)：把真实外层运行身份交给后继任务
- [src/workflow_failure.rs](#file-src-workflow-failure-rs)：从持久化来源恢复失败与独立尝试计数

<a id="file-src-configured-runtime-rs"></a>

## src/configured_runtime.rs：冻结实例的主状态机与任务渲染

源码：[crates/core/src/configured_runtime.rs](../../../crates/core/src/configured_runtime.rs)。

`ConfiguredWorkflowInstance` 保存当前计划引用、control_version、owner/root/父调用、输入、步骤执行、暂停和取消状态。执行 ID、node_id、step key、round、sequence 各有用途：节点身份不能代替一次尝试身份，补充输入也不能被当成新任务。

workflow.start 固定整体输入和计划，但新 StepExecution 的 inputs 为空、input_digest 为空。步骤有本计划的显式 dispatch 记录、依赖已处置且 condition 成立后，首次推进才用 `material_inputs` 解析当时可用的显式绑定，计算摘要并保存输入证据清单；技术 retry 复用原输入并核对摘要，rework 才清空后重解析。condition 不成立时直接记录 Skipped，不启动执行器。

`task_input` 组合 Stage、职责、Skill、Spec、输入来源、输出 Schema、预算及授权，形成 canonical JSON。大于内联阈值的输入使用带 digest/offset 的输出引用，前提是 Agent 具有并获授权使用 outputs.read；原值仍封存。逐端口 content_checks 使用本次 Spec 的检查集合，不内置固定四项检查。

四种执行器在这里分流：Agent 建立 session.open 意图，Session 就绪后再建立 conversation.send 意图；Tool 建立 workflow.tool.execute 意图；Control 不启动外部进程，直接进入 WaitingConfirmation 或 AwaitingAcceptance；Workflow 建立精确子实例及 workflow_children，foreach 为每个成员建立调用。`advance_configured_step` 后续观察 Turn、工具和子流程；父调用等待对应子实例完成并映射输出，不按最近一次同名会话找结果。动态 work_binding 从真实子工作选择它自己的 Standard/Workflow。

`accept_configured_outputs` 核验统一封套、端口、结构、内容检查及实际 producer，再封存 Artifact/WorkflowOutput。执行终态与 awaiting_acceptance/人工确认分开。补充输入沿用原任务，retry 创建新 execution，rework 沿真实消费关系失效后继；写任务返工先保存可核验工作区检查点，不回退到旧 Red 基线丢失已写实现。

取消传播到子调用和真实执行者；未知结果保留 needs_reconciliation。等待人类或祖先暂停的时间通过 budget_wait 字段处理。`workflow.step.approve` 对 waiting_confirmation/awaiting_acceptance 保存确切执行、输入、输出和理由的人工例外；它不会把无效结构或未知外部结果改成成功。该大文件应按“输入/渲染 → 命令 → 观察推进 → 输出接纳/查询”阅读。

StepExecution 另保存 failure 与 retry_of。技术 retry 先归档前次执行，保留冻结 inputs/input_digest，建立新 execution 并把 sequence 重置为本次输入轮次起点；重试次数由同实例/node/round 的历史独立计数。retry_supplemental_input 沿有界 retry_of 链恢复最后补充文本，不把补充输入误算成技术尝试。rework 才开启新业务 round 并重新解析相关输入。

失败接纳从工具/意图/Turn 的可信来源建立 WorkflowFailure；Technical/Unknown 会转 needs_reconciliation；工具或 Turn 已成功而随后输出被拒时，来源恢复记录为 Business/Applied，其余校验失败保留实际类别与效果。派发、接纳和技术 retry 复用共同资格；Task 子实例还要等业务 Task 依赖全部完成，不能仅看本流程内部依赖已经满足。

代码定位：[`ConfiguredWorkflowInstance`](../../../crates/core/src/configured_runtime.rs#L7)、[`material_inputs`](../../../crates/core/src/configured_runtime.rs#L449)、[`task_input`](../../../crates/core/src/configured_runtime.rs#L473)、[`configured_runtime_command`](../../../crates/core/src/configured_runtime.rs#L1260)、[`advance_configured_step`](../../../crates/core/src/configured_runtime.rs#L2111)、[`accept_configured_outputs`](../../../crates/core/src/configured_runtime.rs#L2820)。

关联阅读：[core/src/configured_workflows.rs](workflow-compilation.md#file-src-configured-workflows-rs)、[core/src/orchestration.rs](orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/conversation_runtime.rs](conversations-and-history.md#file-src-conversation-runtime-rs)、[core/src/platform_tools.rs](manager-and-platform-tools.md#file-src-platform-tools-rs)、[core/src/spec_lint.rs](manager-and-platform-tools.md#file-src-spec-lint-rs)、[adapters/tests/configured_workflows.rs](../devdesk-adapters/testing.md#file-tests-configured-workflows-rs)、[core/src/workflow_eligibility.rs](orchestration-and-drivers.md#file-src-workflow-eligibility-rs)、[core/src/workflow_failure.rs](workflow-execution.md#file-src-workflow-failure-rs)。

<a id="file-src-workflow-runtime-rs"></a>

## src/workflow_runtime.rs：执行意图的领取和派发结果登记

源码：[crates/core/src/workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs)。

这是外层调度器使用的窄接口。evaluate_workflows 进入配置流程评估；pending_workflow_intents 只取 prepared、属于活跃运行步骤或活动 Controller 的意图，并排除暂停实例，每批最多 16 项。

Host 领取后在事务外处理 session.open、workflow.tool.execute 或 Engine 命令，再用 observe_workflow_intent 写回 completed/result 或完整结构化 Error。已记录的 completed/failed 意图只允许相同结果再次确认，迟到回执不能改写它。

派发动作成功与业务 Step 完成是两件事；后续状态机继续观察真实会话或工具回执。错误中的 execution_failure 由观察边界提供，供 workflow_failure 解析，不能从模型文字推断副作用是否已发生。

代码定位：[`evaluate_workflows`](../../../crates/core/src/workflow_runtime.rs#L4)、[`pending_workflow_intents`](../../../crates/core/src/workflow_runtime.rs#L7)、[`observe_workflow_intent`](../../../crates/core/src/workflow_runtime.rs#L46)。

关联阅读：[host/src/governance.rs](../devdesk-host/rpc-and-configuration.md#file-src-governance-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)。

<a id="file-src-live-runs-rs"></a>

## src/live_runs.rs：给会话选择代码视图并停止本进程执行者

源码：[crates/core/src/live_runs.rs](../../../crates/core/src/live_runs.rs)。

`discussion_code_view` 为普通工程会话从实际 owner 工作副本和已核对 head 构造 FrozenCode，正式步骤优先使用其冻结代码视图。Controller/Orchestration 会话直接返回无代码视图，避免项目经理因为“管理这个 owner”就变成该工作区的写者。

`stop_owned_runtime` 在 Host 退出时只处理本服务拥有的活动 Turn/未撤销 writer，先标记需核对，再让原生会话管线取得真实停止事实。Queued 输入不会被当成已派发执行者强行完成，关闭浏览器也不走这个退出流程。

代码定位：[`discussion_code_view`](../../../crates/core/src/live_runs.rs#L3)、[`stop_owned_runtime`](../../../crates/core/src/live_runs.rs#L64)。

关联阅读：[core/src/conversation_runtime.rs](conversations-and-history.md#file-src-conversation-runtime-rs)、[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[host/src/workspace.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-workspace-rs)。

<a id="file-src-workflow-runtime-facts-rs"></a>

## src/workflow_runtime_facts.rs：把真实外层运行身份交给后继任务

源码：[crates/core/src/workflow_runtime_facts.rs](../../../crates/core/src/workflow_runtime_facts.rs)。

`observed` 只接受与确切 Turn/Session 对应的 Harness 执行：必须有容器身份、writer 已撤销，而且存在同 operation 的 stop-proof 状态事件。缺任何一项都不输出“已观察到的运行事实”。

`public_binding` 投影当前 Session 选择的安装、配方、镜像/可执行摘要及适配器身份；故意排除凭据、模型参数和宿主路径。`context` 沿 workflow_input_manifest 的源产物找到真正生产 Turn，把执行事实绑定到对应输入及 source_output_id。

任务中的 binding 是选择，inputs 是已观察的外层执行；任意嵌套程序或远端服务的环境仍须另有证据。单元测试检查敏感字段不外泄，以及 operation/停止证明不匹配时不能冒认镜像。

代码定位：[`observed`](../../../crates/core/src/workflow_runtime_facts.rs#L5)、[`public_binding`](../../../crates/core/src/workflow_runtime_facts.rs#L31)、[`context`](../../../crates/core/src/workflow_runtime_facts.rs#L46)。

关联阅读：[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/spec_lint.rs](manager-and-platform-tools.md#file-src-spec-lint-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)。

<a id="file-src-workflow-failure-rs"></a>

## src/workflow_failure.rs：从持久化来源恢复失败与独立尝试计数

源码：[crates/core/src/workflow_failure.rs](../../../crates/core/src/workflow_failure.rs)。

retry_count 按 workflow_instance_id、稳定 node_id 和业务 round 收集历史 execution 身份去重计数，不使用补充输入 sequence。技术重试的旧执行保留在 workflow_execution_history，重启或切驱动不会清空尝试预算。

共同 retry 资格只检查 Failed/Cancelled、原执行者静止和步骤次数预算；人工显式重试并不要求失败一定为 Technical/NotApplied。Rust 自动重试才进一步要求可信安全分类、策略允许码与 max_retries。不能把自动策略的保守条件说成所有人工命令共有的拒绝条件。

observed_failure 优先从工具调用/回执、失败 workflow_intent 或当前 Turn 的 execution-failure 事件恢复完整 Error，保留 source_ref。工具或 Turn 已成功而输出被拒，归 Business/Applied；缺可信分类保持 Unknown。Turn 已消费输入时，即使某边界称未应用也不能据此把当前工作判成安全重发，效果转 Unknown。

retry_supplemental_input 沿 retry_of 查原 conversation.send 的冻结任务与补充文本，核对同实例/node/round、拒绝环和超过 32 次的链。failure_message 仅负责兼容错误展示，不从中文消息推断类别或 retryable。

代码定位：[`retry_count`](../../../crates/core/src/workflow_failure.rs#L4)、[`observed_failure`](../../../crates/core/src/workflow_failure.rs#L36)、[`retry_supplemental_input`](../../../crates/core/src/workflow_failure.rs#L119)、[`failure_message`](../../../crates/core/src/workflow_failure.rs#L160)。

关联阅读：[domain/src/workflow_failure.rs](../devdesk-domain/workflows-and-checks.md#file-src-workflow-failure-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/workflow_runtime.rs](workflow-execution.md#file-src-workflow-runtime-rs)、[core/src/conversation_runtime.rs](conversations-and-history.md#file-src-conversation-runtime-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
