# 软件工程工作台：领域与架构原型实施计划 v0.2

日期：2026-11-07  
状态：实施修订版；以本文件为唯一领域基线；替代《原型实施计划 v0.1》的交付范围  
领域基线：本文件及第 14 节用户确认的实施决策（唯一基线）

## 1. 核心目标与执行边界

**首版集中建立领域规范、Rust 内核与完整 Web 客户端的基本架构，并跑通符合领域模型的完整软件工程流程。**

这是一套真实运行、真实校验、真实保存历史的领域应用。Git、测试执行和 Agent 推理等外部能力可由模拟适配器提供。模拟不能绕过领域规则、直接改数据库状态，也不能把页面上的定时动画当作流程实现。

用户已确认：

- 所有流程均需有符合领域模型、可以跑通的实现。
- 首版不要求真实接入 Git / TDD 测试执行器 / Codex；若确实不可避免才能接入。当前判断：采用明确的外部能力契约即可模拟，无须提前接入。
- 原型起同时支持 Linux / macOS 的 Rust 内核与 Web UI。
- Web 是长期完整功能客户端；未来 macOS 增加 Native App，Linux 继续使用 Web。
- Rust 承载全部工程规则；UI 展示事实并发起领域命令。
- 强制规范、版本锁定、分阶段交付、Review、历史记录和可替换实现仍是设计约束。
- SQLite 与本地文件真实存储；data 根目录真实可选。
- Archive 与 data 迁移沿用已确认范围：仅交互预览。它们不属于本轮必须真实运行的工程开发闭环。
- Docker、其他外部系统与 Swift UI 延期。以后真实 Harness 优先接 Codex。

**与计划 v0.1 的修正：** 移除真实 worktree、真实 rebase/squash、真实测试运行、真实 Codex 和真实外部进程故障恢复作为首版完成前提。保留这些能力的领域契约、模拟行为和后续接入验收要求。不要将两份计划的必做项相加。

本轮停止线：完成本文 F0—F5 与验收矩阵后交付，收集用户反馈；不要继续自动实现后续真实适配器。

## 2. “真实领域流程、模拟外部能力”的明确含义

| 层次 | 首版要求 | 不允许的替代 |
| --- | --- | --- |
| 领域规范 | 真实定义对象、归属、版本、命令、状态转换、门禁和错误 | 只写概念名，所有东西装进任意 JSON |
| 应用用例 | 真实创建需求/设计/任务，调度 Run，交付、Review、合入并保存历史 | UI 按钮直接设置 status |
| 规范执行 | Rust 校验确定版本、证据、Review、配置继承与合入条件 | 让模拟 Agent 直接宣布整阶段通过 |
| 持久化 | 真实 SQLite、文件 Artifact、修订、事件和重启后读取 | 浏览器内存或 localStorage 作为工程库 |
| 并发 | 真实异步调度模拟 Runs，执行依赖、额度和逻辑写许可 | 前端播放“并行”动画 |
| 外部能力 | 模拟 Git、测试结果、Harness、Sandbox/Resource 事件 | 把模拟结果伪装为真实 Git SHA、进程或测试证据 |
| Web | 所有本期领域操作可通过 Web 完成 | 静态截图、只有导航可点击、需手改数据库才能推进 |
| 可替换性 | 通过 ports 隔离模拟实现，用契约测试验证调用边界 | 用 `if demo` 在领域服务中跳过规则 |

模拟模式与真实模式必须可以区分：

1. 首版 Project 固定 `execution_mode=simulation`，暂不开放 live 模式；所有相关输出继承来源标记。
2. 文档、输入和评审记录是真实保存的文件/记录；其中引用的模拟工具事实明确标记 `origin=simulated`。
3. 模拟项目可在自身规则下完成流程，UI 显示“模拟流程已完成”，不能显示“真实代码已合入”。
4. 后续 live 模式不得使用模拟测试或模拟 Merge 证据满足真实门禁；混合来源的 Delivery 必须保留来源明细。
5. 用户的人类 Review 是真实的操作决定，但不会将其评审的模拟证据转换成真实证据。

