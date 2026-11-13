# 当前实现与工程原则的差距

日期及基线见 [总体计划](PLAN.md)。以下代码定位对应分析时工作区，行号会随改造变化，应结合函数名查找。严重性表示对本次能力要求的影响，不把缺少能力一概称为运行 bug。

## 1. 已经成立的基础

- `crates/{domain,core,adapters,host}/Cargo.toml` 的依赖方向正确：domain 不依赖其他层，core 只依赖 domain，adapters 依赖 core/domain，host 依赖三者。没有发现用户举例的 domain → core 反向依赖。credential-helper 是独立平台宿主程序，最终应明确归入 host 职责。
- 检查 domain/core 的 `std::fs`、进程创建、文件读写等使用，domain 没有发现文件/网络/进程 I/O。core 源码的主要问题也不是直接调用系统进程，而是资产、模型和应用职责的归属。
- 这不表示 domain 完全确定：`domain/lib.rs:75` 的 `id()` 调用 `Uuid::new_v4()`，直接依赖随机环境。严格的纯领域边界应只接收已分配身份，把 ID 分配移至应用边界；同样给 core 的时钟建立可控制的来源，便于调度/恢复验证。
- `domain/source.rs:18–40` 支持规范的 `identity:revision` 引用；`domain/governance.rs:339` 的 `validate_governance` 约束历史不可删除、不可变记录不可改写和内容摘要一致。修订号正整数检查和引用相等是必要的身份校验，不是禁止的数据版本门禁。
- `core/configured_workflows.rs` 的 `compile_workflow_sources`、domain 的 Workflow/CompletionRule、Standard snapshot 已有真实的类型、DAG、端口、作用域、来源和闭包规则。默认 TDD/阶段名称并未被普遍写成内核专用流程。
- `core/ports.rs` 已定义 Harness/Git/Tests/ToolExecutor；控制器有 epoch/lease，外部动作有持久化意图、幂等与回执接纳。这些机制应继续共用。
- 源信封扁平化工作区已经清理很多旧版本断言。不能直接沿用 P12 的旧问题数量，把已经消失的 `schema_version == 6` 当作当前缺陷。

## 2. 原则一：物理依赖合格，逻辑职责尚未收敛

| 事实与代码依据 | 如何偏离要求 | 处理 |
|---|---|---|
| `core/build.rs:1–67` 读取仓库配置和 schema、复用 `tests/support/schemas.rs`，生成嵌入源；`core/templates.rs:10–112` 载入清单、解析 Markdown 资源路径并持有全局 Bundle | core 编译依赖具体出厂目录、构建打包和测试基础设施。新出厂资产使应用内核重构建，复用 core 时仍携带整套产品默认 | S1 将资源打包/载入移到外层；core 接收类型化 bundle；保留独立构建期资源验证 |
| `host/workspace.rs:59–126` 的 `template_permission_defaults` 决定默认权限绑定；`host/governance.rs:164` 的 `resolve_agent_composition` 解析角色组合、配置可见性与冻结例外；`host/workspace.rs:132` 的 `open_role_session` 大量拼装业务绑定 | Host 除装配/传输外还决定业务选择和合法性。新增 Agent/API 入口必须复制这些规则 | S1 将选择、验证、绑定接纳移入 core 用例；实际镜像/权限编译仍走 adapter port |
| `host/governance.rs:46–110` 直接写入 Snapshot/Event/经验导入事实；`host/templates.rs:98` 直接打开事务组织目录重置；`host/runtime_setup.rs:131` 直接持久化物化回执 | 多个业务变更入口绕过统一 Engine 命令/Actor 上下文；不能据此断言全都错误，但难以保证 UI 和 Agent 行为一致 | S1 共同命令入口、回执用例与 Actor；Host 不直接写业务状态 |
| `adapters/source_views.rs:397` 调用 `render_source_task`，`:645` 调用 `compile_workflow_sources(None, ...)`，为重建冻结选择临时构造权限/集成治理记录；`adapters/lib.rs:153` 的 `load_with` 在数据库读取中恢复这些视图 | adapters → core 的编译边合法；问题是持久化读取过程执行应用编译和任务渲染，并混合冻结解释。读库结果耦合当前解释器与 `None` 编译路径 | S1 把纯 codec 留 Adapter，冻结描述解释/派生重建移入 core；不声称当前已发生历史数据损坏 |
| `domain/template_catalog.rs:80–139` 包含 RPC 方法名与读写命令分类 | domain 的目录规则混入应用命令和协议路由知识 | S1 将方法注册/命令分类放 core/host，domain 保留目录成员和修订不变量 |
| `core/src/lib.rs:44–90` 定义 Store/Artifact ports，其他执行 ports 在 `ports.rs`；`configured_runtime.rs` 约 2957 行，platform_tools/orchestration/workflow_changes 等文件很大 | 边界表达不统一，审查和测试困难；大文件本身不是反向依赖或原则违背 | S1 按用例和领域拆模块；不为了行数创建跨层公共杂物 crate |

