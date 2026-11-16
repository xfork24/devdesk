# SQLite、Artifact 与源视图恢复

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

工程状态和全局目录分库保存。配置源以不可变定义存储；运行视图可以压缩为源引用，但正文、参数、输入和输出等事实必须可无损恢复。文件交付物另有内容摘要与路径边界。

## 本页文件

- [src/lib.rs](#file-src-lib-rs)：工程数据库、封存文件和生产端口装配
- [src/catalog.rs](#file-src-catalog-rs)：全局配置、项目目录与运行设置
- [src/source_store.rs](#file-src-source-store-rs)：不可变源与运行治理记录分开落盘
- [src/source_views.rs](#file-src-source-views-rs)：无损压缩冻结视图，保留原始事实
- [src/experience.rs](#file-src-experience-rs)：按 Agent 身份只读汇集跨项目经验

<a id="file-src-lib-rs"></a>

## src/lib.rs：工程数据库、封存文件和生产端口装配

源码：[crates/adapters/src/lib.rs](../../../crates/adapters/src/lib.rs)。

`SqliteStore` 打开工程目录，核对 `devdesk-profile-runtime-v2` 数据标记、数据库版本和独占锁；全局库与工程库使用各自 SQL 基线，当前版本分别为 18 和 48。非空的不兼容目录不会自动迁移或清空。只读工程快照用于跨项目查询，不取得执行控制权。

EngineeringStore 的 transact 先取得 writer 锁，核对 command_id 与请求指纹并载入状态；运行业务闭包前释放数据库连接锁。闭包成功且状态发生变化时校验差异，再打开短 SQLite 事务提交状态和操作回执；这期间 writer 锁仍在，其他只读查询可取得连接。重放已提交操作恢复原结果，不再次执行闭包。`has_pending_work` 用数据库投影判断是否值得调度，避免每次 tick 为闲置工程加载全部历史。

`FileArtifacts` 在受管目录写临时文件，刷新后原子发布，返回包含随机 ID、relative_path、SHA-256 和 owner 字段的 `Artifact`。ID 不是内容摘要；读取校验安全相对路径、符号链接、Artifact 根边界和实际 SHA。读取者是否有权取得该 Artifact，由 core/Host 查询边界核对，文件适配器不单凭 owner 字段完成授权。数据库事实提交与文件发布也不是一个原子事务，业务接纳还需 core 核对来源并保存引用。

`execution_adapters` 集中装配 RoutingHarness、CliGit、CommandTests 和 GitHubPullRequests。创建适配器不立即运行模型或 GitHub；实际调用由后续 durable 意图触发。

代码定位：[`SqliteStore`](../../../crates/adapters/src/lib.rs#L51)、[`FileArtifacts`](../../../crates/adapters/src/lib.rs#L620)、[`open_project`](../../../crates/adapters/src/lib.rs#L85)、[`transact`](../../../crates/adapters/src/lib.rs#L537)、[`seal_scoped`](../../../crates/adapters/src/lib.rs#L649)、[`execution_adapters`](../../../crates/adapters/src/lib.rs#L745)。

关联阅读：[adapters/src/source_store.rs](storage-and-source-codecs.md#file-src-source-store-rs)、[adapters/src/catalog.rs](storage-and-source-codecs.md#file-src-catalog-rs)、[core/src/lib.rs](../devdesk-core/engine-and-ports.md#file-src-lib-rs)、[adapters/tests/workflows.rs](testing.md#file-tests-workflows-rs)。

<a id="file-src-catalog-rs"></a>

## src/catalog.rs：全局配置、项目目录与运行设置

源码：[crates/adapters/src/catalog.rs](../../../crates/adapters/src/catalog.rs)。

`GlobalStore` 管理 global.sqlite3、独占锁和共享配置状态。工程目录的 reserve 与 publish 分开：先固定稳定项目 ID，再完成工程初始化并公开，失败或重放不会生成另一项目身份。激活当前工程是目录设置，不会转移历史或执行者。

这里持久化模型凭据引用、运行安装、协议报告和并发设置；并发默认 4，合法范围 1..32。设置更新仍通过明确校验和事务提交。GlobalStore 也实现 EngineeringStore，以复用 core 的配置用例，但全局配置库不冒充某个业务工程运行库。

项目列表只提供路径与目录事实。实际打开工程、恢复操作、调度多个项目由 Host 负责；跨项目读取则使用只读快照。

代码定位：[`GlobalStore`](../../../crates/adapters/src/catalog.rs#L5)、[`open`](../../../crates/adapters/src/catalog.rs#L32)、[`concurrency`](../../../crates/adapters/src/catalog.rs#L67)、[`set_concurrency`](../../../crates/adapters/src/catalog.rs#L74)。

关联阅读：[host/src/workspace.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-workspace-rs)、[adapters/src/configuration_backend.rs](factory-and-configuration.md#file-src-configuration-backend-rs)、[adapters/tests/runtime_registry.rs](testing.md#file-tests-runtime-registry-rs)。

<a id="file-src-source-store-rs"></a>

## src/source_store.rs：不可变源与运行治理记录分开落盘

源码：[crates/adapters/src/source_store.rs](../../../crates/adapters/src/source_store.rs)。

source_definitions 保存配置源，governance_records 保存实例、回执等运行事实。加载时源定义恢复成 core 使用的记录形状，并按确切修订重建 supersedes；Skill 正文来自源本身，不从当前资源目录补写旧版本。

保存只允许新增合法的 identity:revision 源，已有源内容不能被更新覆盖。非源记录交给 source_views 压缩，正式任务必须以 `$source:task` 形状保存。恢复使用 memo 避免重复展开同一闭包。

这层 codec 不是另一套配置编译器：源类型和依赖仍由 domain/core 校验。增加配置类型时，应同时检查源分类、序列化形状和恢复路径，避免写入展开配置副本。

代码定位：[`is_source`](../../../crates/adapters/src/source_store.rs#L4)、[`load_records`](../../../crates/adapters/src/source_store.rs#L14)、[`save_records`](../../../crates/adapters/src/source_store.rs#L80)、[`pack_record`](../../../crates/adapters/src/source_store.rs#L114)、[`restore_records`](../../../crates/adapters/src/source_store.rs#L133)。

关联阅读：[adapters/src/source_views.rs](storage-and-source-codecs.md#file-src-source-views-rs)、[domain/src/source.rs](../devdesk-domain/configuration-and-sources.md#file-src-source-rs)、[core/src/frozen_views.rs](../devdesk-core/workflow-compilation.md#file-src-frozen-views-rs)。

<a id="file-src-source-views-rs"></a>

## src/source_views.rs：无损压缩冻结视图，保留原始事实

源码：[crates/adapters/src/source_views.rs](../../../crates/adapters/src/source_views.rs)。

`pack/restore` 将可重建的配置视图变成确切 `$source` 引用；只有展开后与原值相等才采用压缩。inputs、outputs、arguments、content、value、error、input_sources 等事实字段不会被当作普通源投影替换。用户字面量中的 `$source` 与内部文本标记也有转义，防止把正文误当指令。

正式任务按结构记录来源选择和恢复参数，而不是凭 renderer 字符串猜测。恢复优先查 ConversationEvent 的 User 消息账本，将任务的 source、inputs、input_sources、Agent、补充文本及摘要与 configured_execution_task/archived 记录逐项核验；匹配时直接返回当时实际发送的 JSON，不先用当前渲染器重写。找不到这份已认证正文时，才按原 plan_revision 调用 core.frozen_views.restore_task 纯重建；这里不从原生协议 journal 提取替代正文。

同一次工程快照加载中，完全相同的执行权限选择和作用域只编译一次。复用键包含 Harness/Sandbox 的确切选择及完整 ExecutionScope，不同路径或 WorkType 不共用结果；下一次加载前清除此类条目，重新核对源、摘要与执行后端资产。条目受现有 memo 的 128 项限制，键和值的 JSON 总长度超过 64 KiB 时不保留，不新增权限或持久化展开副本。

本文件的 runtime_digest 是另一项共用计算：对实际安装、adapter、完整 Agent/模型连接、权限、路径、WorkType 和凭据引用等 RuntimeBinding 选择计算摘要，供恢复和权限后继使用，不是任务正文摘要或 Artifact ID。

包内测试覆盖伪造标记、嵌套恢复、文本转义及历史实际正文。修改任务渲染器时，必须同时考虑这里的往返相等和旧会话重放，而不只是新任务是否好看。

代码定位：[`pack`](../../../crates/adapters/src/source_views.rs#L20)、[`restore`](../../../crates/adapters/src/source_views.rs#L257)、[`restore_reference`](../../../crates/adapters/src/source_views.rs#L316)、[`factual_field`](../../../crates/adapters/src/source_views.rs#L6)。

关联阅读：[core/src/frozen_views.rs](../devdesk-core/workflow-compilation.md#file-src-frozen-views-rs)、[domain/src/runtime.rs](../devdesk-domain/runtime-and-permissions.md#file-src-runtime-rs)、[adapters/tests/configuration_resources.rs](testing.md#file-tests-configuration-resources-rs)。

<a id="file-src-experience-rs"></a>

## src/experience.rs：按 Agent 身份只读汇集跨项目经验

源码：[crates/adapters/src/experience.rs](../../../crates/adapters/src/experience.rs)。

此模块从全局目录找到项目，再打开只读工程快照查询 Agent 实例、会话与经验索引。全局 Agent 可在允许的工程范围查询，项目 Agent 的范围受其项目限制；不可用项目作为事实返回，不替用户切换当前工程。

列表去重、过滤、分页；详情再次匹配 Agent 身份、来源项目和快照引用，并经 FileArtifacts 读取封存正文和校验摘要。结果不是可任意读取宿主路径的接口。

经验导入需要创建当前上下文的明确资源回执，应用用例在 core；这里负责来源读取，不把远端项目记录直接复制为当前工程成功。

代码定位：[`query`](../../../crates/adapters/src/experience.rs#L47)、[`global_records`](../../../crates/adapters/src/experience.rs#L5)、[`projects`](../../../crates/adapters/src/experience.rs#L16)、[`instance`](../../../crates/adapters/src/experience.rs#L31)。

关联阅读：[core/src/resource_receipts.rs](../devdesk-core/session-resources-and-permissions.md#file-src-resource-receipts-rs)、[host/src/governance.rs](../devdesk-host/rpc-and-configuration.md#file-src-governance-rs)、[adapters/tests/governance.rs](testing.md#file-tests-governance-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

工作流运行提示及占用计数位于 `runtime_queries.rs`，通过共享只读池查询实际 Turn/执行和待交付记录，不恢复会话或源闭包。未知结果但尚未撤权的执行仍占用额度；提示只决定是否需要轮询，不授予派发或接纳权限。
