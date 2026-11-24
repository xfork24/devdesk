# Workspace Snapshot Specification

## 工作区身份

平台读取或封存的工作区通过 `workspace_id`、`owner_id` 和 `revision` 标识。存在 `changes` 或 `scope_changes` 时，分别核对相邻比较与整个范围比较。使用 Git 版本绑定结果。

## 核验

产物对应本次项目、步骤及实际操作回执。核对工作区归属、当前范围和 Git 对象，版本丢失、来源不符或结果未知时如实报告。
