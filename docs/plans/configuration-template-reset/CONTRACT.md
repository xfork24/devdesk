# 默认模板目录合同

## 资源与数据库

`resources/configuration/manifest.json` 是发布默认的入口，显式列出各类型文件。MCP 与 HTTP 当前为空清单；用户接入的服务属于数据库工作副本。`defaults.json` 声明项目预填引用，所有引用在生成实体时解析为确切数据库 ID。

模板文件包含 `template_key`、`storage`、`record`、`text_files`、`references`、`initial_only`。`storage` 只允许 governance、definitions、harnesses、sandboxes。`text_files` 用 JSON Pointer 声明正文位置；`references` 用 JSON Pointer 声明目标模板 key 及 revision/identity 类型。只替换这些已声明的位置，不递归替换正文或工具参数字符串。

构建脚本离线验证 manifest/entry Schema、实体 Schema、文件存在性、路径、重复项、类型分组与依赖。全部 JSON/Markdown 嵌入二进制；实体化时继续使用领域校验验证 Agent、Capability、Workflow 子流程、Delivery、工具、接入和权限。Harness 配方摘要必须对应内嵌的固定资产。

首次空库初始化保留 main 当前内置修订 ID。重置生成新的目录代次、定义身份、修订 ID，以及同代次的选择器投影；模板 key 在目录代次之间保持稳定。当前版本按同一身份的最高已发布修订解释，应用发布默认仅含当前有效版本。

保持 main 当前的 Permission Profile、精确目标绑定及 assignment 合同，不恢复 v1 Permission 表或兼容解析。Sandbox、Harness 和 Permission Profile 在重置时创建新修订；恢复后的 Agent 使用同代次的 Sandbox 和精确 Profile 绑定，完整绑定与默认图在同一事务提交。旧 Agent、锁和绑定保留原值。

## 目录元数据

采用既有 `governance_records` 账本承载三个类型的目录元数据，复用当前事务、命令幂等和不可变校验，避免增加一套并行序列化机制。全局索引迁移为 011，项目索引迁移为 042，从 main 的 10/41 基线向前迁移，新增唯一索引及代次索引。

| kind | 存储和合同 |
| --- | --- |
| `template_catalog_state` | 全局唯一可变记录，固定 ID 为 `template-catalog-state`。data 包含 generation、initial_generation、bundle_version、bundle_hash、defaults。同步到项目的状态只作为全局状态副本。 |
| `template_catalog_member` | 不可变记录，每个定义修订或草稿一条，definition_id 指向其修订 ID。data 包含 kind、generation、template_key、source_hash、origin。Workflow 草稿的成员 kind 归为 workflow。 |
| `template_reset_receipt` | 全局不可变回执，记录 command_id、前后代次、资源版本/摘要和各类数量。 |

代次从 1 开始。main 当前结构中没有成员元数据的既有定义固定归属 initial_generation=1；延迟打开的项目也采用这个固定值。重置后的代次不能用于补登记旧数据。

当前目录查询先按代次筛选，再应用 Scope、可见范围、删除状态和修订历史。`include_history` 只包含当前代次的修订；`include_archived` 明确请求重置前记录。按确切 ID 的工程、会话和历史查询继续解析完整历史，不应用当前目录筛选。归档不等于原有的撤销或禁用。

成员来源以源模板及包含名称、Scope 和正文的摘要追踪。数据库修订继承模板来源，内容不同时标为 modified；用户新建条目标为 custom，无法证实来源的既有记录标为 existing。实体内容摘要始终包含实际引用 ID，不使用模板符号摘要代替执行摘要。

## 写入与重置

Workspace 使用目录协调锁串行化定义变更、目录查询、项目打开和重置。普通编辑在所属数据库中追加发布修订或修改可变草稿，并在同一事务登记成员。原目录记录和源码资源不会被覆盖。

定义写入 RPC 使用顶层 `expected_catalog_generation`。代次 1 接受原客户端未携带该字段的请求；第一次重置后要求明确代次。按相同 command_id 重试先查命令账本，再进行代次校验。旧修订不能通过携带新代次重新编辑。

`configuration.templates.reset` 的 payload 为：

```json
{
  "expected_catalog_generation": 1,
  "catalog_fingerprint": "预览返回的64位摘要",
  "bundle_hash": "预览返回的64位默认资源摘要"
}
```

预览覆盖全局和全部项目的当前定义、草稿、修订、删除及停用状态，不受 UI 分页限制。提交时在协调锁下重算并比较摘要。改变了定义或默认资源时返回 `catalog_changed`，需要重新预览。

在一个全局 SQLite 事务中生成完整默认图、选择器投影、权限绑定、成员、回执和幂等结果，再切换目录代次。失败时整体回滚。项目的旧定义通过全局代次退出当前目录，后续追加同步新默认，不依赖逐项目写入来完成重置。

当全局及项目当前目录已与同一默认来源一致时返回 `changed: false`，不产生额外代次。运行实例、AgentInstance、交付事实和会话不参与模板目录比较。

普通重启不再发布默认修订。重置过的数据集不自动重载旧的磁盘权限预设；用户仍可在设置中显式重载。恢复后的 Agent 在重置提交后即可使用其完整静态绑定。

## 查询与界面

`query.template_catalog` 提供当前代次、应用默认版本、当前来源版本和按类型统计。`query.template_reset_preview` 提供当前/恢复数量、自建或修改数量、草稿数量、固定旧绑定的项目摘要和目录指纹。

配置目录查询返回 `catalog_generation` 及独立的 `template_sources` 来源映射，保持 `ConfigurationRecord` 序列化合同不变。分页客户端不能组合不同代次的结果。保存后的项目查询附带 pinned_records 与 uses_archived_templates，使项目页能够显示已保存的旧引用。

设置按钮为“重置模板定义”。确认框明确作用于全数据集，列出归档范围，保留项目绑定、运行历史和凭据。取消不写入；确认期间防止重入；冲突可重新预览；失败保留相同幂等键重试。

UI 的目录状态统一追踪代次。配置页和项目页按代次重新挂载，草稿缓存 key 包含代次，旧草稿不会自动覆盖恢复后的默认。重置成功后刷新目录、引用选择器和新项目预填配置；已有项目继续显示其固定的旧定义来源。

已保存的运行偏好保留精确 Model/Harness 引用。设置页只补入这些已选历史记录，并标明“重置前固定版本”；新选择使用当前目录。仅含 Harness/Sandbox 的非空数据库也按已有当前结构登记，不能当作空库覆盖。main 已删除的旧权限、旧 Workflow 和正文升级链不会因重置而返回。
