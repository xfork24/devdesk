# 架构与 ADR

## 模块

- domain：类型、不可变版本、DAG、绑定、来源与验收规则；无 I/O。
- core：命令/查询、调度与交付协调；定义 EngineeringStore、ArtifactStore、HarnessPort、GitPort、TestRunnerPort、SandboxPort。
- adapters：SQLite 关系表、文件封存、模拟执行能力；不得绕过 core 改业务终态。
- host：loopback HTTP/SSE、会话与同源验证、数据集生命周期、静态 Web 装配。
- web：React/TypeScript 展示、编辑和调用 API；不设置业务状态。

## ADR-001：完整 Web

Web 是长期客户端，Rust 编译时嵌入 Vite 产物。Node 仅构建依赖，成品不需要 Node。未来 Native 客户端使用相同协议。

## ADR-002：跨平台单机

Rust stable 固定版本、bundled SQLite、普通路径及文件锁；不引入平台业务 API。Linux x86_64 与 macOS Apple Silicon 均已执行后端及 Web 实机验收，分别保存平台证据，见 [验收记录](acceptance.md)。

## ADR-003：真实规则、模拟外部能力

所有 Project 均为 simulation。Git/Harness/Tests/Sandbox 只产生有来源的能力结果；核心决定状态。适配器结果先校验结构，支持固定故障、取消和 hold/continue。后续真实适配器复用 ports 与契约测试，不需要重写 Review/Attempt 规则。

## ADR-004：持久化与幂等

规范化关系表保存高频标量列、外键和索引；只有确定类型的数组、绑定锁和供应商结果用 JSON 列。初版每次工作单元加载类型化工程状态并写回变化，优先事务一致性；不把整个项目存成 JSON。查询返回有界结果。后续可在同一 port 下优化增量 SQL，不变更领域语义。

命令及模拟 Git 目标推进在同一事务保存；operations 唯一键保存指纹与原结果。单 host 数据集文件锁，进程内写协调。未提交的 Artifact 可成为孤儿文件，但不得成为可见交付；只读时验证 hash。数据库迁移按 user_version 执行，不以删库升级。

## ADR-005：恢复与范围

重启将运行未确认记录标记 interrupted，释放模拟环境并允许显式重试。真实进程/Git 副作用恢复延期。Archive/迁移只预览，不写成功事实。只有自己初始化的空目录才能建数据集；不修改用户仓库。

## 阶段评审

每阶段报告列出实现、测试、限制和评审。独立 Agent 审查与作者自检分别保存；本轮 macOS 补充验收及独立结论见 [阶段报告索引](reports/README.md)。
