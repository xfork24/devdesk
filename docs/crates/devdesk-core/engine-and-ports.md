# Engine、端口与资源提交

Engine 是应用用例的载体。它持有存储、产物、Harness、Git、Tests 等端口，还可装配工具执行、上下文读取和配置后端。入口见 [lib.rs](../../../crates/core/src/lib.rs) 的 `Engine` 与 `Engine::new`。

## 对象如何装配

core 在 [ports.rs](../../../crates/core/src/ports.rs) 声明 trait。adapters 创建具体实现，Host 的 [workspace.rs](../../../crates/host/src/workspace.rs) 中 `engine` 函数完成注入：先创建 FileArtifacts 和 NativeTools，再调用 Engine::new，并接入经验读取和配置后端。

因此测试可以替换端口，生产则使用真实适配器。测试替身不构成产品运行失败时的回退路径。

## 主要端口

| 端口 | 请求与结果 | 用于什么阶段 |
|---|---|---|
| EngineeringStore | 有限资源读取、版本化提交、专用查询 | 准备命令和保存结果 |
| ArtifactStore | 产物准备信息/正文 → 保存、核对、读取 | 交付物保存与查询 |
| Harness | SessionStart / TurnDispatch → 会话结果和观察消息 | Agent 执行 |
| Git / Tests / PullRequests | 固定版本的工程请求 → 实际工程结果 | 检查、测试、发布和合入 |
| ToolExecutor | 工具调用 → 开始、轮询、核对或取消 | HTTP/MCP 等外部工具 |
| SessionResources | 所选配置 → 安装、权限和凭据引用 | 会话运行准备 |
| ApplicationEnvironment | 分配 ID 和读取时钟 | 把环境输入与纯领域规则分离 |

端口的方法往往同时包含开始、轮询、核对和清理。外部调用超时后，业务层仍需要知道原操作是否发生，不能只依赖一个同步成功/失败值。

## 一次状态提交

[concurrency.rs](../../../crates/core/src/concurrency.rs) 定义 StoreNamespace、ResourceKey、ResourceVersion、ReadSet 以及提交相关结构。命令准备时记录用于判断的对象版本，包括“某对象必须不存在”的条件；提交时由存储原子检查。

例如派发步骤时，不能只校验步骤自身，还要校验用于授权的计划、控制状态和依赖。`ReadSet::merge` 遇到同一资源的不同版本会返回冲突，而不是自动选择更新的一版。

应用代码生成变更与事件，存储适配器执行 SQL。外部 I/O 在事务外发生，结果回来后再使用原操作身份和新的明确提交步骤接纳。

## 阅读和扩展

新增能力先判断是否已有端口足够表达。如果需要新端口，定义业务需要的输入输出，不直接暴露 Docker CLI 字符串或数据库连接给 core 调用者。实现端口后在 Host 装配，并为失败和核对路径设置测试。

相关验证：[resource_commits.rs](../../../crates/adapters/tests/resource_commits.rs)、[collection_versions.rs](../../../crates/adapters/tests/collection_versions.rs)、[workflow_resource_concurrency.rs](../../../crates/adapters/tests/workflow_resource_concurrency.rs)。
