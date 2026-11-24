# 默认 Feature / Task 开发流程精简计划

状态：待实施。本次交付为方案与实施计划，尚未修改默认 Workflow、执行内核、Git/GitHub 行为或运行中的实例。日期：2026-11-18。源码核对基线：`7ef26e6`。

## 1. 目标与范围

新建「精简版Feature开发流程」「精简版Task开发流程」和「精简版工程规范快照」，并将三者设为实际的出厂默认配置。原流程继续作为「详细版Feature开发流程」「详细版Task开发流程」供选择，原 Standard 后继修订命名为「详细版工程规范快照」。两个 Standard 各自引用同套 Feature 和 Task，详细版保持原有执行行为。发布和采用规则见第 12 节。

精简版由规划 Agent 产出可独立交付的 Task 列表和结构化依赖 DAG，项目经理根据内核返回的就绪状态并发派发。Task 通过测试和评审后以 PR 自动合入 Feature。全部 Task 完成后，Feature 统一验证，再经人工审核合入 main。

减少独立 Agent 交接、重复材料评审、同版本同配方的无条件复验和手工证据整理。代码基线、版本固定、原始执行记录、依赖检查和状态登记由平台自动完成。不能只隐藏旧节点，保留原有逐节点唤醒与材料搬运成本。

覆盖两套出厂配置、项目测试配置、测试执行、PR 创建与合入、DAG 规划和调度、完成条件、人工审核及必要界面。改进已有框架中缺少的执行能力，复用 Role / Skill / Step / Spec / Delivery / Standard、权限 Profile、持久化意图与回执体系。

PRD、Design、Tasks 的独立评审及人工确认继续保留。复用已合入的有限返工、经理研判与人工例外能力，将其映射到精简后的实际节点，不因删除候选评审而清空共享额度或丢失原始拒绝。本轮不合并开工前确认点，不执行旧实例升级，不修改其他历史计划。

## 2. 基本模型与约束

| 对象 | 工作空间与分支 | 交付目标 | 合入政策 |
|---|---|---|---|
| 项目 | main worktree 与项目配置的主分支 | 保持已验收的项目版本 | Feature PR 经过人工审核后才能合入 |
| Feature | 独立 Feature worktree 与分支 | 完成本次 Feature 的整体需求 | 接纳所属 Task PR；完成后向 main 提交 PR |
| Task | 独立 Task worktree 与分支 | 完成本项 Task 的实现、测试及必要文档 | 验收及合并回归通过后自动合入所属 Feature |
| 临时合并验证 | 平台管理的临时 worktree 或等效隔离副本 | 验证最新目标版本与 PR 的合并结果 | 仅用于验证，不增加业务工作层级 |

1. 修改在对应 scope 的分支完成，通过 PR 向上一级交付。分支归属来自实际工作关系和冻结政策，内核不能根据标题、业务 key 或分支名称猜测权限。
2. 正式 Feature/main 分支只在合入门禁满足后推进。本地合并回归在临时验证空间执行，不预先推进正式目标分支再等待审批或回滚。
3. Task completed 要求 PR 已合入、合入结果已核对、受管理的 Feature 分支与 worktree 已同步。实现结束、测试通过或 PR 已创建均只是中间状态。
4. Feature 交付要求全部当前有效 Task 子流程完成、其 PR 已合入并同步 Feature，且没有仍可写入本次交付范围的执行者。
5. 测试、评审、PR、人工决定和合入回执绑定确切版本。源版本或目标版本变化时，重新验证受影响的结果。
6. 外部副作用继续遵循“持久化意图 → 事务外执行 → 核对接纳回执”；未知效果先核对，不盲目重推、重建 PR 或再次合入。
7. 精简版启用 Task 自动合入，Feature → main 须真实人工审核。详细版继续通过既有平台动作创建 PR 并等待外部合入。两套行为由冻结合同和授权策略确定。
8. 业务评审的人工例外与正式合入审核分别记录。例外仅覆盖其确切材料和缺口，不能伪造测试成功、绕过平台硬规则或自动替代 main 合入审核。

## 3. 已核实的实现基线

