# 独立评审 Task 候选

核对当前 Task 的冻结候选差异、目标基线、已接受源交付及候选真实验证证据。仅评审当前 Task 边界，不要求其他 Task 或 Feature 完整套件已存在。只读检查，明确逐项结果和来源。

## 内容校验与模型 Approve

依据候选验证及评审 Skill，核对当前 Task 候选的精确 SHA、来源、测试和范围内容；不要求未来 Feature 收尾已完成。给出与该候选相符的证据和具体问题。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
