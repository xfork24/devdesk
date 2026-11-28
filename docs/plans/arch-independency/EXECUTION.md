# 执行报告

本文件按阶段记录[改造计划](PLAN.md)的实际执行。每个阶段一节，写明提交、验收命令、实际结果、与计划的偏差和未验证的范围。结论对应当时的工作区；代码继续改动后以符号名为准。

## P0 守卫与基线

| 项 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 分支 | `arch-independency`，独立 git worktree |
| 起点 | `main@e84e2218` |
| 结论 | 门禁 0 通过。两条守卫已进入 `check.sh` 与 CI；已知例外清单在 P1 的 G-3 清空 |

### 提交

| 工作项 | 提交 | 内容 |
|---|---|---|
| G-4 | `ec2221a6` | 内核测试执行器改用 `tests/fixtures/reporters/` 的冻结报告器；`export_runtime_recipes` 经公开的 `devdesk_adapters::packaged_runtime` 读取内嵌资源，不再读取 `plugins` 目录 |
| G-1 | `116dd939` | `check-architecture.py` 增加内核与领域包的边界：领域包路径与命名空间、出厂资源目录、已退役的出厂标识。已知例外精确到“文件加规则”，新增 6 个回归测试 |
| G-2 | `6fee45bc` | `check-web-boundaries.mjs` 对照基线检查 Web import 边界，新增 3 个回归测试，进入 `check.sh` 与 `web.yml` |
| G-5 | `a335547f` | 平台处理器数量改为 55，并说明 `domain.read` 的来源；说明 Feature 与 Task 的当前归属与去向 |

### 初步验收

| 工作项 | 命令 | 结果 |
|---|---|---|
| G-4 | `cargo run -p devdesk-adapters --example export_runtime_recipes --locked`，与改造前的输出 `cmp` | 4557 字节逐字节相同 |
| | `shasum -a 256 tests/fixtures/reporters/*` | 两个摘要与原有的两个常量一致 |
| | `cargo test -p devdesk-adapters --locked --lib reporter_tests` | 2 通过 |
| G-1 | `python3 tests/entrypoints/test_architecture_checker.py` | 6 通过 |
| | `./scripts/check/check-architecture.sh` | 通过，已知例外恰好是 A1 与 A2 |
| | 反向验证：清空例外清单 | 报出 A1 的 6 处与 A2 的 2 处，退出码 1 |
| G-2 | `node --test tests/entrypoints/web_boundaries.mjs` | 3 通过 |
| | `node scripts/check/check-web-boundaries.mjs` | 通过。基线：核心到领域 0 条，领域到核心 2 条 |
| | 反向验证：新增两个越界 import；删除一个已记录的越界 | 两种情况都是退出码 1 |
| G-5 | `python3 scripts/check/check-docs.py` | 通过 |

### 门禁 0

在提交 `a335547f` 上运行，环境为 macOS arm64、Docker 29.9.3、Rust 1.99.1、Node 24.13.0。

| 范围 | 结果 |
|---|---|
| 权限资产、出厂目录、架构检查、Web 边界、文档、RPC 登记、Host 启动器、node 与 live 单元测试 | 通过 |
| `check-style.sh`：rustfmt、clippy、ESLint、TypeScript、Prettier、Web 构建 | 通过 |
| `cargo test --workspace --locked` | 通过 |
| debug 夹具 Host 的 6 组合同：生命周期、事件尾流、HTTP、模板重置、数据集协议、运行环境准备 | 通过 |
| `prepare_browser_runtime.py` | **中断，退出码 1**。`docker pull` 无法连接 Docker Hub，`registry-1.docker.io` 的连接被重置 |
| 浏览器用例，debug 轮 | 162 通过，0 失败 |
| release 夹具 Host 的构建、签名与 6 组合同 | 通过 |
| 浏览器用例，release 轮 | 160 通过，0 失败 |

完整脚本在镜像预热处退出。该脚本只做一次拉取来预热 Sandbox 基础镜像，而这个镜像本地已经存在。随后在同一提交的独立检出中补跑了剩余步骤，唯一的差别是拉取失败时沿用本地镜像。网络恢复后可以直接重跑完整脚本复核。

### 与计划的偏差

