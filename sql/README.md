# 当前 SQLite 结构

系统直接使用当前数据库基线，不在启动时执行旧系统的逐级迁移。

| 文件 | 数据库 | user_version |
|---|---|---:|
| `global/schema.sql` | `<data>/global.sqlite3`，共享配置、安装和项目索引 | 10 |
| `project/schema.sql` | `<data>/projects/<project-id>/engineering.sqlite3`，工程、会话和执行记录 | 41 |

两个文件在编译时嵌入程序，空库以事务直接创建。已有库必须匹配当前版本；旧单库布局和其他旧结构被明确拒绝。Permission profile、精确绑定和活动分配存放在治理记录中，v1 `permissions` 表、旧全局权限设置和项目覆盖列已移除。

结构变更需要显式的数据迁移与校验。不要以删库代替升级或恢复，也不要在运行路径增加旧行为回退。
