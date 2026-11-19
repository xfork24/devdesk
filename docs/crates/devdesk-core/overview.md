# devdesk-core：应用流程与状态

core 将一次用户或 Agent 请求变成受约束的应用操作：读取所需事实，检查身份和规则，准备外部动作，最后核对结果并提交状态。它通过 ports 使用外部能力，不导入具体适配器。

## 按功能阅读

| 专题 | 主要入口 |
|---|---|
| [Engine 与端口](engine-and-ports.md) | lib.rs、ports.rs、concurrency.rs |
| [配置与编译](configuration-and-catalog.md) | configuration_*、configured_workflows、configured_runtime |
| [工作流执行](workflow-execution.md) | workflow_start、dispatch、outputs、evaluation |
| [经理与平台工具](manager-and-platform-tools.md) | controllers、controller_*、manager_workspace、platform_* |
| [会话与运行资源](session-resources-and-permissions.md) | session_*、formal_*、conversation_*、provider_tools |
| [变更与恢复](workflow-changes.md) | workflow_change_*、rework、retries、reconciliation |
| [工程交付](integrations-and-repositories.md) | repository_*、platform_delivery、platform_publish、tool_calls |

阅读时跟踪状态提交点和外部调用点。Engine 调用了一个 trait，不代表业务责任已经移到 adapters；存储返回了一条记录，也不代表该记录能直接授权当前请求。

完整文件表：[files.md](files.md)。整体关系：[分层设计](../../design/layers.md)。
