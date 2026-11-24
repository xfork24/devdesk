# Harness、模型与外部接入

本页列出当前代码和出厂配置支持的范围。服务商端点来自所采用的源修订；账号额度、网络可达性和模型效果仍需实际验证。

## Harness 与模型服务

| 项目 | 当前方式 | 注意事项 |
|---|---|---|
| Codex | 官方 OAuth、app-server | 当前声明根工作流编排能力 |
| DSH | 受管 SDK 会话与工具桥 | 支持普通会话和步骤工具，不据此认定可担任根经理 |
| DeepSeek | DSH 原生路由 | 独立 API key 和模型预算 |
| 智谱国内编程订阅 | OpenAI Chat Completions 兼容端点 | 使用对应订阅配置，不能默换按量端点 |
| MiniMax 国内订阅 | Anthropic Messages 兼容端点 | 账号、密钥与目录独立 |
| Ollama | 本机服务的 OpenAI 兼容 API | 默认免密，需指定实际存在的模型与容量 |

具体端点见[出厂服务商源](../../../plugins/core/model-providers)，驱动转换见 [model_providers.rs](../../../crates/adapters/src/model_providers.rs)。未知模型需要明确容量声明；填写容量不会修改服务端限制。

### 本机 Ollama 与容器

设置保存服务根地址，Model 修订保存实际采用的地址。更改默认地址后，应保存并采用新 Model 及相关组合；已有会话不自动改连接。

宿主 loopback 可在 worker 连接中转换为 `host.docker.internal`。Linux 的 host-gateway 映射只解决寻址，不会使仅监听 loopback 的服务对 bridge 开放；需要服务监听或代理本身可达。远程 Docker daemon 的网关也不能假定是 Host 所在机器。

DevDesk 不复制 Ollama 账号文件，不自动下载模型或代替用户登录。模型目录读取成功，不代表容器已经能够推理。Sandbox 禁网时模型连接仍会受阻。

## HTTP、OpenAPI 与 MCP

外部接入分成来源定义、项目绑定和工具定义。先发现或导入目录，再选择项目账户和权限，最后由 Capability 采用具体工具。全局来源不会自动给所有项目授予账户权限。

| 接入 | 支持内容 | 明确边界 |
|---|---|---|
| HTTP | 对象输入 Schema、path/query/header/JSON body 映射、结构化/文本/文件结果 | 不运行任意转换脚本；认证 Header 由绑定注入 |
| OpenAPI | 3.1.x 子集、已展开 server、支持的认证选择 | 不支持复杂序列化、远程 Schema 引用和任意认证组合 |
| MCP | 固定版本的 stdio / Streamable HTTP 工具发现与调用 | 目录变化须重新发现/发布；未开放服务端采样、询问、roots/resources/prompts 等扩展 |
| OAuth | 公共客户端授权码 + PKCE S256、刷新和撤销 | 账户或 scope 变化需重新授权；不盲重放可能已消费的授权凭据 |

当前 worker 处理 MCP `2025-11-25` 和 `2026-07-28` 对应的协议分支，采用哪一版由来源配置确定。完整限制以[接入领域类型](../../../crates/domain/src/integrations.rs)及[实际 worker](../../../crates/adapters/runtime/tool-worker.mjs)为准。

HTTP/MCP 在受管 Docker 工具进程中执行。目标地址、解析后的私网地址和重定向需满足绑定规则。本机/内网来源要明确允许；不能通过参数覆盖认证或扩大可访问范围。

## Skill

Skill 支持 Markdown、静态 UTF-8 ZIP 包，或公开 HTTPS Git 的确切 commit。导入保留材料、来源和工具依赖；包内脚本不会自动安装或运行。所选材料在 `/skills` 只读挂载，未选择的原生 Skill 不会因为存在于仓库就自动启用。

Skill 提供方法和资料，不授予工具权限。所需工具仍须经 Capability、项目绑定和运行权限允许。

## 失败与验证

发现、导入、OAuth 和工具写入都可能发生外部副作用。远端结果未知时应保留原操作并核对，不把超时解释成从未执行。人工核对结果单独记录，不伪装成远端成功响应。

Schema 校验、Docker 协议夹具、系统凭据授权和真实服务调用分别验证不同边界。代码与测试见[外部接入手册](../../crates/devdesk-adapters/external-integrations.md)。
