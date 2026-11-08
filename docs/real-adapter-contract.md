# 真实工程适配器接入契约

补强计划的内核部分已经实现并由当前模拟用例和故障供应商夹具验证。真实 Harness/GitHub/Docker 的 SDK、账号、凭据、进程和容器接入不在本轮执行范围；接入时实现下列契约，再以隔离仓库/账号/容器运行真实用例，最后开放 live。模拟历史不得作为真实工程证据。

## 基接口、派生实现与装配

Rust 使用 trait 表达抽象基类的契约，用 `impl` 提供派生实现。四个公共接口定义在 `devdesk_core::ports`，模拟类与未来的真实类实现相同接口：

| 基接口 | 保留的模拟实现 | 后续真实实现示例（尚未实现） |
|---|---|---|
| `ports::Harness` | `SimHarness` | `CodexHarness`、`DshHarness` |
| `ports::Git` | `SimGit` | `CliGit` |
| `ports::Tests` | `SimTests` | `CommandTests` |
| `ports::Sandbox` | `SimSandbox` | `DockerSandbox`、`LocalSandbox` |

四个 Sim 实现分别位于 `crates/adapters/src/simulation/`，以 `impl Harness for SimHarness` 等形式实现公共接口；原有 `devdesk_adapters::SimHarness` 等导入仍然可用。旧接口名 `HarnessPort`、`GitPort`、`TestRunnerPort`、`SandboxPort` 保留为同一 trait 的别名，已有实现和故障夹具无需迁移。

`devdesk_domain::Harness` 和 `devdesk_domain::Sandbox` 仍是持久化领域对象；`devdesk_core::ports::Harness` 和 `ports::Sandbox` 是执行接口，分别表达配置/历史与运行行为，使用模块路径区分。

装配使用 `ports::ExecutionAdapters` 的四个 `Arc<dyn ...>` 字段。`Engine::new(store, artifacts, adapters)` 接收接口对象，既可选择全部 Sim，也可逐个注入其他实现。默认集合由 `devdesk_adapters::simulation::adapters()` 创建。host 的 `Workspace::open_with_adapters(root, adapters)` 将同一组实现传入全局配置 Engine 和各项目 Engine，项目切换时不会退回硬编码的 Sim 实现。

真实类直接实现对应 trait，如 `impl devdesk_core::ports::Harness for CodexHarness`。凭据、连接与供应商状态归具体实现持有，工程状态推进仍由 Engine 决定。当前应用入口使用默认模拟集合；真实供应商接入沿用下文的配置与验收约束。

Sim 的 `identity()` 明确固定为原有 `devdesk_adapters::SimHarness`、`SimGit`、`SimTests`、`SimSandbox` 标识，避免模块拆分改变持久化操作的供应商身份。真实实现使用独立、稳定且带版本的身份；历史操作继续由其原始适配器核对。

## 已落地的公共执行边界

- 操作先登记，再调用，再保存回执，最后由 Rust 核对并接纳。调用供应商不持有 SQLite 事务或单写协调器。
- Harness/Cancel/Test/Cleanup 使用 EffectRecord，Git 目标推进使用 MergeAdvance。固定操作 ID、供应商/适配器版本身份、请求、回执、状态和原因，支持持久化、查询及重启核对。
- `identity()` 应由生产适配器实现为稳定的供应商与适配器版本标识。默认类型名仅适用于内置/测试适配器，不能代表真实供应商版本。核对必须使用原适配器，不能用别的实现推定旧操作完成。
- Attempt 冻结 GitContext（Project、Workspace、源/目标 owner、branch、worktree、来源）、范围、绑定、Context 与代码基线。候选/推进/核对显式传入该上下文；修改当前 Project 配置不会重定向旧执行。
- Harness 输入包含 frozen_inputs、stage_outputs、锁定的 Agent/Skills/Standard/Spec 正文、代码定位、基线和运行实例。同一 Run 后续轮询复用首次登记的输入，不因兄弟节点产出而改变。TestRequest 还携带测试定义正文、候选和具体环境实例，适配器不需要依赖隐含全局仓库或私下读取工程数据库。

