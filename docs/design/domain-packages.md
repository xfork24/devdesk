# 领域包与客户端扩展

DevDesk v0.5 独立交付 Rust Host、Web Core 和领域 ZIP。Host 带有 Model、Harness、Docker Sandbox、Git 和领域无关的工具、权限预设及领域模型。业务部署可从空领域开始，在运行时安装所需的 ZIP。

## ZIP 内容

| 内容 | 职责 |
|---|---|
| `manifest.json` | 包 ID、版本、格式与协议版本、依赖声明、文件 SHA-256，以及可选的扩展绑定、运行时或呈现目标 |
| `configurations/` | 不可变源定义和默认选择，以及实体 Schema、工作流绑定和引用索引 |
| `runtime/` | 可选 JavaScript 处理器、输入输出合同关联和运行辅助文件 |
| `ui/` | 可选客户端贡献声明与目标专属资产 |

格式由[领域包 Schema](../../schemas/domain-package.schema.json)、[对象定义](../../schemas/domain-entity-types.schema.json)和[客户端贡献](../../schemas/domain-contributions.schema.json)共同描述。源码分别位于 [plugins/core](../../plugins/core/manifest.json) 和 [plugins/software-development](../../plugins/software-development/README.md)。软件开发包的 Feature、Task 使用冻结证据授权，仓库识别、分支命名、PR 文案、测试报告和页面入口也随包发布。

## 依赖与协商

领域包使用清单格式 2、`domain_protocol` 2，并在 `requires` 中声明依赖：最低 Host 版本 `host`、调用的平台处理器 `handlers`、需要的扩展组 `extensions`，以及可选的 RPC `rpc`。`GET /capabilities` 以同样的名字列出本 Host 提供的全部内容：`package_format`、`domain_protocol`、`platform_handlers`、`core_inputs`、`extensions`、`rpc`。`core_inputs` 是每个包都能用的内核输入；`extensions` 的每一项是一个扩展组，带自己的 `handlers`、`core_inputs` 与 `hooks`。构建工具从包的实际引用推导 `handlers`。

安装时先核对格式与协议，再逐项核对依赖。失败时错误指明缺什么：

| 错误码 | 含义 |
|---|---|
| `domain_protocol_unsupported` | 包的格式或协议版本不是本 Host 接受的版本 |
| `host_version_unsupported` | 包要求的最低 Host 版本高于当前版本 |
| `handler_unsupported` | 本 Host 没有列出的平台处理器，或处理器声明了能力目录里没有的内核输入 |
| `extension_unavailable` | 本 Host 没有列出的扩展组；或包使用了某个扩展组提供的工具、内核输入、钩子与资源，却没有声明依赖该组 |
| `rpc_unsupported` | 本 Host 没有列出的 RPC |

Git、GitHub 与测试属于扩展组 `engineering.git/1` 和 `engineering.tests/1`。只有声明依赖的包才能发布这些处理器上的工具定义、把处理器列为交付的生产者、声明该组的内核输入（Git 组为 `engineering.git.head` 与 `engineering.git.snapshot`），或在清单的 `extensions` 下把处理器绑定到该组的钩子。内核输入以能力目录中的名称书写，名称带着提供它的扩展组。钩子与资源集合由内核登记：`worktree_branch` 与 `pull_request_creation` 属于 Git 组，`test_discovery` 与 `reporters` 资源属于测试组。

处理器在 `core` 中声明一个内核输入，内核在调用前准备好数据，放进请求的 `core` 字段。每个包都能用的内核输入列在 `/capabilities` 的 `core_inputs`，目前是 `domain.entities`：处理器用 `entities` 选择要读取的对象类型和数量上限（`types`、`max_items`，默认 100，最多 200），类型必须由本包声明。内核返回该项目中这些类型的对象，每条带有与 `query.domain.entities` 相同的 `workflow` 摘要；超过数量上限或总大小超过 512 KiB 时停止，并用 `truncated` 说明有对象没有给出。扩展组的内核输入随组发布，例如 `engineering.git.head`、`engineering.git.snapshot`，包需要先声明依赖该组。

