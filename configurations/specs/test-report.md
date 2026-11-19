# Test Report 内容规范

## 目标与范围

平台 tests.run 对确切快照或候选执行后的真实报告。

## 必需内容

报告提供 operation_id、status、candidate、environment、origin、recipe、assertion_failed、expected_failure、failure_type 和 execution。execution 记录命令、目录、起止时间、退出码、原始 stdout/stderr 和测试文件快照。业务断言失败、语法、依赖、环境、超时、取消与未知分别说明；期望的 Red 失败可满足当前阶段，但不等于代码测试通过。

## 来源与一致性

产物必须对应当前 Project、Feature、Task 和确切步骤轮次。引用输入产物、固定代码版本和规范修订；事实来源与作者说明分别保留，不能以成功文字替代实际回执。不得复制其他任务或过期版本的证据。

## 完成与拒绝标准

- 检查实际配方、被测版本、日志、终止结果与报告分类一致；未运行不得通过。
- 报告必须与当前步骤真实 tests.run 请求及回执一致，不能接受 Agent 自述或其他操作的结果。
- 核对本规范要求的对象、版本、依赖和结果一致；指出具体差异和影响。

缺少必需内容、来源不符或结果未知时明确拒绝或标明限制；未执行不写成通过。结构通过后仍须按当前步骤声明的业务范围评审。
