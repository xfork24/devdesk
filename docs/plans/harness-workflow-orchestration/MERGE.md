# 合并 main 与再次验收

日期：2026-11-11。状态：冲突已解决，完整验收执行中。

合并前 main：`1d2c95e`；实施分支：`codex/harness-workflow-orchestration`（`25e9c53`）；共同基线：`e0826a9`。保留 main 的十个后续提交，在主工作区完成真实合并，不覆盖其中的业务合同更新。

## 主工作区改动保留

合并前未提交的内容为 docs/plans/README.md 的计划索引和未跟踪的 PLAN.md。原文已保存到命名 stash `290bc5f2d5432755fbfad90d7aebcaea5c7262c9`（Preserve main workspace before Harness workflow merge），并独立保存文件归档、二进制 patch 和 SHA-256 清单：`/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-harness-main-merge-e41xpi_0/`。逐一核对 Git stash 原始字节与清单一致。计划正文已包含在合并后的同名文件；待实施状态按已交付结果更新，索引采用当前实施及合并验收记录，没有用旧草稿覆盖已完成的实现。

## 冲突处理

- 调度保留 Harness 显式派发、接纳和完成；同时保留 main 的 failed/paused 取消唤醒及收尾修正。SQLite 待办查询合并 Controller 活动状态与取消请求，不恢复旧的就绪节点自动派发。
- 保留 main 的 JSONL 文本合批、原始协议顺序和终态回执，同时保留 Harness 的具体调用方法错误信息。
- 默认流程升级为 v5，采用 main 最新业务图：Feature 22 步、Task 20 步，保留工程约束、逐增量 TDD、环境及终止元数据，两个写入者之间的 tests_snapshot/red_snapshot，以及评审前的确切候选、快照差异和候选测试。
- main 最新默认确认策略 automatic 保留；它表示不要求 Human 签署，派发、业务接纳和整体完成仍由项目经理使用 Harness 工具决定。默认 Git permit 要求实际候选测试，独立评审仍由 DAG 与完成条件约束。自定义 human 门禁、豁免权限和原始失败事实的保护均保留。
- 出厂 manifest 使用独立合并版本，模板引用继续指向 v5 闭包，按 main 节点索引保留完整 references。默认项目普通创建入口及模板重置均有合同测试。
- 配置回归统一到 v5 显式控制夹具，保留两边新增回归。实例冻结比较纳入 v5 节点身份，不删除 main 对默认图、原始证据和范围评审的断言。
- 工程探针保留 Harness 恢复和完成实例复验能力，采用 main 的真实业务 Red、两个有序增量和运行元数据要求。原始增量的未提交中间快照与平台固定版本证据分别核对；评审的实际任务封套也核对证据来源、候选、差异及测试 SHA。旧实例按冻结端口合同复验，不补造历史字段。

## 本轮验收

执行中。源码、日志、JSON、运行数据和探针产物使用独立临时目录；本目录只保存 Markdown。原有用户运行数据不升级、不删除。
