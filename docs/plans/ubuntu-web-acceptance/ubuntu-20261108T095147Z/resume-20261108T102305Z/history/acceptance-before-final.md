# Ubuntu Web 续验：resume-20261108T102305Z

日期：2026-11-08。接续 [首次报告](../acceptance.md)，使用同一专用数据集与测试仓库，旧报告保持原字节。用户已完成 Codex/DeepSeek 配置，并明确批准、实际加载下述专用 AppArmor 策略。本报告当前为执行中记录，尚不代表 U0–U6 全部通过。

## 已确认结果

| 检查 | 实际结果与证据 |
|---|---|
| 两供应商认证 | Codex authenticated=true；DeepSeek 实际读取成功；[认证](credentials.json) |
| 两供应商原生续聊 | 各两轮成功并复用 native Session；[Codex](R3-codex-application.json)、[DSH](R3-dsh-application.json) |
| 项目/角色隔离 | 两供应商各在另一项目运行独立 reviewer 讨论；native Session 不同，均释放；[角色引用](role-reuse.json) |
| 冻结绑定 | 全局 Codex→DSH→Codex 后，四个既有 Session 完整绑定摘要和 native IDs 保持不变；[冻结证据](frozen-bindings.json) |
| 安装复用 | 完整探针退出 0，双镜像并发四次登记同 ID、跨项目/角色引用、五个公共层；[安装探针](R8-installations.json) |
| 正式需求与仓库 | Chromium 讨论、显式采纳 Project Requirement、真实仓库/Feature worktree 通过；[浏览器](R5-browser-proposal.json)、[截图](R5-browser-proposal.png)、[仓库](R4-repository.json) |
| 凭据持久化 | 唯一归属 Host 优雅停止/重启，Codex OAuth 与 DeepSeek 仍可用；[回执](credential-restart.json) |
| 离线检查 | `./scripts/check.sh` 退出 0，完整 Rust、debug/release HTTP/standalone、两轮各六个 Chromium；[回归](offline-policy-integration.log) |
| 原 bwrap 阻塞 | 专用策略下实际 Codex 凭据 canary 通过；[实测](named-policy-runtime.json) |
| Docker/Git/Node | `--include-ignored` 退出 0：Docker 4/4、CommandTests 2/2、Git 2/2；[资源回归](real-resources-after-policy.log) |
| 策略负例 | 外层 CapEff=0；新 user namespace 内对未允许路径的 tmpfs 挂载被拒绝；[负例](apparmor-negative.json)、[内核 audit](apparmor-audit.log) |
| 预检 | 实际集合元数据、已安装策略归属/固定内容及依赖检查通过；[预检](preflight.json) |

精确命令及退出码见 [commands.jsonl](commands.jsonl)。原先的完整安装复用失败只表示当时缺引用；本次已建立真实跨项目/角色引用再执行，不能将旧文件改写成通过。

## AppArmor 变更与授权

旧 `docker-default` 阻止 bwrap 的挂载准备。用户在当前会话明确允许使用专用策略，并在终端安装、加载 `devdesk-harness-v1`。这改变了相对默认策略的有限挂载许可，是原计划约束的已授权例外；没有将它说成完全相同的权限集合。

固定策略 SHA256：`3af35216df62da5ff6bfbe54b47506b219f26ccc99c944d84b826d66870cdb2c`。文件安装到 `/etc/apparmor.d/devdesk-harness-v1`，root 归属、0644；Host 比对内嵌源码并冻结摘要到容器规格，实际容器以 `devdesk-harness-v1 (enforce)` 运行。未添加 capability、privileged/unconfined、宿主 PID/network、socket/HOME 挂载或可写根目录；没有修改默认策略或 sysctl，也不重启 Docker。

策略仅允许根传播、/tmp 准备根、/newroot 的 bind/remount、tmpfs/proc/devpts 与固定 pivot/unmount；未列出的挂载保持默认拒绝。实际内核 audit 明确记录 `/tmp/unapproved/` 的挂载点不匹配及 error=-13。负例首次使用 unshare 默认的 root rprivate 传播，它也被拒绝；调整探针为 `--propagation unchanged` 后才独立验证未列入挂载点的拒绝，未为探针扩大策略。首次结果保留 [初次负例](apparmor-negative-initial.json)。

