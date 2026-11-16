# 本地协议

JSON-RPC 2.0 与同源 HTTP / SSE。当前 SQLite 基线为 global 18 / project 48；出厂 Workflow、Spec、Standard 分别记录 schema_version 6、3、4，任务记录 `agent-task/v6`。业务源按实际结构与规则接纳；SQLite user_version、数据集标记和外部凭据助手协议则有独立兼容校验，不能混为版本展示元数据。精确字段见 [发布 Schema](../schemas/README.md)，语义见 [领域规范](domain.md)。不升级或转换旧数据。

## HTTP 与命令

| 接口 | 合同 |
|---|---|
| GET /connection | 无敏感信息的产品、协议及认证探测；允许跨源读取，不发放会话。 |
| POST /session | 验证连接密钥并建立 HttpOnly / SameSite=Strict 同源会话；本机未配置密钥时可提交空对象。 |
| POST /rpc | JSON-RPC 2.0，要求 request id，不支持 batch 或 notification。 |
| GET /events?project_id=ID&cursor=N | 固定项目的 SSE；事件 ID 为 `ID:cursor`，支持同项目 Last-Event-ID。 |
| GET /artifacts/{id}?project_id=ID | 按活动数据库 ID 读取并校验 SHA-256，客户端不能指定宿主路径。 |
| GET /capabilities | 协议、版本、setup_required、供应商及平台能力声明；不代替 Docker 连通或真实推理验证。 |
| POST /setup | 打开数据集；非空非 DevDesk 目录拒绝，打开后不能热切换。 |
| POST /shutdown | 可信人类会话退出 Host。 |

命令参数包含 `command_id`、`target`、`expected_revision`、`payload`；有版本的目标要求确切修订。相同 ID 与相同参数重放首次结果，检查早于版本冲突；不同参数复用 ID 拒绝。客户端不提交 actor 或已解析执行 binding。工程请求携带 `project_id` 固定请求作用域，过期选择返回 `project_changed`，查询不会隐式切换项目。

```json
{"jsonrpc":"2.0","id":1,"method":"query.workspace","params":{}}
```

`query.workspace` 返回项目目录及活动工程，`query.history` / `query.events` / `query.artifact` 读取对应事实。分页参数有界；SSE 只通知变化，客户端重新查询权威状态。没有通用 `set_status`。

`query.ui_bootstrap` 返回活动项目、项目目录、配置新鲜度、配置健康摘要和页面必须的运行安装信息，不返回工程详情或历史快照；`query.project_directory` 只读取项目目录，支持 `project_offset`、`limit` 和 `github_only`。配置页面使用这两个入口，执行和历史页面继续读取 `query.workspace`。配置目录、默认选择、权限资源等查询保留原有响应内容，并附带 `catalog_version={host,global,project,project_sources}`；该版本不作为执行授权或并发写入令牌，模板重置仍使用 `catalog_generation`。

SSE 可显式传入 `configuration=true`，接收无持久化事件 ID 的 `configuration.changed` 版本通知，普通事件同时附带 `configuration_version`。只有持久化事件推进 `ID:cursor`，通知不改变游标和重放语义。不传该参数时保留原事件流合同。

设置 `DEVDESK_PERF_LOG=1` 可输出启动和 RPC 的 JSON 性能日志，包含请求编号、队列等待、连接等待、源解码/校验、恢复、编译、响应编码、字节数与阶段调用次数；嵌套阶段为包含关系，不能直接相加。RPC 同时返回 `Server-Timing` 和 `X-DevDesk-Request-Id`。日志不包含请求参数或响应正文，默认关闭。

历史记录页使用 `query.history({scope,project_id?,include_tool_calls:true,offset,limit})`：Global 读取全局配置库，当前项目读取对应活动项目库；先合并持久化事件、外部工具及平台工具调用，按事件 time / 调用 created_at 倒序排列，再统一分页，默认每页 50 条、上限 100 条。返回 `{records,total}`，每条为 `{type:"event",time,event}` 或 `{type:"tool_call",time,call}`；调用沿用 `query.tool_calls` 的来源投影和敏感参数脱敏。不传 include_tool_calls 时仍返回 `{events,total}`，`query.events` 的 SSE 游标语义不变。

