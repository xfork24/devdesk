# 架构与代码文档重写计划

状态：P0–P5 已完成；实施与验证见 [EXECUTION.md](EXECUTION.md)。

日期：2026-11-19。

## 1. 要交付什么

重新组织 `docs/design/` 和 `docs/crates/`，让首次接触项目的开发者能够先理解系统，再沿着具体功能读代码。旧文档作为核对材料，不作为章节结构或行文模板。

- `docs/design/` 说明系统解决什么问题、由什么组成、如何运行、为什么采用这些边界，以及有哪些限制。
- `docs/crates/` 说明这些设计落在哪些模块、请求如何穿过模块、数据在哪里读写、修改一个功能需要看哪些代码和测试。
- 每个重要规则只有一处主说明，其他章节通过链接引用。
- 有价值的事实和示例保留，重复、过时、只复述文件名的段落删除。旧页面完成内容迁移和引用更新后删除，不保留两套并行的当前说明。

本计划先于实施编写。实施阶段可以根据源码核对结果微调分篇，但不能遗漏下文规定的主题，也不以保留旧文件数量为目标。此次工作不修改产品行为。

## 2. 当前文档的问题

已梳理两个目录的文件、目录入口、章节结构、主要内容和文档检查方式，并结合已有源码阅读核对主要分层与执行路径。现有架构目录 19 篇、代码目录 37 篇，共 56 篇。

| 问题 | 对阅读的影响 | 重写方式 |
|---|---|---|
| 架构、接口清单、操作指南和历史修复说明混写 | 看完细节仍难以回答“系统如何工作” | 架构正文、参考资料、运行指南分开 |
| 先讲约束和异常，再讲正常过程 | 读者缺少理解规则的上下文 | 先讲一个完整例子，再解释规则和失败分支 |
| 一句话塞入多个未解释的术语 | 必须来回查阅才能理解 | 先用普通语言解释，再引入必要的代码名和术语 |
| 代码手册主要逐文件介绍 | 看得到模块，看不到调用关系 | 按功能过程组织，给出入口、调用、状态变化和返回结果 |
| 完整文件索引挤在 crate 概览中 | 自动生成的清单淹没人工说明 | 索引移入各 crate 的 `files.md`，概览只保留模块地图 |
| 后续改动不断追加到旧章节 | 规则重复，主次和阅读顺序不清 | 按当前行为重写，不沿用补丁叙事 |

保留现有文档中有价值的技术信息：层次边界、状态约束、协议差异、权限落实方式、恢复与幂等要求、测试限制。每项内容在进入新文档前重新核对，不能因为旧文档写过就当作已证实。

## 3. 阅读路线和贯穿示例

提供三条入口路线：

1. **了解系统**：系统总览 → 分层与边界 → 核心概念 → 一次任务的执行过程。
2. **理解运行机制**：Agent 通信 → 工具与接口 → 数据与恢复 → 权限与凭据。
3. **修改代码**：代码地图 → 跨模块调用链 → 对应 crate 专题 → 测试与扩展指南。

主要示例采用“项目经理派发需求分析师，需求分析师生成 PRD，项目经理读取结果并推进后续步骤”。从同一个例子解释运行位置、工具调用、Host 调度、容器启动、结果保存和事件通知，避免每篇都换一套对象。

工程交付另用“修改代码 → 测试 → 独立评审 → 合入”解释 Git 和批准流程。失败分支覆盖进程中断、输出不合格、评审拒绝、人工等待和重复请求；不把所有异常塞进首次阅读的主流程。

示例中的角色、步骤、产物格式和工具名来自当前发布资源。示例是具体配置，不能写成所有工作流都必须遵守的固定业务流程。

## 4. 架构文档的新结构

下表是目标文件结构。文件名以代码样式列出，表示计划中的页面，并非已经存在的链接。