策略编译、未安装时的明确失败见 [提案](apparmor-proposal.json)、[编译检查](policy-install-check.log)、[安装前探针](named-policy-before-load.json)。AppArmor 来源是 [Moby v0.3.0](https://github.com/moby/profiles/blob/apparmor/v0.3.0/apparmor/template.go)，安装方式见 [Docker 官方文档](https://docs.docker.com/engine/security/apparmor/)；bubblewrap 的准备路径依据 [v0.8.0 源码](https://github.com/containers/bubblewrap/blob/v0.8.0/bubblewrap.c)。源文件本身记录许可证与约束。

## 全部组 full

已在新目录 [full-20261108T103202Z](full-20261108T103202Z/live-check-full-git-docker-harness-github-web-workflow-recovery.json) 启动全部组，命令见 [控制台](full-entry.log)。它重新执行 Git/Docker、两原生讨论、浏览器采纳、仓库、正式规划、实施/Review、PR 接纳、取消与跨项目崩溃恢复。此处持续更新最终结果，不以独立通过的子集冒充完整 full 通过。

正式工程、最终运行包的双供应商会话及最后 verify-existing 尚待本次 full 后完成。已通过两次正式规划 Review，initial Snapshot 和两个 Task worktree 已建立；双供应商实施 Attempt 实际同时启动。首次报告 A01–A22 中的历史“待验/失败”仍有效描述当时结果；最终矩阵将在完整结果明确后逐项更新。

### 保留的实施失败与修正

首个 DSH multiply Attempt 的缺实现断言位于模块顶层，实际 Node reporter 返回 environment（ERR_TEST_FAILURE/unknown），领域门禁正确拒绝将其认作 Red；原代码、candidate、Run/Turn 与失败报告保留在 full 目录 history。通过正常 `definition.revise` 发布 Tester 新版本，明确在 node:test 回调内执行动态导入/断言、只把精确目标缺失认作缺实现，随后新建 Attempt；[修正回执](full-20261108T103202Z/test-author-correction.json)。没有修改 reporter 分类或覆盖失败 baseline。

新 Attempt 的首个 test_author Turn 实际返回 max-tokens，按 provider failed 保留；在尚未封存 baseline、执行者已释放的条件下，通过显式 `run.retry` 使用原冻结配置进行技术重试；[回执](full-20261108T103202Z/test-author-provider-retry.json)。这次是真实供应商返回的终止原因，与首次报告只覆盖契约注入的边界不同。验收探针现按 retry_of 取当前有效 Run，旧失败仍在数据库和历史中，不再把已被替代的旧失败误判为当前阶段失败。

全入口增加显式 `--resume`：只复用已经成功且原日志哈希匹配的副作用步骤，失败阶段实际重跑，最终数据库/Artifact/GitHub 核验不复用。完整失败报告逐次保存在 full/history，新的阶段日志带 resume ID。数据集/仓库上下文不匹配、成功日志被改动均在执行外部动作前拒绝，见 [续跑故障注入](resume-contracts.json)。

### 输出预算兼容性修复

相同冻结配置的技术重试再次得到真实 max-tokens。确认 Host 在 DSH initialize 中将所有工作写死为 8192；保持模型/安装版本不变，将 TaskWrite/TestWrite 工作预算调整为 32768，ReadOnly 保持 8192，原十五分钟执行上限和 max-tokens 失败语义不变。当前 [DeepSeek 官方参数说明](https://api-docs.deepseek.com/api/create-chat-completion/)允许该值，思考模式的官方默认输出预算高于 8K。

`harness.rs` 参与 adapter build 摘要，所以重建后对旧 Run 的技术重试实际返回“冻结适配器构建已更新”；未绕过该门禁，已使用第三个新 Attempt 冻结新配置，见 [预算修正回执](full-20261108T103202Z/write-budget-correction.json)。旧两个 Attempt 及各失败 Turn/Run 均保留，新阶段结果继续等待真实执行。

### 正式实施与评审已通过

两 Task 均完成 test_author→Red→implement→Green→docs→verify，并取得独立 Review 的接受。add（Codex）最终代码 `6ad249a3dbc1d1b526ce6a9bb08857abe2548054`，Green/verify 21 项通过；multiply（DSH）最终代码 `d3ad264b6ef81f07ac3d6d3f13c97c8f07addcff`，Green/verify 9 项通过（每项含多组行为断言）。Red 分别基于 `d2b2a3918d8ff8c2a53643fe59eed6798d55b958` 与 `a570e27d3168c9e53b0e6e528487be8f1a937cfa`，均为真正断言失败；[完整实施证据](full-20261108T103202Z/R6-implementation.json)。初始两个不同供应商 Attempt 实际并行；DSH 的后续修正 Attempt 按真实失败顺序另建，不宣称最终修正 Attempt 与已接受的 Codex Attempt 再次并行。

文档阶段曾收到真实 DeepSeek QUOTA/HTTP 402，错误消息已脱敏，见 [协议摘要](full-20261108T103202Z/docs-provider-error-protocol.json)。用户恢复同一 key 的配额后，只显式技术重试文档 Run，Red/Green/实现及冻结配置不变，[回执](full-20261108T103202Z/docs-quota-retry.json)；错误含义对应 [官方 402 说明](https://api-docs.deepseek.com/quick_start/error_codes/)。未保存账户金额。

第一次原生 Review 输出含 JSON 前的说明文字，严格结构门禁拒绝接受；原输出见 [契约失败](full-20261108T103202Z/review-contract-failure.json)。通过正常定义修订强化 JSON-only 指令并新建 Review，未放宽解析器。随后 Review 在原 8192 预算下也实际 max-tokens，正式工程/Review Run 的预算因此统一调整为 32768，普通讨论仍为 8192；新 Review 在新冻结配置下完成，旧失败均保留，[格式修正](full-20261108T103202Z/review-format-correction.json)、[正式预算修正](full-20261108T103202Z/review-budget-correction.json)。最终 source adapter identity 也包含 AppArmor 文件内容，策略变更不能漏出旧执行锁。

当前 PR 候选阶段已经观察到首个 Task 实际合入、旧目标候选拒绝和第二 Task 重建重验。最终 PR/Snapshot/恢复结论待本轮结束后填写。最后一次完整回归见 [offline-formal-run-budget.log](offline-formal-run-budget.log)，退出 0。
