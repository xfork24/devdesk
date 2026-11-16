# Engine、调用来源与可注入端口

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

一次请求先固定项目和可信调用者，再交给 Engine 的命令/查询分派；状态持久化、真实执行与系统环境由端口提供。这里是理解其余专题的入口。调用方向可以是 core 调用一个由 adapters 实现的 trait，但编译依赖仍然是 adapters 依赖 core。

## 本页文件

- [src/lib.rs](#file-src-lib-rs)：命令事务、总分派与恢复入口
- [src/ports.rs](#file-src-ports-rs)：应用需要外部世界提供的完整合同
- [src/invocation.rs](#file-src-invocation-rs)：调用者身份不能来自 RPC 自填
- [src/environment.rs](#file-src-environment-rs)：可复现的 ID 与时间来源
- [src/command_catalog.rs](#file-src-command-catalog-rs)：哪些操作受模板代次围栏约束

<a id="file-src-lib-rs"></a>

## src/lib.rs：命令事务、总分派与恢复入口

源码：[crates/core/src/lib.rs](../../../crates/core/src/lib.rs)。

`Engine` 保存工程存储、Artifact、Harness、Git、Tests、PR 及可选工具/经理/配置后端端口。`Command` 包含 command_id、target、expected_revision 和 payload；`command_fingerprint` 将方法、目标、版本和载荷绑定为重放身份。

`execute_internal` 在带幂等键的存储事务内依次检查跨库配置围栏、当前目录成员，分派应用命令，记录后处理事实、可信 command_context 和目录成员。返回的是已提交结果或 durable 操作身份；长操作由后台执行。`execute_resolved` 只允许明确的装配命令复用原请求指纹，防止内部补全参数破坏调用者幂等。

`bootstrap` 将已派发却未知结果的工具、仓库、Turn/Session 标为需核对，并使未终结实例重新评估；它不假设外部没执行。`tick` 汇合后台管线，`query` 汇合只读视图。这里还保留基本 Project/Feature/Task/材料命令，所以阅读总分派时应继续追踪到相应专题，不能只按 HTTP 方法名理解状态变化。

代码定位：[`Engine`](../../../crates/core/src/lib.rs#L99)、[`Command`](../../../crates/core/src/lib.rs#L89)、[`command_fingerprint`](../../../crates/core/src/lib.rs#L96)、[`execute_internal`](../../../crates/core/src/lib.rs#L455)、[`bootstrap`](../../../crates/core/src/lib.rs#L321)、[`tick`](../../../crates/core/src/lib.rs#L732)、[`query`](../../../crates/core/src/lib.rs#L744)。

关联阅读：[core/src/ports.rs](engine-and-ports.md#file-src-ports-rs)、[core/src/invocation.rs](engine-and-ports.md#file-src-invocation-rs)、[adapters/src/lib.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-lib-rs)、[host/src/workspace.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-workspace-rs)。

<a id="file-src-ports-rs"></a>

## src/ports.rs：应用需要外部世界提供的完整合同

源码：[crates/core/src/ports.rs](../../../crates/core/src/ports.rs)。

此文件是 core 与外层的连接面。EngineeringStore 提供带重放键的事务、快照与项目根；ArtifactStore 提供封存和校验读取。Harness 提供会话打开、Turn 派发、观察、交互回答、工具回复、日志、停止和清理；观察类型把消费、终态和停止证据分开。

Harness 的 begin_session 可返回尚未完成，poll_session_start 只查询已预约原 execution 的启动；真实适配器在后台创建并初始化，默认测试适配器可同步返回。核心先持久化额度占位和操作身份，接纳实际回执后才 dispatch_turn；丢失原任务、取消及未知效果不能靠再次 begin 重发恢复。EngineeringStore 的 workflow_runtime_pending/native_platform_tool_prepared 仅为调度提示，observe_native_call 仅为无参数阶段观测，均不授予业务执行权。

Git 接收明确 GitContext、仓库操作及候选/目标，不使用全局 cwd；Tests 接收冻结 TestRequest，并异步 poll/stop/cleanup/reconcile；PullRequests 通过稳定请求查找或创建确切 PR。ToolExecutor 同时承担离线 Schema/参数校验和实际外部工具生命周期，校验调用不等于执行副作用。

SessionResources 将安装准备、完整性/协议验证、权限编译、模型 profile 摘要和凭据引用提供给会话装配。ApplicationEnvironment 提供 ID/时间；ProjectInventory 提供可访问项目快照与资源事实；ConfigurationBackend 提供权限绑定的可行性准备。ExecutionAdapters 将可共享的原生执行端口打包。

实现者必须返回真实、归属一致且可核对的事实；core 负责接纳业务状态。新增 trait 方法要同时核对生产实现、测试替身和所有失败/取消语义。

代码定位：[`EngineeringStore`](../../../crates/core/src/ports.rs#L329)、[`ArtifactStore`](../../../crates/core/src/ports.rs#L354)、[`Harness`](../../../crates/core/src/ports.rs#L15)、[`Git`](../../../crates/core/src/ports.rs#L194)、[`Tests`](../../../crates/core/src/ports.rs#L278)、[`ToolExecutor`](../../../crates/core/src/ports.rs#L169)、[`SessionResources`](../../../crates/core/src/ports.rs#L375)、[`ExecutionAdapters`](../../../crates/core/src/ports.rs#L309)。

关联阅读：[adapters/src/lib.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-lib-rs)、[adapters/src/harness.rs](../devdesk-adapters/harness-and-protocols.md#file-src-harness-rs)、[adapters/src/session_resources.rs](../devdesk-adapters/runtime-preparation.md#file-src-session-resources-rs)、[adapters/src/configuration_backend.rs](../devdesk-adapters/factory-and-configuration.md#file-src-configuration-backend-rs)、[adapters/tests/support/mod.rs](../devdesk-adapters/testing.md#file-tests-support-mod-rs)。

<a id="file-src-invocation-rs"></a>

## src/invocation.rs：调用者身份不能来自 RPC 自填

源码：[crates/core/src/invocation.rs](../../../crates/core/src/invocation.rs)。

`InvocationContext` 只实现 Serialize，字段私有，没有让客户端反序列化 Actor 的入口。人类请求由可信应用边界构造；Controller、Driver、Segment 来源由内部协调器建立。

`record` 检查固定项目，把 actor、项目、目标与原请求指纹写入 command_context。相同 command_id 再出现时，来源和参数必须相同。这层记录提供审计依据，但不替代原生 worker/Turn/epoch 的实际核验。

代码定位：[`InvocationContext`](../../../crates/core/src/invocation.rs#L4)、[`human`](../../../crates/core/src/invocation.rs#L9)、[`workflow`](../../../crates/core/src/invocation.rs#L15)、[`record`](../../../crates/core/src/invocation.rs#L21)。

关联阅读：[core/src/lib.rs](engine-and-ports.md#file-src-lib-rs)、[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)。

<a id="file-src-environment-rs"></a>

## src/environment.rs：可复现的 ID 与时间来源

源码：[crates/core/src/environment.rs](../../../crates/core/src/environment.rs)。

生产默认使用 UUID 和系统毫秒时间；`with_application_environment` 在当前线程调用范围内安装替代来源。局部 Restore 对象在正常返回和 unwind 时恢复原值，因此嵌套测试不会让确定性时钟污染后续用例。

领域对象不直接访问这些环境。调度、租约和生成稳定动作身份的测试可提供固定值，准确重现“过期前/后”状态。该机制是线程局部范围，不应被误认为跨异步线程自动传播的全局测试时钟。

代码定位：[`with_application_environment`](../../../crates/core/src/environment.rs#L5)、[`id`](../../../crates/core/src/environment.rs#L19)、[`now`](../../../crates/core/src/environment.rs#L24)。

关联阅读：[core/src/ports.rs](engine-and-ports.md#file-src-ports-rs)、[core/src/workflow_driver.rs](orchestration-and-drivers.md#file-src-workflow-driver-rs)。

<a id="file-src-command-catalog-rs"></a>

## src/command_catalog.rs：哪些操作受模板代次围栏约束

源码：[crates/core/src/command_catalog.rs](../../../crates/core/src/command_catalog.rs)。

`template_definition_write` 识别会发布、修订、停用源或改变当前目录的命令，包含独立 CRUD、批量变更、Skill/OpenAPI 导入和权限 Profile 操作。`template_catalog_query` 列出需要返回目录上下文的查询。

这是跨入口共用的分类表，不是工具处理器注册表。模板重置和跨库发布使用它拦截旧页面写入及冲突操作。新增定义类命令时漏登记，会使同一种源从不同入口得到不同的代次保护。

代码定位：[`template_definition_write`](../../../crates/core/src/command_catalog.rs#L2)、[`template_catalog_query`](../../../crates/core/src/command_catalog.rs#L35)。

关联阅读：[core/src/lib.rs](engine-and-ports.md#file-src-lib-rs)、[core/src/templates.rs](configuration-and-catalog.md#file-src-templates-rs)、[core/src/configuration_transfers.rs](configuration-and-catalog.md#file-src-configuration-transfers-rs)、[host/src/templates.rs](../devdesk-host/rpc-and-configuration.md#file-src-templates-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
