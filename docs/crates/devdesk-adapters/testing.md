# 集成测试的场景、夹具与真实边界

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

集成测试放在 adapters，便于同时装配生产 Engine、SQLite 和可控端口。每个文件下面说明它验证的合同、如何制造外部事实以及需要的真实环境；名称中出现 native 或 workflow，不代表一定调用了付费模型。

常规入口为 `cargo test -p devdesk-adapters --locked`。带 `#[ignore]` 的测试应按文件或测试名显式选择，并先阅读其环境变量与专用资源要求。真实验证的统一入口见 [check-live.sh](../../../scripts/check/check-live.sh)；测试支持代码见本页最后的 support/mod.rs。

## 本页文件

- [tests/command_tests.rs](#file-tests-command-tests-rs)：真实测试容器、停止与代码导出防漂移
- [tests/configuration_changes.rs](#file-tests-configuration-changes-rs)：完整候选图、原子提交和跨库故障恢复
- [tests/configuration_resources.rs](#file-tests-configuration-resources-rs)：资源源化、默认闭包与自检配置
- [tests/configured_workflows.rs](#file-tests-configured-workflows-rs)：从冻结计划到产物接纳的完整用例
- [tests/conversations.rs](#file-tests-conversations-rs)：会话隔离、消费证明与不可变历史
- [tests/delivery_acceptance.rs](#file-tests-delivery-acceptance-rs)：真实 Red/Green、候选 diff 与交付合同对齐
- [tests/docker_resources.rs](#file-tests-docker-resources-rs)：容器归属、挂载预检和嵌套沙箱
- [tests/git_resources.rs](#file-tests-git-resources-rs)：本地真实 Git 的 CAS 与故障恢复
- [tests/github_resources.rs](#file-tests-github-resources-rs)：真实 PR 丢失回执后的稳定恢复
- [tests/governance.rs](#file-tests-governance-rs)：Agent 实例范围、经验来源与目录治理
- [tests/manager_workspace.rs](#file-tests-manager-workspace-rs)：经理原生调用链与共享写入边界
- [tests/model_providers.rs](#file-tests-model-providers-rs)：冻结 provider profile 与目录发现边界
- [tests/orchestration.rs](#file-tests-orchestration-rs)：驱动一致性、阻塞调度与生命周期场景
- [tests/permission_profiles.rs](#file-tests-permission-profiles-rs)：权限源发布、冻结绑定和会话切换恢复
- [tests/permissions.rs](#file-tests-permissions-rs)：权限编译拒绝与真实写入隔离
- [tests/runtime_registry.rs](#file-tests-runtime-registry-rs)：安装去重、实际文件摘要和设置原子性
- [tests/runtime_resources.rs](#file-tests-runtime-resources-rs)：配方身份、实际驱动与跨数据集镜像保留
- [tests/schema_contracts.rs](#file-tests-schema-contracts-rs)：离线 Schema 与 Rust 序列化合同对齐
- [tests/standard_snapshots.rs](#file-tests-standard-snapshots-rs)：规范闭包、不同 Task 选择和历史恢复
- [tests/support/mod.rs](#file-tests-support-mod-rs)：共享出厂装配、固定镜像与平台夹具
- [tests/tool_integrations.rs](#file-tests-tool-integrations-rs)：工具合同、OAuth 与未知副作用恢复
- [tests/workflows.rs](#file-tests-workflows-rs)：工程存储、锁、分页和 Artifact 基础合同

<a id="file-tests-command-tests-rs"></a>

## tests/command_tests.rs：真实测试容器、停止与代码导出防漂移

源码：[crates/adapters/tests/command_tests.rs](../../../crates/adapters/tests/command_tests.rs)。

测试用临时 Git 仓库建立固定代码快照。普通测试确认导出内容被修改或路径越界时立即拒绝，且不触碰执行根之外的宿主文件；这部分不需要 Docker。

ignored 场景使用真实固定 Node 镜像验证 Red/Green、环境错误、运行中停止与恢复。断言核对结构化 reporter、容器实际终态及 cleanup，而不只看 stdout。它证明测试执行适配器的边界，不证明模型写出了正确业务代码。

代码定位：[`git`](../../../crates/adapters/tests/command_tests.rs#L14)、[`git_fixture`](../../../crates/adapters/tests/command_tests.rs#L34)、[`fixture`](../../../crates/adapters/tests/command_tests.rs)、[`finish`](../../../crates/adapters/tests/command_tests.rs#L118)。

关联阅读：[adapters/src/tests.rs](engineering-resources.md#file-src-tests-rs)、[adapters/src/git.rs](engineering-resources.md#file-src-git-rs)、[adapters/tests/support/mod.rs](testing.md#file-tests-support-mod-rs)。

<a id="file-tests-configuration-changes-rs"></a>

## tests/configuration_changes.rs：完整候选图、原子提交和跨库故障恢复

源码：[crates/adapters/tests/configuration_changes.rs](../../../crates/adapters/tests/configuration_changes.rs)。

场景在生产 Engine/SQLite 上发布自定义源：消费者先于依赖列出仍可原子提交，普通正文不成为依赖边；过期 preview、类型错误和无效停用都不能留下半个变更。

跨库场景注入阶段故障，验证隐藏 stage、项目独立 receipt、恢复 Actor、重放身份和取消围栏。另有稳定工作选择、Agent→Capability→Skill 链及 typed reference 停用检查。测试关注持久化事实，不通过两个内存对象模拟所谓跨库事务。

代码定位：[`Fault`](../../../crates/adapters/tests/configuration_changes.rs#L198)、[`engine`](../../../crates/adapters/tests/configuration_changes.rs#L14)、[`source`](../../../crates/adapters/tests/configuration_changes.rs#L21)、[`spec`](../../../crates/adapters/tests/configuration_changes.rs#L32)、[`apply`](../../../crates/adapters/tests/configuration_changes.rs#L40)。

关联阅读：[core/src/configuration_changes.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-changes-rs)、[core/src/configuration_transfers.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-transfers-rs)、[domain/src/configuration_graph.rs](../devdesk-domain/configuration-and-sources.md#file-src-configuration-graph-rs)。

<a id="file-tests-configuration-resources-rs"></a>

## tests/configuration_resources.rs：资源源化、默认闭包与自检配置

源码：[crates/adapters/tests/configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs)。

验证 Role CRUD 不派生 Agent/Skill，Agent 只保存源引用，SQLite 重新打开后仍从 source_definitions 恢复完整组合。默认选择、Delivery/Spec、DSH 栈和权限检查都使用真实出厂资源，而不是另写一套预期配置。

当前还核对默认 PRD、Design、TaskPlan 的具体结构、内容证据与受限自检 Skill/工具闭包。改变默认修订时，测试要求新图完整、旧确切引用可恢复；不能仅更新一个快照字符串让断言通过。

代码定位：[`engine`](../../../crates/adapters/tests/configuration_resources.rs#L13)、[`command`](../../../crates/adapters/tests/configuration_resources.rs#L32)、[`current_defaults_exclude_experience_tools_and_reject_them_at_creation`](../../../crates/adapters/tests/configuration_resources.rs#L111)、[`role_crud_has_no_derived_agent_or_skill_side_effects`](../../../crates/adapters/tests/configuration_resources.rs#L126)。

关联阅读：[adapters/src/source_store.rs](storage-and-source-codecs.md#file-src-source-store-rs)、[adapters/src/factory_bundle.rs](factory-and-configuration.md#file-src-factory-bundle-rs)、[core/src/spec_lint.rs](../devdesk-core/manager-and-platform-tools.md#file-src-spec-lint-rs)。

<a id="file-tests-configured-workflows-rs"></a>

## tests/configured_workflows.rs：从冻结计划到产物接纳的完整用例

源码：[crates/adapters/tests/configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs)。

这是配置工作流的主要集成场景集合：自定义角色和子流程发布、显式派发/接纳、规范任务、冻结输入、类型化产物、条件端口、人工确认、rework、取消和历史恢复。生产 Engine/SQLite 配合 RecordingAgent、PlatformCallingAgent 等可控原生端口，精确制造消费、调用和停止事实；这些夹具不代表真实模型推理。

涉及实际 Git 的场景建立临时仓库，验证 workspace/candidate/advance、写入者 rework checkpoint 与保护边界。大输入和原始证据场景核对授权正文完整可读，拒绝只有临时路径、缺少环境或终止事实的材料。

当前失败场景区分可信的派发前技术拒绝与业务输出拒绝，证明补充 Turn 不花 retry 配额、重试保留原任务上下文且必须取得停止证明。自检场景证明普通 Agent 只能 lint 当前冻结输入/输出 Delivery，lint 与真实产物接纳共用结构和证据定位检查；lint 不封存草稿，也不允许查询其他交付类型。

代码定位：[`spec_lint_and_output_admission_share_machine_checks_and_frozen_receipts`](../../../crates/adapters/tests/configured_workflows.rs#L5625)、[`supplemental_turns_do_not_spend_retry_quota_and_recovery_keeps_the_task_context`](../../../crates/adapters/tests/configured_workflows.rs#L5762)、[`native_rejection_retries_only_after_real_stop_proof_and_keeps_the_failure_event`](../../../crates/adapters/tests/configured_workflows.rs#L5895)、[`worker_self_lint_checks_the_same_frozen_report_locations_as_admission`](../../../crates/adapters/tests/configured_workflows.rs#L6179)。

关联阅读：[core/src/configured_runtime.rs](../devdesk-core/workflow-execution.md#file-src-configured-runtime-rs)、[core/src/workflow_failure.rs](../devdesk-core/workflow-execution.md#file-src-workflow-failure-rs)、[core/src/spec_lint.rs](../devdesk-core/manager-and-platform-tools.md#file-src-spec-lint-rs)、[core/src/platform_tools.rs](../devdesk-core/manager-and-platform-tools.md#file-src-platform-tools-rs)。

<a id="file-tests-conversations-rs"></a>

## tests/conversations.rs：会话隔离、消费证明与不可变历史

源码：[crates/adapters/tests/conversations.rs](../../../crates/adapters/tests/conversations.rs)。

用可控 Harness 验证未创建 Feature 时也可讨论，重启不捏造 Run；忙碌、跨项目、不可变 Session 绑定和 append-only 消息规则均在实际存储上检查。

终态场景分别提供 external session、输入消费和 writer release 证据，缺一不能完成。消息重放、后继会话和 Proposal 类型也有覆盖；未知结果不自动重发，global discussion 与工程会话保持不同命名空间。夹具产生原生观察，权威终态仍由 core 接纳。

代码定位：[`ConversationFixture`](../../../crates/adapters/tests/conversations.rs#L787)、[`engine`](../../../crates/adapters/tests/conversations.rs#L17)、[`initialize`](../../../crates/adapters/tests/conversations.rs#L183)、[`discussion_before_feature_survives_restart_without_fabricating_runs`](../../../crates/adapters/tests/conversations.rs#L221)、[`enforce_busy_cross_project_immutable_binding_and_append_only_messages`](../../../crates/adapters/tests/conversations.rs#L285)。

关联阅读：[core/src/conversations.rs](../devdesk-core/conversations-and-history.md#file-src-conversations-rs)、[core/src/conversation_runtime.rs](../devdesk-core/conversations-and-history.md#file-src-conversation-runtime-rs)、[core/src/proposal_commands.rs](../devdesk-core/conversations-and-history.md#file-src-proposals-rs)。

<a id="file-tests-delivery-acceptance-rs"></a>

## tests/delivery_acceptance.rs：真实 Red/Green、候选 diff 与交付合同对齐

源码：[crates/adapters/tests/delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs)。

ignored 场景组合真实临时 Git、Docker Node 测试和候选提交，核对 Red/Green 报告与实际 diff 是否满足交付 Spec。测试请求固定镜像、代码 revision、运行范围和资源预算，不以模型摘要或手写“通过”文本代替运行。

该文件是资源之间的验收连接点，运行前需要可用 Docker 与专用固定镜像。它不调用付费模型或替用户完成远端 PR；那些能力需要另外的真实服务验收。

代码定位：[`run`](../../../crates/adapters/tests/delivery_acceptance.rs#L23)、[`git`](../../../crates/adapters/tests/delivery_acceptance.rs#L36)、[`test_request`](../../../crates/adapters/tests/delivery_acceptance.rs#L50)、[`execute`](../../../crates/adapters/tests/delivery_acceptance.rs#L103)。

关联阅读：[adapters/src/tests.rs](engineering-resources.md#file-src-tests-rs)、[adapters/src/git.rs](engineering-resources.md#file-src-git-rs)、[domain/src/spec_checks.rs](../devdesk-domain/workflows-and-checks.md#file-src-spec-checks-rs)、[adapters/tests/support/mod.rs](testing.md#file-tests-support-mod-rs)。

<a id="file-tests-docker-resources-rs"></a>

## tests/docker_resources.rs：容器归属、挂载预检和嵌套沙箱

源码：[crates/adapters/tests/docker_resources.rs](../../../crates/adapters/tests/docker_resources.rs)。

普通测试在 Docker 调用之前拒绝执行根外的挂载。ignored 场景再使用真实容器核对稳定操作重放、外来归属拒绝、实际停止和清理。

嵌套沙箱场景确认内层不能扩大外层测试目录写权限；DSH runner 场景核对私有 state 与 proc 屏蔽，同时不增加容器特权。这里证明隔离机制与路径边界，不能单靠配置 JSON 断言已完成后端 enforcement。

代码定位：[`reject_outside_mounts_before_touching_docker`](../../../crates/adapters/tests/docker_resources.rs#L13)、[`real_container_ownership_idempotence_stop_and_cleanup`](../../../crates/adapters/tests/docker_resources.rs#L53)、[`nested_sandbox_cannot_broaden_the_outer_test_directory_write_grant`](../../../crates/adapters/tests/docker_resources.rs#L99)、[`dsh_runner_masks_private_state_and_proc_without_expanding_docker_privileges`](../../../crates/adapters/tests/docker_resources.rs#L175)。

关联阅读：[adapters/src/docker.rs](engineering-resources.md#file-src-docker-rs)、[adapters/src/permissions.rs](credentials-and-permissions.md#file-src-permissions-rs)。

<a id="file-tests-git-resources-rs"></a>

## tests/git_resources.rs：本地真实 Git 的 CAS 与故障恢复

源码：[crates/adapters/tests/git_resources.rs](../../../crates/adapters/tests/git_resources.rs)。

在临时仓库中实际运行 Git，检查受管 worktree、单父候选、确切 target、CAS、远端引用恢复和重复操作。脏 target、外来上下文与错误 origin 应被拒绝，并核对引用没有被误改。

这些测试不依赖 GitHub，但并非 Git trait 的内存替身。它们适合定位 Git 内容和归属错误；远端保护规则、PR marker 与 GitHub 回执在另一个测试文件中验证。

代码定位：[`git`](../../../crates/adapters/tests/git_resources.rs#L6)、[`actual_worktrees_single_parent_candidate_cas_and_recovery`](../../../crates/adapters/tests/git_resources.rs#L319)、[`dirty_targets_and_foreign_contexts_are_rejected_without_modifying_refs`](../../../crates/adapters/tests/git_resources.rs#L488)、[`interrupted_clone_rejects_foreign_origin_without_changing_repository`](../../../crates/adapters/tests/git_resources.rs#L544)。

关联阅读：[adapters/src/git.rs](engineering-resources.md#file-src-git-rs)、[adapters/tests/github_resources.rs](testing.md#file-tests-github-resources-rs)。

<a id="file-tests-github-resources-rs"></a>

## tests/github_resources.rs：真实 PR 丢失回执后的稳定恢复

源码：[crates/adapters/tests/github_resources.rs](../../../crates/adapters/tests/github_resources.rs)。

ignored 场景在明确提供的 GitHub 测试仓库上，通过原 request marker 找回已有 PR，确认不会因本地丢回执再创建一个。环境变量 DEVDESK_LIVE_GITHUB_REPO 与 DEVDESK_LIVE_PR_REQUEST 指定专用仓库和请求。

该场景使用真实 gh 认证和 GitHub API，不能作为普通离线回归的一部分。它核对 PR 来源与幂等恢复，不授权平台调用 merge API 或绕过分支保护。

代码定位：[`lost_pr_receipt_is_recovered_by_the_original_marker_without_creating_a_second_pr`](../../../crates/adapters/tests/github_resources.rs#L7)。

关联阅读：[adapters/src/github.rs](engineering-resources.md#file-src-github-rs)、[adapters/tests/git_resources.rs](testing.md#file-tests-git-resources-rs)。

<a id="file-tests-governance-rs"></a>

## tests/governance.rs：Agent 实例范围、经验来源与目录治理

源码：[crates/adapters/tests/governance.rs](../../../crates/adapters/tests/governance.rs)。

覆盖 Agent 实例固定组成、独立 scope、角色/修订历史和目录命令幂等。经验场景实际打开多个只读工程库，按 Agent ID 查询并核对封存材料摘要，验证项目 Agent 不能越界读取。

主线刷新必须是显式操作，目录/实例变更不能暗中开始业务工作。这里将治理选择和已有执行事实分开验证，避免“改了默认”错误地重写历史。

代码定位：[`engine`](../../../crates/adapters/tests/governance.rs#L14)、[`command`](../../../crates/adapters/tests/governance.rs#L27)、[`setup_agent`](../../../crates/adapters/tests/governance.rs#L78)、[`agent_instances_pin_composition_and_choose_independent_scope`](../../../crates/adapters/tests/governance.rs#L108)。

关联阅读：[core/src/governance.rs](../devdesk-core/configuration-and-catalog.md#file-src-governance-rs)、[adapters/src/experience.rs](storage-and-source-codecs.md#file-src-experience-rs)、[core/src/catalog_sync.rs](../devdesk-core/configuration-and-catalog.md#file-src-catalog-sync-rs)。

<a id="file-tests-manager-workspace-rs"></a>

## tests/manager_workspace.rs：经理原生调用链与共享写入边界

源码：[crates/adapters/tests/manager_workspace.rs](../../../crates/adapters/tests/manager_workspace.rs)。

Scripted Harness 用确定的原生工具调用驱动经理工作台，验证查询、分页、分块正文和五类配置工作场景。结果仍经过生产平台工具管线和 Engine 事务，而不是直接调用私有函数伪造最终状态。

跨项目请求、过期 epoch、共享授权、自提权及跨库原 Actor 都有拒绝/恢复覆盖；实例重绑定只影响未来采用，不能替换已冻结工作。权限/Skill 批量变更还验证真实后端 preflight 的原子性。测试证明可控调用链，真实 Codex 模型桥需独立探针。

代码定位：[`Scripted`](../../../crates/adapters/tests/manager_workspace.rs#L17)、[`next`](../../../crates/adapters/tests/manager_workspace.rs#L24)、[`change`](../../../crates/adapters/tests/manager_workspace.rs#L64)、[`open_session`](../../../crates/adapters/tests/manager_workspace.rs#L77)、[`dispatch_turn`](../../../crates/adapters/tests/manager_workspace.rs#L85)。

关联阅读：[core/src/manager_workspace.rs](../devdesk-core/manager-and-platform-tools.md#file-src-manager-workspace-rs)、[core/src/controllers.rs](../devdesk-core/orchestration-and-drivers.md#file-src-controllers-rs)、[core/src/configuration_transfers.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-transfers-rs)。

<a id="file-tests-model-providers-rs"></a>

## tests/model_providers.rs：冻结 provider profile 与目录发现边界

源码：[crates/adapters/tests/model_providers.rs](../../../crates/adapters/tests/model_providers.rs)。

离线场景核对 provider 连接完整快照、profile 摘要、路由互斥和秘密值不进入生成配置。Ollama 测试启动本机元数据夹具，明确不把目录返回当成推理成功。

ignored 场景使用 DEVDESK_DSH_MOCK_IMAGE 指定的固定 DSH 镜像，在禁外网容器中验证原生 resume/工具协议。它验证协议兼容和配置路由，不能据此声称真实模型供应商可用。

代码定位：[`facts`](../../../crates/adapters/tests/model_providers.rs#L69)、[`connection`](../../../crates/adapters/tests/model_providers.rs#L132)、[`generated_profiles_select_only_the_frozen_route_without_secret_values`](../../../crates/adapters/tests/model_providers.rs#L140)、[`new_providers_require_a_complete_snapshot_and_matching_profile_digest`](../../../crates/adapters/tests/model_providers.rs#L170)。

关联阅读：[adapters/src/model_providers.rs](external-integrations.md#file-src-model-providers-rs)、[domain/src/model_provider.rs](../devdesk-domain/runtime-and-permissions.md#file-src-model-provider-rs)、[adapters/src/harness.rs](harness-and-protocols.md#file-src-harness-rs)。

<a id="file-tests-orchestration-rs"></a>

## tests/orchestration.rs：驱动一致性、阻塞调度与生命周期场景

源码：[crates/adapters/tests/orchestration.rs](../../../crates/adapters/tests/orchestration.rs)。

NativeManager 等端口夹具配合真实 Engine/SQLite，覆盖 manual/rust/agent 控制、计划 preview/quiesce/apply、typed bypass/waiver、段执行、经理代次、所有权和 wakeup。控制查询不能隐式派发，Human 决定不能由 Agent 冒充。

共同资格场景让 inspect 提示与实际 dispatch/accept/complete 对照，检查祖先暂停、foreach 配额和 needs_reconciliation 占位。某个子流程阻塞或遇到预期版本冲突时，Rust 驱动仍可选择独立工作，避免队首饥饿。

Task 场景覆盖真实业务 dependencies 与流程图 dependencies 的组合：必须等前置 Task Completed，才能启动后置 Task 的任何步骤。同一根实例下的三个绑定 Task 还分别制造等待人工确认、业务依赖阻塞和独立完成，核对 foreach 两个调用槽位与 Rust 单 worker 配额；受阻成员不占用就绪执行的槽位，人工确认后整个根及三个 Task 都能完成。失败/恢复矩阵另核对可信失败、独立 retry 配额、取消和停止证明，并保留完整生命周期断言。此处原生事件由夹具控制，不代表执行了付费模型验收。

代码定位：[`NativeManager`](../../../crates/adapters/tests/orchestration.rs#L1042)、[`engine`](../../../crates/adapters/tests/orchestration.rs#L87)、[`cmd`](../../../crates/adapters/tests/orchestration.rs#L99)、[`start`](../../../crates/adapters/tests/orchestration.rs#L216)、[`document`](../../../crates/adapters/tests/orchestration.rs#L254)。

关联阅读：[core/src/orchestration.rs](../devdesk-core/orchestration-and-drivers.md#file-src-orchestration-rs)、[core/src/workflow_eligibility.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-eligibility-rs)、[core/src/workflow_driver.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-driver-rs)、[core/src/workflow_change_commit.rs](../devdesk-core/workflow-changes.md#file-src-workflow-changes-rs)。

<a id="file-tests-permission-profiles-rs"></a>

## tests/permission_profiles.rs：权限源发布、冻结绑定和会话切换恢复

源码：[crates/adapters/tests/permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs)。

普通测试覆盖严格 v2 Profile、导入/导出原子性、单一持久化目录、内嵌预设恢复和精确 target CAS。批量变更必须全部可编译才提交；已有 Feature/Session 保留冻结权限。

会话权限切换用进程级边界制造中断，检查停止、快照和后继绑定不会拼接成一半旧一半新的执行。ignored 原生 Codex/DSH 场景验证后端权限落实，不调用模型。源合法、编译成功和真实隔离生效分别有证据。

代码定位：[`global`](../../../crates/adapters/tests/permission_profiles.rs#L18)、[`command`](../../../crates/adapters/tests/permission_profiles.rs#L31)、[`execute`](../../../crates/adapters/tests/permission_profiles.rs#L56)、[`change`](../../../crates/adapters/tests/permission_profiles.rs#L67)。

关联阅读：[adapters/src/permission_profiles.rs](credentials-and-permissions.md#file-src-permission-profiles-rs)、[core/src/permission_switch.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-switch-rs)、[host/src/permission_resources.rs](../devdesk-host/rpc-and-configuration.md#file-src-permission-resources-rs)。

<a id="file-tests-permissions-rs"></a>

## tests/permissions.rs：权限编译拒绝与真实写入隔离

源码：[crates/adapters/tests/permissions.rs](../../../crates/adapters/tests/permissions.rs)。

普通测试验证无法落实的 grant 在执行前被拒绝。ignored 场景实际运行 Codex/DSH 和 Docker，检查 scoped 写入、其他 worktree 拒绝、full access 的确切信任范围，以及未知 scoped 名称不能退化成宽权限。

这些测试针对实际文件和原生沙箱，不只比较配置字符串。运行前需满足对应固定镜像与平台资产条件；实际模型网络不在这些无模型权限探针的证明范围。

代码定位：[`scope`](../../../crates/adapters/tests/permissions.rs#L7)、[`full_access_and_restricted_profiles_use_native_enforcement`](../../../crates/adapters/tests/permissions.rs#L71)、[`unsupported_grants_are_rejected_before_execution`](../../../crates/adapters/tests/permissions.rs#L102)、[`codex_compiled_profile_allows_scoped_writes_and_denies_other_worktree_writes`](../../../crates/adapters/tests/permissions.rs#L119)。

关联阅读：[adapters/src/permissions.rs](credentials-and-permissions.md#file-src-permissions-rs)、[adapters/src/docker.rs](engineering-resources.md#file-src-docker-rs)、[adapters/tests/support/mod.rs](testing.md#file-tests-support-mod-rs)。

<a id="file-tests-runtime-registry-rs"></a>

## tests/runtime_registry.rs：安装去重、实际文件摘要和设置原子性

源码：[crates/adapters/tests/runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs)。

使用带合成 ELF 头的真实临时文件与 8 个线程验证相同内容跨路径注册去重，随后改变文件发现漂移；同一安装键的新内容必须冲突，重开数据库仍保留原安装。这个 ELF 夹具不执行，只用于检验声明平台与内容身份。错误平台、摘要、脚本和可变镜像标识都应拒绝，不把版本标签当作安装证据。

当前设置和用户偏好更新也检查事务原子性。这些场景测试注册合同与持久化，不启动模型，也不替代容器协议验证。

代码定位：[`native`](../../../crates/adapters/tests/runtime_registry.rs#L8)、[`registration_deduplicates_across_paths_and_threads_then_detects_drift`](../../../crates/adapters/tests/runtime_registry.rs#L265)、[`reject_wrong_platform_hash_scripts_and_mutable_images`](../../../crates/adapters/tests/runtime_registry.rs#L296)、[`current_settings_and_preference_updates_are_atomic`](../../../crates/adapters/tests/runtime_registry.rs#L331)。

关联阅读：[adapters/src/runtime_registry.rs](runtime-preparation.md#file-src-runtime-registry-rs)、[adapters/src/catalog.rs](storage-and-source-codecs.md#file-src-catalog-rs)。

<a id="file-tests-runtime-resources-rs"></a>

## tests/runtime_resources.rs：配方身份、实际驱动与跨数据集镜像保留

源码：[crates/adapters/tests/runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs)。

离线场景证明发行标签/原生目录元数据不决定 recipe 身份，允许两份合法外部配方，并拒绝源声明伪造驱动能力。驱动能力来自当前适配器，而非用户配置中的布尔值。

ignored 场景准备两种真实 Docker 配方，只验证初始化协议，不执行推理；另检查已准备镜像在不同数据集中可按实际身份复用和保留。这样将配方兼容、协议兼容和模型可用性分开。

代码定位：[`setup`](../../../crates/adapters/tests/runtime_resources.rs#L9)、[`source`](../../../crates/adapters/tests/runtime_resources.rs#L19)、[`labels_and_native_catalog_do_not_determine_recipe_identity`](../../../crates/adapters/tests/runtime_resources.rs#L77)、[`two_external_recipes_are_valid_and_forged_driver_capability_is_rejected`](../../../crates/adapters/tests/runtime_resources.rs#L90)。

关联阅读：[adapters/src/runtime_preparation.rs](runtime-preparation.md#file-src-runtime-preparation-rs)、[adapters/src/runtime_verification.rs](runtime-preparation.md#file-src-runtime-verification-rs)、[core/src/driver_capabilities.rs](../devdesk-core/session-resources-and-permissions.md#file-src-driver-capabilities-rs)。

<a id="file-tests-schema-contracts-rs"></a>

## tests/schema_contracts.rs：离线 Schema 与 Rust 序列化合同对齐

源码：[crates/adapters/tests/schema_contracts.rs](../../../crates/adapters/tests/schema_contracts.rs)。

遍历发布的 Schema、出厂源、manifest、权限资产与 RPC 定义，确认引用可离线解析，serde 默认值和嵌套对象往返一致。测试也防止把版本标签误用成运行能力门槛。

当前覆盖 WorkflowFailure 的 kind/effect、StepExecution 的 failure/retry_of、inspect 动作资格与 retry_count 等新字段。Schema 通过表示接口形状一致，不证明外部执行真的发生；运行事实还由集成用例和实际端口核对。

代码定位：[`definition`](../../../crates/adapters/tests/schema_contracts.rs#L18)、[`round_trip`](../../../crates/adapters/tests/schema_contracts.rs#L21)、[`all_published_schemas_and_definitions_resolve_offline`](../../../crates/adapters/tests/schema_contracts.rs#L42)、[`published_configuration_wrappers_accept_current_builtin_revisions`](../../../crates/adapters/tests/schema_contracts.rs#L91)。

关联阅读：[domain/src/workflow_failure.rs](../devdesk-domain/workflows-and-checks.md#file-src-workflow-failure-rs)、[core/src/workflow_eligibility.rs](../devdesk-core/orchestration-and-drivers.md#file-src-workflow-eligibility-rs)、[adapters/build.rs](factory-and-configuration.md#file-build-rs)。

<a id="file-tests-standard-snapshots-rs"></a>

## tests/standard_snapshots.rs：规范闭包、不同 Task 选择和历史恢复

源码：[crates/adapters/tests/standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs)。

区分 Standard 的规范内容与实现映射：同一业务工作采用不同 Standard/Role→Agent 选择时，冻结闭包和具体执行绑定都应正确。场景运行两个 Task，再重启、重新采用并查询历史，检查旧快照不被当前默认替换。

访问上下文和确切源引用是断言重点。测试使用可控端口重现执行，不将替身输出声称为真实模型完成了规范验收。

代码定位：[`engine`](../../../crates/adapters/tests/standard_snapshots.rs#L10)、[`command`](../../../crates/adapters/tests/standard_snapshots.rs#L52)、[`publish`](../../../crates/adapters/tests/standard_snapshots.rs#L104)、[`capture`](../../../crates/adapters/tests/standard_snapshots.rs#L117)。

关联阅读：[core/src/standards.rs](../devdesk-core/configuration-and-catalog.md#file-src-standards-rs)、[core/src/frozen_views.rs](../devdesk-core/workflow-compilation.md#file-src-frozen-views-rs)、[domain/src/standards.rs](../devdesk-domain/configuration-and-sources.md#file-src-standards-rs)。

<a id="file-tests-support-mod-rs"></a>

## tests/support/mod.rs：共享出厂装配、固定镜像与平台夹具

源码：[crates/adapters/tests/support/mod.rs](../../../crates/adapters/tests/support/mod.rs)。

support 提供集成测试共用的出厂初始化、平台身份、数据与权限辅助。镜像可由 DEVDESK_ACCEPTANCE_{PROVIDER}_IMAGE 指定；默认引用也会通过实际 Docker inspect 解析不可变 image ID，不能只把一个可变 tag 填进 RuntimeBinding。

它本身不是独立测试目标。新增夹具应保留生产端口合同并明确哪些事件是注入的；真实镜像、无模型协议夹具与真实供应商推理不要共用模糊的“live”通过标记。

代码定位：[`metadata`](../../../crates/adapters/tests/support/mod.rs#L2)、[`image`](../../../crates/adapters/tests/support/mod.rs#L15)、[`platform`](../../../crates/adapters/tests/support/mod.rs#L19)、[`load`](../../../crates/adapters/tests/support/mod.rs#L22)。

关联阅读：[adapters/src/factory_bundle.rs](factory-and-configuration.md#file-src-factory-bundle-rs)、[adapters/src/runtime_registry.rs](runtime-preparation.md#file-src-runtime-registry-rs)、[adapters/tests/model_providers.rs](testing.md#file-tests-model-providers-rs)。

<a id="file-tests-tool-integrations-rs"></a>

## tests/tool_integrations.rs：工具合同、OAuth 与未知副作用恢复

源码：[crates/adapters/tests/tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs)。

离线场景覆盖 Schema 预算/关键字、OpenAPI 安全方案、Skill ZIP/Git 导入和冻结工具依赖。MemorySecrets 与 ObservedExecutor 用于可控凭据和操作时序，验证持久化调用、取消、迟到回执和恢复。

ignored 场景实际在 Docker 中跑 HTTP/MCP 协议夹具，覆盖支持的协议版本/transport、私网与重定向限制、认证注入、超时和副作用计数。OAuth 场景核对 PKCE、账户、scope、刷新与未知结果不盲重发。禁外网夹具证明协议与隔离，真实第三方服务可用性需另行验收。

代码定位：[`MemorySecrets`](../../../crates/adapters/tests/tool_integrations.rs#L512)、[`ObservedExecutor`](../../../crates/adapters/tests/tool_integrations.rs#L738)、[`engine`](../../../crates/adapters/tests/tool_integrations.rs#L16)、[`command`](../../../crates/adapters/tests/tool_integrations.rs#L27)、[`api_tool`](../../../crates/adapters/tests/tool_integrations.rs#L46)、[`resource_revisions_scope_and_actual_schema_validation_are_enforced`](../../../crates/adapters/tests/tool_integrations.rs#L50)。

关联阅读：[adapters/src/integrations.rs](external-integrations.md#file-src-integrations-rs)、[adapters/src/integration_oauth.rs](external-integrations.md#file-src-integration-oauth-rs)、[core/src/integrations.rs](../devdesk-core/integrations-and-repositories.md#file-src-integrations-rs)。

<a id="file-tests-workflows-rs"></a>

## tests/workflows.rs：工程存储、锁、分页和 Artifact 基础合同

源码：[crates/adapters/tests/workflows.rs](../../../crates/adapters/tests/workflows.rs)。

覆盖工程初始化、数据库版本、数据标记、目录清理后的行为、外来内容拒绝与独占锁；状态保存/重放和历史分页使用生产 SqliteStore。文件材料测试检查符号链接和执行根边界。

这些基础合同影响所有上层工作流：若数据集不兼容，应该明确失败，而不是自动迁移或删目录“恢复”。此文件主要验证存储与应用基础设施，不以名称中的 workflow 暗示完成模型工程验收。

代码定位：[`engine`](../../../crates/adapters/tests/workflows.rs#L8)、[`revision`](../../../crates/adapters/tests/workflows.rs#L22)、[`cmd`](../../../crates/adapters/tests/workflows.rs#L43)、[`call`](../../../crates/adapters/tests/workflows.rs#L55)。

关联阅读：[adapters/src/lib.rs](storage-and-source-codecs.md#file-src-lib-rs)、[运行历史读取端口](../devdesk-core/conversations-and-history.md#file-src-history-rs)、[host/src/history_tests.rs](../devdesk-host/testing.md#file-src-history-tests-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
