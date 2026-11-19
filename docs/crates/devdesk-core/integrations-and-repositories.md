# 工程资源与外部工具的应用流程

core 为 Git、测试、交付和接入工具确定执行条件，并检查结果能否用于业务推进。Git 命令、网络请求和容器实现位于 adapters。

## 工程交付路径

[repository_dispatch.rs](../../../crates/core/src/repository_dispatch.rs) 与 [repository_runtime.rs](../../../crates/core/src/repository_runtime.rs) 处理仓库操作的准备与运行；[repository_projection.rs](../../../crates/core/src/repository_projection.rs) 处理结果的应用视图。项目接入和更新有自己的应用上下文，不通过 UI 查询顺便执行。

[platform_preparation.rs](../../../crates/core/src/platform_preparation.rs) 形成固定的工程工具请求，[platform_publish.rs](../../../crates/core/src/platform_publish.rs)、[platform_delivery.rs](../../../crates/core/src/platform_delivery.rs) 处理发布和交付，[platform_facts.rs](../../../crates/core/src/platform_facts.rs) 读取相关依据。

输入包括原仓库/工作区、候选提交、测试或评审结果和操作身份。输出是可追踪的工具结果以及允许的业务修改，不能直接采信模型声称的 commit 或测试结论。

## 外部工具路径

[tool_creation.rs](../../../crates/core/src/tool_creation.rs) 准备已配置工具，[tool_calls.rs](../../../crates/core/src/tool_calls.rs) 管理调用记录和事实，[integration_operations.rs](../../../crates/core/src/integration_operations.rs) 处理发现与导入等操作。

[integrations.rs](../../../crates/core/src/integrations.rs) 固定讨论或执行采用的接入选择。外部服务、账户绑定、工具目录和当前权限共同决定调用内容；定义存在不意味着当前项目已经授权。

工具执行经 ToolExecutor。输出 Schema 校验后仍需保留实际调用来源，尤其是有远端副作用的调用。失败或取消不等于远端回滚，未知结果进入核对。

## 凭据与跨库发布

[credential_updates.rs](../../../crates/core/src/credential_updates.rs) 处理凭据更新的应用身份；实际秘密保存由适配器完成。更新生成新引用，既有运行仍使用原引用。

[manager_publication.rs](../../../crates/core/src/manager_publication.rs) 和 [experience_directory.rs](../../../crates/core/src/experience_directory.rs) 等维护共享目录和查询入口。跨库摘要可以稍后同步，但不能代替来源库的授权与最终提交。

验证入口：[repository_dispatch.rs](../../../crates/adapters/tests/repository_dispatch.rs)、[delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs)、[integration_operations.rs](../../../crates/adapters/tests/integration_operations.rs)、[credential_updates.rs](../../../crates/adapters/tests/credential_updates.rs)。外部实现见[工程适配器](../devdesk-adapters/engineering-resources.md)和[接入适配器](../devdesk-adapters/external-integrations.md)。
