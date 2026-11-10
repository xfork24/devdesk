# 当前 SQLite 结构

本次改造使用全新的开发数据基线，不迁移旧数据、不保留旧业务合同。

| 文件 | 数据库 | user_version |
|---|---|---:|
| `global/schema.sql` | `<data>/global.sqlite3`，共享配置、安装与项目索引 | 13 |
| `project/schema.sql` | `<data>/projects/<project-id>/engineering.sqlite3`，材料、会话与流程运行 | 44 |

空库在事务中直接创建当前结构。其他版本会被明确拒绝；此项目尚未上线，旧开发数据按本次改造授权删除后重建。不要在运行路径加入隐式迁移或旧动作回退。

Workflow 修订、冻结计划、StepExecution、实际输出、确认、历史轮次与工具回执保存于版本化治理记录；材料使用显式 `input_refs`，Feature 使用 `source_materials`。旧 Attempt、Run、Delivery/Review、Merge 等业务调度表及旧配置索引已删除。原生 Session、Turn、RuntimeExecution、仓库操作、Artifact 与幂等记录继续承载实际执行事实。

模板目录使用身份、代次及不可变成员索引。重置只切换当前目录，正常运行中的冻结引用保持确切修订。凭据独立保存于数据集凭据目录或系统凭据库，不写入业务配置与运行数据库。

当前结构已删除对象上的空 BindingLock、项目运行时偏好、Definition 的旧 slots/rules，以及旧 execution_task 索引。每个 Step 独立冻结 Session 运行配置。
