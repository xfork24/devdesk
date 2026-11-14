# 模型路由、外部工具与 OAuth

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

模型目录发现、冻结模型连接和实际推理是不同能力。HTTP/MCP 工具与 OAuth 使用持久化操作和凭据引用，离线 Schema 校验不会发起外部请求。

## 本页文件

- [src/model_providers.rs](#file-src-model-providers-rs)：冻结模型连接与只读模型目录
- [src/integrations.rs](#file-src-integrations-rs)：离线合同校验和持久化 HTTP/MCP 执行
- [src/integration_oauth.rs](#file-src-integration-oauth-rs)：PKCE、固定账户和未知交换结果

<a id="file-src-model-providers-rs"></a>

## src/model_providers.rs：冻结模型连接与只读模型目录

源码：[crates/adapters/src/model_providers.rs](../../../crates/adapters/src/model_providers.rs)。

binding_model_connection 从冻结 Session 解析模型连接，核对 provider、连接快照和 profile 摘要，防止当前设置覆盖旧会话。dsh_model_patch 按确切路由生成 DSH 配置：原生 DeepSeek 路径与 pi-ai 云/本地连接各自设置，并关闭冲突的默认 provider。

生成配置使用凭据环境引用，不把密钥值写进可展示的 profile。Ollama 目录发现通过受限本机 HTTP 请求获取元数据，禁止代理/重定向并限制输出，返回 container_verified=false、inference_verified=false；宿主能列模型不等于容器能调用。

DeepSeek 账户/模型查询使用固定地址和私有输入传递密钥，也不执行推理。模型目录、实际网络可达和付费推理应分别验证。

代码定位：[`binding_model_connection`](../../../crates/adapters/src/model_providers.rs#L6)、[`dsh_model_patch`](../../../crates/adapters/src/model_providers.rs#L37)、[`discover_ollama_models`](../../../crates/adapters/src/model_providers.rs#L56)、[`discover_deepseek_models`](../../../crates/adapters/src/model_providers.rs#L100)。

关联阅读：[domain/src/model_provider.rs](../devdesk-domain/runtime-and-permissions.md#file-src-model-provider-rs)、[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)、[host/src/model_providers.rs](../devdesk-host/rpc-and-configuration.md#file-src-model-providers-rs)、[adapters/tests/model_providers.rs](testing.md#file-tests-model-providers-rs)。

<a id="file-src-integrations-rs"></a>

## src/integrations.rs：离线合同校验和持久化 HTTP/MCP 执行

源码：[crates/adapters/src/integrations.rs](../../../crates/adapters/src/integrations.rs)。

NativeTools 实现 ToolExecutor。离线部分验证 JSON Schema、参数、OpenAPI 和 Skill 包，限制深度、展开量和数据规模，不在校验时抓取外部引用。Schema 只支持明确集合；关键字检查发生在 Schema 位置，不把 examples 或普通属性名误判为执行规则。

执行部分以原 operation UUID 和冻结请求摘要建立私有作业，记录启动、poll、取消与结果。凭据在最后授权边界注入，目标 origin、网络权限、私网地址及重定向都需检查；HTTP/MCP 不接受任意宿主命令。OAuth 请求使用受管容器路径。

OpenAPI 3.1 JSON/YAML 导入保留安全方案的替代关系，不把 AND 授权静默删成可执行的弱规则。Skill ZIP 拒绝非法路径、符号链接、非 UTF-8 和超限内容，Git 来源固定 40 位 commit；导入不运行安装脚本。

失败结果保存完整 Error，包括可信 execution_failure 分类，而非仅错误文本。core 将它与真实工具回执、消费事实和业务拒绝一起判断，不能从供应商返回的文字推断安全重试。

代码定位：[`NativeTools`](../../../crates/adapters/src/integrations.rs#L14)、[`validate_schema_tree`](../../../crates/adapters/src/integrations.rs#L22)、[`schema_budget`](../../../crates/adapters/src/integrations.rs#L99)、[`data_budget`](../../../crates/adapters/src/integrations.rs#L175)。

关联阅读：[core/src/integrations.rs](../devdesk-core/integrations-and-repositories.md#file-src-integrations-rs)、[adapters/src/integration_oauth.rs](external-integrations.md#file-src-integration-oauth-rs)、[adapters/tests/tool_integrations.rs](testing.md#file-tests-tool-integrations-rs)、[core/src/workflow_failure.rs](../devdesk-core/workflow-execution.md#file-src-workflow-failure-rs)。

<a id="file-src-integration-oauth-rs"></a>

## src/integration_oauth.rs：PKCE、固定账户和未知交换结果

源码：[crates/adapters/src/integration_oauth.rs](../../../crates/adapters/src/integration_oauth.rs)。

OAuthFlow 保存授权操作、PKCE verifier/state、callback 与来源绑定。只实现 public client 的 authorization code + S256 路径，避免把 client secret、implicit 等未支持方案当作可用授权。

IntegrationSecrets 隔离秘密的创建/读取/更新；持久化状态保存引用。callback 必须匹配原 state、origin、工具来源和账户，刷新保持同一账户与 scope，撤销和并发 grant 也有独立边界。

token exchange 或 refresh 的结果未知时不能盲目重发：远端可能已经消费一次性 code 或旋转 token。恢复保留原操作与账户事实，向上层报告需核对的状态，而不是删掉失败记录重新登录。

代码定位：[`IntegrationSecrets`](../../../crates/adapters/src/integration_oauth.rs#L6)、[`OAuthFlow`](../../../crates/adapters/src/integration_oauth.rs#L23)、[`create`](../../../crates/adapters/src/integration_oauth.rs#L7)、[`read`](../../../crates/adapters/src/integration_oauth.rs#L8)、[`write`](../../../crates/adapters/src/integration_oauth.rs#L9)。

关联阅读：[adapters/src/credentials.rs](credentials-and-permissions.md#file-src-credentials-rs)、[host/src/integrations.rs](../devdesk-host/rpc-and-configuration.md#file-src-integrations-rs)、[adapters/tests/tool_integrations.rs](testing.md#file-tests-tool-integrations-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
