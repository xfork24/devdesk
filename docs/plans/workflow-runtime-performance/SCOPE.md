# 与 single-host-concurrency 的实施边界

状态：本分支实施约束。2026-11-16。按用户要求分离 scope；本表规定本分支的边界，不把整合基础代码视为已经完成分工或联合验收。

## 已核对的对方工作

`codex/single-host-concurrency` 位于 `/Users/hengliu/.codex/worktrees/single-host-concurrency/devdesk`。首次核对提交至 `35bcaee`，包含连接池、资源提交合同、CAS/幂等及设置资源族；后续观察到 `7a74308` 的池化目录版本修复，以及 `3ac8486` 的项目路由/Host 生命周期提交。P2 全入口迁移、P4 执行页、P5 公共事件与调度、P6 容量仍未完成。不能把它理解成只做“100 个浏览器的前端优化”。

本分支位于 `/Users/hengliu/.codex/worktrees/workflow-runtime-performance/devdesk`，分支 `codex/workflow-runtime-performance`；从 main `026c05d` 建立，`d3ae7d4` 只整合对方已经提交的基础并保留 main 配置 W0–W7。对方脏工作树不复制、不改动，main 的未提交产品代码与暂停数据保留。

## 责任边界

| 领域 | 归属 | 本分支允许的工作 |
|---|---|---|
| SQLite 连接池、资源版本、有限提交、SQL 基线、通用 CAS 迁移机制 | single-host-concurrency | 使用其合同；不新增另一套连接池/CAS，不撤销或改写旧 writer 保护。 |
| 全局并发资源合同、项目注册/路由、跨项公平调度、SSE 广播 | single-host-concurrency | 本分支不重写这些基础能力；仅修正 workflow 调用层重复推进和额度重复扣减。 |
| Workflow 执行读取、空轮询、步骤复制/回滚、执行阶段去重、回执排队 | workflow-runtime-performance | 修改运行专用查询、`configured_runtime`、`conversation_runtime`、`Engine::tick` 相关调用及 Host 的 workflow 阶段；复用公共池和提交合同，不另建权威状态。 |
| 执行页摘要/分页、前端项目上下文、QueryCache 与局部刷新 | single-host-concurrency | 保留兼容性回归；不在此分支同步实现另一套 UI 或查询接口。 |
| 经理原生调用执行到回复、回复失败恢复、精确 worker 绑定 | workflow-runtime-performance | 修改 `core/controllers.rs` 的驱动/交付及对应 fixture；保持持久化先于回复，失败交付不重新执行业务动作。 |
| Agent 原生平台回执、结果负载 | workflow-runtime-performance | 修改 `core/platform_tools.rs` 内的回复编码及计时；不改写一般存储/HTTP 读取接口。 |
| 冻结上下文与权限恢复的重复编译 | workflow-runtime-performance | 修改 `adapters/source_views.rs`；保留确切源/完整作用域及每快照重验，缓存不代替派发授权。 |
| Docker create/inspect 错误分类、输入是否送达及停止证明 | workflow-runtime-performance | 修改 `adapters/docker.rs` 及权限/原生执行回归；不增加安装/凭据资源提交旁路。 |
| 经理失败/取消投影、超时原因分段 | workflow-runtime-performance | 不将取消留作 active；区分平台等待与其他墙钟区间。冻结预算仍按原规则；新预算策略另行配置修订。 |
| Host 启动器生命周期日志与信号转发验证 | workflow-runtime-performance | 修改 `scripts/dev/run-host.py` 及独立子进程测试；不改 Host HTTP 生命周期 CAS。 |
| 100 前端完整容量 | single-host-concurrency 主导联合验收 | 本分支提供 native call 分段指标和执行/故障用例；不重复建设负载器，不以本分支定向结果宣布 100 前端通过。 |

## 共享文件与接入方式

- 本分支不修改 `connection_pool.rs`、`resource_commits.rs`、`core/concurrency.rs`、`sql/`、Host 项目路由、SSE 和 Web 产品代码。基础整合产生的合并内容单列，不能计成本分支功能。
- `core/ports.rs` 只增加 workflow 运行提示/计数的薄端口；实现位于新增 `adapters/runtime_queries.rs` 并通过已有连接池读取。`adapters/lib.rs` 只有委托与冻结恢复清理接入，不修改 Store 结构、连接池、事务或 CAS 语义。
- `Workspace::tick` 只调整 workflow 的重复阶段与本项目额度传递；不重写项目注册/路由和跨项公平队列。`host/main.rs` 仅增加 tick 分段观测与跳过过期补跑，不修改 HTTP/SSE/生命周期 CAS。
- 冻结恢复清理在 `adapters/lib.rs` 的加载开始处调用一次 `source_views::begin_snapshot(memo)`，此外只有运行查询薄委托，不修改 Store 结构、连接或提交。若对方替换该加载路径，在新读取上下文入口执行同一清理语义；不复制旧 Store。
- 共用 `adapters/tests/orchestration.rs` 时只增加经理原生交付/故障用例；fixture 的 transport rejection 属于测试控制，不进入生产。其他共享测试按测试函数区分，不能以整文件覆盖整合。
- 文档只修改本计划、执行链/启动器说明及必要索引。对方计划的进度由其实施记录维护，不在本分支重新宣布完成。
- 后续只整合对方明确提交的变化；遇到其接口演进，调整本分支的薄接入和测试。不能为了减少文本冲突而引入第二套权威运行状态。

## 本计划的收敛

F1 的通用存储基础、F2 的有限提交机制和 F5 的执行页归对方；本分支负责 workflow 对这些能力的使用。Workflow 运行专用查询、步骤检查点延迟复制、空阶段跳过、重复驱动与原生衔接属于本次，不能把主要执行性能工作外移。F0/F6 提供 workflow 自身的调用/恢复证据，完整负载环境复用对方入口。

原验收中的零完整历史恢复、100 前端延迟和跨项目吞吐仍是最终联合目标；它们依赖对方的工程读取/提交/调度落地，不能据本分支的局部改动标为通过。本分支独立完成条件是自有修复、原生驱动/重启/撤权回归、局部性能对照和兼容性验证通过，并列出待联合验证的门禁。
