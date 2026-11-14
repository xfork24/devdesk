# 交付草稿自检与冻结证据定位

## 草稿结构检查

按本次 outputs.<端口>.definition.id 取得精确 delivery_ref，使用当前获准的 spec.lint，参数为 delivery_ref 和完整草稿 value。优先在提交前检查每份必需交付；检查失败时按返回的具体规则修正文稿，再提交给原输出端口。章节和字段来自实际所选 Spec，不在本 Skill 固化文档模板。不能用另一份宽松 Spec、其他步骤的 Delivery 或不存在的工具替代当前检查。

工具成功表示得到了检查报告；structure_valid 只表示本配置的结构项成立。content_report_valid=false 时必须修正当前冻结输入的报告格式或证据；semantic_quality=requires_judgment 必须继续由内容评审判断，lint 不能签署人类确认、伪造测试/Git 回执或把草稿变成已接纳输出。未授权或工具失败应保留实际原因，不能自述检查通过。

## 内容评审证据

若输出为 content_checks 报告，checks 严格取当前 required_checks 的完整集合，每个 key 恰一次。需要定位的检查必须引用本次输入的实际 source_ref、value_digest 和有效 locator；报告的 source_ref 值从 input_sources.<input>.evidence_source_ref 复制，但字段名仍为 source_ref。证据必须指向该检查真正评审的材料，不能借另一份无关但结构合格的输入通过检查。

locator 使用所给类型 Schema。Markdown 字符串可用 1-based 且包含末端的 lines 范围，或 Unicode 字符 0-based 且不包含末端的 characters 范围；结构化输入用 json_pointer。可选 quote 若填写，必须等于所选范围全部文本。空 pointer 选择整个冻结输入，短摘录请改用精确范围或省略 quote。机器断言针对被引用的完整冻结输入重算，不能靠选择局部文字改变被检查对象。

大输入按输出读取工具的 next_offset 连续读取并核对摘要；未完整读取不能宣称全量评审。结构成立后仍须独立判断范围、内容、真实测试及完成条件；不符合时明确拒绝并给出可定位依据。
