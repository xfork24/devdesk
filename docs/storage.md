# 存储与数据布局

本文描述数据集结构、三库分工、幂等与恢复模型及升级策略。表结构清单见 [SQL 说明](../sql/README.md)，配置存储语义见 [源配置与执行解析](source-configuration.md)。

## 数据集总览

一个数据集目录（`--data`）以 `.devdesk-dataset` 标记（内容 `devdesk-source-flat-v1`）并施加独占写锁（`.devdesk.lock`）；同一数据集同时只能有一个可写进程。

```text
<data>/
├── global.sqlite3                      # 全局配置库（并发、共享定义、项目目录、运行时安装、权限预设）
├── conversations/global/engineering.sqlite3   # 全局讨论独立库（固定命名空间，不创建项目）
├── projects/<project-id>/
│   ├── engineering.sqlite3             # 该项目的工程数据、事件与幂等记录
│   ├── artifacts/                      # 该项目的封存产物（SHA-256、fsync、拒 symlink）
│   ├── execution/                      # sessions/ tests/ git-operations/ tool-executions/（意图文件与运行目录）
│   └── repository/                     # 克隆与 worktree（主干/Feature/Task/Candidate）
└── credentials/                        # Codex OAuth 等本机凭据文件（数据集作用域）
```

## 三库分工

- **全局配置库**：共享配置、项目索引、活动选择、全局并发额度、运行时安装登记、权限预设目录与经验目录投影。只能写治理/权限/定义类记录，不得写工程数据。
- **每项目工程库**：该项目的全部工程事实。项目选择把每个请求钉到一个 Engine/一个数据库；显式 `project_id` 过期返回 `project_changed`，查询不会隐式切换项目。历史记录与工具调用按"当前项目 / Global"两个范围读取，互不泄漏。
- **全局讨论库**：scope=global 的会话单独存储，不创建占位项目。

## 配置的唯一存储

配置只存于 `source_definitions` 表（两库同构）：主键 `(identity, revision)`，`reference` 为 `identity:revision` 虚拟唯一列，触发器禁止 UPDATE/DELETE。**没有** definitions/harnesses/sandboxes 等投影表：执行视图（任务、运行时、权限、工具）在加载时由同一解析器按需重建；持久化编码把内嵌完整对象压缩为 `$source` 引用，读取时从源闭包还原——存储与内存永不漂移。

## 事实表族（速览）

| 表族 | 内容 |
|---|---|
| events / governance_records | 项目事件日志；不可变治理账本（配置修订、Controller、计划采用、变更、决定、工具意图与回执、模板目录状态） |
| operations | 幂等操作记录：相同 command_id + 参数指纹重放首次结果 |
| sessions / turns / conversation_events / interactions / proposals | 会话域：绑定、轮次（单活跃约束）、append-only 事件、人工交互、讨论提案 |
| executions | 真实执行事实（容器、镜像摘要、writer 撤销、冻结权限） |
| features / tasks / documents / contexts / snapshots / artifacts | 工程对象与材料 |
| repository_operations / repository_worktrees | Git 动作-回执与工作副本登记 |
| orchestration_wakeups / orchestration_leases | 编排唤醒队列与单写者租约 |

完整表清单见 [SQL 说明](../sql/README.md)。

## 幂等与恢复模型

所有外部调用遵循三段式：**持久化意图 → 事务外执行 → 核对回执并接纳**。命令以 command_id + 参数指纹幂等；相同 ID 不同参数被拒绝；重放先于版本冲突检查。

崩溃恢复（bootstrap）把未知终态打回 NeedsReconciliation：已派发的工具调用、仓库操作、活动轮次均进入待核对；**未知结果绝不盲目重发**，重试必须先核对远端事实（`tool.call.reconcile` / `repository.reconcile` / `workflow.step.reconcile`）。取消必须取得真实执行者的停止与写撤销证明；Git 操作另有落盘意图文件，丢失数据库回执后仍可按操作 ID 核对。

## 模板目录与默认配置

默认配置以 `resources/configuration/manifest.json` 发布（bundle 版本），空库初始化生成数据库副本；UI 编辑写入数据库修订或草稿，应用升级不改写已有目录。模板目录使用**代次（generation）**机制：重置只把当前目录选择切到默认代次，从不改变源身份；自建定义与草稿退出当前目录（归档可查），旧项目绑定、冻结运行与凭据保留。相关命令见 [本地协议](protocol.md) 的模板目录一节。

## 升级策略

系统只支持当前协议与当前 SQLite 基线（见 [SQL 说明](../sql/README.md)）：旧结构明确拒绝，不自动删除、不转换、不保留双版本执行器。升级前备份整个数据集目录；验证新版本使用独立空目录，不用既有数据目录试验。凭据引用绑定数据集与服务商，换机器后需重新配置本机凭据。
