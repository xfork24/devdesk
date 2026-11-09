# 测试计划验证

核对 TestPlan 与每项 Task TestRecipe 是否覆盖冻结需求、设计接口、验收和失败路径。区分 Task 测试与 Feature 汇合测试，指出可执行配方、文件范围、导入及运行环境问题。不要编造已执行的测试。

## 内容校验与模型 Approve

依据冻结测试 Skill 阅读测试计划与配方，判断用例预期结果、正常/错误/边界/恢复覆盖、Task 与 Feature 分工及环境/断言失败区分是否具体可执行。计划阶段不得声称测试已运行。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