| 能力 | 请求与外部身份 | 接收及恢复规则 | 已运行的契约夹具 |
|---|---|---|---|
| Harness | Run/Session ID、完整执行绑定、冻结输入、运行实例与 adapter identity | poll 对尚未确认的执行（包括未确认终止的传输超时）返回未完成，不能伪造 failed 终态；终态回执绑定 Run/Session；取消等待确认；迟到或失效结果保存但不接纳 | 阻塞轮询期间提交取消/需求变化、错误核对身份、未知结果重启保留资源 |
| Git | Attempt.baseline、GitContext；Merge ID、源、预期目标、候选 | advance 只能按固定 ID 幂等调用；dispatched 后先查原供应商；接纳时再次检查目标与验收 | 推进后回执落库前崩溃；不重复推进；确认未推进后观察新目标并重建候选；派发前失效不调用供应商 |
| TestRunner | operation_id、定义 ID/正文、候选、阶段、代码定位、运行实例与环境 ID | TestResult 的 ID、阶段、版本、定义、环境与来源完全匹配；Red 断言失败与技术失败分开 | 错候选/错测试/错阶段/错环境拒绝；保存回执后导出失败不重复执行 |
| Sandbox/清理 | 稳定 Resource ID、Run 所有权及请求中的 adapter identity | resource() 只描述意图，不执行外部创建；真实环境由执行适配器配置/启动并回传实例身份；清理在产物提交后调用，重复 delete 必须按原 Resource ID 幂等 | 导出失败不清理；外部删除后中断，重启重放相同 Resource ID；失败清理的人工重试 |
| GitHub | Merge.change_request 固定 provider/repository/external_id/source_revision/target_revision；当前用例建立 simulation 的本地变更请求实例 | 真实 PR 创建/查询/同步沿用稳定操作 ID 与意图/回执协议；源目标变化重新验收。本地接纳与远端同步分开，不把 GitHub 自动合入当作本地 CAS | 当前冻结引用随候选保存；真实 GitHub 请求映射和远端权限/超时用例在供应商接入轮执行 |
| Artifact | owner、不可变文件 ID、SHA256、相对路径和来源 | 暂存、fsync、rename；提交引用前的孤儿文件不可见；导出失败保留回执与资源 | 磁盘导出故障、哈希及路径边界、重放不重复执行 |

## Git 推进的明确结果

`reconcile_advance(context, operation_id)` 返回 AdvanceObservation：

- `applied { revision }`：原操作已经推进到确定版本。内核只在候选、目标、源交付和 Review 仍匹配时补记完成。
- `not_applied { actual_target }`：供应商确认该操作未执行且不会再执行。内核保存观察目标、废弃旧候选，要求重新验证和 Review。
- `unknown`：无法确认。保留 needs_reconciliation，不盲目再次 advance，不解除目标推进协调。

merge.apply 只提交意图并返回 operation_id。调度器先重新校验意图仍有效，再调用供应商。即使供应商已经完成，回执与当前输入/目标不一致时也只保留事实与核对理由，不伪造 Merge completed。

## 真实供应商的接入验收

1. 为具体供应商提供版本和能力声明、连接/凭据引用、规范化事件映射和外部身份查询。不得把 data 工程库当成 Agent 可写工作区。
2. 用已落地的操作 ID 查询供应商事实，验证成功但回执丢失、确定未执行、未知结果、取消、重复/延迟事件和清理。供应商自己的去重账本须落在供应商或该适配器的持久存储中；内核账本不能撤销外部副作用。
3. 真实 Git 读取冻结仓库/分支/worktree，保留代码对象，构造并测试同一候选，以预期目标比较推进；验证 rebase+squash 与其他写者推进目标的竞争。
4. GitHub 使用测试仓库验证 PR 创建/同步、鉴权失效、请求成功后超时、源/目标变化；Docker 验证实例创建、导出、取消及重复清理，不删除共享服务/镜像。
5. 在真实来源证据和实例历史完整、上述用例通过后开放 live。真实环境重建、Archive 和 data 迁移执行仍需要各自专项实现与验收。

本轮已完成公共领域模型、请求路由、操作账本、回执接纳和恢复框架；以上供应商映射是明确的后续接入工作，不是未完成的内核 H3 工作，也不能用模拟夹具替代真实用例验收。
