# Permission 资源与运行时版本绑定实施计划

日期：2026-11-09。状态：已实施。macOS 自动化、Docker 双供应商原生边界、真实 Codex 运行时改绑和独立发布包验证通过；Ubuntu、DSH 真实模型传输及完整崩溃注入矩阵待补验。详情见 [实施记录](EXECUTION.md)。

将 Permission 建成独立、可修订的运行时配置资源：`permissions/` 中的预定义文件和 UI 使用同一种声明语言；UI 可以读取、编辑、保存、导入和导出；权限修订可以绑定到某个 Harness 或 Sandbox 的确切资源修订。更新绑定后，后续执行无需重启 Host 即可使用新权限；已有讨论在静止边界通过后继 Session 切换，保留原执行证据。

本计划承接 [Permission 标准化](../permission-standardization/PLAN.md)，并与 [Role 与 Capability 配置契约](../role-capability-project-configuration/CONTRACT.md) 共存。审核基于当前工作区，包含尚未提交的配置与工作流改动；实施时应保留这些改动，并重新核对接入位置。

## 一 审核结论

当前系统已有统一权限语义、不可变修订、原生配置编译、预检和冻结执行的基础。主要缺口是权限绑定缺少确切目标身份、不同执行入口采用不同权限来源，以及已有会话不能随绑定变化安全切换。不能只新增文件扫描和 UI 选择框便宣称实现运行时生效。

| 当前事实 | 对设计的影响 | 代码入口 |
|---|---|---|
| 六个预设通过 `include_str!` 枚举后播种 | 新增目录文件不会自动成为运行时资源，需要显式加载与导入机制 | [core/lib.rs](../../../crates/core/src/lib.rs) 的 `seed_configuration` |
| Permission 不含审批，审批在全局配置和 Agent 中分别保存 | 完整 profile 必须统一审批归属，并定义旧数据解释方式 | [domain/permission.rs](../../../crates/domain/src/permission.rs)、[domain/governance.rs](../../../crates/domain/src/governance.rs) |
| 全局绑定以 `codex`、`dsh` 为键，Sandbox 只有一项绑定 | 无法区分同一供应商的不同资源修订、构建与平台 | [permission_catalog.rs](../../../crates/adapters/src/permission_catalog.rs) |
| Agent 使用 `permissions[0]` 与最后一项分别作为两层权限 | 更改全局设置可能不影响新 Agent 路径，列表位置也没有明确类型 | [host/workspace.rs](../../../crates/host/src/workspace.rs) 的 `open_role_session` |
| 旧路径按绑定、项目、角色、本次执行覆盖，新 Agent 路径跳过这些覆盖 | 必须统一解析入口，明确追随目标与固定引用的区别 | 同上，以及 [resolve_permission](../../../crates/domain/src/permission.rs) |
| 新 Harness 使用不可变 `ConfigurationRecord`；旧 Harness 与 Sandbox 另有领域记录 | 需要明确目标修订的权威类型，不能把安装 ID 当成配置修订 ID | [domain/governance.rs](../../../crates/domain/src/governance.rs)、[domain/execution.rs](../../../crates/domain/src/execution.rs) |
| 能力表目前固定验证 Codex 0.167.0 和 DSH 0.2.2-rc.1 | 绑定不同版本不等于该版本已获支持，能力必须按执行合同核验 | [adapters/permissions.rs](../../../crates/adapters/src/permissions.rs) |
| 预检构造空读写目录，并未使用完整 Agent 组合 | 预检通过不能代表实际实施阶段可执行 | [host/permission_api.rs](../../../crates/host/src/permission_api.rs) |
| Session 的 RuntimeBinding 和原生 Session ID 均禁止原地替换 | 已有会话应创建后继 Session，不能覆盖旧绑定或直接续用旧原生线程 | [domain/conversation.rs](../../../crates/domain/src/conversation.rs) 的历史一致性校验 |
| 打开讨论可创建带 Snapshot 的后继 Session；发送消息仍使用现有绑定 | 可以复用继承能力，但需补充自动切换、旧 worker 静止核对与消息幂等 | [core/conversations.rs](../../../crates/core/src/conversations.rs)、[core/conversation_runtime.rs](../../../crates/core/src/conversation_runtime.rs) |
| Feature 已冻结配置闭包，Run 与 RuntimeExecution 保存权限快照 | 动态目标绑定不能绕过既有工程授权冻结与重试语义 | [core/governance.rs](../../../crates/core/src/governance.rs)、[core/project_configuration.rs](../../../crates/core/src/project_configuration.rs) |
| 审批交互处理没有基于冻结 `never` 再次拒绝的通用分支 | 必须补充 Host 兜底，不能完全依赖原生执行器不发审批请求 | [adapters/harness.rs](../../../crates/adapters/src/harness.rs) 的 `interaction` 与 `answer` |
| Codex 的旧受限配置存在明确的兼容转换和校验例外 | 新 profile 不得借 legacy 身份绕过工具规则支持检查 | [adapters/permissions.rs](../../../crates/adapters/src/permissions.rs) |

