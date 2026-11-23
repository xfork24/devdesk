# 默认源配置

此目录使用扁平源定义协议，发布源数据和只读引用说明索引。每个源 JSON 包含 `type`、`identity`、`revision`、`name`、`scope`、根级引用字段及 `data`；引用统一为 `identity:revision`（Delivery 的 `producer.tools` 例外，按工具 `qualified_name` 声明行为约束而非内容引用）。修订不可变，使用方显式采用新引用。`manifest.json` 声明发布资源、默认选择与引用索引文件，不指定数据库表。文件名统一为 `<identity>-v<revision>.json`，目录内修订以文件内部的 `revision` 字段为准。

`reference-usage.json` 按确切源引用保存 Spec 的 Delivery、Capability 与 Workflow 步骤使用方，Skill 的 Capability 使用方，Tool 的 Capability 与 Workflow 步骤使用方，以及 Delivery 的 Workflow 输入与输出位置。该文件随内置包发布并纳入包摘要，可通过 `query.configuration_usage({id,view:"builtin"})` 读取。修改正向引用后运行 `python3 scripts/py/configuration_usage.py --write` 更新索引，出厂检查与 Rust 加载器均核对其与源数据一致。页面默认查询当前配置使用情况，因此自建或后继修订不会被旧的内置说明覆盖。

本地目录中每个配置身份只保留最新修订。发布后继时同步移除旧 JSON 及不再引用的正文文件，并更新清单、默认选择和依赖引用。数据集中的历史源修订及运行快照按原引用保留，供已有执行追溯。

## 职责与引用

| 类型 | 内容及直接依赖 |
|---|---|
| Role | 职责正文和定义适用范围。 |
| Agent | 仅 `role_ref`、`capability_ref`。 |
| Capability | 精确 Model、Harness、Skills、Tools；表示技术能力，不授予权限。 |
| Model / Harness / Sandbox | 模型参数、Harness 程序版本/安装件及 Sandbox Docker 基础环境；二者组合产生内部执行配方。 |
| Model Provider | 模型服务商实例：路由、协议、端点、凭据约定；由 Harness 按 `identity:revision` 引用。 |
| Model Catalog | 单一 Harness 构建已验证的模型容量目录，通过 `recipe_digest` 关联构建配方。 |
| Skill | 工作方法和工具需求，不复制职责、文档规范或权限。 |
| Tool | 参数合同、处理器或接入来源，以及技术运行环境需求。 |
| Workflow | 原有 Stage、Step、依赖、任务正文、端口、预算、业务完成条件及确认要求。 |
| Delivery | 格式、离线 JSON Schema、必填 `spec_refs` 与可选的产出来源约束。所有交付的内容要求由对应 Spec 定义。 |
| Spec | 唯一规范正文和内容检查项。 |
| Standard | Workflow、Delivery、Spec 及声明关系的不可变快照，使用者分别引用。 |
| Permission Profile | 文件、网络、工具、审批与平台操作策略。 |

Role / Skill / Spec 正文通过各自的 `instructions_file` 或 `body_file` 引用 Markdown。文本归其源修订所有，不向 Agent、Delivery 或执行配置复制。

内置 Harness、Permission Profile 和 Tool 的 `name` 使用英文工程名称。Capability 的引用选择器使用同一名称和精确修订，工具调用仍使用 `qualified_name`。

出厂包发布当前 Feature 与 Task 使用的 20 个 Delivery 和 14 个 Spec。PRD、设计、Task DAG、测试配方、实现报告、独立评审与平台原生证据使用具体类型；工作区、任务创建、测试回执、PR、合入同步及任务汇总保留明确的结构和来源。每种 Delivery 关联非空正文及具体检查项的内容 Spec。

平台产出的 `producer` 声明允许的工具处理器，运行时核对真实调用、当前步骤和输出映射；名称及自述状态不作为来源证明。快照和候选的 Diff 通过独立输出端口传递，保留比较端点、平台操作、内容摘要与 Artifact。Test Doc 的可执行部分位于 `recipe`，测试工具显式消费该字段。

## 权限与环境

权限预设唯一来源为 `permissions/profiles/`。仓库根目录 `permissions/backends/` 保存执行器资产。项目选择 Sandbox 与可选 Harness/Sandbox Profile；步骤可直接引用两层 Profile，显式选择替换同层默认绑定。Agent、Capability、Tool 不保存权限规则。

步骤的工作空间输入和工作目录属于任务上下文。文件与工具限制属于 Profile；受限步骤直接引用冻结的专用 Profile，未列出的步骤使用项目选择或目标默认绑定。TDD、评审、确认和候选接纳行为不变。Workflow 的 scheduling 包含并发、动作、租约、重试和 Agent 唤醒预算，随修订与 Standard 一起冻结。

## 执行与存储

