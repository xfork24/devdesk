# 本地协议 v1

## Workflow Step 任务合同

新建默认 Workflow 使用 Schema/编译器 v3。`agent.interact`、`attempt.execute`、`delivery.review`、`merge.review` 使用动作合同 v2，要求非空 `task_instructions`；其余动作保持合同 v1 且拒绝 Agent 任务字段。正式配方的六类 Agent 节点另有必填任务，测试执行器节点不接受任务。`merge.prepare` 是系统执行，Role/Spec 仍有身份和验收用途。旧 Workflow Schema、compiler 和任务渲染器均被拒绝。

任务是无插值 Markdown：单字段最多 32 KiB UTF-8，展开任务总量最多 1 MiB，冻结任务与基础 Role/Agent/Skill 上下文合计最多 512 KiB；超限拒绝，不截断任务。v3 的旧 `interaction.goal` 必须为空。`workflow.task_upgrade` 使用草稿 target/revision 显式创建 v3 草稿，复制旧讨论 goal、映射已知阶段任务；未知映射返回 diagnostics 和待补草稿，不能发布缺任务的流程。原修订、项目采用和已有 Feature 锁不变。

`query.workflow_task_preview` 接收 document 或确切 id、完整 path，可选 recipe_node、agent_revision_id、project_id。传 project_id 时只读取该活动项目的绑定，避免跨项目读取。返回公共/节点任务、阶段、可解析的 Role/Agent/Skill/Spec、输出合同和具体缺口；scope_checked=false。无实际运行授权，不安装运行时或调用模型。`query.execution_task` 必须指定 turn_id 或 run_id 之一，并可固定 project_id；返回持久化 FrozenAgentTask、摘要和来源。尚未准备输入的 Run 返回 task=null/status=not_prepared/executor；旧轮次返回 legacy=true。

Feature 锁的 specification_snapshot 带版本化 WorkflowTaskDefinition。步骤启动产生不可变 execution_task_binding；接受输入或正式 Run 入队产生 execution_task 及 agent-task Artifact。每轮冻结来源包括 Workflow 修订、完整路径、步骤实例、round、配方节点、Run/Turn，以及当时输入和授权。派发使用此快照，Role/Agent 有效正文及有序 Skill 只装配一次；Codex 和 DSH 编码同一 rendered_input。快照摘要、正文与闭包的对应关系均校验。普通 conversation.send 拒绝客户端伪造任务字段。

后续用户输入沿用该步骤任务定义；不同 v3 步骤默认独立 Session。技术重试保留原任务与业务输入，只更新执行身份。权限后继复制任务/Spec 关联并重新冻结权限，活动步骤的 session 输出跟随后继；旧执行记录不变。发布新 Workflow 只影响显式新采用的配置，已有 Feature 不在线改绑。任务不能扩大 Permission、执行范围或绕过真实测试、独立评审与合入门禁。

## Permission 运行时资源

`query.permission_resources` 返回 v2 profiles、绑定修订、确切目标版本、活动分配与来源。可携带 project_id 读取当前项目可见资源；省略时只读取全局目录。v1 权限接口、预设与覆盖字段均已移除。

新增命令均使用 Command：`permission.profile.create/revise` 的 payload 为 identity（创建时）、name、target_kind、policy；revise 的 target 是原修订 ID，expected_revision 为原版本。`permission.profile.import` 接受 payload.profiles，批次原子；`permission.profile.reload` 重新读取登记的预设来源。`permission.source.register` 接受 payload.path，必须为存在的普通绝对目录；注册后需显式重新加载。

`permission.profile.export({id})` 返回 `{profiles:[前序修订…,所选修订]}`，可直接重新导入。导入只接受 v2 profile；policy 的 null 和未知字段拒绝。

`permission.bindings.apply` 的 payload.bindings 为非空数组，每项包含 target={kind,revision_id,content_hash}、permission_revision_id、expected_revision（该目标尚无分配时为 0）。执行合同由 Host 生成，客户端不能决定。一个批次必须属于同一全局或项目配置库作用域；全部校验通过后原子提交。返回各目标的新活动 revision 与 binding_revision_id；command_id 重放优先于版本校验。

