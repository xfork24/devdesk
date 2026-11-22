# SQLite、资源提交与产物文件

存储层实现 core 的 EngineeringStore 和 ArtifactStore。它把明确的读取/修改请求转成 SQL 或文件操作，不自行决定哪一步应该完成。

## 打开与分库

[lib.rs](../../../crates/adapters/src/lib.rs) 的 SqliteStore 提供项目和全局讨论工程库；[catalog.rs](../../../crates/adapters/src/catalog.rs) 的 GlobalStore 管理全局配置库。打开前检查数据集锁、标记、布局及 user_version，拒绝不兼容数据。

[connection_pool.rs](../../../crates/adapters/src/connection_pool.rs) 管理连接，[resource_read_scope.rs](../../../crates/adapters/src/resource_read_scope.rs) 支持读取范围。查询从已提交视图取数据，耗时外部操作不持有 SQL 事务。

工程库打开时，[project_storage.rs](../../../crates/adapters/src/project_storage.rs) 检查已提交的旧记录。材料意图须与原命令回执相符，平台准备须有对应的终态调用和回执，会话准备须有终态 Turn，才可以裁剪相应运行载荷。旧记录核对和改写在同一事务完成，达到空闲页阈值后回收数据库文件空间。

## 一次提交经过什么

1. core 提交明确的资源版本、变更、事件及命令身份。
2. [resource_commits.rs](../../../crates/adapters/src/resource_commits.rs) 将资源类型映射到允许的表或查询，核对原版本。
3. 短事务内写入结果及相关版本、事件和回执；[store_changes.rs](../../../crates/adapters/src/store_changes.rs) 通知读取者。
4. 重放原命令返回原结果；冲突交给调用方处理，存储不重新生成业务决策。

[store_changes.rs](../../../crates/adapters/src/store_changes.rs) 的通知只提示变化，不能替代事务提交。跨库目录摘要和经验索引允许后台修复，原业务提交仍以所属库为准。

命令回执通过 SHA-256 核对较长的命令指纹，通过事件游标读取已保存的资源版本。命令来源记录继续保存工作流审计所需的确切参数。未完成的命令仍使用原冻结计划。材料封存和平台准入成功时，同一提交保留重放回执并裁剪已交接的恢复载荷。会话打开后保留原 Turn、worker 和工作区检查点所需依据。

## 按需读取

| 查询对象 | 主要实现 |
|---|---|
| 工作流摘要、步骤和输出 | [workflow_readers.rs](../../../crates/adapters/src/workflow_readers.rs)、[workflow_details.rs](../../../crates/adapters/src/workflow_details.rs) |
| 调度工作与待执行操作 | [scheduling_read.rs](../../../crates/adapters/src/scheduling_read.rs)、[workflow_intents.rs](../../../crates/adapters/src/workflow_intents.rs) |
| 会话和历史 | [conversation_queries.rs](../../../crates/adapters/src/conversation_queries.rs)、[operational_reads.rs](../../../crates/adapters/src/operational_reads.rs) |
| 事件 | [workflow_events.rs](../../../crates/adapters/src/workflow_events.rs) |
| 源定义与编码 | [source_store.rs](../../../crates/adapters/src/source_store.rs)、[source_views.rs](../../../crates/adapters/src/source_views.rs) |

列表先限制页和内容大小，再解码必要记录。新增 UI 查询时优先增加明确读取方法，不用完整 EngineeringState 恢复来代替局部查询。

## 产物怎样保存

FileArtifacts 在 lib.rs 实现 ArtifactStore；[named_artifacts.rs](../../../crates/adapters/src/named_artifacts.rs) 支持准备后保存与核对。core 先确定 ArtifactSeal，再在 SQL 事务之外写内容，随后提交关联记录。

读取根据 Artifact 身份和摘要验证内容，不接受来自浏览器的任意宿主路径。处理崩溃恢复时要覆盖“文件已写但数据库未完成”的情况，不能把重复写入当作普通新产物。

快照正文与已验证的封存文件逐字节相等时，SQLite 可只存 Artifact 引用和有界列表摘要。正文读取会重新核对封存文件的 SHA-256 和快照身份。没有可验证文件的快照正文继续保存在 SQLite。备份与恢复要同时保留工程库和对应的 `artifacts/`。

project 61 的 [payload_store.rs](../../../crates/adapters/src/payload_store.rs) 用项目内 SHA-256 对象保存长正文。已封存的工作流输出可引用原文件。准备写入时先持久化正文，再在 SQLite 短事务内提交引用。精确读取校验长度和摘要，并按原记录的字段恢复正文。调度查询使用内联的状态、身份和关系字段。

项目资料无过滤列表支持游标模式。客户端首次传 `snapshot_digest="seek"`，随后用响应的 `next_cursor` 作为下一页的 `snapshot_digest`，每页只向后定位并读取一个有界窗口。原偏移和筛选行为仍可用于兼容调用。

[project_migration.rs](../../../crates/adapters/src/project_migration.rs) 在独占锁下准备和验证新库后切换。离线归档把对象合并到不可变文件，SQLite 按摘要记录偏移与长度。归档时核对仍被使用的正文，然后清理散文件。新版项目备份同时保存 `payloads/`。

验证入口：[resource_commits.rs](../../../crates/adapters/tests/resource_commits.rs)、[materialization_receipts.rs](../../../crates/adapters/tests/materialization_receipts.rs)、[advisory_read_bounds.rs](../../../crates/adapters/tests/advisory_read_bounds.rs)、[workflow_storage_scaling.rs](../../../crates/adapters/tests/workflow_storage_scaling.rs)。
