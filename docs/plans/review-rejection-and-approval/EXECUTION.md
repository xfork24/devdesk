# 实施与验证

日期：2026-11-18。独立工作树 `/tmp/devdesk-review-rejection-and-approval`，分支 `codex/review-rejection-and-approval`，起点 `822932b`。原 PLAN 保留历史原文；本记录描述本次落地范围。

## 已实现

- Workflow 新增类型化 `rework_policies`、`review_policies`，校验步骤、端口、依赖、策略引用及额度；编辑器可配置最大额外返工次数、三种方式和人工例外开关。出厂默认 3 次，Task 实现与候选评审共享策略，Feature 各评审绑定实际作者入口。
- 经理读取真实拒绝报告后，自行选择责任文档生产步骤、有效 Human 确认点之后、对应子流程重执行。`workflow.rework.preview` 列出保留/失效结果、需停止者、父调用撤回和预算；提交绑定同一预演及原始版本。新任务带入报告引用、归因、范围和验收要求。
- 共享额度以根/工作/策略身份持久化在现有 governance 事实中，初始化、原子 CAS、不可变消费记录和原生命令幂等复用既有提交机制。技术 retry 不重复扣费；已开始的受策略约束子流程不能通过技术 retry 整体重建。计划重置同样检查额度，子实例替换保留已用次数，纯工具分组不误扣。
- 子流程重执行核对明确代码基线、平台快照原回执、根/工作归属、代码头及 Git 写者预检；只替换原调用集合的该成员，保留旧实例、报告、调用血缘和 Task。父结果及受影响后继撤回；已完成 Task 的合法返工恢复工作状态，未知物理副作用仍先核对。
- `workflow.decision.request` 支持申请 `workflow.step.approve`，Agent 仍不能直接执行 approve/resolve。内核从完整冻结输入和真实报告生成有界 approval_basis，核对版本、来源及硬规则，同一结果不能重复申请。Human 签署保存 manual_approval、override_completion、意见及例外范围，经理随后显式 accept。原报告保持 approve=false，后继评审上下文携带仍有效的上游例外引用。
- 根经理页汇总子实例真实申请，嵌套页不重复提交；展示待经理研判、待用户 Approve、过期/拒绝与额度，人工意见必填。解决签署后请求修订导致卡片误显示过期的问题，展示真实决定和用户签署意见。
- 阻塞主链接进入 execution/overview，携带实际项目、Feature/Task、实例和执行。状态查询沿实际祖先链定位步骤页、子流程页，页面展开、滚动并聚焦；刷新和前进后退保留身份，历史执行仍有独立入口。
- 运行中采用使用 ChangeSet `set_review_policies`，保留旧执行合同、冻结资产和报告；首次采用通过 Human 授权的 `adopt_rework_baseline` 明确历史 used/evidence，不能重置已有计数。
- 发布新出厂修订并闭合引用图：Feature 10、Task 9、Standard 8、编排 Skill 5、Codex PM Capability 12 / Agent 13；DSH 原有根编排能力限制保留。同步协议、工作流、配置和工作台文档。

## 验证

| 检查 | 结果 |
|---|---|
| `cargo test --workspace --locked --offline` | 586 passed，0 failed，30 ignored（原有显式忽略项） |
| 最终代码 `configured_workflows` 集成回归 | 61 passed，0 failed |
| 返工定向回归 | 8 passed；含 0/1/3 额度、方式切换、重放、最后一次额度并发、子后继、历史基准、原始报告保留、真实本地 Git 写者保护 |
| Chromium：workflow-approvals / review-rework / model-approvals / workflow-authoring | 18 项通过；最终构建复核见临时证据目录 |
| `scripts/check/check-style.sh` | rustfmt、ESLint、TypeScript、Prettier、Web build、clippy `-D warnings` 通过 |
| 出厂目录、架构、`git diff --check` | 通过 |

原始日志和截图保存在 `/tmp/devdesk-review-approval-evidence/`。浏览器用专用空数据目录，测试中按用户授权代签审批。Rust 使用显式端口/协议夹具及真实本地 Git；这些结果不等同于真实付费模型服务验收。

## 使用与边界

4318 的既有数据、报告与会话未被接管或签署。其冻结旧 Skill 不会因本地文件更新而自动改变；采用时需先保存旧依据，明确历史额度，应用新策略并创建采用新 Skill/Capability 的后继经理会话。真实 Codex 经理自主决策、真实模型/远端工程和原 4318 场景尚未作为本轮实机验收。没有新增数据库表或数据集迁移。

本工作树保留独立改动，未合并 main。实施期间 main 新增的其他计划文档及另一工作树未纳入本次改动。
