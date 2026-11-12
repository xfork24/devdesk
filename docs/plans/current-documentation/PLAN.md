# P7 · 现行系统文档补全与版本治理

日期：2026-11-11（v2 修订：2026-11-12）。状态：**已完成（2026-11-12：W01–W07、W12；W08–W11 按用户决定撤回不实施）**，执行与验收记录见 [EXECUTION.md](EXECUTION.md)；提案原文存于 `review/plans/p7-doc-version-governance/`。来源：`review/docs/06-problems.md` P7。

> 撤回说明（2026-11-12）：版本锚点提取器 / check-docs 校验器 / version-baseline 生成物（W08–W11）不再实施——版本相关内容后续将从代码中移除，文档只梳理总结当前系统状况，不做硬编码版本比对。相关章节仅作历史提案保留。
v2 修订要点：按最新要求，计划重心从"漂移校验"扩展为**补全 `docs/` 根目录的现行系统文档**——先做覆盖缺口分析并新增文档，再修复既有文档漂移，版本锚点校验降为支撑机制。优先级：**最高（第一批立即启动）**。

## 背景与问题

两层问题：

1. **覆盖缺口（主要）**：`docs/` 根目录的 10 个现行文档各自描述一个侧面（架构/领域/协议/源配置/工程接入/适配器契约/运行包/凭据/工具接入/模型），但系统的几个核心子系统**没有任何现行文档**——最突出的是工作流引擎与编排（系统的核心）、权限运行时语义、存储与数据布局、Web 工作台、讨论与经验域。现有信息散落在 `docs/plans/*`（历史合同与执行记录，性质是快照而非现行说明）与代码/Schema 中。
2. **版本漂移（次要但伤信用）**：同一事实在多文档中互相矛盾（见 §三 清单）。根因是 schemas/ 已是机器可读事实源，但文档手工维护且无锚定校验。

## 范围澄清与前置整理（2026-11-12 已完成）

- `docs/plans/*` 全部为历史快照或历史计划，其漂移**不修**（校验器整目录豁免）。
- 顶层历史验收文档已归档至对应计划目录：`docs/acceptance.md` 拆分为 `docs/plans/permission-standardization/acceptance-20261110.md` 与 `docs/plans/devdesk-prototype/acceptance.md`；`docs/ubuntu-web-acceptance.md` 移至 `docs/plans/ubuntu-web-acceptance/ACCEPTANCE.md`；相关引用已同步。
- 受管范围为：`docs/` 根目录现行文档（10 个既有 + 本计划新增）+ 根 `README.md` 文档索引段 + `AGENT.md`。

---

## 一、覆盖缺口分析（现状盘点，2026-11-12 实测）

| 系统层面 | 现有覆盖位置 | 缺口判定 |
|---|---|---|
| **工作流引擎与编排**（四种 executor、三种推进模式、21 控制工具、ChangeSet 预演/静止/原子切换、decision/waiver、TDD 证据链与 ActionPermit、默认流程 22/20 步结构） | real-engineering.md（操作步骤视角，5 步概述）；protocol.md（命令列表视角）；domain.md（概念表视角）；plans/harness-workflow-orchestration/CONTRACT.md（**历史合同**） | **完全缺失系统视角文档**。这是系统最核心的子系统，新读者无法从任一单篇文档理解"一个 Feature 怎么被推进、谁在什么门禁下批准什么" |
| **权限运行时语义**（Profile/Context 分离、step_grants 叠加解析、绑定 CAS、执行冻结双锁、权限切换三段式与后继会话、preflight、预设目录） | permissions/README.md（资产与预设视角，位于 permissions/ 目录）；protocol.md §Permission（命令细节）；design.md §6（两段概述） | **缺失系统语义文档**：授权如何叠加到步骤、切换为何等价于后继会话、默认 full-access 的边界含义，无处可读 |
| **存储与数据布局**（数据集结构、三库分工、source_definitions 唯一存储、governance_records/operations/events、execution/ 目录族、artifacts、模板目录 generation/reset、无迁移策略） | 根 README 一小节；sql/README.md（表清单，位于 sql/ 目录）；source-configuration.md §存储编码 | **缺失整合文档**：排查数据问题、理解幂等/恢复、做备份与迁移决策时无 docs/ 内入口 |
| **Web 工作台**（9 大导航、初始化与连接内核、SSE+定向轮询刷新模型、各页面操作入口、典型操作旅程） | 无（UI 变更史散落在各 plan 的 REPORT.md） | **完全缺失**。真实用户面对的第一界面没有任何现行文档 |
| **讨论与经验域**（自由讨论 vs 工作流会话的边界、交互审批/澄清、Proposal adopt 语义、permission_status 状态机、经验快照与 restore 边界） | protocol.md 命令级；domain.md 概念级 | **缺失操作语义文档**：formal session 禁止自由输入、提案防陈旧、经验只读等关键边界规则不成文 |
| 默认配置目录全景 | resources/configuration/README.md（清单，位于 resources/ 内） | 部分覆盖；docs/ 内缺一篇入口说明（并入新文档 workflow-orchestration 的一节 + 链接清单） |
| 部署/远程/安全 | run-binary.md + protocol.md §监听与内核连接 | 已覆盖（run-binary 仅版本漂移） |
| 测试体系 | tests/README.md（位于 tests/ 内，根 README 已链接） | 已覆盖，新文档中以链接引用 |
| 架构/领域/协议/源配置/适配器契约/模型/凭据/工具接入 | design / domain / protocol / source-configuration / real-adapter-contract / model-providers / system-credentials / tool-skill-integrations | 已覆盖（个别漂移，见 §三） |

