# 文件索引

返回 [adapters 概览](overview.md)。本表用于查文件，功能过程请先读概览中的专题。

由 `python3 scripts/py/check-docs.py --write` 更新；保留已有的人工专题链接。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/adapters/Cargo.toml) | Cargo.toml |
| [build.rs](../../../crates/adapters/build.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [examples/export_runtime_recipes.rs](../../../crates/adapters/examples/export_runtime_recipes.rs) | Export current factory recipes for native acceptance without duplicating the compiler. |
| [runtime/tool-worker.mjs](../../../crates/adapters/runtime/tool-worker.mjs) | [HTTP、MCP、OAuth 与 Skill 接入](external-integrations.md) |
| [src/catalog.rs](../../../crates/adapters/src/catalog.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/codex_auth.rs](../../../crates/adapters/src/codex_auth.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [src/configuration_backend.rs](../../../crates/adapters/src/configuration_backend.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/configuration_cache.rs](../../../crates/adapters/src/configuration_cache.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/configuration_reset.rs](../../../crates/adapters/src/configuration_reset.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/connection_pool.rs](../../../crates/adapters/src/connection_pool.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/context_calls.rs](../../../crates/adapters/src/context_calls.rs) | Exact context invocation and live caller witnesses, without aggregate restore. |
| [src/conversation_captures.rs](../../../crates/adapters/src/conversation_captures.rs) | Bounded facts for a completed conversation's derived snapshot. |
| [src/conversation_queries.rs](../../../crates/adapters/src/conversation_queries.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/conversation_reads.rs](../../../crates/adapters/src/conversation_reads.rs) | Native conversation polling reads one Turn's metadata and original worker authority. It never restores input bodies, plans or unrelated history. |
| [src/conversation_sources.rs](../../../crates/adapters/src/conversation_sources.rs) | Exact frozen facts for one initial or resumed conversation dispatch. |
| [src/credentials.rs](../../../crates/adapters/src/credentials.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [src/credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [src/docker.rs](../../../crates/adapters/src/docker.rs) | [Harness 协议与 Docker 容器](harness-and-protocols.md) |
| [src/domain_packages/worker.rs](../../../crates/adapters/src/domain_packages/worker.rs) | Bounded read-only domain process transport for the first extension slice. |
| [src/domain_packages.rs](../../../crates/adapters/src/domain_packages.rs) | Installed domain bundles and the active generation, scoped to one dataset. |
| [src/execution_read.rs](../../../crates/adapters/src/execution_read.rs) | Bounded execution-page facts, read from one committed snapshot. |
| [src/experience.rs](../../../crates/adapters/src/experience.rs) | Read-only, AgentID-scoped projection over authoritative project stores. |
| [src/experience_directory.rs](../../../crates/adapters/src/experience_directory.rs) | Bounded display metadata from authoritative experience rows. No source view reconstruction, execution history or snapshot body is read here. |
| [src/experience_index.rs](../../../crates/adapters/src/experience_index.rs) | Read experience metadata and immutable snapshot text without rebuilding execution history. |
| [src/factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/git.rs](../../../crates/adapters/src/git.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/git_delivery_projection.rs](../../../crates/adapters/src/git_delivery_projection.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/git_diff.rs](../../../crates/adapters/src/git_diff.rs) | Compare committed Git objects, bounding patch expansion before reading blobs. Large objects remain addressable by their exact names and object identities. |
| [src/git_test_configuration.rs](../../../crates/adapters/src/git_test_configuration.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/git_workspace_continuation.rs](../../../crates/adapters/src/git_workspace_continuation.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/github.rs](../../../crates/adapters/src/github.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/github_auth.rs](../../../crates/adapters/src/github_auth.rs) | `SavedCredential`、`GitHubAuthStatus`、`prepare_command` |
| [src/github_delivery.rs](../../../crates/adapters/src/github_delivery.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/harness.rs](../../../crates/adapters/src/harness.rs) | [Harness 协议与 Docker 容器](harness-and-protocols.md) |
| [src/harness_startup.rs](../../../crates/adapters/src/harness_startup.rs) | [Harness 协议与 Docker 容器](harness-and-protocols.md) |
| [src/integration_oauth.rs](../../../crates/adapters/src/integration_oauth.rs) | [HTTP、MCP、OAuth 与 Skill 接入](external-integrations.md) |
| [src/integrations.rs](../../../crates/adapters/src/integrations.rs) | [HTTP、MCP、OAuth 与 Skill 接入](external-integrations.md) |
| [src/jsonl.rs](../../../crates/adapters/src/jsonl.rs) | [Harness 协议与 Docker 容器](harness-and-protocols.md) |
| [src/lib.rs](../../../crates/adapters/src/lib.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/model_providers.rs](../../../crates/adapters/src/model_providers.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/named_artifacts.rs](../../../crates/adapters/src/named_artifacts.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/native_command_output.rs](../../../crates/adapters/src/native_command_output.rs) | Immutable, sanitized command results for the originating worker. This is a bounded projection of its protocol journal, never a platform test receipt. |
| [src/operational_reads.rs](../../../crates/adapters/src/operational_reads.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/payload_store.rs](../../../crates/adapters/src/payload_store.rs) | Project-local storage for immutable bodies selected by the SQLite codec. |
| [src/performance/transactions.rs](../../../crates/adapters/src/performance/transactions.rs) | Opt-in individual transaction boundaries, with bounded identities and loss accounting. |
| [src/performance.rs](../../../crates/adapters/src/performance.rs) | Opt-in, bounded request measurements. No parameters or business content are recorded. |
| [src/permission_profiles.rs](../../../crates/adapters/src/permission_profiles.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [src/permissions.rs](../../../crates/adapters/src/permissions.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [src/process.rs](../../../crates/adapters/src/process.rs) | Bounded coordinator subprocesses. No shell interpolation or raw external errors. |
| [src/project_migration.rs](../../../crates/adapters/src/project_migration.rs) | Explicit offline project database upgrades. The original remains usable until the candidate and every new payload reference have been checked. |
| [src/project_records.rs](../../../crates/adapters/src/project_records.rs) | Metadata pages and exact records in one project. No aggregate state restore. |
| [src/project_storage.rs](../../../crates/adapters/src/project_storage.rs) | Physical representation of immutable snapshot bodies and committed receipts. |
| [src/read_models.rs](../../../crates/adapters/src/read_models.rs) | Indexed display reads; no EngineeringState construction or source expansion. |
| [src/repository_contexts.rs](../../../crates/adapters/src/repository_contexts.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/resource_commits.rs](../../../crates/adapters/src/resource_commits.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/resource_read_scope.rs](../../../crates/adapters/src/resource_read_scope.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/runtime_images.rs](../../../crates/adapters/src/runtime_images.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/runtime_preparation.rs](../../../crates/adapters/src/runtime_preparation.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/runtime_queries.rs](../../../crates/adapters/src/runtime_queries.rs) | Workflow scheduling reads through the shared pool, without restoring history. |
| [src/runtime_registry.rs](../../../crates/adapters/src/runtime_registry.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/runtime_verification.rs](../../../crates/adapters/src/runtime_verification.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/scheduling_read.rs](../../../crates/adapters/src/scheduling_read.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/session_cache.rs](../../../crates/adapters/src/session_cache.rs) | Session caches use an index while idle and private files while a worker runs. |
| [src/session_resources.rs](../../../crates/adapters/src/session_resources.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [src/source_dependencies.rs](../../../crates/adapters/src/source_dependencies.rs) | Witnesses for the exact source revisions consumed by stored governance bodies. |
| [src/source_store.rs](../../../crates/adapters/src/source_store.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/source_views.rs](../../../crates/adapters/src/source_views.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [src/standard_reads.rs](../../../crates/adapters/src/standard_reads.rs) | Read adoption records without reconstructing the execution history. |
| [src/storage_maintenance.rs](../../../crates/adapters/src/storage_maintenance.rs) | Host-owned cache storage. Cached files linked to an idle session are materialized privately before Docker can write to them. |
| [src/store_changes.rs](../../../crates/adapters/src/store_changes.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/tests.rs](../../../crates/adapters/src/tests.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [src/tool_calls.rs](../../../crates/adapters/src/tool_calls.rs) | Select a finite set of native tool rows and restore their exact frozen views. |
| [src/workflow_details.rs](../../../crates/adapters/src/workflow_details.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/workflow_events.rs](../../../crates/adapters/src/workflow_events.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/workflow_intents.rs](../../../crates/adapters/src/workflow_intents.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/workflow_observations.rs](../../../crates/adapters/src/workflow_observations.rs) | Selected terminal ledger values and actual stopped-worker evidence. Normal workflow observation does not load conversation or execution history. |
| [src/workflow_projection.rs](../../../crates/adapters/src/workflow_projection.rs) | Transactional display metadata. Canonical runtime facts remain resource rows; these small projections never authorize control or replace sources. |
| [src/workflow_readers.rs](../../../crates/adapters/src/workflow_readers.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [src/workflow_stop_events.rs](../../../crates/adapters/src/workflow_stop_events.rs) | A successful native terminal and its admitted stop proof are ordinary progress. Output admission can follow cleanup seconds later; it still performs all checks. |
| [src/workspace_read.rs](../../../crates/adapters/src/workspace_read.rs) | Legacy workspace shape over bounded committed rows. Source and runtime history bodies are loaded only by their explicit readers. |
| [tests/advisory_read_bounds.rs](../../../crates/adapters/tests/advisory_read_bounds.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [tests/agent_capabilities.rs](../../../crates/adapters/tests/agent_capabilities.rs) | `CallingHarness`、`open_session`、`dispatch_turn` |
| [tests/catalog_commit_cache.rs](../../../crates/adapters/tests/catalog_commit_cache.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [tests/collection_versions.rs](../../../crates/adapters/tests/collection_versions.rs) | Predicate tokens are maintained by SQLite for every fact writer; no I/O or business lock is acquired by this read-set protection. |
| [tests/command_tests.rs](../../../crates/adapters/tests/command_tests.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [tests/configuration_catalog.rs](../../../crates/adapters/tests/configuration_catalog.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [tests/configuration_changes.rs](../../../crates/adapters/tests/configuration_changes.rs) | `engine`、`source`、`spec` |
| [tests/configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs) | `engine`、`project_engine`、`engine_with_store` |
| [tests/configuration_simplification.rs](../../../crates/adapters/tests/configuration_simplification.rs) | Contracts for Profile-only permissions, owned installation inputs and Workflow scheduling. |
| [tests/configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs) | `engine`、`command`、`publish` |
| [tests/conversations.rs](../../../crates/adapters/tests/conversations.rs) | `engine`、`pending_startups_reserve_slots_and_cancel_without_sending_input`、`Deferred` |
| [tests/credential_updates.rs](../../../crates/adapters/tests/credential_updates.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [tests/dataset_preflight.rs](../../../crates/adapters/tests/dataset_preflight.rs) | [适配器工厂与配置装载](factory-and-configuration.md) |
| [tests/delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs) | Explicit local acceptance using the production Git and Docker test adapters. No model, credentials, remote repository, or user dataset is used. |
| [tests/delivery_migration.rs](../../../crates/adapters/tests/delivery_migration.rs) | Explicit offline migration check against an isolated credential-free copy. |
| [tests/directory_publication.rs](../../../crates/adapters/tests/directory_publication.rs) | Directory snapshots are derived observations, not authoritative project edits. |
| [tests/docker_resources.rs](../../../crates/adapters/tests/docker_resources.rs) | [Harness 协议与 Docker 容器](harness-and-protocols.md) |
| [tests/experience_directory.rs](../../../crates/adapters/tests/experience_directory.rs) | Derived metadata only; these fixtures do not run a workflow or a worker. |
| [tests/experience_imports.rs](../../../crates/adapters/tests/experience_imports.rs) | Insert-only history import. Ports are test doubles; no external model is run. |
| [tests/frozen_history_performance.rs](../../../crates/adapters/tests/frozen_history_performance.rs) | Read-only codec benchmark over an explicitly selected backup, never worker recovery. |
| [tests/git_resources.rs](../../../crates/adapters/tests/git_resources.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [tests/github_resources.rs](../../../crates/adapters/tests/github_resources.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |
| [tests/governance.rs](../../../crates/adapters/tests/governance.rs) | `engine`、`project_engine`、`engine_with_store` |
| [tests/integration_operations.rs](../../../crates/adapters/tests/integration_operations.rs) | [HTTP、MCP、OAuth 与 Skill 接入](external-integrations.md) |
| [tests/manager_workspace.rs](../../../crates/adapters/tests/manager_workspace.rs) | `Scripted`、`next`、`change` |
| [tests/materialization_receipts.rs](../../../crates/adapters/tests/materialization_receipts.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [tests/model_providers.rs](../../../crates/adapters/tests/model_providers.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [tests/orchestration.rs](../../../crates/adapters/tests/orchestration.rs) | `a_later_human_stop_survives_reconciliation_of_an_earlier_kernel_stop`、`engine`、`cmd` |
| [tests/permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [tests/permissions.rs](../../../crates/adapters/tests/permissions.rs) | [凭据与权限后端](credentials-and-permissions.md) |
| [tests/project_creation.rs](../../../crates/adapters/tests/project_creation.rs) | Scoped metadata admission only, not directory provisioning or workflow capacity. |
| [tests/project_reservations.rs](../../../crates/adapters/tests/project_reservations.rs) | Reservation admission only; no Host provisioning or worker is started. |
| [tests/project_standards.rs](../../../crates/adapters/tests/project_standards.rs) | Project standards are immutable effective graphs, with frozen consumers and durable provenance. |
| [tests/project_test_configuration.rs](../../../crates/adapters/tests/project_test_configuration.rs) | `git`、`discover`、`missing_repository_and_empty_repository_have_executable_default_recipes` |
| [tests/project_update_context.rs](../../../crates/adapters/tests/project_update_context.rs) | Isolated finite-use-case checks. The production dispatcher deliberately stays on the old path until every writer sharing Project invariants is migrated. |
| [tests/publication_retry.rs](../../../crates/adapters/tests/publication_retry.rs) | Opt-in recovery regression on a credential-free COPY of a real failed scene. This fixture controls its offline workflow explicitly; it is not evidence of autonomous manager recovery and must never point at a live dataset. |
| [tests/read_models.rs](../../../crates/adapters/tests/read_models.rs) | SQL projection fixtures deliberately contain an unreadable historical plan. They do not represent a dispatched worker or a workflow acceptance run. |
| [tests/repository_dispatch.rs](../../../crates/adapters/tests/repository_dispatch.rs) | Dispatch/observation protocol only. No Git process, worker or external effect. |
| [tests/repository_projection.rs](../../../crates/adapters/tests/repository_projection.rs) | Trusted-port receipt fixtures exercise atomic admission, not actual Git/GitHub. |
| [tests/repository_update_context.rs](../../../crates/adapters/tests/repository_update_context.rs) | Preparation only: SQL facts are fixtures, not actual Git receipts or a worker run. |
| [tests/resource_commits.rs](../../../crates/adapters/tests/resource_commits.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [tests/runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [tests/runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs) | [运行环境准备与验证](runtime-preparation.md) |
| [tests/schema_contracts.rs](../../../crates/adapters/tests/schema_contracts.rs) | `definition`、`round_trip`、`all_published_schemas_and_definitions_resolve_offline` |
| [tests/settings_concurrency.rs](../../../crates/adapters/tests/settings_concurrency.rs) | `save`、`independent_edits_commit_but_stale_same_field_cannot_overwrite`、`invalid_batches_and_missing_guards_leave_both_settings_unchanged` |
| [tests/standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs) | Snapshot and adoption contracts through production commands, native material tools and SQLite. |
| [tests/support/mod.rs](../../../crates/adapters/tests/support/mod.rs) | Resolve a test installation on this Docker platform, then freeze its content ID. |
| [tests/support/segment_actions.rs](../../../crates/adapters/tests/support/segment_actions.rs) | `delegated_completion_keeps_the_whole_workflow_rule`、`automated_delegation_does_not_wake_manager_for_ordinary_approval_or_completion`、`delegated_preparation_preserves_human_gate_and_completes_after_signature_across_restart` |
| [tests/tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs) | [HTTP、MCP、OAuth 与 Skill 接入](external-integrations.md) |
| [tests/workflow_performance.rs](../../../crates/adapters/tests/workflow_performance.rs) | Opt-in component measurements. No model, worker, network, or user dataset. |
| [tests/workflow_progress.rs](../../../crates/adapters/tests/workflow_progress.rs) | Read-model fixtures only. No workflow, model, Git or native worker is run. |
| [tests/workflow_resource_concurrency.rs](../../../crates/adapters/tests/workflow_resource_concurrency.rs) | Deterministic resource/reader acceptance. No production dataset or supplier. |
| [tests/workflow_storage_scaling.rs](../../../crates/adapters/tests/workflow_storage_scaling.rs) | [SQLite、资源提交与产物文件](storage-and-source-codecs.md) |
| [tests/workflows.rs](../../../crates/adapters/tests/workflows.rs) | `engine`、`revision`、`cmd` |
| [tests/worktree_receipt_concurrency.rs](../../../crates/adapters/tests/worktree_receipt_concurrency.rs) | [Git、测试和 GitHub 适配器](engineering-resources.md) |

<!-- crate-files:end -->
