# Workflow、完成规则与可定位的内容检查

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

Workflow 是可解释的数据图：步骤声明执行方式、任务、端口、预算和条件，内核检查图并解释这些原语。Spec 的机器检查只证明声明的结构条件；内容评审、真实测试和人类决定仍是不同事实。本页先讲静态合同，再讲执行记录、变更与控制身份。

## 本页文件

- [src/workflow.rs](#file-src-workflow-rs)：流程图、执行记录与完成事实的共同合同
- [src/spec_checks.rs](#file-src-spec-checks-rs)：机器断言与冻结输入位置证明
- [src/agent_task.rs](#file-src-agent-task-rs)：任务正文及大输入协议的技术边界
- [src/workflow_change.rs](#file-src-workflow-change-rs)：类型化变更与保守影响图
- [src/workflow_driver.rs](#file-src-workflow-driver-rs)：有界自动调度的可配置参数
- [src/orchestration.rs](#file-src-orchestration-rs)：控制授权、租约和不可混淆的决定
- [src/workflow_failure.rs](#file-src-workflow-failure-rs)：失败类别、副作用结论与安全重试前提

<a id="file-src-workflow-rs"></a>

## src/workflow.rs：流程图、执行记录与完成事实的共同合同

源码：[crates/domain/src/workflow.rs](../../../crates/domain/src/workflow.rs)。

这是领域层最集中的流程合同。`WorkflowDefinition` 用顶层 stages/steps 表达分组和图；StepExecutor 只有 Agent、Tool、Workflow、Control。Step 另有任务指令、端口、依赖、工作上下文、预算、条件、完成规则和确认策略。输入可以来自常量、明确源端口或封存材料；foreach 显式声明集合输入和并发。

`validate` 检查业务 key、稳定节点身份、依赖 DAG、源端口、角色继承、路径、子流程相关约束及有界规则。静态步骤上限为 1000、子流程深度上限为 8；实际闭包展开还由 core 编译器核对。`CompletionRule.evaluate` 只读取 `WorkflowFacts`：值、来源执行、先后、独立身份、结果和确认，不执行脚本，也不从业务节点名称推导成功。

同一文件定义 StepExecution 的十种状态、补充输入序号、技术重试/返工所需身份和检查点，以及 canonical task 记录。`ContentCheckReport` 要求检查项与当前 Spec 完全对应、批准结论与逐项结论一致；需要位置或机器断言的检查不能只交一句理由。`ConfiguredAgentTask.validate` 核验内外 renderer、正文摘要和来源自洽，历史任务不靠当前版本常量重写。

阅读时先看静态 Step/Port，再看 facts 和完成求值，最后看执行记录；真正推进这些状态的是 core。单元测试使用自由业务名，证明角色继承、缺值不满足相等/覆盖、独立性和先后是通用原语。

StepExecution 的 failure 保存可信失败事实，retry_of 指向本技术尝试的前次执行。sequence 只描述本次执行的输入轮次，独立技术尝试数从历史记录推导，业务返工用 round 区分。字段有 serde default，旧可读记录没有失败证据时不补造安全重试结论。

代码定位：[`WorkflowDefinition`](../../../crates/domain/src/workflow.rs#L14)、[`StepExecutor`](../../../crates/domain/src/workflow.rs#L81)、[`CompletionRule`](../../../crates/domain/src/workflow.rs#L193)、[`ContentCheckReport`](../../../crates/domain/src/workflow.rs#L274)、[`StepExecution`](../../../crates/domain/src/workflow.rs#L795)、[`ConfiguredAgentTask`](../../../crates/domain/src/workflow.rs#L839)。

关联阅读：[core/src/configured_workflows.rs](../devdesk-core/workflow-compilation.md#file-src-configured-workflows-rs)、[core/src/configured_runtime.rs](../devdesk-core/workflow-execution.md#file-src-configured-runtime-rs)、[domain/src/spec_checks.rs](workflows-and-checks.md#file-src-spec-checks-rs)、[adapters/tests/configured_workflows.rs](../devdesk-adapters/testing.md#file-tests-configured-workflows-rs)。

<a id="file-src-spec-checks-rs"></a>

## src/spec_checks.rs：机器断言与冻结输入位置证明

源码：[crates/domain/src/spec_checks.rs](../../../crates/domain/src/spec_checks.rs)。

`MachineAssertion` 提供 text_length、required_sections 和 required_pointers。长度按 Unicode 字符计算；章节只识别代码围栏外的 ATX 标题；JSON Pointer 检查实际值中的字段存在性。规则参数、文本大小和处理工作量都有边界，规范不能调用任意命令。

`EvidenceTarget` 是当前执行已经授权的冻结输入。`EvidenceLocation.verify` 同时校验 source_ref、value_digest 与 locator，行号为 1 起始闭区间，字符为 0 起始、末端排除，JSON Pointer 直接指向冻结值。若提供 quote，它必须等于完整选中范围，不能仅仅是其中一句。`json_schema` 将同一位置合同提供给模型输出结构。

单元测试覆盖围栏、Unicode、指针、摘要篡改、越界和报告与机器观察冲突。该检查证明定位和结构自洽，不自动证明内容在业务上正确。

代码定位：[`MachineAssertion`](../../../crates/domain/src/spec_checks.rs#L9)、[`EvidenceLocator`](../../../crates/domain/src/spec_checks.rs#L109)、[`EvidenceLocation`](../../../crates/domain/src/spec_checks.rs#L116)、[`EvidenceTarget`](../../../crates/domain/src/spec_checks.rs#L125)、[`verify`](../../../crates/domain/src/spec_checks.rs#L141)。

关联阅读：[core/src/spec_lint.rs](../devdesk-core/manager-and-platform-tools.md#file-src-spec-lint-rs)、[domain/src/workflow.rs](workflows-and-checks.md#file-src-workflow-rs)、[core/src/configured_runtime.rs](../devdesk-core/workflow-execution.md#file-src-configured-runtime-rs)。

<a id="file-src-agent-task-rs"></a>

## src/agent_task.rs：任务正文及大输入协议的技术边界

源码：[crates/domain/src/agent_task.rs](../../../crates/domain/src/agent_task.rs)。

本文件集中声明 canonical 任务的正文、Workflow、整体 payload 上限，renderer 身份以及大输入读取工具名和分页大小。`task_content_hash` 使用同一配置内容哈希，供任务落盘和恢复核对。

它不负责拼提示词；真正渲染在 core 的 task_input。把上限和工具名集中起来，使生产任务、输出引用与合同校验共享同一含义。超过内联阈值的输入保留原 Artifact，通过 outputs.read 显式续读，不可把摘要当作已交付全部内容。

代码定位：[`AGENT_TASK_RENDERER`](../../../crates/domain/src/agent_task.rs#L13)、[`OUTPUT_READ_PAGE_BYTES`](../../../crates/domain/src/agent_task.rs#L10)、[`task_content_hash`](../../../crates/domain/src/agent_task.rs#L14)。

关联阅读：[core/src/configured_runtime.rs](../devdesk-core/workflow-execution.md#file-src-configured-runtime-rs)、[core/src/agent_tasks.rs](../devdesk-core/workflow-compilation.md#file-src-agent-tasks-rs)、[core/src/orchestration.rs](../devdesk-core/orchestration-and-drivers.md#file-src-orchestration-rs)。

<a id="file-src-workflow-change-rs"></a>

## src/workflow_change.rs：类型化变更与保守影响图

源码：[crates/domain/src/workflow_change.rs](../../../crates/domain/src/workflow_change.rs)。

`WorkflowChangeOperation` 定义 13 类操作：采用 Standard、回退、替换计划、增改/删除步骤、增改阶段、绕过、输入绑定、角色绑定、整体输入、豁免、任务范围和任务依赖修改。ChangeSet 必须绑定基准计划、有理由并含 1–128 项操作。

`assign_node_ids` 从源身份、节点种类和原 key 生成稳定 ID，明确已有 node_id 的节点继续保留身份。`impacted_nodes` 把依赖边与值/规则中的步骤引用一起传播，计算保守失效范围，不能只按执行先后关系寻找后继。它只计算变化，不停止 worker、不改实际实例；预演和应用在 core。

代码定位：[`WorkflowChangeOperation`](../../../crates/domain/src/workflow_change.rs#L7)、[`WorkflowChangeSet`](../../../crates/domain/src/workflow_change.rs#L62)、[`assign_node_ids`](../../../crates/domain/src/workflow_change.rs#L80)、[`impacted_nodes`](../../../crates/domain/src/workflow_change.rs#L97)。

关联阅读：[core/src/workflow_changes.rs](../devdesk-core/workflow-changes.md#file-src-workflow-changes-rs)、[domain/src/orchestration.rs](workflows-and-checks.md#file-src-orchestration-rs)。

<a id="file-src-workflow-driver-rs"></a>

## src/workflow_driver.rs：有界自动调度的可配置参数

源码：[crates/domain/src/workflow_driver.rs](../../../crates/domain/src/workflow_driver.rs)。

`OrchestrationSelection` 明确选择 manual、rust 或 agent；rust 必须提供精确 policy_ref。`OrchestrationPolicy` 只定义执行并发、每 tick 动作数、租约时间以及技术错误重试集合和上限。

校验限制并发 1–32、动作 1–64、租约 5–3600 秒、最多 8 次策略重试和 32 个错误码。策略定义本身不产生动作，也不证明所有失败都已有结构化错误来源；实际选择、预算解释和持久化围栏由 core 的驱动实现。

代码定位：[`OrchestrationPolicy`](../../../crates/domain/src/workflow_driver.rs#L6)、[`OrchestrationSelection`](../../../crates/domain/src/workflow_driver.rs#L29)、[`validate`](../../../crates/domain/src/workflow_driver.rs#L16)。

关联阅读：[core/src/workflow_driver.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-driver-rs)、[core/src/orchestration.rs](../devdesk-core/orchestration-and-drivers.md#file-src-orchestration-rs)。

<a id="file-src-orchestration-rs"></a>

## src/orchestration.rs：控制授权、租约和不可混淆的决定

源码：[crates/domain/src/orchestration.rs](../../../crates/domain/src/orchestration.rs)。

`WorkflowControlPolicy` 给出允许操作、必须留给人类的操作、允许豁免的要求类别和唤醒预算。`WorkflowController` 固定 Agent、Role、Permission Context、会话、根实例、epoch 与消费游标；workspace_only 表示项目经理独立工作台，并不自动获得某个根的调度权。

`WorkflowActor` 区分 Human、带 Session/Turn/epoch 的 Controller、Driver 和 Segment。决定、封存材料和 ActionPermit 分别记录业务处置、材料来源与候选/目标要求；把它们分开才能保留原拒绝，不伪造人类签名或测试事实。Wakeup/Lease 表达持久投递和单写者围栏；它们需要 core 和实际存储实现生命周期。

`workflow_node_id` 为发布与后续修改提供可重复节点身份。这个包只描述和验证控制数据；原生调用真伪、停止证明和当前 epoch 仍要在用例中核验。

代码定位：[`WorkflowControlPolicy`](../../../crates/domain/src/orchestration.rs#L10)、[`WorkflowController`](../../../crates/domain/src/orchestration.rs#L88)、[`WorkflowActor`](../../../crates/domain/src/orchestration.rs#L136)、[`WorkflowDecision`](../../../crates/domain/src/orchestration.rs#L155)、[`WorkflowActionPermit`](../../../crates/domain/src/orchestration.rs#L181)。

关联阅读：[core/src/controllers.rs](../devdesk-core/orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/orchestration.rs](../devdesk-core/orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/manager_workspace.rs](../devdesk-core/manager-and-platform-tools.md#file-src-manager-workspace-rs)、[core/src/workflow_driver.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-driver-rs)。

<a id="file-src-workflow-failure-rs"></a>

## src/workflow_failure.rs：失败类别、副作用结论与安全重试前提

源码：[crates/domain/src/workflow_failure.rs](../../../crates/domain/src/workflow_failure.rs)。

WorkflowFailure 保存 code/message、可信 source_ref、ExecutionFailureKind 和 ExecutionFailureEffect。kind 区分 Technical、Business、Cancelled、Unknown；effect 区分 NotApplied、Applied、Unknown。两者正交：技术超时不说明副作用没有发生，结构不合格也可能是在模型已真实执行后才发现。

from_error 只使用边界附带的 disposition，否则以调用者提供的 fallback 类别和 Unknown 效果保守记录。automatic_retry_safe 只对 Technical + NotApplied 返回真；策略错误码、次数预算、停止证明和权限还需 core 另行核对。模型建议“再试一次”不构成这份事实。

代码定位：[`WorkflowFailure`](../../../crates/domain/src/workflow_failure.rs#L26)、[`ExecutionFailureDisposition`](../../../crates/domain/src/workflow_failure.rs#L20)、[`automatic_retry_safe`](../../../crates/domain/src/workflow_failure.rs#L49)。

关联阅读：[core/src/workflow_failure.rs](../devdesk-core/workflow-execution.md#file-src-workflow-failure-rs)、[core/src/workflow_driver.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-driver-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