| 事实 | 实现依据 | 与目标的差距 |
|---|---|---|
| 默认 Feature 为 8 阶段 22 步，Task 为 8 阶段 20 步 | [Feature 定义](../../../configurations/workflows/builtin-workflow-feature-sol-high-v10.json)、[Task 定义](../../../configurations/workflows/builtin-workflow-task-sol-high-v9.json) | 存在多轮材料整理、重复评审及固定版本复验 |
| PRD、Design、Tasks 分别等待人工确认，最终候选评审为 automatic | 同上 | 需要 main 合入前的明确人工审核门禁 |
| Task foreach 默认并发为 1，Workflow scheduling.max_concurrency 为 4 | [Feature 定义](../../../configurations/workflows/builtin-workflow-feature-sol-high-v10.json) | 根调度额度与 Task 并发额度需要分别处理 |
| 派发检查 Task 依赖；worktree 等依赖 completed 后从当时 Feature HEAD 准备 | [派发资格](../../../crates/core/src/workflow_dispatch.rs)、[空间准备](../../../crates/core/src/repository_runtime.rs) | 保留既有基础，完成语义对齐真实 PR 合入及本地同步 |
| tasks.create 创建真实 Task、上下文、依赖 ID 和配置绑定 | [平台动作](../../../crates/core/src/platform_facts.rs) | 操作必须保留，可并入计划接纳后的自动登记 |
| 现有 Task Plan 已有 `key` 与 `dependencies`，`tasks.create` 会校验 DAG，`foreach` 展开子实例，平台按依赖和并发资格接受步骤派发 | [Task Plan 合同](../../../configurations/deliveries/builtin-delivery-task-plan-v3.json)、[平台动作](../../../crates/core/src/platform_facts.rs)、[子实例展开](../../../crates/core/src/workflow_children.rs)、[派发资格](../../../crates/core/src/workflow_dispatch.rs) | 保留已有图与身份模型，让规划结果明确每条真实依赖及并行边界，补齐合入完成事实和经理可消费的就绪、容量与事件信息 |
| 工程约束写死 JavaScript、node:test 和 Node 内置依赖；TestRunner 只有 NodeTest | [Feature 定义](../../../configurations/workflows/builtin-workflow-feature-sol-high-v10.json)、[测试类型](../../../crates/domain/src/testing.rs) | 需要项目级工程执行配置及实际回归入口 |
| Task Green、verify 和 candidate_tests 消费同一份 Task 配方，documentation 为只读 | [Task 定义](../../../configurations/workflows/builtin-workflow-task-sol-high-v9.json)、[文档步骤权限](../../../configurations/permissions/profiles/builtin-permission-step-task-sol-high-documentation-v1.json) | verify 存在重复；Task 配方也不等于项目回归 |
| git.candidate 生成目标单父候选，git.advance 提前推进本地目标，git.publish 随后创建 PR 并等待外部合入 | [Git 适配器](../../../crates/adapters/src/git.rs)、[平台发布](../../../crates/core/src/platform_publish.rs) | 需要调整顺序及新增合入端口，不能只改 Workflow JSON |
| 当前发布接纳要求远端 merge_commit 等于原候选，规则诊断对分支保护保守阻塞 | [平台发布](../../../crates/core/src/platform_publish.rs)、[GitHub 适配器](../../../crates/adapters/src/github.rs) | 需要确定受支持合入策略、内容一致性和保护规则能力 |
| 有限返工、共享额度和人工例外已合入本地 main；默认 Feature 10、Task 9、Standard 8 | [实施记录](../review-rejection-and-approval/EXECUTION.md)、[合入验证](../review-rejection-and-approval/MERGE.md) | 保留三种返工和真实决定，将策略入口、作者和预算映射到新流程 |
| 出厂默认值含 `workflow_ref`、`task_workflow_ref` 和 `standard_ref`。已有数据集从活动模板目录读取默认值，项目设置可保存独立选择 | [出厂默认](../../../configurations/defaults.json)、[默认值读取](../../../crates/core/src/builtin_defaults.rs)、[模板目录](../../../crates/core/src/templates.rs)、[项目设置](../../../web/src/components/ProjectManager.tsx) | 同时发布两套资源并切换实际默认值。已有目录需有保留项目及自定义配置的启用路径 |

以上为编写计划时的源码事实；下述目标能力均待实施和验收。

## 4. 目标 Feature 流程

### 4.1 五个阶段、十四个业务步骤

自动准备、登记与归档属于平台动作，不另启动材料编写 Agent。最终编译节点数和内部工具动作数须分别报告，不能用业务步骤数代替实际成本。

| 阶段 | 步骤 | 行为与完成条件 |
|---|---|---|
| 需求 | F01 澄清需求与 PRD | 明确目标、范围及可检查的验收标准，必要信息缺失时向用户提问 |
| 需求 | F02 PRD 评审与人工确认 | 独立评审后，由人类确认本轮确切产物 |
| 设计 | F03 设计架构与接口 | 自动注入代码基线和项目执行配置，设计接口、数据模型、依赖及失败处理 |
| 设计 | F04 Design 评审与人工确认 | 独立评审设计并取得人类确认 |
| 任务规划 | F05 划分 Tasks 与依赖 DAG | 规划 Agent 产出机器可读 DAG，为每项 Task 明确实现范围、前置产物和验收责任。要求见第 6.1 节 |
| 任务规划 | F06 Tasks 评审与人工确认 | 校验 DAG，独立评审范围、验收覆盖及每条依赖，再由人类确认。接纳后平台幂等登记真实 Tasks |
| 实施 | F07 按 DAG 执行 Task 子流程 | 每个 Task 对应独立的「精简版Task开发流程」子实例，经理在并发额度内派发所有就绪工作。完成条件见第 6 节 |
| Feature 交付 | F08 Feature Code Review | 检查整个 Feature 差异，重点核对跨 Task 接口、组合行为、范围及需求覆盖 |
| Feature 交付 | F09 Feature 分支项目回归 | 在固定 Feature 版本执行项目配置的回归入口 |
| Feature 交付 | F10 提交并推送 Feature 分支 | 提交必要变更并推送已验收源版本；已有提交直接推送，不制造空提交 |
| Feature 交付 | F11 创建 PR：Feature → main | 提供变更、实际测试结果、使用说明及已知限制 |
| Feature 交付 | F12 本地合并回归 | 取得最新 main，在临时验证空间构造合并结果并执行项目回归 |
| Feature 交付 | F13 人工审核 | 展示 PR、确切源/目标/合并版本、Code Review 和回归结果，由人类签署 |
| Feature 交付 | F14 合入 main | 核对门禁及版本后合入，验证远端结果，同步本地 main，再完成 Feature |

F08–F14 全部位于 F07 完成之后。整体评审不默认再次逐条人工审计所有 Task 的完整 TDD 日志；平台核对来源与版本，评审者按问题定位原件。

F13 复用应用内真实 Human 决定。人类批准后可由平台执行该次已批准的合入；人类也可在外部 PR 系统完成合入，平台核对实际回执。外部合入观察本身不能伪造或替代本流程要求的人工决定。

### 4.2 原步骤的处理