| 目标文件 | 要回答的问题及内容边界 |
|---|---|
| `README.md` | 系统简介、三条阅读路线、章节地图、架构与代码手册的区别 |
| `overview.md` | 产品目标、用户与外部系统、主要能力与非目标；Host、浏览器、Harness、Docker、模型服务的运行位置；当前部署边界 |
| `layers.md` | domain/core/adapters/host/tools/Web 各负责什么；编译依赖与运行时调用的区别；接口定义、实现、装配的位置；设计原因和代价 |
| `domain-model.md` | Feature、Task、Workflow、Step、Agent、Session、Turn、Artifact 的关系、身份与生命周期；哪些规则属于领域、哪些属于应用流程 |
| `configuration.md` | Role、Capability、Skill、Spec、Delivery、Standard、Profile 等如何组成执行配置；编辑、发布、采用及本次执行选定版本的过程 |
| `workflow.md` | 从工作项到步骤派发、输出检查、评审、批准和完成；经理的权限与职责；正常流程及状态变化 |
| `agent-runtime.md` | Harness 与 Sandbox 的区别、容器与会话生命周期、进程通信和动态工具往返；PM 如何启动分析师并取得 PRD；讨论与经验访问 |
| `interfaces.md` | 浏览器 RPC/SSE、Agent 工具、内部端口的区别；Host 对外提供什么、内部调度依靠什么；完整字段和工具清单链接到参考页 |
| `data-and-recovery.md` | 全局/项目/讨论数据归属、配置与产物存储；事务、版本冲突、外部副作用、重启恢复、取消；retry/rework/reconcile 各解决什么问题 |
| `security.md` | 信任边界、身份与批准、权限配置怎样落实到 Harness/Sandbox、凭据的保存和使用；关键失败条件 |
| `engineering-delivery.md` | 工作区与分支、真实测试、独立评审、PR/合入和交付检查；当前默认流程与可配置机制的区别 |
| `web-workbench.md` | 用户操作如何变为 Host 命令、项目作用域、查询与 SSE 刷新、冲突和等待状态如何显示 |
| `quality-and-limits.md` | 测试层次、运行诊断、已知限制、容量目标与验证范围；主要架构取舍及后续约束 |
| `glossary.md` | 必要术语的短定义和主说明链接；避免复制各章长解释 |
| `reference/rpc.md` | RPC/HTTP/SSE 按用途分组的协议参考、参数和错误语义 |
| `reference/tools-and-primitives.md` | Agent 可调用工具与 Host 内部原语分别列出：用途、权限、输入输出、状态或副作用、处理入口；区分注册、发布与当前 Agent 可见 |
| `reference/integrations.md` | Harness/模型服务商/外部工具/MCP/Skill 的支持范围与协议限制 |
| `reference/contracts.md` | Schema、配置、运行输入与交付格式的对应关系和最小示例；完整字段以 Schema 为准 |
| `run-binary.md` | 独立发布包的安装、启动、停止、数据目录和排错；保留现有路径以适配打包入口 |

正文只保留帮助理解机制的字段和函数名。协议大全、所有工具参数和安装命令放到对应参考页或操作指南。Rust/Web 编码规范归入代码手册，避免和系统设计混在一起。

图只用于解释关系：系统运行拓扑、分层依赖、主要对象关系、PRD 交接时序和关键状态变化。编译依赖图与运行时通信图分别绘制；不会用一张图混合表示两者。使用可渲染的 Mermaid，复杂流程拆图。

## 5. 代码文档的新结构

根目录建立 `README.md` 作为阅读入口，保留并重写 `overview.md` 作为仓库地图。`execution-flows.md` 组织跨模块调用过程，不再重复所有模块职责。

