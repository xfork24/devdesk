# 本地协议 v1

只监听 127.0.0.1。host 启动时生成随机本地会话；同源页面通过 HttpOnly、SameSite=Strict Cookie 使用。校验 Host 和有值的 Origin，缺失或错误凭据拒绝 RPC/SSE/Artifact。用户 human:local 身份由 host 装配，载荷中的 actor 无效。

## HTTP

- POST /rpc：JSON-RPC 2.0，id 必填。查询 params 为过滤参数；命令 params 为 Command。
- GET /events?project_id=ID&cursor=N：SSE，连接固定到该活动项目；事件 ID 为 `ID:cursor`，cursor 在项目内持久化单调递增。支持同项目 Last-Event-ID；其他项目的游标忽略。分批补读，断线后重连；事件只是事实通知，页面重新查询。
- GET /artifacts/{id}?project_id=ID：按活动项目数据库中的 ID 读取，路径不得由客户端提供；SHA256 校验后返回纯文本并禁止内容嗅探。
- GET /capabilities：协议、版本、原生供应商和 Docker 能力与配置/Archive preview/migration preview/platform validation 状态。
- POST /setup：未打开数据集时设置 path；非空非 DevDesk 目录拒绝。打开后不能热切换。
- POST /shutdown：可信人类会话退出 host。

```json
{"jsonrpc":"2.0","id":1,"method":"attempt.start","params":{"command_id":"uuid","target":"feature-id","expected_revision":3,"payload":{"phase":"planning","provider":"codex"}}}
```

命令：project.create/open/update、feature.create、document.revise、definition.revise、binding.update、task.create、attempt.start/pause/cancel、run.cancel/retry、delivery.submit、review.submit/request、override.accept、merge.request/apply、context.update、snapshot.save。不存在 set_status。

`project.open` 使用 Command，target 为项目 ID，payload 可为空；成功后切换活动数据库并保存上次选择。`project.create` 创建独立数据库并自动打开。工程查询和 Command 可在 params 顶层携带 `project_id`，与当前活动项目不符时返回 `project_changed`；不通过查询隐式切换项目。省略作用域时使用请求开始时的活动数据库。没有活动项目时工程命令返回 `project_required`，仍可查询全局工作区和配置、修改并发配置或修订共享定义。

target 为有 revision 对象时 expected_revision 必填；command_id 相同参数返回首次结果（先于修订检查），不同参数复用拒绝。长操作返回 operation_id。错误不写入幂等结果，可修正后重试。

查询：query.workspace（project_id/feature_id/offset/limit）、query.history（project_id/target/offset/limit）、query.events（project_id/cursor/limit）、query.artifact（project_id/id）。limit 默认 50，上限 100；Feature detail 返回有界历史。allowed_actions、blocked_reasons 来自 Rust，界面不实现状态机。

query.workspace 返回全局项目目录，以及仅属于活动项目的 features/detail；query.history 只读取活动项目的历史。工作区附 `active_project_id`、`active_project`、`global_database`、`project_database` 和 `project_data_root`；`data_root` 仍为全局 data 根目录。`settings.update({global_concurrency:N})` 写入全局 SQLite（1–32），`definition.revise` 也作用于共享配置库。工程命令幂等记录按项目隔离；项目创建、打开及共享配置命令的幂等范围为全局配置库。

错误 result 包含 code/message：invalid_params、not_found、blocked、revision_conflict、project_changed、project_required、method_not_found、storage、dataset_locked。JSON-RPC error.code 使用 -32600/-32601/-32602/-32000，domain code 在 data 中。

SSE 和轮询使用同一个数据库事实源。所有命令在 SQLite 短事务内校验和提交；单写协调，状态与事件提交后才可见。原生供应商事件先核对准入、消费与终止证明，再由 Core 决定业务终态。


## 分页与冻结执行输入

query.workspace 的 project_offset 与 Feature offset 独立；detail_pages 对 tasks/attempts/runs/deliveries/merges/documents/snapshots/artifacts/effects 指定页码（从0起）。每页默认50，Runs每页为5倍；detail.totals 为各集合数量。翻 Attempts/Merges 页时客户端将 runs 页码归零。事件与日志报告文件分段读取，不加载全部工程历史到浏览器。

