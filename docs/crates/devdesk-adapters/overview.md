# devdesk-adapters：把端口落实为可核对的外部操作

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

adapters 实现 core 的端口：把工程状态写入 SQLite，把交付物封存成文件，把冻结任务交给真实容器和 Harness，并处理 Git、GitHub、系统凭据和外部工具。它负责证明“外部实际发生了什么”；是否接纳为业务成功，仍由 core 决定。

包里既有生产实现，也有跨层集成测试。测试可以注入 Harness/工具夹具来精确制造时序，但生产装配 [execution_adapters](storage-and-source-codecs.md#file-src-lib-rs) 使用实际执行器。测试目录中的普通离线测试、真实本地 Git 测试、显式 ignored 的 Docker/GitHub 测试应分开理解。

实现上的共同约束是：固定归属与内容身份，在操作前保存意图，在事务外执行，保留原操作回执；结果未知时核对已有资源，不换 ID 重试。存储、容器、原生协议各自有不同的“停止”含义，不能把杀掉一个客户端进程当作所有远端副作用已撤销。

## 专题阅读

- [SQLite、Artifact 与源视图恢复](storage-and-source-codecs.md)
- [出厂资源嵌入与配置后端](factory-and-configuration.md)
- [运行安装、准备意图与协议验证](runtime-preparation.md)
- [Harness、JSONL 与受管进程](harness-and-protocols.md)
- [凭据、登录与权限落地](credentials-and-permissions.md)
- [Git、GitHub、Docker 与测试执行](engineering-resources.md)
- [模型路由、外部工具与 OAuth](external-integrations.md)
- [集成测试的场景、夹具与真实边界](testing.md)

## 完整文件索引

由 `python3 scripts/py/check-docs.py --write` 更新。专题解释主要合同，表格覆盖当前全部文件。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/adapters/Cargo.toml) | [外部实现所需依赖和测试边界](overview.md#file-cargo-toml) |
| [build.rs](../../../crates/adapters/build.rs) | [编译时生成资源表与 Schema 校验器](factory-and-configuration.md#file-build-rs) |
| [examples/export_runtime_recipes.rs](../../../crates/adapters/examples/export_runtime_recipes.rs) | Export current factory recipes for native acceptance without duplicating the compiler. |
| [src/catalog.rs](../../../crates/adapters/src/catalog.rs) | [全局配置、项目目录与运行设置](storage-and-source-codecs.md#file-src-catalog-rs) |
| [src/codex_auth.rs](../../../crates/adapters/src/codex_auth.rs) | [数据集私有的 Codex 登录生命周期](credentials-and-permissions.md#file-src-codex-auth-rs) |
| [src/configuration_backend.rs](../../../crates/adapters/src/configuration_backend.rs) | [为配置预演提供实际后端事实](factory-and-configuration.md#file-src-configuration-backend-rs) |
| [src/configuration_cache.rs](../../../crates/adapters/src/configuration_cache.rs) | Bounded, process-local read models over the authoritative source tables. |
| [src/configuration_reset.rs](../../../crates/adapters/src/configuration_reset.rs) | Explicit destructive catalog reset; execution evidence becomes self-contained. |
| [src/connection_pool.rs](../../../crates/adapters/src/connection_pool.rs) | Bounded connection ownership. Pool mutexes never cover SQL or application work. |
| [src/context_calls.rs](../../../crates/adapters/src/context_calls.rs) | Exact context invocation and live caller witnesses, without aggregate restore. |
| [src/conversation_captures.rs](../../../crates/adapters/src/conversation_captures.rs) | Bounded facts for a completed conversation's derived snapshot. |
| [src/conversation_queries.rs](../../../crates/adapters/src/conversation_queries.rs) | Indexed Session pages and exact Turn/event reads. Runtime source expansion uses only the immutable configuration catalog after the read snapshot ends. |
| [src/conversation_reads.rs](../../../crates/adapters/src/conversation_reads.rs) | Native conversation polling reads one Turn's metadata and original worker authority. It never restores input bodies, plans or unrelated history. |
| [src/conversation_sources.rs](../../../crates/adapters/src/conversation_sources.rs) | Exact frozen facts for one initial or resumed conversation dispatch. |
| [src/credentials.rs](../../../crates/adapters/src/credentials.rs) | [数据集隔离的系统密钥引用](credentials-and-permissions.md#file-src-credentials-rs) |
| [src/credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) | [固定助手的安全安装、校验和调用](credentials-and-permissions.md#file-src-credentials-macos-rs) |
| [src/docker.rs](../../../crates/adapters/src/docker.rs) | [归属明确的容器与不扩权的挂载](engineering-resources.md#file-src-docker-rs) |
| [src/execution_read.rs](../../../crates/adapters/src/execution_read.rs) | Bounded execution-page facts, read from one committed snapshot. |
| [src/experience.rs](../../../crates/adapters/src/experience.rs) | [按 Agent 身份只读汇集跨项目经验](storage-and-source-codecs.md#file-src-experience-rs) |
| [src/experience_directory.rs](../../../crates/adapters/src/experience_directory.rs) | Bounded display metadata from authoritative experience rows. No source view reconstruction, execution history or snapshot body is read here. |
| [src/experience_index.rs](../../../crates/adapters/src/experience_index.rs) | Read experience metadata and immutable snapshot text without rebuilding execution history. |
| [src/factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) | [读取嵌入配置并一次性注入 core](factory-and-configuration.md#file-src-factory-bundle-rs) |
| [src/git.rs](../../../crates/adapters/src/git.rs) | [受管仓库、候选提交与精确引用推进](engineering-resources.md#file-src-git-rs) |
| [src/git_delivery_projection.rs](../../../crates/adapters/src/git_delivery_projection.rs) | Construct a code-only commit without checking out, editing, or advancing the source Feature, its Tasks, or main. No remote effect happens here. |
| [src/git_test_configuration.rs](../../../crates/adapters/src/git_test_configuration.rs) | Read bounded, tracked manifests from the selected mainline. Never execute project scripts here. |
| [src/git_workspace_continuation.rs](../../../crates/adapters/src/git_workspace_continuation.rs) | A native writer may continue only the exact quiescent work recorded by the kernel for that same Session, execution, frozen input and permission scope. |
| [src/github.rs](../../../crates/adapters/src/github.rs) | [稳定 PR 身份、仓库政策与回执核验](engineering-resources.md#file-src-github-rs) |
| [src/github_delivery.rs](../../../crates/adapters/src/github_delivery.rs) | PR publication observes existing repository rules without installing policy. Unprotected targets use an exact remote CAS; protected targets use PR gates. |
| [src/harness.rs](../../../crates/adapters/src/harness.rs) | [原生 worker、私有会话目录与真实停止证明](harness-and-protocols.md#file-src-harness-rs) |
| [src/harness_startup.rs](../../../crates/adapters/src/harness_startup.rs) | Native preparation for one already reserved execution. Threads never write engineering state or send a Turn; the core accepts the actual receipt first. |
| [src/integration_oauth.rs](../../../crates/adapters/src/integration_oauth.rs) | [PKCE、固定账户和未知交换结果](external-integrations.md#file-src-integration-oauth-rs) |
| [src/integrations.rs](../../../crates/adapters/src/integrations.rs) | [离线合同校验和持久化 HTTP/MCP 执行](external-integrations.md#file-src-integrations-rs) |
| [src/jsonl.rs](../../../crates/adapters/src/jsonl.rs) | [有界 JSONL 通道、请求关联与事件日志](harness-and-protocols.md#file-src-jsonl-rs) |
| [src/lib.rs](../../../crates/adapters/src/lib.rs) | [工程数据库、封存文件和生产端口装配](storage-and-source-codecs.md#file-src-lib-rs) |
| [src/model_providers.rs](../../../crates/adapters/src/model_providers.rs) | [冻结模型连接与只读模型目录](external-integrations.md#file-src-model-providers-rs) |
| [src/named_artifacts.rs](../../../crates/adapters/src/named_artifacts.rs) | Publish a preallocated local file without replacing an existing identity. |
| [src/native_command_output.rs](../../../crates/adapters/src/native_command_output.rs) | Immutable, sanitized command results for the originating worker. This is a bounded projection of its protocol journal, never a platform test receipt. |
| [src/operational_reads.rs](../../../crates/adapters/src/operational_reads.rs) | SQL selects the page before any source restoration or call-body decoding. |
| [src/performance/transactions.rs](../../../crates/adapters/src/performance/transactions.rs) | Opt-in individual transaction boundaries, with bounded identities and loss accounting. |
| [src/performance.rs](../../../crates/adapters/src/performance.rs) | Opt-in, bounded request measurements. No parameters or business content are recorded. |
| [src/permission_profiles.rs](../../../crates/adapters/src/permission_profiles.rs) | [权限预设导入、编译与内容身份](credentials-and-permissions.md#file-src-permission-profiles-rs) |
| [src/permissions.rs](../../../crates/adapters/src/permissions.rs) | [把可移植授权变成 Codex/DSH 和容器策略](credentials-and-permissions.md#file-src-permissions-rs) |
| [src/process.rs](../../../crates/adapters/src/process.rs) | [受管进程组、私有 stdin 和输出边界](harness-and-protocols.md#file-src-process-rs) |
| [src/project_records.rs](../../../crates/adapters/src/project_records.rs) | Metadata pages and exact records in one project. No aggregate state restore. |
| [src/read_models.rs](../../../crates/adapters/src/read_models.rs) | Indexed display reads; no EngineeringState construction or source expansion. |
| [src/repository_contexts.rs](../../../crates/adapters/src/repository_contexts.rs) | Exact entity rows and collection predicates share one committed read snapshot. |
| [src/resource_commits.rs](../../../crates/adapters/src/resource_commits.rs) | Finite resource reads and atomic optimistic commits over existing fact tables. |
| [src/resource_read_scope.rs](../../../crates/adapters/src/resource_read_scope.rs) | Disposable, thread-local advisory reads. No connection or transaction is held. Commands do not enter this scope, and every commit invalidates its pool's rows. |
| [src/runtime_images.rs](../../../crates/adapters/src/runtime_images.rs) | Keep recipe tags discoverable and frozen artifacts named without duplicate retention tags. |
| [src/runtime_preparation.rs](../../../crates/adapters/src/runtime_preparation.rs) | [按冻结配方准备镜像与认证组件](runtime-preparation.md#file-src-runtime-preparation-rs) |
| [src/runtime_queries.rs](../../../crates/adapters/src/runtime_queries.rs) | Workflow scheduling reads through the shared pool, without restoring history. |
| [src/runtime_registry.rs](../../../crates/adapters/src/runtime_registry.rs) | [注册不可变安装并发现内容漂移](runtime-preparation.md#file-src-runtime-registry-rs) |
| [src/runtime_verification.rs](../../../crates/adapters/src/runtime_verification.rs) | [按实际驱动与权限资产验证原生协议](runtime-preparation.md#file-src-runtime-verification-rs) |
| [src/scheduling_read.rs](../../../crates/adapters/src/scheduling_read.rs) | Finite scheduling hints. Dispatch still checks authoritative domain facts. |
| [src/session_resources.rs](../../../crates/adapters/src/session_resources.rs) | [把运行准备、凭据和权限接成会话资源端口](runtime-preparation.md#file-src-session-resources-rs) |
| [src/source_store.rs](../../../crates/adapters/src/source_store.rs) | [不可变源与运行治理记录分开落盘](storage-and-source-codecs.md#file-src-source-store-rs) |
| [src/source_views.rs](../../../crates/adapters/src/source_views.rs) | [无损压缩冻结视图，保留原始事实](storage-and-source-codecs.md#file-src-source-views-rs) |
| [src/store_changes.rs](../../../crates/adapters/src/store_changes.rs) | `StoreChanges`、`subscribe`、`notify` |
| [src/tests.rs](../../../crates/adapters/src/tests.rs) | [持久化测试作业与结构化 Node 结果](engineering-resources.md#file-src-tests-rs) |
| [src/tool_calls.rs](../../../crates/adapters/src/tool_calls.rs) | Select a finite set of native tool rows and restore their exact frozen views. |
| [src/workflow_details.rs](../../../crates/adapters/src/workflow_details.rs) | Indexed, committed reads of one actual workflow and its referenced facts. |
| [src/workflow_events.rs](../../../crates/adapters/src/workflow_events.rs) | Runtime notifications are committed with the exact changed resources. This reads only affected instances; it never restores an EngineeringState. |
| [src/workflow_intents.rs](../../../crates/adapters/src/workflow_intents.rs) | Bounded native intent scan and exact task-source decoding. No runtime aggregate is assembled for business validation or scheduling decisions. |
| [src/workflow_observations.rs](../../../crates/adapters/src/workflow_observations.rs) | Selected terminal ledger values and actual stopped-worker evidence. Normal workflow observation does not load conversation or execution history. |
| [src/workflow_projection.rs](../../../crates/adapters/src/workflow_projection.rs) | Transactional display metadata. Canonical runtime facts remain resource rows; these small projections never authorize control or replace sources. |
| [src/workflow_readers.rs](../../../crates/adapters/src/workflow_readers.rs) | Paged workflow readers select actual scoped rows and exact immutable sources. They never construct EngineeringState or compile a runtime permission view. |
| [src/workspace_read.rs](../../../crates/adapters/src/workspace_read.rs) | Legacy workspace shape over bounded committed rows. Source and runtime history bodies are loaded only by their explicit readers. |
| [tests/advisory_read_bounds.rs](../../../crates/adapters/tests/advisory_read_bounds.rs) | Real SQLite reads remain complete when advisory retention runs out of bytes. |
| [tests/agent_capabilities.rs](../../../crates/adapters/tests/agent_capabilities.rs) | `CallingHarness`、`open_session`、`dispatch_turn` |
| [tests/catalog_commit_cache.rs](../../../crates/adapters/tests/catalog_commit_cache.rs) | `change`、`ordinary_resource_commits_preserve_the_source_view_and_source_commits_refresh_it`、`project_runtime_commits_do_not_rebuild_configuration` |
| [tests/collection_versions.rs](../../../crates/adapters/tests/collection_versions.rs) | Predicate tokens are maintained by SQLite for every fact writer; no I/O or business lock is acquired by this read-set protection. |
| [tests/command_tests.rs](../../../crates/adapters/tests/command_tests.rs) | [真实测试容器、停止与代码导出防漂移](testing.md#file-tests-command-tests-rs) |
| [tests/configuration_catalog.rs](../../../crates/adapters/tests/configuration_catalog.rs) | `initialized`、`source`、`configuration_cache_pins_revisions_and_ignores_fact_only_commits` |
| [tests/configuration_changes.rs](../../../crates/adapters/tests/configuration_changes.rs) | [完整候选图、原子提交和跨库故障恢复](testing.md#file-tests-configuration-changes-rs) |
| [tests/configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs) | [资源源化、默认闭包与自检配置](testing.md#file-tests-configuration-resources-rs) |
| [tests/configuration_simplification.rs](../../../crates/adapters/tests/configuration_simplification.rs) | Contracts for Profile-only permissions, owned installation inputs and Workflow scheduling. |
| [tests/configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs) | [从冻结计划到产物接纳的完整用例](testing.md#file-tests-configured-workflows-rs) |
| [tests/conversations.rs](../../../crates/adapters/tests/conversations.rs) | [会话隔离、消费证明与不可变历史](testing.md#file-tests-conversations-rs) |
| [tests/credential_updates.rs](../../../crates/adapters/tests/credential_updates.rs) | Versioned publication uses a test-only OS vault port. No system key is stored. |
| [tests/dataset_preflight.rs](../../../crates/adapters/tests/dataset_preflight.rs) | `reserve`、`reject_unchanged`、`incompatible_registered_projects_are_rejected_before_global_writes` |
| [tests/delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs) | [真实 Red/Green、候选 diff 与交付合同对齐](testing.md#file-tests-delivery-acceptance-rs) |
| [tests/delivery_migration.rs](../../../crates/adapters/tests/delivery_migration.rs) | Explicit offline migration check against an isolated credential-free copy. |
| [tests/directory_publication.rs](../../../crates/adapters/tests/directory_publication.rs) | Directory snapshots are derived observations, not authoritative project edits. |
| [tests/docker_resources.rs](../../../crates/adapters/tests/docker_resources.rs) | [容器归属、挂载预检和嵌套沙箱](testing.md#file-tests-docker-resources-rs) |
| [tests/experience_directory.rs](../../../crates/adapters/tests/experience_directory.rs) | Derived metadata only; these fixtures do not run a workflow or a worker. |
| [tests/experience_imports.rs](../../../crates/adapters/tests/experience_imports.rs) | Insert-only history import. Ports are test doubles; no external model is run. |
| [tests/frozen_history_performance.rs](../../../crates/adapters/tests/frozen_history_performance.rs) | Read-only codec benchmark over an explicitly selected backup, never worker recovery. |
| [tests/git_resources.rs](../../../crates/adapters/tests/git_resources.rs) | [本地真实 Git 的 CAS 与故障恢复](testing.md#file-tests-git-resources-rs) |
| [tests/github_resources.rs](../../../crates/adapters/tests/github_resources.rs) | [真实 PR 丢失回执后的稳定恢复](testing.md#file-tests-github-resources-rs) |
| [tests/governance.rs](../../../crates/adapters/tests/governance.rs) | [Agent 实例范围、经验来源与目录治理](testing.md#file-tests-governance-rs) |
| [tests/integration_operations.rs](../../../crates/adapters/tests/integration_operations.rs) | Operation admission and receipt ports only. No Git clone, Docker or network. |
| [tests/manager_workspace.rs](../../../crates/adapters/tests/manager_workspace.rs) | [经理原生调用链与共享写入边界](testing.md#file-tests-manager-workspace-rs) |
| [tests/materialization_receipts.rs](../../../crates/adapters/tests/materialization_receipts.rs) | Receipt publication only. ResourceFixture never builds an image or starts a worker. |
| [tests/model_providers.rs](../../../crates/adapters/tests/model_providers.rs) | [冻结 provider profile 与目录发现边界](testing.md#file-tests-model-providers-rs) |
| [tests/orchestration.rs](../../../crates/adapters/tests/orchestration.rs) | [驱动一致性、阻塞调度与生命周期场景](testing.md#file-tests-orchestration-rs) |
| [tests/permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs) | [权限源发布、冻结绑定和会话切换恢复](testing.md#file-tests-permission-profiles-rs) |
| [tests/permissions.rs](../../../crates/adapters/tests/permissions.rs) | [权限编译拒绝与真实写入隔离](testing.md#file-tests-permissions-rs) |
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
| [tests/resource_commits.rs](../../../crates/adapters/tests/resource_commits.rs) | `condition`、`prepared`、`transaction_observations_bind_actual_commit_replay_conflict_and_rollback_without_content` |
| [tests/runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs) | [安装去重、实际文件摘要和设置原子性](testing.md#file-tests-runtime-registry-rs) |
| [tests/runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs) | [配方身份、实际驱动与跨数据集镜像保留](testing.md#file-tests-runtime-resources-rs) |
| [tests/schema_contracts.rs](../../../crates/adapters/tests/schema_contracts.rs) | [离线 Schema 与 Rust 序列化合同对齐](testing.md#file-tests-schema-contracts-rs) |
| [tests/settings_concurrency.rs](../../../crates/adapters/tests/settings_concurrency.rs) | `save`、`independent_edits_commit_but_stale_same_field_cannot_overwrite`、`invalid_batches_and_missing_guards_leave_both_settings_unchanged` |
| [tests/standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs) | [规范闭包、不同 Task 选择和历史恢复](testing.md#file-tests-standard-snapshots-rs) |
| [tests/support/mod.rs](../../../crates/adapters/tests/support/mod.rs) | [共享出厂装配、固定镜像与平台夹具](testing.md#file-tests-support-mod-rs) |
| [tests/tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs) | [工具合同、OAuth 与未知副作用恢复](testing.md#file-tests-tool-integrations-rs) |
| [tests/workflow_performance.rs](../../../crates/adapters/tests/workflow_performance.rs) | Opt-in component measurements. No model, worker, network, or user dataset. |
| [tests/workflow_progress.rs](../../../crates/adapters/tests/workflow_progress.rs) | Read-model fixtures only. No workflow, model, Git or native worker is run. |
| [tests/workflow_resource_concurrency.rs](../../../crates/adapters/tests/workflow_resource_concurrency.rs) | Deterministic resource/reader acceptance. No production dataset or supplier. |
| [tests/workflow_storage_scaling.rs](../../../crates/adapters/tests/workflow_storage_scaling.rs) | Opt-in storage attribution, not a business or capacity acceptance run. Only fresh explicitly named datasets; no model, worker or external effect. |
| [tests/workflows.rs](../../../crates/adapters/tests/workflows.rs) | [工程存储、锁、分页和 Artifact 基础合同](testing.md#file-tests-workflows-rs) |
| [tests/worktree_receipt_concurrency.rs](../../../crates/adapters/tests/worktree_receipt_concurrency.rs) | Deterministic external-port race: a business status changes during Git I/O. |

<!-- crate-files:end -->

<a id="file-cargo-toml"></a>

## Cargo.toml：外部实现所需依赖和测试边界

源码：[crates/adapters/Cargo.toml](../../../crates/adapters/Cargo.toml)。

本包依赖 core/domain，并引入 rusqlite、sha2、JSON Schema、ZIP/YAML 等外层实现库。Unix 条件依赖 libc 供进程组等系统操作使用；macOS Security Framework 属于独立 credential-helper 的依赖，Linux Secret Service 通过外部 secret-tool 调用。测试用 tempfile 创建独立数据与仓库。`build.rs` 在编译时校验 manifest/源包装，并将配置资源嵌入产物。

依赖应留在实际使用的边界：SQLite 和协议细节不应反向进入 domain/core。包中的 `tests.rs` 是生产测试执行适配器，`tests/` 才是 Rust 集成测试目录。二者名称相近，职责不同。

关联阅读：[adapters/build.rs](factory-and-configuration.md#file-build-rs)、[adapters/src/tests.rs](engineering-resources.md#file-src-tests-rs)、[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
