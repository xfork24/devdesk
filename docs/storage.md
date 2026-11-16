# 存储与数据布局

本文描述数据集结构、三库分工、幂等与恢复模型及升级策略。表结构清单见 [SQL 说明](../sql/README.md)，配置存储语义见 [源配置与执行解析](source-configuration.md)。

已确认的并发重构继续使用 SQLite：独立读快照、资源级版本比较与短事务提交、无应用层全局/项目串行写锁，视图允许最终一致。实施分支已接入读连接池与有限资源提交，共享项目选择已移除，旧业务写锁尚未替换；本文以下描述现行业务路径，目标结构见 [底层重构方案](plans/single-host-concurrency/PLAN.md)，容量门禁见 [100 前端验收合同](plans/single-host-concurrency/ACCEPTANCE.md)。

## 数据集总览

`resource_versions` 为有限资源提交保存 `(kind, resource_id, revision)`；`operations` 同事务保存结果、新版本集合和事件游标。旧业务写路径仍在迁移，不能将新端口测试视作所有入口已具备并发保护。

一个数据集目录（`--data`）以 `.devdesk-dataset` 标记（内容 `devdesk-profile-runtime-v2`）并施加独占写锁（`.devdesk.lock`）；同一数据集同时只能有一个可写进程。

```text
<data>/
├── global.sqlite3                      # 全局配置库（并发、共享定义、项目目录、运行时安装、权限预设）
├── conversations/global/engineering.sqlite3   # 全局讨论独立库（固定命名空间，不创建项目）
├── projects/<project-id>/
│   ├── engineering.sqlite3             # 该项目的工程数据、事件与幂等记录
│   ├── artifacts/                      # 该项目的封存产物（SHA-256、fsync、拒 symlink）
│   ├── execution/                      # 会话、测试、工具、Git 意图及 worktrees/ 工作副本
│   └── repository/                     # 受管理的 GitHub 仓库克隆
├── runtimes/                           # 共享运行组件与构建工作目录
├── packaged-permission-presets/        # 内嵌权限预设的内容寻址缓存
└── credentials/                        # Codex OAuth 本机凭据文件（数据集作用域）
```

## 三库分工

- **全局配置库**：共享源配置、项目索引、全局并发额度、运行时安装、准备/验证记录和权限预设来源；不保存项目工程执行数据。经验目录从各库只读汇总，不保存第二份工程事实。
- **每项目工程库**：该项目的全部工程事实。显式 `project_id` 把每个请求钉到一个 Engine/一个数据库；Host 不保存活动项目，也不按其他前端的选择回退。历史记录与工具调用按"当前项目 / Global"两个范围读取，互不泄漏。
- **全局讨论库**：scope=global 的会话单独存储，不创建占位项目。

## 配置的唯一存储

配置只存于 `source_definitions` 表（两库同构）：主键 `(identity, revision)`，`reference` 为 `identity:revision` 虚拟唯一列，触发器禁止 UPDATE/DELETE。没有 definitions/harnesses/sandboxes 等投影表：执行视图（任务、运行时、权限、工具）在加载时由同一解析器按需重建；持久化编码把内嵌配置对象压缩为 `$source` 引用，读取时从原源闭包还原，缺失或内容冲突直接报错。SQLite 基线为 global 20 / project 50。

配置目录另有可丢弃的进程内快照，不新增展开配置投影。`catalog_clock` 只保存目录快照时钟，由源、目录治理、Project 和 mainline 工作副本的 SQL 触发器在同一事务中推进；它只用于读取新鲜度，不是项目写入乐观锁。读取在同一 SQLite 快照内取得时钟与目录事实，热读返回该版本的不可变 Arc，不跨连接比较 `PRAGMA data_version`。慢读取不能以新版本标记旧数据或覆盖更高版本的缓存。

共享目录与项目目录的只读组合最多保留 8 个视图；底层缓存按估算字节计账，单项 32 MiB、进程合计 128 MiB，预算随旧 Arc 的最后一个读者释放，过大或预算不足时读取但不保留。跨项目只读目录最多复用 8 个读池，各有最多 4 个读连接；初始化、临时只读连接和各池共享进程 64 个连接的预算，先取得额度再打开 SQLite，SQL 不持有池的互斥锁。
同一快照还保存模板成员、head、来源和停用索引，以及最多 16 项纯查询结果；查询结果按序列化大小的两倍计账，单快照 4 MiB、进程合计 32 MiB，超额逐出或不保留。成功结果才缓存，编译期间不持有 memo 锁。键包含方法、完整参数和快照全部配置输入；权限矩阵额外包含运行程序登记及后端资产摘要。重启与源提交重建视图，命令与执行不使用展示结果替代权威校验。

