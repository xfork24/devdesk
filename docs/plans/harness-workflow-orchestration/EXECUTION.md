# Harness 工作流改造实施与验收记录

日期：2026-11-11。状态：P0—P8 已完成，A01—A18 验收通过。

工作副本：`/Users/hengliu/.codex/worktrees/harness-workflow-orchestration/devdesk`。分支：`codex/harness-workflow-orchestration`；起点：`e0826a9`。原 main 工作区及用户修改保留；未对原数据集执行升级。所有真实工程操作使用隔离临时数据集及专用测试仓库；本次实现未合并或推送到 main。

## 实施结果

| 阶段 | 状态 | 交付 |
|---|---|---|
| P0 | 完成 | CONTRACT.md、领域对象、Workflow v5、agent-task/v5、orchestration-task/v1、控制 Schema、actor/policy、global 14/project 45 |
| P1 | 完成 | observe 与 dispatch/accept/complete 分离；Agent、Tool、Workflow、Control 均显式推进；无旧自动推进回退 |
| P2 | 完成 | 独立 Controller、21 个 Codex 原生工具、真实 Session/Turn/worker 来源、授权与租约、幂等意图及持久回复 |
| P3 | 完成 | 持久唤醒与事件合并、单写者、epoch 接管、Snapshot 后继恢复、预算及并发为一时的执行额度轮换 |
| P4 | 完成 | typed ChangeSet、preview/quiesce/atomic apply、冻结合同历史、carry-forward、新执行身份、Task 范围与依赖修订、指定子调用 override |
| P5 | 完成 | TypedMaterial、Stage bypass、规则 waiver、Standard 处置、当前候选 ActionPermit；实际 Git 豁免接纳回归 |
| P6 | 完成 | Feature 21 步/七阶段、Task 20 步/七组、项目经理 Skill/Capability；对话、直接控制、计划编辑、精确人工决定、历史和移动布局 |
| P7 | 完成 | 明确节点的有界 segment；接管或计划变更撤销授权；分块材料、原合同与大计划分页；工具和唤醒预算 |
| P8 | 完成 | 全套自动化、实际 Codex 材料流程与完整 Feature/Task 工程、Docker/Git/GitHub、权限、离线升级、真实旧库、原始证据独立核对及 macOS arm64 独立包通过 |

项目经理在 Harness 中选择下一步，通过工具派发、接纳、返工、变更计划和明确完成实例。core 保留身份、范围及资源授权、DAG/Schema、不可变历史、真实回执、停止证明、幂等/CAS 和 Git 候选一致性。业务顺序、阶段取舍和可豁免门禁来自当前计划及编排策略。

编排执行器当前支持 Codex。DSH 缺少动态平台工具桥时在配置和执行边界明确拒绝编排；它仍可用于不依赖该桥的业务 Step。此次没有增加 DSH 编排适配。

## 完整真实工程

数据集：`/tmp/devdesk-harness-engineering-20261111c/`。项目：`aeb6a586-3ef9-44a0-826a-c398854fb4c0`。

