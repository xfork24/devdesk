# 边界审计：反向依赖与概念泄漏清单

本文逐条列出内核、Web Core、领域包三者之间不应存在的依赖。每条给出位置、现象、影响和处理方向。处理方向的展开见 [目标内核 SDK 契约](CONTRACT-kernel-sdk.md)、[目标 Web SDK 契约](CONTRACT-web-sdk.md) 和 [报告](REPORT.md)。

基线 `main@2d8421c6`（2026-11-26）。所有位置均已对照源码核实行号；代码改动后以符号名为准。本文是事实记录，不含已实施的变更；[改造计划](PLAN.md)的状态是待实施。

## 图例

| 严重度 | 含义 |
|---|---|
| 高 | 违反依赖方向，或使“领域包可以独立存在”不成立 |
| 中 | 概念或归属错位，当前可用，但会随第二个领域出现而变成缺陷 |
| 低 | 命名、文档、整洁度问题 |

| 类型 | 含义 |
|---|---|
| 字面反向依赖 | 内核代码中出现具体领域包的标识、文件或内容 |
| 概念反向依赖 | 协议或通用逻辑按某个领域的形状编写 |
| 归属错位 | 代码放在不该放的组件里 |
| 构建耦合 | 一方构建时需要另一方的内部文件 |
| 守卫缺口 | 没有机器检查阻止上述问题 |
| 待决策 | 取决于内核定位，不是单纯的缺陷 |

## 总览

| ID | 严重度 | 类型 | 一句话 |
|---|---|---|---|
| A1 | 高 | 字面反向依赖 | core 硬编码软件开发的六个角色标识 |
| A2 | 高 | 字面反向依赖 | adapters 硬编码软件开发的角色命名空间 |
| B1 | 中 | 字面反向依赖 | 内核测试夹具包含领域运行时文件 |
| B2 | 中 | 归属错位 | 单体模式的工程目录夹具与 feature |
| B3 | 低 | 字面反向依赖 | 示例程序直接读取 `plugins/core` |
| C1 | 高 | 概念反向依赖 | 领域包 Schema 写死工程钩子 |
| C2 | 中 | 概念反向依赖 | 处理器的内核输入只有 Git 两种 |
| C3 | 高 | 概念反向依赖 | core 端口 `DomainReader` 的方法是工程概念 |
| C4 | 中 | 概念反向依赖 | 呈现目标写死 `web.react` |
| C5 | 高 | 概念反向依赖 | Host 的通用领域 RPC 按 GitHub 校验 |
| D1 | 高 | 概念反向依赖 | 证据门禁只认 `git.delivery` 一个键 |
| D2 | 高 | 概念反向依赖 | 内核读取领域的输入端口名 `test_configuration` |
| D3 | 中 | 概念反向依赖 | 验证器输入的 `facts` 形状只存在于 Rust |
| D4 | 中 | 概念反向依赖 | 通用引擎按工程工具名分支 |
| D5 | 中 | 概念反向依赖 | 工作流启动时按名字自动填充 `test_configuration` |
| D6 | 高 | 归属错位 | Feature、Task 是内核一等类型（审阅时为待决策，现已定为迁移） |
| D7 | 高 | 归属错位 | 40 个内核处理器的工具定义只发布在领域包里 |
| E1 | 中 | 归属错位 | `plugins/core` 不是 plugin |
| E2 | 高 | 归属错位 | 领域 UI 源码住在 `web/src` |
| E3 | 中 | 归属错位 | 核心样式表包含领域选择器 |
| E4 | 低 | 归属错位 | 核心 Web 外壳包含 Git 页面与仓库文案 |
| E5 | 低 | 概念反向依赖 | Web 上下文类型包含工程钩子字段 |
| E6 | 低 | 归属错位 | 领域目录的文件名用 `builtin-` 前缀 |
| F1 | 高 | 构建耦合 | 领域 ZIP 构建读取内核内部文件 |
| F2 | 高 | 构建耦合 | 领域前端构建依赖核心的配置与源码 |
| F3 | 中 | 构建耦合 | worker 镜像摘要隐式绑定 |
| F4 | 中 | 构建耦合 | 领域包的检查脚本写死了两个目录 |
| G1 | 高 | 守卫缺口 | 架构检查不覆盖领域边界 |
| G2 | 高 | 守卫缺口 | Web 核心包的守卫是三个文件的黑名单 |
| G3 | 低 | 守卫缺口 | 文档与源码的处理器数量不一致 |
| G4 | 中 | 守卫缺口 | 配置包清单在两处的校验深度不同 |
| G5 | 低 | 守卫缺口 | 文档对 Feature、Task 的归属表述不一 |

