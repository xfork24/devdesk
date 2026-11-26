# 目标内核 SDK 契约

| 项 | 内容 |
|---|---|
| 状态 | **待实施**。本文是目标契约，不是当前已实现的接口 |
| 日期 | 2026-11-26 |
| 所属计划 | [架构独立性改造](PLAN.md) |
| 改造前的事实 | [内核 SDK 基线](BASELINE-kernel-sdk.md) |

## 1. 定位与原则

内核定位为**领域无关**。它定义通用概念并提供运行它们所需的全部机制；任何具体行业的词汇都不进入内核的协议、端口和通用引擎。

| 归属 | 内容 |
|---|---|
| 内核定义 | `model`、`model_provider`、`model_catalog`、`harness`、`sandbox`、`tool`、`permission_profile`、`workflow`、`delivery_type`、`spec`、`standard`、`role`、`capability`、`agent`、`skill`，以及会话、工作流引擎、权限、存储、对象、证据门禁、只读处理器运行时、客户端协议 |
| 内置扩展模块 | 工程：Git、GitHub、测试、代码工作区。它随 Host 发布，通过与其他扩展相同的接入点注册，只有领域包声明依赖时才启用 |
| 领域包 | 流程配置、对象类型、只读处理器、页面、领域自己的工具定义 |

五条原则：

1. **单向依赖。** 领域包依赖契约；内核与扩展模块实现契约。`crates/` 的非测试代码中不出现任何领域包的标识、目录或内容形状。
2. **单一入口。** 内核向领域暴露的所有东西集中在一个目录，文档、Schema 与 Rust 校验同源。
3. **显式协商。** 领域包声明依赖，Host 暴露能力目录，安装期逐项核对。
4. **分层暴露。** 任何领域都需要的、内核随版本提供的、只有部分领域需要的，分三层并分别带版本。
5. **机器守卫。** 依赖方向和分层由检查脚本保证，不靠约定。

不做兼容层。清单格式 1 的包（含已发布的 software-development 0.5.2）被新 Host 拒绝并给出明确原因；软件开发包随内核以新格式重发。

## 2. 分层

| 层 | 内容 | 版本 | 使用者 |
|---|---|---|---|
| L0 协议核心 | 包格式、配置模型、对象类型与关系、工作流执行器、只读处理器、证据门禁、贡献、通用平台工具 | `domain_protocol` 主版本 | 所有领域 |
| L1 内核能力目录 | Host 列出自己提供的平台处理器、内核输入、RPC、worker 运行时 | 随 Host 版本 | 所有领域，用于协商 |
| L2 扩展组 | 工程等按需能力，每组一个标识和版本 | 组自带版本 | 声明依赖的领域 |

## 3. 包格式

清单格式为 2。与格式 1 的差异集中在四处：新增 `domain_protocol` 与 `requires`；工程钩子移入 `extensions`；`core` 的取值改为能力目录中的名称；呈现目标的 `api` 改为 `sdk`。

```json
{
  "format": 2,
  "id": "ecommerce-outbound",
  "version": "0.1.0",
  "domain_protocol": 2,
  "requires": { "host": ">=0.6.0", "handlers": ["workflow.start"], "extensions": [] },
  "files": { "runtime/worker.mjs": "<sha256>" },
  "entity_types": "configurations/entity-types/outbound.json",
  "runtime": {
    "entry": "runtime/worker.mjs",
    "handlers": {
      "ecommerce-outbound.order.validate": {
        "core": "domain.entities",
        "entities": { "types": ["ecommerce-outbound.stock-item:1"], "max_items": 200 },
        "input_schema": {}, "output_schema": {}
      }
    },
    "project_connection": "ecommerce-outbound.project.connect"
  },
  "extensions": {},
  "ui": { "contributions": "ui/contributions.json", "targets": [] }
}
```

| 字段 | 含义 |
|---|---|
| `format` | 常量 2 |
| `domain_protocol` | 与 `/capabilities` 的 `domain_protocol` 主版本一致 |
| `requires.host` | 可选的 Host 版本范围 |
| `requires.handlers` | 包内工具定义和工作流引用到的平台处理器。由构建工具从包内容计算，不手工维护 |
| `requires.extensions` | 需要的扩展组，形如 `engineering.git/1` |
| `requires.rpc` | 可选。页面会调用的领域相关 RPC |
| `runtime.project_connection` | 保留。解析项目接入参数的处理器，任何领域都可能需要 |
| `extensions` | 按 L2 扩展组声明的钩子，见第 6 节 |

