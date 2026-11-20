# DevDesk 架构设计

DevDesk 将需求、Agent 执行、工程验证和人工决定组织成可追踪的工作流。Host 管理状态和执行资源，Agent 完成具体任务，用户通过 Web 工作台查看结果并作出决定。

## 从哪里开始

- **首次了解项目**：[系统总览](overview.md) → [分层与依赖](layers.md) → [核心概念](domain-model.md) → [工作流](workflow.md)。
- **理解 Agent 如何协作**：[运行与通信](agent-runtime.md) → [工具和接口](interfaces.md) → [数据与恢复](data-and-recovery.md)。
- **准备修改代码**：先读对应架构主题，再进入[代码手册](../crates/README.md)和[跨模块调用链](../crates/execution-flows.md)。
- **安装和运行**：直接阅读[运行指南](run-binary.md)。

## 架构正文

| 主题 | 读完能回答什么 |
|---|---|
| [系统总览](overview.md) | 系统解决什么问题，各进程在哪里运行？ |
| [分层与依赖](layers.md) | core、domain、adapters、host 如何分工和配合？ |
| [核心概念](domain-model.md) | 工作项、步骤、会话、产物之间有什么关系？ |
| [配置如何生效](configuration.md) | 一个 Agent 的角色、模型、工具、权限怎样确定？ |
| [工作流](workflow.md) | 从需求到交付，谁派发、谁检查、谁批准？ |
| [Agent 运行与通信](agent-runtime.md) | 经理怎样启动分析师，PRD 怎样返回？ |
| [工具和接口](interfaces.md) | 浏览器接口、Agent 工具和内部原语有什么区别？ |
| [数据、并发与恢复](data-and-recovery.md) | 状态存在哪里，中断或冲突后怎样继续？ |
| [权限与凭据](security.md) | 谁可以做什么，限制如何落到实际进程？ |
| [工程交付](engineering-delivery.md) | Git、测试、评审和合入怎样形成可核对的结果？ |
| [Web 工作台](web-workbench.md) | 页面如何提交命令并显示后台进展？ |
| [质量与限制](quality-and-limits.md) | 测试证明了什么，当前架构有哪些取舍？ |

## 按需查阅

[术语表](glossary.md) · [RPC 参考](reference/rpc.md) · [工具和调度原语](reference/tools-and-primitives.md) · [外部接入](reference/integrations.md) · [数据格式](reference/contracts.md)

[2026-11-20 完整工作流回归与性能基准](workflow-benchmark-2026-11-20.md)：包含原始时序、审批和主机休眠排除口径、全部步骤及真实回归证据。

这里描述当前实现。出厂流程示例以 [configurations/manifest.json](../../configurations/manifest.json) 发布的资源为准；历史方案和验收记录另见[计划索引](../plans/README.md)。某项历史测试通过，不代表当前机器已经通过相同验收。