| 原步骤或阶段 | 处理 |
|---|---|
| source_workspace：读取设计基线 | 派发设计时自动提供实际 Feature 工作空间和基线提交 |
| engineering_constraints：固定工程执行约束 | 改为项目执行配置，随本次执行冻结并注入相关步骤 |
| tasks：创建任务集合 | 并入 F06 接纳后的幂等登记，保留 Task、上下文、依赖及配置绑定 |
| verification.workspace / tests_snapshot | 自动准备工作空间和固定版本 |
| release_recipe：编写整体回归测试 | 取消固定收尾编写；Feature 验收用例在任务规划时分配给 Tasks |
| verification：执行测试 | 替换为 F09 项目回归，消费项目回归配置 |
| mainline / candidate / candidate_tests | 收敛为 F12 的目标读取、试合并及合并结果回归 |
| closure / closure_review | 取消独立交付报告及报告评审；说明进入 PR，代码质量归 F08 |
| candidate_review | 合并到 F08 独立代码评审及平台机器门禁；F13 负责正式人工审核 |
| advance / publish | 先发布源分支和 PR，验证及审核后正式合入，再同步本地目标 |

跨 Task 验收不能随 release_recipe 一起丢失。规划时将相关用例分配到具备对应依赖的 Task；确有独立集成范围时才创建集成 Task，不默认给每个 Feature 增加额外任务。

## 5. 目标 Task 流程

### 5.1 五个阶段、十个业务步骤

| 阶段 | 步骤 | 行为与完成条件 |
|---|---|---|
| 独立测试 | T01 编写独立测试 | 独立测试者依据 Task、设计及验收标准编写行为用例，登记入口及测试版本 |
| 独立测试 | T02 执行 Red | 新增行为断言按预期失败；依赖、语法、加载和环境错误不能当作有效 Red |
| 实施 | T03 实现 Task | 实现功能、开发者自有测试及必要文档，保留独立测试所有权和既有行为 |
| 实施 | T04 执行 Green | 固定实现版本后，独立行为测试全部通过 |
| 验收 | T05 Code Review | 独立评审差异、范围、接口、用例有效性及相关测试事实 |
| 验收 | T06 项目回归 | 执行项目配置的回归入口，覆盖已有测试和本次纳入回归的新测试 |
| 提交 | T07 提交并推送 Task 分支 | 提交并推送已验收版本，不包含其他工作范围的修改 |
| 提交 | T08 创建 PR：Task → Feature | base 为实际所属 Feature，包含简明说明及真实测试结果 |
| 合入 | T09 本地合并回归 | 基于最新 Feature 和确切 PR 源版本构造合并结果，执行项目回归 |
| 合入 | T10 自动合入 Feature | 核对归属、门禁和版本后合入；核对远端结果、同步 Feature、归档并登记完成 |

### 5.2 合并与保留的行为

- T01 测试者、T03 实现者、T05 评审者保留现有独立执行身份规则。实现者不能改写独立测试使结果通过；测试有误时返还责任方修改，重做受影响验证。
- Red 前确需补充可加载模块或最小接口时，由受限条件动作准备，不强制每个 Task 启动单独的 red_baseline Agent；不得实现新增业务或破坏现有行为。用例已通过时核对需求和用例，不人为制造失败。
- 内部检查点可以在测试或评审前自动创建以固定对象；T07 表示正式提交整理及发布。评审后任何代码修改都使受影响评审和测试失效，不能只在 T07 补提交未验收修改。
- 取消 documentation、verify、candidate_review 独立交接。原 workspace、各 snapshot、target、handoff、complete 的必要行为并入自动准备、合并验证与 T10 完成登记。
- 保留开发所需测试迭代和明确约定的增量验收；原始命令、结果及对应内容版本由平台记录引用，不要求各 Agent 再生成一套日志与源码全文报告。

## 6. DAG 规划、并发与完成语义

### 6.1 规划 Agent 产出可执行的任务图

为精简版 F05 配置实际绑定的规划 Agent、规划 Skill、Step 指令和交付合同。输入包括已确认的 PRD、设计、代码基线和项目执行约束。按可独立实现、测试、评审及合入的成果拆分，保留小功能的单 Task 路径。每项任务的结构化交付至少包含以下内容，字段类型和边明细的编码在 P0 确定。

| 内容 | 规划要求 |
|---|---|
| 任务清单 | 稳定且唯一的 `key`、`title` 和 `instructions`。描述实现范围、接口或共享文件、交付物和完成条件 |
| 直接前置依赖 | `dependencies` 使用同一计划的任务 key。每条边说明所需的上游产物、接口约定或确需排序的共享修改。无前置依赖时用空列表 |
| 验收责任 | 将需求验收项映射到责任 Task，写明独立行为测试、接入项目回归的方式及验收条件。跨 Task 场景分配给具备相应依赖的 Task |

能基于已确认的接口并行开发的工作应拆开。若多个 Task 会争用同一关键实现、接口仍待落实或共享变更必须按序生效，先缩小重叠范围，再为仍存在的前置关系加边。只有真实依赖才决定顺序，任务列表的排列只服务阅读。

平台在送审前校验唯一 key、依赖存在、自依赖、重复边、环及范围，登记 Task 时按同一冻结图再次核对。规划评审负责判断无环的图是否仍遗漏真实前置、引入无依据的串行边或漏分验收责任。修订走已有 ChangeSet 及人工确认机制，图与已确认计划一同冻结。登记时把任务 key 转成真实 Task ID 并保留映射，后续派发消费这一份事实。

### 6.2 经理 Agent 根据实际就绪状态派发

F07 复用 `foreach` 与 `work_binding`，每项确定的 Task 对应一个子实例和独立工作空间。子实例可先登记为排队，开始执行和创建 worktree 由平台资格控制。精简版经理 Skill 明确依赖驱动的派发规则，并实际绑定到出厂默认的项目经理 Agent。评审、测试和实现 Agent 通过冻结输入读取本 Task 与其实际前置交付。

