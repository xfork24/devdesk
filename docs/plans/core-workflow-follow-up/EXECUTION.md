# 核心工作流补强执行记录

日期：2026-11-14。实施基线为本地 main `8fef081`（包含计划之后的现行文档更新），独立 worktree `/Users/hengliu/workspace/devdesk-worktrees/core-workflow-follow-up`，分支 `codex/core-workflow-follow-up`。本报告随阶段追加，最终结论以合入 main 后验收为准。

## 本轮授权与证据保留

用户明确要求按 C1、C2、C3、C4 顺序实施，合入本地 main 后进行最终验收，不扩展核心范围；本轮专用远端仓库已明确选择 `xfork24/devdesk-project-1`。平台只创建测试分支、提交和 PR，不调用 GitHub 合并接口。用户追加要求保留最终测试实施文件、每步实施者、决策和产物及完整工程现场，供人工验收。

本轮证据根为 `/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14`。最终验收另设 main 目录，保留实际二进制/脚本摘要、输入、逐步 Actor/执行来源、回执、失败/恢复与停止后的 SQLite/Artifact 一致快照和证据索引；认证资料不写入分享包。文档目录仅保存 Markdown。

## C1 实施与阶段验收

先加入 foreach 槽位回归，在改动前实际失败：查询 dispatchable 为 true，预期 false。共享资格进入 core，实际命令继续核对精确计划、输入/输出与控制权，不放宽并发或人工门禁。阶段完整结果待实际回归后补录。

共享动作资格已接入 workflow.inspect、driver.waiting、Rust 候选选择及 dispatch/accept/retry/complete 实际用例。foreach 槽位和独立人工等待回归通过；原停止证明、完成条件、人工确认和事务 CAS 保留。陈旧候选只在有界 tick 内排除/重读，受阻结果不再阻塞其他动作；UI 只展示共同资格返回的原因。

C1 验证：编排 37 项通过，Schema 14 项通过，domain/core 单元通过（数量见原始日志），Web production build、架构/出厂目录及 diff 检查通过；最终 foreach 定向复验通过。Web 初次构建捕获一处可空 data 访问，改为显式空数组后重建通过，失败日志保留。未执行真实模型工程；最终 main 验收尚未开始。阶段日志在证据根 stages/c1-*。

## C2 实施与阶段验收

增加可信边界的 FailureDisposition 与 StepExecution.failure/retry_of；错误源保留结构化 code、类别、实际来源和副作用结论。原生明确拒绝输入可报告 technical/not_applied；超时/未知结论不自动重发，并在根/集合额度中继续占位直至核对。自动驱动同时检查真实停止、两层预算和所选错误码，默认策略仍不重试。补充输入 sequence 与当前 round 的重试历史计数分开，重试创建新身份并保持原冻结输入及最近补充指令；原意图终态拒绝被迟到结果改写。

正向边界覆盖 core Session 创建失败、补充 Turn 派发失败、Tool 准备失败及原生端口明确拒绝；保留停止证明、旧失败事件与新执行关联。业务拒绝即使命中错误码也不自动重试；未知副作用不重试、不释放预留额度；重启不增加次数。测试端口不冒充真实供应商。

验证：编排 40 项和 Schema 15 项通过，domain/core 27 项通过，另新增未知结果占用额度的定向复验通过；生产输入/输出回归首轮 41 项中 40 通过，失败为结构化意图错误仍被旧文本读取器吞掉消息，修正读取器后该项通过。补充输入、原生拒绝/停止与业务拒绝三个定向测试通过；原生测试初版把初始化 Human start 也误计入业务调度，收窄断言至 dispatch/accept/retry/complete 后通过，原日志保留。全套统一复验留到 C4。架构、出厂目录、格式和 diff 检查通过。日志均在 stages/c2-*，尚未开始最终 main 实机验收。

## C3 实施与阶段验收

普通 Workflow Tool 和原生 Agent 的 spec.lint 已接入同一个纯检查器，只允许当前冻结步骤声明的输入/输出类型，不扩大经理或配置写权限。自检不会封存输出；报告自检另返回与实际接纳相同的 content_report_valid/错误，精确核对冻结证据摘要和位置。未经授权和其他交付类型均实际拒绝。

出厂配置按依赖闭包发布 25 个现有源后继和一个自检 Skill：PRD/Design 使用实际必需章节，关键评审要求真实位置；六个 Codex 业务职责通过 Capability 与 Permission Context 明确授权。Feature/Task 新选择为 revision 7，Standard 4，Permission Context 5，原 schema_version、并发及默认零重试不变；历史任务不升级。没有增加长度阈值、另一套 DSL 或账号等外围功能。

验证：domain/core 27 项通过；configuration_changes 7、configuration_resources 15、configured_workflows 46、manager_workspace 8、schema_contracts 15、standard_snapshots 5 项全部通过。新增普通 Tool、原生 Agent、越范围/未授权、草稿不封存、报告伪造位置与实际默认章节测试通过。架构、出厂目录、格式检查通过。原生拒绝测试初版误期待被提前拒绝的调用仍有工具回执，修正断言以核对实际边界；失败日志保留。后继生成脚本和精确引用映射保存在 stages，未放入产品配置存储。仍未以测试端口结果冒充真实模型/Git/Docker 验收。