| 对象 | 身份与实际进展 |
|---|---|
| Feature | `f84d80be-0356-45d6-8e4b-727a41ecc654` |
| 根 Workflow | `21713c81-1ff8-45c9-b8cf-6f0a86813ed7`；21 步和根实例均由项目经理明确完成；Feature 领域状态 completed |
| 唯一 Task | `58eeed5b-19d3-41ef-9926-e8b858823705`；领域状态 completed |
| Task 子 Workflow | `4b9d54b5-2300-4a6b-ac1b-37fa77d1fdb7`；22 步均由项目经理明确完成 |
| Task 发布 | [PR #31](https://github.com/xfork24/devdesk-integration-test/pull/31)；head 和 merge_commit 均为 `6dc2e147bd6a349c1d2135abc87975086b20d31a`，已通过 GitHub API 独立核对 |
| Feature 发布 | [PR #32](https://github.com/xfork24/devdesk-integration-test/pull/32)；head 和 merge_commit 均为 `7c8480653b5197e8c72916ce2871d3200f55020b`，已通过 GitHub API 独立核对 |

Task 的原始 Red 在不可变候选 `673a29adeb2dd4e908580364cf10d9738c92a866` 上实际失败；后续 Green、verify 和最终候选各有真实 Docker 回执，6 项测试通过。独立测试内容与摘要保持一致。实际评审、人工确认、Git advance、发布和 handoff 均保留，子实例由原生 `workflow.complete` 明确完成。

Task 完成后，在没有业务 writer 的检查点停止并重启 Host，项目经理通过后继会话接纳父端结果并继续 Feature，没有重复发布。Feature 新增独立测试后，本地双文件及平台整体验证均为 11 项通过；代码封存为 `d3733111ea5a79e123f9a7e7e61f0e39b1448e9d`，独立收尾评审已通过。

Feature 最终候选为 `7c8480653b5197e8c72916ce2871d3200f55020b`，唯一父提交为目标主干 `3d165e4635e82119788ccf625007e6a3e8bcd231`。新 candidate_tests 操作 `3b87ae01-a91e-4eea-9aab-d269dc8b903d` 的平台结果为 succeeded、退出 0、11 passed；实际运行区间为 unix_ms:1794356326548—1794356326866，原误判 timeout 的回执仍保留。

最终独立候选评审通过后，验收入口经 Human 命令签署确切执行、输入摘要与输出引用，再由项目经理接纳、推进及发布。三个 Human 门禁均精确匹配，没有 completion override、Stage bypass 或 waiver。21 个节点完成时根实例仍为 running 的检查点已经保存；随后原生项目经理明确调用 workflow.complete，根实例及 Feature 领域状态均变为 completed。

严格复验通过：原始 Red 与平台 execution 逐字段相同，补充诊断沿用同一 Red 候选/镜像/配方/测试并被两次评审及 handoff.inputs 实际消费；原始文件全文和 SHA 对应真实 Git blob，两次最终候选均保留已测试/交付的文件内容，Feature 候选包含独立测试。首次终验在 handoff 包装层读取处报 KeyError，实际材料完整；修正校验器读取 inputs.red_diagnostics 并要求最终 tests.status=succeeded 后，通过 Host 重新读取与无认证离线校验。该校验错误没有改写业务结果，也没有重新发布。

该工程经历实际失败、修复和同一数据集多次恢复，并非首次无中断通过。Feature 独立测试是在 Task 实现后编写，**没有 Feature 实现前的独立 Red 记录**。收尾材料和评审明确保留该历史偏差；真实 Task Red/Green 及 Feature 回归分别记录，不能声称严格的双层实现前 Red 已成立。

## 验收发现与修正

- 连续写入者之间缺少封存：默认 Task 在 tests 与 red_baseline 之间新增 `tests_author_snapshot`，将基线 Agent 的 workspace 输入及后续封存来源绑定到该确切版本；保留原 tests_snapshot 作为 Red 版本。出厂 Task 因此从计划的 19 步调整为 20 步。Git 脏工作树保护保持有效。
- 当前冻结 Task 由项目经理通过原生 preview/apply 采用封存修订；另保留 `recovery_snapshot` 和 `red_diagnostics` 两个恢复节点，共 22 步。前者封存失败 writer 已写出的真实模块及 README，后者补充真实诊断；没有删除既有成果或跳过门禁。
- 原 tests.run 缺少原始证据：execution 现在来自实际固定版本、Docker 命令、时钟、退出码和分离 stdout/stderr，包含测试文件全文及 SHA。缺失文件和环境错误明确保留 gaps/stderr；普通协调命令不扩大 stderr 捕获范围。
- 独立评审 `4c9c394e-9abf-40b3-970b-faa1441f6498` 曾真实拒绝：旧 reporter 只有类别，不能核对缺实现的原因。现保留实际 message、stack、cause 与测试输出；输出作为数据封装，伪报告不能改变分类。真实 Docker 回归通过。
- 补充诊断在原 Red **相同候选、镜像、配方、测试文件**上实际重跑，证明 greeting.mjs 缺失及 ERR_MODULE_NOT_FOUND；经原生修订明确传给独立评审、候选评审和 handoff。新独立评审通过，原拒绝与旧回执不变，后补诊断不改写历史 Red。
- 人工确认已存储但旧摘要仍显示等待：复用内核精确确认匹配规则，提供签署引用与 human_satisfied，优先显示待办并将确认纳入进展预算。第 17/18 节点回归验证可见性及确认后仍需显式接纳。
- 项目经理误继承 owner 的业务代码视图：Orchestration/Controller 的讨论代码视图改为 None。带真实项目仓库元数据的测试验证 SessionStart 和每个 Turn 均不挂载业务代码；实际 Feature tests_snapshot 重试成功，业务 writer 停止检查继续有效。
- Codex 原生来源识别、重复工具回复、持久发送与 Session revision 竞争、待发事件合并、租约与 epoch 检查均补充回归。原生读取回复保留真实数据，每次回复只有一个 content item。
- 父 Workflow 的业务预算不计子流程等待接纳且无 writer 的时间；Feature Task 调用上限为 7200 秒。仅延长已展开调用 timeout 时生成 budget_updates 并保留子实例及原合同，其他合同变化继续执行正常失效。
- 最终候选曾出现 11 项通过、退出 0 但平台判为 timeout 的回执。独立 Docker inspect 证明实际容器只运行约 212 毫秒；原判定错误计入准备与 Host 延迟轮询。现在用 Docker 实际 StartedAt/FinishedAt 计算预算及原始执行时间，运行中按实际开始时间检查；已经退出但真正超时的容器仍拒绝，缺失或逆序时钟需核对。单元及真实 Docker 回归同时覆盖延迟观察、实际超限和旧 coordinator 时钟；保留原失败回执，通过 rework 新身份重做，原候选和配方不变。项目经理 Skill 明确区分 execution failed/cancelled 的 retry 与 awaiting_acceptance 失败回执的 rework。
- 一次实际 Codex thread/resume worker 退出后，回合标记结果未知并取得停止证明；核对没有控制调用后通过 Snapshot 后继继续同一候选，没有盲目重放。跨进程数据集锁增加显式释放，重开竞争用例通过。

## 自动化及真实环境验收

- `./scripts/check/check.sh` 最终通过：**192 项 Rust 通过、15 项外部环境用例默认跳过**；Web 构建、HTTP/SSE、模板重置、离线升级和运行准备通过；**debug/release 各 43 项浏览器通过**。最新日志为 `/tmp/devdesk-harness-complete-suite-final.log`。
- 编排专项 28 项通过，配置工作流专项 30 项、Schema 专项 11 项通过。覆盖真实 actor、派发/接纳边界、原生回复前重启、接管撤销委托、运行时修改、冻结来源、停止与迟到回执、Human 精确匹配等。
- 实际 Git/Docker 和 13 个 Permission profile、5 项 Permission 测试通过，包含真实 Codex/DSH 权限边界。Git 豁免专项实际执行成功，测试与确认要求分别记录 waived，没有虚构测试成功或人类签名。
- 真实 Codex 材料流程通过：项目经理原生 workflow.start 后所有节点 queued；运行时预演并绕过可选阶段；业务 Agent 实际调用 materials.capture 得到 `[3,3,2,1]`、总和 9；明确接纳、暂停、接管、Host 重启、Snapshot 后继及完成。项目并发为一，旧合同、输入和产物保持不变；19 次原生回复均只有一个 content item。
- 离线升级夹具通过：v4/13/44、锁拒绝、旧意图归档、跨库 checkpoint、中断重入、未完成 journal 阻止启动、不可变档案/材料/凭据及 SQL 保护恢复，新 Host 明确继续旧实例。
- 真正旧 Host 生成的完整第二代出厂目录通过：自定义流程停在 Human，离线升级后保留配置、项目经理、哨兵材料及凭据，再确认、派发、接纳并完成。转换保留原业务图，采用新版模板另行操作；未升级原用户数据集。
- 最新代码的 macOS arm64 独立包复验通过。检查涵盖源码目录外启动、默认图与 Schema、显式派发接纳、材料捕获、模板历史、升级入口和 global 14/project 45，不自动下载模型或镜像。

本轮真实运行环境为 macOS arm64、Rust 1.99.1、Node 26.12、Docker 29.9.3。此处不代表新的 Ubuntu 验收；模型协议夹具、实际 Codex、Docker、Git/GitHub 和浏览器结果分别记录。

## A01—A18 验收矩阵

| 编号 | 证据与结果 |
|---|---|
| A01 | observations_never_dispatch_or_accept_without_an_explicit_command：无命令就绪节点仍 queued |
| A02 | 真实原生项目经理从零完成材料流程；完整 Feature 21 步、Task 22 步、两次确切候选发布及根实例明确完成通过 |
| A03 | 新增节点并重连依赖；保留已接纳前置及原合同；真实工程采用运行修订 |
| A04 | 运行任务变更等待 writer_revoked；旧 Session/合同/迟到消息隔离 |
| A05 | typed 材料封存、Stage bypass、显式输入重绑与后续消费原值 |
| A06 | 缺失最终输出、规则来源及错误根实例拒绝；不用 skipped 隐藏缺口 |
| A07 | 原拒绝值保留；waiver 与 Human 签名独立，真实拒绝后补验不改写 approve |
| A08 | 实际 Git 正常及豁免路径通过；候选、目标 CAS、测试来源和 permit 失效检查保留 |
| A09 | 指定子调用修订保留兄弟及父端口；集合调用、Task DAG 和工作副本来源回归 |
| A10 | 原生来源、跨根拒绝、epoch 冲突、迟到调用、接管撤销委托；Agent 不可签署 Human |
| A11 | 过期 preview 拒绝、活动计划原子切换、冻结记录不能直接写改 |
| A12 | 原生动作提交后回复前重启不重放；真实 Host/Snapshot 后继；跨库升级中断重入及未知结果核对 |
| A13 | 平台调用取消与原始回执保留；独立暂停子流程不被父级恢复覆盖 |
| A14 | 实际并发一通过；未准入发送合并事件；控制预算和重试冲突回归 |
| A15 | 最小夹具及真正旧程序完整第二代目录升级；保留档案及凭据，无删库/旧意图回退 |
| A16 | 对话与直接编辑、预演、复用、绕过、人工决定、历史及移动刷新；双 profile 各 43 项通过 |
| A17 | 实际数字材料流程，无默认工程阶段名和 TDD 假设 |
| A18 | 原始证据与精确 Human 合同回归；Task 真实 Red/Green、Feature 双文件验证、两级独立候选评审、精确签署、发布和原始证据复验通过；历史 Feature Red 缺口单列 |

## 证据与复现

| 范围 | 临时证据 |
|---|---|
| 最终全套 | `/tmp/devdesk-harness-complete-suite-final.log` |
| 原生材料流程 | `/tmp/devdesk-harness-native-20261111i/`；根 cd2054d9-713a-42b0-82e5-e1d1affc0d48，controller-calls.json、before/after-restart.json、native-reply-shape.json |
| 真正 v4 旧库 | `/tmp/devdesk-harness-real-v4-upgrade-final/report.md`；入口 tests/live/probe_harness_upgrade_legacy.py |
| Git/Docker/权限 | `/tmp/devdesk-harness-live-final/current-native-checks.json`、`permissions.log` |
| 完整诊断 | `/tmp/devdesk-harness-full-diagnostics-docker-verified.log` |
| Docker 时钟 | `/tmp/devdesk-harness-docker-clock-tests.log`、`/tmp/devdesk-harness-docker-clock-live.log`、`/tmp/devdesk-harness-candidate-delay-docker-state.json` |
| 工程事实 | `/tmp/devdesk-harness-engineering-20261111c/`；checkpoint-after-task.json、checkpoint-before-root-complete.json、task-pr-independent.json、feature-pr-independent.json、raw-evidence-validation.json、publication-evidence.json 及两个已完成实例视图 |
| 最后续接及严格复验 | `/tmp/devdesk-harness-engineering-c-clock-recovery.log`、`/tmp/devdesk-harness-final-evidence-recheck.log`、`/tmp/devdesk-harness-engineering-completed-recheck.log`；首次终验读取层级错误保留于续接日志 |
| 独立包 | `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 及 .sha256；`/tmp/devdesk-harness-package-acceptance-pre-final.json`，最终文档归档复验为 `/tmp/devdesk-harness-package-acceptance-final.json` |

完整工程入口为 tests/live/probe_default_engineering_workflow.py，同目录 `--resume` 保留已有成果并核对中断执行；实例已完成时仅复验事实，不接管、派发或再次发布；最终离线核对使用 tests/live/verify_engineering_evidence.py。探针使用认证文件临时副本，退出清理。日志、JSON、数据集及截图留在临时目录，本计划目录仅保存 Markdown。
