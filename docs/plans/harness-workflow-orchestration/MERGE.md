# main 合并及完整验收

日期：2026-11-11。合并、完整验收及用户追加的全新无人值守 Feature 复跑均通过。本轮执行平台为 macOS arm64，实际业务执行使用 Codex 和 Docker 中的 Node 24.13.0。

## 合并与主工作区

合并前 main 为 `1d2c95e`，实施分支 `codex/harness-workflow-orchestration` 为 `25e9c53`，共同基线为 `e0826a9`。合并提交 `cd48ccf` 保留两边历史以及 main 的十个后续提交；实施 worktree 和分支保留，没有向项目远端推送 main。

合并前有两处未提交改动：计划索引和未跟踪的原始方案。原文保存于命名 stash `290bc5f2d5432755fbfad90d7aebcaea5c7262c9`（Preserve main workspace before Harness workflow merge），以及独立归档 `/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-harness-main-merge-e41xpi_0/`。stash 与归档原始字节已交叉核对。

| 原文件 | 原始 SHA-256 |
|---|---|
| docs/plans/README.md | f41d6b44dd739ec460ff58a12f5478fcdea6390282f0cd6869a787279a1ba584 |
| docs/plans/harness-workflow-orchestration/PLAN.md | 17b4117e1187f2f7b75fbfc5f3c4c3edcce37799c20a3b6cd41172d5b00f22ba |

提交中的文档采用完整实施结果。上轮验收结束时，以上两个文件已按表中原始字节恢复为工作区未提交改动，包括原草稿的待实施文字；提交和包内文档保留完整实施结果，stash 和独立归档继续保留。恢复校验记录为 `/tmp/devdesk-harness-main-workspace-preservation.json`。原有用户运行数据不升级、不删除，也不通过改库制造完成状态。

## 冲突与默认合同

- 保留 Harness 项目经理显式派发、接纳和完成，以及 main 对 failed/paused 取消唤醒和收尾的修正。观察结果不会重新变成自动派发业务节点的依据。
- 默认 v5 图采用 main 最新业务合同：Feature 22 步、Task 20 步；保留 engineering_constraints、tests_snapshot/red_snapshot、逐增量 TDD、environment/termination，以及评审前的确切候选、完整差异和候选测试。
- 默认 confirmation 为 automatic，完成条件不可任意覆盖；Git permit 要求实际候选测试。独立评审仍须满足内容和独立性条件。自定义 Human 门禁与显式豁免功能保留，默认验收不使用它们。
- JSONL 保留 main 的文本合批与终态回执。配置、UI 与回归统一到 v5 当前合同，不恢复旧协议或旧调度路径。
- Feature 最终候选评审增加完整 Task 交接输入，同 closure_review 一样接收原始 TDD evidence。出厂闭包版本为 `2026.11.11-harness-v5-main-recovery`。

## 验收发现及修复

`4992a55` 修复长会话 thread/resume 回包触及 1 MiB 普通帧限制的问题。只对已登记的这一回复有界读取至 16 MiB，完整脱敏记账后移除调用方不消费的历史 turns；普通帧仍为 1 MiB、队列仍为 8 MiB。实际原生回复达到 1,096,133 UTF-8 字节后，仍成功发出 turn/start、收到 turn/started 并发生原生工具调用。证据为首轮数据集中的 `large-native-resume-proof.json`。

`57d510a` 修复子流程继承父级闭包后，重编译移除未用资源却使全部节点失效的问题；比较新图实际使用的角色、资源、权限与集成锁。增加受限 `revert_change`：只回退本实例最近的纯 upsert_step 变更，全部受影响替代执行须尚未启动，且不包含子流程调用。正常 preview/apply 创建新修订，校验不可变历史并原样复用原执行；不改变原始拒绝，不撤销外部作用。

首轮实际发生过错误影响全部 20 个 Task 节点的修订。通过受限回退恢复 14 个已完成节点，保留原始 TDD 与原 false 评审；随后项目经理重新预演文档交付澄清，仅影响 review/candidate_review 及下游 6 步，真实重新执行两个独立评审。澄清依据为用户原始要求：README 可选，正式文档通过只读节点的 result 端口交付。全部完成条件、规范和实际测试要求保留。恢复证据 `step-change-recovery.json`；父级追加证据输入只影响 candidate_review/advance/publish。并发期间过期预演被拒绝，项目经理重新预演后应用，未据此跳过条件。

