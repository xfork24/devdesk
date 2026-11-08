# R1 / R3 / R5 领域与会话实施记录（进行中）

已实现：五个不可变 Agent 身份根及角色能力；Project/Feature/Task 讨论 Session、冻结运行时/模型/适配器构建/凭据引用、后继 Session 和上下文快照；独立 Turn、追加事件、双向交互、结构化 Proposal、执行实例及协议 Artifact。每次发送保存输入快照和来源 owner revision，实际派发使用该快照的文档修订。Project 讨论不伪造 Feature/Attempt/Run。

Document/Artifact/Snapshot 采用 Project 或 Feature/Task 的明确互斥归属；Feature 可固定 Project Requirement 修订。Proposal 支持提取、人工编辑、新修订、采纳、拒绝和 TaskPlan 实例化；复用正式领域命令，检查目标版本、输入修订、跨项目引用、验收映射与依赖环。TaskPlan 在规划 Review/Snapshot 门禁通过后才可实例化，稳定 key → Task ID 映射持久化。

数据库目前为全局 v3、项目 v31，逐条追加 SQL 升级。旧 Sim Session 原生身份保留 NULL；旧 provenance 未改成 external。历史 v1 测试从已提交 v1 SQL 创建旧库，只复制原有列，验证升级保留历史。

实际专项：`cargo test --locked -p devdesk-adapters --test conversations` 9 passed；包括派发意图先提交、导出前保留资源、停止失败不能完成、重启未知输入不重发、重复采纳、过期/越界拒绝、TaskPlan 依赖环及规划门禁。此处使用显式故障适配器/领域夹具，不算 live 供应商验收。

真实 HTTP 会话证据见 R3 两供应商 application JSON；真实浏览器生成并采纳 Project Requirement 的证据见 browser-proposal JSON/PNG。均为实际模型调用，不是 UI 拦截响应。Codex/DSH 连续轮次保持原生 Session，容器重建后续用；每轮确认 writer revoked、封存脱敏协议日志并清理所属容器。

已进一步实跑：R5-planning.json 记录真实规划 Run、两次独立 Review、initial Snapshot、两个 Task/worktree；R6-implementation.json 记录 Codex/DSH 两条实际 TDD、固定 SHA 的 Docker 测试报告、Delivery 和独立 Reviewer。首次工具边界失败保留在 R6-tool-boundary-failure.json，修复后采用新 Attempt，未修改旧失败。

PR 操作账本与远端合入正在真实验收；完整 A01–A22 恢复矩阵与运行包尚未完成，计划保持进行中。


阶段交付继续补强：Delivery.source_deliveries 使 Feature 收尾冻结已接纳 Task 的实际交付，候选冻结源交付；原生 Reviewer 接收递归来源证据和固定版本 diff。schema v31 对既有 Merge 仅复制已经明确记录的 source_delivery 关系，不推定历史收尾输入或重写 Artifact。Feature 收尾加入独立测试修订阶段，严格区分 Task TDD Red 和 Feature 全量测试。
