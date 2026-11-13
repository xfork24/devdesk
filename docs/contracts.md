# 原语、领域对象与结构识别

代码提供**原语**和**领域对象**；workflow、agent、delivery、spec 是数据，只能由已定义的领域对象组合而成。合法性由**结构**决定，不由版本号决定——因此 workflow 等可以完全独立演化（包括将来由项目管理 agent 定义与改进），代码零改动。

## 1. 领域对象与原语（代码的职责）

- **领域对象**由 serde 结构定义（`deny_unknown_fields`）：`WorkflowDefinition`（stages/steps/executor/端口/预算/completion）、`ConfigurationRecord` / `SourceDefinition`（type + scope + 按类型允许的根级引用 + data）、`DeliveryTypeDefinition`（format + content_schema，spec_refs 属于根级引用）、`ConfiguredSpecification`（body + checks）、`PermissionProfile` 等。
- **原语**是可指派的执行与组合能力：StepExecutor 类型（agent / tool / workflow / control）、CompletionRule 算子、平台工具与控制工具注册表（`PLATFORM_HANDLERS`、`CONTROL_TOOLS`）、harness 能力要求、受支持的定义 type。
- 组合示例：建立一个开发工程师 agent（agent → capability → model/harness）→ 产出 diff delivery（DeliveryTypeDefinition）→ diff 必须符合 Diff spec（ConfiguredSpecification）。全部由领域对象间的引用表达，引擎按结构解析。

## 2. 结构识别规则（判定规则）

1. **接受 = 反序列化 + 业务规则**：一份 workflow 文档合法，当且仅当它能 decode 为 `WorkflowDefinition` 且通过业务校验（步骤数、DAG、端口存在、引用解析）。不存在"需要 Schema vN"这类版本门禁。
2. **类型分派按结构字段**：外部工具 = `integration_ref` 字段存在（区别于 `implementation: platform` / `harness.native`）；Spec = 能 decode 为 `ConfiguredSpecification`；草稿是否"当前" = 文档能否 parse 为领域对象（`configured_workflows.rs` 的 `parses` 判据）。
3. **存储记录按结构**：workflow 实例 = `steps` 数组存在（查询过滤与完整性检查均用此判据，decode 兜底）。
4. **任务合同自洽校验**：`ConfiguredAgentTask::validate` 校验外层 `renderer_version` 与 task 内层一致 + 内容摘要一致 + 来源一致——比较的是**记录自身的一致性**，不是与代码常量比对。
5. **`schema_version` / `renderer_version` 是记录性元数据**：写入时记录身份（如 `WORKFLOW_INSTANCE_SCHEMA`、`AGENT_TASK_RENDERER`、`ORCHESTRATION_RENDERER`），读取与校验路径**从不**比较版本号。字段增删的兼容边界由 serde 结构天然决定（加字段=旧代码读不了，配合"只支持新数据集、不迁移"的哲学）。

## 3. 实现身份（版本化命名，但不是数据合同）

协议、编译器与存储边界的身份字符串，命名中的 `vN` 标识实现代次：权限编译器 `permission-compiler/v2`、`{provider}-native-v2:{digest}`、`docker-worker/v1`（enforcement 契约）、SQL `user_version`、数据集标记、JSON-RPC 适配器身份（`devdesk_adapters::*/v1`、`RoutingHarness/v2`）、`github-pr/v1:indirect-merge-only`。这些不参与业务数据演进，不改语义。

## 4. 外部运行时锚（关于外部世界的事实）

运行时构建知识全部位于 `crates/adapters/src/runtime_preparation.rs`：固定版本 `CODEX_VERSION` / `DSH_VERSION`（仅作构建 pin 与展示元数据）与配方内容摘要（`default_*_recipe_digest`，覆盖容器 Dockerfile 与权限后端资产）。模型服务商与模型容量是 `resources/configuration` 中受 manifest 管理的源定义（`model_provider` / `model_catalog`），目录通过 `recipe_digest` 与 Harness 构建配方内容关联。接受性判定一律为内容摘要比对（配方摘要、镜像 sha256、冻结 profile 摘要），版本字符串从不作为门槛；换版必须显式推进（`runtime_setup.rs` 拒绝构建未冻结的配方），冻结会话按设计失效。

## 5. 出厂配置自洽

`resources/configuration` 的 manifest ↔ 文件 ↔ defaults 引用 ↔ 文件归属（md 正文与未发布 json 均不得游离）由 `scripts/py/check-defaults.py` 校验（入口 `scripts/check/check-defaults.sh`，接入 `check.sh`）——这是防止历史修订/孤儿文件重新堆积（它们仍会被 build.rs 嵌入二进制成死重）的唯一关卡；结构校验在编译期（build.rs schema），组合与引用校验在测试期（materialize）。

## 6. 证据定位与引文

`EvidenceLocation` 绑定确切 `source_ref`、`value_digest` 和定位器。可选 `quote` 必须等于定位器选中的完整文本；仅仅出现在该范围内不满足合同。空 JSON pointer 选择完整冻结输入，因此引用文档中的一句话时，应使用对应的行/字符范围，或省略可选引文。行号从 1 开始且包含末行；字符位置按 Unicode 字符从 0 开始，末端不包含在范围中。模型输出 Schema 与检查报告说明使用领域层的同一条引文说明，校验器仍拒绝范围和引文不一致的结果。