合计 32 条：高 14，中 12，低 6。审阅时 D6 为待决策，决策后计入高。

## A. 内核到领域包：生产代码中的直接依赖

### A1 core 硬编码软件开发的六个角色标识

- **位置**：[configuration_catalog.rs](../../../crates/core/src/configuration_catalog.rs) 第 49–61、229–237 行；调用点 [configuration_read.rs](../../../crates/host/src/configuration_read.rs) 第 128–130 行
- **现象**：`legacy_software_role` 匹配 `builtin-role-analyst` 等六个标识。活动领域存在时，Host 调用 `archive_legacy_software_roles` 把这些角色从目录视图中归档。
- **影响**：这是单体迁移到领域包时留下的兼容逻辑，但写成了内核对具体标识的永久知识。函数名里的 `software` 已经说明它知道自己在引用谁。
- **方向**：把需要归档的标识变成数据（迁移脚本写入治理记录，或由领域包声明“接管”的旧标识），内核只执行通用的归档规则。
- **状态**：已处理，P1 的 G-3。不兼容决策下没有需要保留的旧角色，兼容逻辑直接删除。

### A2 adapters 硬编码软件开发的角色命名空间

- **位置**：[experience_index.rs](../../../crates/adapters/src/experience_index.rs) 第 214–223 行
- **现象**：`role_matches` 把历史的 `builtin-role-*` 与 `software-development.role-*` 前缀加六个角色名对应起来，用于让旧经验快照匹配新角色。
- **影响**：同 A1。领域包改一个角色名就会让历史经验无法按角色筛选。
- **方向**：与 A1 共用同一份数据。
- **状态**：已处理，P1 的 G-3。经验按角色筛选改为只匹配确切的角色标识。

## B. 内核到领域包：测试与构建期

### B1 内核测试夹具包含领域运行时文件

- **位置**：[adapters/src/tests.rs](../../../crates/adapters/src/tests.rs) 第 13–28 行
- **现象**：在测试或 fixture feature 下，`include_bytes!` 引入 `plugins/software-development/runtime/` 下的报告器文件，并硬编码两个摘要常量。
- **影响**：领域运行时文件一改，内核测试就要同步摘要。文件名是 `tests.rs`，但它是一个生产模块，`lib.rs` 第 60 行以 `pub mod tests` 声明。
- **方向**：夹具文件复制到 `tests/fixtures/`，由测试自行加载。
- **状态**：已处理，P0 的 G-4。

### B2 单体模式的工程目录夹具与 feature

- **位置**：[engineering-catalog.json](../../../tests/fixtures/configuration/engineering-catalog.json)（约 301 KB）；[adapters/Cargo.toml](../../../crates/adapters/Cargo.toml) 第 37 行的 `fixture-catalog`；[e2e.yml](../../../.github/workflows/e2e.yml) 第 59、66、82 行
- **现象**：Host 可以用 `fixture-catalog` feature 把整套工程目录编译进二进制，Web 可以构建成不含领域的单体应用。两者配合用于端到端测试。Web 一侧的成因是 [App.tsx](../../../web/src/App.tsx) 第 1270 行按有无 `domain` 选择两种布局，使它既能作为领域页面，又能作为独立应用运行；[tests/fixtures/workbench/main.tsx](../../../tests/fixtures/workbench/main.tsx) 直接以它为入口。这套双模式是领域页面可以直接依赖核心全部模块的原因之一。
- **影响**：内核与 Web 都保留了“领域内容直接编入”的第二条路径，见 [目标 Web SDK 契约](CONTRACT-web-sdk.md)。
- **方向**：已决定移除，端到端测试改为安装真实 ZIP。见 [改造计划](PLAN.md)。

### B3 示例程序直接读取 `plugins/core`

- **位置**：[export_runtime_recipes.rs](../../../crates/adapters/examples/export_runtime_recipes.rs) 第 7 行
- **现象**：示例从源码目录读取出厂资源包。
- **影响**：低。随 E1 一并处理。
- **状态**：已处理，P0 的 G-4。示例程序改为经公开的 `packaged_runtime` 读取内嵌资源。

