# 与 single-host-concurrency 的实施边界

状态：本分支实施约束。2026-11-16。按用户要求分离 scope；本表规定本分支的边界，不把整合基础代码视为已经完成分工或联合验收。

## 已核对的对方工作

`codex/single-host-concurrency` 位于 `/Users/hengliu/.codex/worktrees/single-host-concurrency/devdesk`。最新只读核对到 `7dfa7ef`，已提交执行页有界读取；其工作树继续修改 scheduling_read、事件尾读/广播和公共调度提示，SQL 目标为 global 21 / project 52。安装登记/事件基础与 P4 后续进展由该分支记录；P2 全入口迁移、P5 公共调度和 P6 容量不能据此视为完成。不能把它理解成只做“100 个浏览器的前端优化”。

本分支位于 `/Users/hengliu/.codex/worktrees/workflow-runtime-performance/devdesk`，分支 `codex/workflow-runtime-performance`；从 main `026c05d` 建立，`d3ae7d4` 整合对方已提交至 `35bcaee` 的基础；`38ec1ff` 整合 main 至 `b75439d`，保留配置 W0–W7、运行诊断和执行页修复。对方后续代码与未提交工作不复制、不改动；暂停数据仍由原 Host 管理，本分支保持 global 19 / project 49 专用测试基线。

## 责任边界

| 领域 | 归属 | 本分支允许的工作 |
|---|---|---|
| SQLite 连接池、资源版本、有限提交、SQL 基线、通用 CAS 迁移机制 | single-host-concurrency | 使用其合同；不新增另一套连接池/CAS，不撤销或改写旧 writer 保护。 |
| 全局并发资源合同、项目注册/路由、跨项公平调度、SSE 广播 | single-host-concurrency | 本分支不重写这些基础能力；仅修正 workflow 调用层重复推进和额度重复扣减。 |
| Workflow 执行读取、空轮询、步骤复制/回滚、执行阶段去重、回执排队 | workflow-runtime-performance | 修改运行专用查询、`configured_runtime`、`conversation_runtime`、`Engine::tick` 相关调用及 Host 的 workflow 阶段；复用公共池和提交合同，不另建权威状态。 |
| 执行页摘要/分页、前端项目上下文、QueryCache 与局部刷新 | single-host-concurrency | 保留兼容性回归；不在此分支同步实现另一套 UI 或查询接口。 |
| 经理原生调用执行到回复、回复失败恢复、精确 worker 绑定 | workflow-runtime-performance | 修改 `core/controllers.rs` 的驱动/交付及对应 fixture；保持持久化先于回复，失败交付不重新执行业务动作。 |
| 已预约原生 execution 的 Docker/SDK 启动 | workflow-runtime-performance | 复用 Dispatching/Prepared、原操作身份和配额；适配器线程只启动并返回实际回执，核心接纳后才发送输入。不建立第二套公共任务队列、调度优先级或业务状态。 |
| Agent 原生平台回执、结果负载 | workflow-runtime-performance | 修改 `core/platform_tools.rs` 内的回复编码及计时；不改写一般存储/HTTP 读取接口。 |
| 冻结上下文与权限恢复的重复编译 | workflow-runtime-performance | 修改 `adapters/source_views.rs`；保留确切源/完整作用域及每快照重验，缓存不代替派发授权。 |
| Docker create/inspect 错误分类、输入是否送达及停止证明 | workflow-runtime-performance | 修改 `adapters/docker.rs` 及权限/原生执行回归；不增加安装/凭据资源提交旁路。 |
| 经理失败/取消投影、超时原因分段 | workflow-runtime-performance | 不将取消留作 active；区分平台等待与其他墙钟区间。冻结预算仍按原规则；新预算策略另行配置修订。 |
| Host 启动器生命周期日志与信号转发验证 | workflow-runtime-performance | 修改 `scripts/dev/run-host.py` 及独立子进程测试；不改 Host HTTP 生命周期 CAS。 |
| 100 前端完整容量 | single-host-concurrency 主导联合验收 | 本分支提供 native call 分段指标和执行/故障用例；不重复建设负载器，不以本分支定向结果宣布 100 前端通过。 |

