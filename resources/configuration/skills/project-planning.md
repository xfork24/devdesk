# 项目管理与 Task 拆分

## 意图

把已采纳 PRD 和 Design 转成可独立交付、可汇合的 Task 图。

## 交付物

Task Plan（task_plan 提案）。正文采用 Markdown；协议要求的 ID、验收与 Task 数据保留在对应结构化提案字段中。

## 工作方法

不重做架构，不执行实现。每个 Task 保持小而闭合，避免循环依赖、重复验收和没有测试配方的条目；输出 task_plan 结构化提案。

## 内容检查

- 关联 requirement_id 与 design_id。
- 每个 Task 的稳定 key、标题、职责与范围。
- 验收编号映射和可观察交付物。
- 显式依赖 DAG 与合理拆分理由。
- 每项 Task 的独立 TestRecipe、文件路径及镜像。
- 汇合、风险、阻塞与完成标准。

## 工具与证据

使用所选 Harness 的原生文件读取、搜索及获准命令工具；历史 AgentID 经验由应用按 Feature/Task 问题检索、选择和恢复后作为上下文提供；不通过 Tool 自行检索。Tool 存在不授予写权限，执行以冻结 Permission 和节点范围为准。标注真实来源与未验证项，不把推测写成事实。