## 二 范围与生效契约

本次实施覆盖统一 profile、权限库、文件导入导出、确切版本绑定、统一解析、讨论切换、冻结审计、迁移和 UI。继续使用现有固定版本执行器；增加规则数据本身无需编译 Host 或重建镜像。修改执行器代码、内核策略或能力合同仍需要发布新后端版本。

| 操作或对象 | 生效规则 |
|---|---|
| 保存 profile | 追加不可变修订；不自动修改任何绑定 |
| 应用目标绑定 | 事务化切换目标的活动绑定；后续解析可以立即读取，无需重启 Host |
| 新讨论 | 使用创建时读取并冻结的绑定 |
| 追随目标的已有讨论 | 下一次提交输入前检查；无活动或待核对轮次时创建后继 Session 并切换 |
| 已接受的 Turn，包括排队 Turn | 保留接受时对应的权限快照，不在派发时追随全局变化 |
| 运行中或等待审批的 Turn | 保留原绑定；UI 显示待切换，不承诺即时撤销已发出的工具调用 |
| 固定到具体绑定修订的 Agent 或会话 | 不追随目标更新；UI 明确显示固定版本 |
| 已冻结的 Feature 工程配置、Attempt、Run 与技术重试 | 保持原授权闭包；采用新权限需显式形成新的授权冻结，不能通过目标默认值偷偷更新 |
| 回退 | 新建一次绑定变更，重新指向历史权限修订；不回写旧绑定或旧执行 |

第一版不提供任意规则脚本、通用表达式引擎、组织 IAM，也不把“每个命令必须审批”混入 `on_request`。Codex 通用工具白名单、网络域名过滤、任意读取拒绝等能力扩展不包含在本轮交付内；可表达但无法落实的规则必须拒绝绑定或执行，不能以配置已保存代替执行保证。

## 三 权限语言与资源身份

沿用声明式 JSON，新增有明确版本号的 v2 Schema。保留 v1 解码与历史表示，禁止通过给旧对象补默认字段而改变已保存的内容摘要。

新的 PermissionRevision 包含 `id`、`identity`、`name`、`version`、`supersedes`、`schema_version`、`target_kind` 和 `policy`。稳定 `identity` 表示同一权限定义；`id` 标识确切修订；保存只追加新修订。存储记录另保存规范化内容摘要和来源信息，文件与 UI 走同一解析、规范化、校验代码。

`target_kind` 为 `harness` 或 `sandbox`。使用同一种语言和版本机制，但按职责校验适用字段：Harness 策略包含 `filesystem`、`network`、`tools`、`approval`；Sandbox 策略包含 `filesystem`、`network`。Sandbox 上配置工具名或审批策略必须报不适用，不能静默忽略。复用的含义是一个 profile 可绑定多个兼容的目标修订，并不要求两个执行层支持相同维度。

v2 profile 保存该目标所需的完整策略，缺省不能解释为全权限；未知字段及策略字段的显式 null 拒绝。字段与列表上限在 Schema 和领域校验保持一致。旧 patch 的省略继承、显式替换及列表整体替换语义仅在兼容入口解释，不把它们混入新的完整 profile。