## 共享文件与接入方式

- 本分支不修改 `connection_pool.rs`、`resource_commits.rs`、`core/concurrency.rs`、`sql/`、Host 项目路由、SSE 和 Web 产品代码。基础整合产生的合并内容单列，不能计成本分支功能。
- `core/ports.rs` 只增加 workflow 运行提示/计数的薄端口；实现位于新增 `adapters/runtime_queries.rs` 并通过已有连接池读取。`adapters/lib.rs` 只有委托与冻结恢复清理接入，不修改 Store 结构、连接池、事务或 CAS 语义。
- Harness 增加 begin/poll 启动端口；默认实现保留同步测试适配器，真实 Docker 路由实现后台启动。`harness_startup.rs` 的缓存是本进程启动结果，不是数据库权威事实；缺失任务不能重开，实际停止且耐久释放后清理。共享的是固定编译资产的期望摘要，动态权限仍逐次校验。
- `Workspace::tick` 调整 workflow 的重复阶段与本项目额度传递，`Engine::tick` 唯一拥有正常轮次评估；不重写项目注册/路由和跨项公平队列。`host/main.rs` 增加阶段观测、跳过过期补跑、Unix 终止信号与广播停止，不改 HTTP/SSE 协议或生命周期 CAS。整合时保留对方 setup/shutdown 的实例与版本校验，在通过校验后接入同一停止路径。
- 冻结恢复清理在 `adapters/lib.rs` 的加载开始处调用一次 `source_views::begin_snapshot(memo)`，此外只有运行查询薄委托，不修改 Store 结构、连接或提交。若对方替换该加载路径，在新读取上下文入口执行同一清理语义；不复制旧 Store。
- 共用 `adapters/tests/orchestration.rs` 时只增加经理原生交付/故障用例；fixture 的 transport rejection 属于测试控制，不进入生产。其他共享测试按测试函数区分，不能以整文件覆盖整合。
- 文档只修改本计划、执行链/启动器说明及必要索引。对方计划的进度由其实施记录维护，不在本分支重新宣布完成。
- 后续只整合对方明确提交的变化；遇到其接口演进，调整本分支的薄接入和测试。不能为了减少文本冲突而引入第二套权威运行状态。

## 本计划的收敛

F1 的通用存储基础、F2 的有限提交机制和 F5 的执行页归对方；本分支负责 workflow 对这些能力的使用。Workflow 运行专用查询、步骤检查点延迟复制、空阶段跳过、重复驱动与原生衔接属于本次，不能把主要执行性能工作外移。F0/F6 提供 workflow 自身的调用/恢复证据，完整负载环境复用对方入口。

原验收中的零完整历史恢复、100 前端延迟和跨项目吞吐仍是最终联合目标；它们依赖对方的工程读取/提交/调度落地，不能据本分支的局部改动标为通过。本分支独立完成条件是自有修复、原生驱动/重启/撤权回归、局部性能对照和兼容性验证通过，并列出待联合验证的门禁。

## 最新在途改动的接入核对

提交本分支实现后，再次只读核对了对方基于 `3ac8486` 的未提交工作，不复制或修改它的文件：

| 对方在途改动 | 与本分支的区别及整合要求 |
|---|---|
| `OrchestrationSummary`、`WorkflowDriverSummary`、`DecisionDetail` 与 `query.*_summary` | 对方提供前端有界显示模型；本分支的 `WorkflowRuntimeWork`/占用计数只服务后台调度。两者都不授予执行权，不能用显示摘要替换经理动作的计划/epoch/停止证明校验。 |
| `source_views::is_encoded_text` | 对方为正文按需读取增加检测；本分支只增加同快照权限 memo。保留两个入口，结合新读取上下文调用 `begin_snapshot`，不复刻旧 Store。 |
| `workflow.driver.select` 的 `expected_driver_epoch` 与竞争测试 | 对方保护修改入口；本分支仅在 `drive_rust_workflows` 增加空阶段提示。测试 helper 中的 epoch 注入须保留，不以本分支整文件覆盖。 |
| `adapters/lib.rs` 的摘要委托及 project SQL 51 基线 | 对方拥有其接口/基线；本分支只追加运行查询委托。整合跟随对方正式基线，不对暂停现场做迁移。 |
| `Workspace::dispatch_with_receipt` 的摘要路由 | 与本分支 `Workspace::tick` 的去重/额度传递分属不同函数；合并时各自保留，不重写另一侧逻辑。 |