## 3. 首版需要跑通的完整闭环

```text
Project / Workspace 配置
  → 创建 Feature（绑定 Agent / Skills / Standard）
  → Feature 规划 Attempt
  → 需求调研与 Requirement 交付 / Review
  → Design、Task / Test 计划交付 / Review
  → 保存 Feature 初始 Session Snapshot
  → 创建独立 Task 与逻辑 worktree
  → 并行 Task Attempts，各自运行串并行 Run DAG
  → 模拟 TDD：Red → 实现 → Green → 交付验证
  → Task Delivery / Review / 最终 Snapshot
  → Task Merge 候选、验证、Review → Feature
  → Feature 收尾 Attempt、完整 Tests、Docs / Review
  → Feature 最终 Snapshot
  → Feature Merge 候选、验证、Review → Mainline
  → 完成记录、执行报告、评审报告、资源释放与历史回看
```

同时必须跑通：技术失败/Run 重试、Review 退回/新 Attempt、暂停派发/取消、需求变化/证据失效、目标版本变化/重新验收、人工 Override 和程序重启/恢复模拟执行。

“所有环节 Review”按正式 Delivery 边界实现；临时 token、日志片段和评审报告本身不再递归产生无限层 Review。

## 4. 技术与架构基线

### 4.1 单机跨平台结构

- Rust library 承载 domain 与 application；独立 host binary 负责生命周期、HTTP 与静态资源。
- Web 使用 TypeScript + React + Vite，构建为 SPA；UI 不包含领域状态机或数据库写入逻辑。
- Tokio + Axum；客户端使用 JSON-RPC 2.0 over loopback HTTP，事件使用 SSE。
- rusqlite + 随程序构建的 SQLite；关系表、显式外键、SQL 初始化与结构升级脚本、短事务和单写入协调。
- 普通本地目录存储 Artifact；JSON 清单、Markdown 文档、分段日志；哈希固定交付版本。
- 构建后由 Rust 提供 Web 静态资源，不依赖 Node 开发服务器；Node 仅用于开发构建。
- 只有一个本机程序，无数据库服务器、云服务部署、多租户或远程服务管理。
- 平台验收基线：Linux x86_64 / Ubuntu 24.04 LTS 与 macOS Apple Silicon；两平台实际运行验证，不以编译代替。
- 平台差异限制在路径定位、数据集锁、浏览器启动等基础适配，领域层不得依赖 macOS API。

依赖选择沿用计划 v0.1；实施开始时验证并锁定工具链与依赖版本。不要因首版是原型而把实现写成后续需要整体替换的一次性脚本。

### 4.2 模块边界

```text
web/                 展示、输入、协议客户端
crates/domain/       ID、修订、领域对象、状态规则、不变量
crates/core/         用例、ports、调度、交付/评审/Merge 协调
crates/adapters/     SQLite、文件、模拟 Git/Test/Harness/Sandbox
crates/host/         CLI、HTTP/SSE、会话鉴权、静态资源、装配
schemas/             公共协议、Delivery/Report 结构
sql/                 数据库初始化与结构升级脚本
fixtures/scenarios/  可重复模拟场景和故障注入
tests/               领域、契约、集成、Web E2E
docs/                领域规范、协议、ADR、演示
docs/plans/          按计划归档；各子目录的 PLAN.md 为计划，所属报告与证据同目录保存
```

domain 不依赖 I/O 或任何适配器；core 依赖 domain 并定义 ports；adapters 实现 ports；host 负责装配。可以适度合并 crate，但不能反转依赖。避免按每个对象拆服务、按每张表建策略接口或预先实现插件装载框架。

### 4.3 最小协议

