# 独立评审方案交付

独立检查冻结的 Requirement、Design、TaskPlan、TestPlan 与配方，评估追溯、完整性、范围、测试设计和失败路径。本阶段在实现之前，按方案明确性而非未来代码是否存在作判断。

## 内容校验与模型 Approve

依据作者所用架构、项目规划和测试 Skill 及本阶段 Spec，独立核对方案取舍、接口、依赖、AC 责任和 TestRecipe 的实际内容及跨材料一致性。指出具体缺口，不要求未来实现已存在。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
