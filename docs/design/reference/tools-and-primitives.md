# Agent 工具与 Host 调度原语

先区分“Agent 可以请求的业务动作”和“Host 执行动作时调用的内部能力”。前者需要工具定义和会话授权，后者是 Rust 端口或适配器方法，不直接交给模型任意调用。

## 发布和授权

当前 manifest 发布 49 个 platform 工具定义及 2 个 Harness 原生能力声明；core 注册表包含 54 个平台处理器。注册但未发布为当前出厂工具的处理器有 `workspace.snapshot`、`tasks.complete`、`git.candidate`、`git.advance`、`git.publish`。不能将这 54 个处理器全部描述为当前 Agent 可调用工具。

实际可见集合由 Capability 引用、运行配置、驱动能力和权限进一步限制。平台工具的模型调用名由 `devdesk_` 前缀加将点号替换为下划线的 qualified_name 生成；本文使用更容易阅读的 qualified_name。

下表从当前发布源整理。“必需字段”只列 Schema 的 required；运行时还检查作用域、身份、原版本及参数组合，不能把这列当作完整授权要求。点开定义可查看可选字段和输出 Schema。

## 项目读取与内容检查

结果及副作用：读取/检查，不直接批准工作流。

授权重点：project.read；spec.lint 属于 configuration.preview。

| 工具 | 用途与完整定义 | Schema 必需字段 |
|---|---|---|
| `project.overview` | [Project Overview](../../../configurations/software-development/catalog/tools/builtin-tool-project-overview-v2.json) | 无；仍需运行上下文 |
| `project.records.list` | [List Project Records](../../../configurations/software-development/catalog/tools/builtin-tool-project-records-list-v2.json) | `kind` |
| `project.record.read` | [Read Project Record](../../../configurations/software-development/catalog/tools/builtin-tool-project-record-read-v2.json) | `kind`、`id` |
| `project.artifact.read` | [Read Project Artifact](../../../configurations/software-development/catalog/tools/builtin-tool-project-artifact-read-v2.json) | `id` |
| `project.resources` | [Inspect Project Resources](../../../configurations/software-development/catalog/tools/builtin-tool-project-resources-v2.json) | 无；仍需运行上下文 |
| `spec.lint` | [Validate Specification](../../../configurations/core/tools/builtin-tool-spec-lint-v2.json) | `value` |

## 配置查询与变更

结果及副作用：查询返回定义或使用关系；preview 返回影响；apply/transfer 保存变更。

授权重点：configuration.read / preview / apply。

| 工具 | 用途与完整定义 | Schema 必需字段 |
|---|---|---|
| `configuration.catalog` | [Configuration Catalog](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-catalog-v2.json) | 无；仍需运行上下文 |
| `configuration.read` | [Read Configuration Revision](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-read-v2.json) | `id` |
| `configuration.usage` | [Find Configuration References](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-usage-v2.json) | `id` |
| `configuration.change.preview` | [Preview Configuration Changes](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-change-preview-v2.json) | `changeset` |
| `configuration.change.apply` | [Apply Configuration Changes](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-change-apply-v2.json) | `changeset`、`preview_hash` |
| `configuration.transfer.stage` | [Stage Configuration Transfer](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-transfer-stage-v2.json) | `global_changeset`、`project_changeset` |
| `configuration.transfer.status` | [Check Configuration Transfer Status](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-transfer-status-v2.json) | `id` |
| `configuration.transfer.apply` | [Apply Configuration Transfer](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-transfer-apply-v3.json) | `id`、`expected_revision` |
| `configuration.transfer.cancel` | [Cancel Configuration Transfer](../../../configurations/software-development/catalog/tools/builtin-tool-configuration-transfer-cancel-v3.json) | `id`、`expected_revision` |

## 工作流与经理

结果及副作用：返回状态、输出、执行记录或控制结果；写操作可创建后台执行。

授权重点：工作流控制授权、原计划和控制版本；不能代替 Human。