`permission.binding.preflight({target,permission_revision_id})` 只检查绑定兼容性，返回 scope_checked=false。完整预览输入 harness_ref、selection={sandbox_ref,harness,sandbox} 和可选 scope={work_type,writable_paths,read_only_paths}；selection 每层为 `{mode:"follow_target"}` 或 `{mode:"pinned",binding_revision_id}`。提供 Scope 时复用实际执行编译器；无 Scope 时不承诺具体工作范围可执行。

AgentComposition 可携带 permission_selection，以命名字段替代旧 permissions 数组的执行权威。RuntimeBinding.permissions.binding_lock 保存两层确切 profile、绑定、目标、摘要和执行合同。旧对象不补写这些字段。

`conversation.send` 对追随目标的讨论在接受新输入前检测改绑，无活动工作时持久化切换并创建后继 Session。返回值可增加 effective_session_id、predecessor，UI 跟随有效 Session；原 command_id 和 client_message_id 保持幂等。query.session/session.read 增加 permission_status，包含 frozen、desired、status、reason、pending_command；待恢复时可用 pending_command 原样重试 conversation.send。已有运行、排队输入、工程冻结与技术重试保留原权限，目标默认更新不改写它们。

默认监听 127.0.0.1，可用 `--host IP` 显式监听其他地址；非 loopback 监听或配置 `--public-url` 时必须设置 `DEVDESK_ACCESS_TOKEN`（32–512 位非空白 ASCII 连接密钥）。内核启动时生成随机会话；默认本机页面自动建立会话，受密钥保护的内核只在 `POST /session` 验证通过后发放 HttpOnly、SameSite=Strict Cookie，公开地址为 HTTPS 时附带 Secure。校验连接地址对应的 Host 和有值的 Origin，缺失或错误会话拒绝 RPC/SSE/Artifact。用户 human:local 身份由 host 装配，载荷中的 actor 无效。

Web 连接其他内核时，先以无凭据请求探测 `/connection`，确认产品及协议后导航到目标 origin，清除原地址中的 Project/Feature 等路由引用。RPC、SSE、产物和 OAuth 回调始终同源，目标内核自行建立会话；会话 Cookie 名按公开端口区分，避免同一主机不同端口相互覆盖。浏览器仅记住目标地址，连接密钥不进入 URL 或持久存储。退出当前内核后，已加载的界面保留连接入口。未来原生客户端可复用探测与会话接口。

## HTTP

- GET /connection：无敏感信息的连接探测，仅返回 product=devdesk、protocol_version 和 authentication_required。此 GET 允许跨源读取，不发放会话，不返回数据集、账号或项目内容；其余接口不开放跨源访问。
- POST /session：JSON `{access_token:"连接密钥"}` 验证并建立同源会话；默认未配置密钥的本机内核接受空对象刷新会话。错误密钥返回 401，错误 Host/Origin 返回 403。
- POST /rpc：JSON-RPC 2.0，id 必填。查询 params 为过滤参数；命令 params 为 Command。
- GET /events?project_id=ID&cursor=N：SSE，连接固定到该活动项目；事件 ID 为 `ID:cursor`，cursor 在项目内持久化单调递增。支持同项目 Last-Event-ID；其他项目的游标忽略。分批补读，断线后重连；事件只是事实通知，页面重新查询。
- GET /artifacts/{id}?project_id=ID：按活动项目数据库中的 ID 读取，路径不得由客户端提供；SHA256 校验后返回纯文本并禁止内容嗅探。
- GET /capabilities：协议、版本、原生供应商和 Docker 能力、数据集配置与平台验证状态。
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

Workflow v3 使用 `Step.role_ref`（稳定 Role 身份）和 `spec_refs`（精确 Spec 修订）。项目配置 `role_agent_bindings` 固定每个 Role 的单个 AgentRevision；Agent 输入引用 RoleRevision/CapabilityRevision，服务端展开实际组件并拒绝不一致覆盖。`standard_ref` 可为 null；Standard v2 的 `baseline_rules` 使用动作选择器、必需 Spec 及非零 `min_matches` 校验覆盖，不改变 Step。逐 Step 审计快照及实际执行/评审保留 Spec 来源、检查方式和输出契约。Spec 的 Markdown 正文与内置 traceability、failure_paths、sections 自动检查分别保留，无任意检查脚本。