## C. 内核被首个领域塑形：领域包协议

### C1 领域包 Schema 写死工程钩子

- **位置**：[domain-package.schema.json](../../../schemas/domain-package.schema.json) 第 24–32 行；[domain_packages.rs](../../../crates/adapters/src/domain_packages.rs) 第 35–48 行
- **现象**：`runtime` 下的 `worktree_branch`、`test_discovery`、`project_connection`、`pull_request_creation`、`test_reporters` 是顶层字段，`test_reporters` 的键限定为三种测试运行器。
- **影响**：任何领域的清单 Schema 都带着软件工程的词汇。新增一种钩子要改 Schema、Rust 结构体和适配器调用点。
- **方向**：移入带版本号的扩展组，见 [目标内核 SDK 契约](CONTRACT-kernel-sdk.md)。

### C2 处理器的内核输入只有 Git 两种

- **位置**：同 Schema 第 41 行；domain_packages.rs 第 63–68、953–963 行
- **现象**：`core` 只能是 `git_head` 或 `git_snapshot`。
- **影响**：领域处理器无法读取项目记录、产物或配置，只能处理调用方传入的内容。
- **方向**：把 `core` 设计为可扩展的内核输入目录。

### C3 core 端口 `DomainReader` 的方法是工程概念

- **位置**：[ports.rs](../../../crates/core/src/ports.rs) 第 262–301 行
- **现象**：除通用的 `read` 外，端口还有 `worktree_branch`、`pull_request_creation`、`test_reporter`、`project_test_discovery`。
- **影响**：端口是内核内部架构最核心的抽象，`check-architecture.py` 守护的正是这一层。工程概念进入这里，意味着“内核的接口定义”已经依赖了领域的词汇。
- **方向**：随 C1 一起泛化。

### C4 呈现目标写死 `web.react`

- **位置**：同 Schema 第 88 行（样式路径限定在 `ui/web/`）；domain_packages.rs 第 453–458 行；[DomainWorkbench.tsx](../../../web/src/DomainWorkbench.tsx) 第 179–181、623–626 行
- **现象**：只有 `renderer` 为 `web.react` 的目标可以声明样式。
- **影响**：Schema 自称“客户端中立”，实际把 Web 的目录结构写进了协议。
- **方向**：样式路径规则由 renderer 自己的 Schema 描述。

### C5 Host 的通用领域 RPC 按 GitHub 校验

- **位置**：[domains.rs](../../../crates/host/src/domains.rs) 第 217–305、306–347 行
- **现象**：`domain.project.create` 带 `connection` 时调用 GitHub 项目模块校验仓库名并查询 GitHub 项目登记；`domain.project.remove` 只允许移除登记了仓库的项目，错误文案也写明“GitHub 仓库”。
- **影响**：非 GitHub 的领域无法使用这两个 RPC 的完整语义。`project_connection` 钩子本来的目的是让领域决定如何解析接入参数，内核却在后面又按 GitHub 判断了一次。
- **方向**：接入结果的解释与可移除性交给领域处理器。

## D. 内核被首个领域塑形：工作流与执行引擎

### D1 证据门禁只认 `git.delivery` 一个键

- **位置**：[workflow.rs](../../../crates/domain/src/workflow.rs) 第 8、646、675 行；[platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) 第 373–379 行；[platform_facts.rs](../../../crates/core/src/platform_facts.rs) 第 533–535 行
- **现象**：`operation_evidence` 在 Schema 上是任意键的映射，内核只读取常量 `REPOSITORY_DELIVERY_EVIDENCE`，即 `git.delivery`。触发点也是写死的：分发逻辑按四个工具名 `workspace.target`、`git.source.publish`、`git.delivery.open`、`git.delivery.merge` 进入证据准备。
- **影响**：领域无法为其他操作声明证据门禁，例如“发布报告前必须通过评审”。
- **方向**：按操作名查找对应的证据合同。

### D2 内核读取领域的输入端口名 `test_configuration`

- **位置**：platform_delivery.rs 第 358–359 行
- **现象**：为验证器拼装 `regression_configured` 时，内核读取工作流输入里名为 `test_configuration` 的端口。
- **影响**：这是软件开发工作流的端口名。换个领域的工作流即使声明了 `git.delivery` 证据，也会因为没有这个端口而得到错误的前提。
- **方向**：随 D1 泛化，由证据合同声明从哪个输入读取。