- `POST /rpc`：领域命令与查询；不提供通用 `set_status`。
- `GET /events`：持久化事件的 SSE 投递，支持 cursor 补读；页面断线后可重新查询。
- `GET /artifacts/{id}`：鉴权后的产物读取，不接受任意文件系统路径。
- `GET /capabilities`：版本、可用能力、模拟/延期/预览状态。

命令包含 command_id、target、expected_revision 和 payload。相同 command_id/参数返回原结果；不同参数复用 ID 拒绝。长操作返回 operation_id。可信 actor 从调用身份与执行绑定产生，不接受 Agent 自报 `actor=human` 来获得 Override 权限。

Rust 返回 allowed_actions 与 blocked_reasons。数据库事务保存状态和相应事件后再对外通知。仅监听 loopback；本地会话凭据、同源/Host 校验覆盖 RPC、SSE 和文件读取；无需实现账号系统。

## 5. 领域规范：先明确语义，再实现流程

F0 必须交付 `docs/domain.md`。每个对象至少说明：身份、所属对象、主要引用、可变/不可变内容、生命周期、命令、约束、领域事件、持久化方式和一个示例。下列对象无需都有独立顶层页面，但不能因 UI 简化而丢失语义。

| 领域组 | 对象 | 首版实施深度 |
| --- | --- | --- |
| 工程 | Project、Workspace、Feature、Task | 真实身份、归属、配置与状态；Workspace 保存路径/仓库描述，worktree 为明确标注的逻辑执行位置 |
| 需求与交付内容 | Requirement、Design、Test、Doc | 真实修订、验收条件、派生与验证关系、Markdown 编辑/查看 |
| 规范与身份 | Spec、Standard、Skill、Agent | 内置定义、克隆修订、版本查看、绑定锁；不实现任意规范脚本 |
| 执行 | Attempt、Run、Session、Harness、Sandbox、Resource | 真实记录、调度、来源和生命周期；外部会话/环境/进程事实由模拟适配器产生 |
| 上下文 | Context、Session Snapshot | 动态内容与不可变快照、继承来源、初始/最终快照；不实现真实环境重建 |
| 验收 | Delivery、Reviewer、Review、Override | 真实固定版本、逐项标准、决定、报告和例外记录 |
| 接纳与历史 | Merge、Artifact | 真实领域操作/文件记录；Git 目标推进与候选由模拟适配器完成 |
| 延期 | Archive | 记录契约与引用需求；只做交互预览，不实现导出、冷数据删除或恢复 |

关键规则：

1. Requirement → Design → Task → Test 是追踪链，不表示测试代码在实现后编写。
2. Feature 和 Task 各自拥有 Agent/Skills 绑定、Attempts、Deliveries、Reviews；Task 合入后仍有 Feature 收尾工作。
3. Attempt 固定输入、scope、StandardLock 与 ExecutionBindingLock；Run DAG 无环。
4. Run 只属于一个 Attempt 或 Merge；技术重试新建 Run，改变输入/范围/身份配置新建 Attempt。
5. Agent 是身份，Harness 是运行时，Sandbox 是环境。首版界面与接口中也分开，不合并成一个枚举。
6. Session 为会话身份，Run 引用 Session；首版可限制单 Run 单 Session，但相同可变 Session 不并发调用。
7. Context 可以更新；Snapshot 保存确定版本和来源。Task 创建自己的 Context，不共享可变 Feature Context。
8. Spec/Skill/Agent 修订不可变。Standard 按用途 slot 固定 Spec 版本；配置变更不回写旧 Attempt。
9. Spec 版本升级与替换实现分开。用 Internal Design Spec v1/v2 与 Alternative Design Spec 的最小数据验证，不冒称外部官方规范。
10. Delivery 固定输入/代码候选/文档/证据版本。Review 必须引用它；需求或交付变化使有关证据 stale。
11. Override 追加人类决定、原因与作用范围；不覆盖原 Review 或 TestResult。
12. 统一使用 Artifact、Resource 和 Merge，不重新引入 Content、ResourceLease 或 Integration 顶层概念。
13. ReconstructionManifest、StandardLock、BindingLock 等作为值对象；不为每个值对象增加独立管理服务。