其余约束与基线一致：ZIP 不超过 2 MiB，解压后不超过 8 MiB，不超过 256 个文件；逐文件摘要；处理器名、对象类型、贡献 ID 以 `{id}.` 开头。

领域包的配置包清单在安装时套用与出厂资源相同的 Schema，消除两种校验深度。

## 4. 能力目录与版本协商

`GET /capabilities` 在现有字段上增加：

```json
{
  "domain_protocol": 2,
  "package_format": 2,
  "platform_handlers": ["configuration.catalog", "entities.create", "workflow.start"],
  "core_inputs": ["domain.entities"],
  "extensions": [
    { "id": "engineering.git", "version": 1,
      "handlers": ["git.source.publish"], "core_inputs": ["engineering.git.snapshot"],
      "hooks": ["worktree_branch", "pull_request_creation"] }
  ],
  "rpc": ["query.domain.active", "query.domain.entities"],
  "worker": { "image": "sha256:…", "runtime": "node-24", "timeout_seconds": 15, "memory_mb": 128 }
}
```

安装时的核对与错误：

| 检查 | 失败时的错误码 | 错误信息包含 |
|---|---|---|
| `format` 与 `domain_protocol` | `domain_protocol_unsupported` | 包的格式与协议版本、Host 支持的版本 |
| `requires.host` | `host_version_unsupported` | 包要求的范围与 Host 版本 |
| `requires.handlers` | `handler_unsupported` | 缺失的处理器名 |
| `requires.extensions` | `extension_unavailable` | 缺失的扩展组及其版本 |
| `requires.rpc` | `rpc_unsupported` | 缺失的方法 |
| 引用了扩展组提供的源或处理器，但没有声明依赖 | `extension_unavailable` | 被引用的源、它所属的扩展组 |

最后一条是“按需启用”的落点：没有声明依赖 `engineering.tests/1` 的领域，不能发布 `tests.run` 的工具定义，也不能声明 `engineering.git.snapshot` 这样的内核输入。

## 5. 配置模型

15 种源类型、`identity:revision` 引用、根部引用字段、`_file` 正文内联、默认选择均不变。变化在资源的归属：

| 提供者 | 资源位置 | 标识前缀 | 何时可用 |
|---|---|---|---|
| 内核 | `kernel/core/`（建议路径，由 `plugins/core` 改名迁入） | `builtin-` | 始终 |
| 工程扩展模块 | `kernel/engineering/`（建议路径） | `engineering-` | 领域包声明了对应扩展组 |
| 领域包 | 包内 `configurations/` | `{id}.` | 包激活时 |

内核处理器的工具定义随内核发布。`configuration.*`、`workflow.*`、`project.*` 等处理器的定义从软件开发包迁到 `kernel/core/tools/`；`tests.*`、`git.*`、`workspace.*` 的定义迁到 `kernel/engineering/tools/`。领域包只发布两类工具定义：领域自己的，以及采用关系。

## 6. 扩展组

工程钩子从清单顶层移入 `extensions`，键为扩展组：

```json
"extensions": {
  "engineering.git/1": { "handlers": { "worktree_branch": "…", "pull_request_creation": "…" } },
  "engineering.tests/1": { "handlers": { "test_discovery": "…" }, "reporters": { "node_test": "runtime/…" } }
}
```

内核侧对应三个变化：

- core 的 `DomainReader` 端口只保留通用方法：`read`、`invoke_hook(group, hook, input)`、`core_input(name, selection)`。
- 工程模块在注册时声明自己提供的钩子名和内核输入名。内核按组查找，不认识 `worktree_branch` 这样的具体名字。
- 基线中的 `git_head`、`git_snapshot` 更名为 `engineering.git.head`、`engineering.git.snapshot`。

## 7. 处理器运行时

**入口是纯模块。** 领域入口只导出处理器表，不处理标准输入输出：

```js
export const handlers = {
  "ecommerce-outbound.order.validate": (input, core) => { /* 返回结果或抛出 DomainError */ },
};
```

读写标准输入输出、校验、错误封装由内核提供并挂载进容器的引导模块完成。基线中“处理器列表必须按字典序返回”“不能向标准输出写任何别的内容”等隐式规则随之消失。