以下是拟定的 v2 Harness profile，最终机器 Schema 在 P0 固定：

```json
{
  "schema_version": 2,
  "id": "permission-readonly-review-v1",
  "identity": "readonly-review",
  "name": "只读评审",
  "version": 1,
  "supersedes": null,
  "target_kind": "harness",
  "policy": {
    "filesystem": { "mode": "read_only" },
    "network": { "mode": "enabled" },
    "tools": { "mode": "all_available" },
    "approval": "never"
  }
}
```

文件路径使用容器内绝对路径，例如 `/work/src`。沿用 `full_access`、`read_only`、`scoped` 及 `deny/read/read_write` 的语义，保留最长路径匹配与同路径拒绝优先规则。符号链接和真实路径边界依赖执行器或挂载保证，字符串匹配不是执行保证。

Harness 的 `network` 表示工具出站网络；Sandbox 的 `network` 表示整个容器网络。模型连接能否建立独立于工具网络能力检查。`tools` 中的名称必须能由目标执行合同识别；不能未经验证把 Codex 与 DSH 的不同工具名称当成等价操作。

`approval: never` 表示允许范围内自动执行，需人工授权的调用直接拒绝；`on_request` 表示允许执行器请求单次授权，不等同于每次调用必问。统一 profile 与 ExecutionScope 是最大授权范围，审批不得扩大这些范围。若原生审批通过“绕过沙箱”执行且无法继续强制维持最大范围，对应组合必须判为不支持，不能仅凭原生审批参数存在便宣称支持。

## 四 文件与 UI 的一致性

运行时权限库是已导入修订和绑定的权威来源。`permissions/presets/` 是可版本控制的预定义来源，`permissions/backends/` 保存固定执行资产；权限导入器不能把后端脚本当成用户策略执行。

1. 发布包和源码环境使用明确登记的预设根目录；保留嵌入预设作为无外部目录时的基础来源。数据集可登记独立预设根目录，具体来源在 UI 可见，不依赖进程当前目录猜测。
2. 启动时加载预定义资源，并提供运行时重新加载入口。文件修改通过“重新加载预设”导入，不需要重启；首版不引入文件监控，也不因目录变化自动应用新绑定。
3. UI 编辑内置条目时保存同一权限定义的新修订；内置原文件和原修订保持不变。UI 保存写入权限库，可导出同格式 JSON；不隐式改写源码或安装目录。
4. 同一修订 ID 且规范化内容相同，重复导入为幂等；同一 ID 内容不同，或同一 identity/version 对应不同 ID，整个导入批次返回冲突，不创建隐式 ID 别名。合法新修订校验 supersedes、版本连续性和父修订存在性，不能重排或覆盖历史。UI 与文件各自产生冲突的新版本时，要求显式解决版本分歧，不按来源优先级覆盖。
5. 批量导入先检查完整依赖闭包，再统一入库。导出提供所选修订及必要前序依赖，支持在空权限库往返导入；展示用来源信息不改变语义摘要。
6. RPC 接受结构化 JSON 或已登记来源的重新加载请求，不接受任意宿主路径读写。目录导入检查真实路径归属、链接与大小上限；文件导出通过下载完成，不把 UI 名称直接拼成宿主路径。

## 五 确切版本绑定

新增 PermissionBindingRevision 与 TargetPermissionAssignment，区分“不可变绑定内容”和“当前采用哪条绑定”。

| 对象 | 核心内容 |
|---|---|
| TargetRef | `kind`、确切资源 `revision_id` 及目标内容摘要 |
| PermissionBindingRevision | 自身稳定身份、修订 ID、版本、前序引用、TargetRef、PermissionRevision 引用及执行合同引用 |
| TargetPermissionAssignment | TargetRef、当前活动绑定修订 ID、用于并发更新的 revision |
| PermissionSelection | 明确选择 `follow_target` 或 `pinned`；pinned 引用确切绑定修订 |
| ResolvedExecutionPermissions | 两层目标与绑定、完整策略、来源、审批、Scope、编译器、执行合同、运行时与资产摘要 |