这次核对没有发现两边正在实现同一套运行计数、步骤检查点或原生交付逻辑。共享文件仍可能有插入位置的文本冲突；本表不是对方已确认分工或最终合并/联合验收通过的声明。

## 明确等待的依赖与联合验收

| 对方交付 | 本分支接入点 | 联合验收事项 |
|---|---|---|
| P1/P2 同库工程资源的完整乐观提交切换 | 会话观察、控制器调用/回复、Workflow 实例/意图/回执；复用同一读写集合与幂等回执 | 切换前后无丢失更新、重复派发、旧 epoch 接纳；正常调用不再完整恢复/复制/比较工程状态。旧 writer 仍在时不提前启用第二条业务写路径。 |
| 安装登记与事件追加的正式提交 | Host 的 materialize_harness / SessionResources；原生工具启动只消费其冻结结果 | 保留其 CAS、tombstone 和数据库分配游标；重新验证真实 Docker 准备/重启/回执，不覆盖对方 registry 实现。本分支不迁移原暂停数据。 |
| P4 执行页有界读模型 | 前端消费 peer 摘要；原生 workflow.inspect 的页内计算/响应裁剪保持独立 | 1/100 活跃前端下测 T0→T6、页面可用和事件收敛；步骤与子流程的独立游标合同仍需共同完成，不能将当前 offset 的双用途当作已修复。 |
| P5 公共调度与通知 | workflow 待办提示、native 收包/回执唤醒与同轮交付；接入有界外部 I/O 完成通知 | 本分支已独立移走原生 Docker/SDK 启动阻塞；普通 Git/Docker 工具、停止/日志导出仍可能顺序排队。公共事件唤醒、跨项目公平、全局额度、通知丢失补扫及重启收敛等待联合验收。 |
| P6 负载发生器与矩阵 | 本分支的真实 Docker 工作流、原生 Agent 和 Host 故障场景 | 同一冻结 release/数据/预算做 0/1/100 前端、历史 10 倍、长稳；原生每类至少 1000 样本。当前本机探索不等于正式容量或全部 SLA 通过。 |

冻结经理预算本轮保持原墙钟语义。活动预算、平台 deadline 和无进展 watchdog 的新配置合同仍未实施，这是本计划自身的后续策略项，不伪装成对方存储分支的已完成或待提供能力；本轮先用真实时间线降低并定位平台等待。

## 本轮独立优化的整合要求

- 原生经理只读工具不等待配置写门，仍使用完整授权快照；单记录/overview 的目标编码与计数不替代 P4 的 SQL 有界读模型。本分支不能把剩余完整授权恢复标为已消除。
- 结果事务提取原生回复所需小范围上下文，提交后交付，消除该处额外全恢复；没有更改旧 Store 的加载、复制、全状态比较或写入合同。其余事务仍须整体切换 P2。
- Host 在原生 Agent 意图回执提交后，定向推进已准备的本机 capture/lint/read；不改一般工具池或推进其他 workflow 的业务决定。
- 对方在途 `active_conversation_count`/`scheduling_hints` 与本分支已有运行计数语义相近。整合时归一到公共 SQL 提示，保留 writer 未撤权仍占位的合同，不保留两套通用调度计数实现。新 QueuedConversations 提示只用于避免无新输入时再次恢复准入状态。
- 两边共享 `conversation_runtime.rs`/`ports.rs` 的不同函数会有文本冲突；原生启动接纳、取消、停止证明和公共提示分别保留，不能用整文件覆盖消除冲突。

611 ms 的逐段分析、后续真实运行与仍未达标的证据见 [INDEPENDENT.md](INDEPENDENT.md)。原 Feature 继续暂停，不部署当前 SQL 基线到原现场。
