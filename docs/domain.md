# DevDesk 领域规范

当前数据结构基线为 global 13 / project 44。Workflow 使用 v4，Spec / Standard 使用 v3，Agent 任务使用 `agent-task/v4`，普通 Proposal 使用 v2。只支持当前合同；过时开发数据需要删除重建，不执行迁移。完整字段见 [Schema](../schemas/README.md)，改造约束见 [通用执行合同](plans/workflow-business-configuration/CONTRACT.md)。

## 配置概念

| 对象 | 职责与引用 |
|---|---|
| Role | 用户定义的稳定业务身份、职责正文和可见范围；修订不能改变身份。角色名称不决定权限、工作类型或输出。 |
| Capability | 精确 Model、Harness、Skills 与 Tools 的能力组合。 |
| Agent | Role、Capability、长期补充指令、命名 Permission 选择和明确的执行能力上限。 |
| AgentID | 某个 Agent 修订的工作身份与经验范围。全局身份可跨项目积累经验，项目身份不能用于全局讨论。 |
| Skill | 可复用方法、材料及工具依赖。正文和包资源保留确切来源，不自动产生执行权限。 |
| Delivery 类型 | 任意业务交付物的说明、格式及可选离线 JSON Schema。 |
| Spec | 正文、任意内容检查项及 Role / Delivery / 标签适用选择器。 |
| Standard | 可选的 Spec 覆盖基线；核对实际匹配落点、必需规范及最低数量，不改写 Step。 |
| Workflow | 显式输入输出、Stage、Step、依赖与整体完成条件。 |
| Stage | 业务分组、背景和可选默认 Role；子流程使用自己的 Stage 配置。 |
| Step | 本次任务、执行方式、Role 覆盖、必需 Skill、Spec、端口、访问范围、预算和推进规则。 |

Step 的执行方式仅为 Agent、工具、子流程、控制。Agent Step 必须提供非空 `task_instructions`。任务正文属于 Step，方法属于 Skill，职责属于 Role，判断依据属于 Spec；没有业务标准动作目录或执行配方。

配置采用稳定 `definition_id` 与不可变修订 ID。发布、项目采用与执行冻结是独立操作。项目的 Role → Agent 绑定须覆盖整个子流程闭包；缺失或身份不符会拒绝执行，不选择内置角色补齐。

## 归属与实际对象

| 对象 | 归属和事实 |
|---|---|
| Project | 独立工程数据库、明确仓库、配置选择与主干。 |
| Feature | 项目内的工作范围，固定所采用的配置与来源材料；业务阶段从 Workflow 投影。 |
| Task | Feature 下的实际任务、完整业务元数据和依赖 DAG。由明确的创建命令或平台工具产生。 |
| RepositoryWorktree | 已登记的实际仓库工作副本、owner、分支、路径、基线与操作来源。 |
| WorkflowInstance | 冻结计划、Role 绑定、输入、调用来源、根实例、步骤执行及暂停/取消状态。 |
| StepExecution | 一次步骤执行的路径、轮次、序号、输入摘要、实际 Agent / Session / Turn / ToolCall、产物及状态。 |
| Session / Turn | 原生会话与轮次，记录准入、输入消费、真实终态和 worker 停止证明。 |
| WorkflowOutput | 精确执行和端口产生的值、Delivery 修订、输入摘要及封存 Artifact。 |
| Proposal / Document | 普通讨论的通用类型化材料，保存 `delivery_ref`、值及 `input_refs`；采纳不会隐式创建 Task。 |
| PlatformCall | 冻结请求、实际处理器及回执。Agent 调用另保存真实调用者和原生回复交付记录。 |
| Artifact / Snapshot | 哈希封存的不可变内容及有来源的上下文，不是可写的工程事实替代品。 |

Task 工作副本在依赖完成后才冻结当时的 Feature 代码基线。并行或重复子流程按完整调用路径和执行身份隔离，不用“最近会话”或业务节点名称寻找结果。

## 输入、判断与完成

端口使用常量或明确的生产步骤、输出名和 JSON Pointer。编译检查依赖、类型修订、子流程闭包与有界规则；实际接纳再次校验值和来源。禁止远程 Schema、任意脚本或模板求值。

模型返回统一封套：`status`、`summary`、`outputs`。完成时必须提供必需端口；`needs_input` 和 `blocked` 可以没有产物。内容报告的检查 key 和数量严格来自当前 Spec，模型拒绝保留原报告。

完成规则只读取有界事实：全部/任一、存在、值相等、集合覆盖、先后、不同执行身份、可信结果与人工决定。模型报告、实际工具回执及人类决定分别保存。人工决定绑定当前输入摘要和确切产物，配置明确允许时才可说明理由覆盖完成条件。

技术重试产生新执行，业务返工产生新轮次并沿消费关系失效后继。子流程恢复同时恢复其父调用。人工等待和祖先暂停不消耗活动执行预算；独立暂停的子流程不会被父流程恢复覆盖。取消等待真实执行者停止，未知副作用保留核对状态。

## 权限与真实证据

Permission 使用完整 v2 profile、确切目标绑定和 `follow_target` / `pinned` 选择。两层策略、执行范围、Tool 和运行后端共同限制实际访问，只能收窄授权；Role 或提示词不能扩大权限。实际运行保存策略、来源、作用域、后端与资产摘要。详见 [权限说明](../permissions/README.md)。

代码输入绑定受管理工作副本、owner、修订、路径和分支。Feature 会话只有在当前冻结任务明确授权时，才可使用其 Task 工作副本。同项目不代表可以任意跨归属读取。目录授权支持安全相对路径，不内置 `tests/` 前缀。

测试事实来自固定代码和实际 Docker 执行。候选事实来自实际 Git，包括父提交与确切 diff；接纳核对对应候选测试、确切确认、本地 CAS、远端 lease 和 PR merged。普通 JSON 或模型自述不能伪造这些事实。

## 存储与经验

全局目录和共享配置位于 `global.sqlite3`；每个项目使用独立 `engineering.sqlite3` 与 Artifact 根。配置使用类型化 `governance_records`，数据库和领域规则共同保护不可变修订、请求及终态回执。具体结构见 [SQL 说明](../sql/README.md)。

全局讨论单独保存于 `conversations/global/engineering.sqlite3`，不创建占位 Project。经验是带来源的只读历史材料，跨项目读取不切换活动工程、不启动源 worker，也不覆盖当前代码、配置或产物。

Project、Feature、Task 不保存空的执行绑定；执行来源由 Workflow 冻结计划、StepExecution 和 Session 的 RuntimeBinding 表达。材料 Snapshot 保留真实 runtime_bindings、执行与会话来源，不再保存旧 BindingLock。Definition 不保留旧 slots/rules，业务判断由当前 Spec 和 Workflow 明确声明。
