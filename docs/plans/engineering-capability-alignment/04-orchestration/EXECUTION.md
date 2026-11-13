# S4 执行与阶段验收

增加 `orchestration_policy` 源、项目/工作 `OrchestrationSelection`、根实例 `workflow_driver` 和可信 Driver Actor。Rust/Agent/人工调用同一控制内核；Rust 使用已有分页就绪/完成/确认投影，自动派发、接纳及完成父子实例。业务 worker 意图在控制事务内落盘，事务外执行。结构化技术错误码与策略/步骤预算同时允许才自动重试，未知结果不重发，业务失败/人工输入不合成成功。

唯一根控制归属、epoch、租约过期刷新、计划/控制版本与命令幂等联合约束自动动作。切换撤销旧 Controller 的控制代次、未派发意图和根/子委托；不销毁业务 worker/已封存结果。已结束根实例空闲 tick 不续租或写业务状态。Agent 模式复用既有配置 Harness 的真实原生工具通道。新增 UI 默认/当前模式及策略精确选择、查询和等待诊断。默认目录仅新增可编辑策略，保留人工默认行为。

定向验收通过：

- Rust Control 图完成、空闲零业务写入、重启不重派；人工确认等待/通过；暂停和子实例完成：3 项。
- 自建 Agent → 实际平台材料 Tool → Control → 子流程原语图：1 项，无经理模型、无 Feature/Task 开发流程；Harness 使用明确 fixture，实际 SQLite/产物/平台回执。
- Agent→Rust 时已准备原生调用拒绝、最终仅两次业务 dispatch：1 项。
- 原生 Agent 动作落盘后重启/回包幂等、人工接管撤销委托：2 项；原有观察不隐式派发：1 项。
- 配置变更/资源组合/Schema：34 项通过；Web production build、分层/目录检查通过。所有 workspace test target 只编译（no-run）。

日志 `/tmp/devdesk-s4-*`。原计划默认 Feature/Task 全流程、全部 foreach/故障/切换组合及真实经理模型对照未运行，遵循用户限制，不将自建原语 fixture 写成供应商/全工程验收。有限错误码重试及 Rust→Agent 的全部在途交错尚需扩展故障验收。