| 目录或页面 | 专题组织 |
|---|---|
| `overview.md` | Cargo 包依赖、目录职责、Host 装配点、Web/资源/Schema/SQL/脚本与 Rust 的关系 |
| `execution-flows.md` | 启动与装配、PM 派发到 PRD 回传、配置生效、人工决定与返工、停止与恢复；每条流程链接对应模块专题 |
| `devdesk-domain/` | `overview.md`；业务对象与产物；配置与引用；工作流和纯规则；会话/权限/接入类型 |
| `devdesk-core/` | `overview.md`；Engine 与 ports；配置加载和编译；工作流执行和状态提交；经理和平台工具；会话与运行资源；计划变更与恢复；工程交付 |
| `devdesk-adapters/` | `overview.md`；工厂与配置装载；SQLite 和产物存储；Harness 协议与 Docker；运行环境准备；Git/测试/远端交付；凭据/权限；外部接入 |
| `devdesk-host/` | `overview.md`；启动/项目作用域/装配；HTTP/RPC/SSE/认证；后台调度和资源生命周期 |
| `devdesk-tools/` | `overview.md`；凭据助手的请求协议和平台实现；构建/签名/验证及以后新增独立工具的入口 |
| 各 crate 的 `files.md` | 自动文件索引和人工专题链接；作为查找辅助，不承担教学任务 |
| `web-integration.md` | Web 客户端、状态读取、命令发送与刷新路径；明确它不是 Rust crate，提供前后端衔接的代码入口 |
| `testing-and-extension.md` | 按功能查测试；新增工具、适配器、领域规则或 RPC 时需要修改的层和验证方法 |
| `standards/rust.md`、`standards/web.md` | 保留仍有效的开发约定、检查入口和原因；删除与自动检查配置重复的长清单 |

专题按模块职责划分，可合并现有文件或采用更明确的名字。每个专题用统一的阅读顺序，但不强制为没有内容的栏目凑段落：

1. 本模块解决什么问题，什么时候进入这里。
2. 关键类型和入口；输入、输出分别是什么。
3. 一次调用经过哪些关键函数；从上游哪里来，交给下游什么。
4. 读取和修改什么数据，何处发生文件、网络、进程或数据库操作。
5. 容易误解的边界及关键失败处理。
6. 如何修改这个功能，相关测试在哪里、能证明什么。

链接指向实际源码文件，并写出可搜索的符号名；只有确有帮助时附行号。不会用大段源码转录替代解释。

## 6. 旧文档如何取舍

### 架构目录

| 旧文件 | 处理方式与内容去向 |
|---|---|
| `README.md` | 重写阅读导航 |
| `design.md` | 拆入 overview、layers、data-and-recovery、quality-and-limits，删除旧页 |
| `domain.md` | 重构为 domain-model，具体代码说明交给 domain 手册 |
| `contracts.md` | 概念移入 configuration/workflow，格式参考移入 reference/contracts |
| `source-configuration.md` | 重写为 configuration，按配置从编辑到执行的顺序组织 |
| `workflow-orchestration.md` | 拆入 workflow、agent-runtime、data-and-recovery；删除追加式历史说明 |
| `conversations.md` | 会话运行机制并入 agent-runtime，界面交互并入 web-workbench |
| `storage.md` | 重构为 data-and-recovery，SQL 和存储函数细节移入 adapters 手册 |
| `protocol.md` | 拆为 interfaces、reference/rpc、reference/tools-and-primitives；删除重复行为说明 |
| `permissions.md`、`system-credentials.md` | 架构合并为 security，代码细节进入 adapters/tools，安装操作进入 run-binary 或链接已有脚本指南 |
| `model-providers.md`、`tool-skill-integrations.md` | 设计边界进入 agent-runtime/interfaces，具体接入范围合并到 reference/integrations |
| `real-adapter-contract.md` | 职责与约束进入 layers、工程流程进入 engineering-delivery、方法对应关系进入代码手册 |
| `real-engineering.md` | 重写为 engineering-delivery，不保留按早期实施阶段组织的叙述 |
| `web-workbench.md` | 保留主题和路径，重写交互与数据流 |
| `run-binary.md` | 保留路径，核对并重写操作指南，确保打包后可独立阅读 |
| `code-standards-rust.md`、`code-standards-web.md` | 迁入代码手册 standards，更新全部当前引用 |

