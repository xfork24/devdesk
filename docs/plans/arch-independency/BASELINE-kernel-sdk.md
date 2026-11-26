# 内核 SDK 基线：改造前的事实

本文记录改造开始前内核向领域包暴露的全部契约，基线 `main@2d8421c6`（2026-11-26），由架构审阅整理。它回答两个问题：SDK 当时到底是什么，以及一个领域包怎样用它写出配置和运行时。目标契约见 [目标内核 SDK 契约](CONTRACT-kernel-sdk.md)，逐条问题的处理方式见 [改造计划](PLAN.md) 与 [审计清单](AUDIT.md)。

阅读时注意：下文是当时的事实描述，不是目标状态。其中“没有文档”“只能读 Rust 才知道”等判断对应的就是本计划要消除的问题。

原有第 0 节结论已并入[报告](REPORT.md)。本文是事实记录；[改造计划](PLAN.md)的状态是待实施。

## 1. SDK 的定义（现状显式化）

### 1.1 角色与方向

| 角色 | 是什么 | 拥有什么 |
|---|---|---|
| 内核 | `crates/*` 编译出的 `devdesk-host` | 全部权威状态、权限、调度、外部副作用、协议定义 |
| 出厂资源包 | `plugins/core` | Model、ModelProvider、Harness、Sandbox、权限预设、通用工具的定义数据。编译进内核 |
| 领域包 | 一个 ZIP | 配置数据、对象类型、受限的只读处理器、前端贡献 |

允许的方向只有一个：领域包依赖内核暴露的契约。内核不应该知道任何具体领域包的标识、目录或内容形状。

### 1.2 七个切面

| 切面 | 内容 | 定义位置 | 状态 |
|---|---|---|---|
| A 包格式 | ZIP 清单、文件与摘要、命名空间 | [domain-package.schema.json](../../../schemas/domain-package.schema.json) 与 `Package::read` | 显式，但混有工程钩子 |
| B 配置模型 | 15 种源定义、引用规则、默认选择 | [source-definition.schema.json](../../../schemas/source-definition.schema.json)、[rpc.schema.json](../../../schemas/rpc.schema.json)、内核 `configuration*` | 显式，是 SDK 中最成熟的部分 |
| C 内核基础能力 | Model、Harness、Sandbox、权限、工具 | 出厂资源包数据加 Rust 驱动与处理器 | 隐式。能扩展什么只能读 Rust 才知道 |
| D 运行时 | 处理器声明、worker 协议、沙箱边界 | Schema 加 [worker.rs](../../../crates/adapters/src/domain_packages/worker.rs) 加样例 `worker.mjs` | 协议只存在于 Rust 实现和样例里，没有文档和辅助库 |
| E 工作流扩展点 | 执行器、`domain.read`、`operation_evidence`、对象类型 | domain crate、core 的 `platform_*` 与 `workflow_*` | 部分显式。证据门禁实际只有一个具名槽位 |
| F 客户端协议 | 领域 RPC、SSE、贡献 | [domains.rs](../../../crates/host/src/domains.rs)、[rpc_contracts.json](../../../crates/core/src/rpc_contracts.json)、两份贡献 Schema | 显式，但 Host 侧写死了 GitHub |
| G 工程钩子 | 分支命名、PR 文案、测试识别、报告器、项目接入 | 清单字段加内核适配器调用点 | 隐式。名字和语义只存在于 Rust |

### 1.3 A. 包格式

清单根字段：`format`（常量 1）、`id`、`version`、`files`（路径到 SHA-256）、可选的 `entity_types`、`runtime`、`ui`。由 `Package::read`（[domain_packages.rs](../../../crates/adapters/src/domain_packages.rs) 第 256–552 行）强制执行的约束：

| 约束 | 取值 |
|---|---|
| ZIP 大小 | 不超过 2 MiB，解压后不超过 8 MiB，不超过 256 个文件 |
| 路径 | 只允许规范相对路径的普通文件，拒绝符号链接、重复项和文件与目录冲突 |
| 完整性 | `files` 的条目数必须等于 ZIP 实际文件数，逐个比对摘要 |
| 标识 | `id`、`version` 为 1–140 字符的标识符 |
| 命名空间 | 处理器名、对象类型引用、贡献 ID 必须以 `{id}.` 开头 |
| 同版本不同内容 | 拒绝安装 |