## 二、新增文档方案（5 篇，docs/ 根目录）

> 撰写事实源：`review/docs/`（2026-11-11 全量核实过的评审文档 02/03/05）+ 代码与 `schemas/`。写作规范：与既有文档一致的中文短段风格；所有版本断言过锚点校验（§四）；每篇 ≤150 行，链接而非复制 domain/protocol 的内容。

### W01 `docs/workflow-orchestration.md` · 工作流与编排（最优先）

大纲：
1. 定位与阅读路径（概念见 domain.md、命令见 protocol.md、本文讲**运行语义**）
2. 定义：Workflow v6 要素速览（Stage/Step/四种 executor/端口与绑定/预算/condition/confirmation）；发布即编译闭包
3. 推进模式三种：人工逐步 / 项目经理 Agent（Controller）经 21 个控制工具 / 有界段委托；core 为权威终态裁判（无自动遍历）
4. 步骤生命周期与产物接纳链：10 态、执行≠接纳、端口校验、content_checks、WorkflowOutput/artifact
5. 完成规则树与四层批准语义（确认 / step.approve 例外 / decision 人工决定 / waiver 豁免；Independent 身份根）
6. 运行中计划修改：ChangeSet 12 类操作、preview_hash、静止等待、原子切换、受限 revert
7. TDD 证据链与 ActionPermit：action_requirements、tests.run 冻结配方、git.candidate→tests→advance→publish
8. 默认流程概览：Feature 22 步/7 阶段、Task 20 步/7 阶段的阶段级摘要表 + 全量步骤清单链接（resources/configuration/README.md）；步骤授权与 Permission 的叠加关系一句话 + 链接 permissions.md
9. 预算与并发（authoring 默认与默认流程实际值、foreach 并发、全局额度公式）

### W02 `docs/permissions.md` · 运行时权限

大纲：
1. 五层模型总览（Profile → Target → Binding/Assignment → 冻结 Lock → Context step_grants/ControlPolicy）与"Agent/Capability 不含权限"原则
2. Profile v2 职责矩阵（Harness 必含四键/Sandbox 不得含）；可移植词汇与后端编译（permission-compiler/v2、资产 sha256 防漂移，链接 permissions/README.md 后端矩阵）
3. Context：follow_target/pinned、step_grants 按流程步骤精确授权（以默认 engineering context 的 16 条为例：red_baseline/implementation 可写全仓但 tests 只读等）
4. 执行冻结：双端锁、重试/恢复不追随新分配
5. 权限切换 = 后继会话：三段式（静止校验→停止证明→快照+后继）、permission_status 五态、崩溃可恢复
6. 默认绑定说明（harness/sandbox full-access-v2 + 容器边界）与收紧指引（受限 preset 的方向）

### W03 `docs/storage.md` · 存储与数据布局

