# SQLite 迁移索引

这里全部是数据库初始化或增量迁移，不是手动执行的运维 SQL。`crates/adapters/src/lib.rs` 负责项目库迁移，`crates/adapters/src/catalog.rs` 负责全局库迁移；使用 `include_str!` 在编译时嵌入程序。

| 目录 | 数据库与主要职责 | 迁移编号 |
|---|---|---|
| `global/` | `<data>/global.sqlite3`：共享定义、安装、Agent、Permission 与治理配置 | 001–007 |
| `project/foundation/` | 项目工程库：初始结构、需求和产物追溯、项目所属文档与快照 | 001、003、016 |
| `project/execution/` | 执行绑定与来源、实例、持久化副作用、Attempt 输入输出、快照执行绑定、协议和报告引用 | 002、004、006–009、020–021、027、030 |
| `project/conversations/` | 会话与轮次、交互回答、生命周期、消息投影、输入快照和代码视图 | 015、017、019、022、024、029 |
| `project/engineering/` | 代码基线、Git 上下文、Merge 推进、规划任务、仓库操作、测试配方与交付来源 | 005、010–013、023、025–026、028、031 |
| `project/configuration/` | Agent 配置、项目运行偏好、Permission 绑定及冻结权限、版本化配置、Workflow 投影与 Role/Capability 目录索引 | 014、018、032–036 |

项目库为 `<data>/projects/<project-id>/engineering.sqlite3`。目录按迁移的主要职责分组；涉及多个领域的历史迁移保留为一个文件。

项目库的 001–036 是同一条迁移序列，全局库的 001–007 是另一条序列。007 与 036 为 Sandbox、Permission profile、不可变绑定和活动分配修订建立唯一索引，项目库另约束 Feature 权限锁与切换意图。程序按版本显式执行，不能按目录顺序执行或把每个子目录视为独立序列。

新增迁移应在所属数据库的最大版本后继续编号，放入对应职责目录，并更新适配器的执行入口。不要修改已发布迁移，也不要以删库代替升级。旧版 `sql/NNN_*.sql` 现位于 `sql/project/<职责>/`，可按原编号和文件名定位。
