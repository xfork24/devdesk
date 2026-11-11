# Delivery 分类与内容规范改进计划

日期：2026-11-11。状态：P0—P5 已实施，已合并 main 并完成统一测试与本地验收，见 [实施与验收记录](EXECUTION.md)。

将 Delivery 统一定义为有明确用途、结构、来源和验收标准的工作产出。采用用户指定的五项命名，按实际内容拆分“结构化材料”，将 Diff 接入真实 Git 快照与评审流程。所有 Delivery 必须关联有效的内容 Spec，默认流程中的文档、测试、代码差异、工具回执和汇总产物都纳入同一套交付管理。

## 一 需要解决的问题

命名调整没有方向性问题，但实现需要同时处理以下问题。

1. **改名必须对应实际内容。** Test Doc 当前只有执行配方，需要补充测试范围、用例与验收映射；Task Doc 是规划清单，实际创建的 Task 另有平台 ID 和依赖，二者应有不同的结构合同。
2. **报告需要区分事实来源。** Execution Report 是 Agent 提交的逐次执行记录；Test Report 来自平台测试工具。更改名称不会使前者获得平台回执的可信性。
3. **Task Report 是多个 Task 的汇总。** 保留用户指定的名称，明确其条目来自每个 Task 的交接报告，并关联实际子流程身份和完成状态，不能继续使用无条目合同的任意数组。
4. **PRD 存在类型误用。** 默认流程把 Red 基线说明、实现说明、交付说明和 Feature 验收总结都声明为 PRD，导致需求规范被带入不适用的任务。此次一起纠正。
5. **Diff 已生成但没有成为独立交付。** `workspace.snapshot` 的 `changes.diff` 和 `git.candidate` 的 `diff` 已来自真实 Git 比较，却仍包含在“结构化材料”里。需要独立输出、持久化、传递和展示，且明确比较端点。
6. **关联 Spec 需要成为规则。** 当前只有 PRD、Design Doc 在 Delivery 上关联了 Spec。仅补齐默认 JSON，仍可通过 API 创建无规范的 Delivery；创建、导入、发布和执行解析应使用一致校验。

当前内置目录有 10 种 Delivery。两个默认流程共 43 个步骤输出端口，其中 25 个使用“结构化材料”；Diff 和 Test Report 均未被声明为步骤输出。数据依据见 [Delivery 发布清单](../../../resources/configuration/manifest.json)、[Feature 流程](../../../resources/configuration/workflows/builtin-workflow-feature-sol-high-v5.json)、[Task 流程](../../../resources/configuration/workflows/builtin-workflow-task-sol-high-v5.json)。

## 二 命名和交付目录

### 用户指定的命名

| 当前名称 | 新名称 | 内容与规范 |
|---|---|---|
| 内容判断 | Review Report | 评审对象及其版本、规范依据、逐项检查、证据、问题和最终结论；新建评审报告规范 |
| 任务集合 | Task Doc | 任务清单、稳定 key、目标与边界、验收责任、依赖、测试责任和汇合安排；修订并复用现有 Task Plan 拆分规范 |
| 测试命令配方 | Test Doc | 测试范围、用例及预期、验收映射、测试文件和可执行配方；修订并复用现有测试设计规范 |
| 原始执行证据 | Execution Report | 每个增量的 Red、Green、回归事实，完整命令、环境、终止状态、输出和文件快照；新建执行报告规范 |
| 集合产物 | Task Report | 按任务标识汇总每个已完成 Task 子流程的交接报告、证据和结果；新建任务汇总报告规范 |

名称变更沿用相同用途的源 identity，发布新修订；不通过改写既有不可变修订实现。Task Doc、Test Doc 等可以继续使用结构化 JSON，Doc 名称不要求改为 Markdown。

### 保留并完善的 Delivery

| Delivery | 用途 | 内容 Spec |
|---|---|---|
| PRD | Feature 需求与验收定义 | 复用现有 PRD 规范，清除非需求文档的引用 |
| Design Doc | 架构、接口、状态与异常行为设计 | 复用现有设计规范 |
| Test Report | 平台真实测试执行报告 | 新建测试报告规范，纠正现有正文混入测试计划要求的问题 |
| Diff | 两个确切 Git 版本间的代码差异 | 新建代码差异规范，替换现有泛化的实现交付说明 |