1. 经理读取当前计划、已登记子实例和平台给出的就绪、阻塞原因及槽位。用内核确认的依赖完成与合入同步事实选出就绪任务。
2. 在可用并发额度内连续提交多个就绪派发。成功登记后继续派发其他就绪 Task，直到当前没有可派发工作或额度用尽。每项派发通过已有版本和授权校验，过期状态重新读取。
3. 等待实际事件。依赖 Task 合入并完成同步、worktree 就绪、可用槽位变化或返工改变依赖资格时，重新计算就绪集合。一个 Task 阻塞时，其无依赖关系的 Task 继续运行。
4. 全部有效子流程完成后再接纳父调用并进入 F08。依赖失败或返工只影响其依赖闭包，经理不得将失败任务计为完成。

例如 A、B 无前置依赖，C 依赖 A，D 依赖 B，E 依赖 C 和 D。在有额度时 A、B 并发。A 合入并同步后，C 可以在 B 运行期间开始。C、D 均完成后才允许 E。调度无需等待整批任务完成。

- 精简版 Task 子流程并发上限建议设为 4，随 Workflow 冻结并服从根流程、项目、全局及实际 worker 额度。详细版保留其既有并发配置。验收为专用项目配置足够额度，无需统一提高生产项目额度。
- 内核继续统一校验 Agent、Rust 和人工模式的派发资格。补充现有读模型和事件缺少的就绪原因、依赖或容量信息，复用已有工具，只有现有合同无法承载时才扩展接口。
- worktree 在依赖成果进入 Feature 后从有效基线准备。同一 Task worktree 保持单写者，不同 Task 使用独立 worktree。需要更新已准备基线时按版本和写者资格重新核对。
- 等待依赖的 Task 不抢占可运行子流程的槽位。等待人工、同步恢复与未知结果按真实执行状态分别核对 worker 和子流程配额，未知结果继续保护原操作。

### 6.3 合入竞争

A/B 都基于 F0 通过合并回归，A 合入使目标变成 F1 后，B 必须在 F1 上重新构造合并结果并回归。目标变化不要求重做未受影响的实现、测试设计和源评审；冲突解决改变代码时重新评审及测试实际变化。

协调按实际目标分支及版本进行，仅覆盖合入所需资源。保留资源级乐观校验、Git 版本前置条件及远端约束，不增加项目/全局业务写锁，不把长时间回归测试放入数据库事务或锁中。

### 6.4 完成与汇合

Task 完成共同核对子实例、源/目标、PR、合入版本、成功回归回执、远端事实和本地同步结果。状态、事件及解锁依据通过既有事务规则登记，模型报告不能替代完成事实。

远端合入成功但本地同步失败时，保留远端事实，仅恢复同步，不重复合入；同步前不解锁依赖。脏 worktree 保留现场并阻塞，不自动丢弃未知修改。

F07 等待全部当前有效子调用完成且交付版本进入 Feature。合法返工或范围变化按原依赖及父子失效规则重新计算汇合资格，不能拿历史“全部完成”快照继续交付。

## 7. 项目测试配置与自动证据

### 7.1 执行配置

复用现有项目配置及不可变引用机制承载工程执行信息。精简版按项目选择语言和测试执行器，详细版沿用原运行合同。拟议配置至少表达以下内容。

| 内容 | 要求 |
|---|---|
| 运行环境 | 实际运行时、绑定的 Harness/Sandbox、依赖准备及工作目录 |
| 独立测试 | Task 行为测试入口、有效 Red 判定方式和测试文件范围 |
| 项目回归 | 项目现有入口、参数、超时及必要环境；新增用例需被实际覆盖 |
| 合并回归 | 默认使用相同项目回归入口，在试合并结果上执行 |
| 文件与网络 | 按项目配置临时写目录、构建缓存和网络边界，由 Profile 落实 |

具体字段与源归属在 P0 冻结，本文不发布未实现的 API。执行入口采用可校验的程序/参数或受管理脚本及工作目录，凭据正文不能写入任务或日志。超时范围适配真实项目，不假定当前最多 300 秒足够。

缺必要配置或执行器不支持时明确阻塞，不能改跑少量 node:test 并声明项目回归通过。执行配置随运行冻结，后续变化按既有计划变更与授权规则处理。

### 7.2 三类测试

1. Red / Green：证明本 Task 新增行为由未满足变为满足。
2. 源分支项目回归：证明 Task/Feature 的完整待交付版本通过项目回归。
3. 合并结果项目回归：证明该源版本与最新目标版本组合后的结果通过项目回归。

Task 和 Feature 都保留源分支与合并结果两次回归，符合用户要求；两次测试对象可能不同。取消的是只读说明后对相同版本、相同配方的无条件 verify。

### 7.3 记录与引用

平台采集真实命令、cwd、环境、起止时间、退出码、终止状态、原始输出、配方身份及代码/测试版本，生成可引用的不可变 Artifact。超时、取消、加载错误和断言失败分开记录，非零退出不得自动解释为有效 Red。

评审与 PR 引用这些事实，避免反复抄录日志和源码。必要来源与内容要求由 Delivery / Spec 规定；日志展示截断时保留可读取的原件。已约定的逐增量证据通过真实内容版本和原始记录关联，不能通过放宽摘要要求丢失历史。

## 8. PR、合入与人工审核

### 8.1 发布与合入分离

精简版通过类型化平台动作分别处理源分支发布、PR 创建/更新、试合并验证、合入授权、实际合入与同步完成。业务门禁留在内核。保留详细版和历史执行使用的 `git.publish`、`git.advance` 与回执语义，新增能力通过独立动作或可并行解析的合同版本接入。

每次交付关联项目、根/子实例、scope、计划、PR 仓库/编号、源分支/SHA、目标分支/基线 SHA、候选提交/tree、配方及测试回执。恢复复用原操作身份，PR 更新沿用本次交付身份，不按相似标题误认其他 PR。

### 8.2 验证与最终内容一致

