# 源配置整体重构验收记录

日期：2026-11-11。实现已合入本地 main，后续修复与验收在 `/Users/hengliu/workspace/devdesk` 完成。主要实现提交为 `f3e5c91`，整合原 main 为 `7d6966d`；源身份、字面量保护、解析缓存、文档规范与权限 Scope 修复分别见 `89dabd4`、`68dd6c0`、`131fceb`、`e053c5d`。

原独立 worktree 为 `/Users/hengliu/.codex/worktrees/source-configuration/devdesk`，分支 `codex/source-configuration`，起点 `082c081`。用户确认合并 main 后，实施期间 main 新增的 Harness 验收与原始草稿归档完整保留；独立 worktree 已按用户后续要求清理。

## 最终配置职责

- 配置统一使用 `{kind, identity, revision, name, scope, data}` 源封套，以 `identity:revision` 引用不可变修订。
- Agent 仅保存 Role、Capability 引用。Capability 选择 Model、Harness、Skills、Tools，不授予权限。
- Permission Profile 保存底层策略；Permission Context 保存 Sandbox、权限绑定选择、步骤范围与控制策略。项目选择确切授权上下文，权限修订保持原 Scope。
- Workflow 仅保存业务图、任务、工作上下文、端口、预算及业务门禁。授权不放在 Agent、Capability 或 Workflow 中。
- PRD/Design 的规范正文只由 Spec 拥有，Delivery 引用该 Spec。生产与消费解析同一个固定修订，不随新发布的 Spec 漂移；内容门禁仍来自 Step 的明确检查集合。
- 删除静态 projections、初始 Agent 实例和重复权限预设。SQLite 新基线为 global 15 / project 46，标记为 `devdesk-source-v2`。不保留旧协议执行路径、升级或转换入口。
- 数据库保存源修订、实际选择、输入、意图、回执和来源；Plan、任务、运行环境及权限视图按源选择重建。Plan 的有界缓存仅在进程内存在，重启重新生成。

## 不改变业务流程的证据

以 main `082c081` 为基线，规范化源引用并移出授权字段后，默认 Feature 22 步及 Task 20 步的业务图、任务、依赖、端口、预算、TDD、确认与完成条件保持一致。

| 合同 | SHA-256 |
|---|---|
| Feature 规范化业务定义 | `ed453ae0e68249f612350dec3e5159c0c889ed7ff27c1b002c6be5d4473b0508` |
| Task 规范化业务定义 | `3124b77dffe25d4484585666625623a33d9a5bef7cf89be25105ac5eaa8cc581` |
| 原 42 步中 11 条显式授权，移入 Permission 后的确切路径与工具范围 | `fb1cbd3c102766d4a847f780a8e0521895d1f42ac4cc1ea4ed700a381275138e` |

这些合同已固化到源配置回归。另核对了 90 个源修订及 218 条直接引用：身份唯一、目标存在，步骤授权指向确切 Workflow 修订与步骤。

## main 验证结果

| 检查 | 结果 |
|---|---|
| `cargo test --workspace --locked`，最终实现 | 207 通过、0 失败、15 忽略 |
| `DEVDESK_E2E_PORT=44320 ./scripts/check/check.sh` | 完整通过 |
| Web TypeScript/Vite 构建、rustfmt、diff 空白检查 | 通过 |
| debug / release Chromium | 各 43 通过 |
| HTTP、Host/Origin、认证、Artifact、SSE、关闭及重启 | 通过 |
| 项目分库隔离、幂等、模板重置及确切源引用恢复 | 通过 |
| 不兼容标记和旧库版本 | 明确拒绝；原数据库文件 SHA-256 保持不变 |
| 环境延迟准备、并发复用、失败重试、克隆核对 | 通过；使用显式 Docker/GitHub CLI 夹具和真实临时 Git 仓库 |
| Schema 离线编译、当前资源与 Serde 合同 | 通过；Python 可选 jsonschema 未安装，Rust 离线验证已覆盖 |
| 新库源存储与任务重建 | 直接检查 SQLite 无展开 Plan/任务正文，无投影表；关闭重开后恢复出与 Harness 端口收到的任务完全一致的内容 |
| 源一致性 | 权限与 Agent 分离、悬空引用拒绝、Profile Scope 保持、项目配置/接入映射身份隔离、字面量 JSON 无损恢复均通过 |

浏览器覆盖编辑、保存、发布、重载、项目授权、权限修订与绑定、模板重置、运行计划修改、输入输出、子流程与任务预览。验收发现并修复了归档新修订遮蔽出厂修订，以及重复解析 Plan 导致项目配置读取延迟的问题。

15 项忽略用例要求真实 Docker 镜像、GitHub 资源等外部条件。本轮没有重新执行真实模型和远端 GitHub 工程验收，不将端口夹具结果当作真实模型证据。所有检查均使用独立临时数据目录，没有升级或改写用户既有数据。

## 独立发布包

`./scripts/release/package.sh` 通过，随后 `tests/live/probe_permission_package.py` 在源码目录外解包验证通过。核对了 7 个执行资产、3 份权限预设、源配置发布与预览、确切绑定、目录热重载，以及通过实际平台处理器完成的通用工作流；没有模型调用或自动下载。

- 包：`dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`
- 包 SHA-256：`b470e025bdee46fb27c9c766c7363203aa0d24c326eda7f0a75e55e15cb033f4`
- Host SHA-256：`dcfac54f436edcae589910619aab5a559e267ff7be6ee204f6ac7f0e18c9f5fd`

日志、JSON 与截图是临时验收输出，不作为本计划的持久交付物；上面的摘要记录本次实际结果，不保证临时文件长期保留。

## 用户要求的本地清理

删除前确认所有 worktree 均无未提交或未跟踪文件，且 HEAD 是 main 的祖先：

| worktree | 清理前 HEAD | 结果 |
|---|---|---|
| harness-workflow-orchestration | `25e9c53` | 已合入 main，已删除 |
| source-configuration | `7d6966d` | 已合入 main，已删除 |
| workflow-step-task-instructions | `38334cf` | 已合入 main，已删除 |

唯一 stash `290bc5f`（Preserve main workspace before Harness workflow merge）包含 Harness 计划草稿与过期的“待实施”索引行。草稿与 main 的 `docs/plans/harness-workflow-orchestration/original-draft/PLAN.md` blob 均为 `5f95912969fdf45839d91caee8cebd42e297b44b`；现行索引已记录完成状态。因此已删除该 stash。

最终 Git worktree 列表仅保留 `/Users/hengliu/workspace/devdesk` 的 main，stash 列表为空。现有未跟踪文件 `review/crates_review.md` 未修改、未纳入本次提交。
