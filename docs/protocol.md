# 本地协议

JSON-RPC 2.0 与同源 HTTP / SSE。当前数据基线 global 16 / project 47；Workflow v6、Spec v3 / Standard v4、Proposal v2 和 `agent-task/v6`。精确字段及参数限制见 [发布 Schema](../schemas/README.md)，语义见 [领域规范](domain.md)。仅执行当前合同，不升级或转换旧数据。

## HTTP 与命令

| 接口 | 合同 |
|---|---|
| GET /connection | 无敏感信息的产品、协议及认证探测；允许跨源读取，不发放会话。 |
| POST /session | 验证连接密钥并建立 HttpOnly / SameSite=Strict 同源会话；本机未配置密钥时可提交空对象。 |
| POST /rpc | JSON-RPC 2.0，要求 request id，不支持 batch 或 notification。 |
| GET /events?project_id=ID&cursor=N | 固定项目的 SSE；事件 ID 为 `ID:cursor`，支持同项目 Last-Event-ID。 |
| GET /artifacts/{id}?project_id=ID | 按活动数据库 ID 读取并校验 SHA-256，客户端不能指定宿主路径。 |
| GET /capabilities | 协议、版本、供应商、Docker 和平台能力。 |
| POST /setup | 打开数据集；非空非 DevDesk 目录拒绝，打开后不能热切换。 |
| POST /shutdown | 可信人类会话退出 Host。 |

命令参数包含 `command_id`、`target`、`expected_revision`、`payload`；有版本的目标要求确切修订。相同 ID 与相同参数重放首次结果，检查早于版本冲突；不同参数复用 ID 拒绝。客户端不提交 actor 或已解析执行 binding。工程请求携带 `project_id` 固定请求作用域，过期选择返回 `project_changed`，查询不会隐式切换项目。

```json
{"jsonrpc":"2.0","id":1,"method":"workflow.start","params":{"command_id":"uuid","target":"project-id","expected_revision":1,"payload":{"workflow_ref":"published-workflow-revision","role_agent_bindings":{}}}}
```

`query.workspace` 返回项目目录及活动工程，`query.history` / `query.events` / `query.artifact` 读取对应事实。分页参数有界；SSE 只通知变化，客户端重新查询权威状态。没有通用 `set_status`。

历史记录页使用 `query.history({scope,project_id?,include_tool_calls:true,offset,limit})`：Global 读取全局配置库，当前项目读取对应活动项目库；先合并持久化事件、外部工具及平台工具调用，按事件 time / 调用 created_at 倒序排列，再统一分页，默认每页 50 条、上限 100 条。返回 `{records,total}`，每条为 `{type:"event",time,event}` 或 `{type:"tool_call",time,call}`；调用沿用 `query.tool_calls` 的来源投影和敏感参数脱敏。不传 include_tool_calls 时仍返回 `{events,total}`，`query.events` 的 SSE 游标语义不变。

JSON-RPC 使用 -32600 / -32601 / -32602 / -32000；领域说明位于 `error.message`，领域代码位于 `error.data.code`，常见为 invalid_params、not_found、blocked、revision_conflict、project_changed、project_required、configuration_required、dataset_locked 和 storage。

## 配置、编译与任务

`role.create/revise/delete` 只管理自由业务身份，不生成默认 Agent 或 Skill。Capability 和 Agent 明确引用当前资源；不从角色名称推导权限或业务输出。`query.configuration_defaults` 返回当前目录的确切默认引用，不创建项目采用记录，也不安装运行时。

`workflow.create` / `workflow.draft.create` 建立 v6 草稿，`workflow.draft.update` 修改草稿，`workflow.publish` 校验并发布不可变修订。`project.configuration.update` 保存确切 Standard、Workflow、Role → Agent 和执行授权；未选快照时原子生成。`spec.create/revise` 使用 v3 数据，`standard.capture.preview/capture` 生成 v4 不可变快照；`delivery_type.create/revise` 定义通用交付格式及可选离线 Schema。

`query.workflow_requirements`、`query.workflow_validation`、`query.workflow_preview` 和 `query.specification_audit` 检查完整闭包、身份、输入输出、Spec 与基线。预演不会调用模型、Docker、Git 或测试，也不产生真实证据。

`query.workflow_task_preview` 使用 document 或确切修订及步骤位置，返回可解析的 Role / Agent / Skill / Spec、任务、输入、输出合同和缺口；不是实际运行授权。`query.execution_task` 按 `turn_id` 返回持久化任务、渲染版本、摘要和来源。

