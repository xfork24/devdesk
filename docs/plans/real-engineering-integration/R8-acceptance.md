# R0–R8 真实工程接入验收

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

日期：2026-11-08。结论：本轮真实工程闭环及 macOS Web 验收通过。Linux 容器运行证明不等于 Ubuntu 宿主验证；后者按用户追加要求继续实施。macOS 原生客户端、Archive 执行、data 搬迁与通用语言测试配方仍不在本轮范围。

本报告为实现者自检。工程交付中的独立角色 Reviewer 实际执行了 Review；没有将其描述为对 DevDesk 实现代码的第三方审查。

## 实际闭环

专用私有仓库为 `xfork24/devdesk-integration-test`，未使用 DevDesk 自身仓库作为被管理项目。Chromium 中与 DSH 讨论并采纳 Project Requirement；随后正式规划、Requirement/Plan 两次独立 Review、initial Snapshot 与两个真实 Task worktree。两个实现 Attempt 先同时启动，分别使用 Codex 与 DSH，经 test_author → Red → implement → Green → docs → verify，独立评审通过后进入候选流程。

| 接纳 | GitHub PR | 单父候选 | 结果 |
|---|---|---|---|
| add → Feature | [#2](https://github.com/xfork24/devdesk-integration-test/pull/2) | `1085d18d9c56228f1386eb4d037f9ea3e8620f32` | merged |
| multiply → Feature | [#4](https://github.com/xfork24/devdesk-integration-test/pull/4) | `3caa8e3aa37becbf378219fca6e5a75db281166b` | merged |
| Feature → main | [#5](https://github.com/xfork24/devdesk-integration-test/pull/5) | `ee4785bd579f13c9479800a64ad53bfa83f04f5d` | merged，Feature completed |

旧目标候选 PR #3 保留未合入，其旧证据不能接纳。Feature 收尾独立执行测试修订、全量 66 项测试、文档、复验和 Review。前两次收尾 Review 分别拒绝了模块缺失错误分类和缺少历史 Task Red 来源的问题，均保留失败并使用新 Attempt 修正；没有通过 Override 绕过。最终 Reviewer 接收冻结的递归交付来源及固定版本 diff。最终主干接纳由真实前端点击，命令 ID、截图与远端事实见 R7（历史路径：`R7-merges.json`）、浏览器（历史路径：`R7-browser-final.json`）。

## A01–A22

每项注明实际外部验证和契约故障注入的边界；不把未发生的供应商故障写成真实服务故障。

| 编号 | 验收结果与证据 |
|---|---|
| A01 | 真实 HTTP 切换 Codex → DSH → Codex，旧 Session 绑定保留；R8 会话（历史路径：`R8-recovery.json`） |
| A02 | 两供应商各连续三轮复用原生 Session；角色和工程 Task 的 Session/历史独立；R3、R6、R8 |
| A03 | 两套实际镜像各并发登记四次得到原 ID；跨项目/角色只引用安装；公共层实际相同；安装证明（历史路径：`R8-installations.json`）。不同版本并存、摘要冲突由 registry 契约回归覆盖 |
| A04 | 真实无效 Codex 模型明确 failed，无 Sim 回退；传输失败/超时/脱敏由实际子进程与原生协议回归覆盖。没有声称主动制造供应商限流或公共网络故障；R8、native-protocol-regression.log |
| A05 | Feature 创建前，真实前端讨论并保存 Project Requirement；R5 浏览器（历史路径：`R5-browser-proposal.json`） |
| A06 | Requirement/Design/TaskPlan/TestPlan → 两次正式 Review → Snapshot → 两真实 worktree；R5（历史路径：`R5-planning.json`） |
| A07 | 客户端消息重放真实返回原 Turn；重复采纳、stale、跨项目拒绝、依赖 DAG 与门禁由 conversation/HTTP 回归覆盖 |
| A08 | 两个不同供应商 Task Attempt 同时启动，真实独立容器/Session/worktree；额度和写许可并发由 workflow/host 回归及实际 Docker 目录授权验证；R6（历史路径：`R6-implementation.json`） |
| A09 | 实际 Red 断言失败、Green/verify 与同 SHA；CommandTests 实际区分环境/语法错误，报告绑定镜像/容器/退出码；R6、real-resources.log |
| A10 | 原生独立 Reviewer 实际接受/退回；人工 Override、不可豁免约束、身份根隔离由 workflow 回归覆盖，未为本真实案例制造 Override |
| A11 | 三次成功接纳均以原目标为唯一父提交；实际 Git/远端复核；R7、live-check-verify-existing-git-docker-harness-github-web-workflow-recovery.json |
| A12 | 三个 PR head/mergeCommit/目标等于候选，merged 才完成 owner；R7、远端复核报告 |
| A13 | 两 Task 先基于旧目标，旧候选拒绝后重建重验；真实 CAS/lease 拒绝旧值；R0/R7、real-resources.log |
| A14 | 专用仓库兼容；对 openai/codex 只读诊断发现规则即阻塞；未修改规则、调用管理员 bypass 或平台 merge；R4-repository.json |
| A15 | Codex 实际 interrupt ACK 后停止容器，DSH 实际 forced stop；协议/写权限撤销/资源保留；R8-recovery.json |
| A16 | 两供应商输入已消费后 SIGKILL 专属验收 Host；重启不重发，原 external IDs 保留，先停止封存再显式取消未知结果；R8-recovery.json、live-check-full-recovery.json |
| A17 | 实际 Git marker、重复 CAS/容器创建与清理；实际已 merged PR 去掉 number 后两次 ensure 均找回同 marker/PR；PR 核对（历史路径：`pr-recovery.log`）。落库前故障由 Core 契约注入覆盖 |
| A18 | 导出失败保留环境、回执复用及清理失败人工重试由 Core 故障注入覆盖；真实 Docker 停止/删除幂等，镜像保留且安装仍可引用。没有声称真实 daemon 恰好发生删除故障 |
| A19 | 崩溃前另一个空项目保持 active；重启先处理后台项目两个实际执行者，再恢复选择；R8。跨库/过期请求与 SSE 隔离通过 HTTP/host 回归 |
| A20 | 全局 v1/项目 v1、v13 历史升级及原 Sim 工作流回归通过，来源和原记录不改写；offline-regression.log |
| A21 | 实际容器最小权限、无 socket/HOME、测试无模型/GitHub 凭据；嵌套沙箱不扩大写目录，DSH 隐藏状态/proc；真实资源与协议脱敏测试通过 |
| A22 | Feature 完整收尾、最终 Snapshot、主干候选/Review 和前端接纳均完成；已封存 Artifact 全量哈希复核，实际执行者停止释放；打包只有一个 Host/Web 二进制及共享镜像构建配方，无按角色重复二进制 |

A04、A17、A18 的分层验证是：真实外部对象路径 + 明确的 Core/协议故障注入；测试报告没有把夹具结果标成真实供应商事实。

## 原设计契约映射

H01→R0 固定版本/角色/权限与 registry；H02→R3/R8 连续会话；H03→原生 JSONL、有序持久化、真实 DSH 快速完成夹具；H04→实际取消/无效模型/崩溃及有界传输与执行超时；H05→真实工具/哈希/代码版本；H06→真实原生 resume 和快照后继；H07→双 Task/目录授权/额度；H08→旧库与协议回归；H09→停止、封存、清理与共享安装。

INV01→R5/R6 来源链；INV02→真实 Git/worktree；INV03→不可变绑定；INV04→DAG/并发/许可；INV05→Run/Turn/Execution/Artifact 查询；INV06→逐阶段 Review/追加 Override；INV07→单父候选与 CAS/lease；INV08→本轮保留的 Git refs/对象、Snapshot/Artifact 核验，Archive 环境重建部分仍延期；INV09→导出/清理契约与实际 Docker；INV10→Web/Rust 入口，native 客户端延期；INV11/INV12→Archive/data 搬迁按原范围延期；INV13→安装与适配器构建锁、后继 Session/技术重试门禁。

## 回归与运行包

- `./scripts/check.sh` 退出 0：Rust、debug/release HTTP、复制后的独立 release 二进制、两轮各 6 个 Chromium 用例。见 原始回归（历史路径：`offline-regression.log`）。
- 实际 Docker CommandTests 2/2、Docker 资源 4/4、真实 Git 2/2；原生协议 4/4、会话 9/9、host 5/5；专项日志同目录保存。
- `scripts/check-live.sh --mode full --groups recovery ... --allow-host-restart` 实际执行一轮双供应商 consumed-input 崩溃/重启；`--mode verify-existing` 再查询真实 GitHub/数据库和所有已提交产物哈希。两种模式名称和报告明确区分。
- 曾有一条启动失败记录在获得任何 container ID/协议帧前中断：保留 interrupted、writer_revoked 与未知 Turn，不伪造不存在的协议产物。复核报告单列 failed_before_worker_receipt；所有实际已启动执行者均有停止/封存/释放证明。
- `scripts/package.sh` 构建 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 及 SHA256。包括嵌入 Web 的 Host、文档、固定容器配方，排除 Finder 元数据；不附 OAuth、API key、验收数据库或每角色运行时副本。

## 验收中修复的问题

真实写工具缺少 code-mode host、DSH proc 挂载冲突、候选/推进操作 ID 文件冲突、递归交付来源遗漏、正式证据包 64 KiB 限制、逐分片全状态写入开销、SSE 与连续命令刷新竞态、DSH 确认前事件与快速完成丢失，以及 SDK 可选 custom 字段不接受 null。失败与旧 Attempt 均保留。DSH 的额外 session-log-deepseek 上传插件也已明确关闭，当前协议与后继绑定重新实测。

## 后续边界

Web UI 为当前客户端。用户追加的稳定 macOS 签名/无后台 Keychain 提示、Linux 系统凭据与 Ubuntu Web 实机验证继续进行；不能把旧原型的 Linux 验收或当前 Linux worker 结果当作这轮 Ubuntu 宿主已经通过。Codex/DSH 固定版本以外的新版本、其他测试语言、网络测试、严格保护仓库的其他合入策略需另行验证。