### 1.4 B. 配置模型

这是 SDK 中最成体系的切面，领域包与出厂资源包共用同一套格式。

**源信封。** 每个源是一个 JSON 对象：`type`、`identity`、`revision`、`name`、`scope`、`data`，引用放在根部而不是 `data` 里。可用的根部引用字段：`capability_ref`、`role_ref`、`model_ref`、`harness_ref`、`sandbox_ref`、`model_catalog_ref`、`spec_refs`、`skills`、`tools`、`model_providers`、`root_workflow_refs`、`default_workflow_ref`、`base_standard_ref`。引用一律写 `identity:revision`。

**类型。** `role`、`skill`、`spec`、`standard`、`workflow`、`delivery_type`、`capability`、`agent`、`model`、`model_provider`、`model_catalog`、`harness`、`sandbox`、`tool`、`permission_profile`。

**正文内联。** `data` 中以 `_file` 结尾的字段在打包时读取为同名字段的内容，例如 Role 的 `instructions_file` 变成 `instructions`。

**配置包清单。** `configurations/manifest.json` 包含 `schema_version`、`bundle_version`、`types`、`defaults`、`usage_index`。[template-manifest.schema.json](../../../schemas/template-manifest.schema.json) 还要求 15 个类型键全部出现。

这里有一处不一致：出厂资源包在内核构建时按这份 Schema 校验（[build.rs](../../../crates/adapters/build.rs) 第 50 行），领域包的配置包清单在安装时**没有**套用它。`read_bundle`（[factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) 第 30–97 行）只反序列化 `types` 并要求 `defaults` 与 `usage_index` 存在。同一个格式，两种校验深度。

**领域包额外的规则。** 与出厂资源包相比有三条不同，由 `validate_domain_configuration`（[configuration_publication.rs](../../../crates/core/src/configuration_publication.rs) 第 702 行起）执行：

1. 每个源的 `identity` 必须在 `{包 id}.` 命名空间内，`scope` 必须是 `global`。
2. `usage_index` 必须与内核按当前引用规则重算的结果完全一致。
3. `defaults.role_agent_bindings` 中的 Agent 必须属于所绑定的 Role。

**默认选择。** `defaults.json` 中已确认被内核读取的键：`workflow_ref`、`task_workflow_ref`、`standard_ref`、`model_ref`、`harness_ref`、`sandbox_ref`、`harness_permission_ref`、`sandbox_permission_ref`、`role_agent_bindings`、`orchestrator_role_ref`。软件开发包还写了 `capability_ref`、`orchestration.mode` 和 `authoring.execution_budget`，本次未逐一追踪它们的读取点。

### 1.5 C. 内核基础能力：能引用什么，能扩展什么

| 能力 | 引用内核实例 | 发布新实例 | 新增实现 | 依据 |
|---|---|---|---|---|
| Model | 可以 | 可以 | 不可以 | `ModelDefinition.provider` 是字符串，需指向已有服务商 |
| ModelProvider | 可以 | 可以 | 不可以 | 协议与路由由内核驱动实现；哪些 `protocol` 取值可工作本次未核实 |
| Harness | 可以 | 可以，`provider` 只能是 `codex` 或 `dsh` | 不可以 | [runtime.rs](../../../crates/domain/src/runtime.rs) 第 9–15 行闭合枚举；[configuration.rs](../../../crates/core/src/configuration.rs) 第 418 行 |
| Sandbox | 可以 | 可以，`provider` 固定为 `docker` | 不可以 | [sandbox.schema.json](../../../schemas/sandbox.schema.json) 用 `const` 固定 |
| 权限预设 | 可以 | 可以 | 不可以 | 后端资产随内核发布 |
| 平台工具 | 可以 | 只能指向已注册处理器 | 不可以 | [governance.rs](../../../crates/core/src/governance.rs) 第 62–72 行 |
| Harness 原生工具 | 可以 | 可以，provider 须已知 | 不可以 | 同上 |
| Role、Agent、Capability、Skill | 可以 | 可以 | 不适用 | 纯数据 |
| Workflow、Delivery、Spec、Standard | 可以 | 可以 | 不适用 | 纯数据 |

