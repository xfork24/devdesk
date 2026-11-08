# R6 后续实施笔记（不是验收报告）

已写入并实跑：TestRecipe (首批 node_test、固定文件/镜像/超时)、CommandTests 异步 Docker 执行、可信 Node reporter、固定 Git blob 导出与内容漂移核验。`cargo test --locked -p devdesk-adapters --test command_tests -- --include-ignored` 2 passed，实际覆盖 Red 断言失败、Green 成功、缺失模块/语法环境失败、强制停止、重复回执和清理。尚未接入正式 Run DAG，不能据此宣布 R6 完成。

下一步拟沿用现有 Attempt/Run/Delivery/Review 内核，不能建另一条跳过 Review 的脚本流程：

1. Host 为 live `attempt.start`/`review.request` 解析并冻结每个实际角色 Session，先查原命令 replay；拒绝客户端注入内部 session/binding。规划用 Analyst/Architect/Tester 只读；实施用 Tester(test_write)、Developer(task_write)，docs 只读；正式评审使用独立 Reviewer(read_only)。保持每 Task/角色/配置独立 Session，安装层引用不复制。
2. Attempt 增加不可变的 stage→Session 映射。live Run 引用这些 Session；不覆盖 legacy Session.instance。真实运行实例追加 RuntimeExecution(run_id)，测试实例用真实 TestRecipe 镜像。测试证据的 environment 从对应 RuntimeExecution 定位，旧 Sim 路径继续读原 instance。
3. 模型 Run 通过现有 ConversationTurn 协调器派发，Turn.run_id 关联实际 Run；避免 Run 与同一 Turn 重复占并发额度。Run 结果只能来自完成且 writer revoked 的真实 Turn。测试节点直接经 CommandTests 的异步 start/poll/stop，不能先调用模型再把输出当测试事实。
4. live planning 是对已采纳 Requirement/Design/TaskPlan/TestPlan 的正式校验，不直接把聊天自由文本写成工程状态；保留 Requirement Delivery Review → Design/Task/Test Plan Delivery Review → initial Snapshot → TaskPlan 实例化。
5. 实施 DAG 添加 test_author(Tester) → red → implement(Developer) → green；docs 为只读结构化交付，verify 复验同一代码对象。测试作者仅能写 tests/；开发者仅能写所属 Task worktree。Docker 根目录只读，tests/ 通过明确覆盖挂载授权，Codex/DSH 内层权限与外层挂载一致。Native Harness 工作目录需要支持受控真实 Task worktree；讨论、评审仍用冻结只读代码导出。
6. 测试作者结束并证明停止后，宿主 Git 封存 tests-only commit 作为实际 Red baseline；实现结束后封存候选 SHA。Git snapshot 用 EffectRecord 意图/回执，适配器操作 ref 核对；更新对应 Task GitHead，外部调用不在 SQLite 事务内。绿灯和 verify 必须测试同一实现 SHA，docs 不改变该 SHA。
7. make_delivery / produce_documents / review / snapshot / Run 日志去除无条件 simulated 标记，按真实来源保存；不能修改旧 Sim provenance。原有 validate_binding 的 simulation-only 分支要扩为明确 external Harness+Docker 能力检查，并按项目 mode 再门禁。
8. 取消需同时停止模型/测试实际执行者；RuntimeExecution 未证明 writer revoked 时，后继写 Run 被阻塞。重启只核对、停旧资源，绝不重发旧输入。正式 Reviewer 用冻结 Delivery/Spec/候选与逐项结构化检查，身份根与作者不同。

然后实现 R7 GitHub PR 操作账本、候选验证和本地 CAS+远端精确 lease+间接 merged 核对，再执行完整 A01–A22。当前 live `attempt.start` 仍明确阻塞，避免误用 Sim 路径。

## 当前接线准备

项目 schema 已追加至 v28：Attempt.live_sessions / source_base、RuntimeExecution.kind、Snapshot.execution_instances、Document.test_recipe。`live_runs.rs` 已有 live_binding / validate_live_sessions / add_live_run；Host 已有 resolve_live_attempt 和可供内部工程入口调用的 open_role_session，但 resolve_live_attempt 尚未接入 RPC，正式 live attempt.start 仍被显式门禁阻塞。

RuntimeBinding 新增 writable_paths。SessionStart 新增 CodeView：只读阶段导出固定 Git blobs，写阶段只能绑定所属 Task 的实际 worktree，Tester 只写 tests/<suite>/ 的明确目录。Docker 为测试目录提供局部 RW 覆盖，屏蔽 .git/.codex/.agents，HOME 指向私有 state；Codex granular approval 禁止 sandbox/network 权限扩张，禁用项目本地配置/插件/hooks/多 Agent。DSH 将测试作者 cwd 收窄到对应测试目录，并使用固定 bwrap + --unshare-net。真实嵌套沙箱局部写用例现已通过；较早失败为 Codex 初始化保护路径时遇到只读挂载，未被当作通过。

下一步具体接线建议：

- 让正式模型 Run 通过带 run_id 的 ConversationTurn 派发；在 Turn 增加不可变 FrozenCode(context,revision)，Coordinator 将其交给 SessionStart.code_view，RuntimeExecution.run_id 从 Turn 传递。
- 单独的原生 Run 和它的关联 Turn 只占一个并发名额；公开 conversation.send 拒绝工程专用工作类型及正在由正式 Run 使用的 Session。queued Run 若 Session 有其他活动 Turn 则等待。
- Git 写入封存可扩展 RepositoryAction::Snapshot{run,context,expected,allowed_paths} / Receipt::Snapshot{revision}，复用仓库意图/回执账本，避免再造不持久的 commit 调用。TestAuthor 结束后封存 Red baseline，Implement 结束后封存实际候选并更新 Task GitHead。CliGit 在提交前校验 staged paths 只能落在冻结写路径内。
- CommandTests.poll_test 已可异步执行；正式测试 Run 需要创建 kind=tests 的 RuntimeExecution，environment=该实例 ID，镜像=TestRecipe.image，使用真实 Session ID 做容器归属。旧 Sim 测试仍用旧 Session.instance；真实测试证据改查对应 RuntimeExecution，不能覆盖共享角色 Session.instance。