大纲：
1. 数据集总览（标记 devdesk-source-v2-standard-v1、独占写锁、目录树：global.sqlite3 / projects/<id>/ / conversations/global/ / artifacts/ / execution/{sessions,tests,git-operations,tool-executions} / repository/）
2. 三库分工与隔离（全局配置库 / 每项目工程库 / 全局讨论库；"项目选择把请求钉到一个 Engine"）
3. 配置唯一存储：source_definitions（(identity,revision) 主键、触发器禁改）；无投影表、执行视图按需重建（链接 source-configuration.md）
4. 事实表族速览：events / governance_records / operations（幂等指纹重放）/ sessions / turns / conversation_events / executions / artifacts / repository_* / orchestration_*（链接 sql/README.md 表清单）
5. 幂等与恢复模型：命令三段式、NeedsReconciliation、bootstrap 行为（链接 design.md）
6. 模板目录与默认配置：bundle、generation 代次、reset 语义（改目录选择不改源身份）、归档 vs 停用
7. 升级策略：不做迁移、旧库拒绝、验证用独立空目录；备份建议

### W04 `docs/web-workbench.md` · Web 工作台指南

大纲：
1. 形态与初始化（嵌入式 SPA、/capabilities 探测→初始设置/密钥登录、连接内核）
2. 九大导航区一页表（overview/execution/discussion/projects/workflows/actors/components/history/settings + 各自一句话与入口命令）
3. 刷新模型（SSE 全量 + 会话 1s/编排 2.5s/仓库与准备 1.5s 定向轮询、草稿 localStorage 持久化）
4. 典型旅程：接入 GitHub 项目 → 采用默认配置 → 建 Feature → 执行页推进/与项目经理协作 → 调整运行计划 → 候选接纳与发布（逐步列出所用页面与命令）
5. 设置页导览（内核连接/凭据/权限/模板重置/运行时/并发）与安全边界（同源会话、密钥不落浏览器存储）

### W05 `docs/conversations.md` · 讨论与经验

大纲：
1. 会话四种 scope 与工作类型（Workflow/Discussion/Orchestration）；RuntimeBinding 冻结含义
2. 自由讨论 vs 工作流步骤会话：formal session 禁止自由输入、任务只能来自冻结执行意图（防伪造）；同会话单活动轮次
3. 运行时交互：工具审批、澄清问题（interaction.answer）
4. Proposal v2：extract/create/adopt 的防陈旧（expected_revision）；采纳只建材料不建业务对象
5. 权限改绑检测与后继会话（链接 permissions.md）；permission_status 与恢复入口
6. 经验：自动快照时机、跨项目只读查询、experience.restore 的"上下文而非状态恢复"边界

---

## 三、既有文档漂移修复清单（2026-11-12 逐行实测）

| 文件 | 位置 | 现状（错） | 应为（代码核实） |
|---|---|---|---|
| real-engineering.md | :3 | "使用 Workflow v5 驱动工作" | Workflow **v6** |
| real-engineering.md | :17 | "canonical `agent-task/v5`" | `agent-task/v6` |
| real-engineering.md | :31 | "数据库基线为 global 15 / project 46" | global **16** / project **47** |
| real-engineering.md | :3 | "完整合同见 plans/harness-workflow-orchestration/CONTRACT.md"（把历史合同当现行合同引用） | 改指新增 `docs/workflow-orchestration.md`；CONTRACT.md 降为历史参照 |
| real-adapter-contract.md | :21 | "canonical `agent-task/v5`" | `agent-task/v6` |
| run-binary.md | :31 | "global 15 / project 46" | global 16 / project 47 |
| system-credentials.md | :7 | "默认 `container-full-access` 将整个 Harness 容器视为信任范围…"（v1 旧预设名，已于 2026-11-10 v2 权限改造删除） | 默认为 v2 profile `harness-full-access-v2` / `sandbox-full-access-v2`（容器即信任范围的语义保留，名称与版本更正；链接 permissions.md） |
| tool-skill-integrations.md | :3 | "左侧「工具与技能接入」…基础组件只管理 Model/Harness"（旧导航结构） | 当前导航：**基础组件** tab 含 Model/Harness/MCP/Skills/HTTP 工具/原生工具；调用记录位于**历史记录** |
| 根 README.md | 文档段 | 复核（多数为现行；若有版本断言错误一并修） | 过锚点校验 |
| AGENT.md | — | 复核结构段（当前准确） | 同上 |

核实为**准确无漂移**的现行文档：design.md、domain.md、protocol.md、source-configuration.md、model-providers.md（DSH 0.2.2-rc.1 现行）。

## 四、支撑机制：版本锚点与校验（防再漂移）