这张表是本次审阅中“最该写成文档却只能读 Rust 才能得出”的部分。`harness.rs` 第 2001 行的注释说得很准确：“configuration cannot add implementations”。这是一条正确的设计，但没有出现在任何面向领域作者的文档里。

平台工具是受限最明显的一类。`implementation` 为 `platform` 的工具定义，其 `qualified_name` 必须在 `PLATFORM_HANDLERS`（[platform_tools.rs](../../../crates/core/src/platform_tools.rs) 第 4–60 行，共 55 项）之内。领域包自己实现的逻辑只有一个入口：`domain.read`，参数固定为 `{handler, input}`。

### 1.6 D. 运行时与处理器

**声明。** 清单的 `runtime.handlers` 是一个映射，键为带命名空间的处理器名，值为：

| 字段 | 含义 |
|---|---|
| `input_schema`、`output_schema` | 调用时双向校验，必须是可离线编译的 JSON Schema |
| `core` | 内核为处理器准备的输入：`git_head` 或 `git_snapshot`，省略则为 `null` |
| `git_snapshot` | `core` 为 `git_snapshot` 时的选择声明：最多 16 个文件、最多 8 组搜索，每组 1–8 个不超过 128 字符的关键字 |

`runtime.entry` 是入口文件，必须在 `runtime/` 下且以 `.mjs` 结尾。

**沙箱边界。** 每次调用启动一个新容器（[worker.rs](../../../crates/adapters/src/domain_packages/worker.rs) 第 7–84 行）：

| 项 | 取值 |
|---|---|
| 网络 | `--network=none` |
| 文件系统 | `--read-only`；只挂载入口文件到 `/worker.mjs`，权限 0444 |
| 权限 | `--cap-drop=ALL`、`--security-opt=no-new-privileges` |
| 资源 | 内存 128 MiB、1 CPU、48 个进程 |
| 时间 | 容器内 `timeout 18s`，宿主侧捕获 15 秒 |
| 镜像 | 按摘要固定在 worker.rs 第 5 行，`--pull=never` |

入口文件是唯一被挂载的包内文件，所以处理器必须是**没有任何本地 import 和依赖的单文件**。这条约束没有写在 Schema 里。

**协议。** Host 向容器标准输入写一行 JSON，从标准输出读回一个 JSON 对象：

```json
{"jsonrpc":"2.0","id":"<uuid>","method":"invoke","params":{"handler":"<name>","input":{...},"core":null}}
```

| 方法 | `params` | 期望的 `result` |
|---|---|---|
| `describe` | `{}` | `{"api":1,"handlers":[...]}`。激活时调用 |
| `invoke` | `handler`、`input`、`core` | 处理器的返回值 |

约定：

- `describe` 返回的处理器列表与清单做整体相等比较，因此**顺序必须是清单键的字典序**。
- 响应的 `id` 必须原样返回，`jsonrpc` 必须是 `"2.0"`。
- 标准输出必须恰好是一个 JSON 值。任何额外的输出（包括调试日志）都会导致解析失败。
- 退出码必须为 0。
- 响应带 `error` 时，Host 返回统一的 `domain_worker_failed`，**处理器写的错误信息不会传给调用者**（worker.rs 第 74–79 行）。
- 握手只协商 `api: 1`，没有能力列表。

**`core` 输入的形状。** 这是处理器能从内核拿到的全部数据：

| 取值 | 输入参数 | 形状 | 位置 |
|---|---|---|---|
| `git_head` | `path` | `{"head":"<oid>"}` | [git.rs](../../../crates/adapters/src/git.rs) 第 42–69 行 |
| `git_snapshot` | `path`、`revision` | `revision`、`paths`（全部被跟踪的普通文件路径）、`files`（选中文件内容）、`omitted`（超限文件）、`matches`（搜索结果布尔值） | 同文件第 111–233 行 |

快照中单个文件不超过 256 KiB，内容总量不超过 1 MiB。

**校验链。** 调用前后共有四道检查：输入按 `input_schema`；`core` 由内核按声明生成；输出按 `output_schema`；具名钩子另做语义校验，例如分支名不得以 `-` 开头、测试识别不得改变原修订。

### 1.7 E. 工作流扩展点

**执行器。** 步骤可用四种执行器（[workflow.rs](../../../crates/domain/src/workflow.rs) 第 159–179 行）：`agent`（引用 Role）、`tool`（引用工具定义）、`workflow`（子流程，可 `foreach`）、`control`。