canonical `agent-task/v6` 包含 Stage 背景、Step 任务、职责、能力、确切输入、绑定来源和值摘要、逐端口输出 Schema、动态检查、授权及预算。文本端口值是字符串；JSON 端口按声明 Schema 校验。封套为 `status`、`summary`、`outputs`，只有 completed 必须提供全部必需输出。

## 运行与交互

`workflow.start` 固定 owner、输入、配置闭包和绑定；`query.workflow_instance` 返回 record、runtime、plan、outputs、children，以及按执行 ID 索引的 completion_status（satisfied、当前确切版本的 confirmation）。调用路径、Step key、执行 ID、round 和 sequence 分别表示不同身份。

`workflow.pause/resume/cancel` 控制实例。`workflow.step.input` 补充本次任务输入；`workflow.step.confirm` 提交确切 input_digest、outputs 及理由；`workflow.step.retry`、`workflow.step.rework`、`workflow.step.reconcile` 分别处理技术重试、业务返工和未知结果。所有命令都经过当前状态与来源校验。

模型内容报告使用 Spec 当前的任意 key：`approve`、`summary`、`checks[{key,passed,evidence}]`。拒绝保留原报告；普通 confirm 覆盖完成条件仍要求 Step 明确允许。独立的 `workflow.step.approve` 是显式人工例外入口，payload 必须提供 step、execution_id、input_digest、outputs 和非空 reason；仅允许 waiting_confirmation 且未请求停止的流程，保存 manual_approval 与 override_completion，绑定确切输入输出，不改写原结论或冻结配置。没有隐藏的固定四项检查。

默认 Task 的 implementation.evidence 使用 Execution Report delivery：runs 包含执行序号、阶段、命令、目录、起止时间、退出码、stdout/stderr，以及对应测试和源码的 path、sha256、content。缺少必需证据或仅提供临时路径会被输出 Schema 拒绝。后续 documentation、review、candidate_review 的 inputs.evidence 和 handoff 工具参数显式绑定此端口；原始内容沿现有 workflow_output / Artifact 存储，来源及 value_digest 进入规范任务。此交付是 Agent 记录，不能冒充 Test Report 的平台测试回执。

`conversation.send` 不能提交伪造的 task、frozen_task、task_instructions 或 task_source。Workflow 会话只接受来自当前冻结执行意图的输入；补充输入仍沿用原 Step 任务。会话或轮次派发失败及时成为步骤失败，子流程重试恢复对应父调用。暂停与人工等待不消耗活动预算。

普通讨论 Proposal v2 保存 `delivery_ref`、`title`、`value`、`input_refs`。`proposal.create/extract/revise/reject/adopt` 管理其修订与采纳；采纳只产生类型化材料，创建 Task 需要明确平台操作。

## 工具与工程资源

独立 Tool Step 通过冻结的工具定义、参数映射和执行身份调用。HTTP / MCP 工具使用明确接入及项目账户绑定。Codex Agent 平台工具还要求当前 Workflow、Capability、Step 与 Permission 的共同授权，绑定真实 Session / Turn / worker / 原生调用 ID；普通讨论不提供这些平台工具。

平台处理器为 workspace.current、workspace.snapshot、materials.capture、tasks.create、tasks.complete、tests.run、git.candidate、git.advance、git.publish。函数桥使用可移植的 `devdesk_` 名称映射，处理器标识不变。DSH 不支持动态平台工具桥，配置时拒绝。需要批准的平台写入使用显式 Tool Step 和确认门禁。

`query.tool_calls` 同时返回实际外部调用与平台调用，保留真实 caller 和平台回执；按 Session、Turn、AgentID 等来源筛选。平台请求和终态回执不可改写，原生回复交付另存记录。`tool.call.cancel` 请求停止尚未结束的调用，未知副作用仍需核对。

工作副本和代码输入核对实际登记、owner、修订、路径、分支与流程来源。目录来自明确授权，不强制业务目录名称。测试只接纳实际固定代码与 Docker 回执；候选返回实际父提交及确切 diff。推进核对同一运行的真实候选、对应测试、确切确认与预期目标；发布继续核对远端 lease 和 PR merged 的确切 SHA。

## 项目、运行时与凭据

Web 使用 `project.open_github`，首次提交 repository、可选 mainline 和 title，再次可指定项目 target；接入意图、克隆及回执异步完成。`project.open` 切换已登记项目，`project.create` 保留为后端元数据入口。仓库实际就绪前不能启动工程执行。

