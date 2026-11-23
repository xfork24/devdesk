# 出厂模型与工程规范整理计划

用户确认同步整理物理文件名，允许旧 Workflow 的历史配置在完整重置后缺失。本计划记录本轮实施和验收依据。

## 基线与目标

以 `221f0b34` 中的 `simplified-standard-engineering:9` 为功能基线。它固定了 Feature 9 和 Task 4，各有 5 个阶段，分别包含 19 和 14 个节点，其中业务步骤分别为 14 和 10 个。新的出厂配置继续使用现有业务图和质量门禁，采用 GPT-6.2 Sol High，并把工程配置的展示名称和正文整理为一套一致的规范。 [当前默认选择](../../../configurations/defaults.json)

| 定义 | 当前 | 计划 |
|---|---:|---:|
| Model | 5 | 3 |
| Skill | 20 | 14 |
| Capability | 21 | 21，全部基于三种保留模型 |
| Agent | 18 | 18，三种模型各有六个业务角色 |
| Delivery | 21 | 20 |
| Spec | 21 | 14 |

## 1. Model、Capability 与 Agent

- 增加 **GLM 5.3**，使用 `glm-5.3` 和现有 Z.AI Coding 连接。DSH 目录已有精确标识和容量数据。把原 GLM Flash 能力和 Agent 迁到新模型，然后从出厂清单移除 GLM 5.3 Flash。 [DSH 模型目录](../../../configurations/harnesses/builtin-model-catalog-dsh-v1.json)
- 增加 **GPT-6.2 Sol High**，通过 Codex 使用 `gpt-6.2-sol` 和 `reasoning_effort=high`。本机 Codex 目录缓存已列出该标识与档位，实施时用当前 Harness 核对账号可用性，并实际检查原生请求。迁移 `defaults.model_ref`、六个默认角色能力和通用 Codex 能力，移除 GPT-6.1 Sol 的 Low、Medium、High 三条出厂定义。
- **MiniMax M3.1 Flash** 继续使用现有 MiniMax 连接和 DSH。Model 管理只发布这三个预定义 Model。Harness 提供的候选目录供自建配置使用，按现有协议独立维护。
- 保留六个角色及三种模型各自的角色绑定，保留三个可供自建 Agent 选用的通用 Capability。默认六角色均使用 Codex；GLM 和 MiniMax 仍可用于现有 DSH 场景。根工作流编排由当前 Codex 驱动提供。按角色核对技能、Spec 和工具清单，保留各驱动的能力边界、环境及权限绑定。 [配置职责](../../../configurations/README.md) · [驱动能力注册](../../../crates/adapters/src/harness.rs)
- Agent 使用 `Requirements Analyst`、`Software Architect`、`Software Developer`、`Test Engineer`、`Code Reviewer`、`Project Manager`。Agent 和 Capability 名称附对应模型全名以便区分，如 `Code Reviewer · GLM 5.3` 与 `Code Review Capability · GLM 5.3`。Agent 定义表达 Role 与 Capability 的组合，Capability 集中表达模型、工具、技能和 Spec 引用。

## 2. Skill 的合并与重写

| 拟保留名称 | 用途与迁移 |
|---|---|
| `Engineering Context` | 合并两套工程上下文。处理冻结输入、仓库和工作树范围、材料缺失与版本核对。 |
| `Requirements Analysis` | 合并两套需求 Skill，保留澄清、范围界定、验收条目及可追溯证据。 |
| `Software Design` | 合并两套设计 Skill，覆盖接口、数据、状态、故障恢复和方案取舍。 |
| `Task Planning` | 合并两套任务规划 Skill，保留唯一任务标识、依赖理由、并行边界和验收责任。 |
| `Test Design` | 合并两套测试设计 Skill。说明独立行为断言、测试配方，以及有效 Red 与环境故障的判定。 |
| `Test-Driven Development` | 合并两套实现 Skill，保留最小可验证改动、Red 到 Green 的证据及变更记录。 |
| `Code Review` | 对确切 Git 版本独立检查正确性、风险、测试覆盖及问题位置。 |
| `Independent Review` | 仅对冻结需求、设计及计划做独立内容评审，保留其现有评审和定位责任。 |
| `Deliverable Validation` | 保留交付草稿自检、`spec.lint`、冻结证据定位及交付前修正方法。 |
| `Workflow Orchestration` | 保留 Codex 经理的 DAG 派发、接纳、等待、委托、重试和有限返工方法。 |
| `Project Operations` | 保留项目经理工作台的项目、配置、工作流与产物操作说明。 |
| `Software Delivery` | 重写通用能力的开发方法。说明需求到发布的工程实践，约束性步骤以当前任务合同为准。 |
| `Release Verification` | 保留 DSH 测试角色的候选验证方法，区分源代码版本、试合并候选与实际合并版本。 |
| `Workflow Coordination` | 保留 DSH 项目经理对任务进度、依赖、等待项和交付证据的协调方法，工具和编排操作以实际授予的能力为准。 |

