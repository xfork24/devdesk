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

## C4 实施与验收准备

新增三 Task 生产命令回归，复现业务依赖只在工作空间准备和完成时核对、派发却仍返回 ready 的缺口。共同资格入口现增加 task_dependencies：A 待执行时 B 保持 queued，独立 C 可完成；A 真实 tasks.complete 回执投影后 B 才派发并完成。初始测试搭建中的缺少 Delivery/目标修订两项错误与实际 ready/blocked 红灯日志均保留，不将搭建错误冒充产品缺陷。

现有真实默认工程探针增加明确的 Rust 驱动与策略选择，两个驱动从相同的预演/应用输入绑定开始；只对明确 CAS 拒绝进行有界重读，不重发未知外部结果。Rust 完成时验证无经理 Controller，业务调度 Actor 必须来自 driver。新增只读现场报告脚本，从停止后的真实快照索引每个执行、历史失败、冻结定义、模型/Session/Turn/运行镜像、控制 Actor、决策理由与产出，不创建成功事实或新归档产品。

本机 Codex 的真实权限 5 项、通用 Docker 3 项，以及命令 Red/Green 2 项通过。整组 check-live 保留原失败：DSH 固定镜像缺失，准备请求又收到 provider_failure，未宣称 DSH 实机通过。原忽略权限用例补齐当前必需运行资源引用，使用实际测试镜像身份；本轮不实现 DSH 工具桥。

完整回归首轮编排 40/42，两个旧用例把短字符串当作默认 Design，实际被新章节规则拒绝。材料替代/分块读取用例已改用独立最小 Delivery；默认规则未放宽，完整复验正在进行。真实小型经理探针首次两次在模型运行前暴露脚本旧写法（隐式 global 作用域、遗漏 sandbox_ref），已修正并另保留失败数据集；第三次已完成真实 start、计划修改、业务 Agent 与结果接纳，暂停/重启收口仍在执行。最终 main 全流程验收尚未开始。

C4 预合入收口：完整 Rust workspace 290 项通过（17 项按现有配置忽略），HTTP/认证连接、模板重置、数据集及运行准备入口通过。debug 浏览器已通过相关布局/运行计划及部分接入用例；在进入 main 构建前主动结束本轮浏览器进程，避免共享 target 的运行二进制被覆盖，不把预合入 check.sh 记为完整通过。完整 debug/release check.sh 留在 main 执行。

第三次真实小图已通过实际 Codex 经理、运行时 preview/apply、业务 Agent 的平台工具调用和精确结果接纳、暂停/人类接管/Host 重启/新控制会话继续，原执行与输出保持不变，完整原生回复均单 content item。根为 a0b4953a-3045-436f-97a1-f854772bcbe3。停止后已保存一致 project-evidence-final、5 个当前/历史执行的 ledger 与现场报告，Artifact 摘要核验通过。本次核心完整 Feature/Task 的最终双驱动验收仍以 main 的新场景为准。

## main 最终验收进行中

已将实施分支快进合入 main 3324d1e。该 main 实际构建并签名的 release Host 已冻结，SHA256 为 fb25758349a079724e82f90fafa98b694c6833553c1f71d8f5da1d85fd97513f；源码、脚本和 BUILD.json 保存在 main/package。完整 check.sh 已在 main 返回 0：Rust 290 项（17 项按原配置忽略），debug/release 浏览器各 57 项，以及 HTTP、连接、模板、数据集和运行准备入口通过。真实 Codex/命令/Docker 定向 10 项通过。实际用于回归的 debug/release 二进制与工程场景冻结二进制分别保存，内容身份不混记。

真实 Rust Default Feature/Task 已通过新 PRD/Design 结构和精确评审位置、实际自检、Task 20 节点、8 条独立测试的真实 Red 到 Green，以及候选评审和 Task 发布；Feature 发布链仍在执行，完整双驱动及最终证据核验尚未关闭。

现场报告关联核对发现 Agent 的原生平台调用以 caller.turn_id 标识来源，不直接用步骤 ID 作 definition_id；索引器已通过真实停止后快照验证并在 11de0de 合入 main，关联原生平台回执与会话事件，不改变运行内核。补充同根三 Task 回归：B 在成员数组最前且依赖 A，A 等待精确 Human 门禁，foreach=2、根 worker=1 时独立 C 完成、B 保持 queued；通过测试 Human 入口签署 A 后 A/B/父流程全部完成。该公开命令/SQLite/测试端口用例不冒充真实模型或真实用户对工程产物的签收。初版 JSON 宏括号编译错误及修正后通过日志保留。

## main 最终验收完成（2026-11-15）

C1–C4 已合入本地 main。完整 check.sh 返回 0，Rust 290 项（17 项忽略）、debug/release 浏览器各 57 项及相关跨进程入口通过；真实 Codex/命令/Docker 定向 10 项、同根三 Task 补充用例及 14 项证据核验回归通过。

Rust 和 Agent 的实际 Feature/Task 均完成 42 个当前步骤，独立 Red/Green、两轮增量 TDD、候选、独立评审、Git 精确推进和发布回执通过；每个场景有 86 个经 Actor 核对的业务决策，默认预算和门禁未改变。完整现场审计核验 424 个注册 Artifact。

Rust 场景完成后的核验器 UTF-8 边界及只读参数拒绝误判已修正，经公开完成现场只读重验；业务表前后完全不变。Agent 场景在用户确认的网络异常后实际停止旧控制 Turn，在相同数据集以原 600 秒策略恢复；此前 37 个完成执行及 45 个输出记录原样保留，完成其余五步及显式根完成，按有指导恢复记录，不称全程无人值守。原失败、前后快照、实际执行者和停止证明全部保留。

最终详见 [ACCEPTANCE](ACCEPTANCE.md) 及本机完整执行报告。逐步 Markdown 补充当前/历史标签及精确决策/产出索引；重新生成前后两份结构化 ledger 的摘要均一致。DSH 实机项继续如实列为未完成，未扩展核心范围。用户签收留待用户核对。