**`domain.read`。** 领域让处理器成为工作流步骤的唯一方式，是发布一个 `qualified_name` 为 `domain.read`、`implementation` 为 `platform` 的工具定义，参数是 `{handler, input}`，输出 Schema 写死在这个工具定义里。它只读，要求项目属于活动领域；恢复路径不会自动重跑，而是报 `needs_reconciliation`（[platform_runtime.rs](../../../crates/core/src/platform_runtime.rs) 第 654–666 行）。软件开发包用这种方式发布了一个 `detect-test-entry` 工具（[build.py](../../../plugins/software-development/build.py) 第 89–93 行）。

**`operation_evidence`。** Workflow 可以声明“执行某个平台工具前必须先满足哪些事实”。结构是通用的：`verifier` 指向处理器，`sources` 列出步骤端口，`checks` 给每个操作列出所需事实名。但内核只读取一个键（[platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) 第 373–375 行，常量 `git.delivery` 定义在 [workflow.rs](../../../crates/domain/src/workflow.rs) 第 8 行），且只在四类交付工具的路径上触发。验证器的输入由内核拼装（同文件第 352–361 行）：

| 字段 | 来源 |
|---|---|
| `operation` | 工具名 |
| `source_revision` | 工具参数 `workspace.revision` |
| `source_branch` | Git 上下文 |
| `destination` | `mainline` 或 `parent_work`，由 Git 上下文的归属推出 |
| `candidate_revision` | 工具参数 `publication.revision` |
| `regression_configured` | 工作流输入端口 `test_configuration` 是否含 `regression` |
| `facts` | 各事实的步骤结果 |

`facts` 中每个条目的形状没有 Schema，只能从样例 `worker.mjs` 的用法反推。`regression_configured` 还读取了软件开发工作流的端口名 `test_configuration`。

**对象类型。** `entity-types/*.json` 声明带版本的类型：名称、JSON Schema、关系（目标类型与基数）、工作流绑定（哪个输入端口装载对象数据，哪个交付端口产出哪种对象类型）。RPC 为 `domain.entity.put`、`query.domain.entity`，以及带归属的 `domain.workflow.start`。

### 1.8 F. 客户端协议

Host 的领域入口（domains.rs 第 100–486 行）处理 18 个方法：9 个 `domain.*` 命令、7 个 `query.domain.*` 查询、2 个 `query.core.git.*`。另有 HTTP 入口 `/domain-events` 和 `/domain-assets`。前端部分见 [Web 嵌入基线](BASELINE-web-embedding.md)。Host 侧有两处写死了 GitHub：

- `domain.project.create` 带 `connection` 时，按 `owner/repo` 校验仓库名，复用项目时查询 GitHub 项目登记（[domains.rs](../../../crates/host/src/domains.rs) 第 236、239 行）。
- `domain.project.remove` 只允许移除登记了仓库的项目（同文件第 323–326 行）。

### 1.9 G. 工程钩子

内核在适配器里按固定名字向活动领域包索取五项策略与资源。它们出现在三个地方，形状一致但彼此没有引用关系：

| 钩子 | 清单字段 | 处理器输入 | 处理器输出 | 内核调用点 |
|---|---|---|---|---|
| 工作树分支命名 | `worktree_branch` | `{owner, kind}` | `{owner, kind, branch}` | domain_packages.rs 第 975–1007 行 |
| PR 标题与正文 | `pull_request_creation` | `{request, marker}` | `{head, base, title, body}` | 同文件第 1009–1033 行 |
| 项目测试入口识别 | `test_discovery` | `{path, revision}` 加 Git 快照 | 含 `revision`、`configuration` 的对象 | 同文件第 1059–1093 行 |
| 项目接入 | `project_connection` | `{repository, mainline}` | `{repository, mainline}` | 同文件第 1165–1187 行 |
| 测试报告器 | `test_reporters` | 无，是文本资源 | 不适用 | 同文件第 1035–1057 行 |

三个位置是：[domain-package.schema.json](../../../schemas/domain-package.schema.json) 第 24–32 行、[ports.rs](../../../crates/core/src/ports.rs) 第 262–301 行的 `DomainReader`、`DomainPackages` 的同名方法。一个与软件工程无关的领域不需要其中任何一个，但协议仍然要求作者理解它们。