Harness 目标采用当前配置目录中的不可变 HarnessDefinition 修订。Sandbox 需要补齐同等的不可变定义与查询能力，并接入通用配置的类型校验、引用检查和存储索引；旧 Sandbox 记录作为兼容来源转换，原记录不改写。配置版本、供应商版本、安装 ID、镜像 ID、内核策略版本分别记录，不能相互替代。

绑定针对资源修订；实际执行还必须核验其供应商版本、构建配方、实际安装与镜像身份、OS/架构及所选内核执行合同。字符串 `codex@0.167.0` 不足以证明两个运行时构建等价。

活动绑定是目标的默认选择。新的配置明确选择追随目标或固定绑定；Agent 的 Harness/Sandbox 权限改用命名字段表达，不再依赖数组首尾位置。旧的 Agent、项目、角色、本次执行覆盖由兼容入口解释，不继续形成一条能绕过版本绑定的独立路径。

已有自定义权限不能迁移为一个影响所有 Agent 的全局默认。迁移时保持它们的有效策略并生成固定选择；用户之后可显式改为追随目标。新建配置默认追随目标，未分配活动绑定的目标可编辑、可保存，但执行前必须报未配置，不能自动落到“最新”或全权限。

每次绑定应用使用 command_id、完整参数指纹和 expected_revision。多个目标需要一同更新时，在全局库一个事务中校验并切换，避免只更新一半。目标修订不变时，切换权限不创建新的 Harness 镜像或 Sandbox 实现版本。

## 六 统一解析与能力校验

新增一个供绑定预检、UI 预览、讨论、工程运行共用的纯解析核心。输入包括目标修订、PermissionSelection、活动绑定快照、权限修订、执行合同、平台信息及实际 ExecutionScope。兼容入口先把旧数据转换成明确请求，再进入同一个核心。

解析按以下顺序进行：解析确切引用，检查作用域与适用字段，展开策略，核验后端能力及相互约束，结合工作范围形成执行计划，规范化并计算摘要。每项规则标明执行者；执行计划中不能存在必须执行却无人落实的规则。

预检分两层：绑定时检查目标、profile 和执行合同的结构性兼容；执行前结合真实工作类型、读写目录和平台就绪状态检查。无 Scope 的绑定检查返回“仍需执行范围检查”，不能伪装成完整可执行结论。预览不创建容器、不获取模型凭据、不发模型调用，也不能成为客户端提供执行授权的入口。

当前能力边界来自仓库适配器；实施时必须对固定版本重新核对，不能作为对供应商未来版本的断言：

| 规则 | Codex 当前适配 | DSH 当前适配 | 本轮要求 |
|---|---|---|---|
| 文件全权限与只读 | 原生配置，受外层挂载限制 | 原生 preset，受外层挂载限制 | 支持组合使用真实执行验证 |
| `/work` 内限定写 | 原生命名 profile | 仅接受能够由冻结挂载落实的范围 | 拒绝 DSH cwd 隐含扩大写权限的组合 |
| 任意读取 deny | 常规路径不支持，存在 legacy 专用例外 | 常规路径不支持 | 不把 legacy 例外开放给新 profile |
| 工具禁网 | 原生权限配置，并关闭 web search | 受限文件模式、受控工具集合与 bwrap runner | 模型连接可用与工具禁网分别举证 |
| 网络 allowlist | 不支持 | 不支持 | 拒绝；后续有强制代理执行器后另立能力版本 |
| 通用工具 allow/deny | 不支持 | `tool-policy.mjs` 在 restricted 时执行 | Codex 拒绝，不用工具事件日志代替拦截 |
| 审批 | 原生参数 | 原生服务与扩展 | 增加 Host 兜底，重新验证最大授权范围 |

Docker 不解释工具名。Harness 容器整体禁网会影响模型连接，应报冲突；独立无凭据测试容器仍可禁网，不得将该冲突规则错误套用到测试执行。

