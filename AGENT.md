# Repository Guidelines

## 事实源与工作范围

先检查 `git status`，保留用户已有改动。当前系统行为以 `crates/`、`web/src/`、`schemas/`、`sql/` 与已发布出厂配置为准；`docs/` 根目录 Markdown 是现行说明和明确标记的设计约束，`docs/plans/` 保存待实施方案与历史验收，不能用未实施方案或旧计划覆盖代码事实。更新文档直接描述当前状态，不追加修订历史；历史计划保持原文，只有用户明确要求的机械修正例外。

项目为 Rust 内核、React Web 和 SQLite 分库的本地工程工作台。生产执行只使用真实外部端口；接口替身和协议夹具只用于测试。默认行为和发行版本 pin 属于配置资源，不在内核按业务名称硬编码。

## 目录与边界

| 路径 | 职责 |
|---|---|
| `crates/domain` | 领域类型、纯规则、源图、Spec 与有界校验；不执行 I/O，不直接取时间、随机数或环境。 |
| `crates/core` | 配置编译、冻结视图、命令/查询、会话组合、工作流/驱动/经理用例、权威终态；外部依赖通过 `ports.rs` 注入。 |
| `crates/adapters` | SQLite/Artifact codec、出厂资源加载、Git/GitHub、Docker、Codex/DSH、运行准备、系统凭据及权限后端。 |
| `crates/host` | HTTP/SSE/认证、项目路由、跨项目调度与端口装配、嵌入 Web；不直接写业务事实。 |
| `crates/credential-helper` | macOS 固定签名 Keychain 助手。 |
| `web/src` / `web/tests` | React/TypeScript 工作台及浏览器回归；界面不裁定业务终态。 |
| `resources/configuration` | manifest 管理的扁平源、默认选择、Role/Skill/Spec 正文、Harness/Sandbox 安装输入与权限预设。 |
| `permissions` / `containers` | 权限后端资产与容器配方；实际发行输入由所选 Harness 和 Sandbox 组合。 |
| `schemas` / `sql` | JSON 结构合同与当前 SQLite 初始化基线。 |
| `tests` | 跨进程入口、原生探针、协议夹具、嵌入式 Node 报告器及共享测试支持。 |
| `scripts` | dev、check、release、platform、py 各类入口。 |

依赖方向为 `host → adapters → core → domain`；core 只能依赖端口，不导入适配器具体类型。ID/时间经应用环境注入；出厂资源与安装、凭据、权限编译经外层端口提供。架构检查入口为 `scripts/check/check-architecture.sh`。

## 单 Host 并发与性能约束

以下是已确认的核心设计约束；落地状态和阶段门禁见 [底层并发重构方案](docs/plans/single-host-concurrency/PLAN.md)，不能把目标写成当前已支持的能力。

- 单个 Host 必须在工作流持续执行时支撑至少 100 个活跃并发 Web 前端。100 个连接建立成功不构成验收，必须覆盖持续查询、页面和项目切换、混合写入、事件传播及后台工作推进。
- 当前重构继续使用 SQLite 和现有全局/项目/全局讨论分库边界，不以增加 Host 或更换数据库规避性能门禁。一个数据集只由一个 Host 管理；数据集所有权文件锁保留。
- 项目选择属于每个前端标签页。项目请求、事件订阅和产物读取显式携带项目作用域，不依赖 Host 的共享 active project；同一项目的数据仍由其数据库统一保存。
- 读请求始终允许并发，不等待全局或项目级业务互斥锁；查询使用独立已提交快照或有版本的只读视图，普通读取不得隐式写业务数据、同步配置或恢复完整工程历史。
- 写请求并发进入，通过资源级乐观版本校验和数据库原子提交解决冲突，不使用应用层全局/项目写锁或单写队列串行化业务。SQLite 每库单物理写事务的短暂竞争由引擎和有界技术重试处理；不得据此把整条命令重新包进互斥锁。
- 所有修改入口都要有明确的版本前置条件和幂等语义，包括设置、凭据、安装、运行控制、回调与 Host 生命周期。新增对象验证不存在；多对象操作校验相关读写集合和领域约束。不能仅依赖客户端提交了 expected_revision 字段。
- 性能优先，跨前端显示、缓存、目录摘要和跨库投影允许最终一致；版本比较、单库业务提交、命令幂等与执行授权仍须原子且基于权威事实。保留未保存草稿及其原始版本，不自动覆盖或静默重试冲突编辑。
- 普通页面按需读取有界摘要和详情，提交后按项目/资源失效缓存；禁止用整库恢复、全状态序列化比较或每个 SSE 客户端独立扫描历史作为正常热路径。验收指标、负载与证据规则见 [100 前端验收合同](docs/plans/single-host-concurrency/ACCEPTANCE.md)。

## 配置、权限与执行约束

