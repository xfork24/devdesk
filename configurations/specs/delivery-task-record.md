# Task Record Specification

## 单项任务身份

子流程输入必须携带确切的 `id`、`feature_id`、计划 `key`、`title`、`instructions` 与 `dependencies`。依赖使用真实 Task ID。

## 核验

对应原任务创建回执和当前 Feature，能够追溯规划条目和执行范围。交给 Task 子流程的是该项记录，整份任务创建报告由上游持有。
