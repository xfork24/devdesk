# HTTP、RPC 与事件参考

本页用于查询接口约定。整体调用关系先读[工具和接口](../interfaces.md)。JSON 结构见 [rpc.schema.json](../../../schemas/rpc.schema.json)，方法登记和实现位置见 [rpc_contracts.json](../../../crates/core/src/rpc_contracts.json)。登记表同时包含多种入口类别，不能把所有条目视为任意客户端可调用的方法。

## HTTP 入口

| 入口 | 作用 | 关键约束 |
|---|---|---|
| `GET /connection` | 探测产品、协议和认证要求 | 可跨源探测，不发放认证会话 |
| `POST /session` | 用连接密钥建立会话 | HttpOnly、SameSite=Strict；密钥不进入 URL |
| `POST /rpc` | 请求应用命令或查询 | JSON-RPC 2.0，必须有 id；不支持 batch/notification |
| `GET /events` | 订阅事件 | 明确作用域；游标只属于相应事件流 |
| `GET /artifacts/{id}` | 读取产物 | 明确 project_id，核验摘要，不接受宿主文件路径 |
| `GET /capabilities` | 查询协议、能力和生命周期 | 能力声明不代表实际运行已通过 |
| `POST /setup` | 打开数据集 | 需要原 Host 实例和生命周期版本 |
| `POST /shutdown` | 请求停机 | 需要可信用户来源及生命周期前置条件 |

## 领域包接口

从 `GET /capabilities` 读取 `domain_protocol`。浏览器和普通 HTTP 客户端均通过 `POST /session` 建立会话。已认证客户端从 `query.domain.active` 获取当前 `epoch`、活动包、公共界面贡献和源修订。静止时 `active` 为空表示空领域。

| 方法或入口 | 作用和前置条件 |
|---|---|
| `domain.install` | 提交 Base64 ZIP `archive`，返回 ZIP SHA-256。重复安装相同 ZIP 可重放 |
| `domain.activate`、`domain.deactivate`、`domain.uninstall` | 提交 `digest` 和原 `epoch`。激活检查允许执行的包摘要。活动根流程尚未结束时返回 `domain_busy` |
| `query.domain.installed` | 列出安装的版本和摘要 |
| `query.domain.contribution` | 提交当前 `epoch`、贡献 ID 和 `input`，返回公开投影 |
| `query.domain.invoke` | 提交当前 `epoch`、声明的 `handler` 和 `input`。返回领域只读运行端的结果 |
| `domain.project.create` | 提交 `epoch`、`title` 和 `command_id`，返回绑定活动领域包的新项目 |
| `domain.workflow.start` | 提交 `epoch`、确切的 `workflow_ref` 和 `command_id`。实体工作流还需 `project_id`、`entity_id` 和 `expected_revision`。默认 `rust`，也可选 `manual`。活动目录的 `agent_workflows` 列出可由 `agent` 调度的修订 |
| `query.domain.workflow` | 按原项目、执行 ID 及当前 `epoch` 读取流程详情。结束后从 `domain_submission.entities` 回读已提交的领域对象 ID |
| `domain.entity.put` | 携带 `epoch`、`project_id`、`entity_id`、`type_ref`、`expected_revision`、`command_id` 和 `data`。新建使用修订 0。更新保留对象类型，Host 按该类型的 Schema 校验载荷 |
| `query.domain.entity` | 按 `epoch`、`project_id` 和 `entity_id` 读取对象当前修订 |
| `GET /domain-events` | 连接时收到当前代次，后续收到 `domain` 事件。客户端据此重读活动目录 |
| `GET /domain-assets/{epoch}/{digest}/{path}` | Web 读取活动包的渲染资源。当前代次和摘要都需匹配 |
| `query.core.git.head`、`query.core.git.diff` | 读取指定宿主本地仓库的 HEAD 或通过两个完整 OID 比较修订 |

包及客户端目录的结构分别见 [领域包合同](../../../schemas/domain-package.schema.json) 和 [呈现贡献合同](../../../schemas/domain-contributions.schema.json)。[`domains.rs` 调用入口](../../../crates/host/src/domains.rs)

领域项目复用 `workflow.step.dispatch`、`workflow.cancel` 等平台 RPC，并在参数中携带 `project_id` 与活动的 `domain_epoch`。领域项目的 `/events` 和 `/artifacts` 在 URL 查询中携带同样两个参数。项目事件订阅在活动代次变化后结束。领域包在 `configurations/defaults.json` 中通过 `role_agent_bindings` 绑定确切的 Role 和 Agent 修订。Workflow 声明 `orchestrator_role_ref` 并采用支持原生编排工具的 Harness 后开放 Agent 调度。

