# Review Report Specification

## 报告内容

对确切冻结材料提交 `approve`、`summary` 与 `checks`。每个 `key` 来自当前输出端口的 `required_checks`，逐项写出 `passed` 和可定位的 `evidence`，每项恰好一次。报告自身的格式检查不加入目标检查集合。

## 接纳与证据

每项判断指向确切产物、版本和有效位置。所有检查通过时才使用 `approve: true`。缺陷、误用证据、缺少来源及未知结果须明确记录；说明影响、修订建议和可复验条件。后续人类决定保留独立记录。