`runtime.prepare` / `runtime.prepare_harness` 可以显式预热固定环境，正常会话按所选 Harness 延迟准备。`runtime.preparation_status` 返回 preparing / ready / failed；构建在事务外进行，失败不登记成功安装。运行和重试沿用原冻结安装。

`codex.auth.start/status/models/cancel` 管理本机登录与目录组件。`credentials.status/check/authorize` 只返回元数据、不透明引用及可用性，不读取显示秘密。系统凭据授权通过专用助手或宿主 Secret Service 完成，详见 [凭据说明](system-credentials.md)。

## Permission 运行时资源

`query.permission_resources` 返回 v2 profiles、绑定修订、确切目标版本、活动分配与来源。可携带 project_id 读取当前项目可见资源；省略时只读取全局目录。v1 权限接口、预设与覆盖字段均已移除。

新增命令均使用 Command：`permission.profile.create/revise` 的 payload 为 identity（创建时）、name、target_kind、policy；revise 的 target 是原修订 ID，expected_revision 为原版本。`permission.profile.import` 接受 payload.profiles，批次原子；`permission.profile.reload` 只读取当前唯一预设目录，payload.builtin=true 时读取内置目录并在成功后恢复内置来源，失败不切换。`permission.source.select` 接受 payload.path，必须为存在的普通绝对目录；选择替换当前来源并持久化，需显式重新加载。`permission.source.browse({path?})` 返回 path、parent、directories 和 locations，仅列出普通子目录，省略 path 从数据目录开始，供文件夹选择窗口使用。`query.permission_resources` 的 source 返回 path、builtin_path、is_builtin；不再返回多来源列表或支持追加登记。

`permission.profile.export({id})` 返回 `{profiles:[前序修订…,所选修订]}`，可直接重新导入。导入只接受 `{kind:"permission_profile",identity,revision,name,scope,data:{schema_version:2,target_kind,policy}}` 源定义；policy 的 null 和未知字段拒绝。

`permission.bindings.apply` 的 payload.bindings 为非空数组，每项包含 target={kind,revision_id,content_hash}、permission_revision_id、expected_revision（该目标尚无分配时为 0）。执行合同由 Host 生成，客户端不能决定。一个批次必须属于同一全局或项目配置库作用域；全部校验通过后原子提交。返回各目标的新活动 revision 与 binding_revision_id；command_id 重放优先于版本校验。

`permission.binding.preflight({target,permission_revision_id})` 只检查绑定兼容性，返回 scope_checked=false。完整预览输入 harness_ref、selection={sandbox_ref,harness,sandbox} 和可选 scope={work_type,writable_paths,read_only_paths}；selection 每层为 `{mode:"follow_target"}` 或 `{mode:"pinned",binding_revision_id}`。提供 Scope 时复用实际执行编译器；无 Scope 时不承诺具体工作范围可执行。

Agent 只保存 role_ref、capability_ref。项目选择独立的 permission_context_ref，授权上下文提供 Sandbox、两层权限选择、步骤范围和控制策略。RuntimeBinding.permissions.binding_lock 为按源引用解析的执行视图，持久化仅保留确切绑定引用和实际执行范围。

`conversation.send` 对追随目标的讨论在接受新输入前检测改绑，无活动工作时持久化切换并创建后继 Session。返回值可增加 effective_session_id、predecessor，UI 跟随有效 Session；原 command_id 和 client_message_id 保持幂等。query.session/session.read 增加 permission_status，包含 frozen、desired、status、reason、pending_command；待恢复时可用 pending_command 原样重试 conversation.send。已有运行、排队输入、工程冻结与技术重试保留原权限，目标默认更新不改写它们。

默认监听 127.0.0.1，可用 `--host IP` 显式监听其他地址；非 loopback 监听或配置 `--public-url` 时必须设置 `DEVDESK_ACCESS_TOKEN`（32–512 位非空白 ASCII 连接密钥）。内核启动时生成随机会话；默认本机页面自动建立会话，受密钥保护的内核只在 `POST /session` 验证通过后发放 HttpOnly、SameSite=Strict Cookie，公开地址为 HTTPS 时附带 Secure。校验连接地址对应的 Host 和有值的 Origin，缺失或错误会话拒绝 RPC/SSE/Artifact。用户 human:local 身份由 host 装配，载荷中的 actor 无效。

