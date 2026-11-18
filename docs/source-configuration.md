# 源配置与执行解析

## 源协议

配置以 `{type, identity, revision, name, scope, 根级引用, data}` 表达不可变修订，引用为 `identity:revision`。源文件与数据库没有重复引用注册表、存储表选择或手工投影。源文件的正文路径在加载其所属修订时读取；其他定义只持有源引用。

Scope 使用 `global`、`project:<id>` 或 `feature:<pid>:<fid>` 字符串。引用白名单按类型定义，Capability/Agent 的组合引用、Delivery 的 `spec_refs`、Harness 的供应商/目录引用 和 Standard 的 `root_workflow_refs` 位于信封根级；其余内容及嵌套引用仍在 `data`。内容摘要覆盖根级引用与 `data`。

Agent 仅组合 Role 与 Capability。Capability 选择模型、Harness、方法、工具及自身规范的 `spec_refs`。完整能力随 Agent 身份装配，讨论、工作流、经理与编排入口保持一致；Workflow 只负责调度。技术能力、任务要求和执行授权分别表达：要求某 Skill/Tool 不会自动给 Agent 增加能力；具备 Tool 也不会自动获得本次调用权限。

## Profile 选择

Permission Profile 是唯一权限规则源。项目选择 Sandbox，可显式选择 Harness/Sandbox Profile；未选择时解析目标的默认绑定。步骤也可直接选择确切 Profile，替换同层默认选择。Harness/Sandbox 的默认绑定独立发布；运行开始固定实际目标、Profile 和绑定，重试与恢复不追随新分配。

存储态 Workflow 的 access 只描述工作空间输入和工作目录；步骤的 `harness_permission_ref`、`sandbox_permission_ref` 直接引用 Profile。内存 StepAccess 根据 Profile 推导文件和工具范围，不持有另一套可编辑权限。平台控制权限位于 Harness Profile 的 `policy.platform`，唤醒预算位于 Workflow 的 `scheduling.agent`。Agent 和 Capability 不含权限字段。

## 解析与恢复

1. 读取 Workflow、Role→Agent、Sandbox、Profile 及接入绑定选择。
2. 沿确切源引用读取 Role、Capability、Model、Harness、Skill、Tool、Delivery 与 Spec。
3. 校验类型、作用域、方法需求、流程依赖及执行授权。
4. 在内存中生成任务、运行环境、工具和权限视图，交付原有执行器。

固定源选择生成的 Plan 在进程内作有界缓存，避免每次状态读取重复编译整个闭包；缓存不落库，重启后重新生成。

配置目录使用独立的 `ConfigurationCatalogSnapshot` 读取源修订及必要的治理元数据，不恢复 turn、execution、session、snapshot 或消息历史。进程内索引保存确切引用、类型、模板成员、当前及完整历史 head、停用状态和模板来源；一次查询复用固定视图生成记录与来源标签。项目视图在内存组合已提交的共享源和项目源，目录读取不写入共享副本；工程命令仍先同步所需确切源，保留项目历史自包含性。 列表使用轻量索引，编辑当前对象和展开关联正文时按确切引用读取；历史选择器聚焦后加载历史修订。默认选择、Standard 闭包/预览和权限支持矩阵在同一配置快照内有界复用，缓存不会生成新权威源或改变规范化 JSON 与摘要语义。

配置提交成功后失效相应内存视图，事实更新不因新增事件或消息而清空配置目录。`catalog_version` 包含 Host 实例、全局配置、当前项目配置序列和已打开项目目录的版本摘要及 `source_epochs`，表示读取的新鲜度；浏览器拒绝同一 Host 下序列倒退的迟到响应。项目自有 Role 通过只读汇总保持全局可发现性，不改变其所有权或可见范围。模板 `catalog_generation` 继续表示重置代次。缓存逐出和重新构建不改变提交序列，重启通过新的 Host 身份隔离旧版本。写入、权限和执行仍按原有源修订与权威事务校验。

数据库只保存源修订和实际执行选择、意图及回执。存储编码将配置视图替换成源引用；读取使用同一解析器重建。执行步骤、调度协议、输出接纳、TDD、独立评审、取消、返工和人工决定沿用原有语义。

源选择在运行开始确定。计划修改先预演并核对授权，接纳时发布新的 Workflow 源修订；已有执行保留原节点、输入、产物和来源。历史依据通过原源引用恢复，不保存第二份展开计划。

## Workflow 工具

`tools/builtin-tool-workflow-*.json` 是平台工具的源合同，定义调用名、输入输出 Schema 与处理器标识，不定义第二套工作流。Capability 引用工具表示具备调用能力；Harness Profile 决定平台操作权限。执行时 Codex 的原生工具桥把 `devdesk_workflow_*` 调用交给 Rust 控制器；core 校验真实调用身份、epoch、计划版本和授权后，调用原有查询或工作流命令，持久化意图及实际回执。JSON 本身不执行调度。