领域包发布两类工具定义。采用内核工具时 `implementation` 为 `platform`，处理器须在本 Host 的能力目录里，并列入 `requires.handlers`。领域自己的工具 `implementation` 为 `domain`：名称在包的命名空间下，`handler` 指向包内注册的处理器，输入 Schema 就是处理器的输入。它既可以作为工作流的工具执行器步骤（参数按处理器的输入绑定），也可以被 Capability 采用后由 Agent 直接调用：调用名是 `devdesk_` 加上把点换成下划线的工具名，经与平台只读工具相同的准入，由领域处理器在沙箱里执行。领域工具在 Agent 的冻结工具锁里单独成一类，既不算内核工具，也不算接入工具。基线中唯一的入口 `domain.read` 及其固定的 `{handler, input}` 参数已移除。

工作流的输入可以声明提供者：输入端口上写 `provided_by`，取值为扩展组，例如 `engineering.tests/1`。启动时内核只为声明了提供者的输入自动取值。提供者必须是该扩展组登记提供的输入（`GET /capabilities` 里每个扩展组的 `provided_inputs`），否则发布工作流时返回 `invalid_configuration`；项目所属的领域包必须声明依赖该组，否则启动时返回 `extension_unavailable`。调用方不能用别的值覆盖内核提供的输入。目前唯一的提供者是测试组的 `test_configuration`，取值为项目选定的测试配置或识别出的配置。输入的名字本身不再触发任何自动填充。

## 安装、激活和版本

安装核对清单、逐文件摘要、引用和包定义。用户可在 Host 运行时显式激活已安装的包。活动目录记录包 ID、摘要和活动代次，整个工作台共享一个活动领域。切换更新代次，客户端重新发现贡献。停用保留已安装的版本，卸载撤掉对应版本的运行入口。切换与停用检查在途工作流和待核对的仓库操作。已移除项目搁置的仓库接入核对可在重新打开项目后继续。

卸载仅在该领域有工作流正在执行时被拒绝。排队、阻塞、等待输入或审批，以及执行已停下的暂停流程均可卸载。卸载保留流程修订、项目数据和仓库操作记录。

核心工作台读取核心配置与未绑定领域的项目。领域工作台携带活动 `domain_epoch`，读取当前包的源修订和同领域的项目目录。项目归属使用清单中的领域 ID。打开 GitHub 仓库时按领域 ID、仓库和主分支复用项目，包括曾被移除但保留数据的项目。同一领域的各版本共享项目、对象和历史记录。项目登记保留原包摘要及领域身份，卸载后仍可恢复归属。旧登记可从已登记的外部数据目录恢复领域身份，也可通过导入原 ZIP 恢复。

项目的接入信息对内核不透明。`domain.project.create` 带 `connection` 时，活动包的 `project_connection` 处理器解析它，返回的结果原样保存为项目的 `connection`，领域页面从 `query.workspace` 读回。只有声明了 `engineering.git/1` 的包，内核的工程部分才从接入信息里读出仓库与主分支，据此登记仓库、复用或接管 GitHub 项目；这部分随工程扩展模块迁出。`domain.project.remove` 对任何领域项目可用，项目里有在途的领域工作流或待核对的仓库操作时拒绝。工程库版本为 63，从 62 及更早版本的离线升级会补上保存接入信息的列。

选定项目之前即可读取包内修订。项目建立时采用包内源定义，执行使用冻结的源修订和交付约束。仅供领域对象工作流使用的源定义由对象入口调用，日常配置目录只列出可选配置。软件开发的 Repository Preflight 属于仓库对象入口。更新 ZIP 使用新版本；同 ID、同版本的不同内容会被拒绝。现有早期包的离线回切检验使用真实历史 ZIP 夹具。

包内 Role 和用户在领域工作台创建的 Role 新修订都按领域显示。旧 Host 发布到全局库的包内 Role 副本仍可由所属领域工作台读取。

从旧数据集接回的项目库可能带有历史执行所引用的全局配置修订。领域工作台的当前目录以 Host 和活动包的定义为准，这些旧副本按历史修订读取。项目级配置仍按自身作用域显示。

