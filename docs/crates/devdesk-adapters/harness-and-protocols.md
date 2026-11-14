# Harness、JSONL 与受管进程

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

Harness 将正式 Session/Turn 转成真实原生协议。此处同时处理操作归属、容器工作目录、流式事件、动态工具、停止和日志；协议流中的模型文本只能作为观察材料，不能自行改写权威业务终态。

## 本页文件

- [src/harness.rs](#file-src-harness-rs)：原生 worker、私有会话目录与真实停止证明
- [src/jsonl.rs](#file-src-jsonl-rs)：有界 JSONL 通道、请求关联与事件日志
- [src/process.rs](#file-src-process-rs)：受管进程组、私有 stdin 和输出边界

<a id="file-src-harness-rs"></a>

## src/harness.rs：原生 worker、私有会话目录与真实停止证明

源码：[crates/adapters/src/harness.rs](../../../crates/adapters/src/harness.rs)。

RoutingHarness 按冻结 provider 选择 Codex 或 DSH 实现，RuntimeWorkers 按执行身份持有 worker。SessionPath 记录数据集、会话、操作和容器归属，私有目录保存协议与日志；路径和 Docker 标签共同防止误接管其他会话。

任务代码使用受管 worktree 或固定只读导出，Skill、状态和认证挂载有各自边界。Codex app-server 与 DSH SDK 的协议分别处理初始化、输入派发、消费事件、文本全量/增量、工具调用、审批和用户问题。平台/编排动态桥只在实际驱动支持时开放；当前 Codex 支持这类桥，DSH 不提供相同能力。

观察结果包含原生输入是否已消费、真实终态、等待交互及日志事实。结束 Turn 与撤销写入者要分别核对；stop 定位原会话和容器，保存实际停止证据后才允许 core 释放执行边界。后台清理不能成为提前宣告业务成功的理由。

错误带可信失败分类：供应商超时是技术失败但效果未知；明确拒绝且没有执行的请求可标记 Technical/NotApplied。core 再结合输入消费与其他证据决定能否自动重试，模型文本中的“超时”不产生同等分类。

代码定位：[`RuntimeWorkers`](../../../crates/adapters/src/harness.rs#L147)、[`RoutingHarness`](../../../crates/adapters/src/harness.rs#L1459)、[`SessionPath`](../../../crates/adapters/src/harness.rs#L1406)、[`managed_adapter_identity`](../../../crates/adapters/src/harness.rs#L14)。

关联阅读：[adapters/src/jsonl.rs](harness-and-protocols.md#file-src-jsonl-rs)、[adapters/src/docker.rs](engineering-resources.md#file-src-docker-rs)、[core/src/conversation_runtime.rs](../devdesk-core/conversations-and-history.md#file-src-conversation-runtime-rs)、[core/src/workflow_failure.rs](../devdesk-core/workflow-execution.md#file-src-workflow-failure-rs)。

<a id="file-src-jsonl-rs"></a>

## src/jsonl.rs：有界 JSONL 通道、请求关联与事件日志

源码：[crates/adapters/src/jsonl.rs](../../../crates/adapters/src/jsonl.rs)。

JsonlProcess 管理子进程管道、读取线程、请求 ID 和待回复集合，使同步请求与异步事件共用一条原生连接。Codex 与 DSH 的消息封装不同，初始化和 RPC 版本按实际协议分支处理。

读写都有大小边界，恢复历史使用单独的受控大帧路径。AgentMessageBatch 合并文本时保留原消息身份与非文本事件，不让工具调用、消费或状态帧消失在拼接文字里。日志写入私有 journal，并对敏感片段做跨文本增量的遮蔽。

超时、EOF、非法帧和迟到回复需要保留各自语义。Drop 负责清理归属明确的进程；它不能证明容器内或供应商侧工作已停止，完整停止仍由 Harness/Docker 回执负责。

代码定位：[`JsonlProcess`](../../../crates/adapters/src/jsonl.rs#L212)、[`Journal`](../../../crates/adapters/src/jsonl.rs#L24)、[`AgentMessageBatch`](../../../crates/adapters/src/jsonl.rs#L178)、[`sanitize`](../../../crates/adapters/src/jsonl.rs#L31)。

关联阅读：[adapters/src/process.rs](harness-and-protocols.md#file-src-process-rs)、[adapters/src/harness.rs](harness-and-protocols.md#file-src-harness-rs)、[domain/src/conversation.rs](../devdesk-domain/agents-and-conversations.md#file-src-conversation-rs)。

<a id="file-src-process-rs"></a>

## src/process.rs：受管进程组、私有 stdin 和输出边界

源码：[crates/adapters/src/process.rs](../../../crates/adapters/src/process.rs)。

capture_command 系列创建自己的进程组，支持将敏感输入通过 stdin 传入，并并行排空 stdout/stderr，避免某条管道填满导致死锁。输出上限和超时有明确处理；普通 capture 不随意暴露 stderr，只有需要原始证据的 capture_split 分别返回两路内容。

超时会终止受管进程组并回收子进程，但返回 needs_reconciliation：终止本机 CLI 不等于远端操作完全没生效。调用方必须结合稳定操作身份继续检查 Docker、GitHub 或其他实际对象。

该工具不负责业务错误分类，也不替调用方决定命令是否可重发。新增 CLI 调用应先检查参数、环境、工作目录和密钥传输方式，再选择合适的捕获入口。

代码定位：[`Output`](../../../crates/adapters/src/process.rs#L10)、[`capture`](../../../crates/adapters/src/process.rs#L32)、[`capture_split`](../../../crates/adapters/src/process.rs#L37)、[`capture_command`](../../../crates/adapters/src/process.rs#L42)、[`capture_command_limited`](../../../crates/adapters/src/process.rs#L49)。

关联阅读：[adapters/src/git.rs](engineering-resources.md#file-src-git-rs)、[adapters/src/integrations.rs](external-integrations.md#file-src-integrations-rs)、[adapters/src/credentials_macos.rs](credentials-and-permissions.md#file-src-credentials-macos-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