| 工具 | 用途与完整定义 | Schema 必需字段 |
|---|---|---|
| `workflow.inspect` | [Inspect Workflow](../../../configurations/core/tools/builtin-tool-workflow-inspect-v3.json) | 无；仍需运行上下文 |
| `workflow.events.read` | [Read Workflow Events](../../../configurations/core/tools/builtin-tool-workflow-events-read-v2.json) | 无；仍需运行上下文 |
| `workflow.outputs.read` | [Read Workflow Outputs](../../../configurations/core/tools/builtin-tool-workflow-outputs-read-v3.json) | `ids` |
| `workflow.execution.inspect` | [Inspect Workflow Execution](../../../configurations/core/tools/builtin-tool-workflow-execution-inspect-v2.json) | `execution_id` |
| `workflow.change.preview` | [Preview Workflow Changes](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-change-preview-v4.json) | `changeset` |
| `workflow.controller.wait` | [Wait for Workflow Events](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-controller-wait-v2.json) | `plan_revision_id`、`delivery_id`、`summary` |
| `workflow.controller.notify` | [Notify Workflow Operators](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-controller-notify-v2.json) | `plan_revision_id`、`interaction` |
| `workflow.start` | [Start Workflow](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-start-v4.json) | `workflow_ref`、`role_agent_bindings`、`expected_revision` |
| `workflow.step.dispatch` | [Dispatch Workflow Step](../../../configurations/core/tools/builtin-tool-workflow-step-dispatch-v3.json) | `plan_revision_id`、`step`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.step.accept` | [Accept Step Outputs](../../../configurations/core/tools/builtin-tool-workflow-step-accept-v3.json) | `plan_revision_id`、`step`、`execution_id`、`input_digest`、`outputs`、`expected_revision`、`control_version`、`expected_step_revision` |
| `workflow.step.input` | [Provide Step Input](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-step-input-v3.json) | `plan_revision_id`、`step`、`input`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.step.retry` | [Retry Failed Step](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-step-retry-v3.json) | `plan_revision_id`、`step`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.step.rework` | [Rework Workflow Step](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-step-rework-v4.json) | `plan_revision_id`、`step`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.execution.stop` | [Stop Workflow Execution](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-execution-stop-v3.json) | `plan_revision_id`、`step`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.pause` | [Pause Workflow Dispatch](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-pause-v3.json) | `plan_revision_id`、`expected_revision`、`control_version` |
| `workflow.resume` | [Resume Workflow](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-resume-v3.json) | `plan_revision_id`、`expected_revision`、`control_version` |
| `workflow.cancel` | [Cancel Workflow](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-cancel-v3.json) | `plan_revision_id`、`expected_revision`、`control_version` |
| `workflow.change.apply` | [Apply Workflow Changes](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-change-apply-v5.json) | `plan_revision_id`、`changeset`、`preview_hash`、`expected_revision`、`control_version` |
| `workflow.materials.import` | [Import Workflow Artifacts](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-materials-import-v3.json) | `plan_revision_id`、`delivery_ref`、`value`、`source`、`expected_revision`、`control_version` |
| `workflow.execution.reconcile` | [Reconcile Workflow Execution](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-execution-reconcile-v3.json) | `plan_revision_id`、`step`、`expected_revision`、`control_version`、`expected_step_revision`、`execution_id` |
| `workflow.complete` | [Complete Workflow](../../../configurations/core/tools/builtin-tool-workflow-complete-v3.json) | `plan_revision_id`、`expected_revision`、`control_version` |
| `workflow.segment.dispatch` | [Dispatch Workflow Segment](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-segment-dispatch-v5.json) | `plan_revision_id`、`reason`、`steps`、`expected_revision`、`control_version`；可显式授予普通审批材料准备与完整实例收尾 |
| `workflow.decision.request` | [Request Human Decision](../../../configurations/core/tools/builtin-tool-workflow-decision-request-v5.json) | `plan_revision_id`、`reason`、`operation`、`arguments`、`interaction`、`expected_revision`、`control_version` |
| `workflow.rework.preview` | [Preview Workflow Rework](../../../configurations/software-development/catalog/tools/builtin-tool-workflow-rework-preview-v2.json) | `instance_id`、`plan_revision_id`、`rework` |

## 工程执行与交付

结果及副作用：产生工作区、测试、代码发布或合入结果，可能有外部副作用。

授权重点：所属执行与工程权限；需要对应输入和结果证明。

| 工具 | 用途与完整定义 | Schema 必需字段 |
|---|---|---|
| `workspace.current` | [Inspect Current Workspace](../../../configurations/software-development/catalog/tools/builtin-tool-workspace-current-v2.json) | 无；仍需运行上下文 |
| `tasks.create` | [Create Tasks](../../../configurations/software-development/catalog/tools/builtin-tool-tasks-create-v2.json) | `tasks` |
| `materials.capture` | [Capture Input Artifacts](../../../configurations/software-development/catalog/tools/builtin-tool-materials-capture-v2.json) | 无；仍需运行上下文 |
| `tests.run` | [Run Independent Tests](../../../configurations/software-development/catalog/tools/simplified-tool-tests-run-v3.json) | `workspace`、`recipe` |
| `tests.project` | [Run Project Regression Tests](../../../configurations/software-development/catalog/tools/simplified-tool-project-tests-v3.json) | `workspace` |
| `workspace.checkpoint` | [Create Workspace Checkpoint](../../../configurations/software-development/catalog/tools/simplified-tool-checkpoint-v2.json) | `workspace`、`allowed_paths` |
| `git.source.publish` | [Publish Source Branch](../../../configurations/software-development/catalog/tools/simplified-tool-source-publish-v2.json) | `workspace` |
| `git.delivery.open` | [Open or Update Pull Request](../../../configurations/software-development/catalog/tools/simplified-tool-delivery-open-v3.json) | `workspace`、`target` |
| `git.delivery.merge` | [Merge Pull Request and Sync Workspace](../../../configurations/software-development/catalog/tools/simplified-tool-delivery-merge-v2.json) | `workspace`、`publication` |
| `workspace.target` | [Inspect and Sync Target Branch](../../../configurations/software-development/catalog/tools/simplified-tool-target-v2.json) | `workspace` |

## 结果怎样返回

查询工具返回当前可见状态或内容。`workflow.outputs.read` 使用确切输出 ID，支持 pointer、offset、max_bytes；大结果返回 value_chunk/next_offset。经理项目读取还会核对内容摘要并脱敏已知凭据字段。

派发和工程工具返回的是对应阶段的操作结果，不一定表示整个工作流已完成。工具调用和回复保留调用 ID、会话、轮次与执行关联。模型自述的成功不代替实际工具回执。

CONTROL_TOOLS 与 MANAGER_TOOLS 是不同授权分组，定义分别在 [controllers.rs](../../../crates/core/src/controllers.rs) 和 [manager_workspace.rs](../../../crates/core/src/manager_workspace.rs)。工程处理从 [platform_tools.rs](../../../crates/core/src/platform_tools.rs) 进入准备、执行、核对和回复模块。

## 原生工具、外部工具和经验接口

| 类别 | 入口 | 执行位置与权限 |
|---|---|---|
| Harness 原生能力 | `harness.native` 定义 | 供应商程序提供命令/文件等能力，受当前 Harness/Sandbox 规则限制 |
| 外部 HTTP/MCP | 采用的 ToolDefinition 与 IntegrationBinding | Host 接入执行器启动受管工具 worker；使用该绑定的账户和地址权限 |
| 经验目录 | `agent.experience.list` | Host 上下文读取接口，返回可见经验入口 |
| 经验正文 | `agent.experience.read` | 按引用读取，不授予源项目修改权 |

经验接口由 [agent_context.rs](../../../crates/core/src/agent_context.rs) 生成和处理，不计入上面的 49 个出厂平台工具。外部接入的能力范围见[接入参考](integrations.md)。

## Host 内部原语

这些是程序内部调用，不是新增一组公开 Agent 工具。core 端口统一描述需求，adapters 完成具体进程或服务操作，Host 装配和调度它们。

| 类别 | 基本操作 | 输入与结果 | 定义/实现 |
|---|---|---|---|
| Harness 会话 | open_session、begin_session、poll_session_start | SessionStart → SessionReceipt 或尚未就绪 | [ports](../../../crates/core/src/ports.rs)、[harness](../../../crates/adapters/src/harness.rs) |
| Harness 回合 | dispatch_turn、poll_session | TurnDispatch → 观察消息/终态 | 同上 |
| 交互回复 | answer_tool_call、answer_interaction | 原请求身份 + 工具结果或用户回答 | 同上 |
| 停止与清理 | stop_session、inspect_worker_stop、cleanup_worker、inspect_worker_cleanup | 运行身份 → 停止/移除证明 | 同上 |
| 日志 | session_log | 固定运行记录 → 可读取的日志 | 同上 |
| Docker 容器 | create、inspect、start、attach_command | ContainerSpec / receipt → 实际容器和连接 | [docker](../../../crates/adapters/src/docker.rs) |
| Docker 结束 | logs、logs_split、stop、remove | 容器身份 → 日志、停止与移除检查 | 同上；不删除共享镜像 |
| 安装与权限准备 | prepare_harness、ensure_runtime、validate_installation、verify_permissions、compile_permissions | 所选配置 → 可执行安装及权限 | [SessionResources](../../../crates/core/src/ports.rs)、[session_resources](../../../crates/adapters/src/session_resources.rs) |
| 外部工具 | start、poll、inspect、cancel、discover | ToolInvocation / 来源 → 调用结果或待核对状态 | [ToolExecutor](../../../crates/core/src/ports.rs)、[integrations](../../../crates/adapters/src/integrations.rs) |
| 工程资源 | Git / Tests / PullRequests 的操作及核对方法 | 固定仓库版本、测试请求或交付请求 → 实际结果 | [ports](../../../crates/core/src/ports.rs)、[工程手册](../../crates/devdesk-adapters/engineering-resources.md) |
| 状态与产物 | EngineeringStore 读取/提交；ArtifactStore seal/inspect/read | 有限资源集合或产物内容 → 版本化提交/产物 | [存储手册](../../crates/devdesk-adapters/storage-and-source-codecs.md) |

异步 start/poll 与 inspect/reconcile 的区别在于：前者推进已知的运行过程，后者确认中断后原操作究竟发生了什么。stop 与 cleanup 也不同，停止的容器仍可能需要单独清理。

## 怎样追踪一次调用

以 `workflow.step.dispatch` 为例，从[平台工具手册](../../crates/devdesk-core/manager-and-platform-tools.md)找到授权和路由，再进入 workflow_dispatch 保存操作，随后由 Host 调度会话，最后进入 Harness 与 Docker。完整过程见[跨模块调用链](../../crates/execution-flows.md)。
