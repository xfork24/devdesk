# 工作流启动、派发与结果接纳

这些模块把 WorkflowDefinition 变为持久化的执行过程。驱动提出动作，core 校验动作并提交状态，Host 执行后续的后台操作。

## 启动和派发

[workflow_start.rs](../../../crates/core/src/workflow_start.rs) 处理流程建立及原计划；[workflow_resources.rs](../../../crates/core/src/workflow_resources.rs) 读取本次操作所需对象；[workflow_commits.rs](../../../crates/core/src/workflow_commits.rs) 用 WorkflowCommit 组织版本检查和修改。

[workflow_dispatch.rs](../../../crates/core/src/workflow_dispatch.rs) 的 `dispatch_step` 是阅读派发逻辑的重点。它检查当前步骤、依赖、权限和额度，准备输入并创建执行。Agent 步骤通过 `runtime_intent` 保存 session.open，工具步骤保存 workflow.tool.execute；子流程则走对应子流程逻辑。

| 关注点 | 对应模块 |
|---|---|
| 谁可操作、原控制权是否仍有效 | [workflow_authorization.rs](../../../crates/core/src/workflow_authorization.rs) |
| 依赖与当前动作是否就绪 | [workflow_readiness.rs](../../../crates/core/src/workflow_readiness.rs)、[workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs) |
| 时间和尝试是否超预算 | [workflow_budget.rs](../../../crates/core/src/workflow_budget.rs) |
| 输入材料及来源 | [workflow_material_inputs.rs](../../../crates/core/src/workflow_material_inputs.rs)、[workflow_materials.rs](../../../crates/core/src/workflow_materials.rs) |
| 子流程与分段派发 | [workflow_children.rs](../../../crates/core/src/workflow_children.rs)、[workflow_segments.rs](../../../crates/core/src/workflow_segments.rs) |

## 后台动作如何回来

[workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) 的 `pending_workflow_intents` 分页取得待处理操作，`workflow_intent_drivable` 核对它仍属于当前数据集和执行条件，`observe_workflow_intent` 保存观察结果。Host 调度这些阶段，core 不创建自己的 Docker 管理入口。

输入提交后发生版本冲突时，原命令不能被偷偷替换成“读取新版本后再试”。外部动作可能已经发生，核对和业务重试需要明确区分。

## 输出如何被接受

[workflow_outputs.rs](../../../crates/core/src/workflow_outputs.rs) 的 `prepare_native_outputs` 接收实际执行结果和 WorkflowOutputProof。Proof 区分 Agent、工具和子流程，防止把任意 JSON 当作另一种来源的证据。

函数检查 summary、status、输出端口、大小、Delivery、内容检查及生产来源，然后准备 ArtifactSeal 和 workflow_output。`needs_input` 与 `blocked` 分别进入等待和失败处理；合格 completed 输出进入等待接纳或人工确认，而不是直接宣布整个流程完成。

[workflow_evaluation.rs](../../../crates/core/src/workflow_evaluation.rs)、[workflow_facts.rs](../../../crates/core/src/workflow_facts.rs)、[workflow_rules.rs](../../../crates/core/src/workflow_rules.rs) 整理事实并调用 domain 完成规则。人类确认和步骤接纳通过控制入口检查，不由输出文本决定。

## 验证重点

修改派发或接纳时，覆盖重复请求、过期版本、错误来源、缺失端口、等待输入和迟到结果。相关测试：[workflows.rs](../../../crates/adapters/tests/workflows.rs)、[workflow_resource_concurrency.rs](../../../crates/adapters/tests/workflow_resource_concurrency.rs)、[delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs)。
