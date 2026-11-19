# Repository Guidelines

## 开始工作

- 先运行 `git status --short`，保留用户已有改动。优先使用 `rg` 查找引用，确认调用与加载路径后再删除。
- 当前行为以 `crates/`、`web/src/`、`schemas/`、`sql/` 和 manifest 发布的出厂资源为准。
- `docs/design/` 及 `docs/crates/` 是当前说明；`docs/plans/` 是方案与历史验收，不能以未实施设计覆盖代码事实。历史记录保持原文，明确要求的机械修正除外。
- 生产只使用真实外部执行端口。接口替身、协议样本与测试夹具不能成为产品回退路径。

## 架构边界

| 目录 | 职责与限制 |
|---|---|
| `crates/domain` | 领域类型、源图、Spec/DAG 与有界规则；不执行 I/O，不获取时间、随机数或环境。 |
| `crates/core` | 配置编译、冻结选择、命令/查询、会话、工作流、驱动及权威终态；外部依赖通过 `ports.rs` 注入。 |
| `crates/adapters` | SQLite/Artifact codec、出厂资源、Git/GitHub、Docker、Harness、安装、凭据及权限后端。 |
| `crates/host` | HTTP/SSE、认证、显式项目路由、跨项目调度与端口装配；不直接写业务事实。 |
| `crates/tools` | 独立工具程序，按 `src/bin/` 组织；当前包含 macOS 固定签名 Keychain 助手。 |
| `web/src` / `web/tests` | React/TypeScript 工作台与浏览器回归；界面不裁定业务终态。 |
| `configurations` | manifest 管理的源修订、默认选择、正文、Harness/Sandbox 安装输入及权限预设。 |
| `permissions` / `containers` | 权限后端资产与项目测试 Dockerfile；执行配方由 Harness/Sandbox 组合生成。 |
| `schemas` / `sql` / `tests` | JSON 合同、当前 SQLite 基线、跨进程/原生探针、协议夹具与共享支持。 |

依赖方向：`host → adapters → core → domain`。core 不导入具体适配器，ID/时间通过应用环境注入。架构检查：`./scripts/check/check-architecture.sh`。

## 配置、权限与执行

- 配置按不可变 `identity:revision` 引用，摘要覆盖根级引用与 data；根引用由类型白名单限定。`source_definitions` 是唯一配置存储，不增加展开配置投影表。恢复使用原源闭包，不追随当前默认。
- Role 管职责，Skill 管方法，Step 管任务，Spec 管内容检查，Delivery 管结构及来源，Standard 固定闭包。业务 key、标题和角色名不选择隐藏执行器。
- Agent 只引用 Role/Capability，Profile 是唯一权限规则源。步骤显式选择替换同层默认绑定，实际执行同时满足 Harness/Sandbox 两层边界。无法落实的策略拒绝执行。
- Harness 保存程序版本、安装与本机登录输入；Sandbox 保存 Docker 基础环境。组合器是执行 Dockerfile 的唯一生成路径。以 recipe/image/driver/权限资产及协议报告核验实际安装，发行标签不代表兼容。
- Codex 与 DSH 提供动态工具、外部工具、平台工具和经验访问桥；当前根编排能力仅由 Codex 驱动声明。配置不能绕过实际驱动能力。
- manual/rust/agent 使用相同派发、接纳和完成规则。经理工作台与根调度分别授权，Agent 不得冒充 Human。automatic 不生成人工签署，仍须满足真实测试、独立评审等条件。
- 外部副作用使用“持久化意图 → 事务外执行 → 核对并接纳回执”。未知结果先 reconcile；取消需要实际停止或写权限撤销证明。模型记录、平台回执和人类决定分别保存。
- Git 核对候选、父提交/diff、本地 CAS 及远端精确 lease。当前精简版合入核对确切源/目标/试合并版本：无保护目标用 beforeOid、force=false 原子快进，已有保护目标遵守 PR 门禁；Feature/main 另须 Human 决定，随后核对父提交/tree 与本地同步。历史详细版只创建 PR。
- 运行计划修改使用 preview/quiesce/apply；保留旧执行、原拒绝及迟到回执。历史恢复与旧数据迁移是不同问题，不能删除仍保护冻结执行的逻辑来清理兼容代码。