Standard 的 slots 结构为 `[{"purpose":"planning","spec":"固定Spec-ID"}, ...]`，同时具备 planning/implementation/merge 三个用途。Spec 仅解释内置 traceability、failure_paths、sections 规则；无任意脚本。定义版本、适用标准与实际逐项结果都留在 Review 历史。

Attempt 除文档与绑定外冻结 Context 内容与 revision。Harness 的执行输入包含冻结 Context 和继承 Snapshot；Reviewer 输入为目标 Delivery 的冻结文档、证据和报告正文。TestRequest 必须包含 operation_id、phase、candidate、test_definitions、environment、origin，可附 session_id、完整 definitions、code 和 runtime，适配器结果必须回传匹配的绑定。

Delivery 文件清单及工程 Snapshot 清单含 `schema_version=1`；Project 上下文和会话输入/交接快照直接序列化 `Snapshot`，没有清单外层。上述格式均由 `schemas/manifest.schema.json` 描述，封存并记录哈希，不成为可写工程事实源。原生交付使用 `origin=external`，`simulated` 仅供历史读取。各 Schema 的对应实现与校验边界见 [Schema 说明](../schemas/README.md)。JSON-RPC 不实现 batch/notification，查询与命令均要求 request id；无参数查询可省略 params，Command.payload 省略时为 null。

