# 611 ms 尾延迟的分解与第二轮独立优化

2026-11-16。分支 `codex/workflow-runtime-performance`，父提交 `26fbc27`。本轮不恢复原暂停 Feature，不替经理或业务 Agent 决策。验证使用独立空数据集、实际 HTTP Host、Docker/Codex 与单独技术流程；真实认证仅用于显式模型验证，结束清理副本。

## 611 ms 具体花在哪里

上一轮 `native-final` 的 16 个真实原生调用中，最慢是 configuration.read，T0（原生完整帧可读）至 T6（回复真正写入）611.003 ms：

| 同一调用的区间 | 耗时 | 当时能证明的含义 |
|---|---:|---|
| T0 → durable 调用创建时间字段 | 87 ms | 等待 Host 观察、调度、会话轮询及登记；不全是定时器等待。 |
| 调用创建 → 结果时间字段 | 492 ms | 恢复/授权、实际处理、事务等混合区间；原日志没有逐调用细分，不能全部叫处理器时间或 SQLite 等锁。 |
| 结果时间字段 → T6 | 32.003 ms | 结果提交剩余部分、回复准备和实际 pipe 写入；时间字段在提交前。 |

另一条 configuration.read 为 496 ms，按同口径为 95 / 370 / 31 ms。原生 materials.capture 为 535.611 ms：135 ms 登记前、222 ms 登记到平台入队、141 ms 入队到结果字段、37.611 ms 到实际写回。

本轮为每个原调用添加 action、manager gate/snapshot/read、result_commit、delivery 阶段；Agent 另分 execute/result_commit/reply，startup 单独记录。阶段使用单调计时；嵌套 span 不相加。上述数据库相关字段是提交前毫秒墙钟，分段存在取整误差，不能替代精确 SQL COMMIT 时刻。

## 独立完成的修改

| 瓶颈 | 修改与边界 |
|---|---|
| 经理读取一条配置却编码整个 EngineeringState；overview 多次重复编码 | 先选择类型化集合/目标记录，只对返回记录序列化和脱敏；overview 直接统计可见对象。经理知识查询复用当前完整授权快照，保留项目范围和隐藏配置规则。 |
| 原生只读工具等待配置写互斥门 | 已知只读方法绕过写门；修改、preview/transfer 保留原检查与写边界。真实 SQLite + Harness 端口测试持有写门时两条只读调用仍可完成。 |
| 已交付历史请求重复解码；无排队输入仍恢复一次准入历史 | 先筛出已有回复的调用；增加 QueuedConversations SQL 提示，无新输入/无槽位时不恢复第二份准入状态。提示不授权派发。 |
| 成功结果登记后为回复再恢复完整历史 | 结果事务从完整权威状态中提取调用、Session 和精确 worker 信息；只有提交成功后才使用，失败传输仍保留耐久结果。未把小视图送入全局验证器，没有更改 Store 事务实现。 |
| 原生 Agent 本机工具多等整个调度轮次 | 意图回执持久化后定向推进已准备的 capture/lint/outputs.read；复用正常权限、取消、计划与 schema 校验。Git/Docker/网络工具不进入此快速路径。 |
| Docker/SDK 同步启动阻塞正在等回复的经理 | 在核心先预约原 Turn/execution 与额度，适配器后台创建原操作并初始化 SDK；调度轮询实际回执，接纳后才发送输入。线程不写工程库、不做业务决定、不自行增加公共配额或通用队列。 |
| 冻结权限恢复反复计算固定嵌入资产/合同摘要 | 仅 memo 本二进制的期望资产摘要与按 provider 区分的固定语义摘要；避免为读取 version 计算随后丢弃的 recipe digest。动态源、profile、绑定、scope、实际安装与资产核对仍执行。 |

## 实跑发现的失败与启动阻塞

第一次完成读取与本机工具修改后的真实运行 `native`，P50 325 ms、P95/最大 **3043.723 ms**，没有隐藏这次变慢：

