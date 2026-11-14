# 工作对象、材料与真实工程事实

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

Project、Feature、Task 描述工作归属，Document、Snapshot、Artifact 描述材料，GitContext 和 TestResult 描述执行对象与外部结果。把它们分开，可以避免“有一份报告”被误当成“已经跑过测试”，也避免同项目中的任意目录被当成当前任务的工作副本。

## 本页文件

- [src/lib.rs](#file-src-lib-rs)：项目、任务、材料与聚合状态
- [src/deliverable.rs](#file-src-deliverable-rs)：交付类型的格式、结构与产出来源
- [src/execution.rs](#file-src-execution-rs)：执行环境与精确 Git 路由
- [src/repository.rs](#file-src-repository-rs)：仓库操作、工作副本与 PR 回执
- [src/testing.rs](#file-src-testing-rs)：可重复执行的 Node 测试配方

<a id="file-src-lib-rs"></a>

## src/lib.rs：项目、任务、材料与聚合状态

源码：[crates/domain/src/lib.rs](../../../crates/domain/src/lib.rs)。

这是包的公共入口，也是基础工作模型的定义处。`Project` 保存仓库和并发选择；`Feature` 保存范围、前序关系和采用的来源材料；`Task` 保存依赖、工作副本和验收范围。`Document` 是可追溯的材料修订，`Snapshot` 固定上下文，`Artifact` 用相对路径与 SHA-256 定位封存内容。

`EngineeringState` 汇集工作对象、会话、仓库记录、治理记录和事件。部分兼容执行视图字段标记为 serde skip：内存中可用，并不意味着应复制进数据库。`material_owner` 要求材料唯一归属于 Global、Project、Feature 或 Task；`document_in_feature` 还允许 Feature 显式采用项目材料，而不是开放整个项目的所有文档。

`valid_dag` 检查重复节点、环和不存在的依赖；`Origin` 只区分 Human 与 External。单元测试覆盖并行 DAG、非法依赖及来源限制。错误统一使用 code/message，供 core、HTTP 和恢复逻辑保留可识别原因。

Error 还可携带 execution_failure（失败类别和副作用结论）。technical_not_applied/technical_unknown 只应由真正观察执行结果的边界调用；默认 err 不附带安全重试结论。源错误码和说明继续保留，完整失败来源在 WorkflowFailure 中表达。

代码定位：[`EngineeringState`](../../../crates/domain/src/lib.rs#L288)、[`MaterialOwner`](../../../crates/domain/src/lib.rs#L52)、[`material_owner`](../../../crates/domain/src/lib.rs#L58)、[`valid_dag`](../../../crates/domain/src/lib.rs#L352)。

关联阅读：[core/src/lib.rs](../devdesk-core/engine-and-ports.md#file-src-lib-rs)、[adapters/src/lib.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-lib-rs)、[domain/src/conversation.rs](agents-and-conversations.md#file-src-conversation-rs)。

<a id="file-src-deliverable-rs"></a>

## src/deliverable.rs：交付类型的格式、结构与产出来源

源码：[crates/domain/src/deliverable.rs](../../../crates/domain/src/deliverable.rs)。

`DeliveryTypeDefinition` 定义 markdown、diff、test_report、json 或 text 等格式，可附离线 JSON Schema 和 producer 限制。平台 producer 必须列出允许的工具处理器；这份声明随后要与真实调用 provenance 核对，不能靠输出中自填工具名称满足。

内容规范通过源根级 `spec_refs` 引用。`validate_delivery_specs` 要求至少一个可见的 Spec，并要求它有正文与检查项；Delivery 自身不是另一套规范正文仓库。格式/Schema 合法与内容评审通过分开，实际产物的结构检查由 core 调用执行端口完成。

代码定位：[`DeliveryTypeDefinition`](../../../crates/domain/src/deliverable.rs#L6)、[`DeliveryProducer`](../../../crates/domain/src/deliverable.rs#L18)、[`validate_delivery_specs`](../../../crates/domain/src/deliverable.rs#L49)。

关联阅读：[domain/src/source.rs](configuration-and-sources.md#file-src-source-rs)、[domain/src/spec_checks.rs](workflows-and-checks.md#file-src-spec-checks-rs)、[core/src/spec_lint.rs](../devdesk-core/manager-and-platform-tools.md#file-src-spec-lint-rs)、[core/src/configured_runtime.rs](../devdesk-core/workflow-execution.md#file-src-configured-runtime-rs)。

<a id="file-src-execution-rs"></a>

## src/execution.rs：执行环境与精确 Git 路由

源码：[crates/domain/src/execution.rs](../../../crates/domain/src/execution.rs)。

本文件区分配置解析出的 Harness/Sandbox、一次实际 `ExecutionInstance`、测试结果和 Git 上下文。ExecutionInstance 记录 operation、外部会话/环境与状态；`TestResult` 同时记录候选、环境、断言失败和预期失败，避免把环境错误当作合法 Red。

`GitContext` 显式携带项目、仓库、源/目标 owner、分支和 worktree。适配器必须按这份上下文定位资源，不能依赖进程当前目录。`AdvanceObservation` 把已应用、确认未应用、结果未知分开；未知推进不能直接重发，也不能被当成成功。

代码定位：[`ExecutionInstance`](../../../crates/domain/src/execution.rs#L32)、[`TestResult`](../../../crates/domain/src/execution.rs#L50)、[`GitContext`](../../../crates/domain/src/execution.rs#L78)、[`AdvanceObservation`](../../../crates/domain/src/execution.rs#L64)。

关联阅读：[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)、[adapters/src/git.rs](../devdesk-adapters/engineering-resources.md#file-src-git-rs)、[adapters/src/tests.rs](../devdesk-adapters/engineering-resources.md#file-src-tests-rs)。

<a id="file-src-repository-rs"></a>

## src/repository.rs：仓库操作、工作副本与 PR 回执

源码：[crates/domain/src/repository.rs](../../../crates/domain/src/repository.rs)。

`RepositoryAction` 表达克隆、准备工作副本、读取/推进和发布等具体意图。`RepositoryOperation` 将操作参数、当前状态和回执关联起来；`RepositoryWorktree` 保存实际 owner、分支、路径和基线。它们使重启后能够按原操作身份核对，而不是再创建一份资源。

`validate_repositories` 检查归属、操作与回执一致性，以及既有事实不能被随意改写。PR 请求/回执另外保存候选、head、合并状态和 merge commit；`PublishStage` 区分发布过程，`marker` 生成可用于远端查找的稳定操作标记。平台创建 PR 与远端实际合入不是同一件事。

代码定位：[`RepositoryAction`](../../../crates/domain/src/repository.rs#L13)、[`RepositoryOperation`](../../../crates/domain/src/repository.rs#L121)、[`RepositoryWorktree`](../../../crates/domain/src/repository.rs#L101)、[`PullRequestReceipt`](../../../crates/domain/src/repository.rs#L245)、[`validate_repositories`](../../../crates/domain/src/repository.rs#L131)。

关联阅读：[core/src/repositories.rs](../devdesk-core/integrations-and-repositories.md#file-src-repositories-rs)、[adapters/src/git.rs](../devdesk-adapters/engineering-resources.md#file-src-git-rs)、[adapters/src/github.rs](../devdesk-adapters/engineering-resources.md#file-src-github-rs)。

<a id="file-src-testing-rs"></a>

## src/testing.rs：可重复执行的 Node 测试配方

源码：[crates/domain/src/testing.rs](../../../crates/domain/src/testing.rs)。

当前 `TestRunner` 只提供 NodeTest。`TestRecipe` 固定文件清单、Docker image ID 与超时，不接受一个任意 shell 命令字符串。校验要求完整小写 sha256 身份、1–300 秒超时、1–100 个不重复文件。

文件必须是安全相对路径，不能以 `/` 或 `-` 开头，不能有反斜线、控制字符、空段、`.` 或 `..`。这同时限制目录越界和被解释成命令选项的输入。合法配方只是执行输入，真实通过/失败仍由报告器与容器回执决定。

代码定位：[`TestRunner`](../../../crates/domain/src/testing.rs#L5)、[`TestRecipe`](../../../crates/domain/src/testing.rs#L10)、[`validate`](../../../crates/domain/src/testing.rs#L17)。

关联阅读：[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)、[adapters/src/tests.rs](../devdesk-adapters/engineering-resources.md#file-src-tests-rs)、[adapters/tests/command_tests.rs](../devdesk-adapters/testing.md#file-tests-command-tests-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