- 配置为不可变源修订，引用使用 `identity:revision`；根级引用由源类型白名单限定，摘要覆盖引用与 data。`source_definitions` 是唯一配置存储，不增加展开配置投影表。冻结任务按原引用重建，不读取当前默认替换历史依据。
- Role 管职责，Skill 管方法，Step 管任务，Spec 管内容检查，Delivery 管结构及来源，Standard 固定闭包。新增业务流程使用这些原语；业务 key、标题或角色名不选择执行器。
- Agent 只引用 Role 与 Capability；Capability 表示技术能力，Profile 是唯一权限规则源。Workflow 步骤可直接选择 Harness/Sandbox Profile；显式选择替换同层默认绑定，实际执行同时满足双层边界。内存 StepAccess 由 Profile 推导，不是另一份可编辑规则。无法落实的后端策略明确拒绝。
- Harness 保存唯一程序版本、安装与本机登录件；Sandbox 保存 Docker 基础环境。二者组合产生内部配方/OCI 身份，不存在独立执行安装源。Workflow 内嵌调度配置并随版本和 Standard 冻结。发行标签不是兼容门槛；核对实际 recipe/image/driver/权限资产与协议报告。Codex 与 DSH 当前提供动态工具及经验访问桥；根编排能力按实际驱动声明校验，配置不能绕过驱动能力。
- manual/rust/agent 根驱动使用相同派发、接纳和完成规则。经理工作台与根调度授权分别校验；Human 决定不能由 Agent 冒充。默认 automatic 不产生人类确认，仍要求真实测试和独立评审等业务条件。
- 所有外部副作用遵循“持久化意图 → 事务外执行 → 核对接纳回执”。未知结果先 reconcile，不盲重发；取消取得实际停止/写撤销证明。模型记录、平台回执、人类决定分别保存。
- Git 核对实际候选、父提交/diff、本地 CAS 和远端精确 lease；平台只创建 PR，不执行远端合入。运行计划修改必须 preview/quiesce/apply，旧执行与原始拒绝保留。
- 当前数据标记 `devdesk-profile-runtime-v2`，SQLite global 24 / project 60。只打开兼容数据集或初始化空目录，不自动迁移、删库或转换旧数据。真实验证使用专用空目录，不清理用户运行数据来掩盖失败。

## 构建与检查

Rust 1.99.1 由 `rust-toolchain.toml` 固定；前端使用 Node 24/npm，脚本还需要 Python 3.10+ 和 C 编译工具链。macOS 先按 `docs/system-credentials.md` 配置签名与固定助手；真实工程另需 Docker、Git、GitHub CLI 及相应认证。

从仓库根目录执行：

```bash
./scripts/dev/build.sh
./scripts/dev/dev.sh --data ./data --port 4317
npm --prefix web run build
cargo test --workspace --locked
cargo fmt --all -- --check
```

`build.sh` 安装前端依赖并构建 release；`dev.sh` 构建 debug 并启动 Host。macOS 通过 `scripts/dev/run-host.py` 使用私有签名副本，避免后续构建覆盖正在执行的程序。Web 嵌入 Host，界面更改后需构建并重启。

完整回归（先激活 Python 虚拟环境，Schema 校验依赖不能跳过）：

```bash
python3 -m pip install -r scripts/check/requirements.txt
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

该入口检查权限资产、出厂目录、架构、风格门禁（`scripts/check/check-style.sh`：rustfmt、ESLint、TypeScript、Prettier、Web 构建、clippy `-D warnings`）、Web、Rust、HTTP/数据集与 debug/release Chromium。按实际改动选择必要检查：纯文档改动核对代码事实、链接和示例即可；协议、状态、持久化和权限改动要覆盖对应边界。Rust 测试位于模块及 `crates/adapters/tests/`，浏览器测试在 `web/tests/`，跨进程检查在 `tests/entrypoints/`。默认回归不调用付费模型或真实 GitHub；MCP 浏览器测试需要 Docker Engine，首次构建执行镜像需要镜像与包源网络。

真实 Git/Docker 可用 `scripts/check/check-live.sh --groups git,docker,permissions`。模型/GitHub 工程及 Standard 探针须使用任务已授权的专用认证、数据集和仓库；接口替身、禁外网协议夹具和真实服务验收分别报告。保存实际 Host/脚本内容身份、原始日志、输入产物及失败项，不能靠模型摘要宣布通过。

## 风格、文档与交付

代码风格以工具链门禁为准：Rust 遵循 rustfmt 默认与 clippy `clippy::all`，Web 遵循 ESLint（typescript-eslint recommended + react-hooks）与 Prettier 默认值；成文规范、豁免协议与警告策略见 [docs/code-standards-rust.md](docs/code-standards-rust.md) 与 [docs/code-standards-web.md](docs/code-standards-web.md)，提交前跑 `./scripts/check/check-style.sh`。参数和状态规则优先使用明确领域类型，保持命令幂等、项目隔离、源修订不可变、历史恢复与迟到回执围栏。

系统行为变更同步更新对应 `docs/*.md`、必要 Schema 和 README 入口。字段和数值从代码/配置验证；展示标签、源修订、SQL 版本、驱动能力和实机验证不能混用。现行文档中给出实现依据，历史验收只描述当时执行。

日志、JSON/JSONL、截图、临时脚本和测试输出放临时证据目录；`docs/plans/` 只保存历史 Markdown。不要提交密钥、OAuth 文件、运行数据、构建产物或本地 `review/` 材料。提交聚焦单一主题；PR 说明具体行为和验证结果，涉及界面时提供对应证据。

完整说明从 [README.md](README.md) 进入；核心合同见 [docs/contracts.md](docs/contracts.md)、[docs/workflow-orchestration.md](docs/workflow-orchestration.md)、[docs/source-configuration.md](docs/source-configuration.md)、[docs/protocol.md](docs/protocol.md)。
