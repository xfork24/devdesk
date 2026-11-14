# 会话命令、原生观察、提案与历史

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

会话输入的排队、原生消费、真实终态、停止证明和上下文快照分别记录。Proposal 是讨论材料的提案，WorkflowOutput 是正式步骤的交付。历史查询只组合已有事实，不回填成功，也不重启来源执行者。

## 本页文件

- [src/conversations.rs](#file-src-conversations-rs)：会话打开、排队输入、交互与快照
- [src/conversation_runtime.rs](#file-src-conversation-runtime-rs)：派发原生 Turn、批量观察与真实终态裁定
- [src/proposals.rs](#file-src-proposals-rs)：讨论提案的提取、修订与显式采纳
- [src/history.rs](#file-src-history-rs)：先合并排序、再分页的事实时间线

<a id="file-src-conversations-rs"></a>

## src/conversations.rs：会话打开、排队输入、交互与快照

源码：[crates/core/src/conversations.rs](../../../crates/core/src/conversations.rs)。

`conversation_command` 管理 session.open、conversation.send/cancel、交互回答和清理重试等持久化命令。它检查 owner、Agent 实例、revision、单活跃 Turn、消息幂等与不可变绑定；实际原生调用交给 conversation_runtime。

正式 Workflow 会话只能发送与当前冻结任务逐字节一致的输入，不能经 conversation.send 覆写 task/task_source 或任务指令；补充输入由对应步骤用例构造。普通讨论保存用户输入与冻结代码视图，等待问题/审批以类型化答案回答，已回答事实不撤回。

`capture_conversation` 保存消息、文档、状态与运行绑定的快照，并建立经验索引；后继会话有明确 predecessor，恢复背景不恢复旧执行状态。conversation_query 提供按实例/范围的分页列表与详情，不把跨项目查询当成隐式项目切换。

代码定位：[`conversation_command`](../../../crates/core/src/conversations.rs#L47)、[`capture_conversation`](../../../crates/core/src/conversations.rs#L543)、[`conversation_query`](../../../crates/core/src/conversations.rs#L635)、[`session_owner`](../../../crates/core/src/conversations.rs#L28)。

关联阅读：[core/src/conversation_runtime.rs](conversations-and-history.md#file-src-conversation-runtime-rs)、[core/src/agent_tasks.rs](workflow-compilation.md#file-src-agent-tasks-rs)、[core/src/proposals.rs](conversations-and-history.md#file-src-proposals-rs)、[adapters/tests/conversations.rs](../devdesk-adapters/testing.md#file-tests-conversations-rs)。

<a id="file-src-conversation-runtime-rs"></a>

## src/conversation_runtime.rs：派发原生 Turn、批量观察与真实终态裁定

源码：[crates/core/src/conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs)。

`tick_conversations` 在并发额度内处理 queued 输入和活动会话，持久化 dispatch 意图后在事务外打开/恢复 worker、发 Turn 并轮询。原生接纳、输入消费和终态分别投影为事件与执行事实，不能把模型普通文本中的“成功”当协议终态。

并发额度限制新 queued Turn，已有活动 Turn 仍会被观察。可写 Turn 另按实际 code_view 的 source.worktree 排队：同一 worktree 有未撤销 writer 时不再启动第二个写者；只读 Turn 不进入该写者集合。这个工作副本边界与项目配额、Rust 根额度和 foreach 调用槽位分别判断。

project_conversation 将端口观察转成领域消息、工具调用、交互及执行事实，commit_conversation_batch 以原身份接纳批量事件。正常 Terminal 路径先提交此前观察，再核对本 Turn、quiescent 和 consumed，然后调用 finish_conversation；取消或派发前失败则不要求输入已经消费。finish_conversation 对原 operation 调用 stop_session，核对 writer_revoked，封存可取得的协议日志，并更新 Turn、SessionLifecycle、RuntimeExecution 和停止事件。只有符合条件的成功会话才自动捕获经验快照；清理失败单独记为 CleanupFailed，不抹掉已接纳的 Turn 结果。

派发阶段明确 Technical/NotApplied 的错误会在取得停止证明后结束为 Failed，其余派发错误进入 unknown_conversation。轮询阶段对 provider_timeout、provider_rejected 或明确 Technical/NotApplied 先停止并将 Turn 记为 Failed，其他错误进入 NeedsReconciliation；record_conversation_failure 保留完整 Error 的不可变状态事件。

Turn 的失败终态与步骤的效果判断是两层：工作流随后从该事件恢复分类，Technical/Unknown 的超时仍使 Step 进入 needs_reconciliation；已消费输入的 Turn 失败也不会因此获得 NotApplied 结论。恢复只停止/封存原 worker，保留未知结果，不重新提交输入；没有可信分类的错误不会被默认当成安全重试。

代码定位：[`tick_conversations`](../../../crates/core/src/conversation_runtime.rs#L74)、[`unknown_conversation`](../../../crates/core/src/conversation_runtime.rs#L461)、[`commit_conversation_batch`](../../../crates/core/src/conversation_runtime.rs#L488)、[`project_conversation`](../../../crates/core/src/conversation_runtime.rs#L505)、[`finish_conversation`](../../../crates/core/src/conversation_runtime.rs#L609)。

关联阅读：[core/src/ports.rs](engine-and-ports.md#file-src-ports-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)、[core/src/platform_tools.rs](manager-and-platform-tools.md#file-src-platform-tools-rs)、[domain/src/conversation.rs](../devdesk-domain/agents-and-conversations.md#file-src-conversation-rs)、[core/src/workflow_failure.rs](workflow-execution.md#file-src-workflow-failure-rs)。

<a id="file-src-proposals-rs"></a>

## src/proposals.rs：讨论提案的提取、修订与显式采纳

源码：[crates/core/src/proposals.rs](../../../crates/core/src/proposals.rs)。

`proposal_command` 处理 create/extract/revise/reject/adopt。extract 从已完成 Turn 的实际 Assistant 消息提取通用 ProposalContent，核对 delivery_ref、值、当前可见交付规范和 input_refs，不从未完成流中抢先建立正式材料。

采纳固定目标、预期修订和来源，把交付值封存为该范围的类型化材料。它不隐式创建 Feature/Task，也不绕过 Workflow 步骤输出合同。修订与拒绝保留提案历史；同项目输入和材料 owner 必须一致，不能借讨论提案导入跨项目或伪造平台 provenance。

代码定位：[`proposal_command`](../../../crates/core/src/proposals.rs#L4)。

关联阅读：[domain/src/conversation.rs](../devdesk-domain/agents-and-conversations.md#file-src-conversation-rs)、[core/src/conversations.rs](conversations-and-history.md#file-src-conversations-rs)、[core/src/spec_lint.rs](manager-and-platform-tools.md#file-src-spec-lint-rs)、[adapters/tests/conversations.rs](../devdesk-adapters/testing.md#file-tests-conversations-rs)。

<a id="file-src-history-rs"></a>

## src/history.rs：先合并排序、再分页的事实时间线

源码：[crates/core/src/history.rs](../../../crates/core/src/history.rs)。

不带 include_tool_calls 时，query 按事件 time/id 倒序返回事件。带该标记时，将项目事件与 integration_records 提供的工具/平台调用投影合并，再统一排序和分页；total 是组合后的总量，而非当前页数。

同时间有稳定的事件/调用次序，工具投影沿用敏感参数遮蔽和来源信息；target 过滤在分页前进行。Global 与项目读哪个状态由 Host 选择，本函数不越库读取，也不生成补写事件。

代码定位：[`HistoryRecord`](../../../crates/core/src/history.rs#L5)、[`query`](../../../crates/core/src/history.rs#L24)。

关联阅读：[core/src/integrations.rs](integrations-and-repositories.md#file-src-integrations-rs)、[host/src/history_tests.rs](../devdesk-host/testing.md#file-src-history-tests-rs)、[adapters/tests/workflows.rs](../devdesk-adapters/testing.md#file-tests-workflows-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
