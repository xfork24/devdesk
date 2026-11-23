# Project Operations

## 读取项目状态

用 `project.overview` 查看项目范围和记录类别。通过 `project.records.list`、`project.record.read`、`project.artifact.read` 按对象和版本读取所需材料。核对 `total`、`complete`、`next_offset` 和摘要；来源变化时重新读取相应页。使用 `project.resources` 取得与安装件、驱动和权限资产绑定的探测记录。

## 管理配置与流程变更

用 `configuration.catalog`、`configuration.read` 和 `configuration.usage` 核对不可变源、历史和消费者。通过 `configuration.change.preview` 预演，再用同一变更集和 `preview_hash` 调用 `configuration.change.apply`。发布源修订、更新未来选择及改变运行中计划分别走各自入口。待修改的源需明确类型、身份、修订、名称、作用域和精确依赖。

项目级授权可创建项目后继并指定消费者。共享写入需要对应授权，跨项目转移用 `configuration.transfer.stage` 检查候选，再执行应用或取消；中断后查询原操作。停用、换绑和移除需检查实际使用方。会话换绑或移除后依据后继组合的授权继续，运行中的变更需要现有根控制权。

## 处理冲突与未知结果

版本冲突时保留原输入，重新查清影响范围。遇到结果未知，先核对原请求、执行回执和实际副作用。状态说明包含已完成事项、等待项和待决定范围。当前任务结束后交还会话，等新的工作请求。