1. **G-4 先于 G-1 提交。** 新守卫会报出 B1 与 B3，先清理才能让每个提交都通过检查。
2. **B3 的处理范围扩大了。** 示例程序改为经公开的 `packaged_runtime` 读取内嵌资源，并按 `HARNESS_PROVIDERS` 遍历。后者原本是 Harness 扩展计划为该示例列出的后续改动，这里提前完成。
3. **G-2 用独立脚本实现，没有用 ESLint 规则。** lint 是零警告门禁，`warn` 级别会直接失败。“先以警告运行”落实为基线：`scripts/check/web-boundaries.json` 记录现存的 2 条越界，新增即失败，消除后必须删除记录。import 的提取使用 TypeScript 的预处理，覆盖静态导入、再导出、`require` 和动态 `import()`。
4. **G-1 对“非测试代码”的判定**会识别经 `#[cfg(test)] mod` 声明、只在测试下编译的文件（例如 `workflow_live.rs`），集成测试目录不计入。核对时发现 Host 的两个本地探针写死了旧出厂的交付标识，它们都属于这类测试模块，不是生产代码。

### 未验证范围

- 没有运行真实模型或 GitHub。
- 浏览器用例里需要联网拉取镜像的分支依赖本地已有的镜像缓存。

## P1 协议核心

实施中。提交哈希在本阶段的门禁通过后统一补记。

### 已完成的工作项

