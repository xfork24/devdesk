# 文件索引

返回 [core 概览](overview.md)。本表用于查文件，功能过程请先读概览中的专题。

由 `python3 scripts/py/check-docs.py --write` 更新；保留已有的人工专题链接。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/core/Cargo.toml) | Cargo.toml |
| [src/agent_context.rs](../../../crates/core/src/agent_context.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/bootstrap_recovery.rs](../../../crates/core/src/bootstrap_recovery.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/builtin_defaults.rs](../../../crates/core/src/builtin_defaults.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/catalog_sync.rs](../../../crates/core/src/catalog_sync.rs) | Explicit background publication; ordinary reads only compose disposable catalogs. |
| [src/command_catalog.rs](../../../crates/core/src/command_catalog.rs) | `template_definition_write`、`template_catalog_query` |
| [src/concurrency.rs](../../../crates/core/src/concurrency.rs) | [Engine、端口与资源提交](engine-and-ports.md) |
| [src/configuration.rs](../../../crates/core/src/configuration.rs) | Explicit reusable capabilities, visibility and configuration reference checks. |
| [src/configuration_catalog.rs](../../../crates/core/src/configuration_catalog.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configuration_changes.rs](../../../crates/core/src/configuration_changes.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configuration_publication.rs](../../../crates/core/src/configuration_publication.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configuration_queries.rs](../../../crates/core/src/configuration_queries.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configuration_transfers.rs](../../../crates/core/src/configuration_transfers.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configured_runtime.rs](../../../crates/core/src/configured_runtime.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/configured_workflows.rs](../../../crates/core/src/configured_workflows.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/controller_automation_event_tests.rs](../../../crates/core/src/controller_automation_event_tests.rs) | `automatic_notices_coalesce_only_owned_normal_human_progress` |
| [src/controller_calls.rs](../../../crates/core/src/controller_calls.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/controller_definitions.rs](../../../crates/core/src/controller_definitions.rs) | Controller configuration is derived from the complete source catalog and explicit actual work/root facts. It does not inspect runtime history. |
| [src/controller_event_tests.rs](../../../crates/core/src/controller_event_tests.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/controller_events.rs](../../../crates/core/src/controller_events.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/controller_interaction_reads.rs](../../../crates/core/src/controller_interaction_reads.rs) | Human-facing projections from exact workflow resources, never an incomplete EngineeringState. Display witnesses do not replace command authorization. |
| [src/controller_interactions.rs](../../../crates/core/src/controller_interactions.rs) | Human statements and immutable interaction evidence. |
| [src/controller_lifecycle.rs](../../../crates/core/src/controller_lifecycle.rs) | Human manager lifecycle commands carry explicit owner/controller versions. Successor snapshots are sealed through the same durable artifact protocol. |
| [src/controller_runtime.rs](../../../crates/core/src/controller_runtime.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/controllers.rs](../../../crates/core/src/controllers.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/conversation_admission.rs](../../../crates/core/src/conversation_admission.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_captures.rs](../../../crates/core/src/conversation_captures.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_controls.rs](../../../crates/core/src/conversation_controls.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_observations.rs](../../../crates/core/src/conversation_observations.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_start.rs](../../../crates/core/src/conversation_start.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_submission.rs](../../../crates/core/src/conversation_submission.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversation_workers.rs](../../../crates/core/src/conversation_workers.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/conversations.rs](../../../crates/core/src/conversations.rs) | Bounded conversation query dispatch. Business writes have native commands. |
| [src/credential_updates.rs](../../../crates/core/src/credential_updates.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/default_permissions.rs](../../../crates/core/src/default_permissions.rs) | `template_permission_defaults`、`ensure_default_permissions`、`reset_factory_catalog` |
| [src/delivery_recovery.rs](../../../crates/core/src/delivery_recovery.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/driver_capabilities.rs](../../../crates/core/src/driver_capabilities.rs) | Executable protocol facts are supplied by the installed adapters, never by source declarations. |
| [src/environment.rs](../../../crates/core/src/environment.rs) | Scoped application sources make scheduling and recovery reproducible in tests. |
| [src/experience_directory.rs](../../../crates/core/src/experience_directory.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/formal_inputs.rs](../../../crates/core/src/formal_inputs.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/formal_sessions.rs](../../../crates/core/src/formal_sessions.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/frozen_views.rs](../../../crates/core/src/frozen_views.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/governance.rs](../../../crates/core/src/governance.rs) | Configuration commands, immutable publication and workflow runtime coordination. |
| [src/human_governance.rs](../../../crates/core/src/human_governance.rs) | Business decisions remain human-owned regardless of the selected driver policy. |
| [src/in_flight.rs](../../../crates/core/src/in_flight.rs) | In-process technical attempt registry; persistent intents retain authority. |
| [src/integration_operations.rs](../../../crates/core/src/integration_operations.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/integrations.rs](../../../crates/core/src/integrations.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/interaction_delivery.rs](../../../crates/core/src/interaction_delivery.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/invocation.rs](../../../crates/core/src/invocation.rs) | Trusted invocation metadata is created at an application boundary, never decoded from RPC. |
| [src/lib.rs](../../../crates/core/src/lib.rs) | [Engine、端口与资源提交](engine-and-ports.md) |
| [src/live_runs.rs](../../../crates/core/src/live_runs.rs) | `stop_owned_runtime`、`stop_owned_external_tools` |
| [src/manager_publication.rs](../../../crates/core/src/manager_publication.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/material_commands.rs](../../../crates/core/src/material_commands.rs) | Human material and manual context edits use explicit ownership facts. File sealing is outside SQL and retains the original finite admission plan. |
| [src/native_context_calls.rs](../../../crates/core/src/native_context_calls.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/orchestration.rs](../../../crates/core/src/orchestration.rs) | Frozen plan publication and bounded control reads. |
| [src/permission_resources.rs](../../../crates/core/src/permission_resources.rs) | Versioned profiles and assignments share the durable configuration command ledger. |
| [src/permission_switch.rs](../../../crates/core/src/permission_switch.rs) | Permission adoption freezes its original request before stopping old writers. |
| [src/platform_cleanup.rs](../../../crates/core/src/platform_cleanup.rs) | Test disposal has its own once-only grant. Missing acknowledgements use read-only inspection and never trigger another cleanup operation. |
| [src/platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/platform_delivery_projection.rs](../../../crates/core/src/platform_delivery_projection.rs) | The source Feature retains validation materials. Only its reviewed code projection is published as a mainline PR, after all source gates have passed. |
| [src/platform_facts.rs](../../../crates/core/src/platform_facts.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/platform_observations.rs](../../../crates/core/src/platform_observations.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/platform_preparation.rs](../../../crates/core/src/platform_preparation.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/platform_publish.rs](../../../crates/core/src/platform_publish.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/platform_reads.rs](../../../crates/core/src/platform_reads.rs) | Read-only platform tools use exact source contracts and selected records. |
| [src/platform_reconciliation.rs](../../../crates/core/src/platform_reconciliation.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/platform_replies.rs](../../../crates/core/src/platform_replies.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/platform_runtime.rs](../../../crates/core/src/platform_runtime.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/platform_tools.rs](../../../crates/core/src/platform_tools.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/ports.rs](../../../crates/core/src/ports.rs) | [Engine、端口与资源提交](engine-and-ports.md) |
| [src/project_creation.rs](../../../crates/core/src/project_creation.rs) | Creation after the Host has durably reserved this project's identity. The registration/provisioning lifecycle is separate from this atomic metadata. |
| [src/project_reservation.rs](../../../crates/core/src/project_reservation.rs) | Durable project identity reservation. Provisioning happens after this finite insert; retries and competing submissions return the same reserved database. |
| [src/project_standards.rs](../../../crates/core/src/project_standards.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/project_updates.rs](../../../crates/core/src/project_updates.rs) | Finite Project metadata preparation. This use case is kept off the legacy dispatcher until every writer sharing Project invariants has cut over. |
| [src/proposal_commands.rs](../../../crates/core/src/proposal_commands.rs) | Proposals freeze a real completed discussion and an exact target revision. Extraction and adoption never grant workflow or platform evidence. |
| [src/provider_tools.rs](../../../crates/core/src/provider_tools.rs) | [项目经理与平台工具](manager-and-platform-tools.md) |
| [src/publication_retry.rs](../../../crates/core/src/publication_retry.rs) | Explicit retries may adopt an already completed publication from their own exact lineage. Old cancellation and physical operation identities stay intact. |
| [src/read_models.rs](../../../crates/core/src/read_models.rs) | Bounded display projections. These values never authorize workflow actions. |
| [src/repository_dispatch.rs](../../../crates/core/src/repository_dispatch.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/repository_projection.rs](../../../crates/core/src/repository_projection.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/repository_runtime.rs](../../../crates/core/src/repository_runtime.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/repository_updates.rs](../../../crates/core/src/repository_updates.rs) | Explicit repository preparation contexts. Public dispatch remains disabled until repository callbacks and their shared business constraints cut over. |
| [src/resource_receipts.rs](../../../crates/core/src/resource_receipts.rs) | `materialization_receipt`、`import_experience`、`record_harness_materialization` |
| [src/rpc_contracts.json](../../../crates/core/src/rpc_contracts.json) | src/rpc_contracts.json |
| [src/rpc_contracts.rs](../../../crates/core/src/rpc_contracts.rs) | Explicit ingress inventory shared by admission and regression checks. |
| [src/runtime_callbacks.rs](../../../crates/core/src/runtime_callbacks.rs) | Bounded recovery of already granted operations. A persisted observation can be admitted again; absence of an observation never grants external I/O. |
| [src/runtime_registry.rs](../../../crates/core/src/runtime_registry.rs) | Immutable installation registration has its own identity and absence guard. |
| [src/sealed_commands.rs](../../../crates/core/src/sealed_commands.rs) | Local material writes retain their exact finite admission plan across file I/O. Recovery authenticates the original file identity before admitting it. |
| [src/session_assembly.rs](../../../crates/core/src/session_assembly.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/session_commands.rs](../../../crates/core/src/session_commands.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/settings.rs](../../../crates/core/src/settings.rs) | Global preferences use independent resource revisions, never a project revision. |
| [src/spec_lint.rs](../../../crates/core/src/spec_lint.rs) | A single structural checker for authoring previews and accepted immutable values. |
| [src/standards.rs](../../../crates/core/src/standards.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/templates.rs](../../../crates/core/src/templates.rs) | [配置加载、发布与编译](configuration-and-catalog.md) |
| [src/tool_calls.rs](../../../crates/core/src/tool_calls.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/tool_creation.rs](../../../crates/core/src/tool_creation.rs) | [工程资源与外部工具的应用流程](integrations-and-repositories.md) |
| [src/work_owners.rs](../../../crates/core/src/work_owners.rs) | Explicit ownership facts shared by material and conversation use cases. |
| [src/worker_reconciliation.rs](../../../crates/core/src/worker_reconciliation.rs) | [会话、运行资源与结果处理](session-resources-and-permissions.md) |
| [src/workflow_authorization.rs](../../../crates/core/src/workflow_authorization.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_budget.rs](../../../crates/core/src/workflow_budget.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_change_commit.rs](../../../crates/core/src/workflow_change_commit.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_change_preview.rs](../../../crates/core/src/workflow_change_preview.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_children.rs](../../../crates/core/src/workflow_children.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_commits.rs](../../../crates/core/src/workflow_commits.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_control_reads.rs](../../../crates/core/src/workflow_control_reads.rs) | Exact read-only control facts do not reconstruct an unrelated frozen plan. |
| [src/workflow_controls.rs](../../../crates/core/src/workflow_controls.rs) | Public workflow controls retain their original draft and native versions. |
| [src/workflow_delivery_migration.rs](../../../crates/core/src/workflow_delivery_migration.rs) | Human-requested transport migration keeps the logical work and its source evidence, while replacing the publication and every downstream decision. |
| [src/workflow_detail.rs](../../../crates/core/src/workflow_detail.rs) | Explicit display facts for one instance. No aggregate runtime restoration. |
| [src/workflow_dispatch.rs](../../../crates/core/src/workflow_dispatch.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) | Rust and Harness drivers use the same explicit control kernel and persisted worker intents. |
| [src/workflow_driving.rs](../../../crates/core/src/workflow_driving.rs) | Rust and delegated drivers issue the same public resource-level commands. Native leases and bounded cursors replace aggregate writer transactions. |
| [src/workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_evaluation.rs](../../../crates/core/src/workflow_evaluation.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_execution_controls.rs](../../../crates/core/src/workflow_execution_controls.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_facts.rs](../../../crates/core/src/workflow_facts.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_failure.rs](../../../crates/core/src/workflow_failure.rs) | Resolve a bounded failure message from the original observed value. |
| [src/workflow_material_inputs.rs](../../../crates/core/src/workflow_material_inputs.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_materials.rs](../../../crates/core/src/workflow_materials.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_observation_facts.rs](../../../crates/core/src/workflow_observation_facts.rs) | Actual ancestors, input-runtime evidence and immutable continuation inputs. |
| [src/workflow_outputs.rs](../../../crates/core/src/workflow_outputs.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_readiness.rs](../../../crates/core/src/workflow_readiness.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_reading.rs](../../../crates/core/src/workflow_reading.rs) | Scope-aware readers backed by bounded native projections. |
| [src/workflow_reconciliation.rs](../../../crates/core/src/workflow_reconciliation.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_recovery.rs](../../../crates/core/src/workflow_recovery.rs) | A manager can reopen the producer of an independently observed test failure inside its original frozen plan. Model prose is never a test receipt. |
| [src/workflow_repair_attempts.rs](../../../crates/core/src/workflow_repair_attempts.rs) | `repair_attempt_id`、`action_digest`、`workflow_notice_escalation` |
| [src/workflow_resources.rs](../../../crates/core/src/workflow_resources.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_retries.rs](../../../crates/core/src/workflow_retries.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_review.rs](../../../crates/core/src/workflow_review.rs) | Exact-result exceptions. A manager can propose, only Human can sign. |
| [src/workflow_rework.rs](../../../crates/core/src/workflow_rework.rs) | [返工、计划变更与恢复](workflow-changes.md) |
| [src/workflow_rule_hints.rs](../../../crates/core/src/workflow_rule_hints.rs) | Eventually consistent display hints retain their exact native witnesses. They never authorize a command; control admissions repeat the actual rules. |
| [src/workflow_rules.rs](../../../crates/core/src/workflow_rules.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_runtime_facts.rs](../../../crates/core/src/workflow_runtime_facts.rs) | Public execution facts. Never project credentials or model connection settings. |
| [src/workflow_segment_actions.rs](../../../crates/core/src/workflow_segment_actions.rs) | Optional deterministic actions under an exact, finite delegation. |
| [src/workflow_segments.rs](../../../crates/core/src/workflow_segments.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workflow_start.rs](../../../crates/core/src/workflow_start.rs) | [工作流启动、派发与结果接纳](workflow-execution.md) |
| [src/workspace_commands.rs](../../../crates/core/src/workspace_commands.rs) | Workspace metadata and new work are admitted from explicit native facts. A Feature and its frozen initial plan are created in one atomic commit. |
| [src/workspace_continuation.rs](../../../crates/core/src/workspace_continuation.rs) | Preserve quiescent, uncommitted work between Turns of the same execution. |

<!-- crate-files:end -->
