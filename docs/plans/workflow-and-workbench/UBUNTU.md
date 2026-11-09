# Workflow 与工作台 Ubuntu 补验

日期：2026-11-09。源码基线：`b0e881928358b0cb82767cfcfd657c194262f6b3`，另含本轮发现的评审失败状态投影修复。本报告验证当前 Ubuntu 上的新治理、Workflow、完整 Agent 组合及统一工作台，不把旧版 Ubuntu 工程闭环计为本轮结果。

结论：按用户最终指定的仅 Codex 范围，Ubuntu 验收通过。完整回归、Linux 专项、Codex 完整 Feature 及修复后 release 权限复验均通过。DSH 曾完成需求/方案、Task TDD 和交付评审；在供应商余额不足后，用户明确要求暂停 DSH，已暂停其 Workflow 并保留原候选和失败历史，不将其完整闭环计为通过。

## 环境、数据及回归

Ubuntu 24.04.5 LTS / x86_64，Linux 7.0.0-42-generic；Rust 1.99.1、Node 24.13.0、Docker 29.9.3。独立数据集为 `data/ubuntu-permissions-workflow-20261109`，Host 使用 51762；旧 51751 服务没有被重启或中断。系统策略、镜像摘要和发布包详见 [权限 Ubuntu 补验](../permission-standardization/UBUNTU.md)。

临时日志、JSON、截图位于 `/tmp/devdesk-ubuntu-validation-20261109-prcVxM`；它们是本机临时证据，不是永久下载地址。运行对象、原生协议、Artifact、快照、冻结配置和工程回执保存在独立数据集中；计划目录只保留 Markdown。

| 检查 | 结果 |
|---|---|
| `./scripts/check.sh` | 首轮 Rust 131 项通过；修复评审失败投影后完整复验 Rust 132 项通过，8 项外部用例默认 ignored；两轮 debug/release Chromium 各 10 项均通过，含工作流编辑/发布/刷新、完整 Agent 和 Model/Tool 管理、内建组合和角色修订 |
| debug/release HTTP | 鉴权、Host/Origin、中文和空格路径、数据锁、幂等、重启、分库/切换和 SSE 均通过；release 在源码外运行 |
| runtime preparation | debug 和额外 release 检查通过；实际 Git 配合明确 Docker/GitHub 夹具验证中断恢复、按需准备、并发复用及失败重试 |
| 真实外部资源 | Docker/Git/Permission/Node 17 项通过；基于本轮真实 PR #16 的 GitHub 丢回执恢复另 1 项通过，无默认忽略项被冒充通过 |
| 治理及恢复规则 | 默认 Rust 回归覆盖 Scope/不可变修订/旧 Feature 冻结、非法图、精确失效、合入前确认、固定成员汇合、经验篡改拒绝、旧库迁移和终态只读 |
| 静态检查 | rustfmt、Python 探针语法和 `git diff --check` 通过 |

## 完整原生 Workflow

专用远端仅为 `xfork24/devdesk-integration-test`。场景以随机路径创建无依赖 greeting ESM 模块和独立测试，覆盖 trim、空白与非字符串 TypeError；每个 Feature 恰含一个 Task。讨论、采纳、正式需求与方案交付/评审、Task 断言 Red/Green、候选测试/评审、Feature 收尾、主干合入前确认、最终快照与资源清理由冻结 Workflow 驱动。

| Harness | Feature | 状态 |
|---|---|---|
| Codex / gpt-6.1-sol | `a4a84d1e-f0fa-466d-9eb1-42bb2ccbaf43` | completed；Task 和主干 PR 均已合并 |
| DSH / deepseek-v4-pro | `fb74e16f-a0c7-466f-96e3-b5e5bdde0e50` | 用户要求暂停；保留已完成阶段和未合入候选，不计为完整闭环通过 |

