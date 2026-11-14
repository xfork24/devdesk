# 工程适配器接入契约

产品执行原生 Codex / DSH、Git、Docker 测试和 GitHub 操作；测试替身不进入运行产品。当前业务编排见 [领域规范](domain.md)，字段见 [Schema](../schemas/README.md)。

## 接口与装配

| `devdesk_core::ports` 接口 | 实现 | 职责 |
|---|---|---|
| Harness | RoutingHarness → CodexHarness / DshHarness | 固定供应商会话、轮次、事件、工具回复、停止及清理。 |
| Git | CliGit | 受管理仓库 / worktree、版本导出、父提交、diff、候选、CAS 和发布核对。 |
| Tests | CommandTests | 对确切代码和配方异步执行测试、停止及清理。 |
| PullRequests | GitHubPullRequests | 固定候选对应的幂等 PR 创建与远端状态核对。 |
| ToolExecutor | NativeTools | 离线 Schema 校验及已授权 HTTP / MCP 工具调用。 |

`execution_adapters(root)` 创建实际实现，构造过程不调用 Docker、模型或 GitHub。`Engine::new` 和 `Workspace::open_with_adapters` 支持注入，切换项目共享执行器。`SessionResources` 端口提供实际安装、凭据和权限编译，core 负责组合及物化回执。DockerSandbox 管理真实容器，领域中的 Harness / Sandbox 是配置和历史描述。

## 执行与回执

执行前持久化确切意图；事务外调用实际实现；core 核对身份、归属、摘要和终态后接纳。适配器不得直接设置业务完成。StepExecution 关联完整路径、轮次、输入及实际 Session / Turn / ToolCall，平台工具可以没有模型会话。

Harness 接收 canonical `agent-task/v6`，包括任务、职责、Skill、Spec、输入来源、逐端口输出 Schema、预算及授权。Codex 和 DSH 使用同一任务正文，协议编码由各适配器负责。原生准入、消费、终态和 writer 停止证明缺一不可；不确定输入不自动重发。

Codex 的平台工具桥核对当前原生 worker、Session、Turn、请求和调用 ID，以及冻结 Capability / Step / Permission 的交集。业务步骤、根控制器和独立经理工作台分别校验所属执行、根/epoch 或固定项目，再共用对应平台处理器；请求、实际回执与回复交付分开记录。DSH 不支持该动态桥，配置时拒绝；普通讨论不提供需要 Workflow/Controller 授权的平台工具。

TestRequest 固定操作 ID、候选、环境、GitContext、实际实例和 TestRecipe。回执须逐项对应；Node 报告器只接纳真实测试事件，区分断言与环境失败，不信任 stdout 伪造报告。测试环境不挂载模型凭据，代码只读且禁网。

目录来自明确授权，不要求 `tests/` 前缀。适配器校验路径规范化、symlink、受保护元数据、读写冲突和实际工作副本。只读根目录不生成重复的点路径挂载，局部只读覆盖不能被更宽的写授权突破。

Git 候选是基于预期目标的实际单父提交，并返回平台读取的父提交和 diff。接纳核对当前计划的 ActionPermit、本次运行的候选和对应测试/评审条件；human 策略或 Approved 条件才要求确切人类决定，automatic 不产生人工签名。授权内的 waiver 单独记录，不能伪造实际测试、候选或 CAS。本地 CAS、远端 lease、PR head / merged / merge_commit 分别核对；平台只创建 PR，不执行远端合入，本地推进不能冒充远端交付。

取消等待真实停止，封存与清理分开确认。未知副作用进入核对，不盲目重试；技术重试使用新执行身份并保留原请求和结果。输出合同拒绝实际回执时保留观察证据并及时失败，不让步骤永久停留在运行状态。

Artifact 使用不可变 ID、相对路径和 SHA-256，先暂存、fsync、rename，再提交引用。执行和策略身份包含固定版本与资产摘要，缺失能力不能用模拟结果替代。

## 验证

默认回归、HTTP、浏览器、真实资源与模型入口见 [测试索引](../tests/README.md)。真实探针使用隔离数据集、二进制副本和专用测试仓库，退出后清理临时登录文件。接口替身、DSH 本地 SDK 夹具、协议准备探针与真实推理结果分开报告；历史报告不能替代当前执行器的验证。

运行准备按源修订中的构建文件/OCI digest 执行，安装以配方摘要、实际镜像及驱动/权限资产验证报告绑定；版本标签不是兼容判据。Codex 的 prepare 探测初始化、会话与动态工具注册，DSH 只探测初始化，均不调用模型或执行工具。

实现依据：[执行端口](../crates/core/src/ports.rs)、[Harness](../crates/adapters/src/harness.rs)、[真实测试](../crates/adapters/src/tests.rs)、[Git](../crates/adapters/src/git.rs)、[协议探针](../crates/adapters/src/runtime_verification.rs)。