| 工作项 | 内容 |
|---|---|
| G-3 | 删除 core 对六个单体时代角色标识的归档逻辑，经验按角色筛选只匹配确切标识；架构检查的已知例外清空 |
| N-8 | 结论：**无仓库的项目可以直接运行 Agent 步骤**，不需要新增文档工作区。新增回归测试 `title_only_project_runs_an_agent_step_without_a_repository_or_workspace` |
| K-5 | 工程钩子从清单 `runtime` 的顶层字段移到 `extensions`，按内核登记的扩展组绑定。新增 `core/src/extensions.rs` 登记两个扩展组；core 的 `DomainReader` 端口只剩 `read`、`has_hook`、`hook`、`resource`；五个工程调用点改走通用钩子，结果的语义校验移到内核调用方 |
| K-1 | 清单格式 2：`format` 2、`domain_protocol` 2、`requires`。格式与协议在 Schema 之前核对，旧格式的包得到指明两个版本的 `domain_protocol_unsupported` |
| K-2 | `GET /capabilities` 增加 `package_format`、`platform_handlers`、`core_inputs`、`extensions`、`rpc`，`domain_protocol` 为 2 |
| K-3 | 安装期协商：Host 版本、平台处理器、扩展组、RPC 四类依赖逐项核对，错误指明缺什么。扩展组同时登记自己提供的处理器与内核输入；包只有声明依赖某个组，才能发布该组处理器上的工具、把它列为交付的生产者、声明该组的内核输入，或绑定该组的钩子与资源 |
| K-4 | 领域包的配置包清单在安装时套用与出厂资源相同的 Schema。两个构建脚本输出全部 15 个类型键，未使用的留空 |
| N-6（第一部分） | 内核输入改为能力目录中的名称：`git_head`、`git_snapshot` 更名为 `engineering.git.head`、`engineering.git.snapshot`，名称带着提供它的扩展组。Schema 不再枚举取值；处理器声明了本 Host 没有的内核输入时，按名称报 `handler_unsupported`。`/capabilities` 的 `core_inputs` 只列每个包都能用的内核输入，`extensions` 的每一项改为带 `handlers`、`core_inputs`、`hooks` 的对象，与 SDK 合同第 4 节一致 |
| N-3 | 新增 `query.domain.entities`：按对象类型分页列出项目的对象，每页最多 100 个，`after` 是上一页最后一个对象的 ID，顺序按存储键稳定，不代表时间先后。类型须由活动包声明。每条记录是 `query.domain.entity` 返回的对象加上 `workflow`：`active` 表示归属对象有在途的根流程，`last` 是最近一次启动的根流程（`id`、`status`、`workflow_ref`） |
| N-6（第二部分） | 新增内核输入 `domain.entities`。处理器用 `entities: {types, max_items}` 声明要读取的类型和数量上限，类型须由本包声明，不需要任何扩展组。内核在调用前从处理器所在项目的库读取对象，返回 `{entities, truncated}`；超过数量上限或总大小约 512 KiB 时停止，并说明有对象没有给出。三条调用路径都接通：RPC `query.domain.invoke` 需带 `project_id`，缺失时返回 `invalid_params`；工作流的工具步骤经内核的 `DomainReader` 读取，Host 的项目读取器持有该项目的库，调用方不必指定项目；钩子同理。能力目录的 `core_inputs` 列出它 |
| N-5 | 包的运行入口是纯模块，只导出处理器表 `handlers`，不再读写标准输入输出。容器里运行的是内核的引导模块（`crates/adapters/runtime/domain-worker.mjs`，编入 Host），它读取请求、加载入口、按名称调用处理器并写回结果，激活时的握手也由它应答。处理器拒绝或失败时抛出 Error：带 `code` 的错误连同信息由 Host 以 `domain_handler_rejected` 返回调用方，RPC 错误的 `data.handler_code` 与 `data.handler` 是处理器的代码与名称；没有代码的记为 `handler_failed`。入口、引导模块与传输的问题记为 `domain_worker_failed`，信息来自 worker。软件开发包与夹具的入口改为纯模块，软件开发包的五处拒绝带了代码。`GET /capabilities` 新增 `worker`：协议版本 2、镜像摘要、`node-24`、15 秒、128 MiB、1 CPU、48 个进程、无网络、只读文件系统，取值与沙箱参数同源 |
| N-1 | 工具定义新增 `implementation: "domain"`：名称在包的命名空间下，`handler` 指向包内注册的处理器，参数就是处理器的输入。它既可以作为工作流的工具执行器步骤，也可以被 Capability 采用后由 Agent 直接调用：调用名与平台工具同一规则（`devdesk_` 加下划线），经平台只读准入（需要工作流执行归属），由领域处理器在沙箱里执行。它在 Agent 的冻结工具锁里单独成类（`domain_tools`），Harness 像告知内核工具一样把它告知 provider。安装时核对领域工具的命名空间与处理器，并拒绝发布了本 Host 没有的平台处理器的工具。`domain.read` 及其平台处理器、RPC 登记一并移除，平台处理器为 54 个。软件开发包的 `detect-test-entry` 与夹具的工具改为领域工具 |
| N-7 | 工作流的输入端口可以声明 `provided_by`，取值为扩展组。启动时内核只为声明了提供者的输入自动取值：提供者必须是该扩展组登记提供的输入（扩展组登记表新增 `provided_inputs`，能力目录里每个扩展组也列出它），否则发布工作流时返回 `invalid_configuration`；有活动领域包时，项目所属的包必须声明依赖该组，否则启动时返回 `extension_unavailable`；没有活动领域包时工作流来自内核自己的目录，提供者按构造可用。调用方不能覆盖内核提供的输入。按名字自动填充 `test_configuration` 的约定移除，目前唯一的提供者是 `engineering.tests/1` 的 `test_configuration`。软件开发包的两个工作流与单体测试模式的冻结工程目录都声明了提供者 |
| N-4 | 项目的接入信息对内核不透明。`domain.project.create` 带 `connection` 时，活动包的 `project_connection` 处理器解析它，结果原样保存为项目的 `connection`（不超过 64 KiB 的 JSON 对象，随 `query.workspace` 等返回）。只有声明了 `engineering.git/1` 的包，内核的工程部分才从接入信息里读出仓库与主分支（`extensions::git_connection`），据此登记仓库、复用或接管 GitHub 项目；通用 RPC 不再按 GitHub 校验。`domain.project.remove` 对任何领域项目可用：移除会藏起来的在途工作（在途根流程、可运行或待核对的仓库操作）时返回 `domain_busy`，空闲的连接复核不阻止移除。为此项目库新增一列，工程库版本升到 63，带 62 及更早版本的离线升级。Web 的移除入口不再以包声明 `project_connection` 为前提 |
| K-7 | 版本号：工作区、Web Core 与软件开发包都升到 0.6.0，`domain_protocol` 为 2、清单格式为 2。制品名随版本变化：`devdesk-core-host-0.6.0-<target>.tar.gz`、`devdesk-core-web-0.6.0.tar.gz`、`software-development-0.6.0.zip`。Web 的组件套件版本为 `0.6`，软件开发包的呈现目标随之声明 `kit: "0.6"`。运行指南的兼容矩阵改为三个工件的当前合同，删去了旧版 ZIP 的行 |
| N-2 | 证据合同通用化。合同的 `checks` 以被守护的操作为键（工具的 `qualified_name` 或 `$completion`），内核不再只读 `git.delivery` 一个键，触发点也不再按四个工程工具名写死：任何工具步骤（含领域工具）执行前，内核为守护它的每个合同封存事实、组装输入并调用验证器，拒绝时工具不执行。工作流完成前，每个带 `$completion` 的合同同样被询问；回答按事实摘要记录，事实不变时再次尝试直接沿用。验证器收到 `{contract, operation, context, facts}`：`context` 由合同声明，来源有 `owner`、`input`、`arguments`、`platform` 四种，字段可用 JSON pointer 取值；`facts` 条目由内核类型 `EvidenceFact` 产生，包在验证器的输入 Schema 里描述同一形状，并以一致性测试约束。完成证据不再强制要求平台操作：标注了平台操作的由内核见证，其余是封存的步骤产物，由验证器权衡。软件开发的 `git.delivery` 合同改写成同一形式，验证器改读声明的上下文；`platform` 来源（来源分支、目标）与子流程核对仍在内核的工程部分 |
| K-6（第一部分） | 内核的出厂目录从 `plugins/core` 改名为 `kernel/core`：它由 Host 构建时内嵌，不是领域包，不经过 ZIP 的安装与协商。`plugins/` 下只剩领域包。构建脚本、架构守卫、默认值检查、权限检查、引用索引、测试与文档都指向新路径；`kernel/README.md` 说明它与领域包的区别，`plugins/README.md` 只列领域包 |
| K-6（第二部分） | 工具定义随提供者发布。内核目录 `kernel/core` 发布 41 个 platform 工具和 2 个 Harness 原生能力声明（`builtin-` 前缀），构建时内嵌进 Host，对所有项目可用；新增工程扩展目录 `kernel/engineering`，发布 8 个工程工具（`tests.*`、`git.*`、`workspace.*` 处理器，`engineering-` 前缀），同样内嵌，但只在项目所属的包声明了对应扩展组时才被采用（项目建立与打开时）。软件开发包删去 40 个工具定义，改为引用两类内核源，包的 Capability 与工作流里的引用改用新标识。安装时：包引用工程工具而未声明所在扩展组，返回 `extension_unavailable` 并指明组；包引用本 Host 没有的平台处理器，返回 `handler_unsupported`。领域包的目录视图包含其被给予的工程工具（否则项目的副本会被读作历史源）。领域目录 `software-development/catalog/tools` 移除，引用索引由内核目录与工程扩展目录各自生成 |
| W-5 | 呈现目标用 `sdk`（Web SDK 主版本）取代 `api`，新增 `kit`（组件套件对应的 Web Core 版本）与绑定的 `home`（进入领域时打开的首页，取代“第一个绑定”）。Web Core 只加载 `sdk` 与自身一致、`kit` 与自身版本一致的目标，其余目标提示“领域包与 Web Core 版本不兼容”。内核只校验 `sdk` 不小于 1、每个目标最多一个首页；SDK 版本是否可用由 Web Core 判断 |

