from pathlib import Path
import json,shutil
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));r=Path(ctx['reports']);(r/'history').mkdir(exist_ok=True)
shutil.copy2(r/'acceptance.md',r/'history/acceptance-before-final.md')
(r/'acceptance.md').write_text('''# Ubuntu Web 实机验收完成记录

日期：2026-11-08。执行者：Codex；性质：实现者自检，正式领域 Review 由独立角色的原生 Session 执行。**Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过**，包含双供应商真实工程闭环、凭据、安装复用、取消/崩溃恢复和源码外 Linux 发布包。

本次从 [首次失败报告](../acceptance.md) 接续，同一独立数据集、同一专用测试仓库。全部组 full 经显式 `--resume` 完成，19 个步骤均通过；它不是一次无中断的首次成功。历史失败报告、原始命令日志、失败 Attempt/Run/Turn 和旧角色版本均保留，没有用 Sim、SQL 改写、放宽 Red/Review 门禁或管理员 bypass 获得通过。专用 AppArmor 策略的有限权限变更已由用户明确批准并在本机加载，详见下文。

## 源码、环境与运行身份

接续源码提交 `a620e92df36e536d66f74a67b31cd7e7c4670c48`，初始工作区干净；修复保留在工作区，未自动提交或推送 DevDesk 源码。最终补丁与新增文件摘要见 [source-changes.patch](source-changes.patch)、[source-files.json](source-files.json)。[环境](../environment.log)、[安装后版本](../dependencies-after-install.log)、[初始上下文](../context.json)和[本轮上下文](context.json)分别保留。验收仅管理 `xfork24/devdesk-integration-test`，未把 DevDesk 自身仓库作为被管理工程。

| 项目 | 实际值 |
|---|---|
| OS / kernel / CPU | Ubuntu 24.04.5 LTS / 7.0.0-42-generic / x86_64；glibc 2.39 |
| 工具链 | Rust/Cargo 1.99.1；Node 24.13.0 / npm 11.8.0；Python 3.12.12；Git 2.43.0；gh 2.45.0 |
| Docker | Engine 29.9.3，Linux/amd64，AppArmor enforce |
| 系统凭据 | libsecret-tools 0.21.4、GNOME Keyring 46.1，当前桌面 D-Bus 默认集合已解锁 |
| 数据集 | `/home/hengliu/workspace/devdesk/data/ubuntu-20261108T095147Z` |
| 保留的专用入口 | `http://127.0.0.1:51751`；[发布包 Host 重启归属](final-package-restart-ownership.json) |
| GitHub | 私有 `xfork24/devdesk-integration-test`，登录账户 xfork24 有权限，兼容诊断通过 |
| 本机 Codex | 0.167.0，实际 Linux ELF；安装 ID `45ef8767-7ed8-46fb-90e7-aa1d83eceb4a`；未将 npm 启动器发现标为实测 |
| Codex Docker 安装 | `a7f46fc9-4185-4749-a82c-f0f269f5956b`；`sha256:b03dc81902ff96bb95978d92648a4953bc16028d089873e3a530377eb67d592e` |
| DSH Docker 安装 | 0.2.2-rc.1；`9a577bdd-db7e-46bd-adaf-bb9759a0c63a`；`sha256:2fbf30a5c26a99742135d321bc30861f633c3aa8f77d6a4fa79b867f34380201` |

两个镜像均在本 Ubuntu 宿主实际构建/登记，没有复用 macOS 的安装 ID、image ID、数据目录或秘密。用户通过设置页独立完成 Codex 官方网页登录及 DeepSeek key 配置；[认证检查](credentials.json)、[原先凭据重启](credential-restart.json)、[最终发布包重启](final-package-restart-verification.json)均通过。

## 必要检查结果

精确命令、起止时间和退出码见 [commands.jsonl](commands.jsonl)；首次安装/OAuth/沙箱失败仍在父目录。以下均为本轮 Ubuntu 实际执行。

| 检查 | 结果 | 证据 |
|---|---|---|
| 最终 `./scripts/check.sh` | 退出 0；100 个 Rust 测试通过，5 个显式 live 用例 ignored；debug/release HTTP 与 standalone、两轮各 6 个 Chromium 通过 | [最终离线回归](offline-formal-run-budget.log) |
| 真实 Git / Docker / CommandTests | 2/2、4/4、2/2，通过实际嵌套沙箱、凭据 canary、生命周期和挂载边界 | [资源回归](real-resources-after-policy.log) |
| 全部组 full | 退出 0，19/19；显式续跑保留已成功副作用，失败阶段重新执行，最终事实重新核验 | [完整结果](full-20261108T103202Z/live-check-full-git-docker-harness-github-web-workflow-recovery.json)、[最后续跑日志](full-resume-auth-refresh.log) |
| 安装复用 | 退出 0；两供应商各并发登记四次返回同 ID，真实跨项目/角色引用，五个公共层 | [完整安装探针](full-20261108T103202Z/R8-installations.json)、[日志](installations-after-full.log) |
| 最后 verify-existing | 退出 0；重新读取数据库、原生 IDs、Git/GitHub、Artifact；331 个 Artifact 哈希一致，79 个执行者均 released | [复核结果](full-20261108T103202Z/live-check-verify-existing-git-docker-harness-github-web-workflow-recovery.json)、[日志](final-verify-existing.log) |
| 宿主与安全策略预检 | 退出 0；依赖、实际 Secret Service 元数据、固定策略内容/归属通过 | [预检](preflight.json) |
| 源码外 Linux 包 | 构建、HTTP/SSE、系统凭据、两个供应商各两轮原生续聊、重启和已有 Feature 查询全部退出 0 | 见后文发布包证据 |

`verify-existing` 复核的是已真实完成的 full，不替代首次外部执行。最后一次实施证据复核仅重读既有成功 Run/Review，未再次启动模型；旧失败保留在 `previous_failures` 和历史文件，[回执](final-implementation-evidence.log)。

## 真实 Web → 工程 → GitHub 闭环

浏览器讨论先生成提案，再显式采纳 Project Requirement，之后建立 Feature。[浏览器采纳](full-20261108T103202Z/R5-browser-proposal.json)、[截图](full-20261108T103202Z/R5-browser-proposal.png)、[仓库](full-20261108T103202Z/R4-repository.json)、[正式规划/两次 Review](full-20261108T103202Z/R5-planning.json)串起需求、设计、TaskPlan/TestPlan、initial Snapshot 和两个独立 Task worktree。

Project：`897f2c31-4693-412a-8e05-23380bf62436`。Feature：`01301ba9-b539-43ed-b7c5-fe654d947a5d`，最终 `completed`。两种供应商的初始 Attempt 实际重叠运行；DSH 后续修正 Attempt 在失败之后另建，未宣称最终修正与已成功 Codex Attempt 再次并行。

| Task / 供应商 | 最终 Attempt | 真正断言失败的 Red SHA | Green / verify 相同代码 SHA | 测试 / 接受的 Delivery |
|---|---|---|---|---|
| add / Codex | `472fb073-aec9-4513-b909-4069f42c5e7e` | `d2b2a3918d8ff8c2a53643fe59eed6798d55b958` | `6ad249a3dbc1d1b526ce6a9bb08857abe2548054` | 21 项；`558e1048-0c24-4858-92c3-44471daab442` |
| multiply / DSH | `98c6dd1b-5733-4cb1-b13e-084433c35810` | `a570e27d3168c9e53b0e6e528487be8f1a937cfa` | `d3ad264b6ef81f07ac3d6d3f13c97c8f07addcff` | 9 项，每项有多组行为断言；`98a67e6b-adc3-4f70-8c36-8e1ca66860cc` |

完整 test_author→Red→implement→Green→docs→verify、Run/Turn/Execution、固定镜像、测试配方、结果 Artifact 与独立 Reviewer 接受见 [R6](full-20261108T103202Z/R6-implementation.json)。Reviewer 接受记录分别为 `38f6f253-24a7-4050-bf41-5375a0638ec6`、`538d13d4-cc3b-4fa4-9a61-abf731956b92`；没有使用人工 Override。

三次实际接纳均为单父候选；远端 PR 的 head、mergeCommit 与本地候选一致：

| 接纳 | PR | 候选 SHA | 唯一父 SHA |
|---|---|---|---|
| add → Feature | [#6，merged](https://github.com/xfork24/devdesk-integration-test/pull/6) | `b13c5c12875049e78ae5111500faeac372ed4b2d` | `ee4785bd579f13c9479800a64ad53bfa83f04f5d` |
| multiply → Feature，旧目标失效后重建重验 | [#8，merged](https://github.com/xfork24/devdesk-integration-test/pull/8) | `e153941b0aff4c16e291e2f920aea4445aa04d8b` | `b13c5c12875049e78ae5111500faeac372ed4b2d` |
| Feature → main | [#9，merged](https://github.com/xfork24/devdesk-integration-test/pull/9) | `fb6ab2925db3669bf75b2d56ce0696ca0a97da76` | `ee4785bd579f13c9479800a64ad53bfa83f04f5d` |

旧 multiply 候选/PR #7 未合入，实际陈旧目标拒绝保留；没有强推绕过。最终 main 接纳由真实 Chromium 点击触发 `merge.apply`，命令 ID `87627265-a78b-496b-9414-b4d9977f7c4c`；[合入事实](full-20261108T103202Z/R7-merges.json)、[浏览器回执](full-20261108T103202Z/R7-browser-final.json)、[截图](full-20261108T103202Z/R7-browser-final.png)。

Feature 收尾 Attempt `c2423f2c-b25a-4a92-8c4d-8f16f4d03c27`，递归来源含两个 Task Delivery；closure Delivery `bb6b08d6-0645-44c3-bd2c-d06661a20e55` 已接受。六份 Snapshot IDs 保存在 R7 和最终包重启回执中；最终 Feature 测试/文档、主干候选和 Review 均属于实际追踪链。

## 失败、修正与证据保留

首次 full 在 Docker 默认策略下 bwrap 失败；这份失败报告保持原字节。后续 full 的每次失败都在 [history](full-20261108T103202Z/history/) 中保留，恢复仅通过正常领域命令与显式 `--resume`：

- Codex OAuth 首次请求失败后 pending 未复位：现在失败会清除 pending、释放所属认证进程；重复点击/HTTP 响应丢失/刷新恢复同一 loginId 和授权地址。弹窗受阻时保留官方链接。[真实原生启动/丢本地响应/取消探针](../U1-oauth-recovery.json)、[超时及退出契约](../oauth-contract.log)通过；官方网页导航被拦截的探针不冒充用户完成登录。
- DSH 最初的缺实现断言位于模块顶层，Node 将其归类为 environment；Red 门禁正确拒绝。通过 `definition.revise` 发布 Tester v2，在 node:test 回调内断言精确缺失目标，再建新 Attempt；[回执](full-20261108T103202Z/test-author-correction.json)。原错误测试未覆盖，未放宽 reporter 分类。
- 实际 test_author 和 Review 多次收到 max-tokens。原初始化统一写死 8192；现在正式工程及 Review Run 采用 32768，普通只读讨论仍 8192，十五分钟上限和失败语义不变。该值符合 [DeepSeek 官方参数范围](https://api-docs.deepseek.com/api/create-chat-completion/)。adapter 摘要变化后旧 Run 重试实际被拒绝，新 Attempt/Review 重新冻结，[实施预算](full-20261108T103202Z/write-budget-correction.json)、[正式预算](full-20261108T103202Z/review-budget-correction.json)。
- docs 实际返回 QUOTA/HTTP 402，对应 [官方错误说明](https://api-docs.deepseek.com/quick_start/error_codes/)。用户恢复同一 key 配额后，仅对文档 Run 显式 `run.retry`，已有 Red/Green/代码和冻结配置保留；[脱敏错误](full-20261108T103202Z/docs-provider-error-protocol.json)、[重试](full-20261108T103202Z/docs-quota-retry.json)。未查询或记录余额。
- 第一次独立 Review 含 JSON 前的说明文字，严格解析器按 adapter_contract 拒绝；通过正常 Reviewer v2 定义修订和新 Review 修正，没有放宽 JSON 解析器，[原失败](full-20261108T103202Z/review-contract-failure.json)、[修订](full-20261108T103202Z/review-format-correction.json)。未实际触发质量结论 changes_requested；该分支由契约/浏览器回归覆盖。
- 续跑探针只取未被 retry_of 替代的有效 Run；旧失败保留。`--resume` 验证数据集/仓库/组/供应商上下文和原成功日志哈希，只复用成功副作用，最终持久事实始终重验；[拒绝篡改/错误上下文契约](resume-contracts.json)。
- full 崩溃重启后探针曾继续使用旧 HTTP cookie 而返回 401；生产 Host 的认证正确。已修复探针在重启后创建新 Client，[实际重启回归](restart-auth-regression.json)证明旧 cookie 401、新 cookie 200；失败 full 保留，之后最终核验重新运行。
- Git-only fixture 去除 Docker 前置依赖；Chromium teardown 等待路由响应排空；doctor 只读查询实际 Secret Service 集合而非仅看环境变量；嵌套探针解析本机镜像身份。最终离线、资源和实际凭据检查覆盖这些修复。

## bwrap / AppArmor 的实际边界

原错误 `bwrap: Failed to make / slave: Permission denied` 表示内层 bubblewrap 在准备自己的 mount namespace 时被外层 AppArmor 拒绝，尚未运行工程工具。原容器实际为 `docker-default (enforce)`；默认策略的 mount 限制见 [Docker 官方说明](https://docs.docker.com/engine/security/apparmor/)。

用户明确批准并在终端安装加载 `devdesk-harness-v1`，这是相对默认策略的有限挂载许可变更，也是原计划“不扩大权限”条款的已授权例外。策略基于 [Moby v0.3.0](https://github.com/moby/profiles/blob/apparmor/v0.3.0/apparmor/template.go)，按 [bubblewrap v0.8.0](https://github.com/containers/bubblewrap/blob/v0.8.0/bubblewrap.c) 的准备路径限定根传播、/tmp 准备根、/newroot bind/remount、tmpfs/proc/devpts、pivot/unmount。没有 blanket mount 许可。

固定文件 `/etc/apparmor.d/devdesk-harness-v1`，root:root、0644；SHA256 `3af35216df62da5ff6bfbe54b47506b219f26ccc99c944d84b826d66870cdb2c`。Host 检查固定文件归属/内容，不满足时失败；策略内容同时纳入容器和 adapter 构建身份。安装器只安装/加载该命名策略，不修改 docker-default、宿主 sysctl 或其他容器，不重启 Docker。

外层仍非 root、cap-drop=ALL、no-new-privileges、只读根、受限 /work 和 /state，没有 Docker socket/用户 HOME/宿主 PID/network，也未设 privileged/unconfined。实际 canary 允许工程写入但拒绝模型工具读取隐藏认证状态，[正例](named-policy-runtime.json)；外层 CapEff=0，新 user namespace 内尝试在未允许的 `/tmp/unapproved/` 挂 tmpfs，被内核明确 error=-13 拒绝，[负例](apparmor-negative.json)、[audit](apparmor-audit.log)。初次负例因 unshare 默认 rprivate 也被拒绝，调整探针 `--propagation unchanged` 后验证目标拒绝；[初次结果](apparmor-negative-initial.json)未删除。

## 凭据、取消、跨项目恢复与资源

[最终包 Linux 凭据探针](final-package-credentials.json)通过真实 Chromium→Host→Secret Service：随机测试条目保存/更新/删除、旧冻结引用保留、重启、既有项目读取、响应/SQLite/浏览器持久存储无秘密。故障只作用于独立测试 Host 的不可达 D-Bus，无明文后备；**没有锁定用户 collection，真实锁定场景未实测**。

[R8 恢复](full-20261108T103202Z/R8-recovery.json)记录两供应商各三轮原生 Session 续用、角色隔离、默认设置切换后历史绑定不变、真实无效 Codex 模型失败且不回退 Sim、多选用户问题与迟答拒绝。Codex 实际 interrupt acknowledged 后停止容器；DSH 实际 forced-stop，分别保留真实能力差异。

崩溃时两供应商输入均已 consumed，唯一匹配数据集/端口的专用 Host 被 SIGKILL；重启时另一个项目保持 active，后台项目执行者也纳入恢复，external IDs 保留且未重发输入。未知结果通过显式取消收尾；所有执行者的停止、协议封存和释放均复核。没有终止用户 Codex 桌面进程或清理无关 Docker 资源。

[最终秘密/资源检查](final-secret-and-resources.json)在实际数据集所属 DeepSeek key 与 OAuth token 字段只进入进程内存的条件下，扫描报告（含旧失败）、全部 SQLite/WAL 和已提交 Artifact，614 个文件、两个归属进程 argv 均无匹配；数据集所属 Docker 容器为零。该检查不将合法的私有原生 OAuth 文件当作泄漏目标，也不输出秘密或秘密摘要。后续文档打包的边界见包回执。专用 51751 Host 为用户保留运行，明确不计为已释放。

## 独立 Linux 发布包

产物 `dist/devdesk-0.2.0-x86_64-unknown-linux-gnu.tar.gz`，最终压缩包 SHA256、清单、解压位置和内容等价检查以 [package.json](package.json) 为准。原生 Host SHA256 为 `c8ab152149e55f9f3075f69fbc4a6eb1cd0c33f5aea15963e05e19f749ff0561`。

包解压到 `/tmp/devdesk-ubuntu-accepted-package-xz92wnbj/`，从包自身目录启动，使用本轮专用数据集，未依赖源码工作目录。实际证据：[HTTP/SSE](final-package-http.log)、[Secret Service](final-package-credentials.json)、[Codex 两轮](final-package-native-codex.json)、[DSH 两轮](final-package-native-dsh.json)、[优雅重启/历史/凭据](final-package-restart-verification.json)、[重启后 Chromium](final-package-browser.json)、[截图](final-package-browser.png)。

包只有一份 Host/嵌入 Web、容器配方与 Linux 专用策略安装器，无验收 data、OAuth/key、macOS helper 或签名私钥。完整镜像/系统依赖为宿主安装前提；AppArmor Docker 宿主按包内说明安装经审阅的命名策略。文档收尾后的压缩包重新生成时，Host、容器配方、策略及安装器必须与上述实测包逐字节一致；该比较和新包 HTTP 结果单独记录，不将文档重打包伪称重新执行模型。

## A01–A22 结果与证据边界

“实际＋契约”表示正常链路在本机完成，未实际触发的故障由离线注入补充；不声称全部外部故障均自然发生。

| 编号 | Ubuntu 结果及证据 |
|---|---|
| A01 | 实际：Codex→DSH→Codex，新会话采用选择，既有绑定/native IDs 不变；R8、[冻结绑定](frozen-bindings.json)。 |
| A02 | 实际：两供应商各三轮续用，跨项目/角色独立；R3/R8、[角色引用](role-reuse.json)。 |
| A03 | 实际＋契约：两镜像各并发四次登记同 ID、跨项目/角色共享五层；不同版本并存/摘要冲突由 registry 契约验证，未另装新版本。 |
| A04 | 实际＋契约：无效 Codex 模型、DSH max-tokens 和 HTTP 402 明确失败，无 Sim；限流/公共断网/通用超时仅契约注入，OAuth 超时单列。 |
| A05 | 实际：Chromium 讨论、提取及显式采纳 Project Requirement 在 Feature 前；R5 浏览器回执/截图。 |
| A06 | 实际：Requirement/Design/TaskPlan/TestPlan、两个正式 Review、initial Snapshot/Task worktree 来源完整；R5 planning。 |
| A07 | 实际＋契约：native client_message_id 重放幂等；重复采纳/陈旧提案/越界由 Core、HTTP 和 Web 契约验证。 |
| A08 | 实际＋契约：初始双供应商 Attempt 并行、独立工作目录/容器/Session；DSH 后续修正另建；额度/写许可故障由调度契约补充。 |
| A09 | 实际：顶层断言的 environment 失败被拒绝；两最终 Attempt 真正 Red，Green/verify 同 SHA/镜像/配方；R6。 |
| A10 | 实际＋契约：独立 native Reviewer 最终接受；无人工 Override，原 JSON 契约失败和预算失败保留；质量退回/Override 分支仅契约与 Web 回归。 |
| A11 | 实际：Task/Feature 单父候选与精确目标匹配；R7 和最终 Git 核验。 |
| A12 | 实际：本轮 PR #6/#8/#9 merged，head=mergeCommit=候选，最终由 Chromium 接纳；R7/远端核验。 |
| A13 | 实际＋契约：旧目标候选真实拒绝，multiply 重建/重验；CAS/精确 lease 与额外外部写者竞争由 Git 契约补充。 |
| A14 | 实际只读诊断＋契约：专用仓库 compatible，规则不兼容的仓库诊断 blocked，不修改规则/无管理员绕过；R4/父轮诊断。 |
| A15 | 实际＋契约：Codex interrupt ACK 后 stop，DSH forced-stop，写许可撤销、协议封存/释放；迟到回执由契约补充；R8。 |
| A16 | 实际：两输入 consumed 后 SIGKILL 专用 Host，跨项目启动恢复，外部 IDs 不变且不重发，显式取消；R8。 |
| A17 | 实际资源＋契约：Git/Docker/PR 幂等正常链路真实；外部成功但落库前故障由 Core 注入验证，未宣称实际在该窗口崩溃。 |
| A18 | 实际资源＋契约：全部执行者封存/释放、共享镜像保留；导出/清理失败和人工重试由故障注入/Web 覆盖，未制造 daemon 故障。 |
| A19 | 实际：HTTP/SSE 分库/项目选择/请求隔离，后台项目实际执行者恢复，最终包重启保留项目及历史。 |
| A20 | 契约＋实际新数据集：旧库升级/身份来源及 Sim 全回归通过；本 Ubuntu 数据集独立，未搬用 macOS 数据。 |
| A21 | 实际＋契约：非 root、cap-drop、只读根、工作区边界、隐藏认证 canary、命名策略允许/拒绝；测试进程无模型/GitHub秘密，实际秘密扫描通过。 |
| A22 | 实际：Feature 收尾 Tests/Doc、递归 Delivery、Snapshot、主干候选/Review/merged PR、全量哈希与源码外发布包通过。 |

## H01–H09 / INV01–INV13 映射

| 条目 | 证据与边界 |
|---|---|
| H01 | 实际 Linux ELF、固定双镜像、registry 登记/复用、不可变绑定。 |
| H02 | R3/R8 两供应商连续原生 Session，跨项目/角色隔离。 |
| H03 | 实际 JSONL 协议封存和 Artifact 哈希；有序持久化及畸形协议由回放/契约补充。 |
| H04 | 实际无效模型、max-tokens/QUOTA、取消和 SIGKILL；有界请求/执行超时由契约补充。 |
| H05 | 实际 Docker 工具、Git/Node 测试、固定代码/镜像/配方和 Artifact。 |
| H06 | 实际原生续用/重启，未知输入不重发；后继快照规则由契约补充。 |
| H07 | 双 Task 初始并行、实际工作目录隔离；全局/项目/Harness 额度由调度契约补充。 |
| H08 | 旧库/协议身份回归；未把模拟记录改成真实。 |
| H09 | 实际停止/封存/清理和共享安装；导出/清理故障由契约注入补充。 |
| INV01 | R5/R6/R7 的正式文档、验收项、Run、Review、Delivery 来源链。 |
| INV02 | 真实 Git/worktree、Task/Feature/主干候选。 |
| INV03 | provider/version/model/role/credential/installation 不可变绑定，默认切换不改历史。 |
| INV04 | 实际 DAG/并行；额度、写许可/冲突由 Core 调度契约补充。 |
| INV05 | 实际 Run/Turn/Execution/Artifact 可查询且哈希一致。 |
| INV06 | 正式独立 Review，失败不改写；Override 仅契约，本轮未使用。 |
| INV07 | 实际单父候选、CAS/lease、陈旧拒绝与 merged 远端校验。 |
| INV08 | 实际 Git 对象/refs、六份 Snapshot 和 Artifact 保留；Archive 环境重建仍延期。 |
| INV09 | 实际资源归属、协议导出/释放；失败恢复用契约补充。 |
| INV10 | 实际 Web→Rust→Docker/GitHub；macOS native 客户端仍延期。 |
| INV11 / INV12 | Archive/data 搬迁按原范围延期，未计为本轮实测。 |
| INV13 | 安装内容、adapter 构建与 AppArmor 策略锁；旧构建重试实际拒绝，新 Attempt/Review；同冻结配置文档技术重试实际通过。 |

证据文件摘要清单见 [evidence-sha256.json](evidence-sha256.json)。当前结论覆盖本次固定平台、工具/供应商版本和用户批准策略的 Web 工程范围；未将未实测的 collection 锁定、新供应商版本、原生客户端、Archive 或外部故障注入扩大为实机成果。
''')
# Update current status only; historical reports remain immutable.
p=Path('docs/plans/ubuntu-web-acceptance/PLAN.md');s=p.read_text();s=s.replace('日期：2026-11-08。状态：已在 Ubuntu 24.04.5 / x86_64 执行；离线基线、Linux 测试凭据和运行包基础检查通过，用户批准的专用 AppArmor 策略已解决 Docker 嵌套沙箱兼容问题，全部组 full 验收进行中，Ubuntu 尚未整体通过。','日期：2026-11-08。状态：Ubuntu 24.04.5 / x86_64 的 U0–U6 已完成并通过。全部组 full（显式续跑）、最终 verify-existing、完整安装复用及源码外 Linux 发布包均通过；AppArmor Docker 宿主使用用户明确批准的专用策略。')
s=s.replace('本轮证据：[ubuntu-20261108T095147Z 验收报告](ubuntu-20261108T095147Z/acceptance.md)。`full`、`verify-existing`、完整安装复用探针均保留非零结果；下方勾选只表示该项完整满足。','本轮最终证据：[Ubuntu 完成报告](ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。[首次失败报告](ubuntu-20261108T095147Z/acceptance.md)及历次失败原始文件保留，不改写为成功；最终 full、verify-existing、安装探针退出 0。')
s=s.replace('- [ ]','- [x]')
s=s.replace('本轮部分证据：本机/镜像登记、默认 Codex → DSH → Codex 已通过；DeepSeek key 已配置并真实调用，Codex 官方 OAuth 按报告中的当前状态继续。隔离测试条目已完成 Linux 凭据保存/更新/删除、重启读取与不可用 D-Bus 探针；未锁定用户 collection。','本轮结果：本机/镜像登记、默认 Codex → DSH → Codex 和历史冻结绑定通过；两供应商实际登录/调用及重启后读取通过。隔离测试条目完成保存/更新/删除、重启与不可用 D-Bus；未锁定用户 collection，真实锁定场景未实测。')
s=s.replace('- [x] U4：独立 Reviewer 接受或退回均保留；拒绝后用新 Attempt 修正，不能覆盖失败证据。递归 Delivery 来源、测试配方和固定版本证据完整。','- [x] U4：独立 Reviewer 接受已实际完成，技术失败保留并通过新 Review 修正；失败实施通过新 Attempt 修正，未覆盖证据。质量退回/人工 Override 由契约和 Web 回归覆盖，本轮未实际触发。递归 Delivery 来源、测试配方和固定版本证据完整。')
old='本轮完整入口实际退出 1：Git 2/2、CommandTests 2/2 通过；Codex/DSH 嵌套沙箱报 `bwrap: Failed to make / slave: Permission denied`。诊断容器实际为 `docker-default (enforce)`，已按归属回执清理；交付闭环保持未通过。独立补充已通过 DSH 三轮原生讨论、浏览器采纳 Project Requirement、专用仓库连接及 Feature worktree；三个讨论执行者已封存释放，11 个 Artifact 哈希通过。'
new='首次完整入口因 docker-default 下 bwrap 挂载权限失败退出 1，原证据保留。用户批准并加载命名策略后，真实 Git 2/2、Docker 4/4、CommandTests 2/2 通过；最终全部组 full 经显式 `--resume` 19/19 通过。双供应商初始 Attempt 实际并行，DSH 后续修正另建；最终两 Task、独立 Review、单父候选和真实 PR #6/#8/#9 merged，Feature completed。精确 SHA、Run/Artifact 与截图见完成报告。'
assert old in s;s=s.replace(old,new)
s=s.replace('本轮完整安装探针因缺少跨项目/角色 Session 引用退出 1；独立登记部分已证明两镜像各并发四次返回原安装 ID、实际共享五层。`verify-existing` 因首次闭环证据缺失退出 1，不作为通过。','首次安装探针/verify-existing 缺少完整前置证据而失败，原结果保留。建立真实跨项目/角色引用及完整闭环后，两者均退出 0。实际 consumed-input SIGKILL 恢复、另项目 active、后台执行者回收、不重发与显式取消通过；最终 331 个 Artifact 哈希一致，79 个执行者均封存释放。')
s=s.replace('生成 `x86_64-unknown-linux-gnu` 包及 SHA256；包内容与摘要见本轮 `U6-package.json`。','生成 `x86_64-unknown-linux-gnu` 包及 SHA256；最终包内容与摘要见续验目录 `package.json`，父目录 `U6-package.json` 为历史产物。')
s=s.replace('本轮包内 Host 已在源码外通过 HTTP/SSE、Linux 系统凭据、已有项目重启查询和释放检查；双 Harness 真实会话保持待验。','最终包内 Host 已在源码外通过 HTTP/SSE、Linux 系统凭据、双 Harness 各两轮真实续聊、重启后凭据/已有项目/Snapshot 查询与 Chromium 检查；所有执行者释放。文档收尾重打包按 Host/配方/策略/安装器逐字节等价核验并复测 HTTP；保留独立专用 Host 供用户查看。')
s=s.replace('后续终端恢复相同绝对路径：','后续终端恢复相同绝对路径：')
s=s.replace('上述代码用于创建新轮次；','本轮完整工程证据位于报告根的 `resume-20261108T102305Z/full-20261108T103202Z/`。同轮 `verify-existing` 的 `--reports` 应指向该目录；新 full 使用新目录。失败 full 需要继续时显式 `--resume`，它核对上下文及成功日志摘要，并保留旧报告；不能用于已通过的 full。\n\n上述代码用于创建新轮次；')
p.write_text(s)
replacements={
 'README.md':[('Ubuntu 已通过离线基线、Linux 测试凭据和运行包基础检查，专用 AppArmor 已解决嵌套沙箱兼容性，完整工程验收进行中','Ubuntu 24.04.5 / x86_64 也已通过双 Harness Web 工程闭环、Linux 系统凭据、安装复用、取消/崩溃恢复和独立运行包验收，AppArmor Docker 宿主使用经批准的专用策略')],
 'docs/ubuntu-web-acceptance.md':[('离线基线、Linux 测试凭据和运行包基础检查通过，已批准的专用 AppArmor 解决了嵌套沙箱兼容性，完整工程验收进行中','U0–U6 全部通过，包含双 Harness 工程闭环、真实 PR merged、系统凭据、安装复用、取消/崩溃恢复和独立发布包；AppArmor Docker 宿主使用经批准的专用策略'),('按该计划继续执行；下面保留简要命令入口。','下面保留新一轮验收的简要命令入口；本轮失败及显式续跑历史均在独立报告中保留。')],
 'docs/run-binary.md':[('Ubuntu 24.04.5 / x86_64 包已通过源码外 HTTP/SSE、测试凭据和项目重启检查','Ubuntu 24.04.5 / x86_64 包已通过源码外 HTTP/SSE、系统凭据、双 Harness 各两轮原生续聊、完成项目与 Snapshot 重启检查')],
 'docs/plans/README.md':[('macOS 凭据通过，Ubuntu 测试凭据/包基础通过，工程闭环待验','macOS/Linux 系统凭据、Ubuntu 双 Harness Web 闭环与发布包通过'),('两供应商讨论/安装复用/沙箱通过；完整工程验收进行中','U0–U6 通过；真实 PR、恢复、发布包与批准的命名策略均有实机证据')],
 'docs/plans/web-platform-and-credentials/PLAN.md':[('P2 已在 Ubuntu 24.04.5 / x86_64 验证离线基线、Linux 测试凭据和运行包基础能力，两供应商认证与续聊通过，专用 AppArmor 沙箱实测通过，完整工程验收进行中','P2 Ubuntu 24.04.5 / x86_64 的 U0–U6 通过，涵盖 Linux 凭据、双 Harness Web 工程闭环、真实 PR merged、复用/恢复与独立发布包，AppArmor 使用用户批准的专用策略'),('，尚未整体通过。','，全部必要项已完成，首次失败及续跑历史保留。'),('Ubuntu 已实际执行离线检查、镜像构建、Linux 凭据和运行包基础验证；双 Harness、Web 工程闭环与恢复继续保留待验。','Ubuntu 已实际完成离线检查、镜像构建、Linux 凭据、双 Harness Web 闭环/恢复与独立运行包；full（显式续跑）、verify-existing 和完整安装探针均退出 0。')],
 'docs/plans/web-platform-and-credentials/web-platform-and-credentials.md':[('Ubuntu 已实际通过离线基线、Linux 测试条目和源码外运行包基础检查；双供应商认证/原生讨论、完整安装复用和专用 AppArmor 已实测通过，工程闭环验收继续进行。','Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过：Linux 凭据、双供应商工程闭环、真实 PR merged、安装复用、取消/崩溃恢复与源码外运行包；专用 AppArmor 策略经用户批准并实测允许/拒绝边界。'),('2. Ubuntu 桌面环境实际编译、Secret Service 保存/读取、双 Harness、Web 工程闭环/恢复与运行包检查。当前 Linux worker 或旧原型报告不能替代该宿主验收。','2. Ubuntu 桌面宿主已实际完成编译、Secret Service 保存/读取、双 Harness Web 工程闭环/恢复与运行包检查；使用独立数据集和报告，未以 Linux worker 或旧原型报告替代。'),('后续 Ubuntu 工程闭环、正式模型凭据与恢复继续按独立报告执行。','Ubuntu 工程闭环、正式模型凭据与恢复的最终结果见上述独立报告，保留全部首次失败与续跑历史。')],
 'docs/acceptance.md':[('本轮 Ubuntu 24.04.5 / x86_64 已实际通过完整离线基线、Linux 测试凭据和运行包基础检查，真实工程闭环在 Docker 嵌套沙箱阶段失败，尚未整体通过。证据、修复和逐项边界见 [Ubuntu 独立报告](plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/acceptance.md)。','本轮 Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过：完整离线基线、Linux 系统凭据、双 Harness Web 工程闭环、真实 PR #6/#8/#9 merged、安装复用、取消/跨项目崩溃恢复和独立 Linux 发布包。Docker 嵌套沙箱使用用户明确批准的专用 AppArmor 策略；首次失败及显式续跑历史保留。full、verify-existing、完整安装探针均退出 0，证据与实际/契约边界见 [Ubuntu 完成报告](plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。')]
}
for filename,pairs in replacements.items():
 p=Path(filename);s=p.read_text()
 for before,after in pairs:
  assert before in s,(filename,before);s=s.replace(before,after)
 p.write_text(s)
print('updated report, plan and',len(replacements),'current documentation files')
