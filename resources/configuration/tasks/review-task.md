# 独立评审 Task 交付

作为独立 Reviewer，只评审当前 TaskScope、验收和自身 TestRecipe，核对真实 Red、Green、verify、差异和文档。Feature 完整测试和其他 Task 不是当前 Task 必需证据，不扩大到未授权模块。只读检查并基于事实给出逐项结果。

## 内容校验与模型 Approve

依据实现、测试及独立评审 Skill，结合真实代码差异和 Red/Green/verify 证据判断当前 Task 的接口、行为、错误路径、独立测试保护和使用文档。通过格式或作者自述不能替代内容和真实测试。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
