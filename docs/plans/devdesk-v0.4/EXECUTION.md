# v0.4 基线核对与验证记录

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

日期：2026-11-10。本文记录 [v0.4 架构与实现基线](PLAN.md) 的源码依据、继承的验证结果及待验边界。本次工作为文档整理，未新增产品功能、修改包版本、迁移用户数据或启动真实工程。

## 源码基准与版本

基准提交为 `e4fbcae855ce59ce44695724d969a195c4c2228a`（`Fix workflow references and configurable model approval gates`），父提交为 `41ff58c1f4ef0f4182651ad70834da14bad63ae9`（Ubuntu 验收修复）。v0.3 只作为原章节和工程原则的来源，实际能力以此基准源码及对应测试为准。

| 对象 | 核对结果 |
|---|---|
| 文档架构基线 | v0.4 |
| 产品版本 | Cargo、Web 和 Host 能力接口仍为 0.2.0 |
| 协议 | JSON-RPC v1 |
| 项目配置 | ProjectConfiguration Schema v2 |
| Workflow | 新默认 Schema v3、compiler_version=3；保留旧 v1/v2 |
| 默认 Workflow 发布修订 | `builtin-workflow-feature-sol-high-v4` / `builtin-workflow-task-sol-high-v4` |
| 任务渲染器 | 新配置 `agent-task/v3`，历史 v1/v2 兼容 |
| 正式内容报告 | `evidence-checks/v2` |
| 数据库 | 全局 v8、工程 v39，包含独立全局讨论迁移 |

## 源码与回归对应

| 能力 | 主要实现依据 | 回归或合同依据 |
|---|---|---|
| Role/Capability/Agent、可见范围、可选 Standard | [领域配置](../../../crates/domain/src/configuration.rs)、[配置处理](../../../crates/core/src/configuration.rs)、[项目配置](../../../crates/core/src/project_configuration.rs) | [配置资源回归](../../../crates/adapters/tests/configuration_resources.rs)、[原实施契约](../role-capability-project-configuration/CONTRACT.md) |
| 六种默认 Role、阶段 Skill/Spec、八个 Feature 阶段 | 默认目录（历史路径：`../../../crates/core/src/default_catalog.rs`）、[默认配置资源](../../../resources/configuration/README.md) | 配置资源回归、[配置浏览器回归](../../../web/tests/workflow-authoring.spec.ts) |
| 任务配置、版本、编译路径与冻结载荷 | [任务领域合同](../../../crates/domain/src/agent_task.rs)、[工作流编译](../../../crates/core/src/workflow_definition.rs)、[任务组装](../../../crates/core/src/agent_tasks.rs) | [任务回归](../../../crates/adapters/tests/agent_tasks.rs)、[任务编辑浏览器回归](../../../web/tests/workflow-tasks.spec.ts) |
| 全阶段业务提示词配置 | 任务播种及显式升级（历史路径：`../../../crates/core/src/default_tasks.rs`）、[冻结配置提示词及质量依据](../../../crates/core/src/workflow_prompts.rs) | 任务回归覆盖讨论、三类校验、测试编写、实现、文档及两类评审；[22 份任务索引](../../../resources/configuration/README.md) |
| 正式结构化输出、模型 Approve | [报告解析](../../../crates/domain/src/stage_answer.rs)、[输出 Schema](../../../schemas/stage-answer.schema.json)、[正式执行](../../../crates/core/src/live_runs.rs) | [Schema 回归](../../../crates/adapters/tests/schema_contracts.rs)、[执行回归](../../../crates/adapters/tests/workflows.rs) |
| 人类 Approve、覆盖、确认失效及原报告保留 | [模型报告与人工门禁](../../../crates/core/src/model_approvals.rs)、[工作流运行器](../../../crates/core/src/workflow_runtime.rs)、[运行界面](../../../web/src/components/WorkflowInstanceView.tsx) | 执行回归、[模型/人工决定浏览器回归](../../../web/tests/model-approvals.spec.ts) |
| 提案替换、Session 后继、子流程端口及返工引用 | 工作流运行器、[提案采纳](../../../crates/core/src/proposals.rs) | [引用回归](../../../crates/adapters/tests/workflow_references.rs)：四种提案、重启、拒绝/修订、后继与无关 Session、冻结已消费输入、旧确认失效、子流程别名及后继输出清理 |
| 旧 Run 取消等待及真实 writer 停止 | 工作流运行器、[执行状态](../../../crates/domain/src/execution.rs)、[Host 准备](../../../crates/host/src/workspace.rs) | 执行回归中的 `cancellation_wait_is_requeued_until_the_old_writer_has_really_stopped` |
| 候选单父来源及规划文档分链 | [Git 端口](../../../crates/core/src/ports.rs)、[Git 适配器](../../../crates/adapters/src/git.rs)、正式执行、[core 文档处理](../../../crates/core/src/lib.rs) | [Git 资源回归](../../../crates/adapters/tests/git_resources.rs)、任务回归、core 单元回归；保留 Ubuntu 修复 |
| HTTP/MCP/Skill、确切授权及迟到调用回执 | [接入运行器](../../../crates/core/src/integrations.rs)、[接入契约](../tool-skill-integrations/CONTRACT.md) | [工具回归](../../../crates/adapters/tests/tool_integrations.rs)、[接入浏览器回归](../../../web/tests/integrations.spec.ts) |
| DSH 四路由、目录/连接/推理分离 | [模型服务商领域](../../../crates/domain/src/model_provider.rs)、[Host 服务商接口](../../../crates/host/src/model_providers.rs) | [服务商回归](../../../crates/adapters/tests/model_providers.rs)、[DSH 设置回归](../../../web/tests/model-providers.spec.ts) |
| 权限绑定、合同重新验证与后继 | [权限切换](../../../crates/core/src/permission_switch.rs)、[权限资源](../../../crates/core/src/permission_resources.rs) | [权限回归](../../../crates/adapters/tests/permission_profiles.rs)、[权限确切绑定报告](../permission-runtime-bindings/EXECUTION.md) |
| 九项导航、连接内核、阶段布局和反馈 | [工作台](../../../web/src/main.tsx)、[内核连接](../../../web/src/components/KernelConnection.tsx)、[配置页面](../../../web/src/components/ConfigurationPages.tsx) | [布局回归](../../../web/tests/execution-layout.spec.ts)、[连接回归](../../../web/tests/kernel-connection.spec.ts)、[设置回归](../../../web/tests/settings-usability.spec.ts) |
| 全局讨论及工程库 v39 | [项目库迁移和讨论目录](../../../crates/adapters/src/lib.rs)、039 迁移（历史路径：`../../../sql/project/conversations/039_global_discussions.sql`）、[全局库迁移](../../../crates/adapters/src/catalog.rs) | Host 全局讨论回归、HTTP 分库回归；迁移编号依据执行入口核对 |