### 拆分结构化材料

按内容含义和来源复用类型，不按每个 Step 单独建立类型。

| 新 Delivery | 实际内容 | Spec 的主要检查内容 |
|---|---|---|
| Workspace Snapshot | 读取或封存后的工作区、归属与确切 Git 版本 | 工作区和项目归属、版本真实来源、快照操作关联、禁止把路径或分支名当确切快照 |
| Engineering Constraints Doc | 当前流程声明的运行时、runner、依赖和证据字段要求 | 支持能力、约束依据、适用范围；配置声明与环境实测分开 |
| Task Creation Report | 平台创建的真实 Task 清单及规划 key 到 ID 的映射 | 规划与真实 Task 一一对应、Feature 归属、依赖转换、创建操作来源 |
| Task Record | 单个真实 Task 的身份、目标、说明和依赖 | 与创建回执一致、ID 和 key 可追溯、依赖属于当前 Feature；用于子流程的 item 输入 |
| Code Candidate | 目标版本、源快照、候选版本及实际父节点 | 候选操作来源、三类版本关系、归属和对应 Diff |
| Acceptance Report | 本地目标从旧版本推进到确切候选的事实 | 旧目标、新版本、真实推进操作及对应候选 |
| Publication Report | 发布与远端 PR 的实际结果 | 仓库、分支、PR、远端候选和合并事实对应；区分本地接纳与远端发布 |
| Task Delivery Report | 单个 Task 交接的代码、说明、测试、执行、评审和发布材料 | 材料完整性、确切输入版本、来源和关联；供 Task Report 汇总 |
| Task Completion Report | 平台登记 Task 完成的结果 | Task 身份、完成操作、依赖状态和真实登记结果 |

上述每种类型都有自己的内容 Spec。Task Record 的字段定义与 Task Creation Report 的列表条目保持一致，输入记录和整个创建报告不混用。

### 纠正 PRD 的复用

| 新 Delivery | 替换位置 | 内容 Spec |
|---|---|---|
| Baseline Doc | Task 的 red_baseline 输出 | Red 基线准备说明，包含稳定接口、预期失败及允许修改范围；不把准备说明当真实 Red 结果 |
| Implementation Doc | Task 的 implementation、documentation 输出 | 同一 Task 实现与交付说明的不同修订，包含行为、接口、使用方式、限制和证据引用；依步骤阶段检查应交付内容 |
| Feature Report | Feature 的 closure 输出 | Feature 验收与交付总结，关联需求、Task Report、整体验证和候选；修订并复用完整验收规范 |

以上形成目标目录 21 种 Delivery：保留并改名的 9 种、拆分所得的 9 种、纠正 PRD 复用的 3 种。当前默认发布目录移除“结构化材料”，不再提供任意对象或任意数组作为标准产出。

## 三 默认流程端口映射

下表覆盖当前使用“结构化材料”的全部 25 个输出端口。

| Delivery | Feature 步骤 | Task 步骤 | 输出数量 |
|---|---|---|---:|
| Workspace Snapshot | source_workspace、workspace、tests_snapshot、mainline | workspace、tests_snapshot、red_snapshot、implementation_snapshot、target | 9 |
| Engineering Constraints Doc | engineering_constraints | — | 1 |
| Task Creation Report | tasks | — | 1 |
| Test Report | verification、candidate_tests | red、green、verify、candidate_tests | 6 |
| Code Candidate | candidate | candidate | 2 |
| Acceptance Report | advance | advance | 2 |
| Publication Report | publish | publish | 2 |
| Task Delivery Report | — | handoff | 1 |
| Task Completion Report | — | complete | 1 |

Task 子流程的 `item` 输入改用 Task Record。Feature 的 `implementation.result` 使用 Task Report，条目合同为 Task Delivery Report，并核对实际子流程已经完成及任务集合没有遗漏、重复或串用。数组顺序来自冻结的任务展开顺序。