JSON-RPC 使用 -32600 / -32601 / -32602 / -32000；领域说明位于 `error.message`，领域代码位于 `error.data.code`，常见为 invalid_params、not_found、blocked、revision_conflict、project_changed、project_required、configuration_required、dataset_locked 和 storage。

Web 的 `feature.create` 显式提交 `payload.start_workflow=false`：保存草稿、初始需求、流程输入及冻结的工作配置，不创建 Workflow、Driver 或经理会话。之后在执行与交付页以 `workflow.start` 启动该 Feature；不传 `inputs` 时使用创建时保存的 `workflow_inputs`。旧 API 调用省略 `start_workflow` 时保留创建并初始化流程的行为。

## 配置、编译与任务

项目页以 Standard 为唯一规范入口。`query.project_standard_preview({project_id,standard_ref,standard_overrides,harness_permission_ref?})` 返回生效快照、默认入口、角色需求、来源差异及 `preview_hash`，不写入数据库。`standard_overrides` 为 `{source_ref,replacement_ref}` 数组，作用于基础 Standard 内该确切修订的所有引用位置。类型、可见范围、重复覆盖、被其他覆盖移除的目标及引用循环会被拒绝。

`project.configuration.update` 的新入口提交 `standard_ref`、`standard_overrides` 和可选 `standard_preview_hash`，无需提交 `workflow_ref`。保存时核对预览；无覆盖项时直接引用所选 Standard，不创建项目专属 Standard、派生源或来源记录，有覆盖项时才原子发布项目专属 Standard 及必要的源后继。移除全部覆盖后恢复引用基础 Standard。存储的 `workflow_ref` 由 Standard 的默认入口解析得到。`query.standard` 返回 `default_workflow_ref` 与项目快照的 `provenance`。基础来源与差异永久保留；旧 Feature 的冻结引用不回填。原有仅提交 Workflow 的 API 继续支持生成规范快照。

`standard.capture` 支持根级 `default_workflow_ref`，必须属于 `root_workflow_refs`；Standard 编辑器要求显式选择默认入口。旧项目曾明确保存的入口及旧目录默认映射可在生成项目快照时沿用，转换后入口写入项目 Standard，不从多个根中任取一个。

`role.create/revise/delete` 只管理自由业务身份，不生成默认 Agent 或 Skill。Capability 和 Agent 明确引用当前资源；不从角色名称推导权限或业务输出。`query.configuration_defaults` 返回当前目录的确切默认引用，不创建项目采用记录，也不安装运行时。

`workflow.create` / `workflow.draft.create` 建立 v6 草稿，`workflow.draft.update` 修改草稿，`workflow.publish` 校验并发布不可变修订。`project.configuration.update` 保存确切 Standard、Workflow、Role → Agent 和执行授权；未选快照时原子生成。`spec.create/revise` 使用 v3 数据，`standard.capture.preview/capture` 生成 v4 不可变快照；`delivery_type.create/revise` 定义通用交付格式及可选离线 Schema。

`query.workflow_requirements`、`query.workflow_validation`、`query.workflow_preview` 和 `query.specification_audit` 检查完整闭包、身份、输入输出、Spec 与基线。预演不会调用模型、Docker、Git 或测试，也不产生真实证据。

`query.workflow_task_preview` 使用 document 或确切修订及步骤位置，返回可解析的 Role / Agent / Skill / Spec、任务、输入、输出合同和缺口；不是实际运行授权。`query.execution_task` 按 `turn_id` 返回持久化任务、渲染版本、摘要和来源。

canonical `agent-task/v6` 包含 Stage 背景、Step 任务、职责、能力、确切输入、绑定来源和值摘要、逐端口输出 Schema、动态检查、授权及预算。文本端口值是字符串；JSON 端口按声明 Schema 校验。封套为 `status`、`summary`、`outputs`，只有 completed 必须提供全部必需输出。

