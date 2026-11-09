# DeepSeek Harness 多模型服务商接入计划

日期：2026-11-09。状态：待用户审核；本轮只编写计划，审核通过后实施。范围：通过 DevDesk 现有 DeepSeek Harness 接入智谱国内编程订阅、MiniMax 国内订阅和本机 Ollama，兼容现有 DeepSeek 官方 API key。

## 一 目标与主要决定

继续使用 `dsh --profile sdk`、DSH JSON-RPC、现有 Session、Run、权限系统和 Docker 执行方式。新增服务商改变 DSH 内部模型路由、连接配置和认证来源。

1. `HarnessProvider` 继续只有 `codex`、`dsh`；智谱、MiniMax、Ollama 是 Model 服务商，不增加同名 Harness 类型。
2. 优先复用 DSH 自带的 `@deepseek-ai/dsh-llm-pi-ai` 模型适配插件。它属于 DSH 内部传输层；DevDesk 不引入 Pi CLI、其他 Agent/Harness 框架，也不在 Rust 中直接实现厂商推理协议。
3. 复用现有 Model 不可变修订保存服务商、模型和连接配置；首版不新增通用连接资源库。凭据按数据集和模型服务商分别管理，每个服务商配置一份当前凭据。
4. 新执行冻结服务商、DSH 路由、实际端点、模型参数和凭据引用。修改设置或发布新 Model 不改变旧 Session、Run 或重试的绑定。

