# Web 警告与外部验收逐项复核

本次以 ef37b778343b805a202fc2213fd41eaa320b088f 为起点，在 main 继续复核。以下行号来自修改前 ESLint JSON，用于唯一标识原来的 75 条；当前文件经过模块拆分，不能将旧行号当作当前定位。原始证据目录为 /tmp/devdesk-web-native-20261115，临时产物不入库。

## 独立判断

75 条不是 75 个已证实的运行故障。49 条 hooks 提示中，稳定回调遗漏属于依赖合同不完整；5 条 ref 清理提示误把请求序号当成可能变化的 DOM 资源；项目对象在加载器内实际只读取 ID，原有 ID 依赖有合理语义。会话当前绑定、候选 Agent、运行配置和导航依赖则存在陈旧状态风险，其中同一 Session ID 下当前 Agent 绑定更新已补针对性浏览器断言。

26 条 refresh 提示约束组件导出边界。当前 Vite 没有 React Refresh 插件，不能将这些提示说成已发生的生产故障或承诺 Fast Refresh 状态保留。按组件、状态和纯函数拆分模块合理，并避免后来启用 Refresh 时的混合边界；本次不为了满足规则引入新的开发插件。

没有关闭规则或新增豁免。ESLint 原规则级别保留，lint 预算收紧为零；RPC 的两条既有 any 豁免原样移动到 rpc-types.ts。未把动态 RPC 边界伪称为强类型。

## 75 条逐项处理

