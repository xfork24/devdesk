# 完整会话绑定、权限修订与执行安装回执

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

会话必须先解析完整 Agent，再确定实际安装、模型连接、凭据引用、权限锁与工具授权。修改默认权限不会原地修改旧 Session；后继会话需要真实静止、停止与快照边界。安装、授权编译和系统凭据由 SessionResources 端口提供，选择与回执由 core 保存。

## 本页文件

- [src/default_permissions.rs](#file-src-default-permissions-rs)：给当前出厂目标补齐默认绑定
- [src/permission_resources.rs](#file-src-permission-resources-rs)：Profile 修订、导入导出与绑定 CAS
- [src/permission_switch.rs](#file-src-permission-switch-rs)：以可恢复后继替换会话权限
- [src/session_assembly.rs](#file-src-session-assembly-rs)：把完整 Agent 装配成可执行 Session
- [src/resource_receipts.rs](#file-src-resource-receipts-rs)：应用侧掌握经验导入和 Harness 物化事实
- [src/driver_capabilities.rs](#file-src-driver-capabilities-rs)：实际驱动能力的进程级事实

<a id="file-src-default-permissions-rs"></a>

## src/default_permissions.rs：给当前出厂目标补齐默认绑定

源码：[crates/core/src/default_permissions.rs](../../../crates/core/src/default_permissions.rs)。

`template_permission_defaults` 从当前目录的默认映射和出厂成员解析所有需要的 Harness，以及默认 Sandbox，只为尚无活动 assignment 的目标创建初始绑定。执行合同由外层传入的纯函数提供，core 不读取后端文件或猜测支持组合。

已有绑定不会被默认值覆盖；ensure_default_permissions 在存储事务内应用。reset_factory_catalog 把目录重置与新目录所需默认绑定放在同一事务，核对 generation、原请求指纹和 pristine 状态，避免目录切完却缺必要权限。

代码定位：[`template_permission_defaults`](../../../crates/core/src/default_permissions.rs#L2)、[`ensure_default_permissions`](../../../crates/core/src/default_permissions.rs#L71)、[`reset_factory_catalog`](../../../crates/core/src/default_permissions.rs#L108)。

关联阅读：[core/src/templates.rs](configuration-and-catalog.md#file-src-templates-rs)、[core/src/permission_resources.rs](session-resources-and-permissions.md#file-src-permission-resources-rs)、[adapters/src/permission_profiles.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-permission-profiles-rs)。

<a id="file-src-permission-resources-rs"></a>

## src/permission_resources.rs：Profile 修订、导入导出与绑定 CAS

源码：[crates/core/src/permission_resources.rs](../../../crates/core/src/permission_resources.rs)。

`insert_permission_profiles` 先解析/规范化整批 Profile，限制批次大小，按修订顺序检查连续前序、Scope、目标类型与已存在内容；相同引用内容不同则冲突，不原地覆盖。导入记录来源，export 带所选修订的连续前序，支持新库重建完整链。

permission_resource_command 处理 Profile 的创建/修订等应用命令，`apply_permission_bindings` 检查目标完整内容身份、权限修订、enforcement_contract 和 expected_revision，形成新 binding/assignment。批次同一状态事务提交，重放先于 CAS；它不启动 worker，也不改变旧执行锁。实际策略是否能落实由注入后端预检。

代码定位：[`insert_permission_profiles`](../../../crates/core/src/permission_resources.rs#L5)、[`permission_profile_export`](../../../crates/core/src/permission_resources.rs#L137)、[`permission_resource_command`](../../../crates/core/src/permission_resources.rs#L156)、[`apply_permission_bindings`](../../../crates/core/src/permission_resources.rs#L239)。

关联阅读：[domain/src/permission_profile.rs](../devdesk-domain/runtime-and-permissions.md#file-src-permission-profile-rs)、[adapters/src/permission_profiles.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-permission-profiles-rs)、[host/src/permission_resources.rs](../devdesk-host/rpc-and-configuration.md#file-src-permission-resources-rs)。

<a id="file-src-permission-switch-rs"></a>

## src/permission_switch.rs：以可恢复后继替换会话权限

源码：[crates/core/src/permission_switch.rs](../../../crates/core/src/permission_switch.rs)。

`send_with_permission_binding` 先按原消息/命令指纹重放，再创建稳定 permission_switch 意图。它只允许同一 Agent/运行安装下明确可切换的权限变化，拒绝活动/排队输入、已关闭或待核对会话，以及另一条尚未完成切换。

接着逐执行核对并停止旧 worker、导出真实日志/停止证明，保存上下文快照，创建 force_new 后继 Session 并迁移正式任务/Controller 关联，最后 replay_successor_input 按原 command_id/client_message_id 消费一次。中断保留 durable intent，重试核对同一原绑定和 operation，不把输入重新解释为新任务。

权限改绑通过后继 Session 生效，旧 RuntimeBinding 不原地修改。允许的差异仅为 permissions 与 configuration_digest，Agent、安装、模型和接入锁等其他字段必须一致；这不是任意配置升级入口。普通 Task 讨论不允许走此切换，已有 configured_session 关联的正式任务则可在静止边界迁移到后继，Controller 关联也有专门迁移规则。活动或结果未知时仍拒绝切换，不能在未知边界直接提权。

代码定位：[`permission_switch_id`](../../../crates/core/src/permission_switch.rs#L12)、[`send_with_permission_binding`](../../../crates/core/src/permission_switch.rs#L70)、[`replay_successor_input`](../../../crates/core/src/permission_switch.rs#L638)。

关联阅读：[core/src/session_assembly.rs](session-resources-and-permissions.md#file-src-session-assembly-rs)、[core/src/formal_sessions.rs](workflow-compilation.md#file-src-agent-tasks-rs)、[host/src/permission_resources.rs](../devdesk-host/rpc-and-configuration.md#file-src-permission-resources-rs)、[adapters/tests/permission_profiles.rs](../devdesk-adapters/testing.md#file-tests-permission-profiles-rs)。

<a id="file-src-session-assembly-rs"></a>

## src/session_assembly.rs：把完整 Agent 装配成可执行 Session

源码：[crates/core/src/session_assembly.rs](../../../crates/core/src/session_assembly.rs)。

`resolve_session_composition` 从 Agent 实例或精确 Agent 源取 Role/Capability，检查可见性、停用和正式冻结例外，拒绝调用者单独覆盖 Role、Model、Harness、安装或权限。没有现成工作身份时通过明确命令建立 Agent 实例，而不是用角色名猜默认组合。

随后解析模型/服务商连接，物化确切 Harness，冻结能力、工具/接入锁、安装、参数和凭据引用，按实际 WorkType 与 Scope 编译/验证权限。普通自由讨论会清除需要 Workflow 归属的平台工具。RuntimeBinding 由协调器构造，客户端不能直接提交 binding。

`open_prepared_session` 在环境准备前检查原请求重放，保存原参数指纹，再通过允许的 resolved 入口提交装配结果。外部资源访问经 SessionResources，已存在物化/冻结选择不能被同版本标签的新镜像替换。

代码定位：[`resolve_session_composition`](../../../crates/core/src/session_assembly.rs#L83)、[`open_prepared_session`](../../../crates/core/src/session_assembly.rs#L255)。

关联阅读：[core/src/resource_receipts.rs](session-resources-and-permissions.md#file-src-resource-receipts-rs)、[core/src/ports.rs](engine-and-ports.md#file-src-ports-rs)、[adapters/src/session_resources.rs](../devdesk-adapters/runtime-preparation.md#file-src-session-resources-rs)、[core/src/conversations.rs](conversations-and-history.md#file-src-conversations-rs)。

<a id="file-src-resource-receipts-rs"></a>

## src/resource_receipts.rs：应用侧掌握经验导入和 Harness 物化事实

源码：[crates/core/src/resource_receipts.rs](../../../crates/core/src/resource_receipts.rs)。

`import_experience` 把外层只读查到的快照转为当前范围的新 context：使用原 command_id 派生身份，清除原文档所有权引用，保留来源和背景，不改当前代码、治理配置或文档 head。导入与后续 session.open 各有独立幂等键。

`materialize_harness` 从确切 Harness 和选定 Sandbox 组合内部配方，以 Harness/Sandbox、配方摘要、Linux 和架构定位回执。已有回执核对实际安装；首次准备核对驱动与配方后保存不可替换的物化事实。重试与恢复使用原锁，不读取新默认替换历史依据。

因此安装操作在适配器，采用与物化身份由 core 裁定。相同源若已对应另一安装不能偷偷替换，需要明确的后继配置或核对。

代码定位：[`import_experience`](../../../crates/core/src/resource_receipts.rs#L26)、[`record_harness_materialization`](../../../crates/core/src/resource_receipts.rs#L125)、[`materialize_harness`](../../../crates/core/src/resource_receipts.rs#L189)。

关联阅读：[adapters/src/session_resources.rs](../devdesk-adapters/runtime-preparation.md#file-src-session-resources-rs)、[domain/src/execution_recipe.rs](../devdesk-domain/runtime-and-permissions.md#file-src-execution-recipe-rs)、[adapters/src/experience.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-experience-rs)、[core/src/session_assembly.rs](session-resources-and-permissions.md#file-src-session-assembly-rs)。

<a id="file-src-driver-capabilities-rs"></a>

## src/driver_capabilities.rs：实际驱动能力的进程级事实

源码：[crates/core/src/driver_capabilities.rs](../../../crates/core/src/driver_capabilities.rs)。

DriverCapabilities 把真实 implementation_id 与支持的功能集合关联起来，由外层安装进 OnceLock。重复安装完全相同的表可收敛，换一份内容会被拒绝；未安装或未知 feature 查询返回不支持。

这份表来自已编译驱动，不来自用户的 Harness capabilities 声明。编译、会话和经理用例通过 driver_supports 检查动态平台/编排桥，防止发布一个标记就冒称底层协议支持。它不探测模型账户，也不证明推理或工具执行成功。

代码定位：[`DriverCapabilities`](../../../crates/core/src/driver_capabilities.rs#L8)、[`install_driver_capabilities`](../../../crates/core/src/driver_capabilities.rs#L13)、[`driver_supports`](../../../crates/core/src/driver_capabilities.rs#L27)、[`driver_capabilities`](../../../crates/core/src/driver_capabilities.rs#L33)。

关联阅读：[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)、[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/configured_workflows.rs](workflow-compilation.md#file-src-configured-workflows-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
