# HTTP、MCP、OAuth 与 Skill 接入

接入代码将用户采用的来源和工具定义转换成受管调用。Rust 层负责准备、权限、凭据、进程及结果管理；工具进程负责具体 HTTP/MCP 协议。

## 运行路径

[integrations.rs](../../../crates/adapters/src/integrations.rs) 的 NativeTools 实现 core 的 ToolExecutor。输入是已准备的 ToolInvocation，调用前检查 Schema、参数、绑定、权限和运行资源；然后启动 Docker 中的 worker，并在后续 poll/inspect 中取得结果。

实际 worker 位于 [runtime/tool-worker.mjs](../../../crates/adapters/runtime/tool-worker.mjs)。这是适配器运行资产，与业务配置目录分开；新增协议逻辑应在这里及对应 Rust 准备/校验层实现，不能放进配置 JSON 作为任意脚本执行。

## 协议与认证

| 部分 | 执行责任 |
|---|---|
| HTTP/OpenAPI | 固定来源、输入映射、认证 Header 注入、结果结构及文件完整性 |
| MCP | 固定协议版本、目录发现、请求 ID、工具调用及未支持交互的拒绝 |
| OAuth | [integration_oauth.rs](../../../crates/adapters/src/integration_oauth.rs) 处理 PKCE、交换、刷新、撤销与原操作核对 |
| Skill | 校验 Markdown/ZIP/固定 Git commit 材料，保留来源与依赖，准备只读挂载 |

外部地址、私网解析和重定向都要满足绑定规则。工具参数不能覆盖认证 Header。发现目录变化时需要重新发布，不能在原工具身份下静默采用新的远端描述。

## 结果与失败

调用结果包括原调用身份、结构化内容、可能的文件和远端效果信息。写请求超时不能直接视为未执行；原操作的 inspect/reconcile 路径用于保留并核对未知结果。

Schema 错误应返回定位和有界说明，避免把敏感参数正文写入错误信息。原始调用、平台结果和人类核对的来源要保持可区分。

## 修改与验证

新增协议或认证方式时同步检查 domain 类型、Schema、导入/发现、运行 worker 和权限限制。只实现工具列表返回，不代表实际调用、取消和恢复已经完成。

测试入口：[tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs)、[integration_operations.rs](../../../crates/adapters/tests/integration_operations.rs)，支持范围见[接入参考](../../design/reference/integrations.md)。Docker 夹具验证协议行为，真实账户和系统凭据授权另行验证。
