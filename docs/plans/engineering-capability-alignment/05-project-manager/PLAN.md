# S5：项目经理的全项目信息与配置创作能力

状态：待实施。依赖：S2 配置服务、S3 真实工具能力、S4 共同控制和驱动所有权；主体/用例边界来自 S1。[总体路线图](../PLAN.md)

## 1. 产品行为

人类可与选定项目的经理 Agent 持续沟通，查询所有有权访问的项目业务事实、创建/编辑/停用任何受支持配置、管理 Agent instance，并明确选择对未来工作还是当前实例生效。项目尚未创建 Feature 或正在使用 Rust 调度时，同样能使用经理。

经理是由 Role/Capability/Model/Harness/Skill/Tool/Permission 组合得到的普通 Agent 加项目工作台授权；不按“project-manager”名称、某出厂 Agent ID 或模型名授予特权。

## 2. 项目查询工具面

提供分页、分块、可追溯的工具外观，复用已有查询服务；以下为能力分组，实施不要求逐字采用工具名。

| 查询面 | 至少覆盖 |
|---|---|
| Project overview/list/search | 项目、Feature/Task、状态、依赖、当前模式与控制者、阻塞/待人事项 |
| Configuration catalog/get/history/usage | 全部配置类型、精确修订、正文、依赖/反向依赖、差异、停用与目录代次 |
| Agent instances/sessions | 项目实例、冻结 Agent 组合、角色、能力、会话/工具/运行时状态 |
| Workflow inspect/execution/outputs/events | 计划、所有子实例、输入、产物、真实回执、确认/豁免、变更和恢复 |
| Materials/conversations/history | 项目材料、允许读取的讨论/经验、关键决定及来源 |
| Resources/requirements | Harness/Sandbox/模型/工具/技能安装、真实能力、权限和资源缺口 |

每次结果返回记录引用、精确修订/摘要、cursor/截断情况和可续读位置。概览不能冒充完整项目状态；大图、大材料和长历史支持按引用续读，回答引用实际事实，不仅依赖会话摘要。查询不返回凭据秘密；可以返回“缺少某连接/认证”的业务状态。

项目范围由工作台绑定决定，客户端/工具参数不能随意切换成另一项目或全局管理身份。需要跨项目/全局配置时按明确的现有授权处理；没有授权仍可读可见依赖并提出请求，不能让全局修改暗中扩大范围。

## 3. 配置和实例写工具面

围绕 S2 的变更集提供 catalog/read、validate、change.preview、change.apply、status 查询；Agent instance 的创建/换绑/移除走其领域用例，由变更集可组合调用。Workflow draft/publish、Skill package import、Integration 发布、Permission profile/binding 等特有操作通过共同服务适配，不开放任意 `rpc(method, params)` 或直接文件/SQL 写入。

- 覆盖所有受支持可配置对象：Role、Skill、Spec、Delivery、Workflow、Standard、Capability、Agent、Model/Provider/Catalog、Harness/Sandbox/运行资源、Tool/Integration/Binding、Permission、项目/工作绑定，以及项目 Agent instance。
- 参数由内核注入真实项目、Actor、会话/轮次、工具调用身份和有效授权；变更 receipt 绑定原调用。相同调用重试不能重复创建实例或发布修订。
- 发布、默认绑定采用、运行中采用是独立明确操作，可按用户指令组合。授权范围内可直接执行，只有配置要求人工决定的操作才产生决定请求。
- Permission/Capability/控制策略变更不能自授权本次调用或让当前经理立即提权；提高权限使用其现有上级授权/真实人工决定，后继会话再生效。权限下降与经理自身改绑也须安全结束/交接原会话。
- “删除任意配置”定义为具有相应权限时执行合法的停用/换绑/移除，不是保证无条件物理抹除被历史引用的内容。被引用时返回使用方和可执行替代方案。

## 4. 五个必须打通的对话场景

| 用户表达 | 经理执行链 | 最终可观察结果 |
|---|---|---|
| “把现有流程增加一个评审环节” | 读取精确流程/相关 Spec/Delivery/权限 → 修改候选图 → 预演依赖/影响 → 发布 → 按明确范围采用 | 新修订可供未来使用；如要求当前运行采用，走停止/核对/新计划，不改原产物 |
| “在这个项目加一个 Agent instance” | 查询可用组合/资源 → 创建实例并必要时显式绑定角色 | 项目可见真实实例，工具、Harness 与权限合法；不凭名字自动授予调度权 |
| “改变某交付物的 Spec” | 读取当前 Spec/Delivery/消费者 → 创建 Spec 和必要消费者后继 → 预演/采用 | 未选择切换的消费者和历史执行仍固定原 Spec |
| “把某 Agent 的这个 Skill 改一下” | 确认目标 Agent/Skill → 新 Skill → 新 Capability/Agent → 选择受影响绑定 | 只修改指定消费者，不更新所有共享该 Skill 的 Agent |
| “删除这个不用的配置” | 查询反向依赖/活动使用 → 停用；有绑定时预演明确移除/替代 | 目录不可新采用；历史可读；失败说明实际使用者 |

如果用户未给足业务意图，经理使用人类输入工具补齐必要信息；已明确的目标、范围与授权不重复询问。自主创建草稿/预演不等于擅自选择对运行中实例生效。

## 5. UI 与配置交付

项目有统一经理会话入口，与当前 Feature Controller 身份/驱动租约区分；展示工具调用、候选差异、受影响对象、授权决定与真实应用结果。可从查询结果跳转精确配置/实例/产物。Rust 模式下可查询/编辑未来配置，接管当前调度需要显式切换。

更新出厂经理 Capability/Skill/Tool/授权配置来开放相应工具；代码不针对出厂身份特殊处理。编辑器表单和 Agent 工具都调用 S2 服务，局部提取共用类型与表单，不开展全前端搬家。

## 6. 工作项与验收

| 工作项 | 交付 | 验收 |
|---|---|---|
| M01 项目工作台会话与授权 | core manager workspace、Session、工具注入 | 无 Feature 时能查询/创作；Rust 驱动时经理可沟通 |
| M02 全项目查询工具 | 现有查询服务的有界外观 | 多 Feature/Task、分页目录/产物/历史无遗漏冒充完整 |
| M03 配置批量工具与实例操作 | S2/S1 用例外观、状态查询 | 五个场景均有真实 preview/apply/查询回执 |
| M04 授权决定与自改配置 | 共同授权、后继会话/控制交接 | 不能自举权限、代签人类、接受过期会话调用 |
| M05 项目经理 UI 和出厂配置 | Web、resources/configuration | UI 与 Agent 变更得到同一诊断；新出厂配置无 Rust ID 特判 |
| M06 真实验收与文档 | conversations/web-workbench/protocol、EXECUTION | 下面的故障/并发与真实对话证据完整 |

先用可控 Harness 测试工具参数/拒绝/幂等，再以真实已支持 Harness 完成五个对话场景。至少包含 UI 与经理同时编辑同一配置、预演后被他人停用依赖、跨库应用中断、经理会话失效后迟到调用、未授权全局修改。

必须核对数据库与应用查询、冻结引用和实际配置差异；模型回复“已完成”不是验收。真实模型措辞不作为逐字断言；结果、引用与业务不变量才是判据。