### D3 验证器输入的 `facts` 形状只存在于 Rust

- **位置**：[build.py](../../../plugins/software-development/build.py) 第 190–210 行；[platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) 第 288–363 行
- **现象**：验证器的输入 Schema 把 `facts` 声明为最多 32 个属性的对象，不约束条目形状。实际形状由内核拼装，领域只能从样例 `worker.mjs` 反推。
- **影响**：内核调整事实结构时没有合同可以校验。
- **方向**：为事实条目补 Schema。

### D4 通用引擎按工程工具名分支

- **位置**：[configured_workflows.rs](../../../crates/core/src/configured_workflows.rs) 第 897、1033 行；[platform_facts.rs](../../../crates/core/src/platform_facts.rs)、[workflow_recovery.rs](../../../crates/core/src/workflow_recovery.rs)、[workflow_retries.rs](../../../crates/core/src/workflow_retries.rs)、[workflow_rework.rs](../../../crates/core/src/workflow_rework.rs)、[publication_retry.rs](../../../crates/core/src/publication_retry.rs)、[delivery_recovery.rs](../../../crates/core/src/delivery_recovery.rs)
- **现象**：内核在十余处以字面量判断工具名，例如 `tests.run`、`tests.project`、`git.delivery.open`、`git.delivery.merge`、`workspace.checkpoint`、`git.publish`，并据此改变权限、恢复、重试和返工行为。`configured_workflows.rs` 第 897 行要求名为 `tests.run` 的工具必须选择冻结运行环境。`platform_facts.rs` 中这类字面量最多，仅 `tests.run` 就出现 7 次。
- **影响**：工具的行为契约不写在工具定义里，而散落在引擎中。新增一个语义类似 `tests.run` 的工具无法获得同样的保障。
- **方向**：把这类语义声明为工具定义的属性，引擎按属性分支。

### D5 工作流启动时按名字自动填充 `test_configuration`

- **位置**：[workflow_start.rs](../../../crates/core/src/workflow_start.rs) 第 244–263 行；[workspace_commands.rs](../../../crates/core/src/workspace_commands.rs) 第 204–223 行；另见 [standards.rs](../../../crates/core/src/standards.rs) 第 180–189 行、[platform_facts.rs](../../../crates/core/src/platform_facts.rs) 第 625 行
- **现象**：工作流定义只要有名为 `test_configuration` 的输入端口，内核就自动从项目测试配置填充它，并禁止调用者覆盖。
- **影响**：这是内核与软件开发工作流之间的命名约定，没有在任何 Schema 中声明。
- **方向**：由工作流定义显式声明“此端口由内核的某个提供者填充”。

### D6 Feature、Task 是内核一等类型

- **位置**：[domain/lib.rs](../../../crates/domain/src/lib.rs) 第 201–224、364–365 行；[project/schema.sql](../../../sql/project/schema.sql) 第 85、189 行；[repository.rs](../../../crates/domain/src/repository.rs) 第 5–10、302 行；[rpc_contracts.json](../../../crates/core/src/rpc_contracts.json) 中的 `feature.create`、`query.feature_branches`、`tasks.create`、`tasks.complete`
- **现象**：`Feature`、`Task` 是 domain crate 的结构体，状态里有 `features` 与 `tasks`，项目库有对应的表。`WorktreeKind` 的取值是 `mainline`、`feature`、`task`、`candidate`。
- **影响**：这不是缺陷，取决于内核定位。若内核定位为“工程工作台”，这是内核能力，应当写进 SDK 文档。若定位为“领域无关工作台”，它们属于软件开发领域，应改为对象类型。
- **方向**：已决定。内核定位为领域无关，Feature、Task 迁为软件开发包的对象类型，不保留内核工作项类型。见 [改造计划](PLAN.md)。

### D7 40 个内核处理器的工具定义只发布在领域包里

- **位置**：[plugins/core/manifest.json](../../../plugins/core/manifest.json) 的 `tool` 列表共 11 项，其中平台工具 9 项；[软件开发 manifest](../../../plugins/software-development/catalog/manifest.json) 的 `tool` 列表共 40 项，全部是平台工具
- **现象**：`configuration.*`、`workflow.*` 的大部分、`project.*`、`tests.*`、`git.*`、`workspace.*` 的处理器都在内核的 `PLATFORM_HANDLERS` 里，但它们的工具定义（包括模型看到的输入 Schema）只作为软件开发包的源发布。
- **影响**：空领域的 Host 上只有 9 个平台工具定义被发布。另一个领域想使用 `workflow.start`，只能把软件开发包里的 JSON 复制一份到自己的命名空间。内核能力的对外契约因此由一个领域包拥有。
- **方向**：内核处理器的定义随内核发布（放进出厂资源包），领域包只发布领域自己的工具定义与采用关系。