错误至少区分无效语法、引用不存在、版本冲突、规则不适用、能力不支持、权限组合冲突、环境未就绪与实际拒绝，并指出目标、字段、原因及处理方向。新的权限不能自动降级成 Sim、全权限或一个看似近似的原生 preset。

## 七 运行时切换与冻结边界

全局活动绑定切换的提交事务是后续解析的生效点。单次解析读取同一个绑定快照，冻结后使用确切引用；不能在生成配置或派发时重新读取“最新”。全局库和项目库不假设跨库事务，项目只追加所需不可变资源副本，使用摘要核对并持久化切换意图处理恢复。

新讨论在 `session.open` 冻结两层结果。已有讨论在接受下一条输入前检查追随目标的活动绑定，不能等到 worker 派发后再改变权限。若有变化，采用现有后继 Session 机制：

1. 校验 Session revision，确认没有运行、排队、等待交互或待核对的 Turn；有则返回待切换状态。
2. 核验新的完整权限与 Scope，通过后保存切换意图，记录旧 Session、预期绑定、新绑定和 command_id。
3. 对旧 worker 核对静止与停止回执，撤销旧 writer；无法确认时进入待核对，不能启动并行的后继 writer。
4. 捕获讨论 Snapshot，创建新 Session 并保存 predecessor，继承业务上下文与 Agent 身份，冻结新权限。
5. 使用新 worker 和新的原生会话身份；旧 config、mount 或原生 thread/resume 状态不得将旧权限带入新会话。
6. 在新 Session 接受原输入，返回 effective_session_id、Turn ID 和 adopted binding；UI 继续显示同一讨论脉络。

以上步骤使用可恢复的持久化意图与回执，不在数据库事务中等待外部进程。幂等范围覆盖原 command_id、client_message_id、后继 Session 和 Turn；相同请求重试不得生成多个会话或重复发模型输入。切换期间新的绑定变更要么返回版本冲突，要么在后续一次切换采用，不能混用两批规则。部分步骤失败不得自动回退到旧权限继续执行。

原来已接受的输入保持原冻结；下一次输入切换会话不改变旧 Turn、Snapshot、RuntimeExecution 的证据。旧审批与单次授权不能转移到后继 Session，旧 Session 的输入重放仍按原幂等记录返回，不得误当成一条新输入。

工程执行继续遵守已冻结的 Feature 配置。追随目标的选择必须在新的工程授权冻结时展开成确切绑定、profile 与执行合同，并进入配置闭包和摘要；已有冻结 Feature 通过现有允许的显式后继或配置更新流程采用新权限。Attempt、Run、失败重试与恢复读取冻结结果，不读当前活动指针。

若需要立即停止正在运行的旧权限，沿用明确取消与静止核对流程后再切换。本轮的“应用绑定”不自动取消用户工作，也不承诺撤销已完成的外部副作用。

## 八 审批与执行器接入

Host 在接收原生审批请求与发送审批回答两处，校验冻结审批策略、Session/Turn 归属、请求是否仍有效及授权范围。`never` 下的意外请求直接拒绝并留下可观察原因；旧 Session 已停止或切换完成后到达的旧回答不能被采纳。仅有目标活动绑定更新、原 Turn 尚未切换时，仍按该 Turn 的原冻结策略处理审批，不能用新默认值改写正在执行的授权。

Codex 继续使用编译后的命名 profile、全权限路径及 App Server 参数；DSH 继续使用 preset、sandbox-policy、approval 与受控扩展。两者读取同一份冻结结果，不在脚本内维护第二份规则。新 profile 的 legacy 特例不得仅由用户可提供的字符串身份触发。

仅修改策略数据时，生成新的只读配置并按新运行身份启动，无需重建供应商镜像。新增工具拦截器、修改 bwrap 或 Docker 内核策略时，更新执行合同、manifest、摘要与构建身份，发布新后端，保留旧证据；找不到匹配执行器的历史重试应明确阻止并引导后继执行。

## 九 存储与协议