### 5.1 状态与命令

```text
Feature: draft → planning → ready → developing → validating → accepted → merging → completed
Task:    draft → ready → running → reviewing → accepted → merging → completed
Attempt: prepared → running → waiting_review → accepted / rejected / failed / cancelled / interrupted
Run:     queued → running → succeeded / failed / cancelled / interrupted
```

状态枚举可在 domain.md 中补齐明确的人工例外终态与转换，但必须保持原始事实。paused / blocked 为独立控制状态，附原因与恢复目标。

必需领域命令包括：创建/修订需求与设计、提交规划交付、建立 Task、配置并锁定绑定、启动 Attempt、暂停派发、取消 Run、重试 Run、提交/评审 Delivery、申请 Merge、保存 Snapshot、人工 Override、查询历史。不要把“推进到下一状态”作为替代领域动作的主要 API。

## 6. 模拟适配器的行为契约

模拟适配器实现与未来真实适配器相同的能力边界；可控性留在适配器和演示入口，不污染核心规则。

| Port | 首版模拟实现 | 必须验证的语义 |
| --- | --- | --- |
| HarnessPort | 按场景产生分析、计划、执行进度、结构化交付与评审建议 | Session/Run 关联、事件、失败、取消、结果结构；Agent 不能直接批准自己的领域门禁 |
| GitPort | 保存逻辑仓库、branch、worktree、source/target revision、候选和提交图 | 来源固定、目标比较、rebase 后 squash 的结果语义、目标变化导致候选失效、一次成功推进一个目标版本 |
| TestRunnerPort | 按固定输入/候选返回可配置 Red/Green/失败结果 | TestResult 绑定候选、测试定义、环境与来源；失败类型可区分预期断言和环境失败 |
| SandboxPort | 创建/结束模拟环境实例及模拟 Resource | 所有权、开始/取消/结束、导出后清理、清理失败保留记录 |
| EngineeringStore | **真实 SQLite** | 关系、事务、版本、事件、分页、幂等与重启读取 |
| ArtifactStore | **真实本地文件** | 暂存、封存、哈希、版本引用、报告/文档可打开 |

模拟 Git revision 使用 `sim-rev-*` 等明确格式，不伪造真实 SHA；模拟 Resource 不填系统 PID；不修改用户仓库。模拟状态要可持久化并在重启后与工程记录对应。

模拟执行器支持事件驱动的受控同步点、固定故障注入和“继续下一步”。它只完成外部执行步骤，仍由 core 判断是否允许转换。测试不能依赖长 sleep 或任意随机数来制造成功。

提供同一契约下的不同测试夹具，验证成功、缺字段、失败、取消、重复/延迟事件；不需要为证明可替换性提前完整实现第二个供应商。

## 7. 真正执行的核心用例

### 7.1 规划与配置继承

- 用户创建 Feature，绑定规划 Agent、Skills 和 Standard。
- Feature 规划 Attempt 内运行需求调研、代码理解（模拟）、设计与任务拆分 Runs。
- 分别提交 Requirement 和 Design/Task/Test 计划 Delivery，按对应 ReviewSpec 评审。
- 只有必需规划交付已被接受，Feature 才进入 ready。
- 初始 Snapshot 固定 Context 与配置；Tasks 继承选择的快照与规范，独立覆盖 Agent/Skills，保存继承引用。

### 7.2 执行、调度与重试

- core 实际调度异步模拟 Runs；默认全局 4、每项目 2，可手动调整。
- 同一逻辑 worktree 写许可互斥；不同 Task 可并行；只读节点使用固定输入。
- DAG 依赖、并发额度、Review 等待与写许可分别显示阻塞原因。
- 暂停表示停止新派发；取消经过取消确认事件再终止，不提前释放许可。
- 技术重试保留旧 Run 并记录 retry_of；Review 退回后修订交付必须产生新 Attempt。
- 模拟 TDD 走 Red、实现、Green 节点并产生模拟证据；核心检查证据顺序、测试定义和候选适用性。不要声称已运行真实测试。