## Agent 能力与经验上下文

`query.discussion_agents({offset,limit})` 返回可用于无项目讨论的原 Agent 实例及确切配置依赖，读取不切换项目。全局讨论用 `scope:"global"`，不同时携带当前 `project_id`；项目实例保持原来源与经验范围。

Capability 可通过根级 `spec_refs` 引用 Agent 自身规范，业务能力从确切 Agent 配置装配。RuntimeBinding 中 AgentExecutionConfiguration 的 `context_protocol` 固定通用上下文协议；`devdesk-agent-context/v1` 提供 `agent.experience.list` 与 `agent.experience.read`。工具调用中的实例由真实 Session 和 worker 确定，参数不能改为其他实例。

列表按来源项目、工作流、Feature、Task、时间和关键字过滤，返回 `{records,total,next_offset,unavailable}`。读取传入 `source_project_id` 与 `snapshot_id`，可指定索引中的 `document_id` 或 `artifact_id`；返回 `{entry,integrity,content_digest,content_chunk,offset,next_offset,total_bytes}`。分块位置按 UTF-8 边界处理。来源引用可通过 `conversation.send.payload.experience_refs` 附加到本轮，正式步骤仍按原冻结任务提交输入。

`query.session.experience_reads` 返回本次讨论的经验读取来源、状态与内容摘要。上下文调用保存为 `agent_context_call`，可通过 `query.tool_calls` 读取（无项目讨论传 `scope:"global"`）；失败回执不使自由讨论丢失工具能力。缺少必要操作对象返回 `context_required`，外部工具未绑定账户返回 `integration_binding_required`，权限拒绝保留具体原因。

## 运行与交互

`feature.create` 可通过 initial_requirement 提供自然语言初始需求，保存不可变 feature_request；工作流将其以 user_request 冻结到实际任务，独立于声明的 workflow_inputs。`workflow.start` 固定 owner、输入、配置闭包和绑定；`query.workflow_instance` 返回 record、runtime、plan、outputs、children，实际执行者的展示索引 actors，以及按执行 ID 索引的 completion_status（satisfied、human_satisfied、当前确切版本的 confirmation）。调用路径、Step key、执行 ID、round 和 sequence 分别表示不同身份。

`query.workflow_materials` 在项目内只读汇总当前工作的交付物，必填 `owner_id`（Project、Feature 或 Task），可传 `project_id` 核对项目。Feature 包含所属 Task 的当前工作流，当前执行指向的嵌套子流程一并读取；历史轮次输出不会冒充当前产物。列表支持 `search`、`offset`、`limit`（默认 20、最多 100），仅返回交付类型、所属工作、流程/步骤、端口及执行状态等索引，不返回正文。列表可用 `execution_id` 限定来源执行；传 `output_id` 独立于搜索和分页读取同一范围内的单份封存产物和该生产执行计划冻结的 Delivery/Spec 定义。查询不写 Document、Artifact 或工作流状态，不重新验收交付物。同范围手工材料的 ID 返回 `{document}`。

`query.workflow_contexts({owner_id,project_id?,instance_id?,agent_instance_id?})` 返回 `{agents}`，按实际 Agent 实例组织当前工作根、Task 根和嵌套流程中的会话/任务轮次索引；不返回全部冻结任务正文。同名实例不合并；缺少身份的会话单独成组。每项保留 instance/execution/session/turn、归档/历史及正文可用标记。传 `session_id,snapshot_id` 只读取属于此范围会话的继承快照。传 `context_id` 返回同一工作范围当前手工背景的原始记录（`source:current_manual_context`），与冻结任务来源区分。

`query.workflow_executions({owner_id,project_id?,instance_id?,execution_id?})` 列表返回执行元数据及关联材料索引；指定 `execution_id` 返回确切执行、元数据与实际关联的决定、确认、派发意图和公开工具回执。历史执行只在当前选定流程的历史范围内读取。`query.workflow_record({owner_id,project_id?,record_id})` 读取同范围的计划/变更或关联事实；工具调用仅返回 `public_execution_receipt` 表示的公开回执，不返回内部运行时/权限请求。上述查询均只读，不建立业务状态投影。