### 代码目录

| 旧文档组 | 处理方式 |
|---|---|
| 根 overview、execution-flows | 保留职责，重新编写；新增 README 导航，清除重复说明 |
| domain 的 work-and-materials、configuration-and-sources、workflows-and-checks | 作为三个主专题的事实素材，按对象关系与规则重写 |
| domain 的 agents-and-conversations、runtime-and-permissions、integrations | 合并关联类型说明；跨进程行为移到 core/adapters，避免让类型声明看起来像执行实现 |
| core 的 engine-and-ports、configuration-and-catalog、workflow-compilation | 前者保留专题，后两者合并配置全过程；明确 ports 定义与 Host 注入 |
| core 的 workflow-execution、orchestration-and-drivers、manager-and-platform-tools | 按工作流执行、经理决策/工具两个专题重组；工具完整清单只在参考页维护 |
| core 的 session-resources-and-permissions、conversations-and-history | 合并为会话与运行资源，突出输入准备、执行、输出接纳与查询 |
| core 的 workflow-changes、integrations-and-repositories | 分别重写为变更/恢复和工程交付；不把调用端口写成外部协议实现 |
| adapters 的 factory-and-configuration、storage-and-source-codecs | 分别重写工厂/装载与存储专题，交代配置解码、数据库和业务决策的边界 |
| adapters 的 runtime-preparation、harness-and-protocols、engineering-resources | 分别重写运行准备、Harness/Docker、工程资源；突出真实进程调用与结果转换 |
| adapters 的 credentials-and-permissions、external-integrations | 保留专题价值，按实际后端和接入过程重写 |
| host 的 workspace-and-lifecycle、rpc-and-configuration、http-and-security | 重组为启动/装配、请求服务、后台调度三个专题 |
| adapters/host 的 testing | 测试入口合并到 testing-and-extension；重要协议和失败案例随对应专题解释，删除重复测试枚举 |
| tools 的 request-protocol、signing-and-keychain | 保留独立进程协议与平台实现的信息，结合工具运行和构建流程重组 |
| 五个 crate 的 overview | 全部重写成短概览；自动索引迁至 files.md，不再复制专题正文 |

不将删除内容整体搬到新的“旧文档”目录。历史演进已有 Git 和 `docs/plans/` 记录；历史计划正文保留，只在必要时机械更新指向当前文档的路径和锚点。

## 7. 写作要求

- 开篇先回答本章的问题，再展开依据。一个段落说明一个主要关系。
- 明确主语，例如“Host 保存任务状态”“适配器启动容器”“core 检查是否满足完成条件”。避免只罗列抽象名词。
- 先解释正常路径，再写约束、故障和恢复；例外应紧邻适用的步骤。
- 首次出现的术语给出具体含义；不能用另一个未解释的术语解释它。
- 区分业务规则、应用编排、外部实现和装配。例如，不能把 core 写成纯转发层，也不能因为 Docker 封装在 adapters 就把调度决策归入 adapters。
- 区分实现能力、出厂配置、当前运行权限和验证结论。目标、实验结果与已实现行为分别标注。
- 只保留有助于理解的表和图。接口比较适合表格，状态和交互适合图，简单职责用文字即可。

表达示例：

> Host 先记录准备执行的操作，再启动外部程序。程序返回后，Host 核对结果并更新步骤。这样即使中途重启，也可以先检查原操作是否已经完成，避免重复执行。

这类说明之后再介绍对应代码中的 intent、receipt 或 reconcile，避免让术语承担全部解释。

## 8. 实施顺序