## 2. 领域包怎样使用 SDK

### 2.1 编写配置

1. 确定包 `id`，之后所有标识都以 `{id}.` 开头。
2. 按 [plugins/software-development/catalog](../../../plugins/software-development/catalog/manifest.json) 的方式组织目录：`manifest.json` 按类型列出文件，`defaults.json` 写默认选择。
3. 逐个编写源文件。引用内核能力时写 `builtin-*:revision`，例如 `builtin-harness-codex-managed:2`；引用包内定义时写 `{id}.xxx:revision`。
4. 正文类字段用 `_file` 指向同目录的 Markdown，打包时内联。
5. 用 [configuration_usage.py](../../../scripts/check/configuration_usage.py) 的 `usage_index` 生成引用索引，写入配置包清单的 `usage_index`。索引与内核重算结果不一致会导致安装失败。
6. 用 [check-defaults.sh](../../../scripts/check/check-defaults.sh) 核对归属与引用闭合。这个脚本目前写死了出厂资源包和软件开发包两个目录（[check-defaults.py](../../../scripts/check/check-defaults.py) 第 87–90 行），新领域需要改脚本才能使用。

### 2.2 编写运行时

1. 写一个单文件 `runtime/worker.mjs`，不 import 任何本地模块或 npm 包。
2. 在清单的 `runtime.handlers` 中声明每个处理器及其输入输出 Schema。需要 Git 数据的处理器声明 `core`。
3. 在入口文件里读取标准输入的一行 JSON，按 `params.handler` 分发，把结果写成一行 JSON 到标准输出。`describe` 返回 `api: 1` 与处理器名列表。
4. 不要向标准输出写任何别的内容。失败时可以返回 `error`，但信息不会到达调用者，所以业务上应返回带状态字段的成功结果。
5. 如果处理器要作为工作流步骤，再发布一个 `domain.read` 工具定义，并在 Workflow 里用 `tool` 执行器引用它，参数里用常量绑定 `handler`。
6. 如果要作为证据验证器，在 Workflow 的 `operation_evidence` 里声明；键必须是 `git.delivery` 才会生效。

### 2.3 一个处理器的五条调用路径

| 路径 | 触发者 | 要求 | 典型用途 |
|---|---|---|---|
| `query.domain.invoke` | 浏览器 | 活动代次；不要求项目 | 页面自己的只读计算 |
| `domain.read` 平台工具 | 冻结的工作流步骤 | 项目属于活动领域 | 工作流中的领域判断 |
| 工程钩子 | 内核适配器 | 项目属于活动领域；清单里登记了钩子名 | 分支名、PR 文案、测试识别 |
| 证据验证器 | 四类交付工具 | Workflow 声明了 `git.delivery` 证据 | 交付门禁 |
| `project_connection` | `domain.project.create` | 活动代次 | 解析接入参数 |

### 2.4 第二个领域的思想实验

假设要做一个与软件工程无关的领域，例如数据分析。按现有 SDK：

| 想做的事 | 能否 | 原因 |
|---|---|---|
| 发布 Role、Skill、Workflow、Spec、Standard、Agent | 能 | 纯数据 |
| 复用内核的 Model、Harness、Sandbox | 能 | 按 `builtin-*:revision` 引用 |
| 定义自己的对象类型和对象工作流 | 能 | `entity_types` 是通用的 |
| 提供 React 页面 | 能 | 见 [Web 嵌入基线](BASELINE-web-embedding.md) |
| 让处理器在页面或工作流里做只读计算 | 能 | 单文件、断网、15 秒内、1 MiB 输入 |
| 让处理器读数据库或外部服务 | 不能 | 容器断网 |
| 让处理器读项目记录、产物、配置 | 不能 | `core` 只有 Git 两种输入 |
| 让处理器执行超过 15 秒的任务 | 不能 | 宿主侧超时 |
| 让 Agent 以稳定名字调用领域逻辑 | 不能 | 只能经 `domain.read` 加 `{handler, input}` |
| 为非 Git 交付的操作设证据门禁 | 不能 | 内核只读 `git.delivery` 一个键 |
| 接入非 GitHub 的项目来源 | 不能 | `domain.project.create` 与 `remove` 按 GitHub 校验 |
| 提供设置页或全局页面 | 基本不能 | 投影贡献必须是 `context: "project"`，`query.domain.contribution` 也要求 `project_id`。没有为全局或设置页面准备的上下文类型 |
| 使用新的 Harness 或 Sandbox 实现 | 不能 | 编译期注册 |
| 给调用者返回可读的错误信息 | 不能 | 错误信息被内核丢弃 |