## 3. 原则二：源配置可演进，但运行时和选择界面仍形成限制

### 3.1 尚存的实质性门禁

1. **显式版本限制**：`core/governance.rs:460` 对 Sandbox 要求 `runtime_version == "1"`、`enforcement == "docker-worker/v1"`；`adapters/permission_profiles.rs:41–50` 重复该限制，并返回 `docker-worker/v2:<digest>`。两种字符串可能分别表示源执行合同和权限编译身份，不能只因 v1/v2 不同判定 bug；应拆出明确类型，并移除无关运行版本限制。
2. **摘要替代了版本白名单，但可选集合仍固定**：`adapters/runtime_preparation.rs:8–43` 固定 Codex/DSH 版本和嵌入 Dockerfile；`host/runtime_setup.rs:111–125` 只物化等于当前内嵌配方的 Harness；`adapters/permissions.rs:408–426` 仍将所选安装配方与当前程序能力表的唯一配方摘要比较；`permission_profiles.rs:24–33` 同样限制权限合同。配置可以写另一个版本标签，但不能独立提供另一个兼容实现。违反的是选择能力，不是“摘要校验”本身。
3. **宿主登录组件仍按固定版本寻找/安装**：`runtime_preparation.rs` 的 `prepare_codex_auth` 比较 `CODEX_VERSION`，`native_release` 将平台下载摘要绑定到代码。默认可信安装来源可以有固定 pin，但应是外层资源目录中的可选条目，不能作为所有运行时兼容性判据。
4. **历史修订可查询，主动选择不完整**：`web/src/components/configuration-ui.tsx:25` 的 `latest()` 和 `:202` 的 `RevisionSelect` 默认只展示每身份最新版，再补已经选中的旧版；即使 `loadCatalog` 请求了历史，也无法主动选择所有其他旧修订。后端 `query.configurations` 已支持 `include_history`，这是界面缺口，不是存储能力缺失。

### 3.2 应保留且不能误删的检查

- `expected_revision`、`control_version`、catalog generation、plan revision、epoch 防止并发覆盖和旧调用越权。
- 内容哈希、镜像 digest、冻结绑定一致性、实际资产校验防止所选资源被替换。S3 改的是“与所选内容比较”，不是取消核验。
- Serde 结构、字段白名单、未知原语拒绝、SQL 基线验证分别维护数据格式和实现边界。“不检查版本”不等于接受代码不会执行的新算子、新协议和任意字段。
- `WORKFLOW_INSTANCE_SCHEMA`、`AGENT_TASK_RENDERER` 等仅写入记录元数据，不应一律当作违规。`core/governance.rs:751` 仍有“当前 Workflow v6”错误文案，属于误导性残留，应改为结构/引用错误，而非重建版本注册表。

## 4. 原则三：已有抽象，但规则和外部资源组织没有统一应用边界

| 差距 | 当前依据与含义 | 改进 |
|---|---|---|
| 配置验证分散、依赖发现不统一 | `domain/source.rs:95` 有根级引用白名单与形状检查；`validate_source_data` 对各 type 的深度不同，workflow 入口并非完整编译；`core/governance.rs:356`、`configured_workflows.rs:83`、`configuration.rs`、integrations/permission 专用模块继续分支处理；templates materialize 又分别调用验证。Adapter 的事务校验仍在兜底 | 保留专门的领域校验器，以 S2 统一管线组织，不能把“重复入口”错误描述为“完全没有校验” |
| 依赖图逻辑存在多份 | Standard 的显式关系/快照已有价值；`core/configuration.rs:161` 的 `references_value` 与 `adapters/source_views.rs:20` 的 `source_closure` 又递归扫描 JSON 字符串。普通业务字符串与真实引用容易混淆，不同用途可能遗漏不同边 | S2 明确有类型的边；用于闭包、影响、删除预检与诊断；不把现有所有数据图强行变成 DAG |
| 关联配置缺少统一批量事务 | 创建/修订以各类型命令为主；编辑 Skill 常需新 Capability、Agent 与项目绑定，Workflow/Standard/PermissionContext 也可能需协同发布。单个命令原子不等于整个配置变更原子 | S2 变更集在候选完整状态上验证，整体提交；跨全局/项目数据库明确分步意图和可见性屏障 |
| 真实能力被厂商名代替 | `core/configuration.rs:107,112`、`controllers.rs:214`、`configured_workflows.rs:198` 判断 provider 为 codex 才能使用外部/平台工具或编排。现状是在保护真实工具桥缺口，但策略写在 core 厂商判断中 | S3 用“配置需求 ∩ 实际安装/适配器能力 ∩ 权限”判定；配置声明不能自行证明能力 |
| 资源版本与权限资产耦合过紧 | `permissions.rs::capabilities`、`permission_profiles.rs::permission_semantics_digest` 将运行配方、权限资产和实现身份绑定；换兼容运行时仍需改程序并重新认可整个摘要 | 分离协议驱动、运行资源、权限后端资产和验证事实，保留实际权限执行的强约束 |
| Spec 目前主要是指令和模型报告 | `domain/workflow.rs:254–315` 的检查定义只有 key/name/instructions，报告校验保证 key 完整、evidence 非空和 passed/approve 自洽；不能证明证据真的定位到产物，也不能证明文档语义正确 | S6 增加共享可执行检查原语与结构化 evidence；人/模型判断继续独立保留，不夸大“确定性验收” |