同义 Skill 统一采用现行默认流程的合同，再迁移可选 Capability 的引用。合并前逐条核对仍有效的特殊规则和工具要求。旧 `Engineering Constraints` Spec 中仍适用的工作方法归入 `Engineering Context` 或 `Software Delivery`，权限规则继续由 Profile 表达。正文用中文，按适用场景、输入、操作、验证、异常处理编排。职责、任务、校验合同分别由 Role、Step 和 Spec 提供。 [Skill 源目录](../../../configurations/skills)

## 3. Delivery 与 Spec 的实际使用范围

从 Standard v9 的两个根 Workflow 向下遍历子流程、输入和输出端口、步骤 `spec_refs` 以及 `Delivery.spec_refs`。20 个 Delivery 均有实际流程消费者或生产者。14 个 Spec 包括 Delivery 引用的内容规范与步骤直接引用的评审规范。

| Delivery 展示名称 | Spec 展示名称 | 用途 |
|---|---|---|
| `PRD` | `PRD Specification` | 需求文档和可追溯验收条目 |
| `Design Document` | `Design Document Specification` | 设计、接口、数据和恢复方案 |
| `Task Plan` | `Task Plan Specification` | 任务 DAG，供任务创建工具消费 |
| `Test Design` | `Test Design Specification` | 独立行为用例、执行配方、接口声明 |
| `Implementation Report` | `Implementation Report Specification` | Task 实现范围、约束、变更与测试状态 |
| `Review Report` | `Review Report Specification` | 评审决定、问题位置及证据 |
| `Test Report` | `Test Report Specification` | 独立 Red/Green 的实际执行回执 |
| `Diff` | `Diff Specification` | 确切比较端点与差异 Artifact |
| `Workspace Snapshot` | `Workspace Snapshot Specification` | 工作区身份、分支和代码版本 |
| `Task Creation Report` | `Task Creation Report Specification` | 已创建 Task 与计划条目的映射 |
| `Task Record` | `Task Record Specification` | 子流程的 Task 身份及执行范围 |
| `Project Test Configuration`、`Code Checkpoint`、`Regression Test Receipt` | `Platform Evidence Specification` | 冻结测试入口、代码检查点、确切版本的回归结果 |
| `Source Branch Publication Receipt`、`Target Branch Baseline`、`Pull Request Record` | `Platform Evidence Specification` | 源分支发布、目标基线、PR 与试合并候选 |
| `Merge Approval Packet`、`Merge and Sync Receipt`、`Task Merge Receipts` | `Platform Evidence Specification` | 人工决定材料、实际合入和本地同步、子任务合入汇总 |
| Workflow 直接引用 | `Task Code Review Specification`、`Feature Code Review Specification` | 分别检查 Task 差异和 Feature 汇合后的整体差异 |

删除 `Checked Markdown` Delivery，并移除七个快照闭包之外的 Spec。它们是旧版 `Design Doc`、`Task TDD`、`Task Doc`、`Test Doc`、`Engineering Constraints Doc`、`Checked Markdown` 和 `Located Review`。旧版四种业务 Spec 的可选 Capability 引用分别接到表中的 `Design Document`、`Implementation Report`、`Task Plan`、`Test Design` Spec。新版评审仍保留冻结证据的精确位置要求。移除无引用的 Markdown 正文及清单项后，核对所有留下的资源类型和确切修订。 [Feature Development](../../../configurations/workflows/builtin-workflow-feature-development-v1.json) · [Task Development](../../../configurations/workflows/builtin-workflow-task-development-v1.json)

