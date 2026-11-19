# 系统设计文档

本目录集中说明 DevDesk 的系统架构、领域模型、配置与执行机制、外部接入、工作台和运行方式，并收录开发规范。本文提供主题索引和阅读路线；具体字段、规则与能力边界以各专题及其引用的实现为准。

[项目入口](../../README.md) · [crates 代码手册](../crates/overview.md) · [完整执行链](../crates/execution-flows.md) · [计划与验收记录](../plans/README.md)

## 系统脉络

DevDesk 由 Rust 内核管理配置和工程事实，React 工作台通过 HTTP / JSON-RPC 与 SSE 读写和观察状态。domain 定义领域类型与规则，core 编译配置、组织执行并接纳回执，adapters 实现存储和外部能力，host 负责入口、装配与生命周期。

理解系统可以沿三条主线阅读：

- **配置如何成为执行依据**：Role、Capability、Agent、Workflow、Spec、Delivery 和 Standard 通过不可变源修订组合；发布、项目采用和执行冻结具有各自的边界。
- **工作如何推进并形成事实**：工作流编译出步骤和输入关系，人工、Rust 或 Agent 驱动共用内核命令；外部操作先保存意图，再执行并核对回执，完成条件依据实际产物、测试和决定求值。
- **执行如何受到约束并恢复**：会话冻结模型、运行时、权限与接入选择；Harness/Sandbox 落实执行环境和权限，SQLite 与不可变产物保留历史，恢复沿原引用核对未知结果。

## 文档索引

### 架构、领域与合同

| 文档 | 主要内容 | 适合查阅的问题 |
|---|---|---|
| [架构与设计决策](design.md) | 模块分层、事实来源、并发与性能约束、事务外执行、项目隔离 | 系统为什么这样分层？哪些约束必须保持？ |
| [领域规范](domain.md) | 配置概念、工作归属、输入与完成、权限、经验和 Standard | 核心对象分别表达什么？对象之间如何关联？ |
| [结构合同与执行事实](contracts.md) | 源修订、结构校验、版本标签、执行原语及证据边界 | 配置能表达什么？哪些能力必须由代码实现和验证？ |
| [本地协议](protocol.md) | HTTP / JSON-RPC / SSE、查询与命令、项目作用域、原版本与幂等 | 客户端和内核怎样交互？变更请求遵守什么合同？ |

### 配置、工作流与数据

| 文档 | 主要内容 | 适合查阅的问题 |
|---|---|---|
| [源配置与执行解析](source-configuration.md) | 不可变源、依赖图、发布与采用、冻结恢复、安装输入及 Spec 检查 | 配置如何被校验、组合和采用？修改如何影响已有执行？ |
| [工作流与编排](workflow-orchestration.md) | 三种推进模式、步骤生命周期、产物接纳、批准、返工、计划变更和预算 | 谁推进步骤？何时完成、等待、返工或允许变更？ |
| [讨论与经验](conversations.md) | 会话范围、正式任务与自由讨论、审批与澄清、提案、权限后继和经理工作台 | 会话能做什么？讨论、工程执行与经验如何衔接？ |
| [存储与数据布局](storage.md) | 数据集、三库分工、源与事实存储、幂等恢复、模板代次及兼容策略 | 数据保存在哪里？如何恢复、重置模板和处理不兼容数据？ |

### 运行时、权限与外部接入

| 文档 | 主要内容 | 适合查阅的问题 |
|---|---|---|
| [运行时权限](permissions.md) | Profile、默认与显式绑定、Harness/Sandbox 双层约束、平台授权和权限切换 | 具备某项能力后，本次执行是否有权调用？ |
| [系统凭据](system-credentials.md) | 凭据信任范围、macOS 签名助手、Linux Secret Service 和服务商凭据 | 秘密由谁保存和读取？系统授权与执行权限如何区分？ |
| [模型服务商与 Harness](model-providers.md) | 服务商配置、端点、Ollama 地址、助手升级、冻结与验证范围 | 模型如何接入？目录读取、协议探针和真实推理各证明什么？ |
| [工具与技能接入](tool-skill-integrations.md) | HTTP / OpenAPI / MCP / Skill、账户绑定、OAuth、隔离及支持范围 | 如何发布和授权外部工具或技能？有哪些协议限制？ |
| [工程适配器接入契约](real-adapter-contract.md) | core 端口与真实实现、Harness/Git/测试/PR 合同、回执接纳与取消 | 外部操作怎样执行和核对？适配器必须提供哪些证据？ |