领域数据目录位于对应领域工作台设置页的「数据与运行」区块末尾。通过 Web SDK 的 `DomainLayout` 挂载的工作台在 `main.settings-page` 内用 `data-domain-storage-outlet` 标记此区块。目录设置按领域包 ID 保存在 Host 的领域登记库，空值表示使用 Host 数据目录。配置独立目录时选择空目录或已登记给同一领域的目录。更换 Host 时可沿用原目录，标记按领域 ID 校验，项目数据库在打开时取得独占锁。新项目在绑定领域包时固定存储位置，工程库、仓库、执行记录和产物位于该目录的 `projects/<project-id>`。保存设置时，Host 接回已移到所选目录且身份匹配的已登记项目。设置页列出仍有项目数据库的可用目录。Host 启动或保存设置时，清除已移除项目指向不存在的旧目录的存储路径，保留项目登记。Host 数据目录保存领域 ZIP、项目索引、共享设置和凭据。架构重构前的数据集可先归档，再用腾出的目录承接新领域项目。

## 运行端与前端

独立 Host 可以同时配置同一领域数据目录，各自使用自己的全局设置和凭据。项目库的独占锁由打开项目的 Host 持有。目录加载会跳过其他 Host 正在使用的项目，其他项目可照常打开。同一个 Host 支持多个 Web 前端同时操作它管理的项目。

保存外部领域数据目录或启动已有配置的 Host 时，会扫描目录中的项目库。通过数据协议、项目 ID 和本地仓库路径校验的未登记项目，用原 ID 加入当前 Host 的项目列表。当前 Host 已移除的项目保持隐藏。再次保存目录可以重试接入此前被占用的项目。

「打开 GitHub 项目」按仓库和主分支匹配已登记项目。如果没有可复用的登记，会在当前领域数据目录的 `projects/<project-id>` 中查找可兼容的旧项目数据库。将完整项目目录拷回原路径后可保留原项目 ID、仓库和历史。遇到多个匹配目录或仓库路径不符时，Host 提示核对目录。

领域包的运行入口是纯模块，只导出处理器表：`export const handlers = { "<包>.<处理器>": (input, core) => 结果 }`，自身不读写标准输入输出。容器里运行的是内核的引导模块：它读取请求、加载入口、按名称调用处理器并写回结果，激活时的握手也由它应答，所以入口导出的处理器必须与清单一致。处理器拒绝或失败时抛出 Error；带 `code` 属性（小写字母、数字、下划线）的错误连同信息由 Host 以 `domain_handler_rejected` 返回调用方，`data.context` 里是处理器的代码。没有代码的错误记为 `handler_failed`。入口、引导模块和传输本身的问题记为 `domain_worker_failed`。Host 在调用前后按处理器声明的 Schema 验证输入与返回值。验证器的拒绝不是错误，仍是正常结果。

沙箱边界写在 `GET /capabilities` 的 `worker` 里，领域作者可以直接读到：没有网络，文件系统只读且只挂载引导模块与入口，内存 128 MiB、1 CPU、48 个进程，宿主侧 15 秒，入口是单个文件，不能有本地 import 和 npm 依赖。核心 Git、持久化模型、调度与发起外部操作的权限仍由平台管理。

呈现目标声明 `renderer`、它所用的 Web SDK 主版本 `sdk`，以及使用组件套件时对应的 Web Core 版本 `kit`（`major.minor`）。Web Core 只加载 `sdk` 与自身一致、且 `kit` 与自身版本一致的目标，否则提示领域包与 Web Core 版本不兼容。绑定可以用 `home: true` 标记进入领域时打开的首页。Web Core 使用 `web.react` SDK 1 将活动包的 ES 模块装进同一 React 树。组件获取当前项目、导航、RPC 和订阅上下文。领域页共用布局和主题，以包提供的样式表扩展。执行脚本与核心前端共用浏览器上下文，前端资源请求需要匹配当前包摘要和代次。

非浏览器客户端通过 `query.domain.active` 和 `/domain-events` 读取相同代次及贡献目录。它可以用通用动作、字段和数据查询自行呈现，也可以实现包声明的专属呈现目标。项目操作与事件订阅使用相同的作用域规则。原生客户端无需加载 Web 资产。

协议示例见[RPC 参考](reference/rpc.md)，安装与兼容矩阵见[运行指南](run-binary.md)，跨端制品验收见[测试索引](../../tests/README.md)。