清单可通过 `entity_types` 指定包内对象类型文件。活动目录按类型修订返回 JSON Schema，实例保存在包绑定的项目库。对象 ID 使用规范 UUID。[领域对象类型合同](../../../schemas/domain-entity-types.schema.json)

对象类型的 `workflows` 按 Workflow 修订声明对象数据对应的输入端口，以及每个交付端口对应的对象类型。Host 冻结原对象修订，工作流完成时用已接纳的实际产物创建交付对象，并绑定原执行、Artifact 和适用的人工签署。完成状态和交付对象在同一个项目库事务提交。

## RPC 请求与响应

```json
{"jsonrpc":"2.0","id":1,"method":"query.workspace","params":{"project_id":"<project-id>"}}
```

工程请求显式携带 `project_id`。缺少必需项目返回 `project_required`，未知项目返回 `not_found`，不会回退到另一个项目。

通常的修改命令包括 `command_id`、`target`、`expected_revision` 和 `payload`。具体命令可能使用更细的 `preconditions`；应从对应读取结果保留原版本，而不是临时获取新版本后重发旧修改。客户端不自行指定 actor 或已经解析好的执行绑定。

资源提交成功后，响应保留原 `result` 结构，并通过 `meta.commit` 返回 namespace、versions 和 cursor。同命令同参数重放返回原结果；同命令 ID 换参数会被拒绝。

| 字段/令牌 | 含义 | 不应拿来替代 |
|---|---|---|
| expected_revision / preconditions | 本次修改依赖的资源版本 | 目录新鲜度、页面最后刷新时间 |
| catalog_version | 配置视图的新鲜度 | 执行授权或写入版本 |
| catalog_generation | 模板重置后的目录世代 | 单个业务对象版本 |
| cursor | 某个库的事件位置 | 另一个项目的事件位置 |
| plan_revision_id / control_version | 工作流计划和控制状态版本 | 单纯的 owner revision |

## 按工作查找方法

| 工作 | 主要方法或方法族 | 实现入口 |
|---|---|---|
| 初次打开页面 | query.ui_bootstrap、query.project_directory、query.workspace | [workspace](../../../crates/host/src/workspace.rs) |
| 配置目录、正文和发布 | configuration.*、对应定义创建/更新方法 | [配置服务](../../../crates/host/src/governance.rs) |
| 运行安装与权限 | runtime.*、权限资源及绑定方法 | [运行准备](../../../crates/host/src/runtime_setup.rs)、[权限](../../../crates/host/src/permission_resources.rs) |
| 会话输入与控制 | session.open、session.delete、conversation.send、conversation.delete_turn、会话及交互方法 | [会话命令](../../../crates/core/src/session_commands.rs) |
| Agent 经验 | query.agent_experience、experience.delete、experience.restore | [经验读写](../../../crates/host/src/governance.rs) |
| 工作流与经理 | workflow.*、orchestration.*、manager.* | [控制入口](../../../crates/core/src/workflow_controls.rs) |
| 执行显示 | query.workflow_progress、query.execution_workspace、query.feature_branches、流程详情查询 | [读取模型](../../../crates/core/src/read_models.rs) |
| 历史与工具调用 | query.history、query.events、query.tool_calls、query.artifact | [运行读取](../../../crates/host/src/runtime_reads.rs) |
| 模型服务与接入 | models.list、integration.*、skill.import_package | [模型目录](../../../crates/host/src/model_providers.rs)、[接入](../../../crates/host/src/integrations.rs) |

`query.orchestration` 接受项目及根流程范围。省略 `view` 时返回原完整协作视图。`current` 返回摘要、当前待处理决定和协作信息。`history` 需要 `root_instance_id`，以 `decision_before` 读取下一页，只返回已处理决定及签署与材料摘要。`decision` 需要 `root_instance_id` 和决定 `id`，核对归属后返回原决定和签署意见。历史游标独立于当前视图的待处理项。

`query.workflow_driver` 的 `view: "actions"` 接受项目、根流程 `id` 与至多 20 个 `steps`。每项提供 `instance_id`、`execution_id` 和 `step_revision`。响应只含仍可按原版本核对的待接纳或待协调步骤。版本推进后重新读取摘要可获得当前步骤。返回的操作资料仅供显示，写入仍验证提交时的原始版本。

`query.template_reset_progress` 查询当前模板重置，或通过 `command_id` 查询指定命令。全局受理、部分库完成、全部完成和可重试故障分别报告。重放原命令可以继续待应用的库。

方法完整清单及 access/scope/precondition 分类直接查登记表，工具的输入 Schema 查[工具参考](tools-and-primitives.md)。这样新增入口不需要手工复制一份容易遗漏的大全。