### 工作台、运行与验证

| 文档 | 主要内容 | 适合查阅的问题 |
|---|---|---|
| [Web 工作台指南](web-workbench.md) | 导航、配置与执行入口、刷新模型、典型旅程、设置和诊断 | 用户在哪里配置、讨论、查看执行和处理审批？ |
| [工程接入与通用流程](real-engineering.md) | 接入项目、配置运行、工程边界、验证入口与证据保留 | 如何从仓库接入走到实际执行和验收？ |
| [本地运行包](run-binary.md) | 启动依赖、数据集、停机恢复、远程连接、模型与平台设置 | 如何部署和运行？如何连接远程 Host？ |

### 开发规范

| 文档 | 主要内容 | 适合查阅的问题 |
|---|---|---|
| [Rust 代码规范](code-standards-rust.md) | 分层不变量、命名、错误与注释、惯用法、测试及 lint 门禁 | 修改内核和适配器时遵守哪些约定？ |
| [Web 代码规范](code-standards-web.md) | React / TypeScript、状态与异步、样式与可访问性、测试和检查 | 修改工作台时如何组织组件、类型和请求？ |

## 建议阅读顺序

| 目标 | 阅读路线 |
|---|---|
| 建立系统全貌 | [架构](design.md) → [领域](domain.md) → [结构合同](contracts.md) → [工作流](workflow-orchestration.md) |
| 修改配置或业务流程 | [领域](domain.md) → [源配置](source-configuration.md) → [工作流](workflow-orchestration.md) → [协议](protocol.md) |
| 开发客户端或会话功能 | [Web 工作台](web-workbench.md) → [讨论与经验](conversations.md) → [协议](protocol.md) → [Web 规范](code-standards-web.md) |
| 接入模型、工具或执行器 | [权限](permissions.md) → [凭据](system-credentials.md) → [模型](model-providers.md) / [工具与技能](tool-skill-integrations.md) → [适配器契约](real-adapter-contract.md) |
| 部署、排障与验证 | [运行包](run-binary.md) → [工程接入](real-engineering.md) → [存储](storage.md) → [测试索引](../../tests/README.md) |
| 定位和修改内核实现 | [架构](design.md) → [Rust 规范](code-standards-rust.md) → [crates 手册](../crates/overview.md) → [执行链](../crates/execution-flows.md) |

## 与其他文档的分工

| 入口 | 内容与边界 |
|---|---|
| [crates 代码手册](../crates/overview.md) | 按 crate、源码文件和调用路径解释实现，与本目录的系统专题互补。 |
| [Schema](../../schemas/README.md) / [SQL](../../sql/README.md) | 结构字段与数据库基线的详细依据。 |
| [默认配置](../../resources/configuration/README.md) | 出厂目录、默认工作流和资源组合。 |
| [权限后端](../../permissions/README.md) | 平台权限资产、后端支持矩阵及执行边界。 |
| [测试](../../tests/README.md) / [脚本](../../scripts/README.md) | 验证分层、实际执行入口、构建与打包工具。 |
| [计划与验收](../plans/README.md) | 待实施方案和历史记录；结论仅对应记录中的代码、环境与输入。 |

维护系统行为时同步更新对应专题及实现依据；新增、移动或删除专题时更新本索引和相关引用。本目录描述当前系统，容量目标、未实施方案和历史验收应保留各自状态，不能互相替代。执行 `python3 scripts/py/check-docs.py` 检查当前文档的本地链接、章节锚点和 crate 文件索引。
