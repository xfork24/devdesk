# DeepSeek Harness 模型服务商

DevDesk 通过同一个 DeepSeek Harness 接入 DeepSeek、智谱国内编程订阅、MiniMax 国内订阅和本机 Ollama。Model 的服务商与 Harness 类型分开选择；新增服务商仍使用 `dsh`，完整 Model、Harness、Capability 和 Agent 组合决定实际执行配置。

## 配置步骤

1. 在设置的 DSH 模型服务商中选择对应服务。DeepSeek 输入官方 API key，智谱输入国内编程套餐 key，MiniMax 输入国内订阅 Key；各自独立保存在本机系统凭据库。Ollama 本地服务免密，无需配置 key。
2. 在基础组件创建 Model，选择服务商、准确模型 ID 和请求输出预算。读取模型候选只填写草稿，保存后才产生不可变修订。未知模型需要填写上下文容量与最大输出容量，容量声明不会改变服务端设置。
3. 选择内建的“DeepSeek Harness 多模型服务商”，或创建明确支持所选服务商的固定 Harness 修订。创建 Capability 选择 Model、Harness 和原生工具，再由 Agent 使用该 Capability。
4. 在已打开项目中选择已保存 Agent，可以检查 DSH 容器连接，或提交实际模型与工具验证。前者查询模型元数据，不推理；后者产生普通讨论 Session/Turn，需在讨论页核对完成状态与工具结果。

设置中检查凭据访问仅验证系统凭据库读取，模型候选仅表示目录结果，容器连接检查也不证明推理和工具调用成功。实际验证消耗所选模型的使用额度，提交成功不代表模型验证完成。

## 服务商与端点

| Model 服务商 | 连接方式 | 配置要求 |
|---|---|---|
| `deepseek` | DSH 原生 `deepseek-official` 路由 | 保留既有 DeepSeek 官方 key 及预算规则 |
| `zai-coding-cn` | OpenAI Chat Completions，`https://open.bigmodel.cn/api/coding/paas/v4` | 使用国内编程订阅 key，不回退普通按量端点 |
| `minimax-cn` | Anthropic Messages，`https://api.minimax.cn/anthropic` | 使用国内订阅 Key；显式覆盖固定 DSH 目录中的旧默认域名 |
| `ollama` | 本机服务的 OpenAI Chat Completions，默认 `http://127.0.0.1:11434/v1` | 填写服务端实际可用的模型及容量；客户端仅使用非秘密占位值 |

云服务商端点固定，不支持通过 Model 覆盖到任意网关。不同订阅的 key、支持模型和额度按供应商账号配置处理。端点依据 [智谱官方说明](https://docs.bigmodel.cn/cn/coding-plan/quick-start)、[MiniMax 官方说明](https://platform.minimax.cn/docs/m-plan/other-tools)和 [Ollama 官方兼容接口](https://docs.ollama.com/api/openai-compatibility)。

## Ollama 本机与容器地址

本地模型及本机 Ollama 已登录账号可用的订阅云模型，都通过本机服务访问。DevDesk 不复制 Ollama 账号文件，不自动登录、安装或下载模型，不直接调用 Ollama 云 API。

宿主 loopback 地址会解析为 worker 的 `host.docker.internal`，连接快照同时保存配置地址和实际地址。Docker Desktop 必须实际允许访问该监听；Linux Docker Engine 会配置 `host-gateway`，但它不能开放只监听宿主 loopback 的服务。Linux 需先配置 bridge 可达监听或已有代理，也可显式填写可达服务地址。远程 Docker daemon 需要它自身可访问的明确地址，不能假设网关就是 DevDesk 宿主。

地址只接受合法 HTTP/HTTPS API 根路径，默认或省略路径使用 `/v1`；禁止嵌入凭据、query 和 fragment。Sandbox 禁网会阻塞模型连接；独立测试容器不会获得模型 key 或新增宿主地址映射。

## macOS 凭据助手升级

DeepSeek 继续兼容旧助手及旧 vault 引用。智谱和 MiniMax 需要支持多服务商账号格式的助手。源码安装使用 `scripts/platform/macos/install-macos-credential-helper.sh --update`；含新版助手的独立运行包使用以下命令显式升级：

```bash
./devdesk-host --update-credential-helper
```

升级验证 Host 与助手的签名和内容，不读取 Keychain。之后首次使用新助手如需系统授权，在设置中选择对应服务商，再执行“授权已保存的钥匙串凭据”。普通 Host 构建及打包不会自动替换本机已授权助手。Linux 继续使用 Secret Service，Ollama 不要求任何系统凭据库。

## 历史与验证范围

新 key 产生新的不透明引用，已有 Session 和技术重试保留原引用；旧引用不可读时明确失败，不自动换 key。Model 或 Harness 新修订不修改既有 Capability、Agent、Feature 或 Session。更新配置后通过新的完整组合及后继 Session 使用新连接。

当前服务商目录和路由适配以 DSH `0.2.2-rc.1` 为基线。模型目录嵌入运行包，未知模型可通过明确容量声明使用；不读取用户全局 DSH 设置。变更适配器后，原会话按既有版本校验要求创建后继，不能在旧运行上偷偷替换代码。

本轮真实模型验收仅使用 Codex。DSH 的三条新路由以固定镜像、禁外网容器和模拟流式服务验证；Ollama 容器目录检查使用本地模拟目录，不代表已验证某个真实订阅。阶段结果见 [实施记录](plans/dsh-model-providers/EXECUTION.md)。
