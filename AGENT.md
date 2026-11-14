# Repository Guidelines

## 事实源与工作范围

先检查 `git status`，保留用户已有改动。当前系统行为以 `crates/`、`web/src/`、`schemas/`、`sql/` 与已发布出厂配置为准；`docs/` 根目录 Markdown 是现行说明，`docs/plans/` 是历史计划/验收，不能用旧计划覆盖代码事实。更新文档直接描述当前状态，不追加修订历史；历史计划保持原文，只有用户明确要求的机械修正例外。

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
| `resources/configuration` | manifest 管理的扁平源、默认选择、Role/Skill/Spec 正文、运行资源与权限预设。 |
| `permissions` / `containers` | 权限后端资产与容器配方；实际发行输入来自所选运行资源源。 |
| `schemas` / `sql` | JSON 结构合同与当前 SQLite 初始化基线。 |
| `tests` | 跨进程入口、原生探针、协议夹具、嵌入式 Node 报告器及共享测试支持。 |
| `scripts` | dev、check、release、platform、py 各类入口。 |

依赖方向为 `host → adapters → core → domain`；core 只能依赖端口，不导入适配器具体类型。ID/时间经应用环境注入；出厂资源与安装、凭据、权限编译经外层端口提供。架构检查入口为 `scripts/check/check-architecture.sh`。

## 配置、权限与执行约束

- 配置为不可变源修订，引用使用 `identity:revision`；根级引用由源类型白名单限定，摘要覆盖引用与 data。`source_definitions` 是唯一配置存储，不增加展开配置投影表。冻结任务按原引用重建，不读取当前默认替换历史依据。
- Role 管职责，Skill 管方法，Step 管任务，Spec 管内容检查，Delivery 管结构及来源，Standard 固定闭包。新增业务流程使用这些原语；业务 key、标题或角色名不选择执行器。
- Agent 只引用 Role 与 Capability；Capability 表示技术能力，Permission Context/Profile 表示执行授权。存储态 Workflow 不包含写路径/工具授权，内存 StepAccess 可承载解析后的 grant。无法落实的后端策略明确拒绝。
- `runtime_resource` 保存不可变配方/OCI digest 和下载摘要。发行标签不是兼容门槛；核对实际 recipe/image/driver/权限资产与协议报告。Codex 当前提供动态平台/编排桥，DSH 不提供，配置声明不能绕过驱动能力。
- manual/rust/agent 根驱动使用相同派发、接纳和完成规则。经理工作台与根调度授权分别校验；Human 决定不能由 Agent 冒充。默认 automatic 不产生人类确认，仍要求真实测试和独立评审等业务条件。
- 所有外部副作用遵循“持久化意图 → 事务外执行 → 核对接纳回执”。未知结果先 reconcile，不盲重发；取消取得实际停止/写撤销证明。模型记录、平台回执、人类决定分别保存。
- Git 核对实际候选、父提交/diff、本地 CAS 和远端精确 lease；平台只创建 PR，不执行远端合入。运行计划修改必须 preview/quiesce/apply，旧执行与原始拒绝保留。
- 当前数据标记 `devdesk-source-flat-v1`，SQLite global 18 / project 48。只打开兼容数据集或初始化空目录，不自动迁移、删库或转换旧数据。真实验证使用专用空目录，不清理用户运行数据来掩盖失败。

## 构建与检查

Rust 1.99.1 由 `rust-toolchain.toml` 固定；前端使用 Node 24/npm，脚本还需要 Python 3 和 C 编译工具链。macOS 先按 `docs/system-credentials.md` 配置签名与固定助手；真实工程另需 Docker、Git、GitHub CLI 及相应认证。

从仓库根目录执行：

```bash
./scripts/dev/build.sh
./scripts/dev/dev.sh --data ./data --port 4317
npm --prefix web run build
cargo test --workspace --locked
cargo fmt --all -- --check
```

`build.sh` 安装前端依赖并构建 release；`dev.sh` 构建 debug 并启动 Host。macOS 通过 `scripts/dev/run-host.py` 使用私有签名副本，避免后续构建覆盖正在执行的程序。Web 嵌入 Host，界面更改后需构建并重启。

完整回归：

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

该入口检查权限资产、出厂目录、架构、Web、Rust、HTTP/数据集与 debug/release Chromium。按实际改动选择必要检查：纯文档改动核对代码事实、链接和示例即可；协议、状态、持久化和权限改动要覆盖对应边界。Rust 测试位于模块及 `crates/adapters/tests/`，浏览器测试在 `web/tests/`，跨进程检查在 `tests/entrypoints/`。默认回归不调用付费模型或真实 GitHub。

真实 Git/Docker 可用 `scripts/check/check-live.sh --groups git,docker,permissions`。模型/GitHub 工程及 Standard 探针须使用任务已授权的专用认证、数据集和仓库；接口替身、禁外网协议夹具和真实服务验收分别报告。保存实际 Host/脚本内容身份、原始日志、输入产物及失败项，不能靠模型摘要宣布通过。

## 风格、文档与交付

Rust 使用 rustfmt 与 snake_case/PascalCase；前端保持现有 TypeScript/React 风格，常规 2 空格缩进。参数和状态规则优先使用明确领域类型，保持命令幂等、项目隔离、源修订不可变、历史恢复与迟到回执围栏。

系统行为变更同步更新对应 `docs/*.md`、必要 Schema 和 README 入口。字段和数值从代码/配置验证；展示标签、源修订、SQL 版本、驱动能力和实机验证不能混用。现行文档中给出实现依据，历史验收只描述当时执行。

日志、JSON/JSONL、截图、临时脚本和测试输出放临时证据目录；`docs/plans/` 只保存历史 Markdown。不要提交密钥、OAuth 文件、运行数据、构建产物或本地 `review/` 材料。提交聚焦单一主题；PR 说明具体行为和验证结果，涉及界面时提供对应证据。

完整说明从 [README.md](README.md) 进入；核心合同见 [docs/contracts.md](docs/contracts.md)、[docs/workflow-orchestration.md](docs/workflow-orchestration.md)、[docs/source-configuration.md](docs/source-configuration.md)、[docs/protocol.md](docs/protocol.md)。
