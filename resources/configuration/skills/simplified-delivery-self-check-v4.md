# 精简版交付自检

以冻结 outputs、output_contract 和所选 Spec 为准，先核对必需标题、字段与范围，再一次提交最终交付。平台会在接纳时执行机器结构检查；结构明确且可直接核对的简单文档无需先把完整正文再发给 spec.lint。仅在结构规则不清楚或诊断明确失败时调用 lint，且必须使用本端口的确切 delivery_ref 和完整草稿。lint 不能替代独立内容评审或 Human 决定。

content_checks 评审报告必须先调用当前步骤已授权的 spec.lint；参数只能是 delivery_ref 和 value。delivery_ref 原样复制 outputs.<port>.definition.id（或 check_report_contract.lint.delivery_ref），value 放完整报告对象，不能用 report/content 代替 value，不能把 result/output:result 当作 Delivery 引用，也不增加 spec_ref、spec_refs 或 output_port。不要先发送空检查项草稿来探测参数。使用该输出端口的确切 delivery_ref 与完整草稿；普通 Markdown 文档的免 lint 规则不适用于评审报告。只有 structure_valid=true 且 content_report_valid=true 才提交最终报告。lint 无法调用或校验未通过时，明确报告实际诊断，不宣称已通过。

定位以 lint 返回的 content_report_sources 为准：复制本轮 source_ref/value_digest，优先使用与判断内容对应的 sections[].locator；自选行范围不得超过 line_count，字符范围按 Unicode 字符和 character_count 计算，不能数 JSON 字符串转义后的显示行或猜测行号。修正定位后在同一轮再次 lint，直到通过；不得通过删除必需证据、替换来源、扩大到无关全文或修改判断来掩盖错误。

lint 顶层 checks 若属于 Delivery 通用格式规范，不是本步骤报告的检查项。报告始终以任务 outputs.<port>.required_checks 为准；诊断返回的 content_report_required_checks 用于核对同一冻结集合，不能据通用 checks 改名。校验失败先按实际诊断修正完整草稿，不试探未声明参数，也不把格式和内容检查的不同层次误判为绑定故障。

content_checks 报告须逐个覆盖 required_checks，每个 key 恰一次；引用 input_sources 中实际的 evidence_source_ref 和 value_digest。Markdown 使用有效行或字符范围，结构化输入使用 json_pointer；省略非必需 quote，填写时必须等于整个定位范围。每项只写可判定结论及必要理由，原始源码和日志保留引用。approve=false 时明确缺口，不用格式合格替代质量判断。

## 明确缺陷与缺少事实

独立评审已能依据冻结输入判定的矛盾、计算错误、遗漏或质量缺口，必须提交 approve:false 的正式报告及准确证据，交由经理按有限返工处理。上游文档已被确认也不等于内容永远正确；不能因为发现已知错误就返回 needs_input、要求用户再次确认明确规则。只有冻结材料确实不足，或存在需要用户作出的业务选择时，才请求补充输入。

已有用户提供的精确输入/输出例子优先原样引用，补充例子须核对预期结果，不重复扩充范围。机械计算和确定转换可用获准的临时原生命令核算，保留实际观察；这只是文档算例检查，不能代替业务独立测试或平台回归回执。
