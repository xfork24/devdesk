# 架构与 ADR

当前实现基线为 [DevDesk v0.3](plans/devdesk-v0.3/PLAN.md)，工程库 v34、全局库 v5。下文保留既有架构决策；旧 BindingLock/单阶段 planning 行为仅适用于历史兼容，新 Feature 使用配置锁及 Workflow 驱动。

## 模块

- domain：类型、不可变版本、DAG、绑定、来源与验收规则；无 I/O。
- core：命令/查询、调度与交付协调；定义 EngineeringStore、ArtifactStore，以及 `ports::{Harness, Git, Tests, Sandbox}` 执行接口。
- adapters：SQLite 关系表、文件封存、执行接口的具体实现；只装配原生供应商实现，不得绕过 core 改业务终态。
- host：loopback HTTP/SSE、会话与同源验证、数据集生命周期、静态 Web 装配。
- web：React/TypeScript 展示、编辑和调用 API；不设置业务状态。

## ADR-001：完整 Web

Web 是长期客户端，Rust 编译时嵌入 Vite 产物。Node 仅构建依赖，成品不需要 Node。未来 Native 客户端使用相同协议。

## ADR-002：跨平台单机

Rust stable 固定版本、bundled SQLite、普通路径及文件锁；不引入平台业务 API。原型的 Linux x86_64 与 macOS Apple Silicon 后端/Web 实机证据见 [验收记录](acceptance.md)。新增真实工程链路目前 macOS 通过；Ubuntu 宿主复验另见 [平台入口](ubuntu-web-acceptance.md)，不能以 Linux worker 代替宿主验收。

## ADR-003：明确区分真实与模拟来源

Web 项目使用真实 GitHub 仓库，打开时登记并接入，Harness 按需准备。产品已移除原型模拟执行器、回退、故障注入和受控继续路径；旧原型项目只读保留，历史来源不改写。Harness、Git、Tests 和 GitHub 只产生有来源的外部事实，核心决定状态；DockerSandbox 负责实际容器边界。测试替身只位于测试代码，不进入产品运行时。

## ADR-004：持久化与幂等

规范化关系表保存高频标量列、外键和索引；只有确定类型的数组、绑定锁和供应商结果用 JSON 列。初版每次工作单元加载类型化工程状态并写回变化，优先事务一致性；不把整个项目存成 JSON。查询返回有界结果。后续可在同一 port 下优化增量 SQL，不变更领域语义。

命令结果及其派发意图在同一事务保存；operations 唯一键保存指纹与原结果。Git 目标推进先持久化意图并在事务外调用，再提交核验后的回执；远端交付单独核对。单 host 数据集文件锁，进程内写协调。未提交的 Artifact 可成为孤儿文件，但不得成为可见交付；只读时验证 hash。数据库迁移按 user_version 执行，不以删库升级。

## ADR-005：恢复与范围

启动时遍历全局项目索引，核对所有项目的未确认运行；Run 标记 interrupted，未知讨论输入保留 needs_reconciliation。通过原适配器核对终态后才释放资源并允许显式重试，结果未知时保留资源与阻塞理由。供应商具体进程管理留在适配器中，内核恢复协议已由故障夹具验证。Archive 导出与 data 目录搬迁只预览，不写成功事实。只有自己初始化的空目录才能建数据集；仅在明确登记和授权的工程仓库执行受控操作。

## 阶段评审

每阶段报告列出实现、测试、限制和评审。独立 Agent 审查与作者自检分别保存；本轮 macOS 补充验收及独立结论见 [阶段报告索引](plans/devdesk-prototype/README.md)。

## ADR-006：持久化意图、回执与短事务

执行分为登记、调用、接纳三个阶段。core/effects.rs 管理 Harness、Cancel、Test 与 Cleanup 的 EffectRecord；core/advances.rs 管理 MergeAdvance。Run/Session/Resource 意图和操作 ID 先提交，再调用供应商；供应商调用不持有 SQLite 事务或单写协调器。收到结果后先保存原始回执，再在短事务内核对 Run 状态、输入版本、绑定、取消和目标版本。迟到结果保留为 discarded，不成为交付证据。

SQLiteStore 的 writer 只协调本地状态变更及文件封存工作单元；查询在工作单元执行期间仍读取上一个已提交状态。Harness/Test/Git 推进/清理已移到该协调器外，取消可在慢轮询期间提交。状态工作单元仍加载类型化状态，SQL 只 upsert 实际变化的记录；前端和事件查询有界分页，未实现大规模历史库压测。

Run 完成时封存文档与日志，随后同事务提交终态及 Cleanup 意图；在提交后以原 Resource ID 调用清理。导出失败复用已保存的回执，不重复执行供应商；清理失败保留历史并允许显式重试。崩溃后重复 delete 必须按同一 Resource ID 幂等，只有确认终止/释放才解除旧 worktree 的写许可。

