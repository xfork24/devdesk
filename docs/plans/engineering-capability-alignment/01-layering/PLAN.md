# S1：分层与应用边界

状态：待实施。依赖：无。目的：让每个入口都可复用同一应用规则，为后续开放 Rust 驱动和经理工具打基础。[总体路线图](../PLAN.md) · [现状证据](../ASSESSMENT.md)

## 1. 交付边界

保持四个主要 crate；`adapters` 命名无需为单复数调整。物理依赖门禁覆盖正常依赖、构建依赖和本仓库 crate 的间接依赖；dev-dependency 若确需例外必须列明，不能通过测试依赖将生产规则倒置。

| 层 | 应保留/承接 | 应移出 |
|---|---|---|
| domain | 配置/执行类型、精确引用、不可变与状态不变量、纯图算法 | RPC 方法名、出厂资产、数据库编码、供应商协议行为 |
| core | 用例、Actor/授权、配置选择/编译、冻结解释、调度、回执接纳、ports | 具体资源路径、内嵌出厂清单、JSONL/SQL/HTTP 和真实环境操作 |
| adapters | SQL/文件 codec、资产获取、进程/容器、协议映射、权限后端编译与事实探测 | 业务计划编译、业务任务渲染、直接裁定业务成功 |
| host | 命令/查询映射、主体来源、项目路由、生命周期、装配与 Web 嵌入 | Agent 组合规则、权限默认规则、业务状态直接写入 |

## 2. 设计

### 2.1 统一用例与主体

将现有命令/查询保持为稳定外观，内部提取 Configuration、Session、Workflow、Project、Runtime 等应用服务。Engine 是协调外观，不再依靠一个巨大 match 加 Host 特殊分支维持能力。Store/Artifact ports 与现有执行 ports 在 `core/ports/` 下按职责归集。

建立可信 `InvocationContext`：主体为人类、控制器、Rust 驱动或业务执行者，附项目/归属、会话/轮次、epoch、权限来源、关联 ID。RPC 参数只能给业务参数；Host 根据已验证会话构造人类上下文，Agent 上下文从真实工具调用及冻结身份解析。先迁移现有 Human/Controller/Segment，S4 再补完整 Rust Driver。不能将客户端字符串转换成任意可信主体。

保留 command_id/原请求指纹与幂等重放顺序；解析后的配置绑定不能改变原请求重放语义。所有入口共同走授权、预期修订检查和同一个事务用例；无外部效果的查询/预演不应创建业务执行记录。

领域对象接收已经分配的 ID/时间；将 `domain::id()` 的环境随机分配移出 domain，core 经可注入的 ID/Clock 来源组织生成。事务重试和恢复复用已持久化的动作身份，不每次生成新身份；测试可以控制时间，而无需真实等待租约或预算过期。

### 2.2 持久化与派生边界

将 `source_views` 拆成明确的存储描述与应用重建两部分：

- Adapter 负责读写原始源修订、选择描述、实际输入和回执；只做编码/解码与存储完整性检查。
- core 接收一个已经加载齐全的事实集合，按精确引用重建计划、任务和只读视图；所需规则为纯函数，不重新读取 Store，不发起事务或外部工具发现。
- 若需新的存储 DTO，在 core 的持久化 port 边界定义；不让领域类型携带 `$source`、SQL 行或路径约定。不为存放 DTO 新建一个无职责的共享 crate。
- 继续保存冻结源引用、实际输入和必要的解释器身份；派生重建不能根据当前默认/最新版重新选择资源。出现不能重建的记录必须报出缺失来源，不能静默替换为当前渲染结果。

### 2.3 出厂资产与运行资源

将 `core/build.rs` 中资源收集与嵌入移至 host 的打包入口，资产读取/路径处理在 adapter loader。core 接收已解析的 `ConfigurationBundle`，执行与 UI 发布相同的领域/组合验证。产品仍可出厂内嵌默认包，但 core 不再绑定该具体目录，也不从 tests 导入生产构建代码。

资源 schema 的构建期验证继续保留，放到产品打包/检查任务。把可复用 schema 工具移到合适的构建支持位置；不用“迁移到 Host”掩盖另一套运行时业务验证。

## 3. 工作项与检查点

| 工作项 | 涉及位置 | 完成依据 |
|---|---|---|
| L01 建立调用/状态写入清单及依赖门禁 | Cargo.toml、core ports、host workspace/governance | CI 检查所有本仓库 crate 依赖；domain 无环境 I/O |
| L02 搬迁出厂包 loader 与构建验证 | core/build.rs、core/templates.rs、host 打包 | core 单独构建无需 resources；独立 Host 包仍可初始化默认 |
| L03 统一 Actor、命令与查询入口 | core/lib.rs、controllers、host 路由 | UI 与工具调用可带真实主体复用用例，参数不能伪造主体 |
| L04 迁移 Agent/会话/默认权限解析 | host/workspace.rs、governance.rs、core services | Host 只获取外部事实并调用 core；原绑定摘要和错误语义不回归 |
| L05 拆分持久化 codec 与派生解释 | adapters/source_views.rs、source_store.rs、core | 读库不调用编译/任务渲染；重建是可独立测试的纯函数 |
| L06 收敛其他 Host 业务写入及命令分类 | experience、templates、runtime receipts、domain/template_catalog.rs | 无 Host 直接改 Snapshot/业务 Event/配置/绑定终态；技术偏好/连接状态可仍在 Host |
| L07 按用例整理模块与现行文档 | core、docs/design.md、storage.md、contracts.md | 稳定公开 API、架构门禁、相关回归与新增 EXECUTION.md |

每次迁移一个用例族并切换唯一入口，不保留两套可写业务实现。新入口适配旧 RPC 方法名是正常路由，不是旧协议回退。

## 4. 验收

- 在不提供出厂资源目录的独立构建环境构建 domain/core；产品 Host 打包后仍无需源码目录运行。
- codec round trip 保持源、选择、实际输入、回执和任务内容摘要一致；恢复路径不使用“当前最新配置”，不依赖 ToolExecutor 可选项差异重新选规则。
- 用同一内存 Store + 可控 ports 验证 Agent 组合/冻结/预期修订/原请求幂等；这些测试归 core，真实 SQLite/文件/协议测试归 adapters。
- 架构检查除 Cargo metadata 外核查 Host 状态写入与 Adapter 的应用服务调用；不用简单 grep 零命中代替人工职责复核。
- 跑受影响的 Rust、HTTP、默认包初始化/重置和 Session 回归；每个有实际行为变化的入口补针对性用例，不写文件移动镜像测试。

## 5. 风险与退出条件

最大风险是编解码重构改变已冻结含义。先用本阶段支持基线的代表性冻结事实建立 round trip/恢复夹具，再切换。跨旧 SQL 代际恢复不在范围内。若无法证明等价，先停在该批次，不能用重新冻结历史或删库绕过。

本阶段不合并 Project/Work 配置语义、不新增调度算法、不承诺 DSH 工具桥；但其公共边界应能承接后续阶段。
