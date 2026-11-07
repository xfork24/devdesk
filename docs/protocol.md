# 本地协议 v1

只监听 127.0.0.1。host 启动时生成随机本地会话；同源页面通过 HttpOnly、SameSite=Strict Cookie 使用。校验 Host 和有值的 Origin，缺失或错误凭据拒绝 RPC/SSE/Artifact。用户 human:local 身份由 host 装配，载荷中的 actor 无效。

## HTTP

- POST /rpc：JSON-RPC 2.0，id 必填。查询 params 为过滤参数；命令 params 为 Command。
- GET /events?cursor=N：SSE，事件 ID 为持久化单调 cursor；支持 Last-Event-ID。分批补读，断线后重连；事件只是事实通知，页面重新查询。
- GET /artifacts/{id}：按数据库 ID 读取，路径不得由客户端提供；SHA256 校验后返回纯文本并禁止内容嗅探。
- GET /capabilities：协议、版本、simulation/live unavailable/Archive preview/migration preview/platform validation 状态。
- POST /setup：未打开数据集时设置 path；非空非 DevDesk 目录拒绝。打开后不能热切换。
- POST /shutdown：可信人类会话退出 host。

```json
{"jsonrpc":"2.0","id":1,"method":"attempt.start","params":{"command_id":"uuid","target":"feature-id","expected_revision":3,"payload":{"phase":"planning","scenario":"success"}}}
```

命令：project.create/update、feature.create、document.revise、definition.revise、binding.update、task.create、attempt.start/pause/cancel、run.cancel/retry/continue、delivery.submit、review.submit/request、override.accept、merge.request/apply、context.update、snapshot.save。不存在 set_status。

target 为有 revision 对象时 expected_revision 必填；command_id 相同参数返回首次结果（先于修订检查），不同参数复用拒绝。长操作返回 operation_id。错误不写入幂等结果，可修正后重试。

查询：query.workspace（project_id/feature_id/offset/limit）、query.history（target/offset/limit）、query.events（cursor/limit）、query.artifact（id）。limit 默认 50，上限 100；Feature detail 返回有界历史。allowed_actions、blocked_reasons 来自 Rust，界面不实现状态机。

错误 result 包含 code/message：invalid_params、not_found、blocked、revision_conflict、method_not_found、storage、dataset_locked。JSON-RPC error.code 使用 -32600/-32601/-32602/-32000，domain code 在 data 中。

SSE 和轮询使用同一个数据库事实源。所有命令在 SQLite 短事务内校验和提交；单写协调，状态与事件提交后才可见。模拟执行按受控同步点产生确定输出，不把适配器原始输出直接当业务终态。


## 分页与冻结执行输入

query.workspace 的 project_offset 与 Feature offset 独立；detail_pages 对 tasks/attempts/runs/deliveries/merges/documents/snapshots/artifacts 指定页码（从0起）。每页默认50，Runs每页为5倍；detail.totals 为各集合数量。翻 Attempts/Merges 页时客户端将 runs 页码归零。事件与日志报告文件分段读取，不加载全部工程历史到浏览器。

Standard 的 slots 结构为 `[{"purpose":"planning","spec":"固定Spec-ID"}, ...]`，同时具备 planning/implementation/merge 三个用途。Spec 仅解释内置 traceability、failure_paths、sections 规则；无任意脚本。定义版本、适用标准与实际逐项结果都留在 Review 历史。

Attempt 除文档与绑定外冻结 Context 内容与 revision。Harness 的执行输入包含冻结 Context 和继承 Snapshot；Reviewer 输入为目标 Delivery 的冻结文档、证据和报告正文。TestRequest 包含 phase、candidate、test_definitions、environment、origin，适配器结果必须回传匹配的绑定。

文件清单含 schema_version=1；Delivery 清单和 Snapshot JSON 均封存并记录哈希，不成为可写工程事实源。JSON-RPC 不实现 batch/notification，查询与命令均要求 request id。

实现参考：[Axum SSE](https://docs.rs/axum/0.8.9/axum/response/sse/)、[Cargo Workspaces](https://doc.rust-lang.org/stable/cargo/reference/workspaces.html)。