`query.workflow_instance` 的 `view: "reason"` 按 `project_id` 和流程实例 `id` 读取调度原因及其 `revision`。传入 `execution_id` 时，返回该步骤的完整 `reason` 和步骤版本。若有核对记录，还返回 `reconciliation_message` 和 `reconciliation_revision`。经理页和流程详情用它补全标记为截断的摘要。

`session.open` 的 `continue_from` 指定原 `session_id`，同时使用原 Session 的 `expected_revision`。指定历史轮次时同时提交 `turn_id` 和 `turn_revision`。后继 Session 从这个来源生成快照，并以独立讨论权限打开。

`session.delete` 和 `conversation.delete_turn` 的 `target` 都是 Session ID，`expected_revision` 使用原 Session 版本。轮次删除在 payload 中同时传入 `turn_id` 和 `turn_revision`。删除会影响角色讨论中的可见记录，确切事件和诊断查询仍可核对原证据。

`query.agent_experience` 接受 `offset` 和 `limit`。明细查询使用 `agent_instance_id`、`source_project_id` 和 `snapshot_id`。这三个字段也用于 `experience.delete` 的 payload，删除命令把 `snapshot_id` 作为 `target`，提交 `expected_revision: 0` 声明删除记录尚不存在。经验操作使用当前 `project_id` 或 `scope: "global"`，按选定 Agent 的可见范围核对来源。

## 生命周期与设置

`/capabilities.lifecycle` 提供 `instance_id`、`revision` 和 `status`。setup 请求携带 `command_id`、`instance_id`、`expected_revision`、`path`；shutdown 不需要 path。旧页面不能用旧实例身份停止新启动的 Host。

生命周期操作进行中可以返回 `operation_in_progress`，调用者使用原命令核对结果。打开数据集期间，工作台展示初始化状态，数据集就绪后自动进入。生命周期回执属于当前 Host 进程，与数据库中持久化业务命令的回执不同。

`query.settings` 同时返回设置值和 versions；`settings.update` 的 preconditions 只包含本次修改的 setting 资源。不同设置项分别检查版本，冲突后保留原草稿。

`global_concurrency` 默认 32，控制 Host 的全局并发额度。`default_project_concurrency` 默认 8，在创建项目时写入项目额度。两项均接受 1 至 32 的整数。已创建项目继续使用各自保存的额度。

`github.auth_status` 返回 GitHub 连接状态、账号、凭据来源及版本。`github.configure_auth` 使用该版本作为 `expected_revision`，验证并保存 `token`。两种响应均只返回状态信息。

`repository.reconcile` 核对原仓库操作。若原接入报错，且物理标记、仓库和工作副本都表明克隆尚未开始，显式核对可以沿用原项目、仓库、分支和操作身份继续接入。

## SSE 和历史

当前工作台使用 `GET /events?tail=true&configuration=true&project_id=ID`。事件 ID 为作用域与 cursor 的组合，同作用域 Last-Event-ID 可恢复窗口内事件。`stream.ready` 表示连接就绪；`stream.reset` 表示需要重新读取当前状态。

配置通知 `configuration.changed` 不带持久化事件 ID，不推进工程事件游标。普通尾流事件只是失效提示，客户端不能把收到它当作命令已被接纳。未启用 tail 的历史流与共享尾流使用不同实现，不能把尾流的资源特性套到旧接口上。

共享尾流按项目读取已提交事件。请求配置通知的连接共用 Host 的配置版本采样，采样完成后间隔 200 ms 再读，只有版本变化才发送通知。消息中的 `project` 是当前流作用域的版本，`source_epochs` 包含已打开的各项目配置版本。

`query.history` 使用 offset/limit 分页；`include_tool_calls:true` 可同时显示事件和工具调用。summary 适合列表，确切记录 ID 用于按需读取全文。列表中的省略标记不能解释为原记录正文为空。

## 错误和诊断

JSON-RPC 使用 `-32600/-32601/-32602/-32000`，领域错误代码位于 `error.data.code`。

| 代码 | 调用者应该怎么处理 |
|---|---|
| invalid_params / precondition_required | 修正参数或补齐原版本 |
| revision_conflict | 保留原草稿，读取变化后重新决定 |
| blocked / configuration_required | 查看权限、依赖或配置原因 |
| needs_reconciliation | 核对原操作，不盲目重发 |
| dataset_locked | 查明是否已有 Host 管理该数据集 |
| storage / provider_failure | 保留关联身份，检查存储或外部服务原因 |

Host 可提供 `Server-Timing` 和 `X-DevDesk-Request-Id` 关联诊断。更详细的生命周期格式见 [host-lifecycle.schema.json](../../../schemas/host-lifecycle.schema.json)。
