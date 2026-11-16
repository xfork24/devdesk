# 真实 Workflow 执行与平台衔接验证

2026-11-16。本报告是独立技术场景的实施验证，不是原 Feature 的完成报告。原 Feature、根/子流程与项目经理保持暂停，未派发其 Agent、重发其输入、生成其业务文档或改变审批。数据集与本地仓库全部新建；真实模型认证仅用于这些明确的原生执行场景，临时副本在退出时删除。

后续 611 ms 分解、原生启动阻塞及第二轮实际结果见 [INDEPENDENT.md](INDEPENDENT.md)。本页保留第一轮原构建成绩，不倒填新实现的效果。

## 源码与环境

分支 `codex/workflow-runtime-performance`；main 已整合到 `b75439d`（合并提交 `38ec1ff`）。本分支沿用已整合的并发基础 global 19 / project 49；对方正在实施 global 21 / project 51，不将候选程序部署到原运行中的 global 18 / project 48。

真实链路验证的 Host SHA-256 为 `1d4990165badad50b6fd0f947e8cda52ceb6c63e1781279c737e1e49ace604ea`，冻结在证据目录的 `candidate-host`，对应源码补丁保存在 `native-final/source.patch`。Rust 使用仓库固定工具链，样式门禁使用 Node 24。实际 Docker 测试还固定 `sha256:3286c4e2bbfc26f10427a9699b0977443e8657baaa157cc33319b314dd28c22a`；Host 自己准备的实际 Harness/测试环境另保留原安装身份和冻结任务。构建日志、探针摘要、源码补丁、SQL 与原始时间线在 `/tmp/devdesk-workflow-live-20261116`；不提交凭据、日志或运行数据。

这是本机开发环境的真实链路探索，未提供三轮受控 before/after、每类 1000 样本、100 活跃前端或历史 10 倍矩阵，不据此宣布全部 SLA/容量通过。较早轮次与构建有重叠，保留原结果但不作算法收益百分比。

## 本轮实际修改

| 问题 | 修复与边界 |
|---|---|
| Host 外层两个意图派发阶段仍各做完整评估 | 正常轮次的评估统一由 Engine::tick 拥有；外层只交付耐久意图。真实场景逐轮断言一次评估，启动恢复的独立评估保留。 |
| 没有意图/原生回复也恢复历史 | WorkflowIntents / AgentReplies 通过现有读池查询待办；没有对应待办直接返回。提示不授予权限，不替代实际接纳校验。 |
| 外部工具等待时反复写同一 running 行 | prepared 首次转 running，后续无结果或 resource_pending 不提交新事实；确定性回归确认重复观察后完整状态摘要不变。 |
| inspect 分页前计算所有步骤并序列化全部冻结定义 | 先限定本页的步骤说明，完整校验后裁掉本地响应对象的页外步骤及未公开定义；原权威状态不改变。 |
| Docker 结束后重复核对同一容器 | 复用本次 inspect 的停止证明，按确切容器 ID 导出日志；仍运行或需 stop 的容器保留实际停止及重新核对。 |
| 启动器转发信号但 Host 对 SIGTERM/SIGHUP 不清理 | Unix 信号进入与 HTTP/SIGINT 相同的停止、SSE 关闭与所属 Harness writer 清理路径；输出来源与清理完成事件。SIGKILL 仍须重启核对。 |
| 原生交付只测登记时间 | 增加读入完整帧到实际回复写入的单调时钟计量，按 operation/序号/请求摘要关联，不存参数或正文；登记字段明确为提交前墙钟边界。 |
| 旧实际探针使用过期 Profile 查询，且 root 完成后立即杀 Host | 改用权限目录，等待实际经理 Turn 结束及 writer 撤权；独立中断场景按停止/核对/后继会话验证，不盲重发。 |

## 实际 Git、Docker 与自动驱动

`git-docker-owner` 使用真实 Workspace/Engine/SQLite、实际本地 Git、实际 Docker Node 测试、Artifact 和 Rust driver。初始化专用本地仓库时，仅以实际 Git 回执登记工作空间；未模拟工具结果、调度或接纳，未调用 GitHub 或模型。

完整图为 `workspace.current → workspace.snapshot → 6 次 tests.run`，8 个步骤都由正式驱动派发/接纳并完成根实例。没有手动逐步推进。实际总耗时 **11.483 s，15 轮调度**；每轮 `runtime.evaluate_workflows.calls=1`。6 次 Docker 的实际退出到平台 `finished` 字段登记为 **244、233、257、241、237、266 ms**。该字段位于事务提交前，不冒充严格的 T5−T4；仍有超过 250 ms 的样本。

同一图的较早诊断记录有 324 次完整恢复、115 次旧事务、42 次 Docker inspect；最终记录为 272 次完整恢复、85 次旧事务、30 次 inspect。减少的是重复工作；**272 次完整恢复和 85 次旧事务仍然存在**，需接入公共有限读取/提交后消除，不能宣称后台已零完整恢复。早期轮次环境不完全受控，不从这些单次时间差计算正式加速比。

额外实际 Docker Red/Green/环境失败矩阵通过，保留断言失败、环境错误、实际固定候选、停止回执和结果归属。没有把模型自述当作测试成功。