1. **锚点提取器** `scripts/py/version-anchors.py`：从代码/schema/manifest 提取锚点（workflow_schema、agent_task_renderer、orchestration_renderer、spec/standard schema、sql_global/project、dataset_marker、bundle_version、default_workflows、默认流程步骤数、codex/dsh 版本等），输出 JSON。
2. **校验器** `scripts/check/check-docs.py`：受管文件（docs/ 根全部 .md + 根 README + AGENT.md + 各 README 索引）中的版本断言与锚点比对；文档引用的脚本路径必须存在；`docs/version-baseline.md`（生成物）必须与锚点一致。`docs/plans/**` 与 `review/crates_review.md` 整体豁免。
3. **挂接**：`scripts/check/check.sh` 在权限资产校验后追加（秒级步骤）。
4. `--normalize-plan` 的 v6 语义修复（写死 schema 5 → 支持读取输入 schema 并仅允许 5→6 单向，或明确拒绝 v6 输入；补单测）。

## 工作项

| 编号 | 工作项 | 涉及 | 验收要点 |
|---|---|---|---|
| W01 | 新增 docs/workflow-orchestration.md（§二大纲 1–9） | docs | 与 review/docs/03 一致；锚点校验通过 |
| W02 | 新增 docs/permissions.md | docs | 五层模型与默认绑定描述准确 |
| W03 | 新增 docs/storage.md | docs | 目录树/表族与 sql、adapters 实测一致 |
| W04 | 新增 docs/web-workbench.md | docs | 九区与命令清单与 web/src 一致 |
| W05 | 新增 docs/conversations.md | docs | formal session 禁自由输入等边界成文 |
| W06 | 漂移修复：§三 清单逐条（含 real-engineering 合同引用改指、system-credentials 预设名、tool-skill 导航） | docs | 人工复核 + 校验通过 |
| W07 | 根 README 文档索引段更新：收录 5 篇新文档、移除指向 plans 历史合同的"现行合同"引用 | README.md | 链接全有效 |
| W08 | version-anchors.py 提取器 | scripts/py | 全部锚点项可提取 |
| W09 | check-docs.py 校验器 + docs/plans 豁免 | scripts/check | 注入错值必 fail；豁免不误报 |
| W10 | docs/version-baseline.md 生成物 + 刷新命令 | scripts | 改代码常量后 diff 恰好一处 |
| W11 | check.sh 挂接 | scripts/check/check.sh | 增时 < 5s、失败码正确 |
| W12 | --normalize-plan v6 语义修复 + AGENT.md 增补文档规范条目 | crates/host/main.rs、AGENT.md | 单测覆盖；规范含 check-docs 要求 |

## 验收标准

- A01 五篇新文档落地：每篇覆盖 §二大纲全部小节，全部内部链接与锚点校验通过。
- A02 §三 清单逐条修复：`grep -rn "v5\|15 / project 46\|container-full-access" docs/*.md`（按语义甄别）无残留错值；历史引用仅存在于 docs/plans（豁免区）。
- A03 新读者走查：仅读 docs/ 根（不看 plans/ 与代码），能回答验收问题集（≥10 问，如"谁能在什么条件下豁免测试要求""权限切换时正在排队的输入怎么办""默认 Feature 流程有几个阶段、最终产物是什么""数据集里 execution/ 目录下有什么"）——由一名未参与开发的同事/评审者执行并记录。
- A04 `./scripts/check/check-docs.py` 通过；故意把 run-binary.md 的 47 改 46 → 报出文件:行并 fail；README 增加死链脚本路径 → 报死链。
- A05 `version-baseline.md` 与提取器输出一致；改 `WORKFLOW_SCHEMA_VERSION` 后刷新 diff 恰好一处。
- A06 全量 `check.sh` 增时 < 5s；`--normalize-plan` 对 v6 输入行为有测试定义。
- A07 文档风格与既有篇目一致（中文短段、表格优先、≤150 行/篇）；不复制 domain/protocol 全文而是链接。

## 风险与兼容

- 新文档与代码的二次漂移风险：靠 A04 门禁 + W12 规范条目约束；新文档避免罗列易变细节（如完整步骤表链接 resources/configuration/README.md 而非复制）。
- 校验器误报必须为零（宁可白名单）；漏报可接受、后续迭代。
- 纯文档与脚本改动 + host CLI 小修（--normalize-plan），无协议/数据影响。
- 实施顺序建议：W08 锚点提取器先行 → W06 漂移修复（立刻消除矛盾）→ W01–W05 新文档（最大工作量，可并行）→ W09–W11 门禁收口 → W12。

## 依赖与顺序

无外部依赖，立即可做。是 P2（术语表）、P3/P6/P9/P10/P11（各计划文档对齐项）的前置基础设施；W01 工作流文档同时为 P1 的提案机制提供现行语义基线。
