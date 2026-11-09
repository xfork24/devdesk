# Workflow 与 Agent 组合实施契约

日期：2026-11-09。状态：已实现。以 PLAN.md 的产品行为为验收依据。

## 配置和身份

Scope 使用带 kind 的 Global/Project/Feature 值对象。配置记录具有独立 id、definition_id、kind、scope、version、前序、内容摘要和不可变正文。草稿和运行投影可修改，已发布配置、AgentID 的组合绑定、Feature 配置锁不可修改。

AgentRevision 固定 Model、Harness、Skills、Tools、Permission、职责和指令。AgentID 固定 AgentRevision 和独立的实例 Scope。全局定义可创建全局或项目实例；项目定义仅能创建所属项目实例。工具调用、权限和实际运行环境分别记录，不把模型请求当作授权。

## 定义和运行

Workflow 草稿支持保存不完整内容；发布须校验结构、依赖、端口、角色与动作契约。发布、Project 默认换绑、Feature 创建是三个独立操作。Feature 创建时保存精确配置闭包和可执行流程；之后的 Task、Attempt、Session、评审和合入使用该锁。

步骤字段包括 key、kind、stage、dependencies、参与者 slot、输入输出、条件、推进方式、执行配方和返工范围。内核根据锁定计划及实际工程事实求值；执行意图持久化后再派发。讨论、人工决定、正式执行、Review 和 Merge 分别产生事实，模型不能直接完成流程。

## 确认和重试

人工确认绑定步骤轮次、输入/输出摘要及候选基线，位于实际动作之前的推进边界。技术重试使用原冻结请求；业务返工创建新轮次。Task 汇合固定已采纳计划的成员集合，输入变化精确失效下游，已完成历史保留。

## 展示和证据

工作流程页面编辑定义、草稿和版本；执行与交付展示 Feature/Task 的运行进度、文档、上下文、测试及交付。角色讨论选择完整 Agent 组合和 AgentID，按项目名称、Feature 名称和时间读取分层经验。

本轮和历史计划目录仅保留 Markdown 计划、契约和执行报告。源码回归测试保留在 crates、web/tests 和 tests，临时验收工作目录及日志不作为交付物。

## 组合与步骤样例

以下引用表示精确修订 ID；它们不是按名称查找的运行偏好。

```json
{
  "role": "developer",
  "instructions": "在冻结 Task 范围内执行，并遵循 TDD 和独立评审",
  "model_ref": "model-revision-id",
  "harness_ref": "harness-revision-id",
  "skills": ["skill-revision-id"],
  "tools": ["native-tool-revision-id", "experience-tool-revision-id"],
  "permissions": ["harness-permission-revision-id", "sandbox-permission-revision-id"],
  "approval": "never"
}
```

AgentInstance 正文为 `{"agent_revision_id":"agent-revision-id","scope":{"kind":"global"},"source_instance_id":null}`。同一全局定义也可以创建 `scope.kind=project` 的独立实例。

默认 Feature 流程中，主干候选 Review 步骤使用 `advance_mode=human_confirmation`。确认仅允许后继 merge.apply 派发；确认输入摘要包括本轮产物及候选/基线。输入端口使用 `{"merge":{"step":"merge_review","port":"merge"}}` 显式选择前置产物，不能引用依赖链外对象。

正式结构见 [Workflow Schema](../../../schemas/workflow.schema.json)、[Agent Schema](../../../schemas/agent-composition.schema.json)、[Model Schema](../../../schemas/model.schema.json)、[Tool Schema](../../../schemas/tool.schema.json) 和 [动作契约](../../../schemas/workflow-action.schema.json)。图、职责、TDD、Review 顺序及 Scope 的业务校验由领域编译器负责。

## 存储与状态机

项目迁移 034、全局迁移 005 追加 governance_records；记录包含身份、Scope、版本、摘要及 JSON 正文。SQL 触发器拒绝不可变记录 UPDATE、全部 DELETE 和身份/实例绑定替换；唯一索引约束发布版本及 Feature 锁。可变记录写入前重新计算摘要并增加 revision。

草稿生命周期为创建 → revision 编辑 → 校验/预演 → 发布引用；发布后只能派生新草稿。Project 配置单独追加修订，Feature 创建事务解析依赖闭包并保存编译器 v1 计划。

已 completed / cancelled 的运行实例只读；新工作创建后继 Feature，不复活原流程。

运行步骤具有 pending / waiting_dependencies / waiting_condition / active / waiting_input / waiting_confirmation / completed / skipped / blocked / failed 状态。求值先持久化 workflow_intent；Host 派发正式命令后记录结果并重新求值。技术重试保存 workflow_retry，业务返工保存原 workflow_step_history 并产生新 step ID / round。未知外部结果先核对原操作。

远端主干变化时，显式 repository.refresh 意图只允许干净主干快进。接纳依据失效后从候选准备返工，不强推旧基线，不把旧确认带到新候选。

## 页面与接口边界

工作流程页面按“版本列表 → 阶段及步骤预览 → 所选步骤表单”组织；发布版只读，Project 绑定位于独立表单。运行视图在执行与交付，显示进度、等待理由、证据引用及 core 给出的允许动作。组合管理在规范与身份；角色讨论显示完整组合，历史按 AgentID 展开。

API 字段、筛选、幂等及运行控制详见 [协议](../../protocol.md)。定义管理不依赖 GitHub 或 Docker；真正运行才核对原生安装及组合能力。当前有限 DSL、五种规范 slot 和 Feature→Task 子流程边界见 EXECUTION.md。