`query.session({id,project_id?,event_id})` 返回属于指定 Session 的单条原始事件；其余参数沿用会话分页协议。`query.execution_task` 在提供 `project_id` 时核对冻结任务范围。原始 JSON 阅读窗口由 Web 的 `read` 路由按这些来源 ID 读取，不启动工作台轮询或执行命令。


`workflow.pause/resume/cancel` 控制实例。`workflow.step.input` 补充本次任务输入；`workflow.step.confirm` 提交确切 input_digest、outputs 及理由；`workflow.step.retry`、`workflow.step.rework`、`workflow.step.reconcile` 分别处理技术重试、业务返工和未知结果。所有命令都经过当前状态与来源校验。

模型内容报告使用 Spec 当前的任意 key：`approve`、`summary`、`checks[{key,passed,evidence,locations?}]`。拒绝保留原报告；普通 confirm 覆盖完成条件仍要求 Step 明确允许。独立的 `workflow.step.approve` 是显式人工例外入口，payload 必须提供 step、execution_id、input_digest、outputs 和非空 reason；允许 waiting_confirmation 或 awaiting_acceptance 且未请求停止的流程，保存 manual_approval 与 override_completion，绑定确切输入输出，不改写原结论或冻结配置。没有隐藏的固定四项检查。

默认 Task 的 implementation.evidence 使用 Execution Report delivery：runs 包含执行序号、阶段、命令、目录、起止时间、退出码、stdout/stderr，以及对应测试和源码的 path、sha256、content。缺少必需证据或仅提供临时路径会被输出 Schema 拒绝。后续 documentation、review、candidate_review 的 inputs.evidence 和 handoff 工具参数显式绑定此端口；原始内容沿现有 workflow_output / Artifact 存储，来源及 value_digest 进入规范任务。此交付是 Agent 记录，不能冒充 Test Report 的平台测试回执。

`conversation.send` 不能提交伪造的 task、frozen_task、task_instructions 或 task_source。Workflow 会话只接受来自当前冻结执行意图的输入；补充输入仍沿用原 Step 任务。会话或轮次派发失败及时成为步骤失败，子流程重试恢复对应父调用。暂停与人工等待不消耗活动预算。

普通讨论 Proposal v2 保存 `delivery_ref`、`title`、`value`、`input_refs`。`proposal.create/extract/revise/reject/adopt` 管理其修订与采纳；采纳只产生类型化材料，创建 Task 需要明确平台操作。

## 工具与工程资源

独立 Tool Step 通过冻结的工具定义、参数映射和执行身份调用。HTTP / MCP 工具使用明确接入及项目账户绑定。Codex Agent 平台工具还要求当前 Workflow、Capability、Step 与 Permission 的共同授权，绑定真实 Session / Turn / worker / 原生调用 ID；普通讨论不提供这些平台工具。

平台处理器包括 workspace.current/snapshot、materials.capture、tasks.create/complete、tests.run、git.candidate/advance/publish，以及受管 `project.*`、`configuration.*`、`spec.lint` 和 `workflow.*` 工具；完整注册表在 core/platform_tools。函数桥使用可移植的 `devdesk_` 名称映射，处理器标识不变。业务步骤、经理工作台和根控制会话按各自归属与冻结授权调用。DSH 当前不支持动态平台工具桥；配置声明不能绕过驱动能力校验。

`query.tool_calls` 同时返回实际外部调用与平台调用，保留真实 caller 和平台回执；按 Session、Turn、AgentID 等来源筛选。平台请求和终态回执不可改写，原生回复交付另存记录。`tool.call.cancel` 请求停止尚未结束的调用，未知副作用仍需核对。