所有来源端口、JSON Pointer、工具参数映射、子流程接口、最终输出与完成规则一并更新。Test Doc 改为包含 `recipe` 的结构化文档后，`tests.run` 显式绑定 `/recipe`；固定测试镜像仍由平台冻结环境提供。

Task Doc 的任务规划内容与 Task Creation Report 分开保存。Task Doc 定义任务计划；Task Creation Report 保存真实创建事实；后续逐项展开使用 Task Record。计划内容不得被工具回执覆盖。

Feature Report 需要引用最终候选及其测试结果，因此同步调整 Feature 收尾顺序为 `verification → mainline → candidate → candidate_tests → closure → closure_review → candidate_review → advance → publish`。解除当前 `mainline` 对 `closure` 的依赖，将 `closure` 移到候选测试之后并显式绑定候选、候选 Diff 和测试报告。这样报告编写时所需事实均已存在，也不会引入循环依赖。远端发布结果仍由后续 Publication Report 记录，Feature Report 不提前宣称发布完成。

## 四 每种 Delivery 的内容标准

### Spec 的必需内容

每个 Delivery 至少关联一个有效、可见、固定修订的 Spec。Spec 包含非空正文和具体检查项，至少覆盖：

- 目标、适用阶段和范围边界。
- 必填内容及各字段、章节或条目的含义。
- 输入来源、产物版本、执行身份及证据要求。
- 与其他产物、代码版本和真实回执的一致性。
- 允许的失败、未知、未执行及不适用情况。
- 完成条件、拒绝条件和适用的检查方式。

JSON Schema 检查结构，Spec 定义结构背后的内容要求。现有通用的“按输入输出声明提交”文字不能作为类型专属规范。内容正文只在 Spec 保存；Delivery 保存用途说明、格式、结构和规范引用，避免重复维护两份验收要求。

工具产出也关联 Spec。字段、摘要、作用域、版本和回执来源等可核验条件由程序执行；内容覆盖和结论是否充分由配置的评审步骤判断。关联 Spec 不意味着每个工具步骤都增加一次模型评审。

### Review Report 的检查边界

Review Report 自身的 Spec 规定如何形成有效的评审报告。报告中 `checks` 的业务检查集合仍取评审 Step 明确声明的目标 Spec，并使用稳定且唯一的检查 key。报告格式要求与被评审内容要求分别解析，避免把报告自身的检查项递归加入自己的 `checks`。

生产者、消费者及评审者都接收关联 Delivery 的确切 Spec。通过 JSON Pointer 只取出某个字段时，保留原产物类型、版本和指针来源，按所消费字段理解规范，不要求一个配方字段独自满足整份 Test Doc。

### Test Doc 和 Test Report

Test Doc 描述将如何验证，至少包含范围、用例、预期、验收映射、测试文件及 `recipe`。测试工具只消费受支持的执行参数，不把整份文档当命令，也不接受其中未经授权的环境或依赖要求。

Test Report 从平台实际测试操作及冻结请求生成，包含被测版本、执行配方、环境、结果和失败分类，并能追溯到真实操作与保存的日志。现有 `TestResult` 仅提供结果摘要，需要补齐报告所需的请求、日志及时间来源；无法采集的事实如实表示，不能由模型补写。

Red 报告可以记录符合预期的业务断言失败。语法、依赖、环境故障、超时、取消和未知结果分别处理；平台调用成功、测试执行完成、断言通过和满足当前阶段完成条件分别判定。

Execution Report 继续保存每个增量的原始执行事实和文件快照。其作者来源必须保留，评审时与 Test Report、Diff 和确切候选交叉核对。

## 五 Diff 的生成与使用

### 比较范围

| Diff 范围 | 比较端点 | 目的 |
|---|---|---|
| 测试变更 | 测试编写前快照 → tests_snapshot | 核对新增独立测试与允许写入的路径 |
| Red 基线准备 | tests_snapshot → red_snapshot | 核对接口准备或占位实现没有修改独立测试及提前完成业务实现 |
| 实现增量 | red_snapshot → implementation_snapshot | 核对从业务失败到通过的实现改动和测试保护 |
| Task 整体修改 | Task 初始 workspace → implementation_snapshot | 覆盖测试、基线准备和实现的完整修改，不只展示最后一次快照差异 |
| 候选修改 | target_revision → candidate.revision | 核对真正待接纳的候选范围；Task 与 Feature 分别使用各自的冻结目标 |

