# 单 Host 并发重构合入记录

2026-11-18：`codex/single-host-concurrency` 已 rebase 到本地 main，解决冲突并通过基本验证后，以 fast-forward 合入 `/Users/hengliu/workspace/devdesk`。分支与独立 worktree 已删除；未推送远端。全流程联合验收与正式容量验收留待后续执行。

## 提交与完整性

| 项目 | 身份或结果 |
|---|---|
| 原 main | `acb95175ccc585a316fb8c1dabe8a241e5a7857f` |
| 原分支末端 | `965a0cf23c51527173b2d7aa891620db3f8d4a8e` |
| Rebase 后代码末端 | `d6c903ce2ec4d59f23867f16ea67487c44192e00` |
| 代码树 | `935e93e67dbca062ecdf74e94636b192e0bde494` |
| 合入方式 | 35 个线性提交，`merge --ff-only`，没有新增 merge commit。 |
| 完整性 | 快进后 main 与分支末端、代码树相同；无未合入提交，1,035 个受跟踪文件摘要与已验证 worktree 一致。本文及 README 状态更新另作纯文档提交。 |
| 清理 | `codex/single-host-concurrency` 分支、worktree 及其空容器目录均已删除，只保留 main worktree。 |

原引用的完整 Git bundle、rebase range-diff、源码摘要、Host 二进制身份、原始失败及修复后日志保存在临时证据目录：

`/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-single-host-concurrency-20261116-wvt3rz2d/rebase-main-20261117/`

## 冲突整合结果

- 保留 main 的事件驱动经理、统一人类审批交互、异步会话启动与 HTTP 信号退出行为，接入本分支的资源级乐观提交、原始授权和迟到回执围栏。流程变化与经理事件在同一资源事务提交，后续唤醒使用有界事件页。
- 经理审批、确认与取消绑定原始证据、步骤版本、实际 Actor 和执行资格；编辑中的人类草稿保留原始审批上下文。执行页保留 main 的协作布局，同时使用有界摘要、精确详情和按需计划编辑。
- main 已发布的 224 个配置源及正文逐字节保留；组合行为发布新的不可变后继版本，未改写历史源。出厂 bundle 为 `2026.11.17-single-host-concurrency-integrated-v2`。
- 整合后 SQLite 初始化基线为 global **24** / project **59**，`dataset_layout=single-host-resource-v2`。此前分支的 23/58/v1 数据在初始化或 WAL 写入前拒绝；不自动迁移或重建。拒绝旧版本及旧标记的回归已通过。
- 保留工作流评估阶段的性能追踪标识，并验证每次评估只记录一次，兼容 main 的诊断探针。

## 基本验证

验证使用 Node 24、固定 Rust 工具链、专用临时数据集与私有 Host。没有重启现场 Host、操作运行中工作流或修改用户数据。以下为分组执行及修复复验的结果，不是一次全 workspace 或全流程运行。

| 范围 | 结果 |
|---|---|
| Rust 单元 | adapters 53/53、core 25/25。 |
| 业务与并发回归 | conversations 11/11、orchestration 55/55、manager_workspace 10/10、resource_commits 10/10、workflow_resource_concurrency 4/4。 |
| 数据合同 | dataset_preflight 7/7、schema_contracts 21/21，包含新布局的兼容性拒绝。 |
| Host | 55 通过、4 忽略；忽略项未计为通过。上述 Rust 分组合计 251 个不同测试通过，最终追踪补丁另有 1 项定向复验通过。 |
| 浏览器专项 | 共 10 项通过，覆盖经理审批/确认、审批原始证据、并行输入、计划草稿、执行页摘要、项目隔离及设置并发。 |
| 真实 HTTP 与数据集 | 认证、Host/Origin、静态资产、显式项目路由、幂等、Artifact、SSE 游标与退出、SIGTERM/SIGHUP 清理及重启持久化通过；空目录初始化和不兼容实际文件原样保留通过。 |
| 构建与静态检查 | 最终 debug Host 构建、check-style.sh（rustfmt、ESLint、TypeScript、Prettier、Web 构建、全目标 clippy `-D warnings`）、架构、出厂配置及 RPC 清单通过；268 个合同、9 个 HTTP 路由、42 个内部非方法标识。 |
| SQL 热路径 | 工作流事件页和事件头的实际 SQL 查询计划使用 `events_workflow_root` 索引定位，没有历史全表扫描或临时排序。 |

初轮回归发现异步启动资源别名、原始审批证据与取消资格等整合问题，已修复并复验；浏览器夹具的方法别名及新布局标题断言也已更新。原失败日志保留在证据目录的 `logs/`，不得只取首次失败或只取历史通过作为当前结论。最终风格、构建和追踪复验分别见 `devdesk-rebase-style-31.log`、`devdesk-rebase-host-build-31.log`、`devdesk-rebase-diagnostic-30.log`；最终布局下的 HTTP、数据集与浏览器专项见 `*-29.log`。

## 保留的验收边界

本次未运行真实供应商/GitHub 的全流程、100 个真实浏览器或独立负载机容量验收，未宣称 P6 通过。原分支 `965a0cf` 的本机 100 HTTP/SSE 客户端样本仅功能通过，稳态写 P95 为 326.60 ms，超过 300 ms 门禁；该旧源码样本不代表本次整合源码的容量结论，详见历史 [实施记录](EXECUTION.md)。

大型流程步骤 JSON 的物理写放大，以及 `query.history`、`query.repository`、`query.runtimes`、`query.tool_calls` 等通用旧读取可能恢复完整快照的成本，仍需后续联合性能验收覆盖。单 Host、并发读、资源级乐观写入和最终一致性设计约束保持不变。
