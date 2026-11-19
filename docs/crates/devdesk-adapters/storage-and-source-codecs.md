# SQLite、资源提交与产物文件

存储层实现 core 的 EngineeringStore 和 ArtifactStore。它把明确的读取/修改请求转成 SQL 或文件操作，不自行决定哪一步应该完成。

## 打开与分库

[lib.rs](../../../crates/adapters/src/lib.rs) 的 SqliteStore 提供项目和全局讨论工程库；[catalog.rs](../../../crates/adapters/src/catalog.rs) 的 GlobalStore 管理全局配置库。打开前检查数据集锁、标记、布局及 user_version，拒绝不兼容数据。

[connection_pool.rs](../../../crates/adapters/src/connection_pool.rs) 管理连接，[resource_read_scope.rs](../../../crates/adapters/src/resource_read_scope.rs) 支持读取范围。查询从已提交视图取数据，耗时外部操作不持有 SQL 事务。

## 一次提交经过什么

1. core 提交明确的资源版本、变更、事件及命令身份。
2. [resource_commits.rs](../../../crates/adapters/src/resource_commits.rs) 将资源类型映射到允许的表或查询，核对原版本。
3. 短事务内写入结果及相关版本、事件和回执；[store_changes.rs](../../../crates/adapters/src/store_changes.rs) 通知读取者。
4. 重放原命令返回原结果；冲突交给调用方处理，存储不重新生成业务决策。

[store_changes.rs](../../../crates/adapters/src/store_changes.rs) 的通知只提示变化，不能替代事务提交。跨库目录摘要和经验索引允许后台修复，原业务提交仍以所属库为准。

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

验证入口：[resource_commits.rs](../../../crates/adapters/tests/resource_commits.rs)、[materialization_receipts.rs](../../../crates/adapters/tests/materialization_receipts.rs)、[advisory_read_bounds.rs](../../../crates/adapters/tests/advisory_read_bounds.rs)、[workflow_storage_scaling.rs](../../../crates/adapters/tests/workflow_storage_scaling.rs)。