## ADR-007：目标推进不是跨系统事务

merge.apply 只验证并登记推进意图，返回 operation_id；调度器调用 GitPort.advance。适配器身份、源交付、候选与预期目标固定。dispatched 表示已发出但尚未确认；重启必须向同一适配器查询，禁止盲目再次 advance。回执保存后，目标比较、逻辑 head 更新、Merge/Task/Feature 完成及事件同事务提交。

AdvanceObservation 区分 applied、not_applied 和 unknown。确认已推进时按同候选/同验收补记；确认未推进时记录观察到的目标、使旧候选失效，重新构造验证；结果未知时保持 needs_reconciliation。若输入或目标在调用期间变化，保存外部回执但不伪造完成。SQLite 回滚不能撤销外部副作用。

## ADR-008：锁定输入与阶段输出

Attempt.documents、scope、binding、context 与 baseline 是冻结输入；Attempt.outputs 保存本尝试生成的文档修订。后续 Run 明确接收 frozen_inputs、stage_outputs、锁定的定义正文及运行实例，不从可变 Feature 默认配置重新解释旧执行。Reviewer 和 Merge 验证继承源交付的 Standard/执行锁；最终 Snapshot 保存已接受 Attempt 实际使用的 Context，Context 更新另存追加修订事件。

Archive/数据目录搬迁执行与通用环境重建仍延期。真实供应商使用固定安装、独立凭据、持久化操作与核对契约。参见 [适配器契约](real-adapter-contract.md)。

## ADR-009：明确的仓库与测试请求路由

Attempt.git_context 在启动时冻结 Workspace、Project 和源/目标分支/worktree。MergeAdvance.context 从源交付复制；candidate、advance 和 reconcile_advance 均显式接收它。派发前再次校验输入、目标和 Review，失效意图记为未执行，不调用供应商。

TestRequest 附冻结的定义正文、GitContext 和 ExecutionInstance（包含执行回传的环境身份）；Harness 输入也携带代码定位。提供者不以进程全局 cwd 或可变 Project 当前设置定位工程。历史缺少定位的数据保持 null，不能自动根据当前配置倒填。

并行 DAG 的请求在首次派发时固定到 EffectRecord。后续轮询不重新吸收兄弟节点输出，避免 Green/Docs 或 Feature Tests/Docs 并行时改变供应商输入。评审独立性比较 Agent 修订链的身份根，而非仅比较 revision ID。

## ADR-010：每项目数据库与全局配置库

`GlobalStore` 打开 `<data>/global.sqlite3`，仅保存全局并发配置、共享 Spec/Standard/Agent/Skill/Harness/Sandbox 定义、项目目录摘要、活动项目 ID，以及配置命令的幂等记录与审计。项目目录摘要用于列举和定位，Project 的权威设置仍在项目库；打开项目时从工程库修正摘要，旧 revision 不覆盖较新的目录摘要。

每个项目独立使用 `<data>/projects/<project-id>/engineering.sqlite3`。项目库保存一个 Project 及其 Feature、Task、文档、执行、交付、评审、Merge、资源、事件与操作记录；Artifact 根目录也在该项目下。项目库不允许插入其他 Project。全局定义是不可变修订，使用时按 ID 复制到项目库以满足本地外键和独立历史读取；同步只追加，不改写已有执行绑定或定义正文。

host 的 Workspace 管理活动 Engine。`project.create` 先在全局库预留稳定 ID，在项目库提交创建，再发布目录并设为活动项目；崩溃后同一命令继续使用原 ID。`project.open` 验证目录和项目库后切换活动 Engine，并持久化选择。数据库缺失、身份错误或打开失败时保留原选择，不创建空库掩盖数据丢失。两库之间没有跨库原子事务；项目库提交与目录摘要分开，重开时修复摘要。

请求取得 Engine 后始终使用该实例，不因稍后切换而改写别的数据库。Web 为工程请求、Artifact 与 SSE 提交 `project_id`；活动项目已变化时拒绝过期作用域。项目切换会清除 Feature、分页、历史和快照对比选择。SSE 固定连接所属 Engine，事件 ID 使用 `<project-id>:<cursor>`，其他项目的 Last-Event-ID 不用于跳过本项目事件。

已打开的 Engine 保留连接，切换回来不会重新执行启动恢复，也不会把正常运行标为 interrupted。单调度器继续处理这些项目，并扣除其他项目正在运行的 Run 后计算可用全局额度。重启遍历全局索引加载并核对所有项目的原执行者，随后恢复上次活动选择，不重发结果未知的工作。

旧版单库按 SQLite 一致性快照拆分；旧文件不原地升级。迁移在临时副本上按当前 schema 序列升级，按领域归属复制所有工程行、事件、操作结果与 Artifact，并校验外键、归属覆盖和产物哈希。全部项目落盘后才在全局事务中发布目录和布局版本。中断或产物损坏后可重试；根目录旧库、旧 Artifact 和 `runtime.json` 留作原始来源，新布局成功后不再读取旧配置。此结构迁移与尚未实现的 data 目录搬迁是不同操作。