## 文档规范

Delivery 描述格式、结构合同及允许的产出来源，必填 `spec_refs` 指向内容规范。所有交付的规范正文只保存在 Spec；创建、导入、模板加载及 Workflow 发布均校验有效规范和具体检查项。任务渲染时按输入输出 Delivery 解析对应规范；Review Report 的业务检查集合仍由评审 Step 明确声明，报告自身规范不递归加入该集合。

生产、输入传递和评审都使用产物所属 Delivery 修订及其固定 Spec 修订，不读取当前默认规范替代历史依据。实际 PRD、设计文档和报告属于 Artifact，不是配置。

平台工具映射的交付保存 `provenance`：实际调用、处理器、输出指针、内容摘要和规范引用。Diff 另外记录确切起始与结果版本；快照增量、Task 整体修改和候选修改分别输出并传入评审与交接。平台产出不能通过 Agent 提案或材料导入伪造。

超过 32 KiB 的显式上游产物输入可渲染为带摘要、字段指针和读取参数的引用。当前步骤必须获得 `workflow.outputs.read` 的能力与授权，按 `next_offset` 读取完整内容；业务 Agent 只能读取自己显式输入的产物和字段。原始内容仍保存为不可变 Artifact，缺少完整读取能力时阻塞派发。

## 数据边界

只接受扁平源协议、新的数据集标记和当前 SQLite 基线。没有旧协议转换器、数据升级入口或兼容执行分支。不要使用既有数据目录验证本次改动；验证使用独立临时空目录。

## Standard 源与执行选择

Standard 不是持久化的展开投影，它是持有确切 Workflow 根引用的不可变源。规范摘要、源图与差异按需计算；原有逐步清单改名为 `specification_view`，仅作为执行要求视图。Spec 发布新版本不会改变已有 Delivery、Workflow 或 Standard。

实际调用记录 `workflow_invocation` 固定工作身份、父执行、初始计划和 Standard；计划修订固定每次采用。Task 绑定真实业务 owner，不从节点名称或集合下标推断。子工作按自己所选配置独立编译，不复制父 Plan 替换文档。存储 codec 保存这些确切引用，并从历史计划恢复当时的原生任务正文。

## 批量创作与依赖诊断

`configuration.change.preview` / `configuration.change.apply` 在完整候选图中处理 Create、Revise、Retire、SelectProject、SelectWork；前端与受管工具共用应用服务。新源显式提供稳定身份与修订，可以反序提交相互依赖的对象。同一身份一次只发布一个后继，保持类型/Scope、检查当前 head；旧消费者不自动跟随。预演令牌绑定基准配置、操作、结果与 Actor，提交时重新核对，同库整体提交。审计时间不参与语义预演摘要。

`query.configuration_graph` 返回对象、带类型和字段路径的依赖边、使用方和诊断。根级引用、流程端口/角色/子流程、权限授权、目录、Integration 和工作选择有明确边定义；说明、示例与常量输入不产生依赖。递归 Workflow 由编译器拒绝，不把历史/普通引用一律当成 DAG。停用以身份为边界检查当前消费者，也检查它们对旧修订的引用；历史与冻结源保留。

`query.configuration_usage` 为 Spec、Skill、Tool 和 Delivery 提供按确切修订的展示目录：Spec 查询直接引用它的 Delivery/Capability，Skill/Tool 查询 Capability，Delivery 查询 Workflow 的阶段、步骤与端口位置。步骤输入沿显式来源解析上游 Delivery，常量和说明文字不产生引用。默认基于当前可见、未停用和未归档的消费者修订，支持分页与目录版本校验；不展开执行历史，也不改变冻结源。`view:"builtin"` 读取包内 `reference-usage.json` 固定索引；该索引由已发布源生成，随包摘要封存，加载及出厂检查核对其与源定义一致。它是只读说明元数据，不构成反向执行依赖，不写入源修订或数据库投影表。

修订选择器允许主动选择任何可见历史修订，包括未曾采用的中间版本；目录/定义编辑列表仍可只显示当前 head。Project 默认选择与 Work 冻结选择保留各自生命周期。新 Work 选择有稳定身份，选择本身不启动流程，已开始的工作必须通过现有运行计划变更入口切换。

共享源发布与项目采用可使用 `configuration.transfer.stage/apply/cancel`。暂存候选不进入可用源目录；采用时在项目库一次提交所需源副本和选择，再完成共享目录发布。途中状态持久化，Host 打开项目时先恢复 applying 操作，再同步目录。prepared 可取消；项目冲突在无采用回执时记录失败。该协议不宣称两个 SQLite 库存在原子事务。

## Harness 与 Sandbox 安装输入