- 最慢 configuration.read 的 2833 ms 在登记前，179 ms 登记到结果字段，31.723 ms 到实际写回。
- 该调用的经理写门等待约 0.001 ms，授权快照 29.82 ms，**目标读取 0.017 ms**；结果事务 30.62 ms，回复快照 15.88 ms，delivery 31.43 ms。它们是嵌套/局部阶段，不能和整个往返直接相加。
- 所在 Host tick 为 7174.81 ms，其中 runtime.conversations 6978 ms；Docker create 1175.52 ms，其余包含 SDK 初始化及核心事务。新 Agent 的同步 open 占住唯一调度路径，经理已经送来的请求等到后面才被观察。

该真实发现触发本轮的可轮询启动修复。它不是把问题全部等待公共 P5，也不是用增大经理超时掩盖阻塞。

改为可轮询启动后的 `native-async`：15 次真实往返，P50 **327.028 ms**、P95/最大 **758.350 ms**。3 次实际启动成功、耗时约 6139.6 / 2549.9 / 1290 ms；启动阶段独立于其间已有原生调用处理。最慢 complete 的分段为 306 / 322 / 130.350 ms；其中动作 105.21 ms、结果事务 108.02 ms、额外回复快照 58.83 ms。后者随后被移除。

这轮 configuration.read 的目标读取约 0.02 ms，往返仍为 252–327 ms；其授权快照约 15 ms、结果事务约 23–24 ms、额外回复快照约 15 ms、delivery 约 29–33 ms。materials.capture 往返 699.23 ms，本机执行本身不到 1 ms，仍有登记前与旧事务管线开销。不能将其归因于模型计算或外部网络。

以上不同方法/大小混合且样本很少，不是同条件性能百分比对照。观察到本机其他工作负载（包括其他 Host 和部分编译）；保留原始结果，不能把干扰未经隔离地认定为全部原因。

## 最终冻结构建与真实调用

最终 Host SHA-256 **`ba3c4bcb512f4ee78eb6d52b7d4321c78db61a0fdd3a7bd7362a9b71aecf0773`**，冻结为证据目录 `host-final`。包含固定摘要复用和消除回复快照，源码 patch、父提交与 runner/probe 身份保存于 `native-final`。

该轮实际 HTTP Host + Docker/Codex：17 次原生调用（16 经理、1 Agent）全部成功，4 个 Turn completed、writer=0。经理自主完成 5 次 dispatch/accept/complete，审计 Actor 均为 controller；模型任务及输入输出断言与第一轮相同。4 次原生启动成功，分别约 3259.6 / 1308.3 / 7096.1 / 1221.3 ms，期间 Host 继续处理其他运行阶段。

| 最终轮次 | 样本 | P50 | P95/最大 |
|---|---:|---:|---:|
| 完整原生 T0→T6 | 17 | 342.610 ms | **521.961 ms** |
| 经理登记→回复登记 | 16 | 178 ms | 456 ms |

这次 P50 没有优于上一轮 279.704 ms；不以 611→522 的小样本尾部差异宣称整体性能达标。**轻量 P95≤200 ms 仍未通过**。

| 最终慢调用 | 总往返 | 登记前 / 登记→结果字段 / 结果字段→写回 |
|---|---:|---:|
| materials.capture | 521.961 ms | 187 / 298 / 36.961 ms |
| workflow.inspect | 486.255 ms | 56 / 412 / 18.255 ms |
| workflow.complete | 477.549 ms | 256 / 200 / 21.549 ms |
| workflow.step.dispatch | 444.983 ms | 250 / 175 / 19.983 ms |
| configuration.read（较慢一条） | 399.110 ms | 163 / 219 / 17.110 ms |

