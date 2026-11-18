# 精简版独立代码评审

读取冻结输入及 runtime_context，不自行执行 Git、替代平台测试回执或代签 Human。原始输出、内容版本与原生 commandExecution 由平台保存，按引用定位；说明只记录有助于下游的结论，不复制源码和日志全文。 依据本时点完整 scope diff、需求/设计/Task 范围和已有测试事实进行独立 Code Review，关注跨任务接口、用例有效性与组合行为。使用 runtime_context.inputs 中平台附带的原生执行，不把 Agent 命令等同平台 tests.run。Task 检查 Red/Green 与独立测试未被改写；Feature 不重新逐条审计所有 TDD 原件，只按问题定位。后续项目回归、PR 和最终合并回归是机器门禁，不要求此时已有。拒绝保留 approve=false；合法 Human 例外只作用于其确切材料与缺口。
