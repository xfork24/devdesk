# 源配置与执行解析

## 源协议

配置以 `{type, identity, revision, name, scope, 根级引用, data}` 表达不可变修订，引用为 `identity:revision`。源文件与数据库没有重复引用注册表、存储表选择或手工投影。源文件的正文路径在加载其所属修订时读取；其他定义只持有源引用。

Scope 使用 `global`、`project:<id>` 或 `feature:<pid>:<fid>` 字符串。引用白名单按类型定义，Capability/Agent 的组合引用、Delivery 的 `spec_refs`、Harness 的供应商/目录引用、Permission Context 的 `sandbox_ref` 和 Standard 的 `root_workflow_refs` 位于信封根级；其余内容及嵌套引用仍在 `data`。内容摘要覆盖根级引用与 `data`。

Agent 仅组合 Role 与 Capability。Capability 选择模型、Harness、方法及工具。技术能力、任务要求和执行授权分别表达：要求某 Skill/Tool 不会自动给 Agent 增加能力；具备 Tool 也不会自动获得本次调用权限。

## 独立授权

Permission Profile 保存底层策略，Permission Context 选择 Sandbox、绑定方式、步骤授权和 Controller 操作策略。项目保存确切 `permission_context_ref`。Harness/Sandbox 的绑定修订独立发布；运行开始记录实际采用的绑定引用，后续重试或恢复不追随新的活动分配。

默认步骤原有可写目录、只读目录和工具范围移至 Permission Context。Workflow 保留工作空间输入、工作目录、任务、预算、依赖及业务门禁。人类确认条件仍由 Workflow 描述；操作授权由 Permission 管理。Agent 和 Capability 都不含权限字段。

## 解析与恢复

1. 读取 Workflow、Role→Agent、Permission Context 及接入绑定选择。
2. 沿确切源引用读取 Role、Capability、Model、Harness、Skill、Tool、Delivery 与 Spec。
3. 校验类型、作用域、方法需求、流程依赖及执行授权。
4. 在内存中生成任务、运行环境、工具和权限视图，交付原有执行器。

固定源选择生成的 Plan 在进程内作有界缓存，避免每次状态读取重复编译整个闭包；缓存不落库，重启后重新生成。

数据库只保存源修订和实际执行选择、意图及回执。存储编码将配置视图替换成源引用；读取使用同一解析器重建。执行步骤、调度协议、输出接纳、TDD、独立评审、取消、返工和人工决定沿用原有语义。

源选择在运行开始确定。计划修改先预演并核对授权，接纳时发布新的 Workflow 源修订；已有执行保留原节点、输入、产物和来源。历史依据通过原源引用恢复，不保存第二份展开计划。

## Workflow 工具

`tools/builtin-tool-workflow-*.json` 是平台工具的源合同，定义调用名、输入输出 Schema 与处理器标识，不定义第二套工作流。Capability 引用工具表示具备调用能力；Permission Context 决定编排操作授权。执行时 Codex 的原生工具桥把 `devdesk_workflow_*` 调用交给 Rust 控制器；core 校验真实调用身份、epoch、计划版本和授权后，调用原有查询或工作流命令，持久化意图及实际回执。JSON 本身不执行调度。

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

修订选择器允许主动选择任何可见历史修订，包括未曾采用的中间版本；目录/定义编辑列表仍可只显示当前 head。Project 默认选择与 Work 冻结选择保留各自生命周期。新 Work 选择有稳定身份，选择本身不启动流程，已开始的工作必须通过现有运行计划变更入口切换。

共享源发布与项目采用可使用 `configuration.transfer.stage/apply/cancel`。暂存候选不进入可用源目录；采用时在项目库一次提交所需源副本和选择，再完成共享目录发布。途中状态持久化，Host 打开项目时先恢复 applying 操作，再同步目录。prepared 可取消；项目冲突在无采用回执时记录失败。该协议不宣称两个 SQLite 库存在原子事务。

## 运行资源源修订

`runtime_resource` 保存驱动、显示标签、许可证、不可变 OCI digest 或有界构建文件，以及按平台校验的登录下载件。Harness 用根级 `runtime_resource_ref` 选择精确修订；旧不可变出厂 Harness 的 `data.recipe_digest` 作为有类型内容地址边解析到该资源，不能用任意版本字符串查找安装。配置发布/预演不构建或执行这些资产。默认发行版只在资源包数据中指定。

安装以独立 `recipe_digest` 保存来源，版本仅为标签。同一驱动允许多个不同配方。`runtime.prepare_harness` 构建/拉取后核验实际镜像，再进行不调用模型的初始化/会话/动态工具注册探针；报告绑定 image、driver 和权限资产。报告明确 `inference_verified=false`、`tool_execution_verified=false`，真实会话仍必须完成权限、认证及执行边界验证。准备意图可通过 `runtime.preparation_status` 查询，取消在外部调用返回后的检查点生效，未知结果须 `runtime.preparation_reconcile` 后重试。

配置变更集补充 `manage_instance`（create/rebind/remove）、`permission_bindings`、`import_skill`，与源修订、项目/工作选择使用同一候选状态和依赖诊断。实例换绑创建后继引用，移除记录停用事实，旧会话/实例修订仍可读；预演中的身份由逻辑操作稳定生成。权限绑定通过注入的后端纯可行性校验，Skill 包路径/引用按既有约束验证，预演不执行包或外部工具。

## 可执行 Spec 与定位证据

检查项可声明 `assertion`：`text_length` 按 Unicode 字符计数，`required_sections` 检查代码围栏之外的 ATX 章节，`required_pointers` 检查 JSON Pointer 存在性。参数和标题属于配置。未声明 assertion 的检查项属于语义判断，不被结构 lint 视为已通过的质量证明。`evidence_required` 要求实际冻结输入位置；带机器 assertion 的评审检查也必须提供位置，并由内核复算结果。

`query.spec_lint` / `spec.lint` 接受精确 Delivery、未发布 Delivery 或 Spec 草稿与内容样本。草稿、Workflow 输入、材料导入、输出接纳使用同一结构检查链，JSON Schema 复用既有 Delivery.content_schema。输出封存 `spec_validation` 的 checker、源引用、内容摘要和机器结论；语义质量单独标为需要判断。预览结果不能替代实际内容接纳。

派发时保存 `workflow_input_manifest`，记录冻结输入摘要及实际源产物链。评审通过 `input_sources.evidence_source_ref` 和 value_digest 声明 `locations`，定位方式是 JSON Pointer、1 起始的闭区间行范围，或 0 起始/末端排除的 Unicode 字符范围；可选 quote 必须与该范围完全相同。外部输入、错误摘要、越界/悬空位置和伪造引用被拒绝。既有纯语义 Spec 的文字依据继续按其声明解释，不能冒充定位证明；可逐个发布后继 Spec 加强要求。

检查器有内容/数量/深度/工作量边界；JSON Schema 只接受有界的非递归本地引用展开，pattern 使用线性 regex 引擎及编译内存边界，不支持回溯式反向引用。规范不会执行任意命令；测试/Git/发布事实继续走实际平台用例和回执。