浏览器配置查询只在内存保留，最多 64 项、估算 8 MiB；模型目录单独限制 16 项、估算 1 MiB。缓存按连接/数据路径、Host、全局及项目提交序列、项目来源摘要与参数隔离。相同进行中请求合并，普通读取在最后一个消费者离开后取消传输；HTTP 服务可跳过已取消的排队读取，已开始的同步工作仍可能完成。模型发现归有界后台服务管理，页面离开后仍可完成，切回继续复用；更换身份或主动失效会清空它。没有新增持久化目录、配置格式或迁移。

SSE 按事件主键和游标读取 `events`，提交后通知唤醒读取，低频补查处理丢失通知；不为事件轮询重建完整状态。全局讨论的 Agent 目录读取配置快照，经验列表按 Agent 查询既有经验记录和快照文本摘要，具体快照详情继续核对封存清单与摘要。

## 事实表族（速览）

| 表族 | 内容 |
|---|---|
| source_definitions | 不可变配置源修订及根级引用；执行视图按原引用恢复 |
| events / governance_records | 项目事件日志；Controller、驱动、计划采用、变更、决定、工具意图与回执、模板目录状态 |
| operations | 幂等操作记录：相同 command_id + 参数指纹重放首次结果 |
| sessions / turns / conversation_events / interactions / proposals | 会话域：绑定、轮次（单活跃约束）、append-only 事件、人工交互、讨论提案 |
| executions | 真实执行事实（容器、镜像摘要、writer 撤销、冻结权限） |
| features / tasks / documents / contexts / snapshots / artifacts | 工程对象与材料 |
| repository_operations / repository_worktrees | Git 动作-回执与工作副本登记 |
| orchestration_wakeups / orchestration_leases | 编排唤醒队列与单写者租约 |

完整表清单见 [SQL 说明](../sql/README.md)。

源描述的文件/SQL 编解码在 adapters；冻结计划与任务选择的纯重建在 core/frozen_views。两者共享精确源引用，不以当前目录最新版重选执行依赖。`command_context` 保留由可信入口构造的 Actor/项目归属与原请求指纹，不接受客户端提供的解析后身份。

## 幂等与恢复模型

所有外部调用遵循三段式：**持久化意图 → 事务外执行 → 核对回执并接纳**。命令以 command_id + 参数指纹幂等；相同 ID 不同参数被拒绝；重放先于版本冲突检查。

崩溃恢复（bootstrap）把未知终态打回 NeedsReconciliation：已派发的工具调用、仓库操作、活动轮次均进入待核对；**未知结果绝不盲目重发**，重试必须先核对远端事实（`tool.call.reconcile` / `repository.reconcile` / `workflow.step.reconcile`）。取消必须取得真实执行者的停止与写撤销证明；Git 操作另有落盘意图文件，丢失数据库回执后仍可按操作 ID 核对。

## 模板目录与默认配置

默认配置以 `resources/configuration/manifest.json` 发布（bundle 版本），空库初始化生成数据库副本；UI 编辑写入数据库修订或草稿，应用升级不改写已有目录。模板目录使用**代次（generation）**机制：重置只把当前目录选择切到默认代次，从不改变源身份；自建定义与草稿退出当前目录（归档可查），旧项目绑定、冻结运行与凭据保留。相关命令见 [本地协议](protocol.md) 的模板目录一节。

## 升级策略

系统只支持当前协议与当前 SQLite 基线（见 [SQL 说明](../sql/README.md)）：旧结构明确拒绝，不自动删除、不转换、不保留双版本执行器。升级前备份整个数据集目录；验证新版本使用独立空目录，不用既有数据目录试验。凭据引用绑定数据集与服务商，换机器后需重新配置本机凭据。

## 配置发布恢复

`configuration_transfer` 保存跨库候选及 prepared/applying/completed/failed/cancelled 状态；暂存正文不是 source_definitions 中的可用源。项目原子提交源副本、选择和 configuration_transfer_receipt；共享库随后发布相同源并标记完成。重启从 applying 与项目幂等回执继续，不重复创建选择。相同源引用的内容必须一致。

## 证据归档

`tests/live/export_evidence.py` 使用 SQLite backup 生成一致快照，关闭快照后计算摘要，按项目登记核验 Artifact 的路径与实际内容，再生成逐文件 SHA-256 清单。记录投影遮蔽已知凭据字段；全局认证文件、进程环境和私有 worker 状态不导出。归档保留实际失败、未运行项、执行安装及原始来源，不用模型摘要替代证据。

实现依据：[全局 SQL](../sql/global/schema.sql)、[项目 SQL](../sql/project/schema.sql)、[数据集与项目库](../crates/adapters/src/lib.rs)、[Git 目录](../crates/adapters/src/git.rs)、[证据导出](../tests/live/export_evidence.py)。