P0 必须冻结可实际支持的合入策略，证明试合并与最终落地内容一致。merge、squash、rebase 的最终 SHA 可能不同于临时提交，接纳应明确核对提交关系与 tree，不能一律要求 merge_commit 等于临时 SHA，也不能只检查 merged=true。

远端合入必须保证测试和批准对应的源/目标前置条件。先读目标再调用合入可能存在竞争窗口，不能把它当作原子版本保证。P0 要验证远端端口、保护规则和必要服务端检查如何约束基线；无法保证时阻塞该组合，不能等错误内容落地后再声称已通过合入前验证。

保留分支保护及权限，不使用管理员 bypass、强推正式目标或直接推送目标伪装 PR 合入。远端额外检查未满足时等待，本地测试成功不等于远端规则通过。

### 8.3 两类政策与人工材料

| 政策 | 合入前提 |
|---|---|
| Task → Feature 自动 | 归属正确，独立测试/Green/Code Review/两类回归满足要求，版本有效，权限及远端规则允许 |
| Feature → main 人工审核后 | 适用检查满足，全部 Task 完成，真实 Human 对本次确切交付签署 |

政策随配置冻结并由内核核对。经理不能代签，也不能借 Task 自动政策写入项目主分支。业务评审已有合法人工例外时，引用原决定和覆盖范围，保留 approve=false 原报告；不扩大成测试成功、硬门禁豁免或默认 main 合入许可。

审核材料包含 PR、变更摘要、Code Review、源分支及合并回归结果、源/目标/合并版本和限制。源或目标变化使相应合并验证与旧批准失效，更新材料并重新审核。

### 8.4 恢复与返工

PR 创建、远端合入、本地同步分别保存意图与观察。网络中断或重启后读取原 PR/分支核对；已合入仅恢复登记与同步，明确未生效且仍有授权时才按既有规则重试。

评审拒绝、测试失败、人工拒绝与合并冲突分别定位。沿用已有经理研判、三种返工、共享额度及人工例外，选择覆盖根因的最小合法范围，不空转重审或把技术 retry 当业务返工。新候选、新问题不继承旧例外；删除节点或替换子流程不能重置额度。已合入任务的修复走后继分支/PR，不改写目标历史。

相关能力依据：[返工与人工批准实施](../review-rejection-and-approval/EXECUTION.md)、[合入记录](../review-rejection-and-approval/MERGE.md)。其历史 PLAN 保持原文，本计划接入已实现能力。

## 9. 主要模块改动

### 9.1 Skill、Delivery 和 Spec 核对

现有资源覆盖流程原语，但精简版需要同步发布下列业务合同。凡工作流节点、Agent Capability、交付端口、Spec 检查项或平台生产者发生变化，实施时从入口编译并检查实际发给 Agent 的任务正文与交付合同。

| 范围 | 当前资源核对结论 | 精简版实施要求 |
|---|---|---|
| 需求与设计 | PRD、Design 已有需求澄清、设计和人工确认合同。设计步骤仍引用仅支持特定技术栈的工程约束材料 | 保留通用 PRD、Design 合同。将代码基线与项目执行配置直接提供给设计及评审，对依赖旧输入、固定运行时或旧材料的指令发布精简版资源 |
| DAG 规划 | 规划 Skill、Spec 已要求 Task DAG 和验收映射，Task Plan Delivery 只强制 `key`、`title`、`instructions`、`dependencies`。旧方法还把 `verify` 写进测试分工 | 新建精简版规划 Skill、Task Plan Delivery 和对应 Spec。校验边的有效性，交付依赖依据、范围与验收责任，按第 6.1 节调整测试分工。Task 登记或工作流接口消费同一结构化输出 |
| 经理编排 | 现有编排 Skill 已要求并发查看就绪工作并等待事件。依赖满足仍以旧 Task 完成事实为入口 | 新建精简版编排 Skill，明确就绪集合、可用槽位、单 Task 完成触发后继、阻塞隔离与实际合入同步。保留原有审批、返工和故障恢复职责。默认经理 Agent/Capability 须实际加载新合同 |
| 独立行为测试 | 测试设计方法支持独立断言和区分环境失败。TestRecipe Delivery、对应 Spec 与执行器都固定为 `node_test`、文件列表和 1–300 秒 | 为精简版输出项目支持的独立测试入口、用例及可核验 Red，复用现有独立测试所有权与失败分类。更新精简版测试设计 Skill、测试配方 Delivery、Spec、工具执行合同与权限 |
| Task 实现与原始证据 | TDD Skill 和实现 Spec 写入文档、`verify` 顺序。Execution Evidence Delivery 声明生产者为 Agent，要求逐次提交日志和文件全文 | 新建精简版 TDD Skill、实现 Spec 和平台证据合同。T03 引用真实记录，保留明确约定的逐增量证据、环境和执行结果，区分 Agent 原生观察与平台测试回执 |
| Code Review | 独立评审 Skill 和 Review Report 信封适合复用。详细版实现、收尾与候选 Spec 需要后续验证、报告或候选，而 F08/T05 早于回归与 PR | 按 Task/Feature 评审时点配置精简版 Code Review Spec。检查实际差异、范围和已产生的测试事实。保留定位证据和独立性，使用机器门禁核对后续回归与合入证明 |
| 项目回归、PR 和完成 | 回归/发布 Skill 与 Spec 按详细版候选评审及本地接纳顺序工作。Publication Report 绑定 `git.publish`，完成报告仅记录 `task_id`、状态、操作 ID，旧交接合同依赖 `verify`、`documentation` 和 `candidate_review` | 配套定义精简版回归、PR 创建、试合并、人工审核输入及合入/同步回执的 Delivery 和 Spec。配置真实平台生产者和输入输出端口，区分已发 PR、已合入、已同步与 Task 完成。回归与合入由平台执行，删去对旧交接产物的必需引用 |

