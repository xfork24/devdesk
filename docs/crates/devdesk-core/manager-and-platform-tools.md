# 经理工作台、平台工具与 Spec 检查

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

原生工具调用、平台副作用及回复交付分别记录。工具名、业务 Role 和界面入口不能替代原执行身份与冻结授权。

<a id="file-src-manager-workspace-rs"></a>

## 固定项目的经理工作台

[manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) 声明经理知识及创作工具，固定项目范围，不接受任意 RPC 方法或数据库路径。[manager_publication.rs](../../../crates/core/src/manager_publication.rs) 处理配置发布，操作仍须核对当前目录、原版本和调用来源。

经理可以查询资料、预演和创作配置；这些权限不自动赋予 Workflow 根所有权。Human 决定不能由经理工具伪造。

<a id="file-src-platform-tools-rs"></a>

## 平台操作与原生回复

[platform_tools.rs](../../../crates/core/src/platform_tools.rs) 注册通用处理器。[platform_preparation.rs](../../../crates/core/src/platform_preparation.rs)、[platform_runtime.rs](../../../crates/core/src/platform_runtime.rs)、[platform_replies.rs](../../../crates/core/src/platform_replies.rs) 分别准备意图、执行/接纳回执和向原生 Turn 交付回复。

仓库、测试、工作空间、配置与交付使用各自有限事实及不可变观察。业务结果和 reply_sent 分开；回复丢失只能重新交付原结果，不能重做副作用。未知结果先核对，取消和清理必须保留真实证明。

[platform_delivery.rs](../../../crates/core/src/platform_delivery.rs)、[platform_publish.rs](../../../crates/core/src/platform_publish.rs) 与 [platform_delivery_projection.rs](../../../crates/core/src/platform_delivery_projection.rs) 核对 PR、实际源/目标/候选和本地同步。确切回归、评审和 Human 门禁不能由模型报告替代。

<a id="file-src-spec-lint-rs"></a>

## 结构与内容检查

[spec_lint.rs](../../../crates/core/src/spec_lint.rs) 提供 `lint_specification_value`、`lint_delivery_value`、`enforce_delivery_value` 和创作入口 `specification_lint`。预览与实际接纳复用结构和 Spec 检查，不在此执行外部工具。

冻结 Spec 决定检查集合，位置证据绑定实际输入及摘要。自检不封存 WorkflowOutput，也不替代独立评审、人类签署或真实测试。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