## E. 归属错位

### E1 `plugins/core` 不是 plugin

- **位置**：[plugins/core](../../../plugins/core/manifest.json)；[build.rs](../../../crates/adapters/build.rs) 第 8–9、46–76 行；[factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) 第 4 行
- **现象**：目录在 `plugins/` 下，文档称其为“核心配置”。它由内核构建脚本读入，校验后生成为 Rust 常量。
- **影响**：“plugin”一词同时指“内核出厂资源”和“可安装的领域包”，讨论边界时容易混淆。内核构建还依赖一个名为 plugins 的目录。
- **方向**：改名以反映其身份，例如 `kernel/resources`。

### E2 领域 UI 源码住在 `web/src`

- **位置**：[entry.tsx](../../../plugins/software-development/web/entry.tsx) 第 1–2 行；[App.tsx](../../../web/src/App.tsx) 及 27 个组件
- **现象**：见 [Web 嵌入基线](BASELINE-web-embedding.md)。28 个只被领域使用的文件（约 419 KB）位于核心包。
- **影响**：Web Core 的源码、lint、类型检查、构建都包含领域代码。领域的任何界面改动都是核心包的改动。
- **方向**：移入领域目录，见 [目标 Web SDK 契约](CONTRACT-web-sdk.md)。

### E3 核心样式表包含领域选择器

- **位置**：[style.css](../../../web/src/style.css) 第 226、509、618、1208 行等
- **现象**：3117 行中有 186 行的选择器含 workflow、manager、feature、branch、repo、delivery 等词。
- **影响**：领域界面的大部分样式由核心拥有。领域自己的样式表只有 175 行，且叠在核心类上。
- **方向**：随 E2 迁移。

### E4 核心 Web 外壳包含 Git 页面与仓库文案

- **位置**：[DomainWorkbench.tsx](../../../web/src/DomainWorkbench.tsx) 第 118、334、372、872–879 行
- **现象**：核心外壳有一个 Git 查询页，通用查询表单的默认字段名是“本地仓库路径”，“移除项目”按钮的提示是“再次打开同一仓库和主分支即可恢复”。
- **影响**：低。但它们说明核心外壳也在吸收领域的词汇。
- **方向**：Git 页面归入工程扩展。

### E5 Web 上下文类型包含工程钩子字段

- **位置**：[domain-sdk.ts](../../../web/src/domain-sdk.ts) 第 45 行；DomainWorkbench.tsx 第 54、640–650、778–782 行
- **现象**：`DomainContext.projectConnection` 是清单里 `project_connection` 钩子的名字，只被核心用来决定是否渲染“移除项目”。
- **方向**：随 C1、C5 处理。

### E6 领域目录的文件名用 `builtin-` 前缀

- **位置**：[plugins/software-development/catalog](../../../plugins/software-development/catalog/manifest.json)
- **现象**：141 个 JSON 文件中 129 个以 `builtin-` 命名，而它们的 `identity` 都在 `software-development.` 命名空间下。
- **影响**：`builtin-` 在出厂资源包里表示“内核提供”。同一前缀在领域目录里出现，读文件名会误判归属。
- **方向**：文件名与命名空间一致。

## F. 领域包到内核：非 SDK 依赖

### F1 领域 ZIP 构建读取内核内部文件

- **位置**：[build.py](../../../plugins/software-development/build.py) 第 48、143 行
- **现象**：构建脚本读取 `schemas/rpc.schema.json` 的 `$defs` 来拼装输出 Schema，并用 `runpy` 执行 `scripts/check/configuration_usage.py` 生成引用索引。第 49–53 行还因为“平台校验器使用线性时间的正则引擎”而改写了内核 Schema 的一段。
- **影响**：领域包无法脱离内核源码构建。内核改这两个文件会让领域构建失败，而且没有任何合同说明它们是对外接口。
- **方向**：把领域构建需要的东西做成 SDK 制品：Schema 片段包和计算引用索引的小工具。

