# 工作流执行、意图与权威事实

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

执行按原计划、节点与 execution 身份推进；模型完成、工具回执、输出接纳和人工决定分别保存。资源级提交在实际写入时重新核对原版本与授权。

<a id="file-src-configured-runtime-rs"></a>

## 运行合同、输入与执行状态机

[configured_runtime.rs](../../../crates/core/src/configured_runtime.rs) 定义实例及请求合同，保存计划引用、控制版本、owner/root/父调用、输入、步骤与状态；它已不承担全部写入状态机。[formal_inputs.rs](../../../crates/core/src/formal_inputs.rs) 渲染任务，[workflow_material_inputs.rs](../../../crates/core/src/workflow_material_inputs.rs) 解析确切输入来源。

[workflow_dispatch.rs](../../../crates/core/src/workflow_dispatch.rs) 派发四类执行器，Agent 通过正式会话与 Turn，Tool 通过平台/外部工具，Workflow 建立子调用，Control 等待明确处置。[workflow_evaluation.rs](../../../crates/core/src/workflow_evaluation.rs)、[workflow_observation_facts.rs](../../../crates/core/src/workflow_observation_facts.rs) 和 [workflow_outputs.rs](../../../crates/core/src/workflow_outputs.rs) 观察并接纳实际结果。

[workflow_commits.rs](../../../crates/core/src/workflow_commits.rs) 固定资源见证并原子提交，步骤更新保留并发兄弟事实。输出结构、Spec、producer 和人工门禁都通过后才可业务接纳。补充输入、技术 retry 和业务 rework 各有身份；retry 保留原输入和历史，rework 才重新解析受影响输入。

时间合同与等待记账由 [workflow_budget.rs](../../../crates/core/src/workflow_budget.rs) 执行；可选 time_limits v1 不替换现有预算含义，超时不伪造停止证明。

<a id="file-src-workflow-runtime-rs"></a>

## 意图领取与派发结果

[workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) 是 Host 的窄调度接口。[workflow_intents.rs](../../../crates/adapters/src/workflow_intents.rs) 提供有限待办读取；外层领取后在事务外准备 session.open、执行工具或提交明确命令，再记录原派发结果。

意图完成不等于业务步骤完成。原执行的 completed/failed 结果不可被迟到观察改写；后续评估继续核对实际 Turn、工具或子实例。

<a id="file-src-live-runs-rs"></a>

## 停止本进程拥有的执行者

[live_runs.rs](../../../crates/core/src/live_runs.rs) 的 `stop_owned_runtime` 停止并核对本进程 Harness 与外部工具。关闭不撤销所有业务工作，也不把未知效果标为成功。原生 writer 的停止、证据封存和清理分别接纳。

<a id="file-src-workflow-runtime-facts-rs"></a>

## 公开执行事实

[workflow_runtime_facts.rs](../../../crates/core/src/workflow_runtime_facts.rs) 描述可公开的实际执行事实，保留原 Turn/运行关联，排除凭据与私有模型连接设置。显示信息不能反过来充当派发授权。

<a id="file-src-workflow-failure-rs"></a>

## 失败与重试

[workflow_failure.rs](../../../crates/core/src/workflow_failure.rs) 提取有界失败消息；实际失败、停止和副作用结论由 [workflow_reconciliation.rs](../../../crates/core/src/workflow_reconciliation.rs)、[workflow_observation_facts.rs](../../../crates/core/src/workflow_observation_facts.rs) 等原事实用例处理。

[workflow_retries.rs](../../../crates/core/src/workflow_retries.rs) 建立独立技术尝试，[workflow_rework.rs](../../../crates/core/src/workflow_rework.rs) 处理业务返工。未知副作用先核对，不从模型错误文字推断“未执行”，不重发旧外部请求来掩盖丢失回执。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