**错误可以传回。** 处理器可以抛出带代码和信息的错误，Host 将其映射为 RPC 错误 `domain_handler_rejected`，保留代码与信息。验证器拒绝放行不是错误，仍是正常结果 `{allowed: false, reason}`。

**沙箱边界不变**，但写进能力目录，领域作者可以读到：

| 项 | 取值 |
|---|---|
| 网络 | 无 |
| 文件系统 | 只读；只挂载入口与引导模块 |
| 资源 | 内存 128 MiB、1 CPU、48 个进程 |
| 时间 | 宿主侧 15 秒 |
| 依赖 | 单文件，无本地 import 和 npm 依赖 |

**内核输入是开放的。** `core` 的取值来自能力目录：

| 名称 | 提供者 | 选择声明 | 返回 |
|---|---|---|---|
| `domain.entities` | 内核 | `types`、可选的 `related`、`max_items` | 本项目内这些类型的对象，或有界的关系遍历结果 |
| `engineering.git.head` | 工程扩展 | 无 | `{"head":"<oid>"}` |
| `engineering.git.snapshot` | 工程扩展 | 文件与搜索声明 | 与基线的快照相同 |

`domain.entities` 填补基线中“处理器只能拿到 Git 数据”的空白。电商领域用它在处理器里读取库存、仓库和承运商，见 [领域设计](DOMAIN-ecommerce-outbound.md)。

## 8. 工作流扩展点

**执行器不变**：`agent`、`tool`、`workflow`、`control`。人工确认、补充输入、返工与评审策略、调度预算都不变。

**领域工具成为一等工具。** 工具定义新增 `implementation: "domain"`，带领域命名空间下的 `qualified_name` 和 `handler`：

```json
{ "type": "tool", "identity": "ecommerce-outbound.tool-order-validate", "revision": 1,
  "data": { "qualified_name": "ecommerce-outbound.order-validate", "implementation": "domain",
            "handler": "ecommerce-outbound.order.validate", "input_schema": {}, "output_schema": {} } }
```

它既可以作为工具执行器步骤，也可以被 Capability 采用后由 Agent 直接调用。基线中唯一的入口 `domain.read` 及其固定的 `{handler, input}` 参数随之移除。

**证据门禁是通用的。** 基线只读取 `git.delivery` 一个键并写死了输入字段。目标形态：

```json
"operation_evidence": {
  "order-confirmation": {
    "verifier": "ecommerce-outbound.order.authorize",
    "operations": ["ecommerce-outbound.order-confirm", "$completion"],
    "context": { "order_id": { "from": "owner", "pointer": "/id" } },
    "sources": { "validation": { "source": { "step": "validate", "port": "result", "pointer": "" } },
                 "approval": { "source": { "step": "review", "port": "result", "pointer": "" }, "human_confirmation": true } },
    "checks": { "ecommerce-outbound.order-confirm": ["validation", "approval"], "$completion": ["approval"] }
  }
}
```

| 项 | 含义 |
|---|---|
| `verifier` | 本包注册的处理器 |
| `operations` | 被守护的操作：工具的 `qualified_name`、步骤的 key，或 `$completion` |
| `context` | 从归属对象或工作流输入按指针取值的字段，取代基线中写死的字段 |
| `sources` | 事实来自哪个步骤端口，可标注它须是平台操作或人工确认 |
| `checks` | 每个操作在执行前必须具备的事实名 |

触发时机：步骤派发前，以及工作流完成前。验证器收到 `{contract, operation, context, facts}`，返回 `{allowed, reason}`。拒绝时步骤进入阻塞并保留原因，事实没有变化之前不自动重试。`facts` 条目有 JSON Schema，并与 Rust 类型做一致性测试。

**工作流输入的提供者是声明出来的。** 基线中内核按名字自动填充 `test_configuration`。目标形态由工作流输入声明 `provided_by`，取值为扩展组；内核只为声明了提供者且扩展已启用的输入自动填充。

**对象与归属。** 内核的工作归属只有两种：项目和领域对象。Feature、Task 不再是内核类型，它们成为软件开发包的对象类型。为此新增一个通用平台工具 `entities.create`，供 Agent 在归属对象下创建声明类型的对象并建立关系；确切语义在工程迁移的设计阶段结合软件开发包确定。

## 9. 对象与项目