Web 连接其他内核时，先以无凭据请求探测 `/connection`，确认产品及协议后导航到目标 origin，清除原地址中的 Project/Feature 等路由引用。RPC、SSE、产物和 OAuth 回调始终同源，目标内核自行建立会话；会话 Cookie 名按公开端口区分，避免同一主机不同端口相互覆盖。浏览器仅记住目标地址，连接密钥不进入 URL 或持久存储。退出当前内核后，已加载的界面保留连接入口。未来原生客户端可复用探测与会话接口。


## 默认模板目录与恢复

`resources/configuration/manifest.json` 声明应用发布的完整默认资源。空库初始化生成数据库副本，所有 UI 编辑继续写入数据库修订或草稿；应用升级不改写已有当前目录。`query.configuration_defaults` 从当前目录保存的映射提供新项目预填配置。

| 接口 | 参数和结果 |
|---|---|
| query.template_catalog | 当前 generation、内嵌默认 bundle_version/bundle_hash、当前来源版本和数量 |
| query.template_reset_preview | 全局和全部项目的定义/草稿数量、恢复后的准确逻辑定义数量、固定旧绑定的项目摘要及 catalog_fingerprint |
| configuration.templates.reset | Command payload 为 expected_catalog_generation、catalog_fingerprint、bundle_hash；返回 generation、changed、各类型 counts 和提交回执 |

重置在一个全局事务中构建完整默认及精确依赖，并切换目录代次。自建定义、自定义修订和流程草稿退出当前目录；旧项目绑定、Feature 锁、运行、会话、经验、凭据、安装和偏好保留。已提交的相同 command_id 重放原结果；目录已与同一默认一致时返回 changed:false。提交前定义或默认资源发生变化时返回 catalog_changed，需要重新预览。

定义写入 RPC 在 Command 顶层携带 expected_catalog_generation。首次重置后必须提供该字段，旧页面不能重新发布旧代次定义。相关目录查询返回 catalog_generation 与 template_sources；分页结果的代次变化时重新加载。include_history 只查询当前代次的修订，include_archived 显式包含旧代次。精确 ID 的工程和历史查询继续读取旧记录。

query.project_configuration 附带 pinned_records 与 uses_archived_templates，供项目页显示其固定的旧定义。归档不等于原有的禁用或撤销。具体存储与兼容合同见 [模板目录合同](plans/configuration-template-reset/CONTRACT.md)。


## 模型服务商接口

`query.model_providers` 返回服务商、Harness、DSH 路由、协议、端点、认证类型与固定目录候选。`models.list` 通过 `model_provider` 选择目录；Ollama 可提供 endpoint，并由 Host 查询本机服务；DeepSeek 读取账号目录，智谱和 MiniMax 返回固定 DSH 版本的内嵌支持目录。候选不自动保存，返回值显式区分目录来源与未验证的推理。

`credentials.configure/status/check/authorize` 增加 model_provider；省略时默认 DeepSeek。configure 保留 provider=dsh，api_key 仅用于 vault 写入，不进入普通查询。Ollama 状态 auth_kind=none，不支持存储或授权 key。


RuntimeBinding 可增加 model_connection 与 model_profile_digest，冻结服务商、路由、协议、声明/worker 地址、容量和生成 profile 的摘要；credential_ref 仍是唯一凭据引用。所选服务商需要的连接快照不完整时拒绝。Model 的 context_window/max_output_tokens 为容量声明，parameters.max_tokens 是实际请求预算。

`query.runtime_options` 只读取安装、受支持配方和服务商凭据的配置状态，不构建运行时或调用模型。安装项标明 `execution_supported` 与用途；本机 Codex 用于登录和模型目录，Linux Docker worker 用于项目执行。设置和基础组件 Harness 的安装列表仅展示项目执行用的 Linux Docker 安装；本机 Codex 登录与模型目录组件保留内部登记，不显示为项目安装。旧偏好的 `use_system_default` 仅跟随 Harness 名称，默认后端改变后须重新明确安装及模型。`models.list` 对 DeepSeek 读取账号 `/models`，对 Ollama 读取本机服务；智谱与 MiniMax 返回固定 DSH 版本支持的目录，并在 `source` 中区分。读取目录不执行推理。

项目只绑定 Workflow 与完整 Agent，不另设项目或角色运行时偏好。模型、连接、容量与参数来自绑定 Agent 的 Model 修订，Harness 按固定配方物化。有效参数冻结在 RuntimeBinding；每个 Workflow Step 使用独立 Session，补充输入沿用该 Step 的冻结运行锁。

