# Standard 不可变规范快照实施与验收记录

日期：2026-11-11。状态：代码实施完成，真实双 Task 工程验收进行中。

实施工作区 `/Users/hengliu/.codex/worktrees/standard-snapshot/devdesk`，分支 `codex/standard-snapshot`，起点 `b70cd10`。与 main 上的 Delivery 改造 `269150b` 整合后，首次合入 main 为 `79d7d96`；后续验收修复继续从该 worktree 提交并合回 main。用户要求的统一测试在 main 执行，原 main 中未跟踪的 `review/` 保留。

## 完成的实现

Standard v4 只保存 `schema_version` 和确切 `root_workflow_refs`。可重建的闭包仅包含 Workflow、Delivery、Spec 及声明关系；根是解析锚点，已包含的子 Workflow 可作为工作入口。源 Scope 只是存储与访问边界，不是 Project / Feature / Task 的 Standard 类型或适用规则。相同规范闭包在不同访问范围内有相同摘要，源封套具有不会跨目录碰撞的身份。

删除独立正文、基线覆盖规则、额外校验层及其编辑路径。逐字段比较按稳定 Step、检查项定位；运行中产生的 Workflow 源通过保留的节点身份配对，存在歧义时明确显示新增与移除。比较不推断规范更严格、等价或实际验收通过。

工作采用统一记录为 `work_configuration`，使用真实 `WorkReference`。默认值与实际执行依据分离；同一 Feature 中的 Task 可以选不同 Standard、Workflow、Agent 和 Permission Context。子调用通过显式 `work_binding` 使用真实 Task 身份，独立编译自身闭包并检查父子输入输出合同。普通内部调用不会被误判为工作的根执行或新的工作采用历史。

运行切换沿 preview / apply、CAS、授权、等待旧执行停止和原子生效路径执行。旧计划、原始输入、会话、产物和首次调用事实保留各自引用；输入或 Agent 绑定变化不改变 Standard。运行例外属于执行事实，不写入 Standard。

页面提供生成、只读详情、比较、归档和使用位置。Feature 与 Task 共用采用组件；工作比较可跨项目，分别展示自身规范、实际入口和子调用依据，不切换当前项目。完整源标识置于只读详情内，列表显示名称及修订；已检查桌面及 390px 移动宽度。

源基线为 Global SQL 16、Project SQL 47，数据集标记 `devdesk-source-v2-standard-v1`。Workflow 数据合同 v6、预置源修订 6（当前文件 v7）；Permission Context 源修订 3；Standard 源修订 3、数据合同 v4。PM 使用独立 Capability 4、Agent 5、编排 Skill 2，其他职责保留 Delivery 改造的 Agent 4 及各自 Capability。旧数据目录不会自动迁移或改写。

## main 的统一检查

| 检查 | 实际结果 |
|---|---|
| Rust workspace | 221 项通过，15 项按原有运行条件跳过。日志 `/tmp/devdesk-standard-main-rust-final-20261111.log`。 |
| 追加的稳定字段比较 | Domain 全部 13 项通过，其中新增 2 项；日志由本次 main 执行输出保留。 |
| 运行根与执行链复验 | `configured_workflows` 35、`orchestration` 30、`standard_snapshots` 5 全部通过；日志 `/tmp/devdesk-standard-main-runtime-final-20261111.log`。 |
| Host | 29 项通过；追加通用冻结工作 Agent 路径后，相关 Host 与 Standard 用例再次通过。 |
| Web | TypeScript / Vite 构建通过；debug 46 项、release 46 项全部通过。日志 `/tmp/devdesk-standard-main-web-{debug,release}-final-20261111.log`。 |
| 最终相关页面 | Standard、讨论 Agent 选择等 6 项 release 浏览器用例通过；日志 `/tmp/devdesk-standard-main-web-last-20261111.log`。 |
| HTTP / 独立 Host | debug / release 鉴权、连接、生命周期、模板重置、重启与独立二进制通过。 |
| 数据与运行准备 | 新目录初始化、旧数据保留、克隆中断恢复、按 Harness 准备与复用、失败重试、内嵌配方通过。 |
| 真实 Docker / Git 本地验收 | 实际 Red 断言失败、Green、候选测试、3 份 Test Report、独立 Test Doc、固定对象 Diff 通过；无模型调用、无远端发布。 |
| 独立发布包 | 校验 7 个不可变后端文件、3 个权限预置及 Schema；脱离源码启动、真实材料工具执行、纯 Standard 源、归档和重启恢复通过。最终包校验另记录 SHA256。 |

Rust 与浏览器的 Harness 夹具明确属于离线回归，不计为真实模型工程闭环。真实 Docker 的 Red / Green 使用本机已有的固定 Node 镜像 `sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f`。

验收修复包括：跨项目源视图使用实际工作访问范围；控制策略重建时规范化；JSON 编解码拆分大型递归栈帧；只读 Workflow RPC 正确路由；归档快照链接刷新恢复；工作根与普通内部调用分离。对应回归均在 main 验证，没有改写旧执行结果来通过检查。

## 真实双 Task 工程验收

入口 `tests/live/probe_standard_snapshots.py`。数据、日志、JSON 与截图位于独立临时目录，不放入计划目录。专用仓库 `xfork24/devdesk-integration-test`，真实 Codex 0.167.0、Docker、Git / GitHub。

当前有效运行证据目录 `/tmp/devdesk-standard-real-20261111-c`。已完成需求、独立需求评审、工程约束、设计、独立设计评审、规划、独立计划评审，实际创建 Add 和 Twice 两项 Task。

- Feature：`5416fa95-88cc-4871-bbaa-1daf3e990a7e`。
- Add：`0077fb77-0cc3-4a78-ba9f-b4593cef459b`，实际入口 `builtin-workflow-task-sol-high:6`，原 Standard 为 `standard.d7015ba25912e198bbf3061f21393b77b981ad8166663b95c819a36a8a8f493e:1`。
- Twice：`257367ea-4e82-4df8-b02a-5a0450bc6296`，实际入口 `builtin-workflow-task-sol-high:7`，Standard 为 `standard.88c600e58aac3445ec427f66fbc1eccdd993e5e48e3d000532472b086167431e:1`，包含额外的 `standard_b_trace` 检查。

两个子实例已按各自配置创建，尚待本次真实运行完成独立测试快照、Add 的规范切换、历史依据重启核验以及完整 TDD、评审、候选测试和 Git 发布。此处没有把仅配置成功或离线端口断言记为真实闭环通过。

该运行在 main 的已合入版本启动；计划中的历史恢复重启将使用最终签名 release。`context.json` 保留最初与重启二进制 SHA256。排队管理轮次自然完成，未执行额外取消或改写业务记录。

此前隔离运行 a 定位到控制策略重建问题，b 定位到 debug JSON 源视图栈溢出，均已修复并保留失败证据；它们不作为通过证据。真实验收完成后在本记录补充确切计划、执行与测试 / Git 回执数量。
