# 跨 Harness 软件工程工作流

2026-11-28，`harness-integration`。

软件开发 ZIP 0.5.3 增加「跨 Harness 软件工程」Standard。Feature 入口固定为 `software-development.workflow-mixed-harness-feature:1`，其 Task 入口固定为 `software-development.workflow-mixed-harness-task:1`。构建时从现有 Feature v6 和 Task v5 派生，保留原有 19 个 Feature 节点、14 个 Task 节点、输入输出、依赖、门禁和 Git 交付证据。原先两个默认入口继续使用原项目配置。

| 职能 | 运行绑定 |
|---|---|
| 项目经理、工作拆解和根调度 | Codex / GPT-6.2-Sol |
| 需求分析 | Pi / MiniMax-M3.1-Flash |
| 架构设计 | OpenCode / MiniMax-M3.1-Flash |
| Task 实现 | mcode / MiniMax-M3.1-Flash |
| 独立测试、阶段与 Task 评审 | DSH / MiniMax-M3.1-Flash |
| Feature 独立交付评审 | 独立 Role，目标为 ZCode。本版登记可见的 Pi 回退 |

需求、设计和评审岗位保留原来的 Spec 与 Skill。Task 测试与实现 Profile 分别改用 DSH、mcode 的原生工具引用，继续保留原文件范围和两项宿主工具。新岗位明确要求最终消息以单个 JSON 封套交付，文档内容放入合同声明的字段。

## 验证入口

- `tests/entrypoints/domain_release.mjs` 核对发布 ZIP、标准树、默认入口、既有门禁、领域回滚及多 Host 恢复。
- `tests/live/probe_mixed_harness_workflow.py` 用证书签名 Host、本机 Codex 认证和实际 MiniMax key。它先验证发布 Standard 可以绑定到项目，再以单独的验收 ZIP 调度五步依赖，核对实际经理工具回执与每步的冻结 Harness、模型、已消费轮次及传递值。可用 `--feature-entry` 观察正式 Feature 到第一道人审。
- ZCode 3.18.2-48 以本机安装单独运行。用实际 MiniMax-M3.1-Flash 审核合成候选，正确指出 `toLowerCase()` 违反保留大小写的验收要求。DevDesk 仍缺少正式 ZCode 驱动及冻结策略的项目级接线，该执行属于独立 CLI 验证。

## Agent 调度验收

签名 Host 从发布 tarball 解包运行。领域包安装、版本回滚、崩溃重启、原项目恢复和多 Host 协作检查均通过。新项目成功保存跨 Harness Standard 与 Agent 模式。

五步调度试验的根实例为 `8f852af1-ffb1-2401-7810-d2ff06302386`。Codex 项目经理使用 `gpt-6.2-sol` 提交 `workflow.segment.dispatch`。Pi、OpenCode、mcode、DSH、Pi 依次交付 11、12、13、14、15，各轮确认使用 MiniMax-M3.1-Flash，并由 Host 消费。Host 依据同一经理委托登记 `workflow.complete`，回执 ID 为 `9a1a2611-eddc-998b-50ae-473f8ad3859f`。本机审计记录为 `review/mixed-harness/agent-latest/result.json`。

正式 Feature 的根实例为 `1c78ab8a-0fe4-a5b6-54cf-ffc919309976`。首轮收到 Markdown 围栏中的 PRD，合同按规则拒收。在需求步骤增加 JSON 封套示例后复跑通过。Pi 完成 PRD，DSH 对确切产物完成独立评审并返回 `approve: true`。流程停在 `requirements_review` 的 `waiting_confirmation`，`source_workspace` 仍待执行。复跑记录在 `review/mixed-harness/feature-entry-latest/engineering-workflow.json`。正式 Feature 的后续阶段及其 Task 子流程尚未实跑。

实跑确认 OpenCode 2.4.1 的 Anthropic 路由需要在基础地址保留 `/v1`。适配器现在补齐该路径且避免重复。定向回归、Clippy 和文档检查通过。`cargo test --workspace --locked` 汇总 790 项通过，39 项按仓库原有标记跳过。