`ed43002` 补齐全部实现后评审的原始证据引用；`8c1f7ff` 将完整 TDD 记录同实际原生命令观察交叉核对。`e039f07`、`59067e7`、`082c081` 收紧无人值守探针：补充输入、Human 门禁、暂停干预、额外控制输入、替换默认角色绑定或改写独立评审提示词均不能作为通过路径。所有推进决定还需追溯到同一个 epoch=1 的原生 Controller 或其持久化有界授权。`de80c3c` 增加实际原生 turn/start 封套核验，完整 rendered_input 必须出现在发出的文本消息中。

## 合并后的完整检查

| 检查 | 结果与可复查记录 |
|---|---|
| scripts/check/check.sh | 202 项 Rust 通过、15 项外部用例默认忽略；debug/release 各 43 项浏览器通过；Web、HTTP/SSE、模板重置、离线升级及运行准备通过。`/tmp/devdesk-harness-main-recovery-full-check.log` |
| 实际 Git、Docker、Permission | Git 3 项、Docker/命令 6 项、Permission profile 13 项、Permission 5 项全部通过，无忽略。`/tmp/devdesk-harness-main-recovery-native-checks/current-native-checks.json` |
| 真正 v4 Host 数据集升级 | 第二代完整出厂目录、旧门禁、材料、凭据哨兵保留并继续旧实例。`/tmp/devdesk-harness-main-recovery-upgrade/`；旧自定义门禁的签署仅属于升级测试 |
| 独立 macOS arm64 包 | 包外启动通过；完整证据输入、回退工具 Schema、当前 API、权限资产、升级入口均验证，模型调用/自动下载为 0。`/tmp/devdesk-harness-recovery-package.json` |
| 合并基线原生控制 | 实际 Codex 材料工作流完成，覆盖 queued、运行时绕过、派发/接纳、暂停/接管、Host 重启及 Snapshot 后继。`/tmp/devdesk-harness-main-orchestration-SrDKp2/`，根 `494d4bb0-bc1e-4668-803b-48e68887b897` |
| 首轮完整工程及最新离线复核 | Feature 22/22、Task 20/20 完成；原始 TDD、实际 AgentTask、固定 Git blobs 与确切 PR 版本通过。`/tmp/devdesk-harness-main-recovered-offline.log` |

首轮保留数据集 `/tmp/devdesk-harness-main-engineering-ARDQlN/`；项目 `07801e3c-7a42-413f-8ba9-b00ee711bdd1`，Feature `b4df553d-0fd1-4310-ae74-894fbf1807bf`，根 `c411977c-85ea-48fd-8926-960c186045a0`，Task 子流程 `3fe3d500-641b-442c-b40a-f9a3055d71fb`。该轮经历真实缺陷和受控恢复，不能当作一次未经干预的连续运行。

首轮证据包含两轮增量 Red/Green 及一次回归，退出码依次为 1、0、1、0、0；每条完整 JSON 与原生命令工具输出的字段和值一致。每对 Red/Green 使用相同命令及测试内容快照。平台 Red 的 7 条失败来自真实业务断言，原始加载错误没有包装为 Red；Green、verify 和 Task 候选各 7 条通过，Feature verification/候选各 11 条通过。测试快照与 Git blob 内容一致，最终候选包含与末次 Green 相同的源码和增量测试。

实际 Task documentation/review/candidate_review 和 Feature closure_review/candidate_review 的 AgentTask 均核对完整 evidence 与来源输出；四个独立评审实际收到逐次命令、原始输出、退出码、测试/源码快照、environment 和 termination。记录为 `execution-tasks.json`、`raw-tdd-evidence.json`、`native-tdd-observations.json`，其中 5 条原生命令记录均已交叉核对。`reviewer-wire-deliveries.json` 进一步核对实际发往 Harness 的完整任务封套（55,012—190,994 UTF-8 字节）；全部 5 个消费者均与持久化 rendered_input 完全匹配。Human 确认、人工批准、旁路和豁免为 0。

| 首轮真实发布 | head = merge = 已测试候选 |
|---|---|
| Task PR #33，目标 Feature 分支 | e81289d68fdd0f980e37e78c71d7ae4089faf394 |
| Feature PR #34，目标 main | 36577f018ac9403f3686d430e4f1b8ac0bfea1dc |

