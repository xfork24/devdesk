# 经理知识、平台动作与 Spec 检查

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

平台处理器实现工作空间、任务、测试、候选和发布等受管动作；经理工具提供固定项目的知识与配置创作；Spec lint 提供结构检查。注册表、配置可选、原生可调用、具体处理分支和输出接纳是连续的不同关卡，不能只看注册名就判断整条调用链已可用。

经理只读方法不等待配置写门，复用当前完整授权快照；单记录仅编码所选记录，overview 仅统计可见集合。原生 Agent 的本机 capture/lint/outputs.read 可在意图回执持久化后定向推进原调用，复用普通平台管线的所有权限、取消和输出检查。此路径不调度一般 Git/Docker/网络工作，也不替 Agent 做业务决定。

## 本页文件

- [src/manager_workspace.rs](#file-src-manager-workspace-rs)：固定项目的知识查询与配置创作
- [src/platform_tools.rs](#file-src-platform-tools-rs)：工作空间、任务、测试和 Git 的受管处理器
- [src/spec_lint.rs](#file-src-spec-lint-rs)：结构预览与实际产物接纳共用的检查链

<a id="file-src-manager-workspace-rs"></a>

## src/manager_workspace.rs：固定项目的知识查询与配置创作

源码：[crates/core/src/manager_workspace.rs](../../../crates/core/src/manager_workspace.rs)。

`MANAGER_TOOLS` 包含项目概览、记录分页、分块正文/Artifact、资源事实、配置目录/读取/使用关系、预演/应用/跨库发布和 spec.lint。ManagerServices 注入共享 Engine、ProjectInventory 和应用互斥门。项目读取始终绑定单项目服务，参数不能切项目或指定任意 SQL/宿主路径。

overview 只给摘要并标 overview_only；records 返回总量和分页；正文/产物 chunk 校验 value_digest、UTF-8 边界和 next_offset，不能把首块当作完整材料。sanitize 遮蔽已知凭据字段，但不表示对任意用户正文做语义脱密。resources 区分实际驱动事实、安装/执行观察和源配置。

manager_live 检查当前 Controller、会话、Turn、worker operation 与 epoch。configuration.* 使用同一候选图和 Actor 绑定预演；共享修改另需 configuration.global.write，当前会话不能通过新源扩张本次授权。实例停用/换绑先交付回执再撤销旧会话，人类采用后继另走停止和快照边界。

经理的 spec.lint 按 configuration.preview 权限调用共用检查器，可以检查创作草稿；普通业务步骤另走 lint_workflow_value，只能使用冻结输入/输出的 Delivery。两种调用共享结构检查，授权范围不同。

代码定位：[`MANAGER_TOOLS`](../../../crates/core/src/manager_workspace.rs#L5)、[`ManagerServices`](../../../crates/core/src/manager_workspace.rs#L22)、[`manager_knowledge`](../../../crates/core/src/manager_workspace.rs#L170)、[`manager_live`](../../../crates/core/src/manager_workspace.rs#L275)、[`manager_change_scope`](../../../crates/core/src/manager_workspace.rs#L318)、[`manager_tool`](../../../crates/core/src/manager_workspace.rs#L394)。

关联阅读：[core/src/controllers.rs](orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/configuration_changes.rs](configuration-and-catalog.md#file-src-configuration-changes-rs)、[core/src/configuration_transfers.rs](configuration-and-catalog.md#file-src-configuration-transfers-rs)、[core/src/spec_lint.rs](manager-and-platform-tools.md#file-src-spec-lint-rs)、[adapters/tests/manager_workspace.rs](../devdesk-adapters/testing.md#file-tests-manager-workspace-rs)。

<a id="file-src-platform-tools-rs"></a>

## src/platform_tools.rs：工作空间、任务、测试和 Git 的受管处理器

源码：[crates/core/src/platform_tools.rs](../../../crates/core/src/platform_tools.rs)。

`PlatformToolRequest` 固定 execution、instance、owner、project、确切 Tool、参数、权限及可选原生 caller。业务 Agent 调用需当前 Session/Turn/worker/native request/call ID 和冻结工具授权；独立 Tool Step 不创建占位 Agent。原生控制器/经理的工具在各自管线校验归属，不与普通讨论混用。

`configured_platform_request` 从冻结步骤和已接纳输入重新解析请求，enqueue 再比对所有字段，阻止客户端提交一个看似相同但改变作用域的请求。处理器读取工作空间、封存快照/完整 diff、创建/完成 Task、执行真实测试、构造候选、推进和发布；写任务、快照和候选前校验实际工作副本与活动 writer。

poll_platform_tool 在事务外调用端口，drive_platform_tools 将回执和应用状态一并接纳。Git advance 要匹配 ActionPermit 和当前候选，publish 分开保存候选分支、PR 和远端推进事实，核对精确 lease 与 PR 身份。真实结果若被输出合同拒绝，保留观察证据并及时失败。

spec.lint 通过普通平台 poll 调用 lint_workflow_value，业务 Agent 和 Tool Step 可检查当前冻结输入/输出声明的 Delivery 与草稿；不能借此查询任意项目配置。注册表、实际驱动桥、Capability、步骤授权与处理分支仍是不同检查。请求/业务回执与回复给模型的交付状态分开保存，取消或旧 caller 不能继续派发。

代码定位：[`PLATFORM_HANDLERS`](../../../crates/core/src/platform_tools.rs#L5)、[`PlatformToolRequest`](../../../crates/core/src/platform_tools.rs#L53)、[`project_agent_platform_call`](../../../crates/core/src/platform_tools.rs#L96)、[`configured_platform_request`](../../../crates/core/src/platform_tools.rs#L316)、[`poll_platform_tool`](../../../crates/core/src/platform_tools.rs#L613)、[`drive_platform_tools`](../../../crates/core/src/platform_tools.rs#L1061)、[`reply_agent_platform_calls`](../../../crates/core/src/platform_tools.rs#L1252)。

关联阅读：[core/src/ports.rs](engine-and-ports.md#file-src-ports-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/manager_workspace.rs](manager-and-platform-tools.md#file-src-manager-workspace-rs)、[adapters/src/git.rs](../devdesk-adapters/engineering-resources.md#file-src-git-rs)、[adapters/src/tests.rs](../devdesk-adapters/engineering-resources.md#file-src-tests-rs)、[adapters/tests/configured_workflows.rs](../devdesk-adapters/testing.md#file-tests-configured-workflows-rs)。

<a id="file-src-spec-lint-rs"></a>

## src/spec_lint.rs：结构预览与实际产物接纳共用的检查链

源码：[crates/core/src/spec_lint.rs](../../../crates/core/src/spec_lint.rs)。

step_content_checks 从冻结步骤 Spec 收集检查项，validate_content_report 同时用于自检和实际评审产物接纳，检查报告 key、结论和冻结输入位置。lint_specification_value 只执行声明的机器 assertion，语义项返回 requires_judgment；structure_valid 不等于业务质量已通过。

lint_workflow_value 是业务 Agent/Tool Step 的受限自检入口。它核对项目和当前 execution/node，参数只允许 delivery_ref 与 value；Delivery 必须属于当前步骤的输出，或显式输入/工具参数所引用的端口与同根 TypedMaterial，而且位于冻结定义闭包中。它不能读取任意配置草稿或借自检取得经理权限。若检查的是 content_checks 输出，还返回 content_report_valid/content_report_error，使用实际接纳的同一报告验证。

lint_delivery_value 合并格式、content_schema 与该 Delivery 的固定 Spec，经 ToolExecutor 做有界离线校验；enforce_delivery_value 拒绝不合格值。specification_lint 为经理/UI 提供精确修订或创作草稿预览，不发布源或生成成功 WorkflowOutput。

freeze_input_evidence 保存本执行输入摘要、origin/source output 与授权目标；input_evidence_targets 从这份清单恢复评审可引用值。模型填同名 spec_validation 字段不能代替内核观察，结构自检也不替代独立评审、真实测试或人类决定。

代码定位：[`lint_workflow_value`](../../../crates/core/src/spec_lint.rs#L39)、[`step_content_checks`](../../../crates/core/src/spec_lint.rs#L4)、[`validate_content_report`](../../../crates/core/src/spec_lint.rs#L23)、[`lint_delivery_value`](../../../crates/core/src/spec_lint.rs#L168)、[`enforce_delivery_value`](../../../crates/core/src/spec_lint.rs#L203)、[`freeze_input_evidence`](../../../crates/core/src/spec_lint.rs#L235)。

关联阅读：[domain/src/spec_checks.rs](../devdesk-domain/workflows-and-checks.md#file-src-spec-checks-rs)、[core/src/configured_runtime.rs](workflow-execution.md#file-src-configured-runtime-rs)、[core/src/manager_workspace.rs](manager-and-platform-tools.md#file-src-manager-workspace-rs)、[adapters/tests/configured_workflows.rs](../devdesk-adapters/testing.md#file-tests-configured-workflows-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
