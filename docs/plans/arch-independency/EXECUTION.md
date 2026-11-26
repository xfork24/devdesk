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

### 初步验收

| 工作项 | 命令 | 结果 |
|---|---|---|
| G-3 | `cargo test -p devdesk-core --locked --lib configuration_catalog` | 2 通过 |
| | `cargo test -p devdesk-host --locked --bin devdesk-host configuration_read` | 9 通过 |
| | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test experience_directory --test experience_imports --test conversations --test governance --test agent_capabilities` | 36 通过 |
| | 反向验证：先删代码、后清例外 | 架构检查报出 3 条过期的例外，退出码 1；清空后通过 |
| N-8 | `cargo test -p devdesk-adapters --locked --features test-fixtures,fixture-catalog --test configured_workflows -- title_only_project_runs_an_agent_step_without_a_repository_or_workspace` | 1 通过 |
| | 同一套件中现有的 4 个用例：`automatic_review_blocks_rejection_and_advances_after_rework_without_confirmation`、`dynamic_spec_rejection_cannot_be_confirmed_and_rework_keeps_the_original_report`、`explicit_manual_approval_requires_reason_and_exact_execution_and_preserves_rejection`、`cancelling_a_failed_workflow_remains_schedulable_and_allows_a_fresh_run` | 4 通过。它们同样用只有标题的项目启动带 Agent 步骤的流程 |

### N-8 的依据

会话在没有工作区租约、也没有代码视图的情况下打开，步骤正常完成。`harness_workspace::prepare` 在这种请求下直接返回私有工作目录，不要求仓库。因此 V1 的风险不成立。

### 执行中的发现

- **仅标题的项目仍然带有工作区定义。** `project.create` 总是写入 `workspace_definition`，其中 `repository` 为空字符串、`mainline` 为 `main`。这说明内核的项目模型带着工程形状，由 N-4 处理。
- **单独运行适配器的集成测试需要 `--features test-fixtures,fixture-catalog`。** 只开 `test-fixtures` 时出厂目录不含角色与 Agent，依赖默认绑定的用例会失败。`cargo test --workspace` 通过 Host 的开发依赖带上了这两个 feature，所以完整检查不受影响。这正是 T-6 要消除的耦合。