### 7.3 交付与 Review

- 执行报告与 Artifact 真实落盘，Delivery 固定其版本和哈希。
- Reviewer 可为人或 Agent；模拟 Agent Reviewer 通过独立 Run/Session 读取冻结交付，提交逐项结果。
- Review 结果由结构化命令交给 Rust 校验。pass、changes_requested、inconclusive 与 not_reviewable 分开。
- 缺少证据、版本失效或必需 Review 未完成时，门禁真实阻塞。
- 人工 Review/Override 可从 Web 完成；结果、理由、原决定与行为人持久保存。

### 7.4 Merge 与 Feature 收尾

- Task accepted 后申请 Merge；固定源/目标，调用 GitPort 构造模拟候选。
- 调用 TestRunnerPort 验证候选，正式 Delivery 经 Review，保存最终 Snapshot。
- 目标版本改变则使候选和相关验收失效，重新构造/验证；成功时调用 GitPort 的带预期目标推进操作。
- 数据库记录 Merge 结果并完成 Task；此处“completed”明确属于模拟项目。
- 所有必需 Tasks 完成后，运行 Feature 收尾 Attempt，补全 Docs、完整 Feature Tests 与 Review。
- Feature 再走同一 Merge 用例进入 mainline。完成后仍可追溯每个 Task、Attempt、Run、Review、Snapshot 和逻辑提交。

### 7.5 重启与历史

- 关闭浏览器不影响内核；页面重新打开通过查询与 SSE 恢复展示。
- 内核退出保存执行边界，重启将未确认运行标为 interrupted / needs_reconciliation，不自动当作成功。
- 提供“查看中断原因”和“创建重试”操作；不要求透明续跑所有内部步骤。
- 模拟 Merge 目标状态持久化，使用 operation_id / command_id 核对重复请求；重试不能产生第二个逻辑提交。
- Snapshot 可读取、比较并用于新模拟 Session 的上下文初始化；真正还原 Git/工具环境留待后续。

## 8. Web 完整功能范围

中文界面，保留英文领域名。采用清晰、适度信息密度的工作台布局，不做营销首页，不搭复杂可视化流程编辑器。

| 页面 | 首版必须可用的操作 |
| --- | --- |
| 初始设置 | data 路径输入/校验、初始化或打开已有数据集；显示模拟能力状态 |
| 项目总览 / Project | 创建/编辑 Project、绑定 Workspace 描述、选择默认配置、创建/筛选 Features |
| Feature | 需求与设计编辑、追踪链、Task 列表、阶段交付、Tests、Docs、Merge 与历史 |
| Task | 独立绑定、依赖、逻辑 worktree、Attempts、重试与评审状态 |
| Attempt / Run | DAG/列表、启动/暂停/取消、阻塞原因、日志、输入与输出、Session/Snapshot/Resource |
| Review | 固定交付版本、逐项标准与证据、通过/退回/不确定、人工例外接受 |
| Merge | 模拟源/目标、候选、验证、目标变化、阻塞和接纳结果 |
| 定义 | Agent、Skills、Spec、Standard 的版本、克隆修订、基本编辑与绑定；Harness/Sandbox 的能力与版本查看 |
| 设置 | data 定位、并发配置、运行状态与退出；Archive/迁移的清楚标注预览 |

要求：

- 始终能定位 Project → Feature → Task（可选）→ Attempt → Run；保留 Feature 自身的 Attempts 入口。
- “运行阶段”可自动推进已满足条件的步骤；门禁/暂停/异常时停下，不强迫用户逐个修改状态。
- 所有操作调用 Rust API；不在前端复制状态机，不靠隐藏按钮代替后端验证。
- 模拟来源持续可见，不能只在欢迎页提示一次。
- 支持基本空态、加载态、错误态、断线态、revision 冲突与 stale 状态。
- 日志/历史分页读取；前端不能一次加载所有项目历史。
- Archive / 迁移预览不写真实业务结果；显示“预览结束”，不能显示“归档成功”。

