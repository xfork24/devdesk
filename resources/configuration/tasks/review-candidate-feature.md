# 独立评审 Feature 候选

核对 Feature 主线候选差异、冻结目标、固定源 Task 交付、完整 Feature 测试及候选验证证据。按完整 Feature 范围独立评审追溯、范围、测试和失败路径。不要宣称已经合入。

## 内容校验与模型 Approve

依据 Feature/候选验证及评审 Skill，检查完整验收、实际候选内容、测试和目标基线是否一致，审查失败恢复与交付限制。只对冻结的精确候选作出结论。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