| 接口 | 状态 | 说明 |
|---|---|---|
| `domain.entity.put`、`query.domain.entity` | 不变 | 人工读写，类型不可变，工作流提交的对象不可覆盖 |
| 对象类型、关系、工作流绑定 | 不变 | 关系有类型与基数，不成环；工作流完成时用已接纳的产物创建交付对象 |
| `query.domain.entities` | 新增 | 按类型分页列出对象，附带在途与最近一次工作流的摘要。任何领域的工作队列都需要它 |
| `domain.project.create` | 变更 | 接入信息对内核不透明，由 `project_connection` 处理器解析，内核只保存 |
| `domain.project.remove` | 变更 | 没有在途工作流的领域项目都可以移除 |

项目是工作范围：标识、标题、所属领域、存储位置和配置。仓库、主分支这些是工程扩展对接入信息的解释，不是内核的概念。

## 10. 领域包的构建与脚手架

**SDK 制品。** 内核构建额外产出一个制品 `devdesk-domain-sdk-<版本>`，包含：

| 内容 | 用途 |
|---|---|
| `schemas/` | 包格式、贡献、对象类型、配置源、RPC 定义的 JSON Schema |
| `worker/bootstrap.mjs` | 处理器引导模块，与 Host 内置的是同一份 |
| `web/` | Web SDK 与组件套件的模块和类型声明，见 [目标 Web SDK 契约](CONTRACT-web-sdk.md) |
| `cli/` | `devdesk-domain` 命令行工具 |

**领域目录是声明式的。** 每个领域包根目录有一个 `domain.json`，描述标识、版本、依赖、处理器声明、扩展钩子和呈现目标。配置源仍然按类型放在 `catalog/`。软件开发包现在的 `build.py` 在代码里生成配置源和清单，这些内容改为普通的目录文件。

**`devdesk-domain` 命令：**

| 命令 | 作用 |
|---|---|
| `init <id>` | 生成目录骨架、最小处理器、最小页面和构建配置 |
| `check` | 归属与引用闭合、命名空间、引用索引、Schema；计算并回填 `requires.handlers` |
| `build` | 生成 ZIP；安装前校验的全部规则在此执行一遍 |
| `verify --url <host>` | 连接运行中的 Host，按其能力目录核对 `requires` |

**构建独立。** 领域包的构建只需要 SDK 制品、领域目录、Node 和 Python。不需要 `crates/`、`web/src`、`schemas/`、`scripts/`。CI 中用一个只提供这些输入的独立作业来证明。

引用索引算法只保留一份实现，位于 `devdesk-domain`。内核现有的 Python 检查脚本改为调用它。

## 11. 与基线的对照

| 主题 | 基线 | 目标 | 审计项 |
|---|---|---|---|
| 内核对领域包的引用 | 两处字面引用，多处概念泄漏 | 零 | A1、A2、C、D |
| SDK 入口 | 七个切面，无文档 | 本契约与 SDK 制品 | P2 |
| 版本 | 两个整数 | 协议主版本加能力目录，安装期核对 | — |
| 工程钩子 | 清单顶层字段 | 扩展组 | C1、C3、C4、E5 |
| 内核输入 | 只有 Git 两种 | 开放，通用与扩展各自注册 | C2 |
| 处理器入口 | `domain.read` | `implementation: "domain"` | — |
| 证据门禁 | 只认 `git.delivery` | 任意操作，输入由合同声明 | D1、D2、D3 |
| 处理器错误 | 信息被丢弃 | 结构化错误传回 | — |
| 工具定义归属 | 40 个内核处理器的定义在领域包里 | 随提供者发布 | D7 |
| 对象列举 | 无 | `query.domain.entities` | — |
| 项目 | 按 GitHub 校验 | 接入信息不透明 | C5 |
| Feature、Task | 内核类型 | 软件开发包的对象类型 | D6 |
| 引擎里的工具名特判 | 十余处字面量 | 工具语义改为定义属性 | D4、D5 |
| 构建与检查 | 读取内核内部文件，脚本写死目录 | SDK 制品与命令行工具 | F1、F4 |

## 12. 留到实施时确定

- 工程扩展模块的 crate 划分和模块注册接口的确切形状，在工程迁移的设计步骤中结合电商领域的反馈确定。
- `entities.create` 的参数、权限与幂等语义。
- `domain.entities` 中关系遍历的深度与预算。
- 处理器运行时的延迟。每次调用启动一个新容器，是否需要常驻 worker 属于性能议题，不在本计划内。
