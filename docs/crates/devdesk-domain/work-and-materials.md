# 工作项与产物

这个专题解释“工作属于谁”和“结果来自哪里”。阅读入口是 [lib.rs](../../../crates/domain/src/lib.rs) 中的工程对象，以及 [artifact.rs](../../../crates/domain/src/artifact.rs) 中的产物类型。

## 对象怎样关联

Project 是工程范围；Feature 表示完整需求；Task 归属 Feature。工作流实例再绑定具体工作项。不能只凭标题或当前 UI 选择判断归属，调用必须使用对象 ID 和明确项目。

MaterialOwner 表达材料的归属范围，包括全局、项目、Feature 和 Task。这个范围会影响输入收集和可见性，不只是展示标签。

## 产物怎样形成

Artifact 保存身份、类别、来源和内容摘要等元数据。`ArtifactSeal::prepare` 根据已经确定的身份与正文准备保存信息；真正写文件由 core 的 ArtifactStore 端口及 adapters 完成。

这样把“这份内容应该具有什么身份和摘要”与“文件怎样安全写入”分开。SQL 中的工作流输出可以引用 Artifact，并保留它对应的端口和执行来源。

## 结构与内容

[deliverable.rs](../../../crates/domain/src/deliverable.rs) 描述交付要求，[spec_checks.rs](../../../crates/domain/src/spec_checks.rs) 表达内容检查和依据。结构合格与内容满足要求是不同检查；测试报告也不能仅靠匹配某个 JSON 字段就获得可信来源。

修改输出类型时，应一起核对 Delivery/Spec Schema、core 的输出接纳、出厂定义和显示组件。已有执行仍引用原定义，不能通过修改最新默认偷偷重解释旧产物。

相关验证：[schema_contracts.rs](../../../crates/adapters/tests/schema_contracts.rs)、[delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs)。前者关注格式，后者覆盖交付检查；它们不替代真实模型产出验收。
