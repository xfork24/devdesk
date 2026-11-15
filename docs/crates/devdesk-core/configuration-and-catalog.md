# 配置创作、共享目录、采用与跨库发布

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

“发布源”“在目录显示”“让某个工作采用”是独立操作。这里建立候选状态、检查完整依赖，再以确切指纹提交；跨库发布保留准备/采用回执，不声称两个 SQLite 库有共同事务。模板重置只切目录代次，旧运行继续保留原引用。

## 本页文件

- [src/configuration.rs](#file-src-configuration-rs)：配置可见性、组合与引用使用方查询
- [src/governance.rs](#file-src-governance-rs)：单项源命令、Agent 实例和工程配置冻结
- [src/configuration_changes.rs](#file-src-configuration-changes-rs)：同一候选图中的预演、诊断和原子提交
- [src/configuration_transfers.rs](#file-src-configuration-transfers-rs)：共享发布与项目采用的跨库恢复协议
- [src/templates.rs](#file-src-templates-rs)：外层资源注入与模板目录重置
- [src/builtin_defaults.rs](#file-src-builtin-defaults-rs)：从当前目录读取默认选择
- [src/catalog_sync.rs](#file-src-catalog-sync-rs)：共享源副本与经验目录索引
- [src/standards.rs](#file-src-standards-rs)：规范快照发布与每个工作的独立采用

<a id="file-src-configuration-rs"></a>

## src/configuration.rs：配置可见性、组合与引用使用方查询

源码：[crates/core/src/configuration.rs](../../../crates/core/src/configuration.rs)。

这里集中处理 Role 的业务身份/可见性、Agent 规范化和 Capability 验证，并为查询构建可用资源与使用关系。`reference_users`、`integration_reference_users` 和 `ensure_unreferenced` 用明确合同发现仍被采用的配置，避免“列表不显示”被误解为安全可停用。

Model Provider、Model Catalog 与 Harness 校验检查服务商、容量目录、执行安装及实际驱动组合。`project_configuration_references` 读取项目的确切选择；`retired` 将配置停用事实纳入后续选择。这里不安装镜像、不取得系统密钥，能解析的组合还要经过执行装配和授权。

代码定位：[`role_record`](../../../crates/core/src/configuration.rs#L5)、[`visible`](../../../crates/core/src/configuration.rs#L15)、[`validate_capability`](../../../crates/core/src/configuration.rs#L63)、[`reference_users`](../../../crates/core/src/configuration.rs#L172)、[`project_configuration_references`](../../../crates/core/src/configuration.rs#L265)、[`validate_harness`](../../../crates/core/src/configuration.rs#L329)。

关联阅读：[domain/src/source.rs](../devdesk-domain/configuration-and-sources.md#file-src-source-rs)、[core/src/configuration_changes.rs](configuration-and-catalog.md#file-src-configuration-changes-rs)、[core/src/governance.rs](configuration-and-catalog.md#file-src-governance-rs)、[core/src/driver_capabilities.rs](session-resources-and-permissions.md#file-src-driver-capabilities-rs)。

<a id="file-src-governance-rs"></a>

## src/governance.rs：单项源命令、Agent 实例和工程配置冻结

源码：[crates/core/src/governance.rs](../../../crates/core/src/governance.rs)。

这是源与治理记录的基础用例集合。`record/record_with_refs/update_record` 构造和更新不同性质的记录；`validate_source_payload` 按类型复用领域校验，检查平台处理器、Harness、权限、Delivery 等组合。`validate_composition` 检查 Agent 实际拥有的 Role/Model/Harness/Skill/Tool。

`governance_command` 分派 Role、Model、Harness、Skill、Capability、Agent 等定义操作、停用、实例创建及项目配置采用；它不会因为创建 Role 就顺便生成 Agent 或 Skill。`freeze_feature_configuration` 在开始工作时固定当时的确切依赖，后续默认修改不会替换它。

`governance_after_command` 保存需要的快照/经验和关联事实，`governance_query` 返回按可见性与当前目录过滤的视图。这里的 ConfigurationRecord 包含源与执行治理两类记录，落盘时 source_store 会分开处理，不能认为所有记录都在同一张源表中。

代码定位：[`validate_source_payload`](../../../crates/core/src/governance.rs#L4)、[`record_with_refs`](../../../crates/core/src/governance.rs#L155)、[`validate_composition`](../../../crates/core/src/governance.rs#L264)、[`governance_command`](../../../crates/core/src/governance.rs#L508)、[`freeze_feature_configuration`](../../../crates/core/src/governance.rs#L475)、[`governance_after_command`](../../../crates/core/src/governance.rs#L401)。

关联阅读：[core/src/configuration.rs](configuration-and-catalog.md#file-src-configuration-rs)、[core/src/configuration_changes.rs](configuration-and-catalog.md#file-src-configuration-changes-rs)、[core/src/catalog_sync.rs](configuration-and-catalog.md#file-src-catalog-sync-rs)、[adapters/src/source_store.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-source-store-rs)。

<a id="file-src-configuration-changes-rs"></a>

## src/configuration_changes.rs：同一候选图中的预演、诊断和原子提交

源码：[crates/core/src/configuration_changes.rs](../../../crates/core/src/configuration_changes.rs)。

`ConfigurationOperation` 包括 Create/Revise/Retire、实例管理、权限绑定、Skill 导入、项目和工作选择。操作可以先列消费者再列依赖：configuration_candidate 先建立候选，再对完整结果校验，而不是逐项提交产生半成品。

预演计算基准配置指纹、发布引用、选择和带 source_ref/path/target_ref 的诊断。修订保持类型/Scope/身份，检查预期 head 和记录 revision；停用检查当前消费者对所有相关修订的引用，不扫描正文猜依赖。实例变更使用稳定逻辑身份，后续应用不能生成另一批对象。

实际 apply 重新核对候选、Actor 和基准，以单库事务提交。预演中的时间不参与语义指纹；预演不运行 Skill、模型或外部工具。权限绑定可经注入后端做纯可行性检查。共享修改还需经理授权，不允许通过同一候选给当前会话自提权。

代码定位：[`ConfigurationOperation`](../../../crates/core/src/configuration_changes.rs#L6)、[`ConfigurationChangeSet`](../../../crates/core/src/configuration_changes.rs#L53)、[`configuration_fingerprint`](../../../crates/core/src/configuration_changes.rs#L78)、[`configuration_candidate`](../../../crates/core/src/configuration_changes.rs#L190)、[`configuration_preview_in`](../../../crates/core/src/configuration_changes.rs#L481)、[`configuration_change_command`](../../../crates/core/src/configuration_changes.rs#L510)。

关联阅读：[domain/src/configuration_graph.rs](../devdesk-domain/configuration-and-sources.md#file-src-configuration-graph-rs)、[core/src/configuration_transfers.rs](configuration-and-catalog.md#file-src-configuration-transfers-rs)、[core/src/manager_workspace.rs](manager-and-platform-tools.md#file-src-manager-workspace-rs)、[adapters/tests/configuration_changes.rs](../devdesk-adapters/testing.md#file-tests-configuration-changes-rs)。

<a id="file-src-configuration-transfers-rs"></a>

## src/configuration_transfers.rs：共享发布与项目采用的跨库恢复协议

源码：[crates/core/src/configuration_transfers.rs](../../../crates/core/src/configuration_transfers.rs)。

共享库先 stage 一个隐藏的候选，固定 global/project 变更集、两边基准指纹、目标项目和原 Actor。共享阶段仅允许发布/修订源，不夹带共享停用；候选尚未进入可用源目录。prepared/applying 操作对相关配置写入设置围栏。

apply 先持久化 applying，再在项目事务里复制所需源、采用选择并保存独立 receipt，最后提交共享源和 completed 结果。项目事务报错后先读取 durable receipt 判断是否实际已提交，不能直接假定没有效果。已完成调用重放原结果；prepared 可以取消，applying 由 recover 恢复。

这是一条可收敛协议，不是分布式原子事务。任一阶段都按同一操作 ID 和指纹核对；恢复仍使用原 Actor，不能把经理发起的共享修改改记为 Human。

代码定位：[`stage_configuration_transfer_as`](../../../crates/core/src/configuration_transfers.rs#L45)、[`apply_configuration_transfer`](../../../crates/core/src/configuration_transfers.rs#L99)、[`cancel_configuration_transfer`](../../../crates/core/src/configuration_transfers.rs#L188)、[`recover_configuration_transfers`](../../../crates/core/src/configuration_transfers.rs#L201)、[`configuration_transfer_guard`](../../../crates/core/src/configuration_transfers.rs#L23)。

关联阅读：[core/src/configuration_changes.rs](configuration-and-catalog.md#file-src-configuration-changes-rs)、[host/src/governance.rs](../devdesk-host/rpc-and-configuration.md#file-src-governance-rs)、[core/src/manager_workspace.rs](manager-and-platform-tools.md#file-src-manager-workspace-rs)、[adapters/tests/configuration_changes.rs](../devdesk-adapters/testing.md#file-tests-configuration-changes-rs)。

<a id="file-src-templates-rs"></a>

## src/templates.rs：外层资源注入与模板目录重置

源码：[crates/core/src/templates.rs](../../../crates/core/src/templates.rs)。

`ConfigurationBundle` 接收外层 loader 已读取的源、正文、默认映射与包身份。core 不从源码目录读取资源；`materialize` 将 bundle 校验为可用状态。空库初始化、共享复制、成员登记与统计都基于这份输入。

目录状态记录当前 generation 与默认映射，成员记录区分默认、修改、自建和归档来源。restore_template_catalog 复用不可变源并切换当前目录代次；旧项目采用、冻结运行与历史仍可按 ID 读取。`pinned_template_records` 用于解释哪些工作还固定旧目录。

单库恢复与跨工程目录同步分开：Host 先在共享配置 Engine 完成重置，项目后续经 catalog_sync 复制当前目录状态。切换选择列表不会把各项目已采用的配置自动换成新默认，也不要求一次写遍所有项目库。

全局 bundle 使用一次性安装，不能在同进程悄悄换成另一套产品资源。包内测试用外部构造的小 bundle 证明 core 不依赖 resources 文件树；adapter factory_bundle 才负责实际内嵌加载。

代码定位：[`ConfigurationBundle`](../../../crates/core/src/templates.rs#L12)、[`install_configuration_bundle`](../../../crates/core/src/templates.rs#L20)、[`materialize`](../../../crates/core/src/templates.rs#L45)、[`initialize_template_catalog`](../../../crates/core/src/templates.rs#L188)、[`restore_template_catalog`](../../../crates/core/src/templates.rs#L287)、[`current_template_defaults`](../../../crates/core/src/templates.rs#L425)。

关联阅读：[adapters/src/factory_bundle.rs](../devdesk-adapters/factory-and-configuration.md#file-src-factory-bundle-rs)、[core/src/default_permissions.rs](session-resources-and-permissions.md#file-src-default-permissions-rs)、[host/src/templates.rs](../devdesk-host/rpc-and-configuration.md#file-src-templates-rs)。

<a id="file-src-builtin-defaults-rs"></a>

## src/builtin_defaults.rs：从当前目录读取默认选择

源码：[crates/core/src/builtin_defaults.rs](../../../crates/core/src/builtin_defaults.rs)。

这里提供 `configuration_defaults` 查询，读取当前模板目录的默认引用与 Role→Agent 绑定，并考虑已经删除/停用的角色。它不给新项目隐式写入配置，也不根据 Role 名字生成执行器。

默认引用的具体值来自注入的 bundle/目录状态，代码只负责选择和展示当前可采用项。使用默认值创建工作后，真正冻结发生在项目/工作采用和流程开始用例中；以后默认变化不会改已有工作。

代码定位：[`role_deleted`](../../../crates/core/src/builtin_defaults.rs#L4)、[`configuration_defaults`](../../../crates/core/src/builtin_defaults.rs#L11)。

关联阅读：[core/src/templates.rs](configuration-and-catalog.md#file-src-templates-rs)、[core/src/governance.rs](configuration-and-catalog.md#file-src-governance-rs)。

<a id="file-src-catalog-sync-rs"></a>

## src/catalog_sync.rs：共享源副本与经验目录索引

源码：[crates/core/src/catalog_sync.rs](../../../crates/core/src/catalog_sync.rs)。

`sync_shared_configuration` 先把项目拥有的 Role/退役身份加入共享目录索引，再将对该项目可见的共享不可变源和当前目录信息追加到项目。已存在 ID 的内容摘要必须相同，不覆盖历史；目录和源都未变化时尽早返回。

复制源的目的是让项目冻结外键和重启读取自包含，不是让所有项目采用共享最新版。内存 Definition/Harness/Sandbox 视图随当前源重建和同步，其本身不构成第二份源存储。

`publish_experience_index` 把项目经验条目发布到全局治理目录，已有同 ID 条目须完全一致。这里复制的是可定位的经验索引；完整会话/文档和 Artifact 仍在原项目，由 experience 适配器只读加载。

代码定位：[`sync_shared_configuration`](../../../crates/core/src/catalog_sync.rs#L3)、[`publish_experience_index`](../../../crates/core/src/catalog_sync.rs#L101)。

关联阅读：[host/src/workspace.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-workspace-rs)、[adapters/src/experience.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-experience-rs)、[core/src/resource_receipts.rs](session-resources-and-permissions.md#file-src-resource-receipts-rs)、[adapters/src/source_store.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-source-store-rs)。

<a id="file-src-standards-rs"></a>

## src/standards.rs：规范快照发布与每个工作的独立采用

源码：[crates/core/src/standards.rs](../../../crates/core/src/standards.rs)。

`capture_standard` 调用领域规范解析，以精确根引用生成或复用不可变快照；归档只影响选择目录。standard_command/query 处理 capture、比较、使用关系和入口约束，展示的是按需源图，不保存另一份展开规范。

`work_configuration` 与 select_work_configuration 为尚未运行的 Project/Feature/Task 保存各自选择。Task 的采用不自动等同于 Feature 的采用。已运行工作从其冻结计划恢复 Standard，变更须走 AdoptStandard 和运行计划预演/应用。

`check_invocation_contract` 核对子工作入口和父调用端口兼容性；work_standard_selection、compare_work_standards、work_standard_choices 分别展示真实采用、差异和可访问选择。跨项目比较读取各自源上下文，不切换活动项目。

代码定位：[`capture_standard`](../../../crates/core/src/standards.rs#L27)、[`select_work_configuration`](../../../crates/core/src/standards.rs#L85)、[`check_invocation_contract`](../../../crates/core/src/standards.rs#L447)、[`work_standard_selection`](../../../crates/core/src/standards.rs#L518)、[`compare_work_standards`](../../../crates/core/src/standards.rs#L577)、[`work_standard_choices`](../../../crates/core/src/standards.rs#L607)。

关联阅读：[domain/src/standards.rs](../devdesk-domain/configuration-and-sources.md#file-src-standards-rs)、[core/src/configured_workflows.rs](workflow-compilation.md#file-src-configured-workflows-rs)、[core/src/workflow_changes.rs](workflow-changes.md#file-src-workflow-changes-rs)、[adapters/tests/standard_snapshots.rs](../devdesk-adapters/testing.md#file-tests-standard-snapshots-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