新增 profile 元数据、绑定修订、活动分配和切换意图的持久化表示。使用独立类型或现有不可变配置 envelope 时都要有明确约束，不能把绑定信息塞进一个无校验的 settings JSON。追加全局与项目 SQL 迁移，编号以实施时最新迁移为准，不改写已发布迁移。

协议在 P0 固定，建议覆盖以下操作，并保持现有入口的兼容适配：

| 接口职责 | 要求 |
|---|---|
| 查询权限 | 修订、来源、目标适用性、摘要、引用关系 |
| 创建与修订 | 统一 Schema，command_id 幂等，基于最新修订的并发校验 |
| 导入与导出 | 同格式、依赖闭包、批次原子性、冲突诊断 |
| 重新加载预设 | 仅登记目录；返回新增、相同、冲突项，不改变活动绑定 |
| 查询目标与绑定 | 真实资源修订、平台与执行合同、活动修订和引用使用者 |
| 绑定预检与应用 | 区分结构性兼容与实际 Scope；支持一批目标原子切换 |
| 会话切换状态 | desired、frozen、pending、adopted、failed、needs_reconciliation，按会话查询 |
| 执行审计 | profile、绑定、目标、运行时与执行合同的确切身份，解析来源和拒绝原因 |

停止使用仅按 provider 选目标的写入口作为新配置的权威。历史 `permission.bind`、项目与角色覆盖入口可以保留兼容适配，但不能写出与版本绑定矛盾的新状态。所有旧入口如何转换和弃用都必须写入 RPC Schema 与协议文档。

## 十 UI 行为

设置中的 Permission 页面提供“权限定义”和“目标绑定”两种操作。权限编辑器支持统一 JSON 与字段诊断，明确区分保存新修订、预检和应用；导入导出沿用同一格式。保存后显示引用仍固定在哪个旧版本，不把保存提示写成已对所有会话生效。

目标选择按 Harness/Sandbox、资源、确切修订展开，显示供应商版本、构建与平台信息。只列真实存在的目标，未安装或未经能力验证的状态单独展示。选择 profile 后显示每项规则由哪个执行器落实，以及哪些工作范围仍需执行前检查。

Agent 与会话页面显示命名的 Harness/Sandbox 选择及追随或固定状态；列出目标绑定变更影响到哪些新执行和已有讨论。运行中的会话显示当前冻结版本和待采用版本。切换成功后跟随 effective_session_id，保留前序记录；失败和待核对状态不得用“已应用”遮盖。

## 十一 兼容迁移

1. v1 Permission、RuntimeBinding、Snapshot、Run 及旧 manifest 引用保持可读，保留原始语义与内容身份；不原地补字段或替换 ID。
2. 将旧 Permission 与全局、Agent、项目和角色审批来源一起解析，必要时派生独立 v2 Harness/Sandbox 修订。相同旧 profile 搭配不同 approval 时不能被合并成错误的同一新修订。
3. 对各配置上下文保留当前有效行为；同一目标存在不同 Agent 权限时生成明确固定选择，不选择其中一个作为全局默认覆盖其他 Agent。迁移记录可追溯原来源并可幂等重跑。
4. 历史工程冻结引用保持固定。新运行无法证明旧身份与新执行器兼容时标记待处理，不伪造等价配置或自动继续。
5. 老 UI 与 API 写入经同一兼容适配器校验；禁止一边写旧全局对象、一边让新 Agent 继续忽略它。
6. 发布包保留预定义资源、两代 Schema 与必要后端资产；无源码目录时可运行。数据库升级后不承诺旧二进制可直接打开，回退流程采用备份恢复或兼容版本。

若 v1 的审批升级等行为无法证明与 v2 最大授权范围等价，保留其固定的兼容记录并标记未转换；既有冻结执行继续按旧合同核对。不能假装已转换成功，也不能擅自把 on_request 改为 never。新 v2 授权须通过当前能力校验后才能派发。

## 十二 实施阶段

### P0 固定契约与审核回归