线上验收用独立数据集和合成任务。无真实 GitHub 合入与人工决定。连接 OpenAI 的试验进程沿用本机选定的 Codex 出站，通过临时隔离代理访问，原网络配置不变。

## 0.5.4 与现有项目验收

0.5.4 将最终交付评审改绑本机 ZCode 3.18.2-48。Host 复用 ACP 和 MCP 接线，以固定版本的 ZCode 进程执行。它使用只读评审工具和独立设置，从 Docker 临时目录读取本轮个人 BYOK 配置。原始 ZCode 发行包只用于本机镜像准备。

签名 Host 下，Codex 使用 `gpt-6.2-sol` 完成真实五步调度，工作模型为 MiniMax-M3.1-Flash。最后一步由 ZCode 执行并通过宿主 MCP 读取 DSH 的确切产物。完成凭证和独立的工具回执保存在 `review/mixed-harness/full-foundation/agent-zcode-mcp/`。

在原 `data` 目录，锁定后升级全局库 v24 → v25 和 `devdesk-project-1` 工程库 v61 → v62，保留迁移工具产生的两份旧库。新库与备份均通过 SQLite `quick_check`。按保留自定义配置的入口更新出厂目录，应用原 Profile 并通过 GitHub 连接复用原项目。

正式 Feature 在原项目以 Agent 模式启动。执行前固定仓库的七个 Node 测试文件和本机 jsdom 26.1.0 依赖。Linux arm64 基线共 43 项通过。修正已迁移数据集无法为失效模型凭据重绑的问题，启动前拒绝的凭据检查也记录为尚无运行副作用。Host 允许说明文字后唯一且完整的裸 JSON 或 JSON 围栏，继续按原合同校验输出及证据。

实跑根实例为 `3072b9e7-cfe8-4fc7-c26f-bb9216d05c1d`，Task 子实例为 `2d21157d-cb0b-d35e-cc2e-cea9afb58cdb`。GPT/Codex 派发 Pi 编写 PRD、OpenCode 设计、Codex 拆分单一 Task 及 DSH 独立评审。依次核对冻结的 PRD、设计和 Task 计划及其评审报告，在用户授权下通过原门禁代签。三份批准回执为 `03bddff6-bfc6-0db7-bf8e-17108017718d`、`efec5408-903a-b2a4-1b56-83cba79e8426` 和 `64e29859-0913-a4fb-a8fd-7102f7134c7c`。最终一轮的记录位于 `review/mixed-harness/full-foundation/project-feature-final/`。

DSH 的计划评审工具调用把 Markdown 证据的行号编码成字符串。原合同拒收，评审者保留原核对结论，改用经平台校验的结构化 JSON Pointer，并在报告中记录两份 Markdown 的摘要与对应章节。独立测试作者的容器暴露了按镜像摘要分区的共享缓存接线问题。按实际挂载位置补齐 jsdom 26.1.0 后，以只读源码、断网的 Linux arm64 镜像重跑冻结源码 `e3eedd89f2d5e9271890bb7e79f121f98c4dd076`。正式工作流使用的镜像 `sha256:cfdd13360965d846cf746bf829782d65247e5cb8e986c71271124f908e09673d` 跑完七个文件，43 项通过。基线实测记录在 `review/mixed-harness/full-foundation/baseline-workflow-image-verified/`。

DSH 将独立断言写入 Task 验证目录，预演时修正了切换到英文后重试用例仍期望中文的错误。平台正式 Red 使用冻结快照 `d65562c011f264ab953fa4592a1f1df66030ba9c`。相比源码基线，它只增加这个独立测试文件。运行 `a4498080-27be-40ba-2e23-c07a8f6f5986` 发现 9 项，5 项为旧的中英文空状态触发的业务断言失败，4 项通过。隔离执行返回退出码 1 及停止证明。该回执接纳后才派发 mcode 实现。

mcode 已写入实现后，原 Host 因最终 JSON 围栏后包含交付说明及文件卡片而拒收。唯一的围栏 JSON 和附属文件卡片识别逻辑按保存的完整响应回放验证，并继续拒绝第二个 JSON 候选。写入者停止后更新签名 Host，使用原执行核对入口接纳原 mcode 轮次 `9a82aa54-edaf-3b2e-663a-b00c6b0ccbb6` 的实现报告 `6df9942b-261d-1b5b-aa27-e278cc6ceb3d`。冻结合同和报告正文均照常校验，工作区和 Red 保留，GPT/Codex 恢复派发。

