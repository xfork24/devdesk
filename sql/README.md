# 当前 SQLite 结构

系统使用当前数据库基线，并只对当前基线追加本次模板目录索引迁移；不恢复旧系统逐级迁移链。

| 文件 | 数据库 | user_version |
|---|---|---:|
| `global/schema.sql` + `global/011_template_catalog.sql` | `<data>/global.sqlite3`，共享配置、安装和项目索引 | 11 |
| `project/schema.sql` + `project/042_template_catalog.sql` | `<data>/projects/<project-id>/engineering.sqlite3`，工程、会话和执行记录 | 42 |

基线文件仍分别为 10、41，在编译时嵌入程序。空库以事务创建基线后应用 `global/011_template_catalog.sql` 或 `project/042_template_catalog.sql`；main 的已有 10/41 库只追加同一事务索引迁移，定义和执行数据保持原值。已有库必须匹配基线或迁移后版本；旧单库布局和其他旧结构被明确拒绝。Permission profile、精确绑定和活动分配存放在治理记录中，v1 `permissions` 表、旧全局权限设置和项目覆盖列已移除。

结构变更需要显式的数据迁移与校验。不要以删库代替升级或恢复，也不要在运行路径增加旧行为回退。

模板状态、不可变修订成员和回执复用 `governance_records`，新增目录唯一索引与代次索引。重置仅通过目录代次切换当前定义，精确历史引用继续有效。
