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
- **报告器的扩展名规则不再由清单校验负责。** 清单只做通用的有界 UTF-8 文件检查；落地时的文件名一直由测试运行器自己决定。
- **读取项目的钩子与资源时用 `try_read`。** 与 `configuration_scope` 的做法一致：项目 RPC 可能已经持有一份读锁，切换领域时直接返回 `domain_busy`，不等待嵌套的锁。
- **单独运行适配器的集成测试需要 `--features test-fixtures,fixture-catalog`。** 只开 `test-fixtures` 时出厂目录不含角色与 Agent，依赖默认绑定的用例会失败。`cargo test --workspace` 通过 Host 的开发依赖带上了这两个 feature，所以完整检查不受影响。这正是 T-6 要消除的耦合。