正式 Green 在实现提交 `47dab7e5d10069c5aec94d91c72597833ccbfac4` 上运行 `fd701a45-44a6-f60b-e075-5d0fa12c618b`，9 项全部通过。对照 Git 对象核验 Red、Green 的独立测试字节和配方一致，容器各自独立，测试文件 SHA-256 为 `d8945d9a6bdcbef3386edcd2b006a61c64b30f837b5b30cca3df68519e6d450a`。同一实现提交还通过维护中的 Chromium 验收脚本。更新签名 Host 后，用本机已有凭据重新绑定五个 MiniMax 配置，原 Task 的 DSH 评审启动错误恢复后重新派发。

DSH 的 Task 代码评审 `e1ef0cce-0892-a99e-602c-b958dd0e4cb1` 三项通过。Task 源提交回归 `1787deb6-07a9-f60d-1bdd-93530d70a02d`、合入候选回归 `73718a63-d74f-8180-ac13-619a6e3f6396` 分别跑完 51 项。在两份确切代码上另用原只读镜像补跑同一冻结独立测试，各 9 项通过。Task PR 已合入 Feature 分支，GitHub 合入 SHA 和受测候选均为 `eded7bf8b54adb2559cf1b0a177af56b24fdd589`。[Task PR #42](https://github.com/xfork24/devdesk-project-1/pull/42)

ZCode 首轮报告缺少外层工作流封套。GPT/Codex 重派同一只读角色，ZCode 在实际轮次 `f799e742-e3e5-5a98-1a7d-882c487132a0` 通过 Host MCP 成功读取五次冻结产物，提交评审 `5204dbb0-adcd-1ab7-f6f5-4612e0458c7a`，三项检查通过。

Feature 已发布源 `eded7bf8b54adb2559cf1b0a177af56b24fdd589` 和合入候选 `494c7bedc61aa1a056567624fc94bfa3d7981c35` 分别取得平台回归 `001dd332-250f-778f-d3ec-8672c366f2fb` 和 `413cf841-1010-554b-2df0-69087e88d543`，七文件各 51 项全部通过。在两个确切版本上补跑同一独立断言，各 9 项通过。候选用只读导出及冻结断言文件进行此项补跑，它的 Chromium 验收也通过。正式 PR 差异只有 `public/screenshots.js` 和 `test/screenshots-ui.test.cjs`。

原主干读取超时后，Host 对冻结 GitHub `origin` 启用精确 API 回退，并通过原核对入口恢复。PR 投影预检随后遇到 TLS 错误，新版核对入口用保留的失败观察和三处写入阶段的版本证明发布尚未应用。验收 Host 通过隔离连接访问 GitHub，GPT/Codex 在原 Feature 中重试发布。核对审批包、评审和精确回归后，按用户授权提交第四份原始人工批准 `a06a7f7a-800b-c7ce-8dbf-3f4e167a4446`。

2026-11-28 15:34（北京时间），PR 合入 `main`。GitHub 的合入 SHA、本地主干工作区和已测试候选均为 `494c7bedc61aa1a056567624fc94bfa3d7981c35`。合入后，本地同步遇到 Git 索引时间戳误判。逐字核对工作树和索引仍是冻结旧主干，正常刷新状态后，通过原 merge 执行核对远端回执并完成本地同步。索引误判的源码修复单独验证了文件内容相同且时间戳变化的场景，并确认实际未提交改动仍会阻止同步。[Feature PR #43](https://github.com/xfork24/devdesk-project-1/pull/43)

Feature 和 Task 均完成，项目经理控制器已关闭。`tests/live/audit_domain_delivery.py` 核对 38 次由 Agent 委托派发、四份实际人审、十个已完成的岗位轮次、六份仪表化测试回执及两个 GitHub 合入回执。机器审计见 `review/mixed-harness/full-foundation/project-feature-final/delivery-audit.json`。