`integration.delete` 将接入源及所属工具、绑定从后续选择中删除，或单独删除已发布工具。操作原子检查在用引用，拒绝仍被 Capability、Agent、Skill 或项目配置使用的资源；修订和历史调用保留，紧急停用另由 `integration.disable` 处理。

全局角色讨论使用 `scope: "global"`，`session.open` 的目标为 `global`，不得同时提供 `project_id`。数据单独保存在 `conversations/global/engineering.sqlite3`；不创建 Project。Session 的 `scope` 为 `global`，其 `project_id` 列保留固定的执行命名空间 `00000000-0000-4000-8000-000000000000`，用于容器归属和 Session/Turn 外键；它不属于项目目录。全局 Session 仅接受全局 AgentID，消息、快照及经验仍按该身份保存。全局讨论中的方案须切换到具体项目后再进入工程采纳流程。

权限合同 v2 绑定权限语义、受支持运行版本与实际策略资产；普通 UI、模型目录及非权限 schema 更新不改变权限合同。适配器精确构建在运行绑定中独立冻结。Feature 冻结确切权限闭包，不再生成旧默认权限转换或 feature_permission_renewal 记录。

`feature.create.predecessor` 只记录同项目 Feature 的前后关联，代码仍从项目主干基线建立。界面枚举同项目所有 Feature，并明确显示这一语义。

`query.workflow_instance` 附带当前 TaskGroup 的 `children` 摘要（实例 ID/修订、任务名称、父步骤），子流程详细计划按实例单独读取。执行与交付以阶段为整行展示，编译展开的子流程保持在其调用阶段内；Task 子流程在所属阶段中按需展开，折叠状态按项目和工作流实例保存在本机。

`query.sessions` 可按 `agent_instance_id` 在分页前筛选。列表和会话详情返回该身份的关联元数据。独立讨论完成后保存 `conversation_completed` 快照，AgentID 历史包含本轮消息与真实终态证据；切换身份不继续沿用原会话。

## 编排与运行计划

`orchestration.open/input/takeover/resume` 管理项目经理及后继会话；`query.orchestration` 返回对话、实际控制调用和待人类决定请求。输入是用户自然语言，只有冻结 Capability/Permission 允许的原生工具调用才产生控制效果。

控制工具：`workflow.inspect/events.read/outputs.read/execution.inspect`，`step.dispatch/accept/input/retry/rework`，`execution.stop/reconcile`，`pause/resume/cancel/complete`，`change.preview/apply`，`materials.import`，`decision.request` 和 `segment.dispatch`（均带 workflow 前缀）。人类通过 `workflow.decision.resolve` 签署确切请求。写入绑定活动 plan_revision_id；接纳还绑定 execution_id、input_digest 及输出 ID，变更绑定预演哈希。

`schemas/orchestration.schema.json` 定义类型化 ChangeSet。新增/修改步骤、依赖、输入、角色绑定及整体计划通过预演编译；应用期间根范围禁止新的写入派发，继续收集原运行事实。旧合同和结果保存只读历史。已派发 segment 只推进确切授权节点，遇到人工等待、评审拒绝、异常、预算或计划修改立即停止，不自动完成根实例。

编排封套 `orchestration-task/v2` 携带当前实例摘要、用户指令、策略及待决定项；持久化唤醒、消费游标、lease 和原生调用回执支持重启恢复。业务 `agent-task/v6` 携带计划修订、node_id 和 contract_digest。

源配置、权限归属及无持久化投影的存储规则见[源配置与执行解析](source-configuration.md)。

## Standard 快照接口

`standard.capture.preview` 只读解析确切 `root_workflow_refs`，`standard.capture` 核对候选并发布或复用不可变快照。`standard.archive` 只改变选择列表，不删除历史。`query.standard_catalog/standard/standard_diff/standard_usage` 返回按需视图及真实采用关系。

`work.configuration.update` 为尚未启动的工作选择 `standard_ref`、`workflow_ref`、执行者和权限；启动后使用 `WorkflowChangeOperation::AdoptStandard`，并沿既有 preview/apply、CAS、授权与静止等待路径切换。`query.work_configuration` 返回来源及计划历史，`query.work_standard_diff` 区分自身规范、入口投影及子工作采用。历史计划中的子调用清单以冻结的首次调用依据标记，当前调用树另行展示。