### F2 领域前端构建依赖核心的配置与源码

- **位置**：[vite.software.config.ts](../../../web/vite.software.config.ts) 全文；[tsconfig.json](../../../web/tsconfig.json) 第 16 行；[package.json](../../../web/package.json) 第 10、15 行
- **现象**：见 [Web 嵌入基线](BASELINE-web-embedding.md)。
- **方向**：领域自带构建配置，SDK 提供配置工厂。

### F3 worker 镜像摘要隐式绑定

- **位置**：[worker.rs](../../../crates/adapters/src/domain_packages/worker.rs) 第 5 行；[builtin-sandbox-docker-v2.json](../../../plugins/core/sandboxes/builtin-sandbox-docker-v2.json) 第 16 行
- **现象**：两处写着同一个镜像摘要。处理器在哪个 Node 版本上运行，领域作者无法从任何声明得知。
- **影响**：内核升级镜像后，依赖新版语言特性的处理器会在没有提示的情况下失败。
- **方向**：在能力目录中暴露 worker 运行时版本。

### F4 领域包的检查脚本写死了两个目录

- **位置**：[check-defaults.py](../../../scripts/check/check-defaults.py) 第 87–90 行
- **现象**：脚本只检查出厂资源包和软件开发包两个目录，领域标识 `software-development` 也作为参数写死。
- **影响**：新领域包无法使用这个检查，除非修改内核仓库里的脚本。它和引用索引脚本一起，是领域作者仅有的两个工具，都不是为第三个目录准备的。
- **方向**：脚本接受目录与命名空间参数，随 SDK 制品发布。

## G. 守卫与文档

### G1 架构检查不覆盖领域边界

- **位置**：[check-architecture.py](../../../scripts/check/check-architecture.py)
- **现象**：脚本检查四个 crate 之间的依赖方向、domain 的纯度等，没有任何规则涉及 `plugins/`。
- **影响**：A1、A2、B1 这类引用可以无阻拦地进入内核。
- **方向**：增加规则：`crates/` 的非测试代码不得出现 `plugins/` 路径与已注册的领域包标识。
- **状态**：已处理，P0 的 G-1。

### G2 Web 核心包的守卫是三个文件的黑名单

- **位置**：[vite.core.config.ts](../../../web/vite.core.config.ts) 第 15–21 行
- **现象**：只有 `App.tsx`、`FeatureBranchQueue.tsx`、`RepositorySettings.tsx` 被禁止进入核心包。
- **影响**：名单靠人维护。`GitHubConnection.tsx`、`WorkflowController.tsx` 等领域文件不在名单里，只要被核心引用就会混入。
- **方向**：改为白名单：核心包的 import 图里不允许出现领域目录；领域包的 import 图里不允许出现核心源码。
- **状态**：守卫已就位，P0 的 G-2，现存 2 条越界记入基线。三个文件的黑名单仍在，由 P2 的 W-1 替换。

### G3 文档与源码的处理器数量不一致

- **位置**：[tools-and-primitives.md](../../../docs/design/reference/tools-and-primitives.md) 第 7 行
- **现象**：文档写“54 个平台处理器”，源码是 55 个；文档列出的 5 个未发布处理器没有包含 `domain.read`。
- **方向**：数量从源码生成，或把登记表作为唯一来源。
- **状态**：已处理，P0 的 G-5。

### G4 配置包清单在两处的校验深度不同

- **位置**：[build.rs](../../../crates/adapters/build.rs) 第 50 行；[factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) 第 30–97 行
- **现象**：出厂资源包在内核构建时按 `template-manifest.schema.json` 校验。领域包的配置包清单在安装时没有套用这份 Schema。
- **影响**：同一个格式，一边严格一边宽松。
- **方向**：安装时套用同一份 Schema。

### G5 文档对 Feature、Task 的归属表述不一

- **位置**：[domain-model.md](../../../docs/design/domain-model.md) 第 7–15 行；[README.md](../../../README.md) 第 3 行
- **现象**：前者把 Feature、Task 列为“核心概念”，后者把“从 Feature、Task 到 PR 合入”描述为软件开发包提供的流程。
- **影响**：读者无法判断它们属于内核还是领域。
- **方向**：随 D6 的决策统一。
- **状态**：已补充当前归属与去向的说明（P0 的 G-5），迁移在 P4。