- [x] 固定 v2 profile、目标、绑定、活动分配、选择方式、执行快照与错误分类。
- [x] 固定审批最大范围、目标追随与工程冻结的关系，以及后继 Session 切换状态机。
- [x] 保存旧数据与双入口行为的回归用例；确认与配置重构工作区的接入点。

完成条件：一个权限从文件导入、UI 修订、绑定应用到讨论与工程执行的身份和生效时间都有唯一解释。

### P1 权限资源与文件交互

- [x] 实现统一解析、规范化、修订存储、来源、摘要及幂等。
- [x] 实现登记目录加载、运行时重新加载、批次导入与闭包导出。
- [x] 保留 v1 与嵌入预设，新增文件无需改 Rust 枚举即可运行时导入。

完成条件：文件和 UI 对同一内容得到同一规范结果，往返不丢信息，重复导入幂等，冲突不覆盖。

### P2 目标与版本绑定

- [x] 补齐 Sandbox 修订资源，并明确旧 Harness/Sandbox 与新目录的转换。
- [x] 实现不可变绑定修订、目标活动分配、follow_target/pinned 选择及批次 CAS 应用。
- [x] 将权限引用改为命名字段，消除 Agent 数组位置和 provider 默认值的独立权威路径。

完成条件：同一供应商不同资源修订可独立绑定；更换绑定无需改目标或镜像；旧配置行为不被全局合并。

### P3 解析编译与审批

- [x] 绑定预检、完整 Scope 预检和执行共用解析核心，能力按执行合同与平台区分。
- [x] 接入 Codex、DSH 和 Docker 编译；每项规则都有执行者，拒绝不支持的组合。
- [x] 增加 Host 审批与回答兜底，封闭新 profile 借 legacy 例外绕过规则的路径。

完成条件：相同输入的预览与派发摘要一致；不支持的规则在模型调用前拒绝；审批不能扩大最大范围。

### P4 新执行与工程冻结

- [x] 新讨论从绑定解析结果冻结；全局修订按需同步到项目，不回写历史。
- [x] 新工程授权冻结记录 Permission、Binding、Target、执行合同闭包；技术重试保留原身份。
- [x] 审计查询区分选择来源、活动目标和实际冻结结果。

完成条件：运行时更换绑定影响新授权执行，已有工程锁与重试不追随更新，Host 不需要重启。

### P5 已有讨论切换

- [x] 接受新输入前检测差异，持久化切换意图，核对并停止旧 writer，建立带 Snapshot 的后继 Session。
- [x] 跨后继 Session 保持命令与消息幂等，返回 effective_session_id；旧审批与旧原生线程不复用。
- [ ] 完整覆盖切换各阶段进程崩溃与并发压力矩阵；已覆盖未知停止结果、持久化意图恢复、后继原子提交、命令/消息去重、队列门禁、CAS 和恢复时再次改绑，范围见实施记录。

完成条件：已有讨论下一轮实际使用新限制；无双 writer、重复输入和失败后隐式退回旧权限。

### P6 UI 与协议迁移

- [x] 实现权限编辑、修订历史、文件交互、版本目标选择、预检与应用。
- [x] 显示固定或追随、当前与待采用版本、切换状态；接入有效 Session 跳转。
- [x] 完成旧入口适配、追加 SQL 迁移、JSON Schema、RPC Schema 与文档。

完成条件：用户可以完整完成“编辑、保存、绑定、发送下一轮、查看采用证据”，且提示与实际生效范围一致。

### P7 验证与发布检查

- [ ] 完成下列全部平台与故障矩阵；相关 Rust、协议、Web、真实 Codex、双供应商原生工具和发布包检查已通过，其余项见实施记录。
- [x] 在可用平台执行真实供应商工具允许与拒绝验证；环境缺失的项明确保留待验状态。
- [x] 写入 EXECUTION.md，记录实际命令、结果、平台、固定版本及未覆盖范围，更新计划状态。

完成条件：文件和 UI 修改不重启 Host 可生效，行为证据与冻结身份一致，历史读取、恢复与发布包回归通过。

## 十三 验证矩阵

