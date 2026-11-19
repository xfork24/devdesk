# 术语表

| 术语 | 本项目中的含义 | 主说明 |
|---|---|---|
| Host | 管理数据集、对外接口和后台运行的服务进程 | [总览](overview.md) |
| Engine | core 中承载应用用例、持有外部端口的对象 | [分层](layers.md) |
| Port | core 声明的外部能力 trait | [分层](layers.md) |
| Adapter | Port 的具体实现，处理数据库、进程和外部服务 | [分层](layers.md) |
| Harness | 与模型交互并执行工具的程序及协议 | [运行](agent-runtime.md) |
| Sandbox | 运行程序的隔离环境与资源限制 | [运行](agent-runtime.md) |
| Recipe | 将 Harness 与 Sandbox 组合后的安装和执行配方 | [配置](configuration.md) |
| Source revision | 一份配置定义的不可变修订 | [配置](configuration.md) |
| 冻结 | 保存本次执行确定采用的配置、输入或资源版本 | [配置](configuration.md) |
| Session / Turn | 一次持续会话 / 其中的一轮输入和响应 | [概念](domain-model.md) |
| Delivery / Spec | 输出结构与来源要求 / 内容要求 | [概念](domain-model.md) |
| Artifact | 带身份、归属和摘要的产物内容 | [数据](data-and-recovery.md) |
| 接纳 | 核对执行结果及条件后，让它成为可推进流程的结果 | [工作流](workflow.md) |
| Intent | 在外部动作前保存的操作记录 | [恢复](data-and-recovery.md) |
| Receipt | 对一次操作结果的记录；不一概代表成功 | [恢复](data-and-recovery.md) |
| Reconcile | 核对外部实际状态，解决结果不明的问题 | [恢复](data-and-recovery.md) |
| CAS / 原版本检查 | 仅在相关对象仍为预期版本时提交修改 | [并发](data-and-recovery.md) |
| 幂等身份 | 用于识别同一请求重放的稳定标识 | [并发](data-and-recovery.md) |
| Projection / 视图 | 为读取或展示整理的数据，不能替代原业务事实 | [数据](data-and-recovery.md) |
| Controller | 管理工作流推进的经理运行及其关联状态 | [工作流](workflow.md) |
| Human | 经可信入口记录的用户决定来源 | [权限](security.md) |

代码中的名称保留英文，解释尽量使用具体动作。遇到一个新术语时，优先回到它对应的流程理解，而不必先背完整表格。