`source_revision` 与 `candidate.revision` 可以不同，测试报告和 Diff 必须各自绑定实际对应的版本。评审同时看到 Task 整体修改及候选修改，并能定位各阶段增量。

### 生成和持久化

复用 Git 适配器，Diff 只由平台比较确切快照生成。工作区先按现有权限封存，再比较固定 Git 对象，模型不负责生成或改写权威 patch。

`workspace.snapshot` 保留相邻快照的比较，并增加可选的显式范围基线输入，供 implementation_snapshot 同时生成 Task 整体修改。范围基线必须来自当前执行范围内的真实快照，不接受任意 SHA 作为可信来源。`git.candidate` 继续生成冻结目标到候选的完整比较。

通过现有 `output_mapping` 将结果映射为独立的 `diff`、`scope_diff` 等命名端口，统一使用 Diff Delivery；工作区和候选信息保留各自的结构化端口。Diff 正文保持原生 patch，产物来源元数据明确保存比较类型、base/head、对应工作区或候选、平台操作和内容摘要，并由平台核验其与原始回执一致。

处理新增、删除、重命名、权限位、符号链接、二进制和空差异。新文件在封存后进入比较；二进制内容或其可核验的完整产物引用必须保留，不能用一行提示宣称已完整交付。空 Diff 表达真实无变化，是否满足当前修改任务由任务范围决定。

完整 patch 保存为 Artifact。大差异按授权引用与分段读取提供给评审，任务输入保持现有载荷限制；摘录必须标记范围，缺少完整读取能力或证据时阻塞相关完成判断，不静默截断成“完整 Diff”。

### 流程消费

独立评审显式绑定 Diff 端口及其来源。Task Delivery Report 引用 Task 整体 Diff、阶段增量、候选 Diff 和对应测试报告；Feature Report 引用最终候选 Diff。Task Report 保留每个 Task 的上述引用。记录和查看均从 Artifact 及冻结来源恢复，重启后不从活动工作区重新计算历史 Diff。

## 六 校验和界面

### 配置与执行校验

创建、修订、导入、模板加载和 Workflow 发布统一校验：Delivery 的 `spec_refs` 非空且无重复，引用类型、确切修订和作用域正确，Spec 正文及检查项有效，结构 Schema 可离线校验。

执行编译沿 Delivery 引用装载 Spec；任务预览、实际派发、子流程、重试和恢复使用相同冻结依据。更新 Spec 需要显式采用新 Delivery 或 Workflow 修订，不修改已有产物的规范归属。

平台报告的来源根据真实调用身份、保存的请求和回执验证，不能仅依赖 JSON 中自述的 `origin`、`status` 或“通过”文字。类型专属结构通过配置表达，避免按英文显示名称写死执行分支；来源元数据由引擎维护，不能由普通产出或导入材料伪造。

### Delivery 和产物界面

Delivery 页面展示用途、格式、关联 Spec 及版本，并可直接查看规范。保存时具体提示缺失或无效的规范。Workflow 输入输出编辑器和预览显示每个端口的类型及适用内容标准。

产物视图区分规划、执行和结果：Test Doc 显示用例与配方，Test Report 显示实际执行，Execution Report 显示逐次记录；Task Report 可按 Task 查看交接结果；Diff 展示比较范围、base/head、变更文件和完整内容入口。

## 七 源配置和数据边界

对已发布的 Delivery、Spec、Workflow、Tool 及必要的 Capability、Permission Context 发布新修订，统一更新 manifest 和默认引用。用途未改变的五项改名沿用 identity；新增类型有独立 identity。“结构化材料”退出当前默认发布套件，其全部默认端口改用具体类型。

遵循当前工程 [源配置与执行解析](../../source-configuration.md) 的新数据集约定，实施和验证使用独立临时空目录，不修改用户现有数据目录，也不使用删除或重置数据作为应用变更的步骤。此次不提供旧数据升级器；若需要让现有数据集采用新合同，应另行明确升级范围。历史实现和报告的结论继续按其原版本理解。

