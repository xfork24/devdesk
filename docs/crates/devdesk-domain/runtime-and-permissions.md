# 执行安装、模型连接与权限

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

执行安装说明准备什么，安装记录说明准备出了什么，会话绑定说明本次选择什么，真实执行记录说明实际运行了什么。权限另分底层策略、目标绑定、步骤范围和后端执行计划。这里负责纯解析和校验；构建、凭据读取及原生隔离由 adapters 完成。

## 本页文件

- [src/runtime.rs](#file-src-runtime-rs)：驱动、安装产物和完整性结果
- [src/execution_recipe.rs](#file-src-execution-recipe-rs)：配置决定运行配方，摘要决定实际来源
- [src/model_provider.rs](#file-src-model-provider-rs)：服务商事实、容量及冻结连接的纯规则
- [src/permission.rs](#file-src-permission-rs)：可移植策略与实际执行计划
- [src/permission_profile.rs](#file-src-permission-profile-rs)：Profile、目标绑定和双端权限锁

<a id="file-src-runtime-rs"></a>

## src/runtime.rs：驱动、安装产物和完整性结果

源码：[crates/domain/src/runtime.rs](../../../crates/domain/src/runtime.rs)。

`HarnessProvider` 当前包含 Codex 与 Dsh，统一注册表供能力门禁使用。`RuntimeKey` 的 version、os、arch 是选择和展示信息，格式和支持平台会校验；真正的内容身份在产物摘要和 recipe_digest。

`RuntimeArtifact` 区分本机可执行文件、不可变 OCI 引用和本地 Docker image ID。RuntimeInstallation 绑定来源、许可证及可选配方；RuntimeIntegrity 只表示内容可核验，并明确不能证明推理成功。注册文件成功、凭据可读、协议初始化成功和真实会话成功应分别解释。

代码定位：[`HarnessProvider`](../../../crates/domain/src/runtime.rs#L6)、[`RuntimeKey`](../../../crates/domain/src/runtime.rs#L28)、[`RuntimeArtifact`](../../../crates/domain/src/runtime.rs#L61)、[`RuntimeInstallation`](../../../crates/domain/src/runtime.rs#L74)、[`RuntimeIntegrity`](../../../crates/domain/src/runtime.rs#L93)。

关联阅读：[domain/src/execution_recipe.rs](runtime-and-permissions.md#file-src-execution-recipe-rs)、[adapters/src/runtime_registry.rs](../devdesk-adapters/runtime-preparation.md#file-src-runtime-registry-rs)、[adapters/src/runtime_verification.rs](../devdesk-adapters/runtime-preparation.md#file-src-runtime-verification-rs)。

<a id="file-src-execution-recipe-rs"></a>

## src/execution_recipe.rs：配置决定运行配方，摘要决定实际来源

源码：[crates/domain/src/execution_recipe.rs](../../../crates/domain/src/execution_recipe.rs)。

`HarnessInstallation` 保存包、入口、扩展与本机发行件；版本唯一来自 Harness.runtime_version。`SandboxBuild` 保存基础镜像、系统包、后端文件及可选预构建镜像映射。`compose_execution_recipe` 生成有界内部 ExecutionRecipe，校验 digest、路径、文件总量及下载件；这些派生数据不是第三种配置源。

`recipe_digest` 对实际镜像引用或生成文件集合计算身份。`resolve_configuration_edge` 只读取确切源引用；安装组合不再通过另一种资源源或内容地址边查找。相同驱动可有不同合法安装组合，发布不启动构建。

因此同一个驱动可以存在多个合法配方；准备和协议验证由适配器完成，发布配置本身不会构建容器。

代码定位：[ExecutionRecipe / compose_execution_recipe](../../../crates/domain/src/execution_recipe.rs)。

关联阅读：[adapters/src/runtime_preparation.rs](../devdesk-adapters/runtime-preparation.md#file-src-runtime-preparation-rs)、[core/src/resource_receipts.rs](../devdesk-core/session-resources-and-permissions.md#file-src-resource-receipts-rs)、[domain/src/configuration_graph.rs](configuration-and-sources.md#file-src-configuration-graph-rs)。

<a id="file-src-model-provider-rs"></a>

## src/model_provider.rs：服务商事实、容量及冻结连接的纯规则

源码：[crates/domain/src/model_provider.rs](../../../crates/domain/src/model_provider.rs)。

`ProviderFacts` 描述 OAuth 托管、云订阅和本地服务三种连接形态，具体服务商列表来自源配置。端点、路由、协议、密钥环境变量和账户前缀由事实提供；容量目录通过配方摘要和精确 provider_ref 与 Harness 关联。

解析 Model 时检查 Harness 覆盖的服务商、容量与请求输出预算。未知模型需要明确容量；云订阅不能用 Model 任意替换固定端点；本地服务支持规范化 HTTP(S) 地址，并把 loopback 的 worker 地址转换为宿主网关形式。地址转换不能证明宿主服务已可达。

`ModelConnection.validate` 再次核验冻结快照自身及参数边界，实际 profile 的摘要由适配器核对。单元测试覆盖未知模型、容量回填、OAuth 严格字段、地址规范化和篡改连接。读取目录与调用模型仍在外层执行。

代码定位：[`ProviderFacts`](../../../crates/domain/src/model_provider.rs#L26)、[`ModelCatalogDefinition`](../../../crates/domain/src/model_provider.rs#L153)、[`ModelConnection`](../../../crates/domain/src/model_provider.rs#L278)、[`resolve_model_connection`](../../../crates/domain/src/model_provider.rs)、[`ollama_endpoint`](../../../crates/domain/src/model_provider.rs#L302)、[`output_budget`](../../../crates/domain/src/model_provider.rs#L520)。

关联阅读：[adapters/src/model_providers.rs](../devdesk-adapters/external-integrations.md#file-src-model-providers-rs)、[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)、[host/src/model_providers.rs](../devdesk-host/rpc-and-configuration.md#file-src-model-providers-rs)。

<a id="file-src-permission-rs"></a>

## src/permission.rs：可移植策略与实际执行计划

源码：[crates/domain/src/permission.rs](../../../crates/domain/src/permission.rs)。

文件权限区分 deny/read/read_write，文件系统支持 full_access、read_only 和显式 scoped 规则；网络支持 enabled、disabled、allowlist；工具支持全部或 allow/deny 列表，审批另有 never/on_request。规范化与校验为策略提供稳定含义，不代表所有后端都能落实所有组合。

`ExecutionScope` 保存当前工作的写入/只读路径。`ExecutionPermissions` 结合双端策略、绑定锁、审批、Scope、资源限制和后端资产计划；`PermissionEnforcementPlan` 由编译器生成实际后端与资产摘要，调用方不能直接把自填原生参数当作授权。

`file_access` 是纯文件权限查询；工具授权由冻结 Profile 与实际执行后端核对，`isolated_test_permissions` 为测试隔离建立明确范围。执行后端仍要检查 symlink、受保护元数据和实际挂载，无法落实时拒绝。

代码定位：[`PermissionPolicy`](../../../crates/domain/src/permission.rs#L48)、[`ExecutionScope`](../../../crates/domain/src/permission.rs#L69)、[`ExecutionPermissions`](../../../crates/domain/src/permission.rs#L139)、[`PermissionEnforcementPlan`](../../../crates/domain/src/permission.rs#L128)、[`file_access`](../../../crates/domain/src/permission.rs#L232)。

关联阅读：[domain/src/permission_profile.rs](runtime-and-permissions.md#file-src-permission-profile-rs)、[adapters/src/permissions.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-permissions-rs)、[adapters/src/docker.rs](../devdesk-adapters/engineering-resources.md#file-src-docker-rs)。

<a id="file-src-permission-profile-rs"></a>

## src/permission_profile.rs：Profile、目标绑定和双端权限锁

源码：[crates/domain/src/permission_profile.rs](../../../crates/domain/src/permission_profile.rs)。

Profile 保存策略正文，`PermissionTargetRef` 固定 Harness/Sandbox 的具体修订和内容摘要，`VersionedPermissionBinding` 把目标映射到 Profile，活动 assignment 再给目标选择当前绑定。`PermissionSelection` 可 follow_target、pinned 或直接选择 Profile；执行开始解析为确切 BoundPermission 和双端锁。

`ProfilePolicy` 要求 Harness 有文件、网络、工具、审批完整策略，Sandbox 不携带工具/审批。`normalize` 生成可移植规范形式，`permission_content_hash` 保持内容身份。`resolve_permission_lock` 检查目标、Profile、绑定及 Scope 一致；`validate_frozen_permissions` 防止保存后的锁与实际策略脱节。

这里还定义 Sandbox 源的形状和从项目和步骤的 Profile 引用取得执行选择的方法。follow_target 只影响下一次解析，不能使已经冻结的运行自动追随新 assignment；自动检测和后继切换由 core 完成。

代码定位：[`PermissionProfile`](../../../crates/domain/src/permission_profile.rs#L84)、[`ProfilePolicy`](../../../crates/domain/src/permission_profile.rs#L45)、[`PermissionSelection`](../../../crates/domain/src/permission_profile.rs#L163)、[`VersionedPermissionBinding`](../../../crates/domain/src/permission_profile.rs#L179)、[`VersionedPermissionLock`](../../../crates/domain/src/permission_profile.rs#L197)、[`resolve_permission_lock`](../../../crates/domain/src/permission_profile.rs#L324)。

关联阅读：[core/src/permission_resources.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-resources-rs)、[core/src/permission_switch.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-switch-rs)、[adapters/src/permission_profiles.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-permission-profiles-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
