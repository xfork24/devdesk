# JSON Schema 契约

这些文件使用 JSON Schema Draft 2020-12，随运行包一起发布。引用其他文件的 Schema 需要按本目录解析相对 `$ref`。

| 文件 | 对应实现与用途 |
|---|---|
| `rpc.schema.json` | Host JSON-RPC 请求封装，以及 domain/core 的共享类型定义。命令使用 `Command`，运行时登记、准备和设置等接口使用各自参数；`$defs` 同时描述冻结请求、运行实例和配置修订等数据。 |
| `manifest.schema.json` | core 输出的不可变文件：Delivery 清单、带 `schema_version` 的工程 Snapshot 清单，以及直接序列化的 Project/会话 Snapshot。嵌套领域对象复用 RPC 定义。 |
| `permission.schema.json` | `Permission` 修订及权限继承、覆盖定义；保留自包含版本，供预设的独立校验使用。回归要求其与 RPC 中对应定义一致。 |
| `permission-backend.schema.json` | `permissions/backends/manifest.json`：后端版本、平台、嵌套沙箱、AppArmor 和资产摘要。资产内容的 SHA-256 另由权限检查验证。 |
| `model.schema.json`、`role.schema.json`、`tool.schema.json`、`agent-composition.schema.json`、`workflow.schema.json` | 对 RPC 共享配置定义的引用入口，避免重复维护。 |
| `workflow-action.schema.json` | Workflow 动作标识与契约版本；动作集合由领域编译器的回归核对。完整步骤结构由 `WorkflowStep` 定义。 |

Schema 描述数据形状和可独立检查的约束。跨对象引用、职责兼容、工作流依赖、当前修订、执行来源和权限可执行性仍由 Rust 领域规则及适配器校验；通过 Schema 不代表通过业务门禁。`InteractionAnswer` 描述可提交给原生供应商的回答，迁移保留的 `legacy` 文本回答不能用于新的交互。

`Command.payload` 省略时为 `null`；无参数查询可省略 `params`，Host 将其视为 `{}`。Serde 默认字段可以省略，普通 `Option` 字段接受省略或 `null`；权限 Patch 和 Binding 中使用 `present` 解码的字段仍明确拒绝 `null`。`BindingLock.harness/sandbox` 在准备原生运行环境前可以为空，具体安装保存在 `RuntimeBinding`。manifest 中的 `simulated` 来源和 `simulation` 模式仅兼容不可变历史数据。

修改 Rust 数据结构或产物格式时同步本目录，并执行：

```bash
cargo test -p devdesk-adapters --test schema_contracts --test workflows --locked
cargo test --workspace --locked
```

`schema_contracts` 离线编译所有 Schema 和 `$defs`，验证相对引用、当前内置配置、Serde 默认值与序列化输出、权限预设、历史产物和关键非法输入。工作流回归校验实际生成的交付与快照文件。JSON Schema 验证器仅作为 Rust 测试依赖，不需要额外安装 Python 包或访问网络。
