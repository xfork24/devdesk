# Standard 不可变规范快照实施与验收记录

日期：2026-11-11。状态：实施、main 统一回归、真实双 Task / Feature 工程验收与只读历史审计全部完成。

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

两个子实例已按各自配置执行原生独立测试并固定快照，随后 Add 显式切换为 Standard B。旧计划、输入、输出及原生任务文本经签名 release 重启核验，仍指向原 Standard A；Twice 的初始执行依据为 B。两项实现分别保留两轮真实 Red/Green 和一次回归运行，每条交付运行记录已与原生命令观察对应。两项 Task 的 20 个节点及各自子流程均已完整完成并真实发布；Feature 全量及最终候选各运行 64 项测试，均为 64 通过、0 失败、0 跳过，三份指定测试均包含在配方中，两份 Task 独立测试内容保持冻结版本。Feature 两轮独立评审、最终接纳与发布均已完成，根流程已显式结束。只读审计确认 62 个当前业务节点全部完成，无跳过、无豁免。

该运行在 main 的已合入版本启动；计划中的历史恢复重启已使用当时最终签名 release。`context.json` 保留最初与重启二进制 SHA256。排队管理轮次自然完成，未执行额外取消或改写业务记录。

此前隔离运行 a 定位到控制策略重建问题，b 定位到 debug JSON 源视图栈溢出，均已修复并保留失败证据；它们不作为通过证据。最终通过证据为运行 c 的同数据集完整闭环；早期失败日志继续保留。

真实执行进一步定位并修复了实际 Task 读取所属 Feature 合入目标的边界遗漏：只允许 `workspace.current` 读取其所属 Feature 的确切版本，不扩展快照、测试和 Git 写操作授权。main 新增真实 Git 回归通过，相关 `configured_workflows` 36 项、`standard_snapshots` 5 项全部通过；日志 `/tmp/devdesk-standard-main-scope-regression-20261111.log`。同一数据集在修复后的签名 release 恢复，只重试失败的只读目标查询，父调用自动恢复，保留原 Task 实例。恢复脚本首次严格检查遇到父调用传播的失败后退出，随后改为只恢复实际失败子步骤；该脚本修正没有重建子调用或改写业务结果。

Twice 的 20 个节点及子流程先行全部完成并真实发布。Add 的旧候选随后因同一 Feature 目标已被 Twice 推进而被 CAS 拒绝（`目标已变化`）；这是并发保护，不将旧候选强行视为已接纳。探针原先在任何失败时停止，现允许真实控制者处理该明确冲突，并通过公开 `workflow.step.rework(target)` 在同一实例重新读取目标、构建、测试和评审候选。上游实现、两轮 TDD、旧候选及失败回执保持原记录；没有重建 Task、放宽检查或新增 Standard 变更。Add 已完成新目标读取、候选重建、测试、两轮独立评审、Git 接纳及发布，并以原 Task 实例完整结束。Feature 随后以已测试的精确候选完成两轮独立评审、接纳、发布与根流程结束。

最新独立包由 main 构建并验证通过，报告 `/tmp/devdesk-standard-package-current-20261111.json`、日志 `/tmp/devdesk-standard-package-probe-current-20261111.log`。包内运行程序 SHA256 为 `b81410fa76a49bb297e4525b00f355939581e1e9fdb84cfaf7ab65c7be14669c`；脱离源码启动、权限资产、不可变修订、纯 Standard 源、归档和重启历史恢复均通过，无模型调用。真实工程当前使用私有签名程序 SHA256 `322e143b34b20acf0d2dfa6cc4c49255128499bec498f13c4f0eadc63c0678da`；两次构建之间运行代码无改动，包另做独立验证，不将不同二进制 SHA 混为同一次测试。

## 最终验收结论

`tests/live/verify_standard_history.py` 直接只读核对 SQLite、真实原生命令日志与 Git 对象，不调用模型、不派发新工作。报告 `/tmp/devdesk-standard-real-20261111-c/integrity-report.json`，执行日志 `/tmp/devdesk-standard-history-final-20261111.log`；真实流程日志 `/tmp/devdesk-standard-main-real-20261111-c-resume3.log`。

- 一个 Feature 根实例和两个真实 Task 实例全部完成，当前节点合计 62，全部为 completed，无 skipped、无 workflow_waiver。
- 两项 Task 分别以 A / B 的确切 Workflow 与 Standard 开始原生执行；A 的早期测试、输入及代码快照固定后，显式采用 B。只有这一条 Standard 变更，没有改变父 Feature 的 Standard A。
- 旧计划、3 个早期已执行节点的输入摘要、完整输入、输出引用及输出内容与切换前快照完全一致；初次子调用的 A / B 引用保留。历史原生输入经重启仍指向其原始依据。
- 两项实现各有 2 轮独立 Red/Green 和一次回归运行，所有交付运行记录均能在实际原生命令输出中找到；每对 Red/Green 使用相同命令及测试内容。两项最终独立评审均实际返回 `standard_b_trace: passed`。
- 成功的真实平台测试回执为 11 份，Git 接纳和发布各 3 份。Feature 全量与最终候选均为 64 / 64 通过，0 失败、0 跳过；配方包含两份 Task 独立测试及 Feature 测试，Task 独立测试内容在集成后保持冻结值。
- 三个发布回执的 head、merge_commit 与各自通过测试的 candidate SHA 完全相同；发布事实来自真实平台账本，不根据模型自述推断。

| 工作 | 专用测试仓库 PR | 实际测试并发布的候选 SHA |
|---|---|---|
| Twice | [#37](https://github.com/xfork24/devdesk-integration-test/pull/37) | `e2d8635e1325cd8b825223e0a252aaedce6b407f` |
| Add | [#38](https://github.com/xfork24/devdesk-integration-test/pull/38) | `f2016637ce338e0a4b1ff4abef573c8a5ca45902` |
| Feature | [#39](https://github.com/xfork24/devdesk-integration-test/pull/39) | `6002ecc0e431acf6a5e34796a54141aa29821623` |

发布包为 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`，SHA256 以同名 `.sha256` 文件为准。独立运行验证后，收尾只更新包内 Markdown；最终运行资产字节与权限的一致性、文档提交信息和封包 SHA 记录在 `/tmp/devdesk-standard-package-current-20261111.json`，实际验证时的原包另保留在 `/tmp/devdesk-standard-package-runtime-validated-20261111.tar.gz`。

环境为 macOS / arm64、Chromium、真实 Codex 0.167.0 和固定 Docker 镜像。此记录不把本机检查称为 Linux Host 实机验收。当前实现只支持新的数据集协议；旧数据未迁移、未改写。
