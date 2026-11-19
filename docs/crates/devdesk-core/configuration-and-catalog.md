# 配置加载、发布与编译

这一组模块把源定义变成可执行的工作配置。输入是明确修订引用和调用者选择，输出是经过验证的发布、目录视图、工作流计划或任务输入。

## 主流程

| 阶段 | 文件与符号 | 处理结果 |
|---|---|---|
| 读取目录 | [configuration_catalog.rs](../../../crates/core/src/configuration_catalog.rs)、[configuration_queries.rs](../../../crates/core/src/configuration_queries.rs) | 摘要、正文、引用及采用关系 |
| 准备变更 | [configuration_changes.rs](../../../crates/core/src/configuration_changes.rs) | 操作集合、诊断和相关版本 |
| 发布定义 | [configuration_publication.rs](../../../crates/core/src/configuration_publication.rs)：prepare_configuration_publication | 可提交的发布结果 |
| 跨范围采用 | [configuration_transfers.rs](../../../crates/core/src/configuration_transfers.rs) | stage/apply/cancel 的记录及结果 |
| 编译流程 | [configured_workflows.rs](../../../crates/core/src/configured_workflows.rs)：compile_workflow_sources | 确定依赖和定义的执行计划 |
| 生成任务 | [configured_runtime.rs](../../../crates/core/src/configured_runtime.rs)：task_input | 本轮使用的任务、材料、Skill 与规范输入 |

目录查询与发布分开。用户打开列表不应隐式同步配置或修复数据；后台维护和明确修改入口负责相应提交。

## 为什么不能只读最新定义

工作项、Standard、会话及执行需要保留当时选择的源修订。[standards.rs](../../../crates/core/src/standards.rs) 与 [project_standards.rs](../../../crates/core/src/project_standards.rs) 处理规范选择及比较；[frozen_views.rs](../../../crates/core/src/frozen_views.rs) 支持原运行视图。

修改默认值只改变后续选择，不重写既有 Agent 的 Capability，也不替换已经启动的会话。恢复使用原引用关系；删除定义前需要检查它是否仍被当前或历史执行引用。

## 出厂资源与模板

[templates.rs](../../../crates/core/src/templates.rs)、[builtin_defaults.rs](../../../crates/core/src/builtin_defaults.rs) 处理模板和默认安装的应用语义；实际文件读取、嵌入和源编码由 adapters 完成。发布集合来自 configurations/manifest.json，不扫描目录猜测最新版本。

目录缓存的版本令牌帮助判断显示是否过时，不等于允许执行或保存的凭证。修改命令仍要使用自己的原版本条件。

## 修改与验证

新增配置类型或引用时同时检查 domain、Schema、发布资源、编译器和界面编辑器。若改变任务正文，检查对应运行身份摘要与恢复语义，避免旧执行悄悄采用新输入。

测试入口：[configuration_catalog.rs](../../../crates/adapters/tests/configuration_catalog.rs)、[configuration_changes.rs](../../../crates/adapters/tests/configuration_changes.rs)、[configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs)、[standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs)。