## 继承的 macOS 自动化结果

上一轮在提交 `e4fbcae` 前、与该提交相同的产品源码上执行 `./scripts/check/check.sh`，退出码为 0。整理本文时重新读取该轮完整日志核对计数，没有把文档检查记为一次新的产品回归。

| 检查 | 已有结果与边界 |
|---|---|
| Rust workspace | 179 通过，0 失败；16 项需外部环境的用例按默认配置忽略 |
| Web | TypeScript 与 Vite 构建通过 |
| debug/release Host | 构建及 macOS 签名通过 |
| debug HTTP | 鉴权、Host/Origin、资源、路径、写锁、幂等、SSE、重启及项目隔离通过 |
| 内核连接 | discovery、连接密钥、Cookie 端口隔离、远程 opt-in 等契约通过 |
| 运行准备 | 中断 clone、按需准备、并发复用、失败重试和嵌入配方通过；使用明确测试替身 |
| Chromium | debug 33 项、release 33 项通过，包含模型通过/人类 Approve 和模型拒绝/人类覆盖两条交互路径 |
| standalone HTTP | release 二进制复制到源码外目录运行，HTTP 及嵌入资源契约通过 |
| 权限资产 | 9 个预设、4 个后端、7 个不可变文件通过 |
| Schema | Rust Schema 回归通过；权限资产脚本的可选 Python jsonschema 校验因未安装而跳过，未将该分支记为通过 |

原日志位于本机临时路径 `/tmp/devdesk-rebase-check.log`，可能被清理；本文保存结论与边界，不把临时路径作为长期可重放证据。执行入口和完整前提见 [测试索引](../../../tests/README.md)。全局 `cargo fmt --all -- --check` 的历史差异在 Ubuntu 报告中已记录，本次没有声称其通过。

## 继承的 Ubuntu 验收

[Ubuntu 六项改造报告](../ubuntu-six-plan-acceptance/EXECUTION.md) 对其当时源码记录了 Rust 165 项、debug/release 浏览器各 31 项、16 项默认忽略用例另行通过、Linux 凭据/权限、真实 Codex 完整闭环及独立 Linux 包通过。报告同时保留 MiniMax 实际调用和完整闭环未通过的结论。

相关修复已通过 `41ff58c` 进入本基线：类型化任务输出、候选实际 parents、Structured Task Plan 与实现说明修订链分离、ToolCall 迟到回执核对，以及浏览器初始化和 Linux 探针修正。`e4fbcae` 在此基础上加入新报告/人工决定及引用恢复，历史冻结任务继续兼容读取。

该 Ubuntu 报告早于双重 Approve 改造，不能证明新增门禁已在 Ubuntu 或真实模型完整工程中重跑。本次文档工作没有重新调用模型、Docker、GitHub、平台凭据或用户原有数据集。

## 本次文档交付

- 新建 `devdesk-v0.4/PLAN.md`，保留 v0.3 的 23 章结构，逐项更新到当前源码。
- 新建本文，将实现、自动化回归、历史原生验收及待验范围分别列明。
- 更新根 README 与计划索引，当前架构入口指向 v0.4；v0.3 保留为历史基线。
- 核对新增文档的本地链接、章节编号、版本常量、任务资源数量和改动范围；检查 Markdown diff 空白。

## 待验范围

- 新 Approve 合同与人类覆盖在 Ubuntu 上的真实端到端工程闭环。
- 当前报告合同在真实 Codex/DSH 各模型上的稳定性；MiniMax 既有混合文本/JSON 与预算限制保留。
- 真实第三方 OAuth 账户，以及平台报告未覆盖的凭据/进程崩溃边界。
- 大历史、任务载荷及跨项目经验目录的容量/延迟测试。

这些是后续验证事项，未作为本次文档整理的已完成结果。
