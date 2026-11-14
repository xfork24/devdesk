# 模型服务商与 Harness

DevDesk 当前发布 OpenAI/Codex 及 DeepSeek Harness（DSH）的模型配置。Codex 使用官方 OAuth 与 app-server；DSH 的出厂服务商包括 DeepSeek、智谱国内编程订阅、MiniMax 国内订阅和本机 Ollama。服务商、端点和容量目录来自 `model_provider` / `model_catalog` 源，Harness 通过精确引用采用它们；Model 的服务商与 Harness 驱动分开选择，完整 Model、Harness、Capability 和 Agent 组合决定实际执行配置。

## 配置步骤

1. 在设置的 DSH 模型服务商中选择对应服务。DeepSeek 输入官方 API key，智谱输入国内编程套餐 key，MiniMax 输入国内订阅 Key；各自独立保存在本机系统凭据库。Ollama 本地服务免密，无需配置 key。
2. 在基础组件创建 Model，选择服务商、准确模型 ID 和请求输出预算。读取模型候选只填写草稿，保存后才产生不可变修订。未知模型需要填写上下文容量与最大输出容量，容量声明不会改变服务端设置。
3. 选择内建的“DeepSeek Harness 多模型服务商”，或创建明确支持所选服务商的固定 Harness 修订。创建 Capability 选择 Model、Harness 和原生工具，再由 Agent 使用该 Capability。
4. 在角色讨论中使用已保存 Agent 发起普通会话，或通过工作流执行实际任务，核对所选模型、工具调用及真实回执。Agent 页面只编辑 Role 与 Capability 的组合，不提供运行诊断入口。

设置中检查凭据访问仅验证系统凭据库读取，模型候选仅表示目录结果。实际讨论或任务会消耗所选模型的使用额度，应按会话终态和工具回执判断执行结果。

## 服务商与端点

| Model 服务商 | 连接方式 | 配置要求 |
|---|---|---|
| `deepseek` | DSH 原生 `deepseek-official` 路由 | 保留既有 DeepSeek 官方 key 及预算规则 |
| `zai-coding-cn` | OpenAI Chat Completions，`https://open.bigmodel.cn/api/coding/paas/v4` | 使用国内编程订阅 key，不回退普通按量端点 |
| `minimax-cn` | Anthropic Messages，`https://api.minimax.cn/anthropic` | 使用国内订阅 Key；端点来自所采用的服务商源 |
| `ollama` | 本机服务的 OpenAI Chat Completions，默认 `http://127.0.0.1:11434/v1` | 填写服务端实际可用的模型及容量；客户端仅使用非秘密占位值 |

上表是当前出厂源的值；云订阅的端点由所选服务商源固定，Model 不能覆盖为任意网关。修改服务商须发布并明确采用新源修订，实际协议仍需已安装驱动支持。不同账号的密钥、可用模型和额度分别配置；保存定义不证明远端可用。

## Ollama 本机与容器地址

本地模型及本机 Ollama 已登录账号可用的订阅云模型，都通过本机服务访问。DevDesk 不复制 Ollama 账号文件，不自动登录、安装或下载模型，不直接调用 Ollama 云 API。

宿主 loopback 地址会解析为 worker 的 `host.docker.internal`，连接快照同时保存配置地址和实际地址。Docker Desktop 必须实际允许访问该监听；Linux Docker Engine 会配置 `host-gateway`，但它不能开放只监听宿主 loopback 的服务。Linux 需先配置 bridge 可达监听或已有代理，也可显式填写可达服务地址。远程 Docker daemon 需要它自身可访问的明确地址，不能假设网关就是 DevDesk 宿主。

地址只接受合法 HTTP/HTTPS API 根路径，默认或省略路径使用 `/v1`；禁止嵌入凭据、query 和 fragment。Sandbox 禁网会阻塞模型连接；独立测试容器不会获得模型 key 或新增宿主地址映射。

## macOS 凭据助手升级

当前 macOS 凭据访问统一要求协议 2 的固定签名助手，包含 DeepSeek、智谱、MiniMax 和接入工具凭据；安装目录名 `v1` 不表示助手协议版本。源码安装使用 `scripts/platform/macos/install-macos-credential-helper.sh --update`；含对应助手的独立运行包提供显式更新入口：

```bash
./devdesk-host --update-credential-helper
```

升级验证 Host 与助手的签名和内容，不读取 Keychain。之后首次使用新助手如需系统授权，在设置中选择对应服务商，再执行“授权已保存的钥匙串凭据”。普通构建的 `--ensure` 复用通过当前协议、内容和同证书签名检查的已安装助手；不兼容安装须显式修复。打包仅构建随包助手，不替换本机共享安装。Linux 使用 Secret Service，Ollama 不要求系统凭据库。

## 冻结与验证范围

新 key 产生新的不透明引用，已有 Session 和技术重试保留原引用；旧引用不可读时明确失败，不自动换 key。Model 或 Harness 新修订不修改既有 Capability、Agent、Feature 或 Session。更新配置后通过新的完整组合及后继 Session 使用新连接。

当前出厂 DSH 资源标签为 `0.2.2-rc.1`，发行文件与构建 pin 保存在运行资源源中。模型容量目录通过配方摘要绑定 Harness，执行检查冻结连接和生成 profile 的内容摘要，不把版本字符串作为兼容门槛。未知模型需要明确容量声明；不读取用户全局 DSH 设置。原会话保留实际安装、连接与适配器身份，不能在旧运行上替换代码或密钥。

`models.list` 的目录读取、运行资源协议探针和真实推理是不同检查。DSH 路由的本地流式夹具不证明某个真实订阅可用；Ollama 宿主目录读取不证明容器连通或推理成功。当前验收入口见 [真实工程接入](real-engineering.md)，历史结果按对应报告的版本和范围解释。

实现依据：[服务商领域规则](../crates/domain/src/model_provider.rs)、[出厂服务商](../resources/configuration/model-providers/)、[目录查询](../crates/host/src/model_providers.rs)、[DSH 连接与 profile](../crates/adapters/src/model_providers.rs)、[凭据助手检查](../crates/adapters/src/credentials_macos.rs)。