最慢项实际为 **Agent materials.capture**：298 ms 再拆为 206 ms 登记到平台入队、92 ms 入队到结果字段；其 execute 为 0.005 ms，结果事务 33.610 ms，reply 阶段 43.698 ms（包含回复登记，不能与 T6 前区间直接相加）。所在 tick 591.955 ms，完整工程恢复 20 次/446.699 ms，旧事务 10 次/328.304 ms，连接池等待仅约 0.022 ms。恢复与事务 span 嵌套，不能把 446+328 当总耗时。瓶颈是反复恢复/处理和顺序管线，不是本机 capture 计算或 SQLite 物理连接池等锁。

486 ms inspect 所在 tick 698.375 ms，同批处理 4 个原生调用；controller_calls 阶段 432.291 ms，完整恢复 22 次/470.427 ms，旧事务 13 次/466.142 ms。该条自身 action 32.423 ms、结果事务 37.073 ms、delivery 47.090 ms。批次串行等待放大尾延迟，不能把 412 ms 全算在 inspect 处理器上。公共 P5 的调度与 P2 的有限提交须联合核对这一批次场景，不能通过限制模型并行调用隐藏等待。

两次最终 configuration.read 的实际目标读取为 0.019 / 0.038 ms、门等待为 0；授权快照约 22 ms、结果事务约 34–38 ms、delivery 约 42–45 ms。额外 reply_snapshot 已消除，剩余旧事务开销仍在。一个观察用 query.workspace 仍常含 5 次完整恢复，接入 P4 后须重新联验。

该轮另记录 121 次进程快照，有 4 次观测到其他 cargo/rustc/cargo-fmt，与 capture 附近时间重叠；另一 Host 持续占用 CPU。没有终止对方任务。该环境不能作为安静、同条件容量基准；本轮后再用相同冻结二进制复核一次，保留两次结果。

### 相同构建复跑，保留 1.19 s 尾部

`native-repeat` 使用同一 Host SHA，18 次真实调用全成功、3 个 Turn completed、writer=0，5 次推进 Actor 均为 controller。P50 **321.511 ms**、P95/最大 **1189.626 ms**；登记到回复登记 P50/P95 为 164/302 ms。不能只选择前一轮 522 ms 作为本轮结论；仍存在秒级平台衔接等待。

最慢 workflow.inspect 是 **1054 ms 登记前 +119 ms 登记到结果字段 +16.626 ms 到写回**。自身 action 18.984 ms、结果事务 30.945 ms、delivery 37.191 ms。所在 tick 1704.502 ms、runtime.conversations 1335.033 ms，其中 **4 次 Docker inspect 共897.540 ms**，完整状态恢复16次/358.285 ms。正常 worker 终结的 stop/proof/清理仍同步执行，拖延同轮其他会话收包；本轮原生 startup 已异步，不代表 terminal stop/log/cleanup 也已隔离。这一残留要与公共 P5 的 I/O 完成通知和 P2 的耐久终态接纳共同完成，不能提前标记 writer 已撤权或以独立私有队列绕过公共合同。

同一轮另一个 accept 为 568.419 ms，293 /236 /39.419 ms，结果事务70.238 ms；对应旧事务管线仍是独立于 Docker 的瓶颈。该轮129次监测有49次观测到外部构建工具，仍不是安静对照。调度阻塞和恢复次数是直接测量事实，外部干扰对最终数值的贡献没有被独立量化；不据此豁免 SLA。

## 停机、取消与恢复

核心确定性测试验证：pending startup 占用原额度、不发送输入；两个启动等待时第三个不能越额；取消一个并取得停止证明后第三个进入；完成启动仅发送一次输入。适配器覆盖取消先于实际创建、结果失败保持、线程中断未知、停止后的缓存不能误删新操作。

`shutdown-startup` 在实际 Docker 容器 Running、首条输入仍 consumed=0 时发送 SIGTERM。Host 退出 0；旧 Turn needs_reconciliation、所有 writer 已撤权、consumed 仍为 0。重启没有新增或重发输入；明确 takeover/核对旧执行后，后继经理自主完成单节点技术流程。该轮使用冻结 `host-async`，SHA-256 `e700fa42fa1c1f378fd9c52064bec8bcf122d2410db16dc80b4178ef5cb42f04`。

