# 工程适配器接入契约

产品只执行原生 Codex/DSH、Git、Docker 测试和 GitHub 操作。原型模拟执行器及其回退、故障场景、逻辑资源和受控继续操作已移除。历史来源字段与数据库升级路径保留，旧原型项目仅支持读取，不派发新任务。

## 接口与装配

公共接口位于 `devdesk_core::ports`：

| 接口 | 当前实现 | 职责 |
|---|---|---|
| `Harness` | `RoutingHarness` → `CodexHarness` / `DshHarness` | 按 Session 冻结的供应商打开 worker、派发轮次、读取事件、回答交互、停止及清理 |
| `Git` | `CliGit` | 管理仓库/worktree，读取冻结版本，构造候选，按预期目标 CAS 推进及核对 |
| `Tests` | `CommandTests` | 对冻结代码、测试定义和运行实例异步执行测试、停止与清理 |
| `PullRequests` | `GitHubPullRequests` | 查询或创建固定候选对应的 PR，记录远端交付事实 |

`devdesk_adapters::execution_adapters(root)` 创建原生实现集合；构造过程不调用 Docker、GitHub 或模型。`Engine::new(store, artifacts, adapters)` 和 Host 的 `Workspace::open_with_adapters(root, adapters)` 保留依赖注入，切换项目共享同一组执行器。Docker 容器由 `DockerSandbox` 管理，不再存在描述逻辑资源的 Sandbox 抽象或同步假测试接口。

## 执行、接纳与恢复

- 先持久化操作意图，事务外调用供应商，再保存回执并由 Core 接纳。适配器不能直接推进业务终态。
- Attempt 冻结范围、文档、配置、GitContext、代码基线与原生阶段 Session。缺少真实仓库或冻结 Session 时拒绝执行，不生成替代结果。
- Harness 使用原生轮次和 `RuntimeExecution` 记录准入、输入消费、worker 归属、停止证明及协议产物。取消或传输不确定时保留未知状态，不能把迟到结果当作新的执行成功。
- TestRequest 包含操作 ID、阶段、候选、测试定义、GitContext 与执行实例。Node 报告器位于 `tests/reporters/node-reporter.mjs`，编译嵌入 Host；它只接纳 Node 测试事件，区分断言失败与环境失败，忽略 stdout 中伪造的报告。
- 测试请求和回执存入 EffectRecord。恢复使用已记录请求及原操作 ID；先确认写入者停止并封存证据，再清理容器。技术重试保留原执行历史。
- 适配器身份必须稳定且包含不可变资产摘要。旧原生测试的路由包装身份仅在其实际执行器和 reporter 摘要仍匹配时允许兼容读取；这不恢复旧模拟执行器。
- Git 目标推进使用 MergeAdvance。`reconcile_advance` 明确返回 applied、not_applied 或 unknown；未知结果不得盲目重发推进。
- 本地 CAS 与远端精确 lease、PR merged 核对分别记录。只有远端事实与冻结候选一致时才完成工程交付。
- Artifact 使用不可变 ID、相对路径与 SHA-256；暂存、fsync、rename 后再提交引用。导出失败不能提前清理执行环境。

## 回归与真实验收

模块与适配器回归位于 `crates/`，跨进程入口位于 `tests/entrypoints/`，浏览器回归位于 `web/tests/`。`tests/support/runtime.rs` 只编译进测试，提供显式原生接口替身；没有产品模式开关或对外模拟入口。`tests/fixtures/legacy/` 保存静态历史数据，用于验证旧库迁移、产物完整性和只读兼容。

真实供应商、Docker、GitHub、凭据及浏览器验收位于 `tests/live/`，使用专用项目、仓库和容器。当前操作流程与平台证据见 [真实工程接入](real-engineering.md) 和 [测试索引](../tests/README.md)。