### 初步验收

| 工作项 | 命令 | 结果 |
|---|---|---|
| G-3 | `cargo test -p devdesk-core --locked --lib configuration_catalog` | 2 通过 |
| | `cargo test -p devdesk-host --locked --bin devdesk-host configuration_read` | 9 通过 |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test experience_directory --test experience_imports --test conversations --test governance --test agent_capabilities` | 36 通过 |
| | 反向验证：先删代码、后清例外 | 架构检查报出 3 条过期的例外，退出码 1；清空后通过 |
| N-8 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- title_only_project_runs_an_agent_step_without_a_repository_or_workspace` | 1 通过 |
| | 同一套件中现有的 4 个用例：`automatic_review_blocks_rejection_and_advances_after_rework_without_confirmation`、`dynamic_spec_rejection_cannot_be_confirmed_and_rework_keeps_the_original_report`、`explicit_manual_approval_requires_reason_and_exact_execution_and_preserves_rejection`、`cancelling_a_failed_workflow_remains_schedulable_and_allows_a_fresh_run` | 4 通过。它们同样用只有标题的项目启动带 Agent 步骤的流程 |
| K-5 | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 4 通过。覆盖合法绑定、钩子只接受所在组要求的内核输入、未登记的组与钩子与集合被拒、项目只看到所属包的钩子与资源 |
| | `cargo test --workspace --locked` | 60 个套件，786 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无警告 |
| | `./scripts/check/check-architecture.sh`、`check-permissions.sh`、`check-defaults.sh`、`check-docs.py`、`check-web-boundaries.mjs`、`rpc_contract_inventory.py` | 全部通过 |
| K-1、K-2、K-3 | `cargo test -p devdesk-core --locked --lib domain_protocol` | 2 通过。版本比较与四类协商错误 |
| | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 8 通过。协商与门禁用真实软件开发包改写清单：格式或协议为 1、Host 版本过高、缺处理器、缺扩展组、缺 RPC，以及未声明依赖的工程工具、内核输入、钩子与资源 |
| | `cargo test --workspace --locked` | 60 个套件，792 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无警告 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` | 通过，含新增的能力目录断言 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <格式 2 的软件开发包>` | 退出码 0。真实 Docker worker 与浏览器：安装、激活、调用钩子、渲染页面 |
| K-4 | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 9 通过。新增一项：从真实包的配置包清单里删去一个类型键后被拒绝 |
| N-6（第一部分） | `cargo test -p devdesk-core --locked --lib domain_protocol` | 2 通过。协商按扩展组对象核对，断言 Git 组发布的处理器、内核输入与钩子 |
| | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 11 通过。新增一项：真实包的 `tests.discover` 使用 `engineering.git.snapshot`；改回旧名称或不存在的名称时被拒，错误信息带着该名称 |
| | `cargo test --workspace --locked` | 60 个套件，795 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及事件尾流、模板重置、数据集协议、运行环境准备四组合同 | 通过。能力目录断言改为扩展组对象 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。真实包经新名称读取 Git 快照 |
| N-3、N-6（第二部分） | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- entities_are_listed` | 1 通过。真实项目库：按类型过滤、游标分页不重不漏、在途根流程的 `active` 与 `last`、处理器输入的数量上限与 `truncated`、非法查询被拒 |
| | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 12 通过。新增一项：`entities` 选择里的类型须由本包声明，缺少选择或多了选择都被拒绝，不需要扩展组 |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test project_test_configuration -- --ignored` | 2 通过。显式启用的 Docker worker 用例，见“执行中的发现” |
| | `cargo test --workspace --locked` | 60 个套件，797 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同 | 通过。能力目录的 `core_inputs` 为 `["domain.entities"]` |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。新增三段：经 RPC 分页读取文稿与校样，带在途与已完成工作流的摘要，未声明的类型与非法参数被拒；经 `query.domain.invoke` 带 `project_id` 调用读取对象的处理器，不带项目时被拒；工作流的一个工具步骤经内核读到所在项目的两个文稿，结果成为校样对象 |
| N-5 | `node --test tests/entrypoints/domain_worker.mjs` | 6 通过。引导模块在本机 Node 上运行：握手按序列出处理器、处理器收到输入与内核输入、带代码与不带代码的拒绝、入口与引导模块的失败、不能序列化的结果、不是 JSON 的请求 |
| | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 13 通过。新增一项：worker 应答映射为结果、`domain_handler_rejected`（带处理器的代码）或 `domain_worker_failed` |
| | `cargo test -p devdesk-host --locked --bin devdesk-host an_error_keeps` | 1 通过。RPC 错误保留内核错误码，领域处理器的拒绝另带处理器名与代码 |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test project_test_configuration -- --ignored` | 2 通过。真实 Docker worker 经引导模块执行软件开发包的处理器 |
| | `cargo test --workspace --locked` | 60 个套件，799 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同 | 通过。新增能力目录 `worker` 字段的断言 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。全部处理器调用经引导模块；新增一段：处理器拒绝后，RPC 错误带着它的代码 `title_rejected` 与信息 |
| N-1 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- a_domain_tool` | 1 通过。没有处理器的领域工具不能发布；工具步骤与 Agent 的直接调用都到达包的处理器，调用名是 `devdesk_sample_echo`，结果回到 Agent；冻结锁里只有 `domain_tools` |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --lib` | 15 通过。新增两项：包的领域工具须在本包命名空间下并指向本包注册的处理器，`domain.read` 上的工具被拒；Harness 把领域工具告知 provider |
| | `cargo test --workspace --locked` | 60 个套件，802 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过。软件开发包的测试入口识别经领域工具步骤在真实 Docker worker 上执行 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。夹具的三个工具步骤都是领域工具步骤 |
| N-7 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- a_workflow_input_is_filled` | 1 通过。未声明提供者的同名输入不再被填充；声明了不提供该输入的组时发布被拒；包没有依赖该组时启动返回 `extension_unavailable`；依赖之后，以及没有活动领域包时，都按项目的测试配置填充 |
| | 同一套件中依赖这一约定的 `actual_git_candidate_is_a_test_grant_only_in_its_original_root`，以及使用冻结工程目录的 `actual_defaults_compile_the_streamlined_suite_and_freeze_project_tests` 等 4 个用例 | 5 通过 |
| | `cargo test -p devdesk-core --locked --lib domain_protocol` | 2 通过。能力目录里每个扩展组带 `provided_inputs` |
| | `cargo test --workspace --locked` | 60 个套件，803 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。软件开发包带着两个声明了提供者的工作流安装并运行 |
| N-4 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test project_creation -- a_project_keeps_the_connection` | 1 通过。接入信息原样保存，重新打开后仍在；不是对象或超过上限的被拒 |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test dataset_preflight -- a_project_database_of_version_62` | 1 通过。62 升到 63，项目行不变，再次迁移报告无变化 |
| | `cargo test --workspace --locked` | 60 个套件，805 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的格式 2 软件开发包>` | 退出码 0。新增两段：有在途工作流的项目不能移除；没有声明 `project_connection` 的领域里，只凭标题创建的项目可以移除，并不再列出 |
| K-7 | `cargo test --workspace --locked` | 60 个套件，805 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的 0.6.0 软件开发包>` | 退出码 0。界面显示 v0.6.0；软件开发包 0.6.0 以 `requires.host` 0.6.0 安装并运行 |
| N-2 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- a_contract_guards` | 1 通过。放行时验证器在工具前与完成前各被询问一次，收到声明的上下文（归属对象、操作参数、工作流输入）与封存的事实，工具在两次询问之间执行；拒绝工具时工具不执行，也不再有别的询问；拒绝完成时流程不完成，之后的尝试沿用回答 |
| | `cargo test -p devdesk-adapters --locked --lib the_software_verifier_describes` | 1 通过。内核的事实带全部绑定时、不带绑定时都符合软件开发包验证器 Schema 里的条目形状，多出的字段与缺少的执行身份被拒 |
| | `node --test tests/entrypoints/domain_delivery_evidence.mjs` | 4 通过。验证器按声明的上下文判断，`$completion` 放行 |
| | `cargo test --workspace --locked` | 60 个套件，807 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的 0.6.0 软件开发包>` | 退出码 0。软件开发包带着改写后的交付合同安装并运行 |
| K-6（第一部分） | `python3 tests/entrypoints/test_architecture_checker.py`、`./scripts/check/check-architecture.sh`、`scripts/check/check-defaults.py`、`check-permissions.sh`、`configuration_usage.py` | 通过。架构守卫里“点名出厂目录”的规则改为 `kernel/core`，只有 `crates/adapters/build.rs` 可以点名 |
| | `cargo test --workspace --locked` | 60 个套件，807 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored`、`node tests/entrypoints/domain_packages.mjs` | 通过，退出码 0 |
| K-6（第二部分） | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 15 通过。新增两项：声明扩展组的包得到恰好 8 个工程工具（按处理器列出），未声明的包一个也得不到；软件开发包验证器 Schema 里的证据条目形状与内核的 `EvidenceFact` 一致 |
| | `scripts/check/configuration_usage.py`、`check-defaults.py` | 通过。各自生成并核对两个内核目录的引用索引（core-v1 43 条、engineering-v1 8 条），闭合检查覆盖软件开发包对它们的引用 |
| | `cargo test --workspace --locked` | 60 个套件，808 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings`、`./scripts/check/check-style.sh` | 无警告；rustfmt、ESLint、TypeScript、Prettier、Web 构建通过 |
| | `python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host` 及四组夹具合同、`project_test_configuration -- --ignored` | 通过。测试入口识别仍经真实 Docker worker |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <带页面的 0.6.0 软件开发包>` | 退出码 0。新增两段：软件开发包的项目目录里有 `engineering-tool-*` 与 `builtin-tool-*`；没有扩展组的领域一个工程工具也没有 |
| W-5 | `cargo test -p devdesk-adapters --locked --lib domain_packages::tests` | 10 通过。新增一项：一个目标标记两个首页后被拒绝 |
| | `cargo test --workspace --locked` | 60 个套件，794 通过，0 失败，39 忽略 |
| | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无警告 |
| | `npm --prefix web run build:core` | TypeScript 与 Vite 构建通过 |
| | `node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core <格式 2 的软件开发包>` | 退出码 0。新增一段：安装 `sdk: 2` 的夹具包后打开它的页面，显示不兼容提示。软件开发包激活后落在标记为首页的“工作总览” |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test schema_contracts --test project_test_configuration` | 28 通过，2 忽略 |