按文档、评审与平台证据三类组织中文正文。文档描述必需内容、验收、异常与未决项。评审描述结论、问题严重程度、定位、修改建议和通过条件。平台证据描述版本、来源、状态和核对方式，分别说明失败、取消、超时、未配置和未知结果。保持当前必填字段、端口、生产者、JSON Schema、`checks` 标识、Markdown 必需标题和引用证据规则，文案与机器校验使用同一组要求。

## 4. 后继修订与模板交付

1. 先记录发布前的源引用图、Workflow 与门禁基线。确认后按 Model、Skill/Spec、Delivery/Capability、Agent、Task、Feature、Standard 的顺序迁移引用。检查 `defaults.json`、子流程、工具的交付映射、活动文档及测试固定引用。历史兼容测试使用独立夹具。
2. 新规范同步整理展示名称、正文标题、源身份和文件名。现有语义准确的身份发布后继修订，原 `simplified-` 的 Skill、Capability、Agent、Delivery、Spec、Standard 和 Workflow 使用新身份。新版默认 `Engineering Standard`、`Feature Development` 和 `Task Development` 均从新身份的 v1 发布。文件名继续遵循 `<identity>-v<revision>.json`。
3. 同步 `manifest.json`、`defaults.json`、正文文件和当前 README，提升 `bundle_version`。运行 `python3 scripts/py/configuration_usage.py --write` 生成引用索引，再构建嵌入该目录的 Host。用户下一次在新构建的 Host 中使用「重置模板定义」即可加载新版资源。 [构建嵌入](../../../crates/adapters/build.rs) · [模板设置入口](../../../web/src/components/TemplateSettings.tsx)
4. 在隔离数据集分别验证「恢复默认配置」与「重置配置」。「恢复默认配置」更新默认选择，保留旧定义、自定义配置与已保存的项目绑定。「重置配置」按出厂清单重建活动模板目录，并按产品现有规则处理历史配置、草稿及失效绑定。三种模型的精确集合以出厂包、空数据集和完整重置后的活动目录验证。真实数据集的重置和项目改绑留待用户后续决定。

## 5. Workflow 验收

| 验收层 | 需要核对的结果 |
|---|---|
| 出厂包和引用 | `check-defaults.sh`、Schema 构建校验、工厂加载、Rust 与 Web 相关回归通过。三种 Model、20 个 Delivery、14 个 Spec 实际装载；清单与正文一一对应，引用索引与当前源一致。 |
| 编译后语义对照 | 用允许的源修订映射归一化前后两套计划。对比全部节点、执行器、依赖 DAG、foreach 并发、输入输出、生产者、权限、预算、完成条件、重试返工和人工门禁。核对需求、设计、规划以及最终主干合入的人工决定点仍生效。 |
| 安装、重置及冻结历史 | 空数据集验证实际默认绑定。旧版隔离数据集验证新版包的预览、两种模板更新路径、重启与按需同步。验证保留路径中的旧 Standard、旧工作与新创建工作分别解析各自的冻结修订；完整重置走现有清理与历史保留合同。 |
| 执行合同 | 用临时工程覆盖至少两个可并发 Task 和一个依赖关系。核对真实 Docker Red/Green、独立评审、配置了项目回归时同版本的源与候选测试回执、Task 向 Feature 合入、Feature 向主干合入与本地同步。加入一次未满足门禁的用例，检查工作流保持等待或拒绝接纳。 |
| 真实默认链路 | 使用 `probe_streamlined_workflow.py` 的当前版本和明确指定的专用测试仓库、Codex 登录及 Docker，核对原生请求确为 `gpt-6.2-sol/high`。核对 PRD、设计、规划、测试、评审、GitHub PR 及合入证据的生产者和提交 SHA，等待真实人工决定。GLM、MiniMax 验证 DSH 连接和角色/能力组合；真实供应商验证按对应凭据可用性单独记录。 |

实施结果按静态检查、隔离回归、真实服务验收分别列出。测试跳过、只有源分支结果或缺少人工决定的运行，记录已完成的步骤和缺少的证据。 [真实资源检查](../../../tests/README.md#真实资源检查) · [默认工作流验收脚本](../../../tests/live/probe_streamlined_workflow.py)