依据包括 [规划 Skill](../../../configurations/skills/project-planning.md)、[规划 Delivery](../../../configurations/deliveries/builtin-delivery-task-plan-v3.json)、[编排 Skill](../../../configurations/skills/workflow-orchestration-v5.md)、[测试配方 Delivery](../../../configurations/deliveries/builtin-delivery-test-recipe-v2.json)、[测试设计 Spec](../../../configurations/specs/test-design-v2.md)、[TDD Skill](../../../configurations/skills/tdd-implementation.md)、[实现 Spec](../../../configurations/specs/implementation.md)、[执行证据 Delivery](../../../configurations/deliveries/builtin-delivery-execution-evidence-v2.json)、[独立评审 Skill](../../../configurations/skills/independent-review.md)、[发布 Spec](../../../configurations/specs/release.md)、[发布回执 Delivery](../../../configurations/deliveries/builtin-delivery-publication-report-v1.json)、[Task 交接 Spec](../../../configurations/specs/task-delivery-report.md)和[Task 完成 Delivery](../../../configurations/deliveries/builtin-delivery-task-completion-report-v1.json)。

新建及改造后专供精简版的 Workflow、Standard、Skill、Delivery、Spec、Agent、Capability、Tool 和 Profile，其源 JSON、配套 Markdown 文件名及独立 identity 统一使用 `simplified-` 前缀，可见 `name` 保留中文“精简版”前缀。同步更新 manifest、`body_file` 或 `instructions_file` 及配置引用，继续使用 `<identity>-v<revision>.json`，例如 `simplified-skill-project-planning-v1.json`。通用且合同未变的资源可由两套复用，在资源清单标注为共享。若精简版仍从 Agent Capability、Spec、工具或 Profile 间接引用详细版专用文件，随本阶段解除依赖。

### 9.2 代码与资源入口

| 模块与现有入口 | 拟定职责 |
|---|---|
| domain：[testing.rs](../../../crates/domain/src/testing.rs)、[repository.rs](../../../crates/domain/src/repository.rs)、[execution.rs](../../../crates/domain/src/execution.rs) | 项目测试、合入策略、版本关联和结果的纯类型与规则 |
| core：[ports.rs](../../../crates/core/src/ports.rs)、[platform_facts.rs](../../../crates/core/src/platform_facts.rs)、[platform_runtime.rs](../../../crates/core/src/platform_runtime.rs)、[platform_publish.rs](../../../crates/core/src/platform_publish.rs)、[platform_reconciliation.rs](../../../crates/core/src/platform_reconciliation.rs) | 发布/PR/合入分离、共同门禁、回执、幂等及恢复 |
| DAG：[workflow_children.rs](../../../crates/core/src/workflow_children.rs)、[workflow_dispatch.rs](../../../crates/core/src/workflow_dispatch.rs)、[workflow_eligibility.rs](../../../crates/core/src/workflow_eligibility.rs)、[repository_runtime.rs](../../../crates/core/src/repository_runtime.rs) | 就绪派发、有效基线、完成/汇合及同步资格 |
| 审核与驱动：[workflow_controls.rs](../../../crates/core/src/workflow_controls.rs)、[workflow_review.rs](../../../crates/core/src/workflow_review.rs)、[workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) | 最终人工门禁、决定绑定、现有返工和例外适配、各驱动一致性 |
| adapters：[tests.rs](../../../crates/adapters/src/tests.rs)、[git.rs](../../../crates/adapters/src/git.rs)、[github.rs](../../../crates/adapters/src/github.rs) | 项目入口、临时验证、受约束 PR 合入、保护规则及实际结果 |
| [默认源配置](../../../plugins/README.md)、[manifest](../../../configurations/manifest.json)、[defaults](../../../configurations/defaults.json) | 发布两套定义及各自资源闭包，建立精简版带前缀的资源，切换实际默认引用 |
| [模板目录](../../../crates/core/src/templates.rs)、[配置默认值](../../../crates/core/src/builtin_defaults.rs)、[项目 Standard](../../../crates/core/src/project_standards.rs) | 新安装默认、兼容目录的出厂默认启用、项目显式选择和两套引用匹配 |
| [项目配置 Schema](../../../schemas/project-configuration.schema.json)、[Workflow Schema](../../../schemas/workflow.schema.json)、[project_updates.rs](../../../crates/core/src/project_updates.rs) | 执行配置、冻结、编辑及编译校验 |
| Web：[ProjectManager.tsx](../../../web/src/components/ProjectManager.tsx)、[WorkflowController.tsx](../../../web/src/components/WorkflowController.tsx)、[WorkflowDecision.tsx](../../../web/src/components/WorkflowDecision.tsx)、[WorkflowStageProgress.tsx](../../../web/src/components/WorkflowStageProgress.tsx) | 测试入口、并发进展、PR/合入/同步状态和真实人工审核 |

按实际依赖补充修改清单，不在 core 按业务名称硬编码流程。页面继续使用有界摘要和按需读取，保留材料及执行记录定位；后台推进不依赖用户打开页面。

## 10. 分阶段实施

