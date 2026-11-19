# devdesk-core：决定什么时候可以推进，以及接纳什么事实

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

core 把领域规则组织成应用用例。它负责发布配置、固定执行选择、准备会话、派发工作、接纳真实回执、处理人工决定与恢复，不直接依赖 SQLite、Docker 或某个模型 SDK。外部能力通过 [ports.rs](engine-and-ports.md#file-src-ports-rs) 注入。

`Engine` 是单个工程状态服务，不是全局项目管理器。Host 为全局配置、各项目和全局讨论装配不同实例，再共享实际执行适配器。命令通常先在短事务内保存决定和意图；原生执行在事务外进行；观察结果后再开事务核对与接纳。`command_id` 与参数指纹处理重放，InvocationContext 处理可信来源，二者都不能由模型的自然语言声明替代。

阅读时先看 Engine/ports，再看配置编译和 configured_runtime，随后进入 orchestration、controllers 和 workflow_driver。三个模块分别负责控制命令、经理原生会话和 Rust 动作选择，不能把它们当成同一层。workflow_eligibility 为查询和命令提供共同业务资格，workflow_failure 从持久化意图/回执解析可信失败及独立重试次数；认证、CAS 和实际状态仍在提交时复核。

## 专题阅读

- [Engine、调用来源与可注入端口](engine-and-ports.md)
- [配置创作、共享目录、采用与跨库发布](configuration-and-catalog.md)
- [从源配置到冻结计划与正式任务](workflow-compilation.md)
- [步骤状态机、执行意图与可信运行事实](workflow-execution.md)
- [运行计划修改、静止等待与有界委托](workflow-changes.md)
- [控制命令、经理会话与 Rust 调度](orchestration-and-drivers.md)
- [经理知识、平台动作与 Spec 检查](manager-and-platform-tools.md)
- [会话命令、原生观察、提案与历史](conversations-and-history.md)
- [完整会话绑定、权限修订与执行安装回执](session-resources-and-permissions.md)
- [外部工具与仓库操作的持久化执行](integrations-and-repositories.md)

## 完整文件索引

由 `python3 scripts/py/check-docs.py --write` 更新。专题解释主要合同，表格覆盖当前全部文件。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/core/Cargo.toml) | [应用层包与依赖注入的构建约束](overview.md#file-cargo-toml) |
| [src/agent_context.rs](../../../crates/core/src/agent_context.rs) | Durable context calls share Agent identity without requiring a Workflow caller. |
| [src/bootstrap_recovery.rs](../../../crates/core/src/bootstrap_recovery.rs) | Opening a dataset publishes only missing configuration sources and marks disconnected original Turns unknown. It never reissues external operations. |
| [src/builtin_defaults.rs](../../../crates/core/src/builtin_defaults.rs) | [从当前目录读取默认选择](configuration-and-catalog.md#file-src-builtin-defaults-rs) |
| [src/catalog_sync.rs](../../../crates/core/src/catalog_sync.rs) | [共享源副本与经验目录索引](configuration-and-catalog.md#file-src-catalog-sync-rs) |
| [src/command_catalog.rs](../../../crates/core/src/command_catalog.rs) | [哪些操作受模板代次围栏约束](engine-and-ports.md#file-src-command-catalog-rs) |
| [src/concurrency.rs](../../../crates/core/src/concurrency.rs) | Storage-independent, resource-level optimistic commit contracts. |
| [src/configuration.rs](../../../crates/core/src/configuration.rs) | [配置可见性、组合与引用使用方查询](configuration-and-catalog.md#file-src-configuration-rs) |
| [src/configuration_catalog.rs](../../../crates/core/src/configuration_catalog.rs) | A disposable, configuration-only read model. Execution facts remain in EngineeringStore. |
| [src/configuration_changes.rs](../../../crates/core/src/configuration_changes.rs) | [同一候选图中的预演、诊断和原子提交](configuration-and-catalog.md#file-src-configuration-changes-rs) |
| [src/configuration_publication.rs](../../../crates/core/src/configuration_publication.rs) | Publication of a complete configuration candidate. Only configuration records are compared; execution history is never loaded or diffed here. |
| [src/configuration_queries.rs](../../../crates/core/src/configuration_queries.rs) | Disposable query projections. Exact source revisions remain the authority for editing and execution. |
| [src/configuration_transfers.rs](../../../crates/core/src/configuration_transfers.rs) | [共享发布与项目采用的跨库恢复协议](configuration-and-catalog.md#file-src-configuration-transfers-rs) |
| [src/configured_runtime.rs](../../../crates/core/src/configured_runtime.rs) | [冻结实例的主状态机与任务渲染](workflow-execution.md#file-src-configured-runtime-rs) |
| [src/configured_workflows.rs](../../../crates/core/src/configured_workflows.rs) | [发布完整配置闭包并生成内存执行计划](workflow-compilation.md#file-src-configured-workflows-rs) |
| [src/controller_calls.rs](../../../crates/core/src/controller_calls.rs) | Native controller actions retain literal arguments and their original write versions. Results and reply acknowledgements have immutable journals. |
| [src/controller_definitions.rs](../../../crates/core/src/controller_definitions.rs) | Controller configuration is derived from the complete source catalog and explicit actual work/root facts. It does not inspect runtime history. |
| [src/controller_event_tests.rs](../../../crates/core/src/controller_event_tests.rs) | `row`、`state`、`args` |
| [src/controller_events.rs](../../../crates/core/src/controller_events.rs) | Commit-time workflow facts. Notifications never choose a business action. |
| [src/controller_interaction_reads.rs](../../../crates/core/src/controller_interaction_reads.rs) | Human-facing projections from exact workflow resources, never an incomplete EngineeringState. Display witnesses do not replace command authorization. |
| [src/controller_interactions.rs](../../../crates/core/src/controller_interactions.rs) | Human statements and immutable interaction evidence. |
| [src/controller_lifecycle.rs](../../../crates/core/src/controller_lifecycle.rs) | Human manager lifecycle commands carry explicit owner/controller versions. Successor snapshots are sealed through the same durable artifact protocol. |
| [src/controller_runtime.rs](../../../crates/core/src/controller_runtime.rs) | The controller coordinator observes one bounded actual work slice at a time. It grants a durable original Session/input intent before Host materialization. |
| [src/controllers.rs](../../../crates/core/src/controllers.rs) | [原生经理会话、唤醒和控制调用围栏](orchestration-and-drivers.md#file-src-controllers-rs) |
| [src/conversation_admission.rs](../../../crates/core/src/conversation_admission.rs) | One finite input admission shared by native discussion and formal callers. |
| [src/conversation_captures.rs](../../../crates/core/src/conversation_captures.rs) | Completed conversation snapshots are derived from explicit, bounded facts. Durable capture attempts keep artifact I/O out of business transactions. |
| [src/conversation_controls.rs](../../../crates/core/src/conversation_controls.rs) | Human conversation control and provider admissions share native Turn CAS. |
| [src/conversation_observations.rs](../../../crates/core/src/conversation_observations.rs) | Original worker observations are admitted against finite conversation facts. No conversation history or partial EngineeringState is used by this path. |
| [src/conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs) | [派发原生 Turn、批量观察与真实终态裁定](conversations-and-history.md#file-src-conversation-runtime-rs) |
| [src/conversation_start.rs](../../../crates/core/src/conversation_start.rs) | Once-admitted worker opening and input dispatch with original-version callbacks. |
| [src/conversation_submission.rs](../../../crates/core/src/conversation_submission.rs) | Discussion submission freezes a bounded committed context and two exact local files before one Session/Turn admission. Formal task inputs use their separate original workflow/controller authority. |
| [src/conversation_workers.rs](../../../crates/core/src/conversation_workers.rs) | A worker stop is a durable, once-admitted external attempt. Its original proof retires only that worker, independently of later Turn decisions. |
| [src/conversations.rs](../../../crates/core/src/conversations.rs) | [会话打开、排队输入、交互与快照](conversations-and-history.md#file-src-conversations-rs) |
| [src/credential_updates.rs](../../../crates/core/src/credential_updates.rs) | Credential publication is a versioned binding to a verified immutable vault candidate. |
| [src/default_permissions.rs](../../../crates/core/src/default_permissions.rs) | [给当前出厂目标补齐默认绑定](session-resources-and-permissions.md#file-src-default-permissions-rs) |
| [src/delivery_recovery.rs](../../../crates/core/src/delivery_recovery.rs) | Reconciliation may authorize local sync after positive remote merge proof. This continuation has no operation capable of publishing or merging a PR. |
| [src/driver_capabilities.rs](../../../crates/core/src/driver_capabilities.rs) | [实际驱动能力的进程级事实](session-resources-and-permissions.md#file-src-driver-capabilities-rs) |
| [src/environment.rs](../../../crates/core/src/environment.rs) | [可复现的 ID 与时间来源](engine-and-ports.md#file-src-environment-rs) |
| [src/experience_directory.rs](../../../crates/core/src/experience_directory.rs) | Eventual, version-monotone metadata projection. It never admits execution or replaces authoritative snapshot contents/history at the source namespace. |
| [src/formal_inputs.rs](../../../crates/core/src/formal_inputs.rs) | Formal input uses its actual immutable workflow grant or an explicit Human supplement to a waiting step. No coordinator is recorded as a Human. |
| [src/formal_sessions.rs](../../../crates/core/src/formal_sessions.rs) | A formal Session freezes its original native grant before runtime setup. Configuration publication during that setup cannot silently select a new Agent, permission pair, integration closure, Actor, or parent execution. |
| [src/frozen_views.rs](../../../crates/core/src/frozen_views.rs) | [从历史源选择纯重建 Plan 与任务](workflow-compilation.md#file-src-frozen-views-rs) |
| [src/governance.rs](../../../crates/core/src/governance.rs) | [单项源命令、Agent 实例和工程配置冻结](configuration-and-catalog.md#file-src-governance-rs) |
| [src/human_governance.rs](../../../crates/core/src/human_governance.rs) | Business decisions remain human-owned regardless of the selected driver policy. |
| [src/in_flight.rs](../../../crates/core/src/in_flight.rs) | In-process technical attempt registry; persistent intents retain authority. |
| [src/integration_operations.rs](../../../crates/core/src/integration_operations.rs) | Only a newly applied intent authorizes its discovery/import attempt; observations are immutable and completion never refreshes its original guards. |
| [src/integrations.rs](../../../crates/core/src/integrations.rs) | [外部工具冻结、调用状态和接纳回执](integrations-and-repositories.md#file-src-integrations-rs) |
| [src/interaction_delivery.rs](../../../crates/core/src/interaction_delivery.rs) | Human answers are delivered by the actual native worker, once per original answer revision. An unknown reply never acquires another external attempt. |
| [src/invocation.rs](../../../crates/core/src/invocation.rs) | [调用者身份不能来自 RPC 自填](engine-and-ports.md#file-src-invocation-rs) |
| [src/lib.rs](../../../crates/core/src/lib.rs) | [命令事务、总分派与恢复入口](engine-and-ports.md#file-src-lib-rs) |
| [src/live_runs.rs](../../../crates/core/src/live_runs.rs) | [给会话选择代码视图并停止本进程执行者](workflow-execution.md#file-src-live-runs-rs) |
| [src/manager_publication.rs](../../../crates/core/src/manager_publication.rs) | The actual controller/native call grants one precise configuration action. Cross-store completion consumes that immutable grant and original sources. |
| [src/manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) | [固定项目的知识查询与配置创作](manager-and-platform-tools.md#file-src-manager-workspace-rs) |
| [src/material_commands.rs](../../../crates/core/src/material_commands.rs) | Human material and manual context edits use explicit ownership facts. File sealing is outside SQL and retains the original finite admission plan. |
| [src/native_context_calls.rs](../../../crates/core/src/native_context_calls.rs) | Resource-owned read intents and once-admitted native context replies. |
| [src/orchestration.rs](../../../crates/core/src/orchestration.rs) | [显式控制、实际接纳、材料读取与动作凭据](orchestration-and-drivers.md#file-src-orchestration-rs) |
| [src/permission_resources.rs](../../../crates/core/src/permission_resources.rs) | [Profile 修订、导入导出与绑定 CAS](session-resources-and-permissions.md#file-src-permission-resources-rs) |
| [src/permission_switch.rs](../../../crates/core/src/permission_switch.rs) | [以可恢复后继替换会话权限](session-resources-and-permissions.md#file-src-permission-switch-rs) |
| [src/platform_cleanup.rs](../../../crates/core/src/platform_cleanup.rs) | Test disposal has its own once-only grant. Missing acknowledgements use read-only inspection and never trigger another cleanup operation. |
| [src/platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) | Frozen source publication, PR preparation, observed merge and local sync. Every physical stage retains its own grant and raw observation. |
| [src/platform_delivery_projection.rs](../../../crates/core/src/platform_delivery_projection.rs) | The source Feature retains validation materials. Only its reviewed code projection is published as a mainline PR, after all source gates have passed. |
| [src/platform_facts.rs](../../../crates/core/src/platform_facts.rs) | A platform action's actual, finite authorization facts and original updates. |
| [src/platform_observations.rs](../../../crates/core/src/platform_observations.rs) | Platform requests are admitted from the literal provider journal against actual worker/Turn/step/controller leases. Rejections are kernel facts. |
| [src/platform_preparation.rs](../../../crates/core/src/platform_preparation.rs) | Freeze a platform request before image preparation. A later live lease may admit that same request; it never recomputes arguments or changes its origin. |
| [src/platform_publish.rs](../../../crates/core/src/platform_publish.rs) | Each remote write has one durable grant. PR polling is read only; publishing a candidate never advances or merges the remote target branch. |
| [src/platform_reads.rs](../../../crates/core/src/platform_reads.rs) | Read-only platform tools use exact source contracts and selected records. |
| [src/platform_reconciliation.rs](../../../crates/core/src/platform_reconciliation.rs) | A new explicit inspection may adopt a positive original physical receipt. It cannot dispatch the old operation or overwrite newer business resources. |
| [src/platform_replies.rs](../../../crates/core/src/platform_replies.rs) | Kernel platform replies have a separate once-only delivery grant and raw acknowledgement. A missing acknowledgement never grants another send. |
| [src/platform_runtime.rs](../../../crates/core/src/platform_runtime.rs) | Once-granted platform effects. Physical observations are immutable and survive rejection by the original resource-level admission conditions. |
| [src/platform_tools.rs](../../../crates/core/src/platform_tools.rs) | [工作空间、任务、测试和 Git 的受管处理器](manager-and-platform-tools.md#file-src-platform-tools-rs) |
| [src/ports.rs](../../../crates/core/src/ports.rs) | [应用需要外部世界提供的完整合同](engine-and-ports.md#file-src-ports-rs) |
| [src/project_creation.rs](../../../crates/core/src/project_creation.rs) | Creation after the Host has durably reserved this project's identity. The registration/provisioning lifecycle is separate from this atomic metadata. |
| [src/project_reservation.rs](../../../crates/core/src/project_reservation.rs) | Durable project identity reservation. Provisioning happens after this finite insert; retries and competing submissions return the same reserved database. |
| [src/project_standards.rs](../../../crates/core/src/project_standards.rs) | Materialize project choices into immutable source revisions; execution never applies overlays. |
| [src/project_updates.rs](../../../crates/core/src/project_updates.rs) | Finite Project metadata preparation. This use case is kept off the legacy dispatcher until every writer sharing Project invariants has cut over. |
| [src/proposal_commands.rs](../../../crates/core/src/proposal_commands.rs) | Proposals freeze a real completed discussion and an exact target revision. Extraction and adoption never grant workflow or platform evidence. |
| [src/provider_tools.rs](../../../crates/core/src/provider_tools.rs) | Native worker tool requests cross a durable observation/admission boundary. Context/platform calls retain a guarded bridge until their writer family moves. |
| [src/publication_retry.rs](../../../crates/core/src/publication_retry.rs) | Explicit retries may adopt an already completed publication from their own exact lineage. Old cancellation and physical operation identities stay intact. |
| [src/read_models.rs](../../../crates/core/src/read_models.rs) | Bounded display projections. These values never authorize workflow actions. |
| [src/repository_dispatch.rs](../../../crates/core/src/repository_dispatch.rs) | Versioned dispatch and immutable observations for new repository intents. This protocol is not wired into the legacy scheduler: its authoritative projection and every writer sharing repository constraints must cut over too. |
| [src/repository_projection.rs](../../../crates/core/src/repository_projection.rs) | Finite authoritative projections for the new repository protocol. Public dispatch/scheduling remains disabled until shared writers cut over together. |
| [src/repository_runtime.rs](../../../crates/core/src/repository_runtime.rs) | Repository effects receive a once-only native grant. Their literal raw observations survive conflicts with the original admission versions. |
| [src/repository_updates.rs](../../../crates/core/src/repository_updates.rs) | Explicit repository preparation contexts. Public dispatch remains disabled until repository callbacks and their shared business constraints cut over. |
| [src/resource_receipts.rs](../../../crates/core/src/resource_receipts.rs) | [应用侧掌握经验导入和 Harness 物化事实](session-resources-and-permissions.md#file-src-resource-receipts-rs) |
| [src/rpc_contracts.json](../../../crates/core/src/rpc_contracts.json) | src/rpc_contracts.json |
| [src/rpc_contracts.rs](../../../crates/core/src/rpc_contracts.rs) | Explicit ingress inventory shared by admission and regression checks. |
| [src/runtime_callbacks.rs](../../../crates/core/src/runtime_callbacks.rs) | Bounded recovery of already granted operations. A persisted observation can be admitted again; absence of an observation never grants external I/O. |
| [src/runtime_registry.rs](../../../crates/core/src/runtime_registry.rs) | Immutable installation registration has its own identity and absence guard. |
| [src/sealed_commands.rs](../../../crates/core/src/sealed_commands.rs) | Local material writes retain their exact finite admission plan across file I/O. Recovery authenticates the original file identity before admitting it. |
| [src/session_assembly.rs](../../../crates/core/src/session_assembly.rs) | [把完整 Agent 装配成可执行 Session](session-resources-and-permissions.md#file-src-session-assembly-rs) |
| [src/session_commands.rs](../../../crates/core/src/session_commands.rs) | Public discussion Session admission uses native ownership, immutable Agent sources and the actual predecessor. Formal sessions retain their own grants. |
| [src/settings.rs](../../../crates/core/src/settings.rs) | Global preferences use independent resource revisions, never a project revision. |
| [src/spec_lint.rs](../../../crates/core/src/spec_lint.rs) | [结构预览与实际产物接纳共用的检查链](manager-and-platform-tools.md#file-src-spec-lint-rs) |
| [src/standards.rs](../../../crates/core/src/standards.rs) | [规范快照发布与每个工作的独立采用](configuration-and-catalog.md#file-src-standards-rs) |
| [src/templates.rs](../../../crates/core/src/templates.rs) | [外层资源注入与模板目录重置](configuration-and-catalog.md#file-src-templates-rs) |
| [src/tool_calls.rs](../../../crates/core/src/tool_calls.rs) | Resource-owned tool lifecycle. Only a fresh durable claim grants external I/O. |
| [src/tool_creation.rs](../../../crates/core/src/tool_creation.rs) | Manual and workflow tool admission. Configuration validation uses the complete catalog; original source references and absence predicates commit together. |
| [src/work_owners.rs](../../../crates/core/src/work_owners.rs) | Explicit ownership facts shared by material and conversation use cases. |
| [src/worker_reconciliation.rs](../../../crates/core/src/worker_reconciliation.rs) | Explicit lifecycle recovery receives a distinct once-only stop grant. Recovery of that grant inspects its original worker and never resends it. |
| [src/workflow_authorization.rs](../../../crates/core/src/workflow_authorization.rs) | Invocation authority is checked from real driver/controller leases and exact human decisions, independently of the workflow's business rules. |
| [src/workflow_budget.rs](../../../crates/core/src/workflow_budget.rs) | Budget time is the union of real pause/wait intervals. Overlapping parent, child and human waits are counted once; an open interval causes no tick writes. |
| [src/workflow_change_commit.rs](../../../crates/core/src/workflow_change_commit.rs) | Durable plan activation owns only the selected plan, executions and Tasks. Stop requests and immutable observations precede the final optimistic commit. |
| [src/workflow_change_preview.rs](../../../crates/core/src/workflow_change_preview.rs) | Plan preview over a complete configuration catalog and explicitly selected actual runtime facts. No whole-project snapshot or synthetic runtime state. |
| [src/workflow_children.rs](../../../crates/core/src/workflow_children.rs) | Foreach expands the declared collection into actual child roots in one native grant. Work bindings validate each selected owner and frozen catalog. |
| [src/workflow_commits.rs](../../../crates/core/src/workflow_commits.rs) | Explicit workflow resources and mutations. A step update never replaces a sibling, and a header update never writes the steps array. |
| [src/workflow_control_reads.rs](../../../crates/core/src/workflow_control_reads.rs) | Exact read-only control facts do not reconstruct an unrelated frozen plan. |
| [src/workflow_controls.rs](../../../crates/core/src/workflow_controls.rs) | Public workflow controls retain their original draft and native versions. |
| [src/workflow_delivery_migration.rs](../../../crates/core/src/workflow_delivery_migration.rs) | Human-requested transport migration keeps the logical work and its source evidence, while replacing the publication and every downstream decision. |
| [src/workflow_detail.rs](../../../crates/core/src/workflow_detail.rs) | Explicit display facts for one instance. No aggregate runtime restoration. |
| [src/workflow_dispatch.rs](../../../crates/core/src/workflow_dispatch.rs) | Dispatch prepares one actual execution and its durable runtime commands. Source compilation is configuration-only. Writer preflight only reads Git; external mutations remain in durable runtime commands. |
| [src/workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) | [Rust 根驱动的动作选择、预算与围栏](orchestration-and-drivers.md#file-src-workflow-driver-rs) |
| [src/workflow_driving.rs](../../../crates/core/src/workflow_driving.rs) | Rust and delegated drivers issue the same public resource-level commands. Native leases and bounded cursors replace aggregate writer transactions. |
| [src/workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs) | [查询与提交共用的业务动作资格](orchestration-and-drivers.md#file-src-workflow-eligibility-rs) |
| [src/workflow_evaluation.rs](../../../crates/core/src/workflow_evaluation.rs) | Bounded projection of actual runtime receipts into their original execution. Business dispatch/acceptance remains an explicit versioned command. |
| [src/workflow_execution_controls.rs](../../../crates/core/src/workflow_execution_controls.rs) | Execution controls revoke original grants and retain the exact old attempt. Requesting stop never manufactures a physical stop or remote-effect proof. |
| [src/workflow_facts.rs](../../../crates/core/src/workflow_facts.rs) | Explicit rule inputs, selected native executions and immutable observations. A WorkflowFacts is the rule's declared interface, never a partial store state. |
| [src/workflow_failure.rs](../../../crates/core/src/workflow_failure.rs) | [从持久化来源恢复失败与独立尝试计数](workflow-execution.md#file-src-workflow-failure-rs) |
| [src/workflow_material_inputs.rs](../../../crates/core/src/workflow_material_inputs.rs) | Platform material is supplemental evidence, never a Human answer or approval. |
| [src/workflow_materials.rs](../../../crates/core/src/workflow_materials.rs) | Bounded material metadata and exact, explicitly selected bodies. |
| [src/workflow_observation_facts.rs](../../../crates/core/src/workflow_observation_facts.rs) | Actual ancestors, input-runtime evidence and immutable continuation inputs. |
| [src/workflow_outputs.rs](../../../crates/core/src/workflow_outputs.rs) | Output acceptance validates literal producer facts, prepares named files, and seals them outside SQL under the original finite admission plan. |
| [src/workflow_readiness.rs](../../../crates/core/src/workflow_readiness.rs) | Bounded control queries and readiness use actual selected workflow facts. Display eligibility is advisory; every command repeats authoritative CAS. |
| [src/workflow_reading.rs](../../../crates/core/src/workflow_reading.rs) | Scope-aware readers backed by bounded native projections. |
| [src/workflow_reconciliation.rs](../../../crates/core/src/workflow_reconciliation.rs) | An explicit reconciliation is a new, versioned authorization. Its immutable observations never rewrite the old callback's versions or resend its effect. |
| [src/workflow_resources.rs](../../../crates/core/src/workflow_resources.rs) | Actual workflow resources, with independent native step/header epochs. |
| [src/workflow_retries.rs](../../../crates/core/src/workflow_retries.rs) | A retry/rework creates new execution identities and archives each original attempt. Unknown effects remain behind an explicit reconciliation request. |
| [src/workflow_review.rs](../../../crates/core/src/workflow_review.rs) | Exact-result exceptions. A manager can propose, only Human can sign. |
| [src/workflow_rework.rs](../../../crates/core/src/workflow_rework.rs) | Business rework uses immutable previews and a shared logical-work CAS budget. |
| [src/workflow_rule_hints.rs](../../../crates/core/src/workflow_rule_hints.rs) | Eventually consistent display hints retain their exact native witnesses. They never authorize a command; control admissions repeat the actual rules. |
| [src/workflow_rules.rs](../../../crates/core/src/workflow_rules.rs) | Business rules consume explicit facts. Activity and waivers contribute their actual resource witnesses to the same optimistic commit. |
| [src/workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) | [执行意图的领取和派发结果登记](workflow-execution.md#file-src-workflow-runtime-rs) |
| [src/workflow_runtime_facts.rs](../../../crates/core/src/workflow_runtime_facts.rs) | [把真实外层运行身份交给后继任务](workflow-execution.md#file-src-workflow-runtime-facts-rs) |
| [src/workflow_segments.rs](../../../crates/core/src/workflow_segments.rs) | Finite delegation consumes its own operation budget in the step commit. |
| [src/workflow_start.rs](../../../crates/core/src/workflow_start.rs) | Work start compiles only the complete configuration catalog. Native owner and active-root predicates protect creation, plan publication and source heads. |
| [src/workspace_commands.rs](../../../crates/core/src/workspace_commands.rs) | Workspace metadata and new work are admitted from explicit native facts. A Feature and its frozen initial plan are created in one atomic commit. |
| [src/workspace_continuation.rs](../../../crates/core/src/workspace_continuation.rs) | Preserve quiescent, uncommitted work between Turns of the same execution. |

<!-- crate-files:end -->

<a id="file-cargo-toml"></a>

## Cargo.toml：应用层包与依赖注入的构建约束

源码：[crates/core/Cargo.toml](../../../crates/core/Cargo.toml)。

`devdesk-core` 只依赖 domain、serde/serde_json 和 uuid，使用 workspace 的版本与 edition。它没有 rusqlite、Docker 客户端、HTTP SDK 或出厂资源 build.rs；这些能力由外层实现并注入。

uuid 用于应用层默认身份来源，确定性测试可以通过 ApplicationEnvironment 替换。新增用例若需要访问环境，应先扩展或复用端口，而不是让 core 引入适配器具体类型。


---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
