# JSON Schema 契约

所有 Schema 使用 Draft 2020-12，可离线解析相对引用，随运行包发布。

| 入口 | 当前合同 |
|---|---|
| `orchestration.schema.json` | ControlPolicy、actor、ChangeSet、typed material 和运行时变更。 |
| `rpc.schema.json` | JSON-RPC、配置修订、原生执行，以及 Workflow 运行实例和 StepExecution。 |
| `workflow.schema.json` | Workflow v5：Stage、四种执行器、显式端口、访问范围、预算与有界完成规则。 |
| `agent-task.schema.json` | `ConfiguredAgentTask`，renderer `agent-task/v5`；保存实际 Turn、Agent、输入正文、来源与摘要。 |
| `spec.schema.json` / `standard.schema.json` | Spec / Standard v3，动态内容检查项和配置化覆盖基线。 |
| `project-configuration.schema.json` / `feature-configuration.schema.json` | 项目配置与 Feature 冻结关联 v3。 |
| `manifest.schema.json` | 真实材料、会话 Snapshot 与 Artifact，不再描述旧业务 Delivery 清单。 |
| `role.schema.json` / `capability.schema.json` / `agent-input.schema.json` / `agent-composition.schema.json` | 自由 Role、能力及 Agent 组合；Role 不推导工作类型或写权限。 |
| `tool.schema.json` / `integration*.schema.json` / `tool-call.schema.json` / `skill-package.schema.json` | 平台工具、HTTP/MCP 接入、授权绑定、实际调用和 Skill 包。 |
| `permission*.schema.json` / `sandbox.schema.json` | 完整权限 profile、确切绑定、冻结权限闭包及执行后端。 |

Schema 校验数据形状。跨修订引用、作用域、依赖 DAG、Role→Agent 兼容、真实来源、权限落实与人工确认摘要由领域层和执行器核验。通过 Schema 不等于业务内容通过。

Agent 输出统一为 `status`、`summary`、`outputs`。各输出按显式 Delivery 的格式与内容 Schema 接纳；内容评审检查项来自当前 Spec，不固定为四类。可信测试和 Git 回执只能来自真实平台调用，模型 JSON 不能冒充执行证据。

只接受当前合同，不保留 `workflow-action`、`stage-answer`、`execution_recipe`、旧正式 Run 任务及旧 Proposal 业务枚举。普通讨论提案 v2 保存 `delivery_ref`、`title`、`value`、`input_refs`。

验证命令：

```bash
cargo test -p devdesk-adapters --test schema_contracts --test configured_workflows --locked
```

回归离线编译每个文件和 `$defs`，验证当前资源、Serde 默认值、实际冻结计划和运行实例、权限预设及非法输入。JSON Schema 的字符限制不能替代 Rust 对 UTF-8 字节数的精确限制。

源配置、权限归属及无持久化投影的存储规则见[源配置与执行解析](../docs/source-configuration.md)。