| 阶段 | 依赖 | 工作与交付 | 完成条件 |
|---|---|---|---|
| P0 合同与基线 | 无 | 核实两套资源闭包、模板启用入口、DAG 交付、测试/权限/PR 路径，确定合入策略与返工映射并记录现有成本 | 缺口与验收场景明确，定义资源的专用和共享关系，远端保证有可验证实现路径 |
| P1 项目测试执行 | P0 | 扩展测试类型/执行器，接入项目回归、环境准备、超时和自动证据 | Node 兼容；至少另一个实际项目入口可执行；Red/Green/回归/环境失败可区分 |
| P2 PR 与正式合入 | P0 | 分离发布/PR/合入/同步，实现试合并、Task 自动政策及 Feature 人工政策，覆盖远端规则与恢复 | 正式目标不提前推进；陈旧基线及未批准 main 合入被阻止；未知效果不重做 |
| P3 DAG 与完成 | P2 | 接入真实合入及同步状态，补齐规划/登记验证、经理可读取的就绪与容量事实、并发派发及依赖解锁 | A/B 并发，A 完成即解锁 C。陈旧候选重验，同步失败保持阻塞，全部子流程完成才交付 |
| P4 两套配置发布 | P1、P2、P3 | 新建精简版 Workflow、Standard 和第 9.1 节配套资产，绑定实际 Agent。发布详细版更名修订，保持原闭包行为 | 两套均可解析与运行。精简版没有对详细版专用资源的依赖，能力、权限及交付合同可用 |
| P5 默认切换与界面 | P4 | 同步切换三项实际默认引用，覆盖新安装与兼容已有目录。完善项目选择、DAG 进展、人工审核及说明 | 采用默认配置可启动精简版，详细版仍可选，现有项目绑定与历史保持有效 |
| P6 联合验收 | P1–P5 | 两套兼容回归、真实 Git/Docker、多 Task DAG、远端 PR、人工审核、故障恢复及成本对比 | 第 11 节场景通过，记录实际边界、构建身份和成本，产出详细版专用资产的退役清单 |

P1/P2 中无依赖的实现部分可在 P0 后并行。各阶段实施及验收结果另写本目录 Markdown，原始日志留专用临时证据目录。本次仅编写计划，不执行这些阶段。

## 11. 验收标准

### 11.1 必选场景

| 编号 | 场景与通过条件 |
|---|---|
| A01 源闭包 | 业务阶段和步骤符合第 4/5 节。两套 Standard、工作流、工具、Spec、Delivery、权限及返工策略的确切引用均可解析 |
| A02 自动准备 | 派发时提供本次确切代码/工程配置；任务登记幂等，依赖环、缺失或跨 Feature 依赖被拒绝 |
| A03 Red | 测试先于新增业务实现；真实新增行为断言失败，环境/加载错误和已通过用例不能冒充有效 Red |
| A04 Green 与独立性 | 独立用例真实通过；实现者改写独立测试、缺回执或版本不符不能接纳 |
| A05 项目回归 | Task、Feature 及试合并实际执行配置入口，新用例进入回归；不支持或缺配置明确阻塞 |
| A06 DAG | 用规划 Agent 生成并通过评审的非平凡 DAG 驱动经理，在有额度时至少两个 Task 实际同时运行。A 合入并同步后，C 可在无关 B 仍运行时开始。依赖等待、并发额度、阻塞隔离与单 worktree 写者有效 |
| A07 依赖基线 | 后继基线包含全部依赖成果，不提前使用缺依赖的 worktree |
| A08 完成汇合 | PR 创建或实现结束不算完成；同步前不解锁；所有有效子实例完成后才启动 F08 |
| A09 PR 归属 | Task 指向所属 Feature，Feature 指向主分支；错误项目、base 或 scope 合入被拒绝 |
| A10 临时验证 | 在实际源/目标组合上运行回归，验证及审核前正式 Feature/main 未提前推进 |
| A11 目标竞争 | 两 Task 基于同目标验证，一个合入后另一个陈旧候选被拒，基于新目标重新回归 |
| A12 远端竞争 | 在远端合入窗口推进目标，所选策略阻止陈旧验证落地；最终提交关系/tree 与有效测试对应 |
| A13 Task 自动 | 满足门禁可无人确认合入 Feature；不能借此触达主分支或绕过远端规则 |
| A14 Feature 人审 | 无真实 Human 决定时 main 不变；经理不能代签；拒绝保持阻塞，批准仅作用于确切交付 |
| A15 版本过期 | 源、目标或相关交付变化后，旧回归、普通批准及人工例外不能越界用于新版本 |
| A16 分支保护 | 真实规则下走允许路径；不支持或要求未满足时阻塞，无 bypass、正式目标强推 |
| A17 未知效果 | PR 创建和合入请求边界注入故障，重启核对原效果，无重复 PR/合入或覆盖后继 |
| A18 同步故障 | 保存远端已合入事实，只恢复同步；脏 worktree 保留；成功后恰好一次完成登记 |
| A19 返工与例外 | 三种返工、共享额度、最小影响范围及真实人工例外保留；节点删除/子替换不重置额度，原拒绝不改写 |
| A20 自动证据 | 执行按版本和操作定位，超时/取消不可当成功，原始日志可读，权限、停止及计划变更门禁有效 |
| A21 驱动一致 | manual/rust/agent 对同状态的派发/审核/完成资格一致，自动模式不能越过人审 |
| A22 界面 | 根流程显示 Task/PR/合入/同步状态，可定位材料并签署真实决定，刷新不覆盖草稿 |
| A23 冻结历史 | 新默认不重写旧执行或决定，兼容合同下历史按原引用恢复，旧发布语义不被新工具替换 |
| A24 命名与实际默认 | 六个名称与第 12.1 节一致，`workflow_ref`、`task_workflow_ref`、`standard_ref` 均指向精简版。新目录及已启用新版出厂默认的兼容目录均通过正常项目预填、采用及启动路径，Feature 派发精简版 Task |
| A25 详细版兼容 | 项目可显式选择详细版 Standard 并启动其 Feature 和 Task。与改动前比较，详细版节点、顺序、策略、权限及行为一致。已保存项目、旧实例、旧决定和自定义配置均可按原引用恢复 |
| A26 配套合同 | 精简版专用 JSON、正文文件名及 identity 使用 `simplified-` 前缀，显示名保留中文“精简版”。编译实际默认 Agent 绑定和端口，确认任务正文、所加载 Skill、Spec、测试配方、权限及平台回执匹配各步时点。无详细版专用依赖 |
| A27 退役就绪 | 生成两套资源正向闭包和旧资源反向引用清单。在隔离副本移除仅详细版使用的出厂资源，验证精简版仍可完整发布、编译与运行，并记录历史或自定义引用的处理方式 |