Harness 的 `runtime_version` 是程序版本的唯一声明；`installation` 包含精确包来源、许可证、入口、扩展文件及本机发行件。Sandbox 的 `build` 包含基础镜像 digest、系统包、后端文件及可选的预构建镜像映射。组合器生成内部 ExecutionRecipe，或按 Harness 安装内容身份选择不可变 OCI 镜像。不存在第三种安装配置源，也不能用任意版本字符串猜测安装。发布和预演不构建资产。

安装以独立 `recipe_digest` 保存来源，版本仅为标签。同一驱动允许多个不同配方。`runtime.prepare_harness` 构建/拉取后核验实际镜像：Codex 探测 initialize、thread/start 与动态工具注册，DSH 探测 initialize，均不调用模型。报告绑定实际产物、driver 和权限资产，明确 `inference_verified=false`、`tool_execution_verified=false`。准备完成不证明推理或工具执行成功；真实会话另做权限、认证及执行边界验证。准备意图可通过 `runtime.preparation_status` 查询，`runtime.preparation_cancel` 在检查点生效，未知结果须先 `runtime.preparation_reconcile`。

准备器只保留 `devdesk/managed-<provider>:<recipe_digest 前 24 位>` 配方标签，安装与会话仍冻结实际 Image ID。同一配方的准备操作使用跨数据集的用户级文件锁串行执行；构建完成后才发布配方标签。替换标签前，若旧产物没有其他 managed 标签，先将其保留为 `<配方标签>-<完整 image digest>`；恢复旧安装使用已有标签或归档标签，不回退当前配方标签。旧版 prepared 标签在核验等价 managed 引用后移除，正常情况一个产物只有一个 managed 标签。

配置变更集补充 `manage_instance`（create/rebind/remove）、`permission_bindings`、`import_skill`，与源修订、项目/工作选择使用同一候选状态和依赖诊断。实例换绑创建后继引用，移除记录停用事实，旧会话/实例修订仍可读；预演中的身份由逻辑操作稳定生成。权限绑定通过注入的后端纯可行性校验，Skill 包路径/引用按既有约束验证，预演不执行包或外部工具。

## 可执行 Spec 与定位证据

检查项可声明 `assertion`：`text_length` 按 Unicode 字符计数，`required_sections` 检查代码围栏之外的 ATX 章节，`required_pointers` 检查 JSON Pointer 存在性。参数和标题属于配置。未声明 assertion 的检查项属于语义判断，不被结构 lint 视为已通过的质量证明。`evidence_required` 要求实际冻结输入位置；带机器 assertion 的评审检查也必须提供位置，并由内核复算结果。

`query.spec_lint` / `spec.lint` 接受精确 Delivery、未发布 Delivery 或 Spec 草稿与内容样本。草稿、Workflow 输入、材料导入、输出接纳使用同一结构检查链，JSON Schema 复用既有 Delivery.content_schema。输出封存 `spec_validation` 的 checker、源引用、内容摘要和机器结论；语义质量单独标为需要判断。预览结果不能替代实际内容接纳。

派发时保存 `workflow_input_manifest`，记录冻结输入摘要及实际源产物链。评审通过 `input_sources.evidence_source_ref` 和 value_digest 声明 `locations`，定位方式是 JSON Pointer、1 起始的闭区间行范围，或 0 起始/末端排除的 Unicode 字符范围；可选 quote 必须与该范围完全相同。外部输入、错误摘要、越界/悬空位置和伪造引用被拒绝。既有纯语义 Spec 的文字依据继续按其声明解释，不能冒充定位证明；可逐个发布后继 Spec 加强要求。

检查器有内容/数量/深度/工作量边界；JSON Schema 只接受有界的非递归本地引用展开，pattern 使用线性 regex 引擎及编译内存边界，不支持回溯式反向引用。规范不会执行任意命令；测试/Git/发布事实继续走实际平台用例和回执。

实现依据：[源协议与引用白名单](../crates/domain/src/source.rs)、[安装组合](../crates/domain/src/execution_recipe.rs)、[配置变更](../crates/core/src/configuration_changes.rs)、[冻结视图](../crates/core/src/frozen_views.rs)、[运行准备](../crates/adapters/src/runtime_preparation.rs)、[镜像标签与保留](../crates/adapters/src/runtime_images.rs)、[机器检查](../crates/domain/src/spec_checks.rs)。

Workflow 的 review_policies 与 rework_policies 随源修订和 Standard 冻结；新 Workflow 可编辑共享业务额度、三种返工方式与人工例外开关。策略引用真实步骤和规则原语，发布及计划替换验证入口/评审依赖和端口。旧执行采用后续处置策略须显式预演和计划变更，保留旧步骤合同与拒绝报告；既有工作预算首次采用需用真实 Human 决定明确历史基准。实现见 [业务返工](../crates/core/src/workflow_rework.rs)。

运行中的流程使用 ChangeSet `set_review_policies{rework_policies,review_policies}` 只调整后续拒绝处置规则，保留原步骤合同、执行和报告；首次采用时同批提交 adopt_rework_baseline 明确历史额度。
