# 独立评审 Feature 交付

独立评审完整 Feature 范围、固定的已接受 Task 交付、完整测试配方、实际执行结果和文档。核对验收追溯与失败路径；TDD Red 属于各 Task 原始实现，不要求 Feature 收尾另造一次 Red。

## 内容校验与模型 Approve

依据 Feature 验证及评审 Skill 检查完整 AC 覆盖、跨 Task 行为、当前代码上的实际测试、交付说明和剩余局限，局部 Task 通过不能替代完整 Feature 验证。

依据当前 Skill 的实际要求判断，不固定为测试版 Skill 的章节名称或关键词规则；Skill 调整后以所选精确修订为准。四类 checks 分别给出结论、对应材料位置或实际证据及具体缺口。输出 output_contract 规定的单个 JSON 对象：approve、summary、checks。存在内容缺口时 approve=false；不得因格式正确直接 Approve。该结果是模型建议，随后等待人类 Approve；人类可以保留理由覆盖模型结论。