## 3. 评估

### 3.1 隔离性

做得好的部分都有代码对应：

- 领域处理器无法产生副作用：RPC 合同把它登记为 `read`，平台工具登记为 `platform_read`，容器断网只读。
- 命名空间、摘要、Schema 校验在安装前完成，失败即拒绝。
- 内核对领域返回值不直接采信：分支名、修订、PR 创建参数、测试配置都有二次校验。
- Rust 四个 crate 之间的依赖方向有机器守卫。

不合格的部分：

| 问题 | 性质 | 证据 |
|---|---|---|
| core 硬编码软件开发包的六个角色标识 | 字面反向依赖，生产代码 | [configuration_catalog.rs](../../../crates/core/src/configuration_catalog.rs) 第 49–61 行 |
| adapters 按领域包的命名空间匹配历史角色 | 字面反向依赖，生产代码 | [experience_index.rs](../../../crates/adapters/src/experience_index.rs) 第 214–223 行 |
| 领域包协议包含软件工程钩子 | 概念反向依赖 | [domain-package.schema.json](../../../schemas/domain-package.schema.json) 第 24–32 行 |
| 内核端口 `DomainReader` 的方法名是工程概念 | 概念反向依赖 | [ports.rs](../../../crates/core/src/ports.rs) 第 270–300 行 |
| Host 的通用领域 RPC 按 GitHub 校验 | 概念反向依赖 | [domains.rs](../../../crates/host/src/domains.rs) 第 236、323–326 行 |
| 通用工作流引擎按工程工具名分支 | 概念反向依赖 | [configured_workflows.rs](../../../crates/core/src/configured_workflows.rs) 第 897、1033 行等 |
| 证据门禁只认 `git.delivery`，并读取领域的端口名 | 概念反向依赖 | [platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) 第 358、373–375 行 |
| 内核测试夹具包含领域运行时文件 | 测试期反向依赖 | [adapters/src/tests.rs](../../../crates/adapters/src/tests.rs) 第 13–28 行 |

完整清单见 [审计清单](AUDIT.md)。

### 3.2 清晰度

| 维度 | 现状 |
|---|---|
| 可发现性 | 没有任何一份文档告诉领域作者：能发布哪些类型、能扩展哪些能力、处理器拿到的数据长什么样。这些只存在于 Rust 源码和一个样例里 |
| 单一可信来源 | 没有。处理器数量在文档与源码之间已经漂移（54 与 55）。`domain.read` 在仓库任何配置目录里都找不到，由 `build.py` 生成 |
| 版本 | 只有两个整数：`/capabilities` 的 `domain_protocol: 1`（[main.rs](../../../crates/host/src/main.rs) 第 496 行）和 worker 的 `api: 1`。没有细粒度版本，领域包无法声明自己依赖哪些处理器、钩子或 RPC |
| 类型与校验 | 安装侧校验完整；但 `facts` 条目、各钩子输出等形状只存在于 Rust 结构体，领域作者看不到 |
| 错误反馈 | 处理器的错误信息在内核处被丢弃，作者只能得到“执行失败” |
| 脚手架 | 没有。处理器协议靠手写，构建脚本靠复制 `build.py` |
| 构建独立 | 领域 ZIP 的构建需要读取内核仓库的两个文件 |

### 3.3 可演进性

协议没有版本协商，意味着任何一方变更都只能靠“同版本同内容”的 ZIP 摘要和 Host 升级一起推进。新增一个 Harness provider 需要改 domain crate 的枚举、adapters 的驱动、Schema 的枚举、出厂资源包和全局库。这与 [harness-shared-foundation](../../../docs/plans/harness-shared-foundation/PLAN.md) 计划中的“编译期 provider 注册”口径一致，是当前有意的选择。问题不在这个选择本身，而在于它没有被声明为 SDK 的显式约束。