SIGKILL 在途 Docker CLI 的安全回收本轮没有实机证明；缺失本进程启动任务时禁止重开，仍进入核对。历史 Docker exit 1 的 stderr 和历史 Host 原退出信号仍缺失，新验证不回填历史根因。冻结经理墙钟预算未改，新活动预算/deadline/watchdog 合同也未实施，不能写作对方分支的阻塞项。

## 确实依赖 single-host-concurrency 的部分

| 剩余开销/能力 | 依赖和联合门禁 |
|---|---|
| 经理授权、会话观察、控制器结果/回复、Workflow 评估的旧 transact 全量恢复、clone、序列化比较 | P2 对所有共享约束入口整体切换有限读取/提交；资源集合包括 Session/Turn、execution/operation/停止证明、控制器 epoch/lease、plan/rootstep/control version、配额和输入输出。旧 writer 尚在时本分支不能加第二套业务 CAS。 |
| 原生收包至 Host 观察、慢一般工具/停止/日志导出排队 | P5 公共事件唤醒、有界外部 I/O、跨项目公平和丢失通知补扫。原生 startup 的阻塞已在本分支独立修复；剩余一般队列不能以本次 startup 修复宣称完成。 |
| HTTP 观察/执行页抢占运行资源、完整 workspace 查询 | P4 有界执行页/步骤/材料读模型及事件失效。当前探针有一个 HTTP 观察者，仍会执行较重查询；联验使用对方稳定接口，步骤与子流程游标合同也须明确。 |
| 安装登记、事件游标和 SQL 基线 | 对方 global21/project52 的正式 registry/CAS/event 实现；本分支保持 global19/project49，不迁移暂停现场。 |
| 正式 P95/P99 与容量结论 | P6 固定构建/数据/预算、0/1/100 活跃前端、10倍历史、长稳、逐方法至少1000样本。小样本混合 P95 只能用来发现问题。 |

只读核对对方至 `7dfa7ef` 及其未提交 scheduling_read/event_hub 等工作，没有复制在途代码。具体共享文件接入和计数归一要求见 [SCOPE.md](SCOPE.md)。

## 回归与现场

- 最终核心/适配器定向回归：core library17、adapters library45、manager_workspace10、orchestration50通过；配置权限14通过/1显式真实入口ignored，runtime_registry3、standard_snapshots5通过。原生启动/定向衔接改动后的 configured_workflows49、conversations10通过。最终Host41通过/3显式入口ignored，其中实际Git/Docker入口另行显式通过。
- 最终实际 Git/Docker 工作流通过：8步、6次实际 Docker Node 测试、15个 tick、总11.301s；每轮唯一评估断言保留。它不调用模型或 GitHub，也不是正式容量成绩。
- 实际 SIGTERM 启动中断/无输入重发/后继经理完成验证通过；独立 HTTP 信号/SSE合同通过。具体冻结身份见各原始证据，不能把所有历史入口倒填成最终源码成绩。
- 最终 Node24完整 style（rustfmt/clippy/ESLint/TypeScript/Prettier/Web build）、架构、RPC清单260合同/9路由、Python编译、差异空白通过。
- 23:15只读复核：原业务 Feature、根/子流程与经理继续暂停，经理epoch5、活动Turn0、未撤权writer0；未恢复原输入、未做业务审批、未部署或迁移原数据。原Host HTTP200。所有本轮专用测试容器已停止，认证副本已清理。main后来出现其他任务的文档/数据修改，本分支没有操作它们或对方工作树。

证据根目录 `/tmp/devdesk-workflow-independent-20261116`，原 611 ms 证据 `/tmp/devdesk-workflow-live-20261116/native-final`。日志、patch、身份、SQLite 和 auth 不提交；临时目录不保证长期保存，复跑入口见 [测试说明](../../../tests/README.md)。
