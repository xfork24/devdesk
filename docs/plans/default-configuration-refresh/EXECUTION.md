# 出厂模型与工程规范整理验收

## 出厂目录

新包 `2026.11.23-engineering-standard` 发布 GLM 5.3、GPT-6.2 Sol High、MiniMax M3.1 Flash。默认六个 Agent 通过 Codex 绑定 `gpt-6.2-sol/high`。GLM 和 MiniMax 各有六个 DSH Agent。三个通用 Capability 可供自建 Agent 采用。三套角色绑定共用工程上下文及可复用方法。

新目录包含 14 个 Skill、21 个 Capability、18 个 Agent、20 个 Delivery、14 个 Spec。被快照 v9 使用的 Delivery 和 Spec 全部迁移。相关英文展示名称、Markdown 标题、文件、清单、默认引用及 `reference-usage.json` 同步更新。旧详细版测试所需的源作为显式测试夹具发布到测试目录。

新版默认引用为 `builtin-standard-engineering-delivery:1`、`builtin-workflow-feature-development:1`、`builtin-workflow-task-development:1`。从 `221f0b34` 取前一版源，按精确映射替换引用后逐字段比较。两个 Workflow 和 Standard 的执行数据一致，Stage、Step、依赖、端口、权限、预算、完成条件、返工及人工决定的配置保持基线。20 个 Delivery 的结构和生产者以及 14 个 Spec 的机器检查一致。Tool、Permission Profile 及默认各角色的工具集合完全一致。

## 本地回归

| 验收 | 结果 |
|---|---|
| 出厂目录、生成索引、当前文档链接、权限资产和 Clippy | 通过 |
| 隔离旧版数据集升级 | 保留式恢复更新默认并保留旧定义和自定义模型。完整重置后活动目录精确为三个 Model。两种目录状态重启后均可读取。 |
| HTTP 模板重置、幂等与启动 | debug 和 release Host 均通过 |
| Rust 工作区回归 | 757 项通过，37 项走显式手动或性能验收入口。两个默认 Workflow 编译及配置执行测试通过。 |
| Chromium 配置及 Workflow 回归 | release Host 40 项通过 |
| 本地 Git 与 Docker 交付 | 真实业务 Red、Green、候选测试和 Diff 校验通过 |

本地 4319 Host 已载入新版静态包。读取正式数据集仍为目录代次 5，重置预览指向新包并报告 3 个 Model，两个项目均可打开。操作只切换正在运行的 Host 可执行文件，正式模板重置留待用户执行。

## 真实默认流程

在专用测试仓库 `xfork24/devdesk-default-workflow-test-20261118` 的独立目标分支 `devdesk/configuration-refresh-20261123` 运行。基线为 `a0d68f288dc82899dafb12e0fb8166b160478f61`，根实例为 `670a74c0-4e7f-1e92-f1c0-e79cf32c3ade`。从启动探针到根流程结束耗时约 27 分 27 秒，包含材料审核和确认时间。

- Feature 完成 19 个节点，三个 Task 各完成 14 个节点，共 61 个节点。两个前置 Task 实际并行，第三个从同时包含两次合入的 Git 版本开始。三个 Task 均经历了真实业务 Red 和相同用例的 Green，通过用例数分别为 4、7、8。
- 四个实例各运行源分支和候选分支的 `npm test`，共 8 次。核对独立 Docker 容器、原始测试输出、版本、命令配方、Git 中的测试配置与回执摘要。项目回归实际保留并执行原仓库的 `identity` 用例，三个新增接口的行为由各自独立测试覆盖，并在批准前直接于最终候选再次断言。
- 四次正常人审逐项核对冻结材料后，按本轮用户授权经 `workflow.decision.resolve` 签署。独立评审通过，未使用门禁豁免、人工接管或运行时计划修补。最终 PR 的测试候选、远端合入版本及本地同步版本均为 `7649b8cb0a17013eeb88efa4c3c65f845637061f`。 [Task PR #23](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/23) · [Task PR #24](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/24) · [Task PR #25](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/25) · [Feature PR #26](https://github.com/xfork24/devdesk-default-workflow-test-20261118/pull/26)
- 六类默认 Agent 的全部 18 份原生协议记录均使用 `gpt-6.2-sol` 和 `high`。项目经理已结束，执行者写权限均已撤销，临时认证副本已清理，两个验收 SQLite 库的完整性及外键检查通过。GLM 与 MiniMax 的验证覆盖目录、角色和 DSH 能力组合，真实闭环使用 GPT-6.2 Sol High。
