# 本地协议 v1

只监听 127.0.0.1。host 启动时生成随机本地会话；同源页面通过 HttpOnly、SameSite=Strict Cookie 使用。校验 Host 和有值的 Origin，缺失或错误凭据拒绝 RPC/SSE/Artifact。用户 human:local 身份由 host 装配，载荷中的 actor 无效。

## HTTP

- POST /rpc：JSON-RPC 2.0，id 必填。查询 params 为过滤参数；命令 params 为 Command。
- GET /events?project_id=ID&cursor=N：SSE，连接固定到该活动项目；事件 ID 为 `ID:cursor`，cursor 在项目内持久化单调递增。支持同项目 Last-Event-ID；其他项目的游标忽略。分批补读，断线后重连；事件只是事实通知，页面重新查询。
- GET /artifacts/{id}?project_id=ID：按活动项目数据库中的 ID 读取，路径不得由客户端提供；SHA256 校验后返回纯文本并禁止内容嗅探。
- GET /capabilities：协议、版本、simulation/live unavailable/Archive preview/migration preview/platform validation 状态。
- POST /setup：未打开数据集时设置 path；非空非 DevDesk 目录拒绝。打开后不能热切换。
- POST /shutdown：可信人类会话退出 host。

```json
{"jsonrpc":"2.0","id":1,"method":"attempt.start","params":{"command_id":"uuid","target":"feature-id","expected_revision":3,"payload":{"phase":"planning","scenario":"success"}}}
```

命令：project.create/open/update、feature.create、document.revise、definition.revise、binding.update、task.create、attempt.start/pause/cancel、run.cancel/retry/continue、delivery.submit、review.submit/request、override.accept、merge.request/apply、context.update、snapshot.save。不存在 set_status。

`project.open` 使用 Command，target 为项目 ID，payload 可为空；成功后切换活动数据库并保存上次选择。`project.create` 创建独立数据库并自动打开。工程查询和 Command 可在 params 顶层携带 `project_id`，与当前活动项目不符时返回 `project_changed`；不通过查询隐式切换项目。省略作用域时使用请求开始时的活动数据库。没有活动项目时工程命令返回 `project_required`，仍可查询全局工作区和配置、修改并发配置或修订共享定义。

target 为有 revision 对象时 expected_revision 必填；command_id 相同参数返回首次结果（先于修订检查），不同参数复用拒绝。长操作返回 operation_id。错误不写入幂等结果，可修正后重试。

查询：query.workspace（project_id/feature_id/offset/limit）、query.history（project_id/target/offset/limit）、query.events（project_id/cursor/limit）、query.artifact（project_id/id）。limit 默认 50，上限 100；Feature detail 返回有界历史。allowed_actions、blocked_reasons 来自 Rust，界面不实现状态机。

query.workspace 返回全局项目目录，以及仅属于活动项目的 features/detail；query.history 只读取活动项目的历史。工作区附 `active_project_id`、`active_project`、`global_database`、`project_database` 和 `project_data_root`；`data_root` 仍为全局 data 根目录。`settings.update({global_concurrency:N})` 写入全局 SQLite（1–32），`definition.revise` 也作用于共享配置库。工程命令幂等记录按项目隔离；项目创建、打开及共享配置命令的幂等范围为全局配置库。

错误 result 包含 code/message：invalid_params、not_found、blocked、revision_conflict、project_changed、project_required、method_not_found、storage、dataset_locked。JSON-RPC error.code 使用 -32600/-32601/-32602/-32000，domain code 在 data 中。

SSE 和轮询使用同一个数据库事实源。所有命令在 SQLite 短事务内校验和提交；单写协调，状态与事件提交后才可见。模拟执行按受控同步点产生确定输出，不把适配器原始输出直接当业务终态。


## 分页与冻结执行输入

query.workspace 的 project_offset 与 Feature offset 独立；detail_pages 对 tasks/attempts/runs/deliveries/merges/documents/snapshots/artifacts/effects 指定页码（从0起）。每页默认50，Runs每页为5倍；detail.totals 为各集合数量。翻 Attempts/Merges 页时客户端将 runs 页码归零。事件与日志报告文件分段读取，不加载全部工程历史到浏览器。