| # | 原位置 | 规则 / 判断 | 处理 |
|---|---|---|---|
| 1 | web/src/components/ActionFeedback.tsx:25 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 状态、hooks 与函数移至 action-feedback.ts |
| 2 | web/src/components/ActionFeedback.tsx:26 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 状态、hooks 与函数移至 action-feedback.ts |
| 3 | web/src/components/ActionFeedback.tsx:36 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 状态、hooks 与函数移至 action-feedback.ts |
| 4 | web/src/components/ActionFeedback.tsx:191 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 状态、hooks 与函数移至 action-feedback.ts |
| 5 | web/src/components/AgentCatalog.tsx:251 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 6 | web/src/components/AgentCatalog.tsx:260 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 7 | web/src/components/AgentCatalog.tsx:350 | exhaustive-deps：初始路由消费回调依赖不完整 | 稳定 choose；声明选中项/类别；保留一次性消费标记避免覆盖草稿 |
| 8 | web/src/components/AgentExperience.tsx:63 | exhaustive-deps：请求序号清理，DOM ref 启发式不适用 | 保留单调请求序号和核对；稳定 invalidate 回调显式取消 |
| 9 | web/src/components/AgentExperience.tsx:65 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 10 | web/src/components/AgentInstances.tsx:42 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 11 | web/src/components/DshProviderSettings.tsx:58 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 12 | web/src/components/DshProviderSettings.tsx:72 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 13 | web/src/components/Integrations.tsx:199 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 14 | web/src/components/Integrations.tsx:301 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 15 | web/src/components/Integrations.tsx:306 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 16 | web/src/components/Integrations.tsx:356 | exhaustive-deps：初始路由消费回调依赖不完整 | 稳定 choose；声明选中项/类别；保留一次性消费标记避免覆盖草稿 |
| 17 | web/src/components/Integrations.tsx:980 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 18 | web/src/components/Integrations.tsx:2445 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 19 | web/src/components/Integrations.tsx:2702 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 20 | web/src/components/Integrations.tsx:2724 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 21 | web/src/components/KernelConnection.tsx:6 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 地址规范化移至 kernel-address.ts |
| 22 | web/src/components/PermissionResources.tsx:114 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 23 | web/src/components/PermissionResources.tsx:143 | exhaustive-deps：派生权限选择依赖不完整 | effect 内由 catalog/target 推导候选，保留仍可用的用户选择 |
| 24 | web/src/components/ProjectIntegrationBindings.tsx:61 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 25 | web/src/components/ProjectManager.tsx:121 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 26 | web/src/components/ProjectManager.tsx:130 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 27 | web/src/components/ProjectManager.tsx:152 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 28 | web/src/components/RepositorySettings.tsx:73 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 29 | web/src/components/RoleConversation.tsx:159 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 30 | web/src/components/RoleConversation.tsx:180 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 31 | web/src/components/RoleConversation.tsx:190 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 32 | web/src/components/RoleConversation.tsx:234 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 33 | web/src/components/RoleConversation.tsx:246 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 34 | web/src/components/RoleConversation.tsx:259 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 35 | web/src/components/RoleConversation.tsx:265 | exhaustive-deps：会话身份/默认选择的陈旧状态风险 | 补齐实际读取的绑定、会话与候选依赖；稳定派生集合，保留请求代次围栏 |
| 36 | web/src/components/RoleConversation.tsx:350 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 37 | web/src/components/RoleConversation.tsx:396 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 38 | web/src/components/RoleManager.tsx:67 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 39 | web/src/components/RoleManager.tsx:76 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 40 | web/src/components/RoleManager.tsx:104 | exhaustive-deps：初始路由消费回调依赖不完整 | 稳定 choose；声明选中项/类别；保留一次性消费标记避免覆盖草稿 |
| 41 | web/src/components/StandardManager.tsx:250 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 42 | web/src/components/StandardManager.tsx:284 | exhaustive-deps：请求序号清理，DOM ref 启发式不适用 | 仅 effect 使用，改局部 active 标记阻止迟到响应 |
| 43 | web/src/components/StandardManager.tsx:286 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 44 | web/src/components/WorkStandard.tsx:118 | exhaustive-deps：运行计划数据同步不完整 | 依赖精确标准/流程/权限引用；绑定按序列化内容比较，避免同值轮询重置草稿 |
| 45 | web/src/components/WorkflowController.tsx:80 | exhaustive-deps：请求序号清理，DOM ref 启发式不适用 | 保留单调请求序号和核对；稳定 invalidate 回调显式取消 |
| 46 | web/src/components/WorkflowInstanceView.tsx:87 | exhaustive-deps：请求序号清理，DOM ref 启发式不适用 | 保留单调请求序号和核对；稳定 invalidate 回调显式取消 |
| 47 | web/src/components/WorkflowInstanceView.tsx:89 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 48 | web/src/components/WorkflowManager.tsx:131 | exhaustive-deps：作用域/类别依赖应明确，不能盲加项目对象 | 加载器传项目 ID；声明类别与稳定 setter；保留 revision 刷新触发 |
| 49 | web/src/components/WorkflowManager.tsx:135 | exhaustive-deps：请求序号清理，DOM ref 启发式不适用 | 保留单调请求序号和核对；稳定 invalidate 回调显式取消 |
| 50 | web/src/components/WorkflowManager.tsx:137 | exhaustive-deps：稳定反馈/草稿回调遗漏，未证实运行故障 | 核对 useCallback 稳定性后显式加入依赖；不改变请求触发频率 |
| 51 | web/src/components/WorkflowManager.tsx:159 | exhaustive-deps：多余依赖，非运行故障 | 移除未读取的 setError；choose 不依赖整个 draft |
| 52 | web/src/components/configuration-ui.tsx:19 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 53 | web/src/components/configuration-ui.tsx:29 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 54 | web/src/components/configuration-ui.tsx:37 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 55 | web/src/components/configuration-ui.tsx:72 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 56 | web/src/components/configuration-ui.tsx:97 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 57 | web/src/components/configuration-ui.tsx:142 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 58 | web/src/components/configuration-ui.tsx:148 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 59 | web/src/components/configuration-ui.tsx:164 | exhaustive-deps：导航读取依赖与复杂表达式不完整 | 稳定 allowed 内容键，在回调内重建；声明 group；前进后退与草稿验收 |
| 60 | web/src/components/configuration-ui.tsx:164 | exhaustive-deps：导航读取依赖与复杂表达式不完整 | 稳定 allowed 内容键，在回调内重建；声明 group；前进后退与草稿验收 |
| 61 | web/src/components/configuration-ui.tsx:177 | exhaustive-deps：导航读取依赖与复杂表达式不完整 | 稳定 allowed 内容键，在回调内重建；声明 group；前进后退与草稿验收 |
| 62 | web/src/components/configuration-ui.tsx:434 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 63 | web/src/components/configuration-ui.tsx:444 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 非 JSX 函数与 hooks 移至 configuration-state.ts；RPC 类型独立 |
| 64 | web/src/components/reading.tsx:4 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 导航、字段映射与 hooks 移至 reading-state.ts |
| 65 | web/src/components/reading.tsx:12 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 导航、字段映射与 hooks 移至 reading-state.ts |
| 66 | web/src/components/reading.tsx:29 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 导航、字段映射与 hooks 移至 reading-state.ts |
| 67 | web/src/components/reading.tsx:61 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 导航、字段映射与 hooks 移至 reading-state.ts |
| 68 | web/src/components/reading.tsx:256 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 导航、字段映射与 hooks 移至 reading-state.ts |
| 69 | web/src/main.tsx:146 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 70 | web/src/main.tsx:151 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 71 | web/src/main.tsx:182 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 72 | web/src/main.tsx:1705 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 73 | web/src/main.tsx:1726 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 74 | web/src/main.tsx:1747 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |
| 75 | web/src/main.tsx:1935 | react-refresh/only-export-components：Refresh 边界约定，非已证运行故障 | 入口仅挂载 AppRoot；组件迁入 App.tsx |

