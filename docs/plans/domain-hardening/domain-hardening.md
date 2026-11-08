# 领域与工程接入基础补强验收

日期：2026-11-08。状态：H1–H4 已完成。范围基线：[补强计划](PLAN.md)。本轮验证平台为当前 macOS；不声称 Linux 已复验。评审性质为作者自检和自动化验证，不冒称独立评审。

## 计划完成映射

| 计划 | 落地内容 | 可执行证据 |
|---|---|---|
| H1 四项缺陷 | 文档权威目标与 revision；Run 独立锁与重试；正式交付结构；按 owner 失效传播 | hardening_document_target_cannot_bypass_revision、hardening_reviewer_retry_preserves_full_binding、hardening_empty_success_cannot_accept_plan、hardening_task_document_change_is_local |
| H1 影响范围补全 | Feature Doc 不使规划/兄弟 Task 合入失效；Task 重规划中断旧输入 | completion_feature_doc_edit_keeps_planning_and_sibling_merge_valid、hardening_replanned_task_uses_current_criteria_without_rewriting_attempt |
| H2 追踪与冻结 | 验收条目、设计/测试修订引用、Task/Attempt scope、TestResult 操作/阶段/代码/环境/定义校验 | completion_context_history_and_delivery_scope_are_enforced、原 TDD/来源/Override 测试 |
| H2 身份与规范 | Agent 身份根与修订分开；Review 与候选验证使用源交付的 Standard/执行锁；能力不足拒绝绑定 | completion_agent_revision_is_not_a_separate_reviewer_identity、completion_reviewer_and_merge_keep_source_standard_when_feature_defaults_change、completion_binding_checks_capabilities_instead_of_provider_literal |
| H2 上下文与定位 | Context 追加修订；Snapshot 捕获已执行 Context 和来源；GitContext 冻结 Workspace/branch/worktree | completion_final_snapshot_captures_executed_context、completion_git_and_test_requests_freeze_workspace_and_runtime_routing |
| H2 历史迁移 | v1→v13 顺序迁移；未知绑定、范围、输出和代码定位不倒填 | hardening_v1_migration_preserves_reviewer_binding_and_legacy_provenance |
| H3 取消/失效竞态 | 外部轮询不占用 writer；调用前登记意图；回执先保存，接纳时重新验证 | completion_cancel_during_poll_discards_late_success、completion_input_change_during_poll_discards_late_document |
| H3 回执与清理 | 导出失败复用回执；资源在产物提交后清理；失败历史保留及幂等重试 | completion_export_retry_reuses_persisted_receipt、completion_cleanup_retry_preserves_failed_history_and_is_idempotent、completion_cleanup_crash_replays_original_resource_identity |
| H3 未知结果 | 中断不自动成功；错误核对回执不能解除资源所有权；匹配原供应商后才核对 | completion_restart_keeps_unknown_resources_until_explicit_reconciliation、completion_wrong_reconciliation_receipt_does_not_release_owned_resources |
| H3 目标推进 | 意图/回执/逻辑推进分开；固定供应商和操作 ID；已推进/未推进/未知结果分别处理 | completion_git_crash_after_side_effect_recovers_from_original_provider_receipt、completion_git_unknown_outcome_is_reconciled_without_duplicate_advance、completion_confirmed_not_applied_refreshes_target_and_requires_new_candidate |
| H3 派发前门禁 | 接纳命令先返回 operation_id；调度时复核，已失效意图不调用供应商 | completion_invalidated_advance_intent_never_calls_provider |
| H4 阶段数据与 Web | 冻结输入与阶段输出分开；每 Run 复用首次请求；回执/核对/清理重试均有 Web 入口 | completion_stage_outputs_do_not_replace_frozen_inputs、completion_parallel_output_does_not_change_existing_run_request、4 项 Web E2E |

## 领域对象覆盖

| 对象组 | 表达与约束 |
|---|---|
| Project / Workspace / Feature / Task | 稳定归属、配置、独立状态和绑定、依赖、逻辑工作区、冻结实现范围；Project 设置变化不回写旧 GitContext |
| Requirement / Design / Test / Doc | 共享不可变文档修订的存储结构，各有正式交付契约；可验证验收条目、派生引用、Task 范围与测试映射；Artifact 文件与来源 |
| Spec / Standard / Skill / Agent | 不可变 Definition 修订和 slot 锁；源标准不被默认配置改写；Agent 身份根用于独立评审校验 |
| Attempt / Run / Session | 冻结输入/输出分离、DAG、执行锁、技术重试、新尝试、取消和中断；外部会话身份与实际操作回执保留 |
| Harness / Sandbox / Resource | 定义、版本能力与运行实例分开；资源所有权、导出/释放顺序、错误/未知结果保留、幂等清理 |
| Context / Session Snapshot | 当前可变 Context、追加修订历史、不可变捕获、继承及确定执行来源；真实环境重建仍属原计划延期范围 |
| Delivery / Reviewer / Review / Override | 固定版本/阶段/证据，评审分配与身份、逐项 checks、保留原决定的质量例外；完整性/来源/stale/目标竞争不可豁免 |
| Merge / Artifact | 本地变更请求实例、源/目标/候选、验证与 Review、持久化推进核对；文件哈希、封存和受控读取 |
| Archive | 类型化、可查询的只读范围预览；不写“已归档”业务事实，导出/冷数据/恢复仍按原范围延期 |

## 最终验证

最终 `./scripts/check.sh` 全部通过，原始输出保存在 [验证日志](domain-hardening-validation.txt)。

- Rust：**65 项通过**（62 项集成/契约测试 + 3 项领域测试）；其中本次完成计划新增 completion_ 测试 20 项。
- TypeScript/Vite 构建通过；debug/release 二进制构建通过。
- debug HTTP 与 release 独立复制二进制 HTTP 检查均通过。
- debug Chromium：**4 项通过**，1.1m。
- release Chromium：**4 项通过**，29.0s。
- `cargo fmt --all -- --check` 与 `git diff --check` 通过。
- 实际运行平台为 Darwin arm64（macOS Apple Silicon）；Linux 未在本轮复验。

全量浏览器验收发现的 Green/Docs、Feature Tests/Docs 并行输入变动问题已修复，并由固定顺序的 `completion_parallel_output_does_not_change_existing_run_request` 覆盖；最终主流程在 debug/release 均通过。

## 可复用的实现边界

本轮已经完成公共领域模型、明确请求路由、内核操作账本、回执接纳和恢复框架，不再将这些 H3 内容推迟到真实接入轮。外部供应商故障由有状态的测试夹具模拟：在实际修改夹具外部状态后、回执保存前抛出中断，再重开数据库验证核对与不重复副作用。

真实 SDK、账号/凭据配置、供应商版本协商、事件/工具调用映射和供应商端操作查询/去重实现属于下一轮接入。内核恢复框架不等于供应商恢复能力已验收；真实 GitHub/Harness/Docker 应按 [接入契约](../../real-adapter-contract.md) 使用隔离资源跑真实用例，随后才开放 live。

本轮没有连接真实账号、执行用户 Git 仓库、启动 Docker 或调用真实模型。模拟会话/环境/变更请求均用明确的模拟 ID 与来源；Archive/迁移执行及真实环境重建保持原计划的延期边界。历史缺失信息保持缺失，已完成历史仍冻结。
