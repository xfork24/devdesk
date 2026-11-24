# Diff Specification

## 内容和版本

交付完整 Git patch 以及平台保存的 `base_revision`、`revision`、比较范围、操作标识和正文摘要。保留新增、删除、重命名、文件模式、二进制变化及空差异的实际结果。

## 来源和验收

Diff 由真实快照或候选工具生成，关联确切 Artifact 和所属项目、Task 或 Feature 步骤。核对比较端点、原始结果和修改范围。后续评审引用原件；失效、缺失或截断的差异需先恢复完整证据。