## 17 项外部验收判断

保留 ignore 是外部资源分层，不是验收失败豁免；此前缺少全量入口和 CI 执行才是实施缺口。新增 native_acceptance.py 明确选择 Docker 16 项或含 GitHub 的全部 17 项，缺资源/失败/执行数量不符立即失败。默认 Rust 测试仍不隐含 Docker、模型或 GitHub 认证。

| # | 测试二进制 / 用例 | 首轮判断及处理 |
|---|---|---|
| 1 | command_tests / actual_node_red_green_environment_failure_and_stopped_receipts | 真实 Node Red/Green、环境失败与停止；首轮通过，加入统一入口及 CI |
| 2 | delivery_acceptance / actual_docker_red_green_candidate_reports_and_git_diff_match_their_specs | 真实 Docker 报告与 Git diff 的合同；首轮通过，加入统一入口及 CI |
| 3 | docker_resources / real_container_ownership_idempotence_stop_and_cleanup | 归属/幂等/停止清理；首轮通过 |
| 4 | docker_resources / nested_sandbox_cannot_broaden_the_outer_test_directory_write_grant | 嵌套不能扩大外层授权；首轮通过，CI 加载已有命名 AppArmor 策略 |
| 5 | docker_resources / dsh_runner_masks_private_state_and_proc_without_expanding_docker_privileges | 私有状态/proc 隐藏且不扩大 Docker 权限；首轮通过 |
| 6 | model_providers / pinned_dsh_uses_local_fixture_streams_tools_and_native_resume | 真产品缺陷：Ollama 的空 apiKeyEnv 导致固定 DSH 不注册路由。省略空引用并提供公开占位认证头；云服务商与需要密钥的自定义本地服务仍只写密钥引用；为后者补回归，两种云服务商、免密 Ollama 与带密钥本地路由四组流/工具/恢复重验通过 |
| 7 | permission_profiles / v2_binding_compiles_into_real_codex_and_dsh_file_and_network_boundaries | 真实 Harness 文件/网络边界；首轮通过 |
| 8 | permissions / codex_compiled_profile_allows_scoped_writes_and_denies_other_worktree_writes | 受限写允许与拒绝；首轮通过，但硬编码旧可变镜像 tag；改用入口冻结的 Codex image ID |
| 9 | permissions / full_access_container_writes_and_exposes_only_its_declared_trust_scope | 完整权限仍受外层挂载范围限制；首轮通过，但实际镜像未采用入口声明 ID，改为统一冻结 ID |
| 10 | permissions / outer_scoped_mounts_accept_user_defined_directory_names | 用户目录名不影响授权；首轮通过，修正旧镜像 tag 与声明验收镜像不一致的问题 |
| 11 | runtime_resources / real_two_recipes_probe_without_inference | 首轮通过，但固定可变 tag 降低可复现性；改为已核验 image ID 的专属内容 tag，保留两配方/无推理断言 |
| 12 | runtime_resources / real_prepared_image_survives_other_dataset_rebuild | 首轮通过，但缓存命中可能让两次准备得到相同镜像；加强为第二数据集实际生成不同镜像，再验证旧安装与两个内容身份 tag 都保留 |
| 13 | tool_integrations / docker_http_executor_validates_real_results_and_blocks_private_network_without_grant | 首轮 ECONNRESET，未到夹具；未改执行器连续 8 次通过，不能认定产品缺陷。改用标准 Node HTTP 服务与 RAII 清理；另发现 Linux 无 host-gateway 时负例可能因 DNS 失败假阳性；改用私网 IP 字面地址并断言 endpoint_denied。保留并强化精确路径、幂等/恢复/私网拒绝/输出 Schema 和请求计数 |
| 14 | tool_integrations / docker_mcp_both_versions_and_transports_discover_call_and_reject_drift | 两协议版本/两 transport 与 descriptor 漂移；首轮通过 |
| 15 | tool_integrations / docker_oauth_pkce_exchange_refresh_and_revoke_use_isolated_network | 真实 Docker OAuth/PKCE 刷新撤销，隔离协议服务与内存 vault；首轮通过，不声称验证真实 OAuth 供应商 |
| 16 | tool_integrations / docker_http_side_effect_timeout_recovery_artifact_and_redaction_are_real | 副作用/未知超时/恢复/Artifact/脱敏；首轮通过 |
| 17 | github_resources / lost_pr_receipt_is_recovered_by_the_original_marker_without_creating_a_second_pr | 使用 xfork24/devdesk-integration-test 既有 PR #41，读取并两次恢复同一回执；首轮通过。依赖专用 GitHub 授权，保留显式入口，不给普通 CI 增加业务令牌 |
