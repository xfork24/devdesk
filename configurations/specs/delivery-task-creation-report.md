# Task Creation Report Specification

## 已创建任务

`tasks` 保存真实 `id`、`feature_id`、计划 `key`、标题、说明和依赖 ID。存在规划依赖时，使用 `dependency_keys` 关联原任务键。

## 核验

将结果与冻结 Task Plan 逐项对照，确认键和 ID 一一对应，Feature 归属一致，依赖可解析、无重复且无环。报告记录本次任务创建工具的真实产出。
