# 业务硬编码审查结果

审查基线为 `206deae` 及开始时的工作区。用户批准直接建立新合同、删除旧开发数据，不做迁移。计划中的历史代码位置见 [H01—H21 原始审查表](PLAN.md#3-当前代码审查与改造清单)。

| 编号 | 处理结果 | 当前落点与验证 |
|---|---|---|
| H01—H03 动作、配方与阶段语义 | 删除标准动作目录、`execution_recipe`、按 phase/key 调度的引擎及旧动作 Schema。 | `domain/workflow.rs`、`core/configured_workflows.rs`、`configured_runtime.rs`；自定义调研、控制、子流程和工具回归。 |
| H04—H06 职责与绑定 | Role 改为自由身份；不从职责推导写权限、输出 Schema 或 WorkType；缺失绑定明确报错。 | `domain/agents.rs`、`core/configuration.rs`、Host 解析；自由 Role、能力上限、作用域与权限后继测试。 |
| H07—H08 执行与提示组装 | 按 StepExecution 组装唯一 canonical 任务，包含任务、职责、技能、规范、材料、输出与授权的独立来源。删除旧提示分支。 | `agent-task/v4`；记录端口派发、实际 Artifact、补充输入、防伪、预览和重载。 |
| H09—H10 输出与提案 | 端口显式引用 Delivery 修订；支持离线结构 Schema。Proposal 使用通用类型和值与输入引用，采纳只创建材料。 | `proposals.rs`、`WorkflowDeliveries.tsx`；自定义风险材料、精确端口、泛化提案及跨项目拒绝。 |
| H11—H12 内容与完成 | 检查集合由 Spec 提供，模型结论与人工决定分别记录。完成规则只读取有界事实。 | 动态 key、模型拒绝、禁止覆盖、明确允许覆盖、确切版本确认、真实 Git/测试回执核验。 |
| H13 输入、重试与返工 | 删除文档标题和业务 kind 取材；确切输入绑定与摘要关联每次执行；返工失效传递后继，旧执行与子流程留痕。 | 输入不可变、技术重试、新轮次、兄弟子流程取消隔离及原执行者核对回归。 |
| H14 Spec / Standard | 适用与覆盖规则使用 Role、Delivery、标签和最低匹配数量，不依赖动作或固定阶段。 | `ConfiguredSpecification` / `ConfiguredStandard` v3；资源闭包、动态检查及编辑器。 |
| H15 界面 | 移除标准动作和配方生成器、固定阶段旅程及旧 Attempt/Delivery/Merge 操作区。表单和执行视图读取发布定义。 | 浏览器编辑、复制/导入/导出、Stage Role 继承、子流程、材料、任务预览及确认。修复刷新覆盖本地草稿的问题。 |
| H16 默认选择 | 业务 builtin ID 常量仅留于测试夹具；生产从当前目录及模板成员元数据读取，不按前缀选身份。 | 默认闭包、模板重置、精确历史绑定及离线独立包检查。 |
| H17 执行预算 | Step 明确配置轮数、时限、重试预算；编辑默认来自资源。原生 worker 使用实际 Step 时限；人工等待与暂停不消耗执行预算。 | 预算进入 canonical 任务；等待时钟和祖先暂停回归。保留协议默认、最大轮数/时限/并发及 payload 上限作为资源边界。 |
| H18 材料关系 | Feature 使用 `source_materials`；TaskScope / Document 使用 `input_refs`。删除旧 requirement/design/acceptance/test_recipe 文档特例及旧业务清单。 | 材料所有权、跨项目拒绝、快照、通用提案和 Schema。 |
| H19 目录与挂载 | 目录来自显式授权；删除 domain 和 Docker 中剩余的 `tests/` 前缀约束。工作目录和本次 Tools 可配置。 | 真实 Docker 的 `quality/checks` 局部写入成功，越界写入失败；保留规范化、symlink、元数据屏蔽与真实 writer 约束。 |
| H20 模型元数据 | 名称和默认地址移入 `model-provider-defaults.json`；Model 配置继续保存选择与参数。 | 服务商/端点/冻结参数回归；SDK 路由、认证和端点安全属于真实协议实现。 |
| H21 生命周期 | 业务阶段由 Workflow 投影；Step 使用明确技术状态，取消等待真实停止，未知副作用进入核对。 | 原生停止/恢复、取消、暂停、重试、平台请求/终态回执不可篡改和 Host 退出回归。 |

## 保留在代码中的能力

- 四种执行方式、原生供应商协议、JSON-RPC、SQLite、调度和持久化事务。
- 实际平台工具处理器，包括 Node 测试 runner、Git 候选/CAS/精确 lease/PR 核对；配置不生成不存在的工具能力。
- 权限、作用域、可信来源、不可变修订、输出和输入摘要、真实停止证明、幂等和未知副作用处理。
- 路径规范、symlink 防护、受保护元数据、载荷大小、调用深度、总节点和资源预算上限。

普通业务变化应修改 Role、Skill、Workflow、Delivery、Spec 或 Standard。技术能力扩展仍需要代码和对应验证。默认 TDD、独立检查、测试目录、确认门禁及职责是普通 JSON/Markdown 配置。

## 审查中补充修复

平台请求必须等于冻结的调度意图；会话和代码授权绑定实际执行来源；启动核对对象版本。Git 接纳只使用同一运行的候选和真实测试回执。Task 创建保留完整业务元数据，依赖 Task 的工作副本在前置完成后，才冻结当时的 Feature 代码基线。嵌套暂停约束整棵调用树，独立暂停的子流程不会被父流程恢复操作覆盖。

真实原生运行还发现了 Feature 会话读取 Task 工作副本时残留的 owner 相等限制。现在仅凭当前 canonical 任务的明确工作副本授权放行，并继续核对实际登记、项目、父子归属、代码修订、路径与分支；实际 Git 加录制执行器回归覆盖此路径。轮次派发失败会及时使 Step 失败，子流程的明确重试恢复父调用；新增回归检查失败原因、原执行留痕和最终完成。

补齐 Codex Agent 与独立 Step 共用的平台工具处理器，回归覆盖原生调用身份、冻结参数防伪、结构拒绝、取消及不可改写回执；实际 Codex 调用 `materials.capture` 已通过。候选回执补入实际父提交与 diff，使评审在不访问 `.git` 的情况下获得所需证据。前端讨论身份不再跳过 builtin 前缀的查询；接入绑定的默认 Harness / Permission 从当前配置目录读取。

移除 `git.publish` 隐式设置 Task 完成的业务耦合。默认 Task Workflow 在证据交接后显式调用 `tasks.complete`；非 Git 的材料工作也可按声明条件完成 Task 并释放其后继依赖。真实对象、作用域、依赖和停止证明仍由处理器核验。

原生验收补齐两处默认方法职责：实现评审前必须有文档后的真实交付复验；Feature 整体测试由其独立测试 Step 编写，不能假设已经由单个 Task 交付。后者明确任务、Test Design Skill 和测试目录写授权，再由快照与测试工具执行验证。两项均由 Workflow 配置表达，不增加业务节点名称分支。

## A01—A12 验收证据落点

| 验收项 | 证据 |
|---|---|
| A01、A07 自由身份与替换默认 | `configured_workflows` 的自建 Role / Agent、缺失绑定拒绝及冻结任务回归；实际 Codex 的数据研究流程只绑定自建身份，无默认 Agent 回退。 |
| A02、A06 非内置业务方法 | 实际“数据研究员 + 顺序分析 Skill”流程完成排序、封存和后继消费，不创建 PRD、TDD 或 Git 交付；纯材料 Task 完成回归不调用 Git。 |
| A03 结构化材料 | 实际输出 `[3,3,2,1]` / `sum=9` 由后继 Step 确切消费；错误类型、缺字段和 Artifact 持久化由 Rust 回归验证。 |
| A04 自定义检查 | 动态 Spec 检查 key、拒绝、确切确认、禁止覆盖和允许例外的 Rust / 浏览器回归。 |
| A05 名称与布局 | 自定义 inquiry、decision、research 等阶段与步骤实际运行；编译依赖和运行只使用显式引用，生产业务名称分支复查无命中。 |
| A08 Role 继承与覆盖 | 浏览器编辑、发布、重载和预览 Stage 默认 Role / Step 覆盖；编译器核对整个闭包绑定。 |
| A09 调用隔离 | 重复子流程、集合成员、兄弟取消、返工、不同执行路径和旧确认失效回归。 |
| A10 配置修订 | 浏览器任务 / Skill / Spec 编辑保存与重载，实际执行任务冻结及新旧修订隔离；原生执行记录保留采用来源。 |
| A11 目录与预算 | 真实 Docker 的任意 `quality/checks` 局部写、只读覆盖、根目录及越界拒绝；Step 预算、人工等待与祖先暂停回归。 |
| A12 模型元数据 | `model-provider-defaults.json` 为显示及默认地址来源；模型表单、服务商目录、端点与冻结参数回归。配置不能声明未实现的 SDK 路由。 |

## 数据与验收边界

旧 `data` 的 global 11 / project 42 数据已在停止旧 Host 后删除，保留 `credentials`、系统凭据和外部源码仓库。首次新数据集使用 global 12 / project 43，提交前复查清理后使用 global 13 / project 44；不保留转换或双版本运行路径。

macOS 与 Ubuntu 当前合同的默认回归、真实 Git/Docker、独立发布包及真实 Codex 完整工程发布均已分别记录。本轮 Ubuntu 按用户要求仅验收 Codex Harness，DSH/Ollama 不在本轮真实供应商验收范围内。不能用历史版本报告替代此次证据。执行命令、确切 SHA、PR 及具体限制见 [EXECUTION.md](EXECUTION.md)。

## 提交前独立复查

重新逐层检查生产代码中的业务身份、阶段名、默认 ID、旧协议、快照与会话路径，补充发现并清理：

- 删除未被执行链读取的项目/固定角色运行时偏好，包括 UI、保存接口、类型、SQL 与旧 Schema；Project 更新拒绝未支持字段，模型和运行后端只取完整 Agent。
- 删除 Project / Feature / Task / Snapshot 上的旧 BindingLock、空继承绑定和 live_binding 派生分支；快照保留真实 RuntimeBinding、Session 与执行来源。
- 删除 Definition 的旧 slots/rules、SpecSlot、AcceptanceCriterion、Check、Decision 与模板投影字段；Spec 检查项和 Workflow 规则仍使用各自明确的当前合同。
- 删除通用弹窗按 acceptance / slots / acceptance_keys 等字段名特殊解析的分支；输入解析只使用明确字段类型。
- 删除始终成功的 validate_workflow_command、旧 execution_task 运行锁覆盖、无效标准引用校验，以及 Run 为空/非空决定清理和快照的旧分支。技术测试环境使用 operation_id。
- 删除按 Task 共用或放宽 Session 运行锁比较的旧逻辑；每个 Agent Step 使用独立 Session，确切补充输入仍沿用同一 Step。新增回归覆盖同 Agent 的另一 Step 在已有活动 Turn 时独立建会话。
- 外部工具请求和直接会话补充输入统一检查祖先暂停。当前新库基线提升至 global 13 / project 44，无迁移或双合同路径。

保留的业务描述只在默认 JSON/Markdown、用户配置或测试夹具中；工具处理器、SDK 路由、安全约束与资源上限仍由代码实现。当前说明文档已同步，历史记录明确标注其当时版本。复查后的验证结果见执行报告。
