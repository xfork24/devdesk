# 治理、工具、模型和权限的 RPC 路由

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

这里把界面方法转成 core 用例及 adapters 的资源操作。每条路径都要先确定全局/项目范围、可信调用者和原始请求身份；不要让外层便利参数绕开冻结合同或幂等指纹。

## 本页文件

- [src/governance.rs](#file-src-governance-rs)：治理路由和步骤执行的事务外编排
- [src/integrations.rs](#file-src-integrations-rs)：工具配置、实际调用和 OAuth callback 装配
- [src/model_providers.rs](#file-src-model-providers-rs)：当前模型目录、凭据设置和冻结选择的区别
- [src/permission_resources.rs](#file-src-permission-resources-rs)：权限目录、精确绑定与后继会话发送
- [src/templates.rs](#file-src-templates-rs)：模板重置预演、目录代次与旧引用保留

<a id="file-src-governance-rs"></a>

## src/governance.rs：治理路由和步骤执行的事务外编排

源码：[crates/host/src/governance.rs](../../../crates/host/src/governance.rs)。

governance_dispatch 把源、Agent 实例、工作流控制、驱动与 Spec 请求交给正确 Engine，并通过可信应用边界建立 Actor。源预演/发布、共享采用和项目选择分别走 core 的用例，不靠客户端自填身份。

`tick_workflow_engine` 是关键装配循环：先 evaluate 获得待执行意图，在事务外准备 SessionResources 或执行平台/外部工具，再 observe 保存真实结果，最后重新 evaluate。准备和 I/O 不塞进工程数据库长事务；失败、未知和停止事实原样交回 core 判断。

经验列表/详情走只读来源查询，导入走当前 Engine 的资源回执；跨工程比较不切换活动项目。Agent 组合解析复用 core，不能在 Host 另写一份默认映射。

模块内测试覆盖源修订后的实例/历史仍可查询等治理合同。增加方法时应同时核对 schema、Host 路由、core 命令和工具入口是否一致。

代码定位：[`experience_dispatch`](../../../crates/host/src/governance.rs#L4)、[`tick_workflow_engine`](../../../crates/host/src/governance.rs#L60)[`governance_dispatch`](../../../crates/host/src/governance.rs#L128)。

关联阅读：[core/src/workflow_runtime.rs](../devdesk-core/workflow-execution.md#file-src-workflow-runtime-rs)、[core/src/configuration_transfers.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-transfers-rs)、[adapters/src/experience.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-experience-rs)、[host/src/runtime_setup.rs](workspace-and-lifecycle.md#file-src-runtime-setup-rs)。

<a id="file-src-integrations-rs"></a>

## src/integrations.rs：工具配置、实际调用和 OAuth callback 装配

源码：[crates/host/src/integrations.rs](../../../crates/host/src/integrations.rs)。

integration_dispatch 提供 API Tool、MCP、Skill 等配置与发现/导入入口，把纯资源修改交回 core。execute_configured_tool 从冻结请求解析平台或外部执行器，核对项目、工具来源、账户和权限后才交给 NativeTools。

integration_permissions 将当前授权编译到工具网络和运行合同；密钥在适配器的最后执行边界注入，不经 RPC 返回。OAuth callback 则关联原 flow/state 和固定来源，不把 URL 参数当作选择任意账户的权限。

OpenAPI、MCP 发现和 Skill 获取是受管资源操作，不是任意 URL/宿主命令代理。长操作保留稳定 ID 与回执，未知结果按原操作恢复。测试夹具与错误路径分布在本模块和 adapters 的 tool_integrations 集成测试中。

代码定位：[`execute_configured_tool`](../../../crates/host/src/integrations.rs#L14)、[`integration_permissions`](../../../crates/host/src/integrations.rs#L138)、[`integration_oauth_callback`](../../../crates/host/src/integrations.rs#L171)、[`integration_dispatch`](../../../crates/host/src/integrations.rs#L240)。

关联阅读：[core/src/integrations.rs](../devdesk-core/integrations-and-repositories.md#file-src-integrations-rs)、[core/src/platform_tools.rs](../devdesk-core/manager-and-platform-tools.md#file-src-platform-tools-rs)、[adapters/src/integrations.rs](../devdesk-adapters/external-integrations.md#file-src-integrations-rs)、[adapters/src/integration_oauth.rs](../devdesk-adapters/external-integrations.md#file-src-integration-oauth-rs)。

<a id="file-src-model-providers-rs"></a>

## src/model_providers.rs：当前模型目录、凭据设置和冻结选择的区别

源码：[crates/host/src/model_providers.rs](../../../crates/host/src/model_providers.rs)。

model_provider_facts 汇集当前可见模型 provider 源及设置事实，selected_provider 校验显式选择。目录发现、凭据状态/配置/授权和模型列表路由到对应 adapters；未知 provider 和不完整参数明确拒绝。

界面看到的是当前配置目录，已经启动的 Harness 则继续使用冻结 provider 修订和 profile 摘要。这里不能用新默认重新解析旧 Session 的密钥或连接。

Ollama/DeepSeek 目录结果只描述发现能力，不把宿主可达当作容器推理通过。实际模型调用还需完整运行安装、权限、认证与 Session 绑定。

代码定位：[`model_provider_facts`](../../../crates/host/src/model_providers.rs#L8)、[`selected_provider`](../../../crates/host/src/model_providers.rs#L39)。

关联阅读：[adapters/src/model_providers.rs](../devdesk-adapters/external-integrations.md#file-src-model-providers-rs)、[adapters/src/credentials.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-rs)、[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)。

<a id="file-src-permission-resources-rs"></a>

## src/permission_resources.rs：权限目录、精确绑定与后继会话发送

源码：[crates/host/src/permission_resources.rs](../../../crates/host/src/permission_resources.rs)。

permission_resource_api 提供 Profile/Context、预设目录和绑定入口。目录浏览仅接受受控普通绝对目录，选择后写入单一持久化目录；内嵌预设可恢复资源，却不自动授权当前工程。Profile 发布与绑定修改分开。

绑定预演先用实际运行/资产计算合同；提交保留原请求指纹并核对 target、revision 和所属目录。调用者传来的项目字段不能覆盖实际 BindingLock 归属，项目绑定不能写到共享配置库。

permission_session_status 分别展示 desired 与已冻结 Session 权限。send_permission_conversation 在需要切换时先取得静止/停止和快照，再创建后继会话发送，旧 Session 与历史保持原绑定。失败和重放继续使用原操作，而不是重复产生后继。

发送前还核对实际 adapter 身份与已有权限锁；正式步骤的 integration_lock 保持原锁，普通讨论可重新计算接入选择。后续 core 切换仅接受权限与运行摘要的差异，所以这里发现接入锁改变，并不表示允许借同一权限切换原子替换接入配置。具体可切换字段与 Task/Controller 边界见 [permission_switch](../devdesk-core/session-resources-and-permissions.md#file-src-permission-switch-rs)。

模块测试覆盖目录选择/恢复、权限切换重启、项目归属、原请求重放与不支持规则拒绝。后端是否实际落实策略，另见 adapters 的原生权限测试。

代码定位：[`permission_runtime_digest`](../../../crates/host/src/permission_resources.rs#L7)、[`send_permission_conversation`](../../../crates/host/src/permission_resources.rs#L13)、[`permission_session_status`](../../../crates/host/src/permission_resources.rs#L170)、[`permission_resource_api`](../../../crates/host/src/permission_resources.rs#L226)。

关联阅读：[core/src/permission_switch.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-switch-rs)、[core/src/permission_resources.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-resources-rs)、[adapters/src/permission_profiles.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-permission-profiles-rs)、[adapters/tests/permission_profiles.rs](../devdesk-adapters/testing.md#file-tests-permission-profiles-rs)。

<a id="file-src-templates-rs"></a>

## src/templates.rs：模板重置预演、目录代次与旧引用保留

源码：[crates/host/src/templates.rs](../../../crates/host/src/templates.rs)。

template_snapshot_summary 在全局及各工程只读快照中统计当前代次的定义、停用、草稿、修改数量和 pristine 状态，以 namespace/ID/revision/hash 等组成预演指纹；pinned_projects 列出已有 project_configuration 的项目，并非逐条枚举冻结 Session 或执行历史。template_dispatch 核对 generation、catalog_fingerprint、bundle_hash 和原请求重放，再调用 core 的目录恢复。

恢复使用 adapters 注入的发行 bundle 与权限合同，在全局配置库事务内切换目录并补齐默认绑定；项目库随后通过 sync_shared_configuration/copy_template_catalog 接收新代次，不是一次事务同时修改所有项目。只读 preview 会在临时快照上投影全局代次，不因此写项目库。不可变源、项目采用和旧执行引用保留，旧页面写入由目录代次围栏拒绝。

模块测试检查只改名称的所有源类型、MCP 绑定、已固定引用、重放、stale preview 与失败事务回滚。默认修订断言跟随实际 manifest，不能仅更新一个硬编码 Agent ID 而漏掉整个源闭包。

代码定位：[`template_snapshot_summary`](../../../crates/host/src/templates.rs#L21)、[`template_dispatch`](../../../crates/host/src/templates.rs#L96)。

关联阅读：[core/src/templates.rs](../devdesk-core/configuration-and-catalog.md#file-src-templates-rs)、[core/src/command_catalog.rs](../devdesk-core/engine-and-ports.md#file-src-command-catalog-rs)、[adapters/src/factory_bundle.rs](../devdesk-adapters/factory-and-configuration.md#file-src-factory-bundle-rs)、[core/src/configuration_changes.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-changes-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