Standard 的 slots 结构为 `[{"purpose":"planning","spec":"固定Spec-ID"}, ...]`，同时具备 planning/implementation/merge 三个用途。Spec 仅解释内置 traceability、failure_paths、sections 规则；无任意脚本。定义版本、适用标准与实际逐项结果都留在 Review 历史。

Attempt 除文档与绑定外冻结 Context 内容与 revision。Harness 的执行输入包含冻结 Context 和继承 Snapshot；Reviewer 输入为目标 Delivery 的冻结文档、证据和报告正文。TestRequest 包含 phase、candidate、test_definitions、environment、origin，适配器结果必须回传匹配的绑定。

文件清单含 schema_version=1；Delivery 清单和 Snapshot JSON 均封存并记录哈希，不成为可写工程事实源。JSON-RPC 不实现 batch/notification，查询与命令均要求 request id。

实现参考：[Axum SSE](https://docs.rs/axum/0.8.9/axum/response/sse/)、[Cargo Workspaces](https://doc.rust-lang.org/stable/cargo/reference/workspaces.html)。

## 领域补强协议

- document.revise 只接受 Feature target，必须携带该 Feature 的 expected_revision；payload.feature_id 不能作为替代入口。
- Requirement 可附 acceptance=[{key,description}]；Test 可附 verifies=[key]。Web 使用每行“标识 | 条件”和逗号列表录入。省略时分别登记完整正文为 AC-1、继承关联需求的全部条目。
- task.create 可附 scope={requirement,design,acceptance} 或 acceptance_keys。默认从已接受的初始 Snapshot 解析。task.replan(target=Task, expected_revision) 采用当前已接受规划及选定条目，新测试修订保留 supersedes。
- query.runtimes 返回 Harness/Sandbox 定义；query.archive_preview(feature_id) 返回产物/快照范围与不可执行理由。当前注册表仅内置模拟定义，真实定义的注册与部署在真实接入轮提供。
- Run 返回不可变 binding；Session 返回 instance；Review 返回 assignment；Snapshot 返回 capture；Attempt 返回 baseline。
- 数据库支持从 v1 顺序迁移至 v13。旧文档缺少结构化验收映射时保持空值，旧交付历史可读；重新参与增强门禁须明确修订和验收，不能将迁移当成人类复核。

历史 Run 的 binding_verified=false，保留可从 Session 确定的身份，但不把无法证明的完整绑定用于技术重试；需要新建执行。新 Run 的 binding_verified=true。

## 持久化外部操作

- query.workspace.detail.effects 分页返回外部操作；detail.advances 返回当前 Feature 的有界目标推进记录。操作包含适配器身份、请求、回执、状态、revision 与理由；Web 提供查看、核对和清理重试入口。
- effect.reconcile(target=Effect ID, expected_revision) 登记核对请求并返回 operation_id；调度器只查原供应商事实，不把 interrupted Run 改成成功。未知结果保留资源。
- effect.retry_cleanup(target=失败的 Cleanup ID, expected_revision) 创建新清理操作并引用 retry_of；保留原失败。同一 command_id 返回原结果；已释放资源不能再次新建重试。
- merge.apply(target=Merge, expected_revision) 返回 {id,operation_id,accepted:true}，表示意图已保存；是否完成以查询的 Merge/owner 状态为准。供应商调用由调度器负责。
- merge.reconcile(target=Merge, expected_revision) 请求核对 needs_reconciliation 的推进。确认未推进后，旧候选 stale，重新 merge.request、验证与 Review。
- Attempt 另返回 outputs 和 scope；Harness 输入明确区分 frozen_inputs / stage_outputs，附锁定定义和运行实例。持久化回执不是批准命令；核心仍核对版本、身份、来源及门禁。
- 新表 effects、advances 与业务事件使用同一 SQLite 事实源。部分副作用的意图、调用、回执和投影分开提交；不能把这个协议解释为跨供应商原子事务。

GitContext 随 Attempt.git_context 和 MergeAdvance.context 返回；它包含冻结的 Workspace 与源/目标 owner、branch、worktree。Harness 输入的 code、code_baseline 和 TestRequest 的 definitions/code/runtime 均来自锁定版本，避免适配器查找隐含的全局目录。旧定位为 null，需要新建执行。
