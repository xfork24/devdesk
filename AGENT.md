# Repository Guidelines

## 开始工作

- 先运行 `git status --short`，保留已有改动。优先用 `rg` 核对入口、引用和装载路径后再删除代码。
- 当前行为以源码、Schema、SQLite 基线和发布 manifest 为准。设计与代码手册放在 `docs/design/` 和 `docs/crates/`。计划及历史验收放在 `docs/plans/`。
- 使用专用测试数据目录。凭据、实际运行数据、日志和本地 `review/` 留在版本库外。

## 核心与领域边界

| 位置 | 职责 |
|---|---|
| `crates/domain` | 领域类型、源图、Spec、DAG 和有界校验。保持 I/O、时钟与环境无关 |
| `crates/core` | 配置编译、命令、查询、会话和调度。通过端口访问外部能力，裁定并提交权威终态 |
| `crates/adapters` | SQLite、产物、Git、Docker、Harness、凭据及外部执行能力 |
| `crates/host` | HTTP、SSE、认证、项目作用域、领域代次和端口装配 |
| `web/src/core-main.tsx` | Web Core 入口。领域组件通过客户端贡献和公开 SDK 加载 |
| `configurations/core` | 随 Host 构建的核心配置 |
| `configurations/software-development` | 独立的软件目录、运行时处理器及 Web 入口 |

依赖方向为 `host → adapters → core → domain`。Host 可独立构建并从空领域启动。业务配置在 ZIP 内发布，领域页面经可信摘要校验后由 Web Core 同文档渲染。程序通过领域的实际声明选择处理器。Web 和非浏览器客户端使用相同的项目、对象、运行和贡献标识。

## 配置与执行

- 用不可变 `identity:revision` 引用配置。摘要包括根引用和数据，根引用遵循类型白名单。`source_definitions` 是配置存储，执行恢复使用冻结的源闭包与包摘要。
- Role 决定职责，Skill 记录方法，Step 指定任务，Spec 校验内容，Delivery 约定产物结构及来源，Standard 固定闭包。
- Agent 引用 Role 与 Capability；Profile 定义权限。执行同时满足 Harness 和 Sandbox 两层边界。配方组合器生成 Dockerfile，实际安装依镜像、驱动与权限资产核验。
- 人工、Rust 和 Agent 调度共享派发、接纳与完成约束。经理工作台和根调度分别授权，人工决定必须来自实际的人。在核心入口以冻结输入校验操作及其证明。
- 外部操作先持久化意图，再在事务外执行并核对回执。未知结果先核对，取消需要停止或撤销写权限的证据。
- 软件开发 ZIP 用 `operation_evidence` 定义交付门禁，领域运行端检查冻结证据。核心 Git 和托管端口执行确切提交、远端版本与同步核对。
- 计划变更走 preview、quiesce、apply，保留旧执行、拒绝和迟到回执。改变业务路径时核对冻结历史的读取与恢复。

## 数据与并发

- 一个数据集由一个 Host 独占，全局、项目与全局讨论分库。生产布局为 `single-host-domain-packages-v1`，数据标记为 `devdesk-profile-runtime-v2`，SQLite 基线为 global 24、project 62。
- 同一工作台有一个活动领域，可为空。客户端按代次访问，过期代次由 Host 拒绝。项目与运行保留所用包摘要。每个标签页独立选择项目。
- 普通读使用已提交快照或有界版本视图。业务写带上原版本、读写集合与幂等身份，版本检查和事实提交保持原子。
- SQLite 短事务的物理竞争允许有界重试。副作用重发需要单独的核对。显示和缓存允许最终一致。编辑冲突保留草稿与原版本。
- 摘要、详情与事件按需有界读取，共享 SSE 失效尾流。并发容量以独立验收判据为准。见[并发合同](docs/plans/single-host-concurrency/ACCEPTANCE.md)。

## 构建和验证

```bash
./scripts/dev/dev.sh --data ./data --port 4317
./scripts/check/check-style.sh
cargo test --workspace --locked
python3 scripts/py/check-docs.py
```

运行完整 `./scripts/check/check.sh` 前安装 `scripts/check/requirements.txt`、Web 依赖和 Chromium。正式制品验证使用 `scripts/release/package-core.sh` 的 `host`、`web`、`software` 入口。单体时代的测试数据和浏览器入口仅供回归，显式使用 `fixture-catalog` 和 `npm --prefix web run build:fixture`。跨端验收以正式制品为输入。见[测试索引](tests/README.md)。

Rust 使用 rustfmt、clippy `-D warnings`，Web 使用 ESLint、TypeScript 和 Prettier。修改行为时同步更新合同和当前文档。调整 crate 文件后运行 `python3 scripts/py/check-docs.py --write` 更新索引。

默认回归使用隔离的数据集与明确的测试端口。真实模型和 GitHub 只在已授权的专用账号、仓库与数据集中验收。报告请写明命令、制品版本、实际结果及未验证范围。