### 11.2 验证层次

- 领域及配置测试覆盖规则、DAG、两套源闭包、默认引用、权限和修订。集成测试通过正式 core/SQLite/端口及故障注入覆盖状态和幂等。显式固定详细版用于兼容测试，让默认路径测试验证精简版，避免改用旧引用掩盖默认切换问题。
- 浏览器通过既有 tests/browser 覆盖配置、子进展、材料定位、人工决定和返工。
- 真实 Git/Docker 使用专用空数据集及仓库，验证实际入口、worktree、试合并、竞争和同步。
- 真实远端使用已授权的专用仓库/认证，验证 PR、Task 自动合入、Feature 人审、分支保护及最终内容；不接管用户正在运行的工作流。
- 实施后运行必要定向检查及完整 `./scripts/check/check.sh`。真实服务参数以届时已实现入口为准，不编造命令；付费模型、真实 GitHub、协议夹具及本地结果分别报告。

### 11.3 精简效果

以相同需求、Task DAG、环境和资源额度对比：业务阶段/步骤、内部工具动作、Agent/经理启动及轮次、材料数量/体积、测试对象及次数、编排等待和总耗时。外部模型、网络和测试耗时单列，不承诺未经测量的加速比例。

必须证明 documentation/verify/closure/重复 candidate_review 不再产生独立交接；快照、登记及归档没有变成同等数量的经理往返；同版本同配方不会仅因只读说明被重复执行。两类回归、独立评审、现有合法返工及最终人审继续保留，不能通过删除验收换取更短耗时。

## 12. 两套发布、默认切换与后续退役

### 12.1 出厂资源和名称

| 类型 | 精简版出厂默认 | 详细版可选 |
|---|---|---|
| Feature Workflow | 精简版Feature开发流程 | 详细版Feature开发流程 |
| Task Workflow | 精简版Task开发流程 | 详细版Task开发流程 |
| Standard | 精简版工程规范快照 | 详细版工程规范快照 |

精简版三个对象各用新身份。详细版三个对象继续沿用既有身份，以后继修订发布新名称。详细版 Feature 的子流程引用、Standard 的根引用和默认入口随详细版更名修订一起校对。详细版其余工作流正文、权限、并发、评审与发布行为沿用原合同，已发布历史修订及运行、决定、回执仍按原确切引用恢复。

两套 Standard 的 `root_workflow_refs` 分别包含对应 Feature 和 Task，`default_workflow_ref` 指向各自的 Feature。Feature 的 `foreach` 子调用指向同套 Task。任务登记和子流程编译以父工作已采用的 Standard 及冻结定义为准，避免详细版在默认切换后误跑精简版 Task，反之亦然。

### 12.2 实际默认值与兼容目录启用

1. 在同一出厂包更新 manifest、`defaults.json` 的 `workflow_ref`、`task_workflow_ref`、`standard_ref` 和所需的默认 Agent 绑定。用出厂 materialize 和现有编译入口同时验证两套闭包。实际启动按三项确切默认引用及其绑定走通。
2. 新数据集初始化时采用精简版出厂默认值。项目管理的预填及「采用默认配置」读取活动目录中的精简版，按现有保存和启动路径产生工作选择。项目仍可选择并保存详细版 Standard。
3. 已有兼容目录的默认值持久化在模板目录，单纯替换包内文件不会切换。实施范围包括通过现有配置发布机制接入新版出厂资产并启用相关默认值，必要时补齐该能力。只更新相关出厂成员和默认引用，保留自定义成员、已保存的项目和工作配置。对已启用新版出厂默认的目录，核对正常默认查询及新工作采用结果。启用过程可重放，不要求用户先执行会归档其他目录成员的全量重置。
4. 默认切换只影响取得默认值的后续配置。现有项目原有选择优先，重新采用按项目操作。沿用原冻结计划的运行无须重派发或重签。实施时保留两套共存所需的存储与合同兼容性。

### 12.3 最终验收后的详细版退役

本次交付保留详细版，同时为用户确认验收后发起的独立清理准备引用清单。对两套 Workflow、Standard 及默认 Agent 从根向下计算完整依赖，标记精简版专用、详细版专用和共享资源，并核对工作或自定义配置对详细版资源的反向引用。

验收通过后的清理以精简版运行及出厂配置可独立发布为条件。清理从 manifest 和当前出厂目录移除详细版 Workflow、Standard 及只服务详细版的资源，逐项核对 Skill 正文、Delivery、Spec、Agent、Capability、工具和权限文件，保留精简版仍依赖的通用资源。现有项目或仍在运行的工作引用详细版时先完成或经显式选择调整，数据集中供历史读取的冻结修订继续保留。移除后重新校验 manifest、默认引用、精简版闭包及可恢复历史。

### 12.4 完成判定

同步更新 README、工作流编排、协议、真实工程接入、权限/配置说明及 AGENT.md 的现行行为。遵守资源级并发约束并复用现有返工和人工决定。相关依据见[并发重构约束](../single-host-concurrency/PLAN.md)及[返工与审批实现](../review-rejection-and-approval/EXECUTION.md)。

实施完成要求两套配置可用、精简版为实际出厂默认、详细版保留既有行为。真实多 Task 流程能按 DAG 并发开发并自动汇入 Feature，整体评审和两类回归在所有 Task 完成后执行，main 仅在真实人工审核后推进。合入与故障恢复须有实际回执。交付包括相应代码、配置、说明、验收记录及退役清单。详细版移除安排在用户确认验收后的独立工作中。