两次发布均通过独立 GitHub API 查询，保存于该数据集的 `github-independent.json`。首次两次未发出 turn/start 的恢复帧中断、错误计划修订及全部后续恢复记录仍保留，没有将失败记录改成成功。

## 验收完成后的全新无人值守复跑

用户追加要求在上述验收之后重新跑完整 Feature。采用最终代码、新项目和新 Feature，只有一次初始业务目标，不 resume、不追加指导，不替代业务 Agent 编写代码或证据。

数据集 `/tmp/devdesk-harness-main-unattended-6gmzz5e8/`；项目 `397aa807-fdce-464c-8aa3-59bb065c557c`，Feature `aff8748a-ae97-4dee-88e3-3c5d7b808773`，根 `8ec5f270-c55f-4493-9087-c6094d176c50`。入口使用 `--unattended`，日志 `/tmp/devdesk-harness-main-unattended.log`。项目经理自行通过唯一一次 bind_input 计划修订绑定已提供的需求事实，后续没有改写任何默认评审提示词或角色绑定。Feature 22/22、Task 20/20 均完成；只有一次初始控制输入，Controller 保持 epoch=1，没有接管、resume 或追加指导。

共 87 条推进决定：原生 Controller 直接产生 5 条，有界委托产生 82 条；每个委托均有持久化授权并追溯到同一个 epoch=1 Controller。Human 确认、人工批准、旁路、豁免均为 0；原始出厂节点、完成条件、规范、必需端口和 Git tests 要求保留。

本轮新产生的五条 TDD 记录为 Red(1)、Green(0)、Red(1)、Green(0)、regression(0)，全部含真实命令、原始 stdout/stderr、退出码、测试/源码快照、Node v24.13.0 / linux / arm64 环境和正常退出元数据。五条完整 JSON 与本轮原生命令观察一致，两对 Red/Green 的命令和测试快照一致。平台 Red 的 33 条业务断言实际失败；Green、verify、Task 候选均 33 条通过；Feature verification 和最终候选均 38 条通过，无失败、跳过、取消。完整快照内容与实际 Git blob 交叉核对。

Task documentation/review/candidate_review 及 Feature closure_review/candidate_review 均收到完整 evidence。实际发出的 turn/start 文本逐一包含完整 rendered_input，五个封套分别为 56,876、198,967、80,982、270,975、262,350 UTF-8 字节；不仅核对数据库冻结值，也核对了向 Harness 的真实发送。四个独立评审的实际 approve=true 来自各自业务 Agent，未通过人工端点或完成条件覆盖产生。

| 全新无人值守发布 | head = merge = 已测试候选 |
|---|---|
| Task PR #35，目标 Feature 分支 | bf15d16b8117d9d82129dd173b6080816a606497 |
| Feature PR #36，目标 main | 089c9b41405e2154166a04caa1467cad533d8a7f |

运行入口退出 0；使用最新验证器离线重放全部事实再次退出 0（`/tmp/devdesk-harness-main-unattended-offline.log`），没有重开 Controller、重派发或重复发布。该数据集的 `acceptance-summary.json`、`github-independent.json`、`execution-tasks.json`、`raw-tdd-evidence.json`、`native-tdd-observations.json`、`reviewer-wire-deliveries.json` 保存对应证据。验收私有凭据副本已清理。

## 最终交付核对

最终包为 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`，含 SHA-256 sidecar。最终包复验记录 `/tmp/devdesk-harness-merge-package-final.json`，验收打包基线 `2a32988` 的文件一致性记录 `/tmp/devdesk-harness-merge-final-delivery.json`。基线包使用提交中的完整文档，上轮随后按原始散列恢复用户两处未提交改动。本次按用户追加的 commit/push 请求将原文归档提交，主文档继续保留完整验收结果。

运行日志、JSON、协议账本及数据集均在临时目录，本计划目录仅保存 Markdown。当前测试范围为 macOS arm64 与 Docker 实际执行，编排 Harness 为 Codex；历史其他平台结果继续保留在原报告中，不算作本轮补验。

## 原始草稿提交归档

按用户追加的 commit/push 请求，将原始[方案草稿](original-draft/PLAN.md)和[计划索引快照](original-draft/PLAN-INDEX.md)逐字节归档。两份归档的 SHA-256 与上表一致，原路径采用已验收的主文档。归档仅调整 Markdown 文档，上述产品代码验收与真实发布结果继续适用；推送目标为 origin/main。