## 八 实施阶段

| 阶段 | 主要工作 | 完成标准 |
|---|---|---|
| P0 固定合同 | 确定 21 种 Delivery、对应 Spec、字段、来源、Diff 比较端点及全端口映射；记录必要的合同版本变化 | 每个现有输出与子流程输入都有目标类型，无任意对象或数组兜底；规范不要求当前阶段尚未产生的事实 |
| P1 规范与源配置 | 完成五项命名、专属 Spec、结构 Schema、新源修订和发布清单；纠正 PRD 类型误用 | 所有当前发布 Delivery 均有关联 Spec 和有效检查项，引用闭包无悬空 |
| P2 统一校验和报告 | 强制 Spec 关联；完成 Task Doc、Test Doc、各类平台报告、Task 汇总及来源核验 | 无规范 Delivery 在各写入入口均被拒绝；真实回执能映射并通过对应结构与来源检查 |
| P3 Diff 交付 | 补齐快照范围比较、独立输出、来源元数据、Artifact、大差异读取及评审传递 | 每个代码修改阶段均有正确范围的真实 Diff，最终候选有独立 Diff，不能用模型文本替代 |
| P4 默认流程与界面 | 更新所有端口、指针、工具参数、子流程、完成规则和产物展示；同步必要的能力与授权引用 | Feature 和 Task 完整流程消费新合同，界面可查看内容规范与确切版本 |
| P5 验证与文档 | 定向回归、完整检查、真实 Git 和本地执行闭环；更新长期文档与实施报告 | 下列验收项全部满足，报告区分自动化、真实执行及尚未覆盖的平台范围 |

## 九 验收标准

1. 当前发布的全部 Delivery 均关联有效 Spec，空规范、错误类型、不可见或悬空引用、重复引用在创建、导入和发布路径均有明确错误。
2. 五项名称与用户指定完全一致；默认流程不再引用“结构化材料”；PRD 仅用于需求产出。
3. 原 25 个通用对象输出端口逐一替换，Task Record 输入与真实创建条目一致；每种 JSON 产出均有具体字段和条目合同。
4. Task Doc、Test Doc 的内容要求进入生产任务和评审；Test Doc 的 recipe 可以直接由测试工具消费，环境仍来自冻结运行配置。
5. 平台 Test Report 覆盖实际 Red、Green、回归和候选测试，能核对代码版本、配方、环境和日志；假回执、失效版本、跨 Task 来源、环境故障冒充 Red 均被拒绝。
6. Review Report 使用正确目标 Spec 的检查集合；关联报告自身的 Spec 不改变或递归扩展其业务 checks。生产、消费和评审预览与实际派发使用同一冻结规范。
7. 在临时真实 Git 仓库核对阶段 Diff、Task 整体 Diff 和候选 Diff；覆盖新增、删除、重命名、二进制、模式变化、空差异、错误端点及测试目录保护，完整内容与固定对象比较一致。
8. Diff 经过真实命名端口传递给评审和交接，能够从持久化记录恢复；超出输入限制时使用完整 Artifact 读取，不将截断内容视为完整证据。
9. Task Report 的成员与已创建和完成的 Task 一一对应；拒绝遗漏、重复、跨 Feature 及仅凭自述完成的条目。
10. 浏览器覆盖 Delivery 规范查看、编辑保存、流程预览及报告和 Diff 查看；刷新和重启后仍显示原类型、原 Spec 修订及原始产物。
11. 执行适用的 Rust 领域、配置、Schema、真实 Git 和 Workflow 集成回归，构建 Web 并运行相关浏览器测试；定向检查通过后运行仓库要求的完整检查。以临时新数据集完成包含任务创建、真实测试、Diff、独立评审及交接汇总的流程验收，不因本计划擅自发布到远端。

实施结果记录在同目录 `EXECUTION.md`，区分类型和规范是否配置完整、真实数据是否正确产生、内容是否通过评审及最终工程是否完成，分别提供依据。