## 9. 数据与版本化最小规范

- SQLite 真实保存 Project/Feature/Task、Requirement/Design/Test/Doc 修订、定义/绑定、Attempts/Runs/Sessions、Context/Snapshot、Delivery/Review/Override、Merge、Artifact、Resource、事件与操作记录。
- 对高频查询字段使用规范化列、外键与必要索引；扩展配置可有 schema 约束的 JSON，不将整个项目作为一个 JSON blob。
- 同一个 data 只允许一个可写 host；初始化和重新打开真实可用。data 改到已有新位置时需重启，不能暗中搬动原文件。
- 产物目录镜像 Project/Feature/Task；Feature 与 Task Attempt 都有 runs、deliveries、reviews 和 execution-report。
- Artifact 先暂存、封存并校验哈希，再提交可见引用；manifest 不成为第二套可写工程状态。
- 定义与交付修订不可变；最低限度 schema_version、migration 和协议版本从首版建立。
- 历史查询以 owner/status/time/id 为常用索引方向；日志文件分段，不将每个流式字符写成数据库事件。
- 本轮允许内部数据结构演进，但不得用“删库重建”作为正常升级或恢复方式；仅演示 reset 可清理自己创建的独立数据集。

## 10. 阶段计划与退出条件

### F0 — 领域规范与架构契约

交付：

- `docs/domain.md`：对象、归属、状态/命令、规则、事件、版本与示例。
- `docs/protocol.md`：命令/查询、幂等、revision、错误与事件。
- `docs/design.md`：模块边界、ports、持久化与跨平台结构。
- ADR：完整 Web、跨平台、领域真执行/外部模拟、后续真实适配边界。
- 工程骨架、工具链/lockfile、TDD 与阶段报告约定。

退出条件：从“创建 Feature”到“完成 Merge”的每个步骤均有明确 owner、输入、命令、产物、Review、状态与失败处理；关键规则已有可运行的领域测试，不只交付文字和空类型。

### F1 — 前后端与持久化纵向基础

实现：Rust host、SQLite 初始化与结构升级脚本、文件存储、HTTP/SSE、data 选择/锁定、Project/Feature 创建查询、Web 基础导航。

退出条件：用户通过 Web 创建 Project/Feature，刷新和重启后仍存在；Linux/macOS 都能运行；构建后的程序可直接提供 Web，无需开发服务器。

### F2 — 规划与版本化交付闭环

实现：定义与绑定、Requirement/Design 修订、Feature 规划 Attempt、Task/Test 计划、Delivery/Review、人类入口、初始 Snapshot。

退出条件：可在 Web 完成规划与评审；未通过 Review 不能进入开发；修订不会覆盖旧结果；Task 的独立绑定及继承关系可查看。

### F3 — 模拟执行与完整开发闭环

实现：模拟适配器、Run DAG、异步并发、逻辑写许可、Session/Context/Resource、模拟 Red/Green、Task Delivery/Review、Task Merge、Feature 收尾与主干 Merge、报告和最终快照。

退出条件：从 Feature 需求输入到模拟 mainline 完成，全部通过同一真实领域 API 和规则跑通；每个正式交付有 Review，每个 Task/Feature 有完整历史。

### F4 — 失败回路与证据治理

实现：Run 重试、新 Attempt、Review 退回、需求/候选版本失效、目标变化重验、暂停/取消、Override、重启历史与中断处理、重复命令核对。

退出条件：第 11 节异常场景可重复演示；任何失败都不会被悄悄覆盖或自动通过；重试与重新打开不会重复创建逻辑提交。

### F5 — 完整 Web 交付与架构验收

实现：补齐定义、执行、交付、评审、Merge、历史页面的所有必需操作；Archive/迁移预览；双平台验证；README/demo/阶段报告。