## 实际 Codex 经理与 Agent

`native-final` 使用正式 HTTP Host 与调度器、真实 Docker 内 Codex 经理及研究 Agent。Agent 按输入 `[3,1,2,3]` 使用冻结 Skill 计算并实际调用 materials.capture；经理自主读取、检查、派发、接纳两个冻结步骤并完成根实例。测试只核对输出 `[3,3,2,1]` / `9`、实际平台回执与状态，没有替经理/Agent执行其计算、决定或签署。

16 次原生请求全部取得实际写入终点（15 次经理调用、1 次 Agent 调用），3 个 Turn 最终 completed，未撤权 writer 为 0。审计 5 次 dispatch/accept/complete 的 Actor 均为真实 controller，探针没有直接完成这些动作。零浏览器，另有一个观察状态的 HTTP 探针。

| 实际区间 | 样本 | P50 | 本轮 P95/最大 |
|---|---:|---:|---:|
| 完整帧可读 → 回复实际写入 T6−T0 | 16 | 279.704 ms | 611.003 ms |
| 经理调用登记 → 回复登记 | 15 | 203 ms | 544 ms |
| 原生可读 → 调用登记 | 15 | 119 ms | 最大 194 ms |
| 调用登记 → 结果时间字段 | 15 | 137 ms | 最大 492 ms |
| 结果时间字段 → 回复写入 | 15 | 35.5 ms | 最大 41.7 ms |

后面三个区间是逐调用关联，墙钟字段为毫秒精度、位于提交前；只有完整 T0→T6 使用同进程单调时钟，不将它们当作精确事务提交 span。最慢两次为 configuration.read；样本按方法与大小仍需扩充。轻量调用也有超过 200 ms 的结果，所以**未通过轻量 SLA**。没有将数百毫秒称为零开销，或把查询性能等同于原生往返。

较早 `native-observed` 取得 18 个实际往返，P50 576.929 ms、最大 1141.148 ms；其开始阶段与编译重叠。它支持发现登记前排队和重复阶段，不作为同条件 before。过去现场的分钟级数字与这些新数据的运行图、历史量及构建不同，也不能直接计算收益倍数。

## Host 中断与恢复

正式 HTTP 检查通过 HTTP、SIGINT、SIGTERM、SIGHUP 与保持连接的 SSE；终止信号的实际 Host 退出码为 0，保留 shutdown_requested 来源及 runtime_stopped 终点。最终 HTTP 轮次约 0.05–1.09 s，仍只是少量正确性样本。

实际 Docker/Codex 经理在原生请求可读时收到 SIGTERM，旧 Turn 保留 needs_reconciliation 与实际 consumed，原 writer 已撤权。重启后不新增 Turn、不重发原输入。未知 Turn 必须先明确取消并取得原停止事实，随后 orchestration.resume 建立不同 Session；后继经理使用新继续指令，自主完成独立技术流程。最终 `native-shutdown-final` 通过：原 Turn cancelled、后继 Turn completed、未撤权 writer 为 0，实际等待会话结束后才关闭测试 Host。这不恢复原暂停 Feature。

先前直接 resume 的探针被“请先停止并核对旧编排执行者”拒绝，是正确围栏；没有绕过它或改库。探针后续加入显式 takeover/等待取消。另保留一轮在根完成后立即停止经理的诊断，最终入口已改为等待实际 Turn 收尾，避免将 root 完成冒充会话结束。

历史 Docker 首次 exit 1 原 stderr 丢失、旧 Host 原信号/退出码缺失的事实没有改变。新故障验证不证明已经追溯那两次历史根因。

## 回归与剩余门禁

最终定向回归：adapters library 39、configured_workflows 48、conversations 9、orchestration 50、core library 15、Host 41 项通过；Host 3 个显式入口默认忽略。另实际 Git/Docker 工作流、Docker Red/Green/环境失败矩阵、正式 HTTP/连接合同通过。Node 24 完整 style 门禁通过，包含 lint、类型检查、Prettier、Web 构建和全目标 clippy；不是完整浏览器/工作区容量验收。

末尾入口清单检查发现 main 的 3 个只读查询未登记，以及阶段指标被静态扫描误认为 RPC。补录 configuration_index / configuration_records / model_catalog_identity 的只读合同，将指标明确列为非方法，并排除独立的测试模块；260 个合同/9 路由的清单、合同单测、架构检查和最终 clippy 通过。这项最后的元数据补齐另行构建检查，不倒填到前面的原生二进制成绩。

依赖与接入点逐项见 [SCOPE.md](SCOPE.md)：共享业务 CAS 整体切换、安装登记/数据库事件游标、执行页有界读取、P5 外部 I/O 与事件唤醒、公平调度，以及 100 前端/历史增长/长稳矩阵。步骤与子流程独立游标尚未完成。冻结经理预算保持原墙钟语义；新活动预算/平台 deadline/watchdog 属于本计划的后续策略，尚未实施，不归因于对方未提供能力。

复跑命令见 [测试说明](../../../tests/README.md)。继续保持原 Feature 暂停；整合后在新的兼容专用数据集联合验收，不自动迁移、删库或恢复原业务现场。
