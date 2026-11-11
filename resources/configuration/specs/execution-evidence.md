# Execution Report 内容规范

## 目标与范围

记录 Agent 实际执行的每个增量的 Red、Green 与回归。

## 必需内容

runs 按实际顺序保存 sequence、phase、完整命令、cwd、起止时间、exit_code、完整 stdout/stderr、files、environment 和 termination。files 包含 path、SHA-256 和当时内容。环境记录 runtime/version/platform/arch；终止记录 status/signal/error/timed_out/cancelled。多增量逐项提供 Red 和 Green，环境故障不能作为业务 Red，中断或未知不能作为 Green。不能仅交临时路径或摘要，不能冒充平台测试回执。

## 来源与一致性

产物必须对应当前 Project、Feature、Task 和确切步骤轮次。引用输入产物、固定代码版本和规范修订；事实来源与作者说明分别保留，不能以成功文字替代实际回执。不得复制其他任务或过期版本的证据。

## 完成与拒绝标准

- 逐项核对报告必填字段、检查集合或逐增量事实，内容充分且有可定位依据。
- 保留 Agent 作者与执行来源；交叉核对平台报告和确切代码快照。
- 核对本规范要求的对象、版本、依赖和结果一致；指出具体差异和影响。

缺少必需内容、来源不符或结果未知时明确拒绝或标明限制；未执行不写成通过。结构通过后仍须按当前步骤声明的业务范围评审。