Project：`a9437b28-8fee-4d5a-8e58-0d790f1e7619`。Codex Feature 的配置锁在运行中即保存原始正文及摘要，用于在重启和后续 DSH Project 默认换绑后独立比较。

Codex 原生闭环结果：

- Task：`c1fb4c82-c39d-4ad7-a43c-124f7fd78de1`；实际 Red 为断言失败，随后实现、Green、文档、verify、正式交付 Review、候选测试和独立 Review 均完成。
- [Task PR #16](https://github.com/xfork24/devdesk-integration-test/pull/16)：`60f936ffdd486dfc07cea4bf974f9c3a89f3db98`。
- [主干 PR #17](https://github.com/xfork24/devdesk-integration-test/pull/17)：`6a74053cb3f4ba2a6d255f688426a1b26d04d44a`，父提交为 `faf9142a61cc037570c0b4b9b94c122e6f1d23a2`。独立 `gh api` 确认两个 PR 的 merged=true、head.sha=merge_commit_sha=精确候选。
- 合入前完成 1 次精确输入/候选确认；确认前 committed_revision=null。根 Workflow 与 Feature 均 completed，21 个根步骤全部完成，含最终快照和资源清理。
- 从 debug Host 正常退出，改用与已验收发布包 SHA 相同的 release Host 打开同一数据集。Feature 仍为 completed；旧配置锁 `27640c93-0101-411a-a6e1-6508fa052f85` 的原始正文和摘要 `a6d7e408f882845c7cace34d2b14023653aa8c40d0b236073123ff9128c0ef34` 完全一致。
- 随后 Project 换绑到新 DSH 组合并创建第二个 Feature，旧 Codex 锁仍逐字一致。已完成的旧流程允许动作列表为空，直接 pause/rework 返回 blocked，持久化记录不变；真实页面没有暂停、返工、重试或确认控件。

## 实际 Model、AgentID 与经验

以下均为真实原生模型轮次，结果依据持久化事件、工具调用与快照核对：

- 同一精确 Codex Harness `0.167.0+docker.09fbe4ee9575` 上实际运行 `gpt-6.1-sol` 和 `gpt-6.1-terra`；AgentRevision、composition_hash 和 AgentID 分别保存。
- Global AgentID A：`0d71c811-9ff7-43ea-8670-f1440a436888`；B：`3faacf7b-c1df-48c2-9727-7f14ee7fa95a`。B 的历史中不存在 A 的来源 marker。
- 原生工具先读 `/experience/index.json`，再读取相关 detail_file；来源快照 `f41c3b9b-4fe2-49fe-a82f-ebf2327d671e`，完整性查询为 verified。
- 来源项目为上述 Project；恢复项目为 `03619f2a-0099-4488-a3d2-c67cd72a985d`，新 Session 为 `2c992797-6a56-49ec-bebb-65e7633dfce3`。Global 实例可按项目名称和精确时间读取跨项目经验，Project 实例越界被拒绝。
- 新原生 Session 实际读到继承来源 marker；恢复前后当前 Project 对象相等，没有覆盖当前配置或代码。
- DSH 完整组合 Session `4d8751d1-81eb-4acb-b016-a73773c7cf9c` / Turn `ea1e503d-1e1b-49cc-840e-ed0a2d860c00` 使用 `deepseek-v4-pro` 完成真实输入消费和精确 marker 输出；旧角色入口与独立 Model 覆盖均被拒绝。

## 真实工作台

Chromium 在实际验收 Host 上检查 1440px 桌面和 390px 窄屏：7 个导航入口、6 个详情页签和 21 个根流程节点正常；直接 Feature 地址及刷新保留定位；无 pageerror，窄屏 document.scrollWidth=390、innerWidth=390。导航中的实际运行计数和流程状态来自本轮原生工程数据。截图仅保存在临时证据目录。

## 最终数据核对

按用户最终的 Codex 范围核对通过：3 个 SQLite 库的 integrity_check、foreign_key_check 和版本检查通过；273 个已提交 Artifact 的路径边界和 SHA-256 一致；40 个 RuntimeBinding 和 64 个 ExecutionPermissions 通过 Schema 校验，全部 64 个执行已停止、导出协议并 released，对应容器均已停止或移除。

共校验 10 个 Schema，以及 421 个持久化 Permission/Workflow/Model/Tool/Agent/AgentID/运行绑定/权限正文（包含分库副本）。Codex 的两个 WorkflowInstance 均 completed，1 次主干确认及 2 个 PR 已独立复核；旧 Feature 锁完全保留。DSH 的两个未完成实例和一个未合入候选保留，根 Workflow 已暂停；两次供应商失败 Turn 保留，不改写为成功。

## 中断及修正记录

1. 同时编排原生 Workflow 与跨项目经验探针时，活动项目切换使驾驶脚本收到预期 `project_changed` 保护。原 Feature 的内核仍继续执行到等待采纳；保留原失败日志和报告，在经验探针结束后以原 Project/Feature 显式 `--resume-project/--resume-feature` 续跑，没有重新创建 Feature 或伪造工程状态。
2. 旧探针平台文字写死 macOS，已改为实际 OS/架构；完整 Workflow 探针增加 `--provider codex|dsh`，并在解析精确 Harness 前显式准备当前平台安装。
3. 原生组合、权限、发布包探针中的旧入口及 schema 假设修正见权限报告；这些属于验收工具修正，没有放宽领域门禁或 JSON 输出契约。
4. DSH 首次 Task Review 在最终答案中添加 prose 和 Markdown 围栏，Run `95390551-207a-4303-b67f-2fd3ee667668` 被严格契约记为 adapter_contract。实测同时发现产品缺陷：没有形成 Review 事实的失败 Run 未投影为步骤失败，Workflow 停留 active，无法使用步骤重试。已在 [workflow_runtime.rs](../../../crates/core/src/workflow_runtime.rs) 修复：采用该交付最新评审 Run 的终态，失败/中断/取消时等待显式重试；已持久化但未派发的 retry intent 保持 active，避免重试前再次求值导致步骤重新失败。
5. 新增 [治理回归](../../../crates/adapters/tests/governance.rs) `failed_workflow_review_waits_for_explicit_retry_and_preserves_its_binding`：修复前稳定复现 active/failed 不一致，修复后覆盖停止自动推进、显式重试、派发前再次求值、原失败记录保留和 Reviewer/运行锁保持。原生 JSON 解析未放宽，原失败输出和中断前工作流投影保存在临时证据与数据集中。
6. 修复后的 release Host 从原数据集恢复，步骤正确投影 failed 并返回 workflow.step.retry 允许动作。显式技术重试创建 Run `f2eca3d8-2bc5-42bb-b7fa-2e4505953282`，原 Run 保留；实际 RuntimeBinding、Permission 和 Reviewer 与原执行逐字段相等。重试评审通过，沿用原 Task/Feature 推进，没有重新生成实现或替换已采纳测试。
7. 随后的 Task 候选评审 Run `4ac91aef-b3df-41b6-ac24-f7e436b849da` 返回 provider 失败；同一时间的新 release DSH 权限探针明确返回 HTTP 402 / QUOTA / Insufficient Balance，原生脱敏协议保留。修复后的步骤已正确变为 failed。用户随后明确要求“暂时不测试 DSH，仅测试 Codex”，已暂停 DSH 根 Workflow 并停止新的 DSH 模型请求；候选、工程和权限锁均保留，未把未完成阶段计为通过。

## 验收边界

离线、注入故障、无模型真实资源测试和真实模型/GitHub 结果分别列示，不将 A1–A44 全部宣称为逐项外部模型验收。结论限于当前 Ubuntu x86_64；Archive、冷数据恢复、Swift 客户端、任意 BPMN/递归工作流和任意模型 endpoint 不在本轮实现范围。
