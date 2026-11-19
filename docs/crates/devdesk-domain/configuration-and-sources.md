# 源修订、配置依赖与规范快照

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

配置由不可变源修订组成。身份与修订回答“用的是哪一份”，Scope 回答“在哪些工作中可见”，有类型依赖边回答“它依赖谁”。Standard 固定规范关系，模板目录只决定当前选择列表；二者都不改写已有执行。实际发布和采用见 [core 配置变更](../devdesk-core/configuration-and-catalog.md#file-src-configuration-changes-rs)。

## 本页文件

- [src/configuration.rs](#file-src-configuration-rs)：Role 定义的额外可见范围
- [src/governance.rs](#file-src-governance-rs)：源记录信封、Scope 与执行组合类型
- [src/source.rs](#file-src-source-rs)：规范源引用与纯解析入口
- [src/configuration_graph.rs](#file-src-configuration-graph-rs)：只从有类型字段提取依赖
- [src/template_catalog.rs](#file-src-template-catalog-rs)：目录代次与不可变历史分离
- [src/standards.rs](#file-src-standards-rs)：规范闭包、稳定摘要与结构差异

<a id="file-src-configuration-rs"></a>

## src/configuration.rs：Role 定义的额外可见范围

源码：[crates/domain/src/configuration.rs](../../../crates/domain/src/configuration.rs)。

这里的 `DefinitionVisibility` 表达一个定义对全部项目可见，或仅对明确的项目列表可见。Projects 列表须非空、最多 1000 项、身份不重复且不为空；`permits` 在给定项目上下文判断可见性。

它与源封套的 Scope 配合，而不是代替执行权限：能够看见 Role 不表示可调用工具或写目录。此处保持纯数据判断，实际配置查询和引用接纳由 core 组合这些条件。

代码定位：[`DefinitionVisibility`](../../../crates/domain/src/configuration.rs#L6)、[`validate`](../../../crates/domain/src/configuration.rs#L14)、[`permits`](../../../crates/domain/src/configuration.rs#L28)。

关联阅读：[domain/src/governance.rs](configuration-and-sources.md#file-src-governance-rs)、[core/src/configuration.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-rs)。

<a id="file-src-governance-rs"></a>

## src/governance.rs：源记录信封、Scope 与执行组合类型

源码：[crates/domain/src/governance.rs](../../../crates/domain/src/governance.rs)。

`ConfigurationRecord` 把稳定身份、源修订、名称、Scope、内容摘要、根级 refs 和 data 放在同一封套中。这里的 `version` 是源修订，`revision` 用于记录状态并发控制，不能互换。`decode` 只解码 data；对外扁平引用与存储 refs 列通过 `to_storage/from_storage` 转换。

`Scope` 严格解析 global、project 和 feature 定位符；全局可向下可见，项目源仅在本项目及其 Feature 可见，Feature 源不能跨 Feature。`reference_fields` 给每种源列出允许的根级引用，`content_digest` 同时覆盖引用与载荷。

文件还定义 Model、Harness、Tool、Role、Agent 实例和解析后 Agent 执行配置。`ToolDefinition.call_name` 将平台 qualified_name 转成可移植函数名，处理器身份保持原值。聚合状态的治理校验检查源与记录关系；它并不实际安装运行时或执行模型。

代码定位：[`Scope`](../../../crates/domain/src/governance.rs#L9)、[`ConfigurationRecord`](../../../crates/domain/src/governance.rs#L134)、[`reference_fields`](../../../crates/domain/src/governance.rs#L92)、[`content_digest`](../../../crates/domain/src/governance.rs#L107)、[`AgentExecutionConfiguration`](../../../crates/domain/src/governance.rs#L305)、[`ToolDefinition`](../../../crates/domain/src/governance.rs#L244)。

关联阅读：[domain/src/source.rs](configuration-and-sources.md#file-src-source-rs)、[domain/src/configuration_graph.rs](configuration-and-sources.md#file-src-configuration-graph-rs)、[core/src/governance.rs](../devdesk-core/configuration-and-catalog.md#file-src-governance-rs)、[adapters/src/source_store.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-source-store-rs)。

<a id="file-src-source-rs"></a>

## src/source.rs：规范源引用与纯解析入口

源码：[crates/domain/src/source.rs](../../../crates/domain/src/source.rs)。

`SourceDefinition` 是文件/导入使用的扁平源形状。`source_reference` 与 `parse_source_reference` 约束规范的 `identity:revision`：修订必须是正整数，不能用别名或非规范表示指向同一版本。`into_record` 先校验名称、类型载荷与根级引用，再形成不可变记录和内容摘要。

`validate_source_data` 按类型检查源：Agent 只持 Role/Capability；Profile 保存权限规则；Workflow 步骤可直接引用 Profile，access 仅描述工作空间输入与工作目录。resolve_workflow_source 在内存中按 Profile 推导 StepAccess，不生成另一套可编辑权限源。

解析不会选择“最新同名配置”，也不执行外部操作。`refresh_configuration_views` 重建派生的内存视图，支持存储只保留原源引用。包内测试核对扁平引用、摘要和 Scope 编码。

代码定位：[`SourceDefinition`](../../../crates/domain/src/source.rs#L5)、[`source_reference`](../../../crates/domain/src/source.rs#L16)、[`parse_source_reference`](../../../crates/domain/src/source.rs#L28)、[`validate_source_data`](../../../crates/domain/src/source.rs#L115)、[`resolve_agent`](../../../crates/domain/src/source.rs#L216)、[`resolve_workflow_source`](../../../crates/domain/src/source.rs#L313)。

关联阅读：[domain/src/governance.rs](configuration-and-sources.md#file-src-governance-rs)、[domain/src/permission_profile.rs](runtime-and-permissions.md#file-src-permission-profile-rs)、[core/src/configured_workflows.rs](../devdesk-core/workflow-compilation.md#file-src-configured-workflows-rs)、[core/src/frozen_views.rs](../devdesk-core/workflow-compilation.md#file-src-frozen-views-rs)。

<a id="file-src-configuration-graph-rs"></a>

## src/configuration_graph.rs：只从有类型字段提取依赖

源码：[crates/domain/src/configuration_graph.rs](../../../crates/domain/src/configuration_graph.rs)。

`ConfigurationEdge` 保存出发源、字段路径、关系、期待类型和目标。`configuration_edges` 读取根级引用，还识别 Workflow 的角色/工具/子流程/端口、权限绑定、目录、项目和工作选择等有明确合同的嵌套字段。

自由正文、例子和 constant 输入不是引用。Workflow 子流程、步骤 Profile 与 Profile 的精确工具引用都有明确类型及字段路径；Harness 安装输入与 Sandbox 构建数据不形成另一种资源源。依赖图提取不等于可运行校验，递归与技术组合仍由编译器检查。

代码定位：[`ConfigurationEdge`](../../../crates/domain/src/configuration_graph.rs#L5)、[`configuration_edges`](../../../crates/domain/src/configuration_graph.rs#L12)。

关联阅读：[core/src/configuration_changes.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-changes-rs)、[domain/src/standards.rs](configuration-and-sources.md#file-src-standards-rs)、[domain/src/execution_recipe.rs](runtime-and-permissions.md#file-src-execution-recipe-rs)。

<a id="file-src-template-catalog-rs"></a>

## src/template_catalog.rs：目录代次与不可变历史分离

源码：[crates/domain/src/template_catalog.rs](../../../crates/domain/src/template_catalog.rs)。

模板目录回答“当前编辑和选择列表显示哪些修订”，不负责删除源。状态记录保存 generation，成员记录把源修订归入某个代次；`template_record_current` 判断当前成员，同时让 Skill 包、发现结果等从属于它们的来源。

Workflow 草稿按 workflow 成员归类。`template_projection_current` 为内存旧式定义视图提供相同目录过滤，`template_source_hash` 比较名称、Scope、引用和载荷，显示信息变化也会影响重置预演。停用事实另由 `template_record_disabled` 判断，归档、停用和删除历史不是同一个操作。

代码定位：[`template_catalog_generation`](../../../crates/domain/src/template_catalog.rs#L10)、[`template_record_current`](../../../crates/domain/src/template_catalog.rs#L58)、[`template_projection_current`](../../../crates/domain/src/template_catalog.rs#L80)、[`template_source_hash`](../../../crates/domain/src/template_catalog.rs#L97)。

关联阅读：[core/src/templates.rs](../devdesk-core/configuration-and-catalog.md#file-src-templates-rs)、[host/src/templates.rs](../devdesk-host/rpc-and-configuration.md#file-src-templates-rs)。

<a id="file-src-standards-rs"></a>

## src/standards.rs：规范闭包、稳定摘要与结构差异

源码：[crates/domain/src/standards.rs](../../../crates/domain/src/standards.rs)。

Standard 的根是精确 Workflow 源引用。解析器沿规范相关关系收集 Workflow、Delivery、Spec 及相应角色/方法需求等节点，形成 `StandardSnapshot`；运行会话、执行结果和模型连接选择不能被当成规范本身。`WorkReference` 统一 Project、Feature、Task 的工作定位与可见范围。

`canonical_standard_content` 仅对本来无顺序语义的集合字段排序，保留步骤和检查项顺序。快照摘要包含节点与边，所以相同文本但关系改变也会反映在 diff。`standard_entry` 检查某 Workflow 是否属于可用入口；差异比较沿稳定 node_id 对齐步骤，同时拒绝把歧义复制的节点身份强行合并。

本文件只计算规范视图和差异；实际 capture、工作采用、运行中切换由 core 决定。历史计划仍能从原引用恢复旧规范。

代码定位：[`StandardSnapshot`](../../../crates/domain/src/standards.rs#L79)、[`resolve_standard_sources`](../../../crates/domain/src/standards.rs#L114)、[`canonical_standard_content`](../../../crates/domain/src/standards.rs#L88)、[`standard_entry`](../../../crates/domain/src/standards.rs#L340)、[`diff_standard_snapshots`](../../../crates/domain/src/standards.rs#L383)。

关联阅读：[core/src/standards.rs](../devdesk-core/configuration-and-catalog.md#file-src-standards-rs)、[core/src/workflow_change_commit.rs](../devdesk-core/workflow-changes.md#file-src-workflow-changes-rs)、[adapters/tests/standard_snapshots.rs](../devdesk-adapters/testing.md#file-tests-standard-snapshots-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
