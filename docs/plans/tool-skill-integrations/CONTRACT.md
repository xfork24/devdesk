# 工具与技能接入执行契约

开发在隔离 worktree `codex/tool-skill-integrations` 完成；已 rebase 到主干 `4cca5d8` 并移除临时依赖快照，保留主干最终 Permission、模型服务商和凭据助手兼容合同。分阶段及真实运行证据见 [EXECUTION.md](EXECUTION.md)。

资源以不可变治理记录保存。接入源、外部 Tool v2、绑定、Skill 包分别修订；项目接入映射单独版本化。Feature 冻结每个 Agent 的完整 IntegrationLock，讨论 RuntimeBinding 冻结同一结构。旧 Tool v1、旧配置锁与历史 Agent 不修改。

外部动作只使用锁中定义的名称和参数。工具调用由 core 持久化意图，经 ToolExecutor 事务外执行，再由 core 接纳有来源回执；重复调用不能重做未知副作用。调用归属包括 project、session、turn、run、worker operation 和原生 callId。

HTTP 和 MCP I/O 在 Docker 中执行，使用冻结的执行权限、确切镜像、当前 worktree 范围及绑定的 origin/动作约束。认证引用只由适配器解开，不向模型、配置导出或普通协议日志提供秘密。管理员手工测试与模型调用复用执行通路。

SchemaValidator 为无外部引用 I/O 的实际运行校验。外部工具必有输入 Schema，输出 Schema 可省且只验证结构化结果；原生工具组不使用这些占位字段。Skill 包只有静态指令/材料，依赖缺失、脚本和越界路径不能隐式授权或执行。

Codex 动态工具需要显式协议支持。真实模型验收仅使用 Codex / GPT-6.1 Sol High；DSH 仅保留原接口，不执行真实模型。协议/HTTP/MCP/Docker/模型证据分开记录。