### N-8 的依据

会话在没有工作区租约、也没有代码视图的情况下打开，步骤正常完成。`harness_workspace::prepare` 在这种请求下直接返回私有工作目录，不要求仓库。因此 V1 的风险不成立。

### 执行中的发现

- **仅标题的项目仍然带有工作区定义。** `project.create` 总是写入 `workspace_definition`，其中 `repository` 为空字符串、`mainline` 为 `main`。这说明内核的项目模型带着工程形状，由 N-4 处理。
- **历史 ZIP 夹具不能再用于测试。** 三个单元测试和两个 Docker 用例直接安装 `software-development-0.3.0.zip`，清单形状一变它就不再合法。夹具改为由 `plugins/software-development/build.py --runtime-only` 生成的当前格式包 `software-development-current.zip`，只含运行端与配置。`domain_release.mjs` 原来用历史 ZIP 做跨版本回切，现在改用这份夹具并从清单读取版本号；该用例需要单独构建的制品和 Docker，本轮没有运行。
- **`requires.extensions` 由包显式声明，不从内容推导。** 声明依赖表达包作者的意图，内核负责核对包的内容没有越出声明。`requires.handlers` 则由构建脚本从包引用的工具推导，包内定义与出厂定义都算。
- **`requires.host` 取自 `Cargo.toml` 的 workspace 版本。** 包与 Host 从同一检出构建时，恰好满足“不低于”。
- **格式与协议在 JSON Schema 之前核对。** Schema 的 `const` 只能回答“不符合 Schema”，说不出本 Host 接受什么。
- **能力目录暂时不含 worker 的运行时限制。** 这些数字现在写死在 worker 的容器参数里，N-5 重写 worker 时以具名常量发布。
- **钩子要求的内核输入是登记的数据。** 扩展组登记表里的每个钩子声明自己要求的内核输入，通用校验只核对“绑定到钩子的处理器声明了输入输出 Schema，并使用该输入”，不再写死“测试识别要用 Git 快照”。
- **测试夹具包不声明首页。** 声明首页后，Web Core 按设计不再提供自己的“领域工作流”入口，而端到端用例要在那里读取执行。首页路径改由真实软件开发包的那一段覆盖，夹具包继续覆盖没有首页的形态。
- **对象行的类型只在载荷里。** `domain_entity` 行的 `definition_id` 是存储键（`domain-entity:<摘要>`），不是对象类型；类型是 `name` 列，也在载荷里。对象列举按载荷的 `type_ref` 过滤，以载荷为准。
- **对象的 `created_at` 不是创建时间。** 通用写入在更新时会覆盖除 `id` 以外的全部列，每次修订都刷新它。因此分页顺序用不可变的存储键，客户端需要时间顺序时从对象自己的数据里取。
- **在途判定只有一份定义。** 对象列举与 `workflow_active_owner` 虚拟资源使用同一段 SQL（`active_root_workflows`）；“最近一次”按 `rowid`，即启动顺序。
- **两个默认忽略的 Docker 用例早已失效。** `project_test_configuration.rs` 里显式启用的两个用例从 K-5 起失败：测试替身没有实现 `has_hook`，包装的 Git 没有转发 `registered_revision`。其中一处断言更早（main 的 `6184df1d`）就因 `domain_project_busy` 增加 `active_when_removed` 字段而过期。已修复，并把断言改为只看 `active`。这类用例不在任何门禁里，纳入测试改造。
- **内核输入的名称形如 RPC 方法。** `domain.entities` 会被 RPC 登记检查当作未登记的方法字面量，需要列入 `non_methods`。
- **处理器的代码原本在 RPC 边界丢失。** Host 的 RPC 错误只带内核的错误码与信息。现在 `domain_handler_rejected` 的 `data` 另带 `handler` 与 `handler_code`，其余错误的形状不变。
- **不能序列化的结果算处理器的失败。** 结果来自处理器，记为 `handler_failed`；引导模块与传输的问题才是 `domain_worker_failed`。
- **引导模块的测试进了两处门禁。** `check.sh` 的 Node 单元测试列表与 `rust.yml` 的 Rust 作业都会运行它，它不需要 Docker。
- **夹具的 `requires.handlers` 自 K-3 起就是错的。** 夹具构建器按错误的键格式筛选采用的内核工具，结果一直只有 `domain.read`。现在从源记录里的 `tool_ref`、`tools`、`tool_refs` 推导，与软件开发包的构建脚本同一做法；文稿夹具声明了它采用的 5 个 `workflow` 处理器。
- **“由内核执行的工具”原先散落在多处。** 发布、Capability 组合、冻结锁、工作流编译的采用工具与工具步骤、交付生产者、平台请求的准备与准入、动态工具，都以 `implementation == "platform"` 判断。现在由 `ToolDefinition::kernel_executed` 与同一模式 `platform | domain` 表达；只有按处理器名分派的地方才区分两者。
- **领域工具目前只在工作流执行归属下可被 Agent 调用。** Controller 授权路径仍只认经理与编排的工具清单。宿主 MCP 桥与 pi 隧道的工具名注册表要把 `domain_tools` 作为单独一类，按 0.5 节“谁后落地，谁负责补齐”。
- **“扩展已启用”需要一条明确的规则。** 没有活动领域包时，工作流只能来自内核自己的目录，它声明的提供者按构造可用；只有在有活动领域包时才核对包是否依赖该组。单体测试模式（冻结工程目录）依赖这一条。P2 移除单体测试模式后，这条规则只剩“空领域”一种情形。
- **冻结工程目录是单体测试模式的数据。** `tests/fixtures/configuration/engineering-catalog.json` 里两个工作流的 `test_configuration` 输入补了 `provided_by`，随测试改造一并处理。
- **“忙”有两种。** 切换与卸载用的 `domain_project_busy` 把空闲的连接复核也算在内，因为复核入口会随切换消失。移除项目只会藏起在途工作，复核可以等到重新打开。两处判定分开，第一次验收时复用同一个检查，软件开发包里一个带待复核连接的项目因此不能像以前那样移除。
- **新增持久化字段要跟着工程库版本走。** 仓库对项目库有版本号与离线升级（暂存候选库、校验、备份），这里按既有做法补了 62 到 63，而不是只改建表语句；已有数据集打开时仍被接受，经升级后才有这一列。
- **内核外壳里还没有项目管理页。** “移除项目”插槽目前只在软件开发包的工作台里渲染。按领域移除项目的界面入口随 W-4 的项目操作插槽落地，RPC 与 Web 的门槛条件已就位。
- **版本来自三处。** `Cargo.toml`、`web/package.json` 和软件开发包构建脚本的 `VERSION`。发行脚本与两个构建脚本都从这三处读取，没有第四份清单。`domain_release.mjs` 里写死的制品名同步更新；该用例需要发行制品，本阶段没有运行。
- **协议客户端不再按旧版本号分支。** `domain_client.py` 原先按软件开发包的三个旧版本号决定是否断言默认流程，现在直接断言当前的默认流程。
- **只读工具步骤在授权后立刻执行。** 值类与领域工具步骤在平台调用的授权提交之后马上执行，仓库交付才走效果意图与恢复路径。验证要加在前一条路径上，只改后者时门禁对这些步骤不生效。
- **完成证据原先必须是平台操作。** 这会拒绝以人工确认为完成证据的合同。现在标注了平台操作的由内核见证，标注人工确认或未标注的是封存的步骤产物，交给合同的验证器权衡。
- **完成前的拒绝需要被记录。** 只返回错误的话，驱动器每一轮都会重新询问验证器。回答按事实摘要记录并沿用，才有“事实不变不自动重试”。
- **仓库交付合同还有三处留在内核的工程部分**，随工程扩展模块迁出：`platform` 来源（来源分支与目标）、名为 `children` 的事实触发的子流程核对、人工确认与发布参数的绑定。内核仍以 `git.delivery` 这个键区分该合同。
- **按步骤 key 守护非工具步骤尚未实现。** 触发点在工具步骤的准备里。电商设计里的三个门禁守护的都是工具。
- **“领域目录视图”是包源与项目副本之间的桥。** 项目里的全局定义是否算“当前”，由组合出的领域目录视图决定：视图里没有、项目里有的定义被读作历史源。工程工具随包按组提供后，视图也要包含它们，否则刚被采用的工程工具立刻显示为过时。这条规则随“目录即视图”的设计在 P2 复核。
- **包对内核源的引用闭合在安装时核对。** `validate_domain_configuration` 现在收第三个参数：包可以引用的内核源（内核目录全部源，加上包要求的扩展组的工程工具）。这代替了“引用闭合只对包自己的源成立”的隐含约定。
- **软件开发包的 Capability 引用了 40 个内核工具。** 引用改写为新的 `builtin-tool-*` / `engineering-tool-*` 标识后，包的 `requires.handlers` 推导不变——构建脚本从两个内核目录读取工具定义。
- **`requires.handlers` 含义未变，但来源变了。** 包不再“发布”平台工具，只“采用”；49 个处理器的清单正好是内核目录 41 个加工程 8 个里被引用的部分。
- **报告器的扩展名规则不再由清单校验负责。** 清单只做通用的有界 UTF-8 文件检查；落地时的文件名一直由测试运行器自己决定。
- **读取项目的钩子与资源时用 `try_read`。** 与 `configuration_scope` 的做法一致：项目 RPC 可能已经持有一份读锁，切换领域时直接返回 `domain_busy`，不等待嵌套的锁。
- **单独运行适配器的集成测试需要 `--features test-fixtures,fixture-catalog`。** 只开 `test-fixtures` 时出厂目录不含角色与 Agent，依赖默认绑定的用例会失败。`cargo test --workspace` 通过 Host 的开发依赖带上了这两个 feature，所以完整检查不受影响。这正是 T-6 要消除的耦合。