“本地 Ollama 订阅”在本计划中解释为访问用户本机已运行的 Ollama 服务，既可使用本地模型，也可使用该服务已登录账号后提供的订阅云模型。登录由本机官方应用或 CLI 完成；首版不直接接入 `ollama.com` 云 API，不把本机账号文件复制到容器。[Ollama 官方兼容接口](https://docs.ollama.com/api/openai-compatibility)说明本地服务可以代理已登录账号的云模型请求。

## 二 当前实现与缺口

| 入口 | 当前行为 | 修改方向 |
|---|---|---|
| `domain/runtime.rs`、`domain/governance.rs` | 已区分 Harness 和 Model；Model 已有 `provider`、`endpoint`、`parameters` | 保持职责，增加服务商支持信息和必要的模型容量字段 |
| `core/governance.rs`、`host/governance.rs` | 只允许 Codex/OpenAI 与 DSH/DeepSeek，统一拒绝 endpoint | 按确切运行版本、服务商及参数能力校验 |
| `host/workspace.rs` | DSH 统一读取 DeepSeek 凭据，缺省模型写死为 DeepSeek | 从完整 Model/Capability 组合解析连接，保留旧路径的 DeepSeek 语义 |
| `adapters/catalog.rs`、`credentials.rs` | 只有 `credentials.dsh` 和 DeepSeek vault 账号格式 | 分开保存服务商凭据引用，兼容旧引用 |
| `credential-helper/main.rs` | macOS 助手仅允许 DeepSeek 账号前缀 | 增加有限账号命名空间，核对助手升级和签名流程 |
| `adapters/harness.rs` | 固定写入 `DEEPSEEK_API_KEY`，initialize 固定 `deepseek-official` | 动态生成路由 patch、环境变量和初始化参数 |
| `adapters/docker.rs` | 没有 Ollama 宿主访问配置；测试容器检查既有凭据变量 | 增加受控宿主映射，扩充凭据边界检查 |
| `RuntimeSettings.tsx`、`AgentCatalog.tsx` | 设置仅有 DeepSeek key；模型表单仅有 OpenAI/DeepSeek | 增加服务商设置、模型配置和运行检查 |
| `containers/dsh/Dockerfile`、`runtime_preparation.rs` | 固定 npm `@deepseek-ai/dsh@0.2.2-rc.1` | 核验实际发行包插件能力，必要时形成新固定构建配方 |

本地参考源码为 `/Users/hengliu/workspace/deepseek-harness`，读取时 HEAD 为 `5aeed699a011563980a9597f9fc8bc3ee2889066`。其中 `packages/llm/llm-pi-ai` 支持多服务商配置，生成目录包含 `zai-coding-cn`、`minimax-cn`，`packages/bundle/base/cordis.patch.yml` 装配了该插件。**本地源码能力不能直接作为固定发行包的能力证明**，实施第一阶段必须检查实际构建产物。

本计划基于包含未提交 Permission 与配置改动的工作区。实施时保留已有改动，并核对 [Permission 运行时绑定计划](../permission-runtime-bindings/PLAN.md)、[配置与工作流契约](../role-capability-project-configuration/CONTRACT.md)及 [工具与技能接入计划](../tool-skill-integrations/PLAN.md)。本计划只验证 DSH 模型路由与既有原生工具，不扩大工具接入计划的实现及验收范围。

## 三 服务商映射

| 用户选择 | Model provider | DSH 路由 | 协议与端点 | 认证来源 |
|---|---|---|---|---|
| DeepSeek 官方 API | `deepseek` | `deepseek-official` | 继续使用固定 DSH 原生 DeepSeek 适配器 | `DEEPSEEK_API_KEY` |
| 智谱 AI 编程订阅 国内 | `zai-coding-cn` | `zai-coding-cn` | OpenAI Chat Completions；`https://open.bigmodel.cn/api/coding/paas/v4` | `ZAI_CODING_CN_API_KEY` |
| MiniMax 订阅 国内 | `minimax-cn` | `minimax-cn` | Anthropic Messages；`https://api.minimax.cn/anthropic` | `MINIMAX_CN_API_KEY` |
| 本机 Ollama | `ollama` | `ollama` 自定义路由 | OpenAI Chat Completions；宿主默认 `http://127.0.0.1:11434/v1` | 本地服务免密；客户端需要时使用非秘密占位值 |

智谱使用编程套餐专用 key 与端点，不回退到普通按量端点。官方文档区分套餐 key 与标准 API key，并列出上述端点。[智谱快速开始](https://docs.bigmodel.cn/cn/coding-plan/quick-start)

MiniMax 使用订阅 Key，不把普通按量 API key 标记为订阅。官方目前以 M Plan 命名订阅，给出的国内 Anthropic 端点是 `api.minimax.cn/anthropic`；本地 DSH 目录默认仍是 `api.minimaxi.com/anthropic`，因此 profile 显式写入核验端点，不依赖目录默认值，也不通过携带密钥的跨域重定向切换。[MiniMax 接入配置](https://platform.minimax.cn/docs/m-plan/other-tools)、[订阅 Key 说明](https://platform.minimax.cn/docs/m-plan/faq)

Ollama 自定义路由显式声明 `api: openai-completions`、`baseURL` 和模型清单。若 DSH 内部客户端要求 key，则使用 `ollama` 等固定占位值，不创建 vault 条目；官方本地接口忽略此值。[Ollama OpenAI 兼容说明](https://docs.ollama.com/api/openai-compatibility)

实际可用模型由固定 DSH 目录、显式模型声明和账号权限共同决定。不根据模型名猜服务商，不把任意模型改名为 `deepseek`，不自动切换其他服务商或计费方式。

## 四 Model 配置与运行解析

### 复用 Model 修订

- `ModelDefinition.provider` 使用上述标识；OpenAI 继续属于现有 Codex 接入。
- 复用 `endpoint`：首版仅 Ollama 允许填写本机服务地址，省略时使用明确的本机默认值；云服务商使用版本化内建端点，不开放任意云网关 URL。
- DSH Model 增加可选容量声明 `context_window`、`max_output_tokens`，映射为 DSH profile 的 `contextWindow`、`maxTokens`。它们描述模型容量，与请求参数 `parameters.max_tokens` 分开。
- 云模型可用固定目录容量；Ollama 等未收录模型必须获得或填写明确容量，不能套用插件对未知模型的超大默认值。声明不会修改 Ollama 服务端实际上下文配置。
- 新服务商首版支持 `max_tokens`；其他参数仅在固定 DSH、协议和模型能精确传递时接受。Codex 推理强度保持原语义。
- 保留 DeepSeek 现有预算规则；新增路由按模型容量确定缺省预算，显式参数越界报错，不直接套用 DeepSeek 的预算与统一上限。

服务商目录提供路由、协议、认证类型、端点和参数能力，供 Core 校验、Host 解析和 Web 共同使用。Harness 的 `model_providers` 仍属于精确修订；旧 Harness 只声明 DeepSeek 时，需要创建支持新服务商的新修订，不能原地扩大旧声明。

### 单一解析流程

从 `Agent → Capability → Model/Harness` 精确修订解析连接，再校验 DSH 运行版本和权限。Project、Feature、Task 讨论与正式 Run 共用结果，工程入口不能绕过校验。

`RuntimeBinding` 增加可选连接快照，记录 Model 服务商、DSH 路由、协议、声明端点、容器实际端点、容量和 profile 摘要；现有 `credential_ref` 保持为凭据引用的唯一权威字段。Agent 执行配置保存相关字段或不可变引用，连接结果纳入 configuration digest。

旧绑定缺少连接快照时，仅按原有 DSH/DeepSeek 语义兼容读取；不能把缺字段的新服务商记录误判为 DeepSeek。新服务商必须有完整快照，缺失则启动前拒绝。

旧项目/角色 Harness 偏好继续表达原有默认值。新服务商通过完整 Model/Capability/Agent 组合选择，不新增独立覆盖已冻结 Model 的快捷开关。发布新模型修订后，讨论通过现有后继 Session 使用新组合；既有 Feature、Run 和技术重试保持原配置。

## 五 凭据与设置接口

凭据仍保存在 macOS Keychain 或 Linux Secret Service；SQLite、Model、配置锁和报告仅保存不透明引用。服务商使用独立账号前缀和设置键，读取时验证数据集及服务商归属。

| 接口 | 兼容扩展 |
|---|---|
| `credentials.configure` | 保留 `provider: dsh`，增加 `model_provider`，为对应服务商保存或更换 key |
| `credentials.status/check/authorize` | 按 `model_provider` 查询或操作；不传时保持历史 DeepSeek 行为 |
| 凭据状态 | 区分已配置、vault 可用、认证方式和服务商；不返回 key |
| Ollama 状态 | 显示本地服务无需 key；不要求 vault 可用，不把占位值作为真实凭据 |
| 模型目录与运行检查 | 返回目标模型候选、容器可达性和脱敏错误；与 vault 读取检查分开 |

保存新 key 产生新引用，新执行选择当前引用，旧执行保留原引用；读取失败不替换成当前新 key 或其他服务商 key。首版不包含多账号选择、自动网页登录订阅、配额看板或自动购买。

兼容读取 `credentials.dsh`、`deepseek-official-api-key`、`deepseek-api-key-<uuid>`。迁移只登记旧引用，不读取、重存或删除秘密。macOS 助手增加账号格式时，同步更新源码指纹、签名和安装检查，遵守已有显式升级流程，避免 Host 更新后仍调用不支持新账号的旧助手。

环境过滤、协议脱敏和测试容器检查扩展到 `ZAI_CODING_CN_API_KEY`、`MINIMAX_CN_API_KEY` 及相关模型环境变量。每个模型 worker 只注入自己需要的凭据；保持既有临时凭据文件权限和清理保证，覆盖启动失败、取消和重启场景。

## 六 DSH 装配与版本验证

根据冻结连接生成受控 patch：DeepSeek 使用现有原生插件，其他三类路由使用 DSH 内置多服务商插件。显式指定凭据环境引用、协议和端点，必要时用 `models` 声明固定目录未收录的准确模型。仅启用本次需要的路由，不依赖宿主环境或用户全局 DSH 设置。

`initialize.provider` 使用解析后的路由，`initialize.model` 使用实际模型 ID，输出预算使用已校验参数。模型 patch 与 Permission、工具配置合成，保留现有审批、指令和日志上传禁用设置，不能互相覆盖。

M0 核验实际 `0.2.2-rc.1` 发行包与镜像中的插件、patch 格式、协议事件、模型清单及思考内容回放。优先保持上游版本。若无法完整支持，确定并记录可复现的固定 DSH 版本或配套插件构建，不混用本地滚动源码，不使用 `latest`。变化进入构建配方、镜像身份、能力表和适配器摘要；运行版本改变造成的权限协议差异一并验证。

MiniMax 至少验证“流式工具调用 → 工具结果 → 后续回复 → 恢复会话”，由 DSH 原生适配器回放完整 thinking/tool 内容；DevDesk 不另写消息裁剪转换器。[MiniMax 多轮工具调用说明](https://platform.minimax.cn/docs/api-reference/text-anthropic-api)

## 七 Ollama 容器访问

宿主和容器的 `127.0.0.1` 不是同一服务。冻结用户地址与实际访问地址，UI 显示转换结果，从目标 DSH 容器验证访问。

| 环境 | 处理方式 | 通过条件 |
|---|---|---|
| macOS Docker Desktop | 宿主 loopback 默认解析为 `host.docker.internal:11434/v1` | 当前 Docker Desktop 与 Ollama 监听方式实际允许访问 |
| Linux 本机 Docker Engine | 受控 `host.docker.internal:host-gateway` 映射，或用户配置的可达地址 | 用户已配置 bridge 可达监听或已有代理；host-gateway 本身不能访问只监听宿主 loopback 的服务 |
| 显式服务地址 | 保留合法 URL，不重复拼接 `/v1` | 分别报告 Host 与目标 worker 的检查结果 |
| 远程 Docker daemon | 不假设网关指向 DevDesk 宿主 | 提供真实 worker 可达地址，否则预检阻塞 |

首版不内置转发服务，不自动修改 `OLLAMA_HOST`、启动/重启 Ollama、下载模型、改防火墙或绑定全部网卡。Ollama 默认监听 `127.0.0.1:11434`；监听调整使用已有官方配置。[Ollama 网络配置](https://docs.ollama.com/faq)

URL 校验限定 HTTP/HTTPS、合法主机/端口和明确 API 根路径，拒绝嵌入账号密码、query 和 fragment；只有本地 Ollama 路由允许 HTTP，云路由固定 HTTPS。

Sandbox 禁网时启动前报告模型连接冲突；Harness 工具禁网不应误关模型进程连接能力。宿主映射只作用于相应模型 worker，不扩展独立测试容器网络或挂载，不使用 host network 模式。

模型目录由 Host 受控查询，用户显式保存后形成 Model 修订；目录可用不代表工具调用可用。服务断开、模型不存在、账号未登录、额度不足和上下文不足分别定位；无法可靠分类的上游错误保留脱敏诊断，不伪造 HTTP 状态。

## 八 Web 操作流程

1. 设置页保持 Codex/DeepSeek Harness 选择，在 DSH 下展示 DeepSeek、智谱编程订阅、MiniMax 国内订阅和 Ollama 配置入口。
2. 云服务商输入各自 key，保存后清空输入框并独立展示状态。Ollama 展示服务地址和本机账号登录提示，不要求填写 key。
3. Model 表单选择实际服务商和模型；Ollama 可填 endpoint、容量，查看容器访问地址；云服务商展示固定连接信息。
4. 目录候选显式保存；无法列目录时可明确填写模型，但仍做参数和实际模型验证，不能把候选列表当执行证明。
5. Capability/Agent 选择兼容 DSH 修订，不兼容时显示服务商或版本原因；项目角色和 Workflow 继续使用完整 Agent。
6. 分别展示凭据检查、目录查询和实际模型验证。实际验证使用所选固定运行时、连接和权限，从容器完成最小模型调用及工具回环。
7. 讨论与运行详情显示 Harness、服务商、模型和固定连接摘要。新连接通过后继 Session 生效，不原地替换原生会话。

默认套件为新路由发布 Harness 新修订，不原地改既有项目绑定或旧默认修订；不写死随订阅变化的“最新模型”，由用户完成连接和 Model 配置后选用。

## 九 实施阶段

| 阶段 | 工作 | 完成条件 |
|---|---|---|
| M0 固定运行时核验 | 检查实际 DSH 发行包、插件、三种路由、MiniMax 端点和 Ollama 占位认证 | 确定版本、协议和构建方案，以固定 DSH 访问模拟端点验证配置 |
| M1 领域与兼容解析 | 服务商目录、Model 字段、参数校验、运行快照、摘要和旧数据兼容 | 所有执行入口使用统一解析；错误组合在派发前拒绝 |
| M2 凭据 | 独立引用、RPC/Schema、macOS 助手、Linux 存储和旧引用迁移 | 新旧 key 独立可用，引用不能跨数据集/服务商使用，Ollama 不依赖 vault |
| M3 DSH 与容器 | 路由 profile、initialize、patch 合成、宿主映射、脱敏和清理 | 三类新路由文本与工具回环通过，权限和测试容器边界有效 |
| M4 Web 与默认配置 | 设置、Model 表单、目录/运行检查、摘要和新 Harness 修订 | Web 可完成配置，通过完整 Agent 发起讨论与正式执行 |
| M5 回归与真实验收 | 迁移、并发、恢复、跨平台、发布包及文档 | 按矩阵记录模拟、真实通过和待验范围 |

只修改 DevDesk 必要接入层及受管 DSH 构建材料，不预设修改相邻 `deepseek-harness` 仓库。若 M0 确认必须改上游源码，先把具体差异和固定交付方式补入计划提交审核。

## 十 验证与验收

### 自动化回归

| 范围 | 必须覆盖 |
|---|---|
| 解析和 Schema | 合法映射；Codex/非 OpenAI、旧 Harness/新服务商、未知 provider、非法 endpoint、不支持参数均拒绝 |
| 凭据 | 独立保存/更换/读取、旧 DeepSeek 引用、跨数据集/服务商拒绝；无 vault 环境可配置 Ollama |
| DSH 协议与模拟服务 | 固定 DSH 进程访问模拟 OpenAI/Anthropic 端点，验证 URL、header、model、流式终态、工具与恢复；报告不输出假 key |
| Ollama 网络 | Host 成功但容器失败不得通过；两类 Docker 地址处理、无模型、容量错误、断连、禁网、远程 daemon |
| 并发和隔离 | 三类路由并行不串用 key/profile/原生 Session；测试容器无模型凭据及新增宿主映射 |
| 冻结和迁移 | 设置变化不改旧执行；旧凭据不可读明确失败；新服务商缺快照不回退；旧修订内容摘要不重算覆盖 |
| Web | key 保存清空、切换服务商清除无效字段、完整组合创建、目录/实际调用错误定位 |
| 发布包 | 内嵌配方/Schema/目录完整，不依赖相邻源码；debug/release 连接语义一致 |

先运行相关 Rust 单元/集成测试、HTTP/Schema 契约、Web build 和 Playwright，再运行仓库规定的完整检查。计划审核阶段不运行模型调用、镜像构建或凭据变更。

### 真实调用

保留 DeepSeek 与 Codex 回归；新增智谱、MiniMax、本地 Ollama、Ollama 订阅云模型各至少一个案例。新路由验证流式讨论、工具调用与结果消费、后继/恢复、取消/故障恢复，并通过完整 Agent 的正式工程入口验证输出和权限。本地模型和订阅云模型分别记录，不能互相替代。

macOS、Ubuntu 分别验证 vault、DSH 容器和 Ollama 连通。只有具备对应环境、订阅和模型并完成调用的案例标为通过；缺失账号或平台列为待验，模拟通过不升级为真实验收通过。实际调用使用用户在设置页配置的凭据，报告仅保存构建身份、模型、配置修订和脱敏结果。

## 十一 交付与审核重点

交付包含领域/RPC/Schema、存储兼容、凭据、DSH 装配、Docker Ollama 连通、Web、回归测试及用户文档。实施记录写入同目录 `EXECUTION.md`，计划目录只保留 Markdown 交付物。

审核重点：复用 DSH 内置适配器；Model 承载连接配置并按服务商分开凭据；Ollama 通过本机服务使用本地与订阅云模型；Linux 首版采用显式可达地址，不内置 loopback 转发；先核验固定发行包再确定构建变化。用户确认后开始 M0–M5。
