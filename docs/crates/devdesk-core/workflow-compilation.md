# 从源配置到冻结计划与正式任务

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

编译器将 Workflow、Role→Agent、Delivery/Spec、权限和工具选择组合成执行计划；运行准备再补上实际安装和凭据。纯恢复逻辑只根据历史确切选择重建，不在加载旧记录时触发模型、发现工具或安装资源。

## 本页文件

- [src/configured_workflows.rs](#file-src-configured-workflows-rs)：发布完整配置闭包并生成内存执行计划
- [src/frozen_views.rs](#file-src-frozen-views-rs)：从历史源选择纯重建 Plan 与任务
- [src/agent_tasks.rs](#file-src-agent-tasks-rs)：正式会话与具体执行的唯一关联

<a id="file-src-configured-workflows-rs"></a>

## src/configured_workflows.rs：发布完整配置闭包并生成内存执行计划

源码：[crates/core/src/configured_workflows.rs](../../../crates/core/src/configured_workflows.rs)。

`ConfiguredWorkflowPlan` 包含原 Workflow 修订、Standard/工作采用/Permission Context、解析后文档、Role 绑定、确切定义、权限锁、工具绑定和规范视图。它是执行时需要的组合，不意味着整个展开对象都应持久化。

compile_workflow_sources 递归访问确切子流程，核对 DAG、深度/展开数、Role→Agent 身份、必需 Skill、Tool 能力和授权，解析 Delivery 与 Spec，并检查平台 producer 和真实执行方式相容。独立 Tool Step 不能把一个原生工具组当可调用动作；测试工具需要冻结运行环境；外部工具需要账户和权限绑定。

同一文件处理 Workflow/Spec 草稿与发布、项目配置选择和纯预览查询。草稿可不完整，发布与采用要形成完整可执行闭包；项目 ready 状态返回实际缺口，不补内置角色。编译不会启动 Docker、Git 或模型，正式执行还需资源物化与会话装配。

代码定位：[`ConfiguredWorkflowPlan`](../../../crates/core/src/configured_workflows.rs#L7)、[`compile_workflow_sources`](../../../crates/core/src/configured_workflows.rs#L616)、[`compile_configured_workflow`](../../../crates/core/src/configured_workflows.rs#L63)、[`configured_workflow_command`](../../../crates/core/src/configured_workflows.rs#L79)、[`project_configuration_status`](../../../crates/core/src/configured_workflows.rs#L27)。

关联阅读：[domain/src/workflow.rs](../devdesk-domain/workflows-and-checks.md#file-src-workflow-rs)、[domain/src/source.rs](../devdesk-domain/configuration-and-sources.md#file-src-source-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/frozen_views.rs](workflow-compilation.md#file-src-frozen-views-rs)、[adapters/tests/configured_workflows.rs](../devdesk-adapters/testing.md#file-tests-configured-workflows-rs)。

<a id="file-src-frozen-views-rs"></a>

## src/frozen_views.rs：从历史源选择纯重建 Plan 与任务

源码：[crates/core/src/frozen_views.rs](../../../crates/core/src/frozen_views.rs)。

持久化 codec 保存精确选择，`restore_plan` 沿 Workflow、Standard、角色、权限和集成引用找回当时源闭包，再调用同一编译器生成内存计划。绑定到 permission_binding 的选择按 pinned 恢复，不追随当前 assignment，也不使用当前模板默认替换历史。

`restore_task` 根据原 plan_revision、实例、execution 和来源重建任务，必要时从 workflow_execution_history 找回被替代执行，再放回冻结输入、Agent 和补充文本。它没有环境访问：不会重新发现工具、读取凭据或安装镜像。

原生实际发送正文还要由 adapter codec 核对历史摘要和已认证封存正文；不能因当前渲染器变化便改写过去的 task。源缺失或不一致是存储错误，而不是自动采用新版的理由。

代码定位：[`source_closure`](../../../crates/core/src/frozen_views.rs#L16)、[`restore_plan`](../../../crates/core/src/frozen_views.rs#L125)、[`restore_task`](../../../crates/core/src/frozen_views.rs#L80)。

关联阅读：[adapters/src/source_views.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-source-views-rs)、[core/src/configured_workflows.rs](workflow-compilation.md#file-src-configured-workflows-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)。

<a id="file-src-agent-tasks-rs"></a>

## src/agent_tasks.rs：正式会话与具体执行的唯一关联

源码：[crates/core/src/agent_tasks.rs](../../../crates/core/src/agent_tasks.rs)。

configured_session 记录把 Session 指向精确 workflow_instance、step 和 execution。`formal_session` 与 session_matches_context 供会话输入校验使用，阻止把普通自由对话伪装成正式任务，或把一次 Turn 归给“最近的同名步骤”。

`inherit_task_binding` 为权限/运行接续把关联迁往后继 Session：业务步骤必须确实仍引用原会话，并刷新公开运行上下文；Controller 后继则核对当前会话、提高 epoch、更新 lease 并保存新 controller_session。原记录保留，旧会话不能继续作为当前执行者控制后续工作。

代码定位：[`session_binding`](../../../crates/core/src/agent_tasks.rs#L4)、[`session_workflow_context`](../../../crates/core/src/agent_tasks.rs#L13)、[`formal_session`](../../../crates/core/src/agent_tasks.rs#L26)、[`inherit_task_binding`](../../../crates/core/src/agent_tasks.rs#L30)。

关联阅读：[core/src/conversations.rs](conversations-and-history.md#file-src-conversations-rs)、[core/src/permission_switch.rs](session-resources-and-permissions.md#file-src-permission-switch-rs)、[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/workflow_runtime_facts.rs](workflow-execution.md#file-src-workflow-runtime-facts-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
