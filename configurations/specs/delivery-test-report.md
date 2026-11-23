# Test Report Specification

## 记录实际执行

报告提供 `operation_id`、`status`、`candidate`、`environment`、`origin`、`recipe`、`assertion_failed`、`expected_failure`、`failure_type` 和 `execution`。执行记录包括命令、目录、起止时间、退出码、原始标准输出和错误输出及测试文件快照。

## 结果判定

核对当前步骤的真实 `tests.run` 请求、配方、版本及终止事实。业务断言失败和语法、依赖、环境、超时、取消、未知分别记录。有效的预期 Red 满足其阶段条件，Green 仍需同一测试在确切实现版本上实际通过。
