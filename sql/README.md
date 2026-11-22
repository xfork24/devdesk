# 当前 SQLite 结构

| 文件 | 数据库 | user_version |
|---|---|---:|
| `global/schema.sql` | `<data>/global.sqlite3`，共享源配置、安装与项目索引 | 24 |
| `project/schema.sql` | `<data>/projects/<id>/engineering.sqlite3`，项目源选择、材料与执行事实 | 60 |

数据集标记为 `devdesk-profile-runtime-v2`。空库在事务中创建当前结构；清空后仅剩标记、锁及 Finder 元数据的目录可重新初始化。其他包含数据的目录若协议或数据库版本不兼容则直接拒绝，不执行升级、回退或转换，也不改写旧库。

`source_definitions` 是源配置的唯一存储，主键为 `(identity, revision)`，`reference` 为 `identity:revision` 的虚拟唯一列。源修订不可更新或删除。正文只属于一个源修订，Permission 正文不重复身份、名称及版本。没有 `definitions`、`harnesses`、`sandboxes` 投影表。

`governance_records` 承载执行事件、Controller、Plan 采用记录、变更、决定、工具意图及回执。JSON 中的配置内容保存为确切源引用和实际选择；完整 Plan、Agent 执行配置、权限、工具与接入视图在读取时统一重建，不保存展开闭包。

project 60 的执行合同支持显式 time_limits v1 和步骤 time_budget 事实，显示行保存其下一期限提示。步骤物理权威仍是原实例的 steps JSON，没有拆分或双写另一套执行事实。project 59 及更旧版本在初始化写入前拒绝，不自动转换数据。

project 61 将运行记录的长正文字段改为不可变对象引用。SQLite 保留查询和校验所需的字段。归档表按摘要定位包内偏移和长度。project 60 通过显式命令迁移，Host 启动保持原版本。

项目库另有从 Scope 计算的 owner_project 虚拟列及有界运行查询索引，覆盖 project/feature 归属与子流程父执行。通用事实编解码只选择可写的物理列；虚拟列不进入源正文或变更提交。

Session、Turn、RuntimeExecution、仓库操作及 Artifact 保存不可由配置重建的执行事实。幂等记录同样保存源引用，重放时解析；人工决定、失败报告、输入来源与实际回执不因解析而改写。流程修改在接纳时形成新的 Workflow 源修订，既有执行仍关联原计划和源修订。

模板目录使用独立的代次选择记录；恢复出厂选择复用同一源引用。运行引用的源修订永久保留。凭据只保存不透明引用，不进入源正文或运行日志。

有限资源提交通过 `resource_versions(kind, resource_id, revision)` 保护资源前置条件。回执与业务事件在同一事务提交，新回执的 `operations.versions` 通过 `operations.cursor` 引用 `resource.changed` 事件中的版本列表。新 Host 同时支持旧回执的内联列表。业务入口和回调以原资源读集合校验版本，原生工作流事件随资源变化在同一事务追加。global 18 / project 48 不自动转换为当前基线。
