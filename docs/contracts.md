# 原语、领域对象与结构识别

代码提供**原语**和**领域对象**；workflow、agent、delivery、spec 是数据，只能由已定义的领域对象组合而成。合法性由**结构**决定，不由版本号决定——因此 workflow 等可以完全独立演化（包括将来由项目管理 agent 定义与改进），代码零改动。

## 1. 领域对象与原语（代码的职责）

- **领域对象**由 serde 结构定义（`deny_unknown_fields`）：`WorkflowDefinition`（stages/steps/executor/端口/预算/completion）、`AgentDefinition`（role_ref + capability_ref）、`CapabilityDefinition`（harness_ref + model_ref + skills + tools）、`DeliveryTypeDefinition`（format + spec_refs + content_schema）、`ConfiguredSpecification`（body + checks）、`PermissionProfile` 等。
- **原语**是可指派的执行与组合能力：StepExecutor 类型（agent / tool / workflow / control）、CompletionRule 算子、平台工具与控制工具注册表（`PLATFORM_HANDLERS`、`CONTROL_TOOLS`）、harness 能力要求、21 种定义 kind。
- 组合示例：建立一个开发工程师 agent（AgentDefinition → capability → model/harness）→ 产出 diff delivery（DeliveryTypeDefinition）→ diff 必须符合 Diff spec（ConfiguredSpecification）。全部由领域对象间的引用表达，引擎按结构解析。

## 2. 结构识别规则（判定规则）

1. **接受 = 反序列化 + 业务规则**：一份 workflow 文档合法，当且仅当它能 decode 为 `WorkflowDefinition` 且通过业务校验（步骤数、DAG、端口存在、引用解析）。不存在"需要 Schema vN"这类版本门禁。
2. **类型分派按结构字段**：外部工具 = `integration_ref` 字段存在（区别于 `implementation: platform` / `harness.native`）；Spec = 能 decode 为 `ConfiguredSpecification`；草稿是否"当前" = 文档能否 parse 为领域对象（`configured_workflows.rs` 的 `parses` 判据）。
3. **存储记录按结构**：workflow 实例 = `steps` 数组存在（查询过滤与完整性检查均用此判据，decode 兜底）。
4. **任务合同自洽校验**：`ConfiguredAgentTask::validate` 校验外层 `renderer_version` 与 task 内层一致 + 内容摘要一致 + 来源一致——比较的是**记录自身的一致性**，不是与代码常量比对。
5. **`schema_version` / `renderer_version` 是记录性元数据**：写入时记录身份（如 `WORKFLOW_INSTANCE_SCHEMA`、`AGENT_TASK_RENDERER`、`ORCHESTRATION_RENDERER`），读取与校验路径**从不**比较版本号。字段增删的兼容边界由 serde 结构天然决定（加字段=旧代码读不了，配合"只支持新数据集、不迁移"的哲学）。

## 3. 实现身份（版本化命名，但不是数据合同）

协议、编译器与存储边界的身份字符串，命名中的 `vN` 标识实现代次：权限编译器 `permission-compiler/v2`、`{provider}-native-v2:{digest}`、`docker-worker/v1`（enforcement 契约）、SQL `user_version`、数据集标记、JSON-RPC 适配器身份（`devdesk_adapters::*/v1`、`RoutingHarness/v2`）、`github-pr/v1:indirect-merge-only`。这些不参与业务数据演进，不改语义。

## 4. 外部运行时锚（关于外部世界的事实）

`crates/domain/src/defaults.rs` 与冻结配方：`DEFAULT_CODEX_VERSION = 0.167.0`、`DEFAULT_DSH_VERSION = 0.2.2-rc.1`、recipe digest、容器 Dockerfile、`dsh-model-catalog.json`。换版必须显式推进（`runtime_setup.rs` 拒绝静默换版），冻结会话按设计失效。

## 5. 出厂配置自洽

`resources/configuration` 的 manifest ↔ 文件 ↔ defaults 引用 ↔ 文件归属（md 正文与未发布 json 均不得游离）由 `scripts/py/check-defaults.py` 校验（入口 `scripts/check/check-defaults.sh`，接入 `check.sh`）——这是防止历史修订/孤儿文件重新堆积（它们仍会被 build.rs 嵌入二进制成死重）的唯一关卡；结构校验在编译期（build.rs schema），组合与引用校验在测试期（materialize）。