| 场景 | 验证要求 |
|---|---|
| 文件与 UI 一致 | 同内容摘要相同；编辑保存产生新修订；导出闭包可在空库导入 |
| 文件运行时更新 | Host 启动后新增预设，重新加载可见；不编译、不重启、不自动绑定 |
| 导入冲突与原子性 | 相同修订不同内容、缺父修订、重复 identity/version、非法字段均失败且不部分入库 |
| 确切版本 | 相同 provider/version 的不同资源修订和构建不能串用；未验证平台明确拒绝 |
| 并发应用 | 两个相同 expected_revision 的修改仅一个成功；重放同 command_id 返回同结果；多目标原子应用 |
| 固定与追随 | 新配置追随活动目标；固定选择不变；UI 显示实际差异 |
| 真实 Scope | 预检使用正式读写范围；只读与实施冲突、DSH cwd 扩大授权均拒绝 |
| 文件读写 | 授权路径真实写入成功，越界真实失败；受保护元数据和测试目录边界不变 |
| 工具网络 | 受控本地 HTTP 收据证明允许或拒绝；禁网时模型通信仍成功，覆盖 SDK 其他出站入口 |
| 能力不支持 | Codex restricted、域名 allowlist、未支持 deny 等在执行前拒绝，无模型请求副作用 |
| 审批 | never 下意外请求和回答都拒绝；on_request 的单次授权不越过最大范围；旧审批不能移交 |
| 新执行生效 | 改绑前后新执行采用不同确切快照，并出现相应允许与拒绝行为 |
| 讨论切换 | 空闲时产生后继与新原生身份，继承上下文；运行、排队、等待审批时显示待切换 |
| 切换故障 | 在保存意图、停止回执、创建后继、接受输入及派发边界注入故障，恢复不双写、不重复发送 |
| 切换并发 | 改绑与 send 竞态只有一个明确采用结果；客户端 CAS 与重试返回同一有效 Session/Turn |
| 工程冻结 | 已有 Feature/Attempt/Run 和重试保留旧锁；显式新授权冻结才采用新绑定 |
| 迁移 | 旧全局与 Agent 不同审批、项目/角色覆盖、legacy 专用规则均保留行为和原始证据 |
| 发布与恢复 | 源码外包加载预设、导入、绑定和执行；重启保留活动绑定与切换意图，后端漂移明确阻止 |

真实权限测试采用实际工具结果、文件状态和网络收据，不依赖模型自述；协议 fixture 用于故障、幂等和 UI 合同验证，不能替代真实权限保证。macOS Docker 与 Linux AppArmor 分别记录平台结论，当前历史报告不自动视为本轮通过。

## 十四 预计改动范围

| 范围 | 主要文件或模块 |
|---|---|
| 权限领域与兼容 | `crates/domain/src/permission.rs`、`governance.rs`、`execution.rs`、`conversation.rs` |
| 资源与绑定存储 | `crates/adapters/src/permission_catalog.rs`、`catalog.rs`、追加全局与项目 SQL |
| 统一编译与能力 | `crates/adapters/src/permissions.rs`、`harness.rs`、`docker.rs`、后端 manifest |
| 命令与工程冻结 | `crates/core/src/permissions.rs`、`governance.rs`、`project_configuration.rs`、`conversation_runtime.rs` |
| Host 解析与切换 | `crates/host/src/permission_api.rs`、`workspace.rs`、相关配置解析模块 |
| UI | `PermissionSettings.tsx`、`AgentCatalog.tsx`、`RoleConversation.tsx`、必要的共用类型与状态 |
| 预设与协议 | `permissions/`、`schemas/permission.schema.json`、绑定与 profile v2 Schema、RPC Schema |
| 验证与发布 | 权限、会话、配置 Rust 回归，HTTP 合同，Web 测试，独立运行包与真实工具探针 |

实施顺序为 P0 → P1 → P2 → P3 → P4 → P5 → P6 → P7。各阶段先保留兼容入口，再切换调用方；不得在解析、存储和 UI 只接通其中一部分时宣称运行时绑定已完成。