工作副本和代码输入核对实际登记、owner、修订、路径、分支与流程来源。目录来自明确授权，不强制业务目录名称。测试只接纳实际固定代码与 Docker 回执；候选返回实际父提交及确切 diff。推进核对同一运行的真实候选、对应测试、确切确认与预期目标；发布继续核对远端 lease 和 PR merged 的确切 SHA。

## 项目、运行时与凭据

Web 使用 `project.open_github`，首次提交 repository、可选 mainline 和 title，再次可指定项目 target；接入意图、克隆及回执异步完成。`project.open` 切换已登记项目，`project.create` 保留为后端元数据入口。仓库实际就绪前不能启动工程执行。

`runtime.prepare({provider})` 准备当前默认 Harness 与 Sandbox 的组合镜像；`runtime.prepare_harness({id,project_id?})` 的 id 是确切 Harness 源引用，正常会话也可按需物化。构建/拉取在事务外执行，核验实际镜像后保留 managed 配方标签，保存固定 Image ID 的安装和协议报告；替换配方标签时按需归档旧产物，不再额外创建 prepared 标签。`runtime.preparation_status` 同时返回当前准备状态及持久化 intents；`runtime.preparation_cancel({recipe_digest})` 在检查点生效，`runtime.preparation_reconcile({recipe_digest})` 核对未知准备结果。运行和重试不按新标签替换原冻结安装。

协议探针只验证初始化及当前驱动实际支持的会话/动态工具注册，返回 `inference_verified:false`、`tool_execution_verified:false`，不证明模型、账户或工具调用已经成功。

`codex.auth.start/status/models/cancel` 管理本机登录与目录组件。`credentials.status/check/authorize` 只返回元数据、不透明引用及可用性，不读取显示秘密。系统凭据授权通过专用助手或宿主 Secret Service 完成，详见 [凭据说明](system-credentials.md)。

## Permission 运行时资源

`query.permission_resources` 返回 v2 profiles、绑定修订、确切目标版本、活动分配与来源。可携带 project_id 读取当前项目可见资源；省略时只读取全局目录。可选 target_ref 精确定位 Harness/Sandbox 修订，在当前目录外仍保留该目标及其绑定所需的权限修订历史；profile_ref 定位 Permission 修订并保留同一逻辑定义的修订来源，供历史链接查看。两者校验源类型、不可变修订与作用域；已停用目标仅返回不可应用的历史视图。v1 权限接口、预设与覆盖字段均已移除。

新增命令均使用 Command：`permission.profile.create/revise` 的 payload 为 identity（创建时）、name、target_kind、policy；revise 的 target 是原修订 ID，expected_revision 为原版本。`permission.profile.import` 接受 payload.profiles，批次原子；`permission.profile.reload` 只读取当前唯一预设目录，payload.builtin=true 时读取内置目录并在成功后恢复内置来源，失败不切换。`permission.source.select` 接受 payload.path，必须为存在的普通绝对目录；选择替换当前来源并持久化，需显式重新加载。`permission.source.browse({path?})` 返回 path、parent、directories 和 locations，仅列出普通子目录，省略 path 从数据目录开始，供文件夹选择窗口使用。`query.permission_resources` 的 source 返回 path、builtin_path、is_builtin；不再返回多来源列表或支持追加登记。

`permission.profile.export({id})` 返回 `{profiles:[前序修订…,所选修订]}`，可直接重新导入。导入只接受 `{type:"permission_profile",identity,revision,name,scope,data:{schema_version:2,target_kind,policy}}` 源定义；policy 的 null 和未知字段拒绝。

`permission.bindings.apply` 的 payload.bindings 为非空数组，每项包含 target={kind,revision_id,content_hash}、permission_revision_id、expected_revision（该目标尚无分配时为 0）。执行合同由 Host 生成，客户端不能决定。一个批次必须属于同一全局或项目配置库作用域；全部校验通过后原子提交。返回各目标的新活动 revision 与 binding_revision_id；command_id 重放优先于版本校验。