## ADR-011：原生执行接口与测试注入

`devdesk_core::ports::{Harness, Git, Tests, PullRequests}` 定义当前执行契约，分别由原生供应商、CliGit、CommandTests 和 GitHubPullRequests 实现。DockerSandbox 管理真实容器；领域模型中的 Harness/Sandbox 只描述配置与历史，不承担执行接口职责。

`ExecutionAdapters` 持有原生接口对象，由 `Engine::new` 和 `Workspace::open_with_adapters` 在装配时注入；只按冻结的供应商绑定路由。测试替身位于 `tests/support/`，不会编译进产品。历史操作的来源与固定身份只用于兼容核验，不提供模拟执行能力。具体约束见[适配器契约](real-adapter-contract.md#接口与装配)。

## ADR-012：真实会话、共享安装与双重交付

RuntimeInstallation 独立于 Agent、Session 和 RuntimeExecution。同平台同版本同摘要的安装只登记一次；Docker 不同角色共用镜像层，独立容器和状态目录。角色是不可变 Definition；同范围连续轮次原生 resume，不共享不同项目/Task 的可变 Session。Host 解析偏好并冻结 RuntimeBinding，客户端不能注入工程写权限。

讨论输出先形成 Proposal，经 schema、修订、归属、依赖与验收映射校验后采纳。正式工程阶段另有 Run/Delivery/独立 Review 门禁。同步模型事件按有序批次持久化，逐条去重；长输出保存在哈希封存 Artifact。

CliGit 负责工作树、固定版本导出、候选、CAS 和发布；GitHubPullRequests 负责策略检查、幂等 PR 创建与状态观察。单父候选在 Docker 重验并正式 Review 后，先本地 CAS，再远端精确 lease，最后核对同一候选 PR merged；不能把本地完成作为远端完成。严格规则不兼容时阻塞，不修改规则。

Feature 收尾递归固定已经接纳的 Task 交付，再执行完整测试、文档、复验和独立 Review。失败 Review 保留，修订使用新 Attempt。所有副作用先保存意图，外部 I/O 在事务外；未知输入与结果不盲目重发，停止、封存、清理分开确认。详细流程、限制与运行依赖见 [真实工程接入](real-engineering.md)。

## ADR-013 Permission 策略与执行后端

Permission 修订作为共享配置保存于全局库，被使用的修订追加复制到项目库。项目库 v32 接入权限定义和绑定，v33 将权限快照保存到每次 RuntimeExecution；全局库升级为 v4。SQL 升级保留既有运行锁和原始数据，既有默认映射为 legacy，新数据集采用容器全权限。

Host 在创建 Session/Run 时解析目标绑定、项目、角色与本次覆盖；适配器校验固定供应商版本和支持能力，再生成原生配置及 Docker 执行计划。两层权限取交集，Scope 和操作系统权限继续限制实际访问。未知字段、null 覆盖、无法执行的 allowlist 和冲突返回明确错误，不降级为更宽权限或模拟执行。

策略资产位于 `permissions/`，manifest 固定后端、平台及文件摘要，构建和运行预检校验所用资产。默认外层 Docker 隔离不要求嵌套 AppArmor 策略；只有选择内层受限执行器时才使用对应 seccomp/AppArmor。DSH 工具过滤器读取同一只读策略，原生 FullAccess 不继续挂载禁网 wrapper。容器临时空间的 exec/noexec 必须显式设置。

审批、隔离和资源额度与访问规则分别建模。模型连接与工具网络属于不同主体；Sandbox 禁网会影响整个运行时，不能用它实现仅工具禁网。当前不提供强制域名代理，网络 allowlist 明确不支持。全权限不提供同容器凭据隔离；凭据代理不属于本轮。实现与平台证据见 [执行报告](plans/permission-standardization/EXECUTION.md)。

## ADR：Workflow 及版本化治理

Workflow 是核心领域对象，组合交互契约与已有执行处理器。Global/Project 定义采用不可变修订；Project 默认可换绑，Feature 创建时保存完整依赖、编译计划和语义版本。AgentRevision 固定 Model/Harness/Skills/Tools/Permission，AgentID 的经验 Scope 单独选择。

持久化选择 governance_records 的类型化记录集合，避免草稿文档与可写图表形成两套事实。core 求值及记录幂等 intent，Host 在事务外解析真实运行时、派发并接纳结果；前端不设置完成状态。自动推进、人工确认、固定 TaskGroup 和返工轮次共用同一门禁。

经验目录是项目事实的只读、可重建投影；原生 Agent 只挂载自身 ID 的冻结经验文件。跨项目引用作为有来源的历史材料导入新会话，不回写当前 Feature 的配置、文档或代码。