## 并发与存储

- 一个数据集只由一个 Host 管理，保留所有权文件锁及全局/项目/全局讨论分库。
- 当前数据标记 `devdesk-profile-runtime-v2`、布局 `single-host-resource-v2`，SQLite **global 24 / project 60**。只打开兼容数据集或初始化空目录，不自动迁移、删库或转换旧数据。验证使用专用空目录。
- 项目选择属于每个浏览器标签页。请求、SSE 和产物读取显式携带项目作用域，不能引入共享 active project。
- 普通读取使用独立已提交快照或有版本只读视图，不等待全局/项目业务锁，不隐式同步配置、写业务事实或恢复完整历史。
- 修改入口明确声明原版本、相关读写集合、领域约束与幂等身份；新增对象校验不存在。版本检查和事实提交必须原子，不能只接收 expected_revision 而不核对。
- 业务写入不使用全局/项目写锁或单写队列。SQLite 的短事务物理竞争可有界技术重试，不能重新准备业务命令或重发副作用。
- 显示、缓存、目录摘要和跨库投影允许最终一致；原版本比较、执行授权和单库业务提交保持权威。冲突保留草稿及原版本，不静默刷新后重发。
- 摘要、详情和事件按需有界读取，共享 SSE 失效尾流；避免整库恢复、全状态序列化比较或每个客户端独立扫描历史。
- 单 Host 在工作流运行时支撑至少 100 个活跃前端是容量要求，连接建立或局部性能回归不等于正式验收。门禁见 [并发验收合同](docs/plans/single-host-concurrency/ACCEPTANCE.md)。

## 构建与验证

Rust **1.99.1** 由工具链文件固定，前端使用 Node **24**。脚本需要 Python **3.10+** 与 C 工具链。macOS 签名/助手见 [系统凭据](docs/design/system-credentials.md)。

```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install -r scripts/check/requirements.txt
./scripts/dev/build.sh
./scripts/dev/dev.sh --data ./data --port 4317
```

`build.sh` 安装前端依赖并构建 release；`dev.sh` 构建 debug 并启动。Web 与出厂资源嵌入 Host，修改后重新构建并重启。macOS 启动器使用独立签名副本。

```bash
npm --prefix web ci
(cd web && npx playwright install chromium)
./scripts/check/check.sh
```

完整入口覆盖权限、出厂目录、架构、文档、RPC、风格、类型、Web 构建、Rust 及 debug/release HTTP/数据集/Chromium。Docker 和包源网络是部分准备与浏览器测试的前置条件；Schema 依赖不能跳过。

按实际改动运行必要检查：

```bash
./scripts/check/check-style.sh
cargo test --workspace --locked
python3 scripts/py/check-docs.py
```

风格以 rustfmt、clippy `-D warnings`、ESLint、TypeScript 和 Prettier 为准。Web 启用未使用局部变量/参数检查，清理无引用导出时仍需检查测试和运行时入口。

默认回归不调用付费模型或真实 GitHub。Docker 协议验收用 `tests/entrypoints/native_acceptance.py --docker-only --prepare`，通过当前 Rust 配方组合器准备镜像；显式镜像变量不回退旧版固定标签。真实模型/GitHub 工程使用明确指定的专用认证、数据集及仓库，见 [测试索引](tests/README.md)。记录真实输入身份、原始日志与失败，接口夹具、协议探针、真实服务验收分别报告。

## 文档与交付

- 行为变更同步更新当前文档、必要 Schema 与 README。参数、版本、默认值、能力和验证范围从代码/资源核对，不把目标写成现状。
- crate 文件索引用 `python3 scripts/py/check-docs.py --write` 更新；检查入口核对索引和当前文档本地链接。源码符号链接需核对位置，避免保留失效的行号。
- 日志、JSON/JSONL、截图、临时脚本及测试输出写临时证据目录；不提交凭据、运行数据、构建产物或本地 `review/`。
- 提交聚焦可评审主题，说明行为变化、验证及限制；涉及界面行为时保留相应证据。

入口：[README](README.md) · [代码手册](docs/crates/overview.md) · [结构合同](docs/design/contracts.md) · [工作流](docs/design/workflow-orchestration.md) · [源配置](docs/design/source-configuration.md) · [协议](docs/design/protocol.md)。