实现参考：[Axum SSE](https://docs.rs/axum/0.8.9/axum/response/sse/)、[Cargo Workspaces](https://doc.rust-lang.org/stable/cargo/reference/workspaces.html)。

## 领域补强协议

- document.revise 只接受 Feature target，必须携带该 Feature 的 expected_revision；payload.feature_id 不能作为替代入口。
- Requirement 可附 acceptance=[{key,description}]；Test 可附 verifies=[key]。Web 使用每行“标识 | 条件”和逗号列表录入。省略时分别登记完整正文为 AC-1、继承关联需求的全部条目。
- task.create 可附 scope={requirement,design,acceptance} 或 acceptance_keys。默认从已接受的初始 Snapshot 解析。task.replan(target=Task, expected_revision) 采用当前已接受规划及选定条目，新测试修订保留 supersedes。
- query.runtimes 返回 Harness/Sandbox 定义；query.archive_preview(feature_id) 返回产物/快照范围与不可执行理由。注册表提供原生定义和已登记的真实运行时，旧定义只作历史引用。
- Run 返回不可变 binding；Session 返回 instance；Review 返回 assignment；Snapshot 返回 capture；Attempt 返回 baseline。
- 数据库支持从 v1 顺序升级至 v33（全局库 v4）。旧文档缺少结构化验收映射时保持空值，旧交付历史可读；重新参与增强门禁须明确修订和验收，不能将迁移当成人类复核。

历史 Run 的 binding_verified=false，保留可从 Session 确定的身份，但不把无法证明的完整绑定用于技术重试；需要新建执行。新 Run 的 binding_verified=true。

## 持久化外部操作

- query.workspace.detail.effects 分页返回外部操作；detail.advances 返回当前 Feature 的有界目标推进记录。操作包含适配器身份、请求、回执、状态、revision 与理由；Web 提供查看回执，原生执行恢复通过 Session/RuntimeExecution 与 merge.reconcile 核对。
- merge.apply(target=Merge, expected_revision) 返回 {id,operation_id,accepted:true}，表示意图已保存；是否完成以查询的 Merge/owner 状态为准。供应商调用由调度器负责。
- merge.reconcile(target=Merge, expected_revision) 请求核对 needs_reconciliation 的推进。确认未推进后，旧候选 stale，重新 merge.request、验证与 Review。
- Attempt 另返回 outputs 和 scope；Harness 输入明确区分 frozen_inputs / stage_outputs，附锁定定义和运行实例。持久化回执不是批准命令；核心仍核对版本、身份、来源及门禁。
- 新表 effects、advances 与业务事件使用同一 SQLite 事实源。部分副作用的意图、调用、回执和投影分开提交；不能把这个协议解释为跨供应商原子事务。

GitContext 随 Attempt.git_context 和 MergeAdvance.context 返回；它包含冻结的 Workspace 与源/目标 owner、branch、worktree。Harness 输入的 code、code_baseline 和 TestRequest 的 definitions/code/runtime 均来自锁定版本，避免适配器查找隐含的全局目录。旧定位为 null，需要新建执行。

## 真实接入基础接口

全局配置库 v2 追加 `runtime_installations`。`query.runtime_installations` 返回安装引用和 `default_harness`；`query.workspace` 同步附带这两个字段。`settings.update` 可原子更新 `default_harness: codex|dsh`、`global_concurrency: 1..32`，至少一个；已有 Session/Run 锁不因偏好变化而改变。默认偏好初始为 Codex，历史原型项目只读保留，不再进入调度。

- `runtime.register({key:{provider,version,os,arch},artifact,source,license})`：登记原生 executable（canonical_path+sha256）或以 digest 固定的 oci_image 引用。同键同内容返回原 ID；内容冲突拒绝。原生文件检查 header/platform、执行权限和摘要；脚本入口不等于完整安装，不接受作为完整运行时。登记不下载、不执行工具、不复制或删除共享安装。
- `runtime.discover_codex({})`：显式发现 PATH 中本机原生 Codex，校验格式并查询版本，登记一次。不调用模型。
- `runtime.integrity({id})`：重新校验文件摘要；镜像只登记引用时明确标记尚未 Docker 核验。`inference_verified` 不因摘要正确而变 true。
- `credentials.configure({provider:"dsh",api_key})`：macOS Keychain 写入；普通返回只含 configured、reference、storage。`credentials.status({})` 查询 DSH 状态。秘密不写 SQL、事件、日志或返回对象；Linux 当前不提供明文文件 fallback。
- `codex.auth.start({installation_id,device?:boolean})`：官方 account/login/start；默认浏览器 ChatGPT OAuth，返回临时授权 URL 与 loginId；可选官方 device-code 流程。认证进程仅发送账户方法，不运行模型或工具。使用 data/credentials/codex 的独立私有目录，Codex 自行管理令牌。不是读取／复制宿主桌面凭据。
- `codex.auth.status({installation_id})`：只返回 authenticated/type/pending，不返回账户个人信息或令牌。
- `codex.auth.cancel({installation_id,login_id})`：仅取消当前认证实例的原 login 请求。

这些配置/认证接口使用专用参数而非工程 Command；安装的幂等范围为全局安装唯一键，认证派发未知时拒绝盲目重复登录。真实工程会话、消息、提案与执行 API 尚在实施中，不能据此认为 R3 已通过。

### Persistent role conversations (integration work in progress)

`session.open` targets a Project/Feature/Task at its current revision. Its payload selects `agent` (immutable role revision), optional `provider`, `installation_id`, `model`, `work_type`, `snapshot_id`, and `predecessor`. Clients cannot supply a resolved `binding`. The host resolves defaults once and freezes the installation, image digest, adapter, model, tool policy, role and credential reference. Replaying the original command returns its original result before resolving current defaults.

`conversation.send` targets the Session revision with `input` and `client_message_id`. A repeated client message ID with the same input returns the original Turn; changed input is rejected. One active or uncertain Turn blocks another input. `conversation.cancel` targets a Turn revision and records intent; a dispatched Turn only becomes cancelled after owned-container stop proof. Restart marks dispatched work `needs_reconciliation`, stops the previous writer and never resends its input. Explicit cancellation retires uncertain work; continuing uses a successor Session with a handoff Snapshot.

`query.sessions` accepts `owner_id`, `offset`, `limit`. `query.session` accepts `id`, an event `cursor` (exclusive), turn `offset`, and `limit` (maximum 100), returning the frozen Session, Turns, ordered events and next cursor, interactions, proposals, and execution history. Event `mode=delta` appends to `provider_item`; `mode=full` replaces that item's displayed content. Stored events remain append-only.

`interaction.answer` targets the request revision, requiring `session_id`, `turn_id`, and typed `answer` (`tool_approval` or `user_questions`). Runtime permission approval, clarifying questions, Proposal adoption and formal Delivery Review are separate decisions.

`proposal.extract` reads the last complete assistant JSON from a completed `turn_id`. `proposal.create` accepts human-edited `content` conforming to `ProposalContent`; both require an explicit source Turn and resolve a `target` and `expected_revision`. `proposal.revise` creates a new immutable draft revision. `proposal.adopt`/`proposal.reject` target the Proposal revision. Adoption is atomic, validates current target and immutable input documents, and invokes the existing formal object commands. `proposal.instantiate` materializes an accepted TaskPlan's stable keys and dependencies after the existing planning Review/Snapshot gate; the key-to-Task mapping is persisted and cannot be replaced. Proposal acceptance does not accept a Delivery or a Merge.

Project-owned Requirement/Artifact/Snapshot records use `project_id` with NULL `feature_id`; Feature/Task materials use the inverse ownership. Feature `source_requirement` pins a same-project Requirement revision. Runtime installations, role definitions and Permission revisions live in global schema v5; engineering records and conversation projections currently use project schema v34. Sequential SQL upgrades preserve historical provenance and never fabricate native IDs. Historical prototype projects are read-only; active execution is native-only.

## 真实工程操作与证据

- `repository.connect` 登记专用仓库；`query.repository` 返回 Project、worktrees、operations。`repository.reconcile` 只核对原操作，不新建替代副作用。
- `execution.retry_cleanup` 仅允许已停止、有协议 Artifact 且清理失败的 RuntimeExecution。历史失败事件保留，共享镜像不删除。
- `merge.capture_snapshot` 为已完成接纳追加审计快照，原快照不改写；`merge.apply` 返回意图受理，不等同远端交付完成。
- `RuntimeExecution` 返回 `kind/operation_id/session_id/run_id/image_digest/container_id/status/writer_revoked/protocol_artifact`。停止方式保存为结构化 status 事件（原生 interrupt、forced stop、恢复停止等）。
- `Delivery.source_deliveries` 是不可变来源链；原生 Reviewer 冻结输入包含递归交付、文档、测试、评审、远端回执与固定版本 diff。
- 普通 `conversation.send` 输入最多 64 KiB；正式 Run 由 Host/Core 生成的完整证据包最多 512 KiB。工程 Session 禁止前端伪造工程输入。派发前契约错误只记一次失败；未知派发结果保留待核对。
- `PendingInteraction.questions` 附 `detail/multi_select/intent`；回答使用类型化 `answer`，精确包含 Session/Turn。凭据与持久权限请求不通过普通问题卡片批准。

schema 与字段定义见仓库 `schemas/rpc.schema.json`；真实安装、凭据与运行方式见 [接入说明](real-engineering.md)。

## 系统凭据与 Web 平台

`credentials.status` 返回配置元数据、storage、available 与不透明引用，不返回秘密。`credentials.check` 在当前 Host 验证引用可读，只返回 authorized/storage；macOS 后台访问禁止交互 UI。`credentials.authorize` 是显式系统授权入口，由固定共享凭据助手执行（校验对应签名 Host），最长 30 秒；授权拒绝/超时返回错误，不要求通过 RPC 提交系统密码。Linux 通过 Secret Service/secret-tool 私有 stdin 保存，状态页不解锁读取。


## GitHub 项目入口与按需运行时

正常 Web 使用 `project.open_github`（`Command` 参数）：首次传 `payload.repository`、可选 `payload.mainline`（默认 `main`）及可选 `payload.title`；再次打开可传 `target`。仓库支持 `owner/repo`、GitHub HTTPS 和 SSH URL。相同仓库/分支复用项目；接入意图、回执与核对沿用 `repository.connect` / `query.repository` / `repository.reconcile`。该接口返回项目 ID 后，克隆在调度器执行；只有真实仓库回执被接受后才可执行工程工作。

`query.workspace` 的 `github_only=true` 在分页前过滤后端历史项目。`project.create` 保留为后端项目元数据创建入口，新项目固定为 live；`project.open` 可只读打开历史项目。模拟场景参数已拒绝，Web 使用 `project.open_github`。

`session.open`、真实 `attempt.start` / `review.request` / `merge.request` 在解析实际供应商后自动准备所需运行时；无显式固定安装时使用内嵌配方构建并复用共享镜像。`runtime.preparation_status` 返回 `{preparations:[{provider,status,message,code?}]}`，状态为 `preparing` / `ready` / `failed`。`runtime.prepare`（`{provider}`）可显式预热/重试，正常 Web 不要求此步骤。准备期间不持有工程数据库事务，准备失败不生成成功安装。

`codex.auth.start` / `status` / `models` / `cancel` 的 `installation_id` 改为可选；省略时自动准备版本固定的本机登录组件。首次准备可能需要下载与构建，相关 Web 请求允许较长超时并轮询进度。运行中和技术重试仍使用原冻结绑定，不自动更换安装。

## Permission 配置与预检

`query.permissions` 返回不可变权限修订、带 revision 的全局绑定、当前角色修订和后端能力。`permission.create`、`permission.revise`、`permission.bind` 使用 Command；修订和绑定要求 expected_revision，相同 command_id/参数重放原结果。权限修订不能原地修改，绑定更新只影响后续会话。

```json
{"jsonrpc":"2.0","id":1,"method":"permission.preflight","params":{"provider":"codex","overrides":{"harness":{"overrides":{"filesystem":{"mode":"read_only"}}}}}}
```

预检返回 `resolved` 和 `configuration_digest`，不调用模型、不启动容器。可携带明确的项目作用域及拟用 configuration；结果包含标准权限、覆盖来源、原生选项和执行后端。错误 `permission_unsupported` 表示规则缺少执行器，`permission_conflict` 表示配置与必要运行条件冲突，`permission_denied` 表示实际访问被拒绝；保留后端原始错误分类。

`project.update` 的 payload 可包含 `permission_overrides`；Agent profile 同名字段通过 `agent.revise` 创建新角色修订。`session.open` payload 支持本次 `permission_overrides`，客户端不能提供解析后的 binding。字段含义见 `schemas/permission.schema.json` 和 `schemas/rpc.schema.json`。

`query.session` 的 RuntimeBinding 与各 RuntimeExecution 返回冻结权限；旧历史的缺失值保持 null，不伪造既有执行保证。独立测试的策略明确标记无模型连接，Sandbox 禁网与 Harness 工具禁网不会混为一谈。

## v0.3 Workflow 与 Agent 组合接口

仍使用 JSON-RPC v1 和 Command 信封。配置的 id 是精确修订 ID，definition_id 是跨修订稳定身份；version 不等于用于 CAS 的 revision。Scope 为 `{kind:"global"}` 或 `{kind:"project",project_id}`；Feature Scope 由内核建立锁和运行事实。

| 接口 | 主要参数及结果 |
|---|---|
| model/tool/harness/skill/spec/standard/agent.create | payload.name/scope/data；返回不可变 ConfigurationRecord。Agent 的 data 固定 model_ref/harness_ref/skills/tools/permissions、职责及指令 |
| 上述类型 .revise | target 为旧修订，expected_revision；追加新版本，旧 AgentID 不变。agent 的旧无 data 接口仅为兼容 |
| agent.instance.create | payload.agent_revision_id/name/scope；返回固定组合及经验范围的 AgentID |
| configuration.retire | 精确修订 target、expected_revision、payload.reason；停止新绑定，保留历史 |
| workflow.create | payload.name/scope；template_ref 或 document；返回可编辑草稿 |
| workflow.draft.create | base_revision_id，可选新 Scope/name；同范围编辑形成同定义的新草稿，跨范围为派生定义 |
| workflow.draft.update | 草稿 target、expected_revision、payload.document；冲突不覆盖本地内容，已发布草稿不能再编辑 |
| workflow.publish | 草稿 target 及 revision；校验并追加不可变 Workflow 修订，幂等返回原结果，不自动换绑 Project |
| project.configuration.update | Project target/revision，workflow_ref、standard_ref、agent_refs、可选 source_configuration_id；追加默认绑定，只影响后续 Feature |
| feature.create | 指定 project_id/title，可选 configuration_revision_id/predecessor；live Project 必须有默认配置；Feature 与配置锁原子提交 |
| workflow.pause / resume | 运行实例 target、revision；控制后续调度 |
| workflow.step.confirm | 步骤实例 target、revision 和 input_hash；结果齐备且确认依据仍有效才推进 |
| workflow.step.retry / rework | 步骤实例 target/revision；retry 受技术次数限制，rework 可指定 from_step 并生成新轮次 |
| experience.restore | 当前 owner target/revision，agent_instance_id、snapshot_id、source_project_id；验证完整性，导入有来源上下文并开启新 Session |

`query.configurations`、`query.agent_instances`、`query.workflows` 支持 offset/limit 和 Scope；活动项目查询使用 local:true/project_id，可见 Global 发布版本及本项目定义。`query.workflow`、`query.workflow_draft`、`query.agent_revision` 按精确 id 读取，不依赖列表当前页。

`query.workflow_validation` / `query.workflow_preview` 接收 document 或 id，返回 valid/errors、document_hash、顺序及演练状态，不调用模型、Docker、Git、测试或 Merge。sample.outputs 和 sample.accepted 表示演练事实；通过预演不代表真实证据成立。动作及 DSL 结构见 schemas/rpc.schema.json，当前契约和 compiler_version 均为 1。

`query.workflow_instance` / `query.workflow_actions` 返回运行记录、冻结计划/配置及可用动作。运行实例 ID、步骤 key、步骤实例 ID 和 round 分别表示不同概念；旧 round 的确认不能用于新 round。

`query.agent_experience` 必须携带 agent_instance_id；可组合 project_name、feature_name、from/to（UTC 毫秒，边界包含）、offset/limit。带 snapshot_id/source_project_id 才读取具体快照和证据引用。目录保留工作时间及捕获时间、历史名称、稳定 ID、实际组合和可用性；正文哈希失败不返回假成功。跨项目读取不切换活动工程或启动源 worker。

完整 Agent 会话只提交 agent 或 agent_instance_id，不允许附加独立 model/provider/installation_id/permission_overrides。Host 解析精确安装、组合和权限并写 RuntimeBinding.agent_configuration；不支持的映射在派发前拒绝。

query.workspace 增加项目 summary、配置锁和流程实例投影；work_filter 可选择 tasks/running/waiting_review/waiting_confirmation/blocked，task_id 可定位分页外 Task。地址和前端筛选不是业务权限；直接 RPC 同样经过内核门禁。query.history 可通过 scope:"global" 读取全局配置与发布事件，默认工程历史仍按活动项目隔离。

`repository.refresh` 使用 Project target/revision 和 payload.revision（已观察的远端精确 SHA）。命令只写 RefreshMainline 意图；适配器核对仓库、所属干净 worktree、旧本地头及远端 SHA 后执行 `merge --ff-only`，回执才更新主干事实。存在未核对的接纳或在途仓库操作时拒绝同步。旧主干候选及其确认失效，需从候选准备返工；旧 operation 保留并标记 superseded_target。`query.workspace.detail.mainline_sync` 返回近期同步状态，失败可按原 ID 使用 repository.reconcile，不重新解释旧候选。


## Role 与默认套件

- `query.default_configuration`：当前默认套件修订、Role/Agent/Model/Harness/Workflow/Spec/Standard/Skills/Tools、Permission/Sandbox 及已物化安装；无需 Project 或 Docker。
- `runtime.prepare_defaults`：按冻结的 Codex 配方准备并核验本机环境，首次保存不可变 harness_materialization；重复调用复用原安装，原镜像缺失时拒绝换绑。
- `role.create` / `role.revise`：命令 envelope；payload.name、scope、data.execution_role、data.instructions；revise 使用当前 Role 修订 target/revision。自动创建对应默认 Skill、Agent 和 AgentID。角色编辑返回新版本，旧引用不改写。
- `role.delete`：Role target/revision，可选 reason；删除保存停用记录并隐藏整条定义的修订。若当前默认流程采用该角色，需要有效的同执行职责全局替代角色。
- `query.roles`：local/project_id、offset/limit；默认最新未删除版本，可用 include_history/include_deleted 查看历史。

Agent 新建可提供 role_ref，省略模型与能力字段时填入 Sol High / Codex 默认组合；legacy role 指执行职责，缺少 role_ref 时解析当前默认 Role。直接 Session 不得单独覆盖 Role 修订，必须采用完整 Agent。Role 职责与 Agent 额外指令一起冻结到角色配置。

`query.workspace.repository_ready` 只在已持久化当前项目的主干 worktree 时为 true；`execution_mode=live` 不代表仓库接入已完成。Web 根据该事实阻止接入中或失败项目的工程操作。
