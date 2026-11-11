# 源配置与执行解析

## 源协议

配置以 `{kind, identity, revision, name, scope, data}` 表达不可变修订，引用为 `identity:revision`。源文件与数据库没有重复引用注册表、存储表选择或手工投影。源文件的正文路径在加载其所属修订时读取；其他定义只持有源引用。

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

Delivery 描述格式和结构合同，`spec_refs` 指向唯一内容规范。PRD 及 Design Doc 的正文只保存在 Spec。任务渲染时按输出 Delivery 解析对应规范；内容报告及完成门禁仍使用 Step 明确声明的检查集合，避免配置重构改变业务检查行为。

生产、输入传递和评审都使用产物所属 Delivery 修订及其固定 Spec 修订，不读取当前默认规范替代历史依据。实际 PRD、设计文档和报告属于 Artifact，不是配置。

## 数据边界

只接受源协议 v2、新的数据集标记和当前 SQLite 基线。没有旧协议转换器、数据升级入口或兼容执行分支。不要使用既有数据目录验证本次改动；验证使用独立临时空目录。