`query.configuration_defaults` 返回可直接采用的分阶段 Feature/Task、Role→Agent 和组件精确引用；读取不创建项目配置、不安装运行时。`query.workflow_requirements`/`query.specification_audit` 可预览未保存 document 或已保存修订，递归返回 Role 需求、规范快照、基线判定和快照差异。`runtime.prepare_harness` 按明确 Harness 修订准备安装。历史 `query.default_configuration` 仅供已有 bundle 读取，不再是正常产品入口。

Standard 只使用显式 baseline_rules，不支持旧用途 slots。新增子流程 `workflow.call` 支持声明输入/输出、递归引用和真实运行；入口条件/确认先于内部节点，深度最多 8、总展开节点最多 1000。

Attempt 除文档与绑定外冻结 Context 内容与 revision。Harness 的执行输入包含冻结 Context 和继承 Snapshot；Reviewer 输入为目标 Delivery 的冻结文档、证据和报告正文。TestRequest 必须包含 operation_id、phase、candidate、test_definitions、environment、origin，可附 session_id、完整 definitions、code 和 runtime，适配器结果必须回传匹配的绑定。

Delivery 文件清单及工程 Snapshot 清单含 `schema_version=1`；Project 上下文和会话输入/交接快照直接序列化 `Snapshot`，没有清单外层。上述格式均由 `schemas/manifest.schema.json` 描述，封存并记录哈希，不成为可写工程事实源。原生交付使用 `origin=external`；不接受 `simulated` 来源。各 Schema 的对应实现与校验边界见 [Schema 说明](../schemas/README.md)。JSON-RPC 不实现 batch/notification，查询与命令均要求 request id；无参数查询可省略 params，Command.payload 省略时为 null。