`permission.binding.preflight({target,permission_revision_id})` 只检查绑定兼容性，返回 scope_checked=false。完整预览输入 harness_ref、selection={sandbox_ref,harness,sandbox} 和可选 scope={work_type,writable_paths,read_only_paths}；selection 每层为 `{mode:"follow_target"}` 或 `{mode:"pinned",binding_revision_id}`。提供 Scope 时复用实际执行编译器；无 Scope 时不承诺具体工作范围可执行。

Agent 只保存 role_ref、capability_ref。项目保存 sandbox_ref，以及可选 harness_permission_ref/sandbox_permission_ref；步骤可直接选择两层 Profile，显式选择替换同层默认绑定。RuntimeBinding.permissions.binding_lock 固定实际目标、Profile 和绑定选择，持久化只保留确切源选择及执行事实。

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

`query.runtime_options` 只读取安装、Harness 安装组合和服务商凭据的配置状态，不构建运行时或调用模型。安装项标明 `execution_supported` 与用途；本机 Codex 用于登录和模型目录，Linux Docker worker 用于项目执行。设置和 Harness 的安装列表只展示项目执行用安装；登录/模型目录组件内部登记。`models.list` 对 DeepSeek 读取账号 `/models`，对 Ollama 读取本机服务，智谱与 MiniMax 使用所采用目录的容量数据，结果标明 `source` 和未验证的推理状态。

项目配置选择 Workflow、Standard、Role→Agent、Sandbox、可选双层 Profile 和驱动模式，不另设项目或角色运行时偏好。模型、连接、容量与参数来自绑定 Agent 的 Model 修订，Harness 与 Sandbox 组合并按内容摘要物化。有效参数冻结在 RuntimeBinding；每个 Agent Step 使用独立 Session，补充输入沿用该 Step 的冻结运行锁。

`integration.delete` 将接入源及所属工具、绑定从后续选择中删除，或单独删除已发布工具。操作原子检查在用引用，拒绝仍被 Capability、Agent、Skill 或项目配置使用的资源；修订和历史调用保留，紧急停用另由 `integration.disable` 处理。

全局角色讨论使用 `scope: "global"`，`session.open` 的目标为 `global`，不得同时提供 `project_id`。数据单独保存在 `conversations/global/engineering.sqlite3`；不创建 Project。Session 的 `scope` 为 `global`，其 `project_id` 列保留固定的执行命名空间 `00000000-0000-4000-8000-000000000000`，用于容器归属和 Session/Turn 外键；它不属于项目目录。全局 Session 仅接受全局 AgentID，消息、快照及经验仍按该身份保存。全局讨论中的方案须切换到具体项目后再进入工程采纳流程。

权限合同 v2 绑定权限语义、实际运行配方与策略资产；普通 UI、模型目录及非权限 schema 更新不等于权限语义变化。适配器内容身份在运行绑定中独立冻结，Feature 冻结确切权限闭包，不生成旧权限转换记录。

`feature.create.predecessor` 只记录同项目 Feature 的前后关联，代码仍从项目主干基线建立。界面枚举同项目所有 Feature，并明确显示这一语义。

`query.workflow_instance` 附带当前 TaskGroup 的 `children` 摘要（实例 ID/修订、任务名称、父步骤），子流程详细计划按实例单独读取。执行与交付以阶段为整行展示，编译展开的子流程保持在其调用阶段内；Task 子流程在所属阶段中按需展开，折叠状态按项目和工作流实例保存在本机。

`query.sessions` 可按 `agent_instance_id` 在分页前筛选。列表和会话详情返回该身份的关联元数据。独立讨论完成后保存 `conversation_completed` 快照，AgentID 历史包含本轮消息与真实终态证据；切换身份不继续沿用原会话。

## 编排与运行计划

项目和工作选择中的 `orchestration` 为 `{mode:"manual"|"rust"|"agent"}`。每个 Workflow 的 scheduling 包含并发、每轮动作、租约、技术重试和 agent 唤醒预算，随工作流和 Standard 冻结。`workflow.driver.select` 是人类命令，target 为根实例，expected_revision 为其当前修订，payload 只选择模式。它撤销旧控制代次和未派发控制意图，不停止业务 worker。`query.workflow_driver` 返回实际模式、冻结调度参数、epoch 与等待门禁。