退出条件：无需改源码、手改数据库或访问开发控制台，即可在 Web 运行和观察全部本期流程；代码边界测试与领域/契约/E2E 通过；交付已知限制和下一轮讨论清单。

各阶段都有执行报告与 Review。实现者自检不得冒充独立评审；可由独立 Agent 或人类评审，未完成则标记 pending。必要 Review 通过前不得宣称阶段已验收，可继续不依赖该结论的工作。

## 11. 演示与验收矩阵

| 编号 | 场景 | 必须观察到的结果 |
| --- | --- | --- |
| A1 | 需求 → 设计 → 两个 Task → Feature 收尾 → mainline | 完整模拟闭环；Feature 规划/收尾 Attempts 独立存在；所有交付与报告可读取 |
| A2 | 两 Task 并行、单 Attempt 内串并行 | 后端活动 Run 时间重叠；依赖、额度和逻辑写许可真实生效 |
| A3 | Run 技术失败后重试 | 新 Run 指向 retry_of；旧失败保留，Task 未被错误完成 |
| A4 | Reviewer 退回后修订交付 | 新 Attempt；新旧输入/产物/评审可比较；旧结论不被覆盖 |
| A5 | 修改 Requirement / Design | 关联证据标记 stale，受影响 Merge 阻塞；重新交付验收后才能继续 |
| A6 | Task B 验收后目标被 Task A 推进 | B 原候选失效、重新构造测试与 Review；不能沿用旧结论 |
| A7 | Spec 升级、替换与绑定继承 | 新执行用新版本，旧 Attempt 保留旧锁；Task 可独立配置 |
| A8 | 暂停、取消和人工 Override | 暂停停止派发；取消等待确认；Override 追加理由，保留失败事实 |
| A9 | 刷新/重启/重复命令 | 状态与文件仍存在；中断清楚标记；无重复交付或逻辑提交 |
| A10 | Context 与 Snapshot | 动态 Context 更新不改变旧 Snapshot；Task 引用确定快照；最终快照可查看 |
| A11 | 模拟来源与模式边界 | UI/报告均标明模拟；模拟证据不能满足未来 live 模式规则测试 |
| A12 | Linux / macOS | 分别实际运行后端与 Web 主流程；缺失平台验证单列，不冒称已支持 |

测试分层：领域状态/规则单元测试；模拟与存储契约测试；SQLite/文件集成测试；真实后端驱动的 Web E2E。测试对象是本原型自身的行为，不是被管理项目的真实 Git 或测试运行器。

**区分两种 TDD：** 开发本原型时，对领域与后端功能执行真实 TDD 和测试；原型管理的演示任务，其 Red/Green 来自模拟测试适配器并明确标注。不能因为产品在模拟执行，就省略实现本身的真实验证。

基础性能只测可重复小样本：例如 10 Projects / 100 Features / 1,000 Tasks 的分页和事件展示，记录启动与包体积。首版不承担百万 Run 压测、分库、全文检索、生产级恢复编排等扩展工作。

## 12. 后续能力如何接入

本节是路线图，**不属于首版自动执行范围**。

1. **真实本机工程执行：** 实现 GitPort、TestRunnerPort、SandboxPort；把逻辑 worktree 与模拟候选替换为真实 Git/文件/进程。保留 core 状态机与 Review 门禁，增加真实 Git 和取消/清理/竞态测试。
2. **真实 Codex：** 实现 HarnessPort，建立能力/版本矩阵与供应商事件映射；验证任务、独立 Review、取消和产物。不要因供应商事件名称变化改写领域对象。
3. **恢复与数据生命周期：** 真实 Snapshot 重建、Git 副作用核对、Archive、冷数据、恢复与 data 迁移，分别建立验收规范。
4. **其他适配器与客户端：** Docker、GitHub、Hugging Face、DSH、Swift Native App；Web 保持每版完整能力。

