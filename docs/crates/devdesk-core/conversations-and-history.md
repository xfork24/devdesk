# 会话命令、原生观察与只读历史

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

会话的创建、排队、原生执行、业务投影与历史读取分别处理。命令固定原始身份和版本，外部端口返回实际观察，查询只读取已提交事实。

<a id="file-src-conversations-rs"></a>

## 会话查询与命令入口

[conversations.rs](../../../crates/core/src/conversations.rs) 只分派有界会话查询；[session_commands.rs](../../../crates/core/src/session_commands.rs) 准备会话变更，[conversation_start.rs](../../../crates/core/src/conversation_start.rs) 处理讨论命令，[conversation_controls.rs](../../../crates/core/src/conversation_controls.rs) 处理实际控制。

创建会话冻结完整 RuntimeBinding。输入由 [conversation_submission.rs](../../../crates/core/src/conversation_submission.rs) 排队，实际准入由 [conversation_admission.rs](../../../crates/core/src/conversation_admission.rs) 校验。正式步骤和控制器会话另须通过原执行/计划/epoch 的校验，不能从普通讨论获得工作流授权。

<a id="file-src-conversation-runtime-rs"></a>

## 原生执行与回执接纳

[conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs) 协调有界运行切片，保存原意图后在事务外调用 Harness。启动、发送、停止与清理分别需要持久化执行资格，重放不重新调用外部副作用。

[conversation_workers.rs](../../../crates/core/src/conversation_workers.rs)、[conversation_observations.rs](../../../crates/core/src/conversation_observations.rs) 和 [worker_reconciliation.rs](../../../crates/core/src/worker_reconciliation.rs) 核对真实 worker、Session、Turn、消费及停止证明。未知结果保持待核对；取消不等于已停止。完成快照由 [conversation_captures.rs](../../../crates/core/src/conversation_captures.rs) 独立派生，保留原始观察。

<a id="file-src-proposals-rs"></a>

## 讨论提案

[proposal_commands.rs](../../../crates/core/src/proposal_commands.rs) 处理创建、提取、修订、拒绝和采用。提案固定已完成讨论、确切目标及版本；仅可信 Human 来源可编辑和采用。读取原提案与后继断言后，用有限资源变更和幂等身份提交。提案不替代工作流输出、真实测试或平台回执。

<a id="file-src-history-rs"></a>

## 按范围与页面读取历史

[ports.rs](../../../crates/core/src/ports.rs) 定义读取合同，[adapters/operational_reads.rs](../../../crates/adapters/src/operational_reads.rs) 与 [conversation_queries.rs](../../../crates/adapters/src/conversation_queries.rs) 执行查询。列表先过滤和分页，正文按确切 ID 读取；普通查询不恢复整个工程、不启动 worker、不回写业务事件。

Host 使用显式项目或全局讨论作用域。Artifact 正文继续通过所属范围和内容摘要校验；跨项目经验访问使用独立来源读取端口，不改变当前标签页的项目。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
