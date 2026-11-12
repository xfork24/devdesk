# 默认源配置

此目录使用源定义协议 v2，只发布源数据。每个 JSON 包含 `kind`、`identity`、`revision`、`name`、`scope`、`data`；引用统一为 `identity:revision`。修订不可变，使用方显式采用新引用。`manifest.json` 只声明发布资源与默认选择，不指定数据库表，也不重复维护引用图。

## 职责与引用

| 类型 | 内容及直接依赖 |
|---|---|
| Role | 职责正文和定义适用范围。 |
| Agent | 仅 `role_ref`、`capability_ref`。 |
| Capability | 精确 Model、Harness、Skills、Tools；表示技术能力，不授予权限。 |
| Model / Harness / Sandbox | 模型参数、执行协议及隔离后端技术条件。 |
| Skill | 工作方法和工具需求，不复制职责、文档规范或权限。 |
| Tool | 参数合同、处理器或接入来源，以及技术运行环境需求。 |
| Workflow | 原有 Stage、Step、依赖、任务正文、端口、预算、业务完成条件及确认要求。 |
| Delivery | 格式、离线 JSON Schema、必填 `spec_refs` 与可选的产出来源约束。所有交付的内容要求由对应 Spec 定义。 |
| Spec | 唯一规范正文和内容检查项。 |
| Standard | Workflow、Delivery、Spec 及声明关系的不可变快照，使用者分别引用。 |
| Permission Profile | 文件、网络、工具和审批策略。 |
| Permission Context | Sandbox 及权限绑定选择、Workflow/Step 的授权范围、Controller 操作策略。 |

Role / Skill / Spec 正文通过各自的 `instructions_file` 或 `body_file` 引用 Markdown。文本归其源修订所有，不向 Agent、Delivery 或执行配置复制。

当前发布 21 种 Delivery。Review Report、Task Doc、Test Doc、Execution Report、Task Report 分别用于评审、任务规划、测试设计、逐次执行和任务汇总；工作区、任务创建、测试回执、候选、接纳、发布、交接及完成记录使用具体类型，不再使用通用“结构化材料”。每种 Delivery 关联非空正文及具体检查项的内容 Spec。

平台产出的 `producer` 声明允许的工具处理器，运行时核对真实调用、当前步骤和输出映射；名称及自述状态不作为来源证明。快照和候选的 Diff 通过独立输出端口传递，保留比较端点、平台操作、内容摘要与 Artifact。Test Doc 的可执行部分位于 `recipe`，测试工具显式消费该字段。

## 权限与环境

权限预设唯一来源为 `permissions/profiles/`，授权上下文位于 `permissions/bindings/`。仓库根目录 `permissions/backends/` 只保存执行器资产。项目选择 `permission_context_ref`；Agent、Capability、Tool 均不保存权限选择或写入策略。

步骤的工作空间输入和工作目录属于任务上下文。可写目录、只读目录和可用工具限制由 Permission 中的 `step_grants` 表达，使用确切 Workflow 引用及步骤 key 定位。默认 42 个步骤原有授权原样迁移，TDD、评审、确认和候选接纳行为不变。

## 执行与存储

没有 `projections/` 或静态 Agent 实例。执行端通过统一解析器读取源修订，在内存中生成 Agent、Skill、运行环境、权限和 Plan 视图。源库只保存源修订；运行库保存选择的引用、执行身份、输入产物及真实回执。派生视图不落库。重启后从原引用重新解析，默认选择变化不影响已有运行。

数据库中的源修订使用 `(identity, revision)` 主键；完整引用是虚拟列。配置摘要、显示元数据和依赖闭包按需计算。模板重置重新选择出厂源修订，不改写或重新分配源身份。

## 发布与校验

新协议只支持新数据集，不升级旧库。构建校验源形状；发布和执行检查引用类型、修订、作用域、能力需求、工具合同、依赖 DAG、权限及输出来源。

默认 Feature 仍有 22 个步骤，Task 仍有 20 个步骤。目录自洽（manifest ↔ 文件 ↔ defaults 引用 ↔ 正文归属）由 `scripts/check/check-defaults.py` 把关；结构校验在构建期，交叉引用与能力组合校验在测试期 materialize 完成。