实现参考：[Axum SSE](https://docs.rs/axum/0.8.9/axum/response/sse/)、[Cargo Workspaces](https://doc.rust-lang.org/stable/cargo/reference/workspaces.html)。

## 领域补强协议

- document.revise 只接受 Feature target，必须携带该 Feature 的 expected_revision；payload.feature_id 不能作为替代入口。
- Requirement 可附 acceptance=[{key,description}]；Test 可附 verifies=[key]。Web 使用每行“标识 | 条件”和逗号列表录入。省略时分别登记完整正文为 AC-1、继承关联需求的全部条目。
- task.create 可附 scope={requirement,design,acceptance} 或 acceptance_keys。默认从已接受的初始 Snapshot 解析。task.replan(target=Task, expected_revision) 采用当前已接受规划及选定条目，新测试修订保留 supersedes。
- query.runtimes 返回 Harness/Sandbox 定义。注册表提供原生定义和已登记的真实运行时，旧定义只作历史引用。
- Run 返回不可变 binding；Session 返回 instance；Review 返回 assignment；Snapshot 返回 capture；Attempt 返回 baseline。
- 数据库直接使用全局 v10、工程 v41 的当前结构基线；旧结构必须显式离线迁移后打开，启动不执行旧系统转换链。

历史 Run 的 binding_verified=false，保留可从 Session 确定的身份，但不把无法证明的完整绑定用于技术重试；需要新建执行。新 Run 的 binding_verified=true。

## 持久化外部操作

- query.workspace.detail.effects 分页返回外部操作；detail.advances 返回当前 Feature 的有界目标推进记录。操作包含适配器身份、请求、回执、状态、revision 与理由；Web 提供查看回执，原生执行恢复通过 Session/RuntimeExecution 与 merge.reconcile 核对。
- merge.apply(target=Merge, expected_revision) 返回 {id,operation_id,accepted:true}，表示意图已保存；是否完成以查询的 Merge/owner 状态为准。供应商调用由调度器负责。
- merge.reconcile(target=Merge, expected_revision) 请求核对 needs_reconciliation 的推进。确认未推进后，旧候选 stale，重新 merge.request、验证与 Review。
- Attempt 另返回 outputs 和 scope；Harness 输入明确区分 frozen_inputs / stage_outputs，附锁定定义和运行实例。持久化回执不是批准命令；核心仍核对版本、身份、来源及门禁。
- 新表 effects、advances 与业务事件使用同一 SQLite 事实源。部分副作用的意图、调用、回执和投影分开提交；不能把这个协议解释为跨供应商原子事务。

GitContext 随 Attempt.git_context 和 MergeAdvance.context 返回；它包含冻结的 Workspace 与源/目标 owner、branch、worktree。Harness 输入的 code、code_baseline 和 TestRequest 的 definitions/code/runtime 均来自锁定版本，避免适配器查找隐含的全局目录。旧定位为 null，需要新建执行。

## 真实接入基础接口

全局配置库保存 `runtime_installations`。`query.runtime_installations` 返回安装引用和 `default_harness`；`query.workspace` 同步附带这两个字段。`settings.update` 可原子更新 `default_harness: codex|dsh`、`global_concurrency: 1..32`，至少一个；已有 Session/Run 锁不因偏好变化而改变。默认偏好初始为 Codex，仅支持原生执行项目。

- `runtime.register({key:{provider,version,os,arch},artifact,source,license})`：登记原生 executable（canonical_path+sha256）或以 digest 固定的 oci_image 引用。同键同内容返回原 ID；内容冲突拒绝。原生文件检查 header/platform、执行权限和摘要；脚本入口不等于完整安装，不接受作为完整运行时。登记不下载、不执行工具、不复制或删除共享安装。
- `runtime.discover_codex({})`：显式发现 PATH 中本机原生 Codex，校验格式并查询版本，登记一次。不调用模型。
- `runtime.integrity({id})`：重新校验文件摘要；镜像只登记引用时明确标记尚未 Docker 核验。`inference_verified` 不因摘要正确而变 true。
- `credentials.configure({provider:"dsh",api_key})`：macOS Keychain 写入；普通返回只含 configured、reference、storage。`credentials.status({})` 查询 DSH 状态。秘密不写 SQL、事件、日志或返回对象；Linux 当前不提供明文文件 fallback。
- `codex.auth.start({installation_id,device?:boolean})`：官方 account/login/start；默认浏览器 ChatGPT OAuth，返回临时授权 URL 与 loginId；可选官方 device-code 流程。认证进程仅发送账户方法，不运行模型或工具。使用 data/credentials/codex 的独立私有目录，Codex 自行管理令牌。不是读取／复制宿主桌面凭据。
- `codex.auth.status({installation_id})`：只返回 authenticated/type/pending，不返回账户个人信息或令牌。
- `codex.auth.cancel({installation_id,login_id})`：仅取消当前认证实例的原 login 请求。

这些配置/认证接口使用专用参数而非工程 Command；安装的幂等范围为全局安装唯一键，认证派发未知时拒绝盲目重复登录。工程会话、消息、提案与执行由后续原生协议承载，实际验收范围以对应计划的执行记录为准。

### Persistent role conversations (integration work in progress)

`session.open` targets a Project/Feature/Task at its current revision, or `global` with `scope: "global"` for a dataset-wide discussion. Its payload selects `agent` (immutable role revision), optional `provider`, `installation_id`, `model`, `work_type`, `snapshot_id`, and `predecessor`. Clients cannot supply a resolved `binding`. The host resolves defaults once and freezes the installation, image digest, adapter, model, tool policy, role and credential reference. Replaying the original command returns its original result before resolving current defaults.

`conversation.send` targets the Session revision with `input` and `client_message_id`. A repeated client message ID with the same input returns the original Turn; changed input is rejected. One active or uncertain Turn blocks another input. `conversation.cancel` targets a Turn revision and records intent; a dispatched Turn only becomes cancelled after owned-container stop proof. Restart marks dispatched work `needs_reconciliation`, stops the previous writer and never resends its input. Explicit cancellation retires uncertain work; continuing uses a successor Session with a handoff Snapshot.

`query.sessions` accepts `owner_id`, `offset`, `limit`. `query.session` accepts `id`, an event `cursor` (exclusive), turn `offset`, and `limit` (maximum 100), returning the frozen Session, Turns, ordered events and next cursor, interactions, proposals, and execution history. Event `mode=delta` appends to `provider_item`; `mode=full` replaces that item's displayed content. Stored events remain append-only.

`interaction.answer` targets the request revision, requiring `session_id`, `turn_id`, and typed `answer` (`tool_approval` or `user_questions`). Runtime permission approval, clarifying questions, Proposal adoption and formal Delivery Review are separate decisions.

`proposal.extract` reads the last complete assistant JSON from a completed `turn_id`. `proposal.create` accepts human-edited `content` conforming to `ProposalContent`; both require an explicit source Turn and resolve a `target` and `expected_revision`. `proposal.revise` creates a new immutable draft revision. `proposal.adopt`/`proposal.reject` target the Proposal revision. Adoption is atomic, validates current target and immutable input documents, and invokes the existing formal object commands. `proposal.instantiate` materializes an accepted TaskPlan's stable keys and dependencies after the existing planning Review/Snapshot gate; the key-to-Task mapping is persisted and cannot be replaced. Proposal acceptance does not accept a Delivery or a Merge.

在采纳步骤完成前，其上游讨论及子流程输出会重新核对同一讨论的有效提案，包括同身份、同目标的后继 Session；拒绝、失效或已有后继修订的提案不再作为当前输出。新提案改变输出时，原人工确认失效，必须确认新版本。采纳步骤完成后，已消费的提案、执行、交付及候选仍固定到原轮次；后续业务变更使用返工，重新生成受影响步骤的引用。重新打开项目会将未结束流程标为待核对，以恢复中断前的等待状态。

Project-owned Requirement/Artifact/Snapshot records use `project_id` with NULL `feature_id`; Feature/Task materials use the inverse ownership. Feature `source_requirement` pins a same-project Requirement revision. Runtime installations, role definitions and Permission profiles use global schema v10; engineering and conversation records use project schema v41. Only the current schema and native execution paths are supported.

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

## 执行权限冻结

权限统一使用 v2 完整 profile 与确切目标绑定，接口见“Permission 运行时资源”。Agent 必须提供 Role、Capability 与命名的 `permission_selection`。项目、角色及本次执行的 `permission_overrides` 被拒绝；客户端不能提供解析后的执行 binding。

`query.session` 的 RuntimeBinding 与 RuntimeExecution 返回冻结权限。独立系统测试使用无模型连接的固定测试策略；Sandbox 禁网与 Harness 工具禁网分别编译和核验。

## v0.3 Workflow 与 Agent 组合接口

仍使用 JSON-RPC v1 和 Command 信封。配置的 id 是精确修订 ID，definition_id 是跨修订稳定身份；version 不等于用于 CAS 的 revision。Scope 为 `{kind:"global"}` 或 `{kind:"project",project_id}`；Feature Scope 由内核建立锁和运行事实。

| 接口 | 主要参数及结果 |
|---|---|
| model/tool/harness/skill/spec/standard/agent.create | payload.name/scope/data；返回不可变 ConfigurationRecord。Agent 的 data 固定 model_ref/harness_ref/skills/tools/permissions、职责及指令 |
| 上述类型 .revise | target 为旧修订，expected_revision；追加新版本，旧 AgentID 不变。Agent 必须使用包含完整 Role/Capability/权限选择的 data |
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

### 默认模板目录与恢复

`resources/configuration/manifest.json` 声明应用发布的完整默认资源。空库初始化生成数据库副本，所有 UI 编辑继续写入数据库修订或草稿；应用升级不改写已有当前目录。`query.configuration_defaults` 从当前目录保存的映射提供新项目预填配置。

| 接口 | 参数和结果 |
|---|---|
| query.template_catalog | 当前 generation、内嵌默认 bundle_version/bundle_hash、当前来源版本和数量 |
| query.template_reset_preview | 全局和全部项目的定义/草稿数量、恢复后的准确逻辑定义数量、固定旧绑定的项目摘要及 catalog_fingerprint |
| configuration.templates.reset | Command payload 为 expected_catalog_generation、catalog_fingerprint、bundle_hash；返回 generation、changed、各类型 counts 和提交回执 |

重置在一个全局事务中构建完整默认及精确依赖，并切换目录代次。自建定义、自定义修订和流程草稿退出当前目录；旧项目绑定、Feature 锁、运行、会话、经验、凭据、安装和偏好保留。已提交的相同 command_id 重放原结果；目录已与同一默认一致时返回 changed:false。提交前定义或默认资源发生变化时返回 catalog_changed，需要重新预览。

定义写入 RPC 在 Command 顶层携带 expected_catalog_generation。首次重置后必须提供该字段，旧页面不能重新发布旧代次定义。相关目录查询返回 catalog_generation 与 template_sources；分页结果的代次变化时重新加载。include_history 只查询当前代次的修订，include_archived 显式包含旧代次。精确 ID 的工程和历史查询继续读取旧记录。

query.project_configuration 附带 pinned_records 与 uses_archived_templates，供项目页显示其固定的旧定义。归档不等于原有的禁用或撤销。具体存储与兼容合同见 [模板目录合同](plans/configuration-template-reset/CONTRACT.md)。

`query.workflow_validation` / `query.workflow_preview` 接收 document 或 id，返回 valid/errors、document_hash、顺序及演练状态，不调用模型、Docker、Git、测试或 Merge。sample.outputs 和 sample.accepted 表示演练事实；通过预演不代表真实证据成立。动作及 DSL 结构见 schemas/rpc.schema.json，只接受当前 v3 契约和编译器。

`query.workflow_instance` / `query.workflow_actions` 返回运行记录、冻结计划/配置及可用动作。运行实例 ID、步骤 key、步骤实例 ID 和 round 分别表示不同概念；旧 round 的确认不能用于新 round。

AgentID 经验属于有来源的上下文，不是 Tool；原生工具必须来自明确的 Harness 工具组。

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

## 模型服务商接口

`query.model_providers` 返回服务商、Harness、DSH 路由、协议、端点、认证类型与固定目录候选。`models.list` 通过 `model_provider` 选择目录；Ollama 可提供 endpoint，并由 Host 查询本机服务；DeepSeek 读取账号目录，智谱和 MiniMax 返回固定 DSH 版本的内嵌支持目录。候选不自动保存，返回值显式区分目录来源与未验证的推理。

`credentials.configure/status/check/authorize` 增加 model_provider；省略时默认 DeepSeek。configure 保留 provider=dsh，api_key 仅用于 vault 写入，不进入普通查询。Ollama 状态 auth_kind=none，不支持存储或授权 key。

`agent.connection.check` 和 `agent.model.verify` 接收 project_id、agent_revision_id。前者创建带冻结绑定的完整 Agent Session，从 DSH 镜像查询模型目录，返回 container_verified 和 inference_verified=false；后者通过既有 session.open/conversation.send 提交真实模型与工具验证，返回 Session/Turn/marker，完成状态从 query.session 读取。

RuntimeBinding 可增加 model_connection 与 model_profile_digest，冻结服务商、路由、协议、声明/worker 地址、容量和生成 profile 的摘要；credential_ref 仍是唯一凭据引用。旧缺字段绑定只按原有 Codex/DeepSeek 语义读取，新服务商缺快照必须拒绝。Model 的 context_window/max_output_tokens 为容量声明，parameters.max_tokens 是实际请求预算。

`query.runtime_options` 只读取安装、受支持配方和服务商凭据的配置状态，不构建运行时或调用模型。安装项标明 `execution_supported` 与用途；本机 Codex 用于登录和模型目录，Linux Docker worker 用于项目执行。设置和基础组件 Harness 的安装列表仅展示项目执行用的 Linux Docker 安装；本机 Codex 登录与模型目录组件保留内部登记，不显示为项目安装。旧偏好的 `use_system_default` 仅跟随 Harness 名称，默认后端改变后须重新明确安装及模型。`models.list` 对 DeepSeek 读取账号 `/models`，对 Ollama 读取本机服务；智谱与 MiniMax 返回固定 DSH 版本支持的目录，并在 `source` 中区分。读取目录不执行推理。

项目默认与角色运行时偏好只接受确切 `model_ref`、`harness_ref` 修订及 provider。保存前校验引用类型、可见性、退役状态和 Model/Harness 兼容性。执行的模型、连接、容量与参数来自完整 Agent 的 Model 修订，Harness 按固定配方物化；不接受原始模型名、安装 ID 或思考深度覆盖。有效参数冻结在 RuntimeBinding，已有会话不随偏好变化。

`integration.delete` 将接入源及所属工具、绑定从后续选择中删除，或单独删除已发布工具。操作原子检查在用引用，拒绝仍被 Capability、Agent、Skill 或项目配置使用的资源；修订和历史调用保留，紧急停用另由 `integration.disable` 处理。

全局角色讨论使用 `scope: "global"`，`session.open` 的目标为 `global`，不得同时提供 `project_id`。数据单独保存在 `conversations/global/engineering.sqlite3`；不创建 Project。Session 的 `scope` 为 `global`，其 `project_id` 列保留固定的执行命名空间 `00000000-0000-4000-8000-000000000000`，用于容器归属和 Session/Turn 外键；它不属于项目目录。全局 Session 仅接受全局 AgentID，消息、快照及经验仍按该身份保存。全局讨论中的方案须切换到具体项目后再进入工程采纳流程。

权限合同 v2 绑定权限语义、受支持运行版本与实际策略资产；普通 UI、模型目录及非权限 schema 更新不改变权限合同。适配器精确构建在运行绑定中独立冻结。Feature 冻结确切权限闭包，不再生成旧默认权限转换或 feature_permission_renewal 记录。

`feature.create.predecessor` 只记录同项目 Feature 的前后关联，代码仍从项目主干基线建立。界面枚举同项目所有 Feature，并明确显示这一语义。

`query.workflow_instance` 附带当前 TaskGroup 的 `children` 摘要（实例 ID/修订、任务名称、父步骤），子流程详细计划按实例单独读取。执行与交付以阶段为整行展示，编译展开的子流程保持在其调用阶段内；Task 子流程在所属阶段中按需展开，折叠状态按项目和工作流实例保存在本机。

`query.sessions` 可按 `agent_instance_id` 在分页前筛选。列表和会话详情返回该身份的关联元数据。独立讨论完成后保存 `conversation_completed` 快照，AgentID 历史包含本轮消息与真实终态证据；切换身份不继续沿用原会话。

### 当前冻结任务渲染器

配置冻结只使用 `agent-task/v3`，正式检查输出的 `schema` 与 `output_schema` 一致。文档任务包含 `document.kind=doc` 和正文类型。旧任务渲染器与旧输出合同被拒绝；当前已冻结任务、业务输入和实际执行证据保持不可变。

候选评审的冻结输入另含 `candidate_provenance`：协调器通过 Git 端口读取确切候选的实际 parents，要求恰好一个且等于冻结 expected_target，并附 Merge 及对应已完成仓库操作回执。模型不需要访问隐藏的 Git 元数据来猜测父子关系；历史评审输入不回写，新的评审 Run 冻结当前实际证据。

`Structured Task Plan` 与普通实现说明分别维护 doc 修订链。普通说明不能通过 supersedes 隐藏规划输入；历史跨链引用保持原记录，只在新 Attempt 的有效输入投影中区分两类文档。

工具正常调度会核对 needs_reconciliation 调用迟到的实际持久化回执；未取得真实回执时保持未知状态，不自动重派写调用。

### 模型内容校验与人类 Approve

正式内容校验使用 `evidence-checks/v2`，结构见 `schemas/stage-answer.schema.json`。新请求要求 `approve`、`summary` 和四类 `checks`，每项为 `key`、布尔 `passed`、非空字符串 `evidence`。`approve` 必须与逐项结论一致。错误返回具体字段路径。

业务提示词来自冻结 Workflow Step 的任务和 Agent 配方节点任务，运行器不追加隐藏的阶段业务指令。模型读取当前 Skill/Spec、作者 Skill 及实际材料判断内容；章节、关键词探测不再作为新模型报告的内容质量门禁。

有效报告的 Run 记录 `model_approve`、`approval_required`，模型拒绝与执行失败分开。校验和独立评审均进入 `waiting_confirmation`，即使步骤配置自动推进也等待人类决定。`workflow.step.confirm` 携带当前 `input_hash`；覆盖模型拒绝还需 `overwrite_model:true` 和非空 `reason`。不可变确认记录保留模型报告、Skill 修订、轮次、操作者、原结论与覆盖理由；独立评审覆盖另外写入 Review Override，原模型 Review 不改写。报告或输入改变会使旧确认失效。

取消中的旧 Run 返回 `run_cancellation_pending`。该错误仅表示启动前等待，工作流保留 prepared 意图，待 Run 和旧 writer 均停止后继续。等待状态保持 prepared 意图，不会把尚未停止的执行者标成已停止。

`query.session.formal_execution` 标识正式执行及其后继会话；仅从当前冻结任务绑定及同归属后继链解析输出合同。正式 Run 的后继不接受独立讨论输入或提案采纳，界面提供“查看执行会话”和“返回当前工作流”。模型报告尚未齐备时显示等待条件，报告完成后由对应步骤提供人类 Approve。

正式 Turn 的 `needs_reconciliation` 不再使 Run 永久显示运行中。取得原执行的 writer 撤销和协议封存证明后，Run 记录为 `interrupted`，保留未知结果及原 Turn，并提供技术重试。显式重试创建使用原绑定/任务的新 Run；Task 继续使用原 Session，Feature 级执行创建后继 Session，原 Attempt 的 Session 映射保持冻结。未知结果须先核对，旧输入不会自动重发，独立会话回复不会被补认成正式报告。

Delivery 类型配置与跨步骤材料引用：`delivery_type.create/revise` 管理确切修订的 `{format, body}` 定义，与正式运行的 `Delivery` 业务实例分开。流程与规范的 Delivery 页提供 PRD、Design Doc、Diff、Test Report 默认定义。Workflow v3 的 `delivery_outputs.<name>={delivery_ref,source_port}` 将本步骤的真实输出命名为交付物；`delivery_inputs.<name>={delivery_ref,source:{step,port}}` 引用依赖链上同一类型修订的交付物。编辑器选择引用时自动加入生产步骤依赖；发布校验端口、依赖、作用域及类型修订，不用正文声明替代实际输出。子流程展开后保留来源映射；Agent 的冻结任务携带 Delivery 定义正文、产出/引用声明及真实材料。类型修订不会改写已冻结的 Feature 或 Turn。

Task 会话按 owner 与 Agent 身份复用最新 Session 和原生 thread，不以 Workflow 步骤或执行配方节点分组；同一 Task 的实现与文档、交付评审与候选评审因此可以续用对应 Agent 的会话。不同 Task、不同 Agent 身份彼此隔离，Feature 级会话继续按步骤与节点隔离。仅显式 `force_new` 新建同一 Task/Agent 的会话，已有活动轮次须先结束或核对；新节点不会因运行基线漂移自动新建会话。Task 技术重试保留 Session，未知结果仍须先核对，不能自动重发输入。

Session 的 Agent、Model/Harness、凭据和权限基线保持固定，Task 的权限或集成变更不会自动创建后继 Session。Task 的节点上下文作为不可变 `execution_task_binding` 追加，`data.session_id` 关联共享 Session；首条绑定使用 `definition_id=SessionID`，后续绑定拥有独立目标 ID。无 Agent 任务的系统步骤通过 `execution_runtime_binding` 冻结相同的上下文及范围。正式 Run 创建时按 RunID 单独冻结节点来源与运行锁，后续打开其他节点不会改变已排队 Run。每个 Turn 的 `execution_task` 保存本轮 `runtime_binding` 与确切任务来源，分别冻结节点指令、Spec、输出合同及执行范围。协调器和外部工具调用均读取本轮锁，使用固定的原生 thread 恢复执行；Session 的初始锁与旧任务快照不会改写。共用 Session 的节点串行派发，测试节点仍由系统测试执行器执行。

`query.sessions` 用 `workflow_session_id` 锚定 Task 时返回同一 Task/Agent 的会话历史；显式步骤/节点筛选匹配该会话曾绑定的上下文。`query.session.current_runtime_binding` 提供当前节点范围，`session.runtime_binding` 继续表示初始锁。角色讨论默认选最新会话，也可查看历史或显式新建 Session；正式 Run 保留技术重试及人类 Approve 入口。

角色与 Agent 界面保留 Role、Capability、Agent 三个页签；Agent 页用于组合 Role 与 Capability。内部 AgentID / `agent_instance_id` 身份协议不变，角色讨论中的“新建讨论身份”从所选 Agent 的确切修订创建全局或当前项目身份。每个身份独立保留会话和经验，重复打开同一执行上下文时复用最新会话。
