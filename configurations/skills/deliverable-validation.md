# Deliverable Validation

## 先读输出合同

核对冻结 `outputs`、`output_contract` 和 Spec。简单 Markdown 的标题和正文规则可直接核对，结构不明或遇到校验诊断时使用当前端口的确切 Delivery 及完整草稿。

## 检查内容评审报告

每份 `content_checks` 报告提交前调用一次已授权的 `spec.lint`，传入 `delivery_ref` 和完整 `value`。从 `outputs.<port>.definition.id` 或 `check_report_contract.lint.delivery_ref` 复制引用。按 `outputs.<port>.required_checks` 填写，每个 key 恰好一次；`content_report_required_checks` 可复核此集合，通用格式检查不追加到报告。

使用 `content_report_sources` 给出的实际 `source_ref`、`value_digest` 和定位。Markdown 优先用对应段落的 `sections[].locator`，行或字符区间在真实 `line_count` 和 Unicode `character_count` 内；结构化输入使用 `json_pointer`。选填的 `quote` 必须与整个定位范围相等。

根据诊断修正草稿，保留来源、摘要和必需证据，重新校验。只有 `structure_valid` 与 `content_report_valid` 同时为真才提交通过格式检查的报告。无法调用或无法修正时如实说明实际诊断。

## 区分内容缺陷与材料缺失

能够判定的错误、矛盾或遗漏应提交 `approve: false` 及准确证据，交由经理处理。必要事实或人类决定缺失时才请求输入。允许的机械计算可以核对原例，计算和格式检查的结果只证明该次核对。
