# 默认配置目录

`manifest.json` 发布随二进制嵌入的普通配置修订；`defaults.json` 声明默认 Workflow、Role → Agent 绑定、能力和编辑预算。空数据集建立数据库副本，界面编辑和发布写入数据库。源码资源是出厂默认，修改资源不会覆盖已有修订。设置中的模板重置以目录代次切换当前配置，不回写已经冻结的流程。

## 概念职责

| 概念 | 配置内容 |
|---|---|
| Role | 任意业务身份及职责说明，不决定执行权限。 |
| Capability / Agent | 精确 Model、Harness、Skills、Tools、Permission 选择，以及明确的执行能力上限。 |
| Workflow v4 / Stage | 输入输出、步骤依赖、业务分组、背景、阶段默认 Role。 |
| Step | Agent 任务指令、必需 Skills、Spec、材料引用、输出合同、访问范围、预算及完成要求。 |
| Delivery | 内容说明、格式及可选离线 JSON Schema；不是一组固定业务文档。 |
| Spec v3 | 正文、Role/交付类型/标签选择器、任意内容检查项。 |
| Standard v3 | 可选的 Spec 覆盖基线，使用选择器、必需规范与最低匹配数量。 |

任务正文直接保存在 Workflow Step 的 `task_instructions`，只在 Agent Step 出现。Role 提供职责、Skill 提供方法、Spec 提供判断依据、Stage 提供背景。不存在标准动作目录、执行配方或任务文件选择分支。四种执行方式是 `agent`、`tool`、`workflow`、`control`；它们只规定执行协议。

## 默认工程流程

默认 Feature 流程配置需求、独立检查、设计、规划、创建 Task、逐项调用 Task 子流程、整体测试、文档和候选接纳。默认 Task 流程配置测试编写、工作空间快照、实际 Red、实现、实际 Green、文档、交付复验、独立评审与候选检查、人工确认及接纳。TDD 顺序、测试目录、职责和确认策略均在 JSON 或所引用的 Skill/Spec 中，可以发布自己的流程替代。

默认六种职责是示例配置，不限制用户定义新 Role。所有引用在 `manifest.json` 中声明，模板重置按身份重建完整引用图，不通过 builtin ID 前缀猜测默认对象。`projections/` 保存当前执行配置的投影；`initial/` 只负责首次初始化 Agent 身份。

平台工具包含 `workspace.current`、`workspace.snapshot`、`materials.capture`、`tasks.create`、`tasks.complete`、`tests.run`、`git.candidate`、`git.advance`、`git.publish`。这些处理器需要代码提供真实能力；业务流程选择哪个工具、参数来自哪里、如何等待确认属于配置。测试工具需要明确的运行环境，不创建占位 Agent 或 Session。

Codex Agent 可在 Capability 和当前 Step 中显式选择平台工具，通过真实原生调用进入同一处理器；函数名称使用 `devdesk_` 加处理器名称的可移植映射。当前 DSH 不支持动态工具桥。需要批准的平台写入使用显式工具步骤和确认门禁。Git 候选返回实际父提交及确切 diff，供配置中的独立评审引用。

内容检查输出按 Spec 的动态 key 提交 `approve`、`summary`、`checks[{key,passed,evidence}]`，检查集合须精确匹配。模型拒绝保留原报告；人工决定绑定确切输入及输出，只有 Step 明确允许时才能说明理由覆盖完成条件。

模型显示名称与默认地址在 `model-provider-defaults.json`，实际 SDK 协议、认证与安全校验仍由适配器实现。MCP/HTTP 服务及凭据引用在数据库中配置；资源目录不包含密钥。