`orchestration.open/input/takeover/resume` 管理项目经理及后继会话；`query.orchestration` 返回对话、实际控制调用和待人类决定请求。输入是用户自然语言，只有冻结 Capability/Permission 允许的原生工具调用才产生控制效果。

控制工具：`workflow.inspect/events.read/outputs.read/execution.inspect`，`step.dispatch/accept/input/retry/rework`，`execution.stop/reconcile`，`pause/resume/cancel/complete`，`change.preview/apply`，`materials.import`，`decision.request` 和 `segment.dispatch`（均带 workflow 前缀）。人类通过 `workflow.decision.resolve` 签署确切请求。写入绑定活动 plan_revision_id；接纳还绑定 execution_id、input_digest 及输出 ID，变更绑定预演哈希。

控制工具还包括 `workflow.start`。根/子实例派发和完成、Rust tick、人工直接命令遵守同一业务规则；有界 segment 不是独立根驱动。默认流程 automatic 确认不产生 Human 签署，显式 human 门禁、人工例外和授权内 waiver 分别记录。

`schemas/orchestration.schema.json` 定义类型化 ChangeSet。新增/修改步骤、依赖、输入、角色绑定及整体计划通过预演编译；应用期间根范围禁止新的写入派发，继续收集原运行事实。旧合同和结果保存只读历史。已派发 segment 只推进确切授权节点，遇到人工等待、评审拒绝、异常、预算或计划修改立即停止，不自动完成根实例。

编排封套 `orchestration-task/v2` 携带当前实例摘要、用户指令、策略及待决定项；持久化唤醒、消费游标、lease 和原生调用回执支持重启恢复。业务 `agent-task/v6` 携带计划修订、node_id 和 contract_digest。

业务任务的 `runtime_context.binding` 是当前 Session 的冻结资源选择，`runtime_context.inputs.<input>.runtime_executions` 是绑定上游真实 Turn 和停止证明的外层执行事实。渲染器也将同一执行事实附到 `input_sources.<input>.runtime_executions`，与该输入的来源引用/摘要一同提供。冻结选择不等于实际运行观察；公开运行投影不含凭据、模型连接参数或宿主可执行路径。

## 独立经理工作台

`manager.open` 的 target 是项目 ID，payload 可指定精确 `agent_ref` 与 Harness Profile 的 `harness_permission_ref`；它可以在没有 Feature 或使用 Rust 根驱动时建立工作台。`query.orchestration({owner_id:project_id,workspace_only:true,project_id})` 读取工作台；已建立 Controller 的输入、接管与恢复沿用 `orchestration.input/takeover/resume`。

经理原生工具包括 `project.overview/records.list/record.read/artifact.read/resources`、`configuration.catalog/read/usage/change.preview/change.apply/transfer.stage/transfer.apply/transfer.cancel/transfer.status` 和 `spec.lint`。概览只给摘要；列表与正文提供有界分页/分块、value_digest、next_offset。工具绑定固定项目，不接受任意 RPC、SQL、文件路径或改项目参数。已知凭据字段遮蔽；共享配置写入另需 `configuration.global.write`。

工作台没有根控制授权时不能派发根步骤。自身实例换绑/停用先交付回执、再撤销旧会话；人类采用后继配置时核对并停止旧 writer，不覆写原会话的授权。

源配置、权限归属及无持久化投影的存储规则见[源配置与执行解析](source-configuration.md)。

## Standard 快照接口

`standard.capture.preview` 只读解析确切 `root_workflow_refs`，`standard.capture` 核对候选并发布或复用不可变快照。`standard.archive` 只改变选择列表，不删除历史。`query.standard_catalog/standard/standard_diff/standard_usage` 返回按需视图及真实采用关系。规范差异与项目覆盖预览仅比较 Workflow、Delivery、Spec 定义及关联；默认启动入口元数据的缺省或不同不构成规范变化。