## 5. 原则四：有 Agent 编排基础，缺完整 Rust 模式和项目管理面

### 5.1 现有调度准确边界

`core/workflow_runtime.rs:4` 的 `evaluate_workflows` 进入 `configured_runtime.rs:1847`，负责观察执行、处理取消/超时和推进已授权工作；`:2109` 要求 queued 步骤已有 `workflow_dispatch`，`:2097` 对 WaitingConfirmation/AwaitingAcceptance 直接返回；根完成还依赖 `workflow_completion`。因此后台 tick 自动执行，不等于完整的 Rust 自动调度。

`core/workflow_changes.rs:1152–1230` 的 segment 能自动派发和接纳**显式授予的有限步骤集合**。这是可复用的局部驱动，不能据此宣称没有 Rust 自动化；但还没有项目/Feature/Task 层可选的完整 Rust 驱动、统一所有权与切换机制。

`controllers.rs` 已有持久经理会话、唤醒和 21 个控制工具，执行实际派发/接纳/变更。`WorkflowActor` 当前只有 Human、Controller、Segment，尚无明确的通用自动驱动身份。S4 应扩展共有控制面，而不是再建一个各自判定终态的执行器。

### 5.2 项目经理的创作和信息范围不足

- `CONTROL_TOOLS` 集中于 workflow 实例；默认经理 Capability 也只持有这 21 项控制工具及 native 工具。没有项目配置、Role/Skill/Spec/Delivery/Capability/Agent/Harness/Sandbox 等通用的配置查询与发布工具。
- `controllers.rs` 的 `workflow.inspect` 在没有根实例时只返回项目配置与 start 提示，有实例后主要返回实例、执行和产物。不能覆盖项目全部 Feature/Task、配置依赖、Agent instances、讨论和历史等业务事实。
- `host/governance.rs:300` 对普通讨论清除 `integration_lock.platform_tools`；普通 PM 讨论和正式控制器也不是同一能力面。经理在流程之外缺少项目工作台身份，不能依靠“在 prompt 中告诉它有 CRUD”补齐。
- `core/proposals.rs` 是讨论材料提案，不是通用配置发布。UI 能编辑配置，不意味着 Agent 已经获得同一用例。
- `WorkflowChangeOperation` 已支持 ReplacePlan、UpsertStep、UpsertStage、SetBindings、AdoptStandard 等。`workflow_changes.rs:225–249` 接受完整 document 或插入新 step；P1 所说“只能改原骨架、不能改变端口/执行器”过度概括。是否可应用取决于真实编译、影响与授权，而非缺少所有结构操作。通用配置发布与配置对象新增才是更确定的缺口。
- DSH 工具调用目前未投影成同等的双向控制工具桥，默认 DSH PM 也只有 native 工具。这是实现与既有范围决定的限制；不能只加 `dynamic-tools=true` 就视为完成。

## 6. 改造优先级

先统一应用边界与配置变更语义，再开放经理写工具和双驱动。否则把现有 Host 内部方法直接暴露成通用 RPC 工具，会复制规则、混淆 actor、放大跨库不一致。运行时外置必须与真实能力检测一起交付，不能通过删掉摘要限制来“完成去版本化”。

完整数据模型重写、配置概念强行合并、旧库尽力读取、全局前端重构、吞吐和监控专项均不应阻塞上述主线。详细取舍见 [review 评估](REVIEW.md)。
