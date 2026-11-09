# AgentID 原生经验与场景恢复验收

日期：2026-11-09 02:55:01 +0800

- 同一精确 Harness 构建实际运行两个 Model：gpt-6.1-sol / gpt-6.1-terra。
- Model、composition_hash、AgentRevision 和 AgentID 分别保存；两个实例经验互相隔离。
- 原生工具按 index → detail 渐进读取本实例的历史材料，查询完整性为 verified。
- AgentID A：85bbf09e-f67f-4661-8699-2c9ffcf3c51c；AgentID B：681b1846-cfba-40bb-ac74-53d5328e2386。
- 来源 Project：fee25cdb-61bd-407c-a260-88202869f332；快照：03a5336f-afe2-4775-852e-14a7404794fe。
- Global AgentID 在第二个现有验收项目读取第一项目经验；Project AgentID 越界被拒绝。
- 项目名称和精确时间过滤命中原快照；跨项目恢复产生新 Session，并在真实模型输入中保留来源 marker。
- 恢复前后当前 Project 对象相等；没有改写代码、正式文档或工作流配置。
- 恢复 Project：a8e76164-5c41-4817-b575-e8eba1206b21；新 Session：b4de6536-d64d-4c0b-b614-f47e6bccd789。

结果：通过。宿主 macOS Apple Silicon，原生 Codex Linux Docker。

数据可用性更新（2026-11-09）：用户随后明确要求清空本工作区全部 data/ 和数据库，相关旧本机验收数据已删除。本文记录当时的执行事实；其中运行 ID、Artifact 和快照不再代表可读取的当前本机数据。新初始化见 ../builtin-defaults-and-roles/EXECUTION.md。