可复用性的验收不是“将来保证一行不改”，而是：替换适配器不需要重写 Requirement/Feature/Task/Attempt/Review 的核心规则，也不要求 Web 绕过统一协议。

## 13. 给 Codex 的执行指令

1. 本文件为当前首版计划；不要把 v0.1 中真实 Git/Codex/环境恢复的必做项带回本轮。
2. 在用户提供的新仓库实现，按 F0—F5 交付；尽早形成可见的前后端纵向切片。
3. 建立领域规范并用代码测试固定它；不要先堆页面，再用零散条件拼接流程。
4. 模拟只位于外部适配器和明确的演示入口；不得直接写终态、跳过 Review 或伪造真实证据。
5. 使用一个主场景与可控故障分支覆盖全部流程，不做多个互不相通的演示页面。
6. 保留最小必要抽象；不开发通用插件平台、流程设计器、分布式任务系统或第二套 UI 状态机。
7. 不需要网络账号、模型调用或真实 Git 修改即可完成本轮。若认为某项必须真实接入，先说明无法由契约模拟的具体原因，再实施最小必要接入；不要仅因为工具可用就扩大范围。
8. data、文件和历史必须真实落地；独立演示 reset 只清理程序自己创建且可确认归属的数据，不能删除用户任意路径。
9. 阶段交付保存设计、执行、测试和评审报告。未完成的能力/平台验证必须如实记录。
10. F5 通过后交付可运行程序与说明，停止扩展，等待基于原型的下一轮反馈。

最终交付：源码、依赖锁、SQL 脚本、Linux/macOS 启动/构建方式、完整 Web、可运行模拟场景、领域/API/架构规范、阶段报告、演示说明和已知限制。不要求真实工具适配器，不以空接口数量作为架构完成度。


## 14. 实施决策（2026-11-08 用户确认）

- 本文件为唯一领域基线，缺失的外部架构文档不作为阻塞前提。
- 独立 Agent 评审各阶段；实现者自检与独立评审分别保存。
- 首先验收 Linux。macOS 提供兼容实现及构建说明，实机验证 pending，不宣称 F5 全部通过。
- Override 仅豁免明确的测试/Review 质量条件，记录行为人、理由、对象版本及条目；不豁免权限、完整性、来源、stale 或目标竞态。
- 完成的 Feature 冻结，后续变化创建关联的新 Feature；已合入 Task 的修改由后续 Task 承接。
- DAG 节点与 Run 执行分离；技术重试新 Run 带 retry_of，依赖跟随当前执行；改变输入/绑定/范围或 Review 退回后的修订新建 Attempt。
- 模拟 Git 与业务状态同 SQLite 事务推进；幂等操作避免重复逻辑提交。
- 正式 Delivery 边界：Requirement、Design/Task/Test 计划、Task 实现、Merge 候选、Feature 收尾。报告随所属 Delivery 冻结，不递归评审。
- 最终 Snapshot 固定已接受交付及 Context；Merge 独立引用其候选及验证，避免循环引用。
- 场景确定性生成可编辑建议和模拟证据，自动执行在 Review/门禁/异常处停止。故障注入不能直接写业务终态。


## 15. 本轮实施与交付状态（2026-11-08）

- F0、F2—F4 已实现并通过独立后端评审；F1/F5 的 Linux 范围实现和实际运行验收通过。
- 已完成 Rust 内核、真实 SQLite/Artifact、完整中文 Web、模拟主场景和异常回路；32 项 Rust 测试、真实 HTTP 检查及 3 项真实后端 Chromium E2E 通过。
- 运行包、构建说明、演示步骤、规范、逐阶段设计/执行/测试/独立评审和已知限制均已保存。
- macOS Apple Silicon 实机验证 pending，不宣称双平台 F1/F5 全部通过。
- 本轮停止扩展；真实 Git/Test/Codex、Archive/迁移执行和后续平台实际验收仍按明确边界处理。
- 详细证据以 `docs/acceptance.md` 和 `docs/plans/devdesk-prototype/` 为准。