没有 `projections/` 或静态 Agent 实例。执行端通过统一解析器读取源修订，在内存中生成 Agent、Skill、运行环境、权限和 Plan 视图。源库只保存源修订；运行库保存选择的引用、执行身份、输入产物及真实回执。派生视图不落库。重启后从原引用重新解析，默认选择变化不影响已有运行。

数据库中的源修订使用 `(identity, revision)` 主键；完整引用是虚拟列。配置摘要、显示元数据和依赖闭包按需计算。模板重置重新选择出厂源修订，不改写或重新分配源身份。

## 发布与校验

新协议只支持新数据集，不升级旧库。构建校验源形状；发布和执行检查引用类型、修订、作用域、能力需求、工具合同、依赖 DAG、权限及输出来源。

默认 Feature Development 有 19 个节点（14 个业务步骤），Task Development 有 14 个节点（10 个业务步骤），各 5 阶段。目录自洽（manifest ↔ 文件 ↔ defaults 引用 ↔ 正文归属）由 `scripts/py/check-defaults.py` 把关；结构校验在构建期，交叉引用与能力组合校验在测试期 materialize 完成。

源与运行时记录统一使用 `type` 判别类型；`scope` 为 `global`、`project:<id>` 或 `feature:<pid>:<fid>` 字符串。Capability 的 `harness_ref/model_ref/skills/tools`、Agent 的 `role_ref/capability_ref`、Delivery 的 `spec_refs`、Harness 的 `model_providers/model_catalog_ref`、Standard 的 `root_workflow_refs` 位于根级，其余载荷保留在 `data`。能力标签和嵌套引用保持原位。内容摘要同时覆盖引用与载荷。只支持当前协议的新数据集。

## 默认交付自检

默认六个角色通过 Codex 使用 `builtin-model-gpt-6-2-sol-high:1`（`gpt-6.2-sol`、`high` 推理档位）。GLM 5.3 和 MiniMax M3.1 Flash 提供对应的 DSH 可选 Agent。Harness 候选目录用于创建自定义 Model。带定位的 content_checks 报告必须先调用本步骤 spec.lint，通过 structure_valid 与 content_report_valid 后才最终提交。lint 返回实际冻结文本边界和有界章节定位，允许在同一轮修正草稿；不能删除必需证据、改摘要或放宽越界检查。简单 Markdown 文档仍可直接按冻结合同自检。

GPT-6.2 Sol High、GLM 5.3 和 MiniMax M3.1 Flash 的 Independent Reviewer 均采用 Independent Review Capability，其中包含 Independent Review 和 Code Review 两项 Skill 以及 Review Report Specification。三个 Requirements Analysis Capability 均引用 PRD Specification。

三个模型的 Project Management Capability 均引用 Project Operations、Workflow Coordination 和 Workflow Orchestration，并引用同一组平台工具。原生工具组按各自的 Harness 选择。根流程自动编排还需要 Harness 驱动支持和相应的控制工具授权。

三个模型的六类业务 Agent Capability 均引用 Deliverable Validation。三个 Test Engineering Capability 也都引用 Release Verification。

Codex 的六类业务 Agent Capability 和 DSH 的两个 Project Management Capability 配置了 `spec.lint`。Agent 步骤通过所选 Profile 允许该只读工具。它只检查当前步骤冻结的输入/输出 Delivery 与内容，不获得经理配置写权限。默认 PRD/Design Spec 检查实际必需章节，默认关键评审要求冻结输入的精确位置；语义质量、真实测试、独立评审和最终接纳仍按原流程判定。所有依赖通过精确修订引用发布。

## 默认流程与出厂升级

实际默认引用为 `builtin-workflow-feature-development:3`、`builtin-workflow-task-development:3` 和 `builtin-standard-engineering-delivery:3`，Sandbox 使用 `builtin-sandbox-docker:2`。工程源定义和正文使用对应的英文工程名称，正文规范使用中文。兼容性回归使用 `tests/fixtures/configuration` 中的测试夹具。

出厂资源嵌入 Host。重新构建并启动新版 Host 后，在设置的「重置模板定义」弹窗中选择「恢复默认配置」会更新默认选择并保留自定义定义与已保存项目配置。选择「重置配置」会以新版出厂目录重建活动模板，并清理自定义定义、草稿和失效选择。需要重新配置的项目应采用新版 Standard 与 Agent 绑定。

## 自动项目测试入口

项目配置默认读取所选主分支的已提交配置，不执行仓库脚本。优先使用 `.devdesk/tests.json`（ProjectTestConfiguration），其次识别 `package.json` 的有效 `scripts.test`、具有测试的 Cargo/Go 项目、pytest 配置及 Python unittest 文件。没有回归配置时保存 `regression:null`，平台为对应代码版本记录未配置、直接通过；独立 Red/Green 不受影响。配置中不必填写 image，执行时使用冻结工具运行环境的实际镜像。已有入口失败、缺依赖、超时仍按失败处理。

项目页展示识别结果，可重新识别；JSON 手动覆盖收在高级选项。入口与设置随新工作冻结，后续修改不改变既有运行。
