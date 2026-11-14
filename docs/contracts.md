# 结构合同与执行事实

源定义、领域结构、外部协议和持久化格式有不同的校验边界。业务配置可以组合现有原语；新增执行原语、驱动协议或隔离能力仍需要代码实现。不能把一个配置标签当成已实现的能力，也不能用记录版本号代替内容校验。

## 源修订与领域结构

配置使用 `{type, identity, revision, name, scope, 根级引用, data}`，引用为 `identity:revision`。修订不可变，摘要覆盖根级引用及 data；按类型限定引用字段并校验类型、作用域和依赖。源定义是唯一配置存储，内存执行视图从原引用恢复。

`WorkflowDefinition`、`DeliveryTypeDefinition`、`ConfiguredSpecification` 等按 Serde 结构与领域规则接纳。Workflow 定义由顶层 stages 与 steps 组成，step 通过 stage 归组；发布检查 DAG、端口、角色、能力、权限及完整子流程闭包。草稿可有引用缺口，正式发布和执行不得留下缺口。

Agent 只组合 Role 与 Capability；任务属于 Step，方法属于 Skill，内容规范属于 Spec，执行授权属于 Permission Context/Profile。Delivery 保存格式、离线 Schema、必需 spec_refs 及可选 producer 来源约束。源定义中的声明不能直接扩大执行授权。

## 记录标签与兼容边界

| 信息 | 当前处理 |
|---|---|
| 业务 schema_version、renderer_version | 标记记录的生成合同；业务读取按实际结构和规则校验，不统一按版本常量拒绝。 |
| ConfiguredAgentTask | 校验外层与 task 内的 renderer_version 一致、内容摘要一致、来源一致；历史任务按实际冻结合同读取。 |
| 数据集标记与 SQLite user_version | 明确校验 `devdesk-source-flat-v1`、global 18 / project 48；不兼容数据集拒绝，不自动迁移或重建。 |
| RuntimeInstallation 版本 | 仅是标签；安装来源、镜像内容、协议验证与权限资产另行核验。 |
| macOS 凭据助手协议 | 当前要求协议 2、实际二进制摘要与同证书签名；目录名 v1 不是协议版本。 |
| HTTP、Codex/DSH、MCP 等外部协议 | 由实际适配器实现和探测结果限定，源配置不能凭空增加协议支持。 |

新增字段是否可省略由各结构的 serde default 决定，未知字段是否拒绝由对应结构与 Schema 决定；不能笼统声称任何加字段都会不兼容。

## 执行原语

StepExecutor 有 agent、tool、workflow、control 四类。CompletionRule 提供 Always、All、Any、Exists、Equals、Same、Covers、Before、Independent、Outcome、Approved，按接纳后的值、执行事实和人类决定求值，不执行任意脚本。工作流递归深度上限 8、展开步骤上限 1000；规则树有独立深度和数量上限。

平台工具注册表定义实际处理器；Capability 表示可用能力，Permission 表示本次允许调用。业务 Agent、根控制器和项目经理工作台分别核验真实 Session/Turn/worker、当前步骤或根 epoch、固定项目及冻结授权。普通讨论不能调用需要正式工作归属的控制工具。当前 Codex 提供动态平台/编排桥，DSH 不提供该桥。

根实例的 manual、rust、agent 驱动均使用同一内核命令。Rust 策略源只配置有界并发、动作预算、租约及结构化技术重试，不替代业务完成规则。经理工作台的资料查询/配置创作与根实例调度权分开。

## 运行资源与模型目录

`runtime_resource` 源保存驱动、版本标签、许可证及不可变 OCI 镜像或有界构建文件，可包含按平台校验的本机登录组件下载件。默认资源从出厂配置选择，Rust 中没有 CODEX_VERSION/DSH_VERSION 发行表。Harness 通过精确 runtime_resource_ref 或 recipe_digest 内容边选择资源；同一驱动允许多个配方。

`model_provider` 保存路由、端点及凭据约定，`model_catalog` 按 recipe_digest 固定容量目录，Harness 采用确切源。实际会话核对冻结连接与生成 profile 摘要；显示名、发行标签和目录候选均不证明推理成功。

准备器核验实际镜像并保存 `devdesk/prepared-<provider>:<image-digest>` 标签，登记安装和当前 driver/权限资产下的协议报告。Codex 探测初始化、thread/start 和动态工具注册；DSH 探测初始化。报告明确未验证推理和实际工具执行，真实工作还需原生准入、输入消费、终态及停止证明。

## 内容检查与证据

Spec 可声明 text_length、required_sections、required_pointers 机器断言，由同一结构检查链用于草稿 lint、输入、材料导入和产物接纳；未声明 assertion 的语义检查仍需要独立判断。Spec 不运行任意程序，测试、Git 和发布事实只接受平台实际回执。

EvidenceLocation 绑定冻结输入的 source_ref、value_digest 和定位器。可选 quote 必须等于选中范围的完整文本；空 JSON pointer 选择完整输入。行范围从 1 开始且包含末行；字符范围按 Unicode 字符从 0 开始，末端排除。悬空、越界、摘要不符或不属于当前输入的引用均拒绝。

`runtime_context.binding` 是当前会话的运行资源选择，`runtime_context.inputs` 是上游确切 Turn 的外层 Harness 执行、镜像和停止证明。原生命令日志、代码快照、平台测试报告、模型记录和人类决定是不同事实；都不能互相冒充。

## 出厂目录与实现依据

`manifest.json` 声明发布资源，`defaults.json` 选择默认引用；Markdown 正文归属于对应源修订。构建校验结构，`check-defaults.py` 检查目录/manifest/默认引用/正文归属，应用和测试继续核对完整组合、权限与来源。目录自洽检查不能替代实际执行验证。

实现依据：[源协议](../crates/domain/src/source.rs)、[Workflow 与任务校验](../crates/domain/src/workflow.rs)、[运行资源](../crates/domain/src/runtime_resource.rs)、[平台处理器](../crates/core/src/platform_tools.rs)、[协议验证](../crates/adapters/src/runtime_verification.rs)、[出厂目录检查](../scripts/py/check-defaults.py)。
