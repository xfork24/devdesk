# main 最终验收记录

日期：2026-11-15（Asia/Shanghai）。技术验收通过；用户对本次实施和工程产物的签收待用户核对。

## 结果与代码身份

C1、C2、C3、C4 已依次在独立 worktree `codex/core-workflow-follow-up` 实施并合入本地 main，随后在 main 开展最终测试。运行实现提交为 `3324d1e66a8611d29ea03dc8da4f2a7baafac532`；后续提交补充三 Task 用例、证据核验和报告支持，未修改冻结运行源。最终 main 身份和源码归档见证据目录的 MAIN_FINAL_IDENTITY.json。

仅处理基本执行、工作流调度、任务分配与验收。14 项提案的取舍见 [ASSESSMENT](ASSESSMENT.md)，阶段实施和失败见 [EXECUTION](EXECUTION.md)。未扩展到账号等外围功能。

真实场景使用从 main 构建的冻结 release Host，SHA256：`fb25758349a079724e82f90fafa98b694c6833553c1f71d8f5da1d85fd97513f`。实际回归使用的 debug/release 二进制另存，内容身份分别记录，不将签名不同的文件宣称为字节相同。

## main 测试

| 通道 | 结果 |
|---|---|
| 完整 check.sh | 返回 0；Rust 290 项通过、17 项按原配置忽略；HTTP、连接、模板、数据集和运行准备通过 |
| 浏览器 | debug 57、release 57 项通过 |
| Codex/命令/Docker 定向 | 10 项通过 |
| 同根三 Task | B 在数组首位且依赖 A；foreach=2、根 worker=1；C 独立完成，B 不占位；测试 Human 精确签署 A 后全部完成 |
| 证据读取器离线回归 | 14 项通过，包含错误摘要、缺块、异 Turn、未知失败与错误 UTF-8 边界的拒绝 |

分通道次数不直接相加作为不重复测试总数。三 Task 的端口夹具与真实外部服务验收分别记录；测试 Human 不冒充用户签收。

## 两个真实工程现场

专用 GitHub 仓库为用户明确确认的 `xfork24/devdesk-project-1`。两个场景均使用真实 Codex、Docker、Git 与 GitHub；最终审计核对当前 84 个步骤、172 个业务派发/接纳/完成决策、停止证明及 424 个注册 Artifact。

| 驱动 | 结果 | 自检调用 / 结构有效 / 报告定位有效 | 增量 Red/Green | PR |
|---|---|---|---|---|
| Rust | 42/42；业务链连续完成，无经理 Controller | 19 / 19 / 7 | 2 轮、5 个独立记录的 run | #10、#11 |
| Agent | 42/42；网络中断后同数据集、原预算恢复 | 17 / 17 / 7 | 2 轮、5 个独立记录的 run | #12、#13 |

Rust 项目 `126a2f67-80e8-49d8-a557-30e025011b50`，根 `fe0edc6c-4729-4b52-8844-ffa159ae6ab0`；Agent 项目 `a00846b3-203a-4786-8f83-9fb92628d88f`，根 `77515602-0015-4040-b0e1-4920c6e01b5f`。每个场景另有 22 个启动前输入绑定替换产生的 queued 历史执行，不计作失败或重试。

实际测试包括业务断言产生的 Red、同份独立测试的 Green、至少两轮增量 TDD、独立评审、候选测试、精确父提交及远端 lease、发布观察回执。最终 Feature 保持 Task 独立测试不变，仅修改冻结目标允许的四个路径；默认执行预算、foreach=1、默认零重试、独立验收与 Human 边界保持。零 waiver、零默认 Human 批准。

PR 观察：Task [#10](https://github.com/xfork24/devdesk-project-1/pull/10)、Feature [#11](https://github.com/xfork24/devdesk-project-1/pull/11)、Task [#12](https://github.com/xfork24/devdesk-project-1/pull/12)、Feature [#13](https://github.com/xfork24/devdesk-project-1/pull/13)。平台通过 Git CAS/精确 lease 推进目标，再查询 PR；没有调用 GitHub 合并接口。完整回执与实际 SHA 见各场景 publication-evidence.json。

## 失败与恢复的界限

- Rust 业务 42 步完成后，旧证据核验器误判 UTF-8 分块偏移及已有明确拒绝回执的只读参数错误。修正核验器后完成只读重验，仍严格核对原生请求/回复、精确边界、完整字节覆盖、重叠一致、摘要与冻结交付；14 项离线回归通过。前后业务表逐项相同，没有重新派发、接纳或发布。
- Agent 在完成 37 步后发生用户确认的网络异常，原 600 秒控制 Turn 超时并实际取消；旧执行 released、writer_revoked。最后两次只读调用均有成功回执，无未知写操作。用户要求继续后走公开 takeover/resume，同一数据集保留原 37 个执行的全部字段及原 45 个输出记录，原控制策略和预算不变，完成剩余五步及根完成。该结果包含有指导恢复，不宣称全程无人值守成功。
- DSH 专属实机项因镜像缺失及准备 provider_failure 未完成；原整组失败日志保留，未将该项记为通过，也未实现 DSH 工具桥。本次核心真实闭环使用 Codex。
- 技术错误、取消、规范拒绝、技术重试和历史 queued 执行分别记录。结构/位置检查通过不替代语义评审，技术通过不代替用户签收。

## 完整报告与现场

完整执行报告和逐步材料位于本机专用证据目录，源码仓库仅保存 Markdown 验收记录：

- [完整执行报告](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/ACCEPTANCE_REPORT.md)：C1–C4 实施、验收矩阵、实际 Actor、失败恢复、发布与内容身份。
- [最终审计](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/FINAL_EVIDENCE_AUDIT.json)、[恢复前后核对](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/agent-engineering/network-recovery-proof.json)、[原生实施命令核对](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/NATIVE_IMPLEMENTATION_AUDIT.json)。
- [Rust 逐步记录](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/rust-engineering/EXECUTION_SCENE.md)、[Agent 逐步记录](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/agent-engineering/EXECUTION_SCENE.md)、[三 Task 夹具记录](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/three-task-core-scene/EXECUTION_SCENE.md)。
- [现场保留说明](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/RETENTION.md)、[逐文件摘要清单](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/EVIDENCE_MANIFEST.json)、[清单核验结果](/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14/main/manifest-verification.json)。

每步的精确 Agent/模型、Session/Turn、实际运行、控制 Actor、决策原文、输入摘要和产出记录均在逐步报告及 step-ledger.json 中。保留实际源码、独立/增量测试、Git 仓库/worktree、完整 Host 数据、Artifact、原生日志、各次脚本、原失败、停止后一致快照、实际镜像归档和实际 Host。临时 TDD 采集脚本从真实原生命令重建并注明来源，不伪称停止后直接导出。

认证副本在场景结束后清除，原认证源独立保留。完整本地工程数据没有宣称为已全面脱敏的公开分享包。未删除用户数据、覆盖并行文档工作或清理测试现场。
