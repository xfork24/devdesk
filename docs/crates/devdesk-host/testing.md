# Host 测试与跨库隔离

本机联合性能使用 `active_frontend_performance.rs`、`performance_history.rs` 和 `performance_repository.rs` 测试入口。D1/D2/D3 历史为类型化夹具，讨论 Turn 在派发前取消，不伪造模型或 Docker 完成。W1 使用测试端口及生产调度循环；W2 准备合法 Git 工作副本后启动正式 release Host，实际运行 Git/Docker/Artifact。

`tests/performance/joint_frontends.py` 支持 0/1/2/10/25/50/100 客户端、开放到达率、独立写入及读回、SSE、历史增长、真实浏览器和 W2。`--renew` 只为确定性 Rust 夹具接续合法工作流。浏览器通过 `joint_browser.mjs` 的真实控件完成动作，保留排队、失败、RPC 及发生器资源；默认发生器进程树 RSS 总和预算 12 GiB，超预算整轮失败。

`tests/performance/workflow_runtime.py --frontends 0|100` 使用真实 Codex 和新数据集。100 客户端的修改目标是独立 Draft Feature 的背景，负载工具不向经理发送调度指令。`native_protocol` 记录实际收包与写回；`DEVDESK_PERF_LOG=1` 开启按 call_id 的 manager action/result/delivery 及 platform prepare/external/admission/delivery 分段与同 Host 时间范围。嵌套 span 不能相加，记录创建时间不能冒充精确 COMMIT。

构建身份、命令、结果及尚未通过的容量门禁见[联合验收记录](../../plans/integrated-workflow-performance-acceptance/EXECUTION.md)。同机成绩不代替独立负载机容量认证。

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

`cargo test -p devdesk-host --locked` 运行连接参数、项目路由、治理、权限和模板等模块内测试。它们使用独立临时数据集并可注入执行端口，不启动付费模型。

连接 origin/token 校验见 [HTTP 入口](http-and-security.md#file-src-main-rs)；工程切换、inactive 调度与停机见 [Workspace](workspace-and-lifecycle.md#file-src-workspace-rs)；源修订与实例历史见 [治理路由](rpc-and-configuration.md#file-src-governance-rs)；权限后继会话与单目录恢复见 [权限 API](rpc-and-configuration.md#file-src-permission-resources-rs)；模板代次和回滚见 [模板路由](rpc-and-configuration.md#file-src-templates-rs)。本页独立文件专门检验组合历史的数据库归属。

真实 HTTP 进程、数据集入口和浏览器交互另由 [跨进程入口测试](../../../tests/entrypoints) 与 [浏览器测试](../../../web/tests) 覆盖；模块测试通过不能替代这两层，也不表示外部 Docker/GitHub 已验收。

## 本页文件

- [src/history_tests.rs](#file-src-history-tests-rs)：全局讨论与多个工程的组合历史隔离

<a id="file-src-history-tests-rs"></a>

## src/history_tests.rs：全局治理与多个工程的组合历史隔离

源码：[crates/host/src/history_tests.rs](../../../crates/host/src/history_tests.rs)。

此文件仅在 cfg(test) 下挂载。add_history 在指定 Engine 直接写入一条 tool_call 治理记录和一条 Event，主场景为全局配置 Engine、项目 A 和项目 B 分别写入事实，再从 Workspace 路由组合历史查询。它未建立全局讨论 Session 或 Snapshot，不能用来证明全局讨论库的恢复。

断言 scope=global 读取全局配置历史、项目查询读取指定工程；创建项目 B 后继续显式读取 A，不改变 A 的范围。它防止为实现统一历史而混合数据库；另有普通分页/确切调用读取的零完整恢复断言。

代码定位：[`add_history`](../../../crates/host/src/history_tests.rs#L3)、[`combined_history_keeps_global_and_each_project_in_their_own_database`](../../../crates/host/src/history_tests.rs#L25)。

关联阅读：[运行历史读取端口](../devdesk-core/conversations-and-history.md#file-src-history-rs)、[host/src/workspace.rs](workspace-and-lifecycle.md#file-src-workspace-rs)、[adapters/tests/conversations.rs](../devdesk-adapters/testing.md#file-tests-conversations-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

本机等待态 CPU 可用 `python3 tests/performance/waiting_cpu.py --binary /绝对路径/devdesk-host --output /不存在的专用目录 --seconds 60`。它通过公开命令建立单个 Rust 驱动 Human 门禁，保持等待且不签署，用进程累计 CPU 时间差计算单核占用；准备期排除在区间外，不调用模型或 GitHub。联合负载的 SSE 原始时间样本保存在 events.jsonl.gz，内存只保存毫秒上取整的有界直方图，原始值可离线重算。`--sse-reconnect` 需要至少 120 秒协议负载，每个订阅者断开/带原游标重连/读取最终状态十轮；与慢订阅者及浏览器显示收敛分别报告。