| 阶段 | 工作 | 完成条件 |
|---|---|---|
| P0 内容盘点 | 目录、内容结构、检查器与打包引用梳理 | 已完成；本计划记录问题和迁移方向 |
| P1 事实核对与样章 | 对照源码/Schema/SQL/发布资源建立主题与证据映射；先写 layers、agent-runtime、core 的 Engine/ports 专题 | 分层关系和 PM→分析师→PRD 全链条能讲通；统一术语和写法 |
| P2 架构正文 | 按阅读路线重写设计章节，补齐参考页和运行指南 | 所有架构主题有主说明；正常路径、边界、限制完整 |
| P3 代码手册 | 先写跨模块调用链，再按 domain/core/adapters/host/tools 完成专题，补 Web 和测试入口 | 读者能从行为定位入口、状态写入、外部执行及测试 |
| P4 迁移与清理 | 移出自动索引，适配检查器；更新全仓库文档引用和锚点；删除已替代旧页；核对发布包 README | 只有一套当前说明，无孤立旧页和失效入口 |
| P5 核验 | 技术对照、阅读路线检查、文档检查与必要的脚本验证 | 下文验收问题全部可回答，链接/索引检查通过，验证范围如实记录 |

P1 的样章用于先检验整套结构是否成立，再批量展开，不为每篇文档设置额外确认步骤。实施期间若发现代码缺陷，单独记录，不夹带产品修复。

## 9. 验证与验收

### 内容正确性

- 源码事实来自 `crates/`、`web/src/`、`schemas/`、`sql/` 和 manifest 实际发布的资源；历史计划仅用于理解背景。
- 逐项核对关键状态、协议、工具名、调用方向和权限判断。注册的处理函数、发布的工具定义和某个 Agent 可见的工具不能混为一谈。
- 每个核心流程至少跟踪到入口、应用逻辑、外部端口/实现、保存结果和返回/通知位置，并给出代码入口。
- 重写时重新核对供应商支持范围、默认流程和数据库版本。避免在多个正文中重复易变数字。
- 取舍表中的旧内容必须有“已迁移、重复删除、过时删除”结论，防止重排目录时遗漏重要约束。

### 可读性验收问题

新读者应能顺着导航找到答案，不必先阅读源码或历史计划：

1. 浏览器、Host、Harness、Docker、模型服务分别在哪里运行？
2. core 没有依赖 adapters，为什么仍能使用 Docker 和 Harness？谁完成装配？
3. domain、core、adapters 分别决定和执行什么？
4. 项目经理如何发起需求分析师任务？PRD 如何保存、通知并被读取？
5. 某个 Agent 能调用哪些工具，由什么决定？Host 内部原语和这些工具有什么区别？
6. 修改默认配置会不会改变已经启动的执行？实际采用哪个版本？
7. 输出合格、评审通过和人工批准有什么关系？谁能改变步骤状态？
8. 容器中断或 Host 重启后，如何避免把同一操作执行两次？停止、重试和返工有什么不同？
9. 新增一个工具、适配器或领域规则，应改哪些位置，运行哪些测试？
10. 当前支持与已验证范围是什么，哪些性能要求仍属于目标？

### 自动检查与交付检查

- 调整 `scripts/py/check-docs.py` 以支持独立 `files.md` 索引，保留源文件覆盖和链接/锚点检查；生成范围排除本地临时产物，不能靠忽略缺失文档让检查通过。
- 执行文档检查和 `git diff --check`；检查器变更补充必要的定向验证，覆盖缺文件、过时索引和错误锚点等情况。
- 搜索旧文档路径并分类处理当前链接、历史链接和字面示例；避免替换命令示例中的无关文本。
- 核对根 README、AGENT、脚本/测试/资源索引和计划索引中的阅读入口。
- `scripts/release/package.sh` 会把 `docs/design/run-binary.md` 复制为包内 README；核对其内容不依赖包外源码和失效的仓库相对链接。
- 纯文档修改不需要重新执行付费模型、真实 GitHub 交付或完整产品回归；文档中引用已有测试时注明原验证范围，不声称本轮重新运行。

最终交付为两个重构后的文档目录、必要的引用与检查器改动，以及本计划目录内的实施记录。实施记录说明内容取舍和实际验证结果，不把过程日志混入架构正文。