`work.configuration.update` 为尚未启动的工作选择 `standard_ref`、`workflow_ref`、执行者和权限；启动后使用 `WorkflowChangeOperation::AdoptStandard`，并沿既有 preview/apply、CAS、授权与静止等待路径切换。`query.work_configuration` 返回来源及计划历史，`query.work_standard_diff` 区分自身规范、入口投影及子工作采用。历史计划中的子调用清单以冻结的首次调用依据标记，当前调用树另行展示。
`query.work_standard_choices` 分页返回可访问工作及其采用依据；工作比较可跨项目，不切换活动项目。未运行工作以已选配置为依据，已运行工作以冻结计划为依据。Standard 对比同样可跨可访问目录，解析双方各自的源修订。


## 配置创作与跨库采用

| 方法 | 参数与结果 |
|---|---|
| configuration.change.preview | `{changeset:{operations:[...]}}`；返回 preview_hash、基准指纹、published_refs、影响与选择。执行资产状态明确为尚未探测。 |
| configuration.change.apply | Command.payload `{changeset,preview_hash}`；单库原子应用。project_id 指定项目库，缺省使用共享库。 |
| query.configuration_graph | `{id,project_id?}`；返回精确对象、类型化依赖、使用方及带字段路径的诊断。 |
| configuration.transfer.stage | Command.payload `{global_changeset,project_changeset}`，并指定 project_id；返回持久化暂存操作 ID。共享阶段只发布源修订，不混入共享停用/删除。 |
| configuration.transfer.apply | Command.target 为暂存 ID，project_id 固定目标；可重试/恢复，项目回执与最终源发布分别核对。 |
| configuration.transfer.cancel | Command.target 为仍处于 prepared 的操作；不删除历史，也不取消已经采用的内容。 |

操作结构见 rpc.schema.json 的 ConfigurationChangeSet/ConfigurationOperation。同一变更集允许先写消费者再写依赖；修订和停用均要求预期 revision。停用多对象时整体检查候选，不把正文中的引用样例作为依赖。目录暂存期间限制冲突配置写入，不能通过修改候选授权自己。

`query.spec_lint` 是只读结构预览；原生 `spec.lint` 受工作台的配置预演权限约束。新增 MachineAssertion/EvidenceLocation 契约见 RPC `$defs`；位置证明针对本次冻结输入，`workflow_output.spec_validation` 是内核生成的检查事实，模型不能通过输出同名字段替代它。

实现依据：[HTTP 与认证](../crates/host/src/main.rs)、[项目及运行时路由](../crates/host/src/workspace.rs)、[配置路由](../crates/host/src/governance.rs)、[RPC Schema](../schemas/rpc.schema.json)、[控制工具](../crates/core/src/controllers.rs)、[经理工具](../crates/core/src/manager_workspace.rs)。

`workflow.inspect` 每步附 `actions`，其 dispatch/accept/retry 使用 RPC `$defs/WorkflowActionEligibility`；`allowed_actions` 仅包含当前业务条件成立的动作，具体身份授权和 CAS 仍在命令入口检查。`query.workflow_driver.waiting` 返回相同动作资格与等待原因，查询不会派发或接纳。

StepExecution 可带 failure（WorkflowFailure）和 retry_of；workflow.inspect 的 retry_count 从同一实例、节点和 round 的历史执行推导。可信端口错误可附 ExecutionFailureDisposition，缺失分类时不推测为可自动重试。旧的文本意图错误只保留原文；新观察保存结构化错误，已记录终态只能幂等重报。

普通 Workflow Tool/Agent 的 spec.lint 仅接受 delivery_ref/value，目标必须是当前冻结步骤声明的输入或输出 Delivery；任意 specification/delivery 草稿参数只属于已授权经理预演入口。未授予该工具的业务步骤不能调用。输出为内容报告时，附 content_report_valid/content_report_error，复用接纳时的检查集合与冻结输入证据校验；草稿检查不生成 workflow_output 或人工决定。
