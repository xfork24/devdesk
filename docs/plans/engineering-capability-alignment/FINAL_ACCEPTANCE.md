# 工程能力对齐：本地 main 验收报告

日期：2026-11-14。六阶段已实施、提交并合入本地 main。最终验收完成：Rust 默认 Feature/Task 图在 Driver 启动后无人工业务干预地通过，Agent 默认图经辅助恢复通过，经理五项配置创作通过。A01–A12 的实际覆盖与未验证范围逐项列于下文，不作所有平台和故障时点均已覆盖的声明。

## 实施与集成

原状问题及代码依据见 [ASSESSMENT](ASSESSMENT.md)，对另一 agent 的计划逐项独立评估见 [REVIEW](REVIEW.md)。实现遵循六份阶段计划；各阶段的实现范围、测试与限制见对应 EXECUTION。

| 阶段 | 提交 | 实现结果 |
|---|---|---|
| S1 | `cfc40c2` | 四层职责与依赖门禁、core ports、Host 装配和可信主体 |
| S2 | `5336f89` | 类型化配置图、精确引用、原子变更集、跨库候选隔离与历史修订选择 |
| S3 | `09f2bb2` | 配置化运行资源、内容身份、实际驱动与协议能力核验 |
| S4 | `dd2b4e1` | Rust/Agent 共享控制内核、有界自动驱动、唯一租约与恢复 |
| S5 | `37298c1` | 独立项目经理工作台、全项目有限查询、配置与实例管理 |
| S6 | `317c191` | 可执行 Spec、冻结输入证据定位、共用 lint/接纳和本地证据归档 |

上述阶段在独立 worktree `codex/engineering-capability-alignment` 依次实施、限定验收并提交。集成提交 `f01dec6a05308a11fe94016405829156e9fa05a5` 保留 main 已有 `6d7d2a0` 的工作；34 份原 untracked 计划/review 逐文件核对并备份。全部合入本地 main 后，才开始真实 Feature/Task 工程验收。没有向产品仓库远端推送本轮 main。

## 环境与证据

- 宿主为 macOS arm64；Rust 使用项目锁定工具链；业务 Agent 为真实 Codex Harness，测试在实际 Docker Linux 容器中运行。
- 专用外部测试仓库由用户指定为 [xfork24/devdesk-project-1](https://github.com/xfork24/devdesk-project-1)。测试允许产生分支、提交及 PR。
- 本地完整证据根：`/Users/hengliu/workspace/devdesk-acceptance/engineering-capability-alignment-2026-11-14`。本文件只保存脱敏摘要，原始日志、二进制、SQLite、业务产物和运行脚本保存在该目录。
- 修复权限预设源码目录依赖后的独立 Release 包，源码提交为 `3f061dfaccd776d62a67804cd4751b51e9b433d0`，SHA256 为 `920c17b20d3cfd4c274467828e90b674d004a06ed612e21ebe7ce911eb2d6eef`，见 `standalone-preset-final/BUILD.json`。该包已在操作系统拒绝读取项目及 worktree 源码目录时通过权限 UI/预设选择与重新加载验收。
- 前期独立 Release 包 `0943fb0f933191cd589132e9a30f00cb452715ca` / `f001258a8d19a76ccae43eb5f8abcd5726c26a0fb930a8da7b398213207c4fb6` 已在源码目录外通过全部 57 项浏览器测试；仅改变工作目录不能证明完全没有访问编译时源码，因此另增加上述真实源码隔离。
- 真实 Agent 恢复轮使用包 SHA256 `a5cbc8c0df92e81843b572c7e6a48dd7af7fac907214c49c4aeb407f8b42ee06`；已冻结轮次的精确 runner/helper 文件、参数和包摘要见 `native-default-flow-final/attempts/*/ATTEMPT.json`，早期轮次通过提交与日志追溯。不得把不同包混记为同一轮。

## 已完成回归

| 验收 | 结果 | 本地证据 |
|---|---|---|
| `cargo test --workspace --locked` | 274 通过，0 失败，17 默认忽略 | `rust-workspace-runtime-facts-final.log` |
| Web production build 与 Release 编译/签名 | 通过 | `web-current-main-build.log`、`release-current-main-build.log` |
| 最新独立 Release 包浏览器全套 | 57/57，被测主 Host 受 OS 源码目录读取拒绝；客户端测试仍使用开发工具 | `browser-runtime-facts-full-source-isolated.log`、`standalone-runtime-facts-final/full/browser-report/` |
| 最新运行事实合同及经理状态定向浏览器回归 | 4/4，同样的主 Host 源码隔离 | `browser-runtime-facts-source-isolated.log` |
| 最新包 HTTP、认证、重启与隔离 | 通过 | `http-runtime-facts.log` |
| 前期 Release 独立包浏览器全套 | 57/57，源码目录外运行 | `browser-standalone-current-main.log`、`standalone-current-main/browser-report/` |
| 权限预设修复后的受影响浏览器场景 | 1/1，OS 拒绝源码目录访问；含实际目录选择/重载/恢复及修订绑定 | `browser-preset-source-isolated.log`、`source-isolation-preflight.json` |
| 初轮 Debug 浏览器及修复 | 首轮 52/54；旧 fixture 修复后定向 2/2 | `browser-debug.log`、`browser-followup.log` |
| HTTP、认证、Host/Origin、连接、重启与隔离 | 独立复制包通过 | `http-standalone-current-main.log` |
| core/domain 脱离外层及配置资产独立构建 | 通过 | `core-independent-build.log` |
| 分层、出厂目录、数据集协议及模板重置 | 通过 | `architecture-current-main.log`、`dataset-protocol.log`、`template-reset.log` |
| 两套真实资源配方协议探针及跨数据集镜像保留 | 显式 4 项通过；不调用模型 | `runtime-retention-final.log` |
| 证据导出、脱离原数据目录复核及拒绝私有路径 | 通过；补充实际 WAL 源、稳定文件清单和独立全包核对 | `evidence-archive-wal-final.log`、`archive-verification.json`、各轮 `evidence-index.jsonl` |

默认忽略项是显式 Docker/供应商/权限/GitHub 外部场景，不能把默认命令成功解释为这些场景都已执行。真实端到端与显式资源测试补充了其中部分覆盖，未执行部分仍保留限制。

## 真实项目经理配置创作

在项目 `d70bc5c9-aa64-407d-beaa-f6277776e182`，人类向独立 workspace manager 提出五项自然语言请求，全部由实际经理 Native tools 查询、预演、应用并读取结果：

1. 在已有工作流后继修订加入依赖 `first` 的 `review` 控制节点。
2. 创建指定 Agent 的项目 instance，恰好一个，不绑定项目 Role、不启动执行。
3. 发布 Spec 后继，并发布精确采用它的 Delivery 后继；旧 Delivery 仍引用旧 Spec。
4. 发布 Skill、Capability、Agent 后继，使指定 Agent 使用新 Skill；模型、Harness 和 Tools 保留。
5. 退役未使用配置，保留历史源。

独立一致读得到 22 项持久化断言通过。五次成功 apply 均关联相同 preview_hash 的成功预演，且 call_id 出现在真实 coordinator journal。旧 preview 重用失败、原 600 秒预算中断和后继身份恢复记录均保留。显式选择的后继 Permission Context 仅将预算改为 1800 秒，没有增加其他授权。项目默认配置未采用测试源，当前 Feature 冻结配置不被这五项未来修订改写。

证据：`manager-authoring/report.md`、`independent-audit.json`、`native-receipt-audit.json`、`scenario-*-session.json`、`scenario-*-records.json` 和各轮运行日志。

## main 验收发现与修复

| 实际问题 | 修复与核对 |
|---|---|
| 测试 runner 没显式选用共享 Agent 驱动；旧浏览器 fixture 缺新查询与导航 | `788fa8c`、`a9d7f45`；补跑受影响断言，后续 Release 全套通过 |
| 另一数据集重建共享 Docker 标签后，旧冻结镜像失效 | `47431f0`；保留内容地址镜像 tag，并按实际内容复用；真实跨数据集重建验证 |
| 模型 locations Schema 只有 object，无法获知严格字段 | `208a90b`；完整 Schema、映射与实际生成合同断言；旧非法报告保留 |
| 归档将生产项目相对 Artifact 路径重复拼接 artifacts | `aad2413`；改按项目根解析并限制产物目录；旧失败包保留、校正包独立创建并验证 |
| SQLite WAL 源的归档清单包含关闭连接后消失的临时 sidecar | `78011ec`；显式关闭连接、将备份转成独立单文件，再计算清单；WAL 回归和独立全部文件/Artifact/SQLite 表计数复核通过。旧清单保留并标为不完整，新 stable 包另建 |
| 实际运行镜像符合要求，但上游运行事实未送达 Agent；业务拒绝又未出现在驱动等待列表 | `7ef4008`；冻结只含公开字段的 Session 选择与对应 Turn 的真实停止证明，随实际任务送达；补齐 RPC Schema 与 UI 展示；等待投影使用现有门禁谓词。保留原拒绝报告、不批准和不放松完成规则；274 Rust 与源码隔离浏览器/HTTP 通过 |
| 当前 renderer 重建历史任务，导致旧任务摘要变化而无法重启 | `e21f621`；恢复实际发送且摘要一致的历史消息，验证冻结源与输入；不放松摘要检查 |
| 接纳旧预算后根经理无法采用后继组合 | `eeac8ee`；核对旧 writer 已停，再建新控制器/策略身份并交接根租约；旧身份和迟到调用被拒绝 |
| 权限预设默认路径回退到编译时源码目录，工作目录外启动掩盖了该依赖 | `3f061df`；无附带预设目录时使用包内数据生成内容地址缓存，保留独立的自定义目录；14 项通过/1 忽略，OS 源码隔离下 UI 场景通过，并加入架构回归门禁 |
| 验收观察器过早终止经理可恢复的失败；长请求结果不明；项目源误查全局目录 | 有界观察真实进展、保留每轮 runner/摘要；延长单次请求且不重试 mutation；按本地目录核对实际提交结果 |

验收期间独立合入的 `e8010eb` 精确引用说明与 `6a123c8` 经理状态加载修复也已保留并纳入回归。精确 quote 必须等于所选范围的全部文本；根 pointer 配短摘录的报告确实不合法。本轮没有把校验放宽成子串匹配来使测试通过，而是保存拒绝报告并由经理通过合法变更补充格式说明。

## 真实工程与矩阵状态

真实 Agent 工程根 `971d3cd8-cb96-46ca-8828-303c2699cfbf` 已完成，Feature 与唯一 Task 均为 completed。唯一 Task 通过 [PR #4](https://github.com/xfork24/devdesk-project-1/pull/4) 合并，实际 head/merge commit 为 `f4c7463740de6b48ab8783551eecd91303b5a951`；Feature 通过 [PR #6](https://github.com/xfork24/devdesk-project-1/pull/6) 合入 main，实际 head/merge commit 为 `6e0b8467b807d977bb6716ec6040732d668d646b`。两个 PR 已另行直接查询 GitHub，均为 MERGED 且与实测候选一致；最终候选双文件测试 33/33。

`raw-evidence-validation.json` 独立核对通过：五条原始运行记录、两轮增量 Red/Green、独立测试未改动、环境与终止事实、原生命令 stdout 观察、两份大输入的完整续读、最终候选文件与证据一致；无 bypass/waiver、无人工确认。经理显式完成子流程及根流程，保留其真实控制调用。最终稳定归档为 413 个文件，已复核每个文件摘要、登记 Artifact、SQLite 完整性和表计数；原运行中复制的认证目录和 auth.json 已清理。精确包与索引见 `native-default-flow-final/FINAL_ARCHIVE.json`。

原无人干预首轮失败，此后的同库恢复包含技术说明、运行计划格式说明后继和显式预算选择；不能报告成一次无人干预成功。独立真实 Rust 原语图 `fba7ff19-311d-4649-a4c9-b97b6612d6da` 已在停止/重启后完成实际 Agent、Tool、Control 和子 Workflow，没有该根的经理 Controller；它不能替代完整默认工程图。

完整 Rust 图另保留三次准备失败：重复创建已有认证目录、控制请求漏传顶层 plan_revision_id，以及选择驱动时提交的流程修订已过期。均发生在 Driver/业务模型启动前；相应归档及零 Turn/Controller 审计保留，不记作业务通过。首次实际运行的根为 `63df4b05-40e7-49d1-9e94-5f7ca2522321`；包为上述 `920c17…`，OS 拒绝读取源码目录，PATH 屏蔽本机 Rust/Node 开发工具。额外选用一份澄清既有证据格式的 Reviewer Skill；默认业务图、业务 Spec、门禁和其他角色组合保留。Driver 启动后观察脚本只读，不代填、批准或重试业务结果。此首轮已在 Task 评审正确停止：评审无法从收到的输入证明执行镜像满足计划中的固定配方要求。独立读取持久化 RuntimeBinding 与 RuntimeExecution，确认实际镜像完全相同，缺的是交付给 Agent 的可信信息；25 次 dispatch 和 23 次 accept 均为 Driver，零 Controller、零人工批准。拒绝报告、实际任务、镜像比对及停止后的稳定归档保存在 `rust-default-flow/`，见 `runtime-image-gap-audit.json`。

修复后的新包源码提交 `7ef40086dfe42bc8c50bdb16dc266279c1b0f722`，SHA256 `ab6952ba1c9aff9e0b132643e744f5cedaa6dfd704c9ee25ea4ded7a7c239140`，已冻结在 `standalone-runtime-facts-final/`。全新 `rust-default-flow-retry/` 使用相同的源码访问拒绝与宿主工具屏蔽；额外 Reviewer Skill 同时澄清引用定位和可信运行事实的使用范围。该轮现已完成：根 `81cb475c-ebbe-4890-b139-3948cfaec7f9` 的 22 步与子实例 `ba360b42-f905-4e57-9dc1-2e4af0fae5b3` 的 20 步全部完成。42 次 dispatch、42 次 accept 和 2 次 complete 均为 Rust Driver；零经理 Controller、零人工审批或豁免。唯一 Human 控制决定是在 Driver 启动前绑定已授权业务输入；准备阶段明确收到 CAS 拒绝后读取新修订再选择 Driver，完整请求记录保存在 `driver-selection-preparation.json`。

完整 Rust 图的 Task 通过 [PR #7](https://github.com/xfork24/devdesk-project-1/pull/7) 发布，head/merge 为 `6031186d8579a6a8599f3daab9462db50f3c8a6b`；Feature 通过 [PR #8](https://github.com/xfork24/devdesk-project-1/pull/8) 发布至 main，head/merge 为 `54ce8e3b2aae8ac0eed22ff4fd5c258e5b733de8`。GitHub 直接查询与内核回执一致；平台通过确切远端租约推进已实测候选，GitHub 记录间接合并，没有调用 merge 端点。Feature 最终候选测试 10/10。独立核对通过五条原始记录、两轮增量 Red/Green、独立测试未改写、两份大输入完整续读、16 份实际原生任务正文及 29 条确切生产者停止证明。所有相关任务/实际执行镜像均与本轮授权配方 `sha256:4d915bb4e1aab1bd64ea75849a1ba5d4791f582a58b79cd5898755ffc9a6c717` 相同。

本轮业务完成后的附加离线审计器最初把 `turns.input` 的来源引用 codec 当成 JSON 正文，导致外层 runner 非零退出。原脚本、摘要、退出日志保留；仅修正并重跑离线审计，改用摘要一致的封存 User 正文与实际原生 turn/start 帧，未重跑或改动业务流程。修正后的独立审计通过，结果见 `rust-default-flow-retry/RESULT.json`、`runtime-facts-independent-audit.json` 及 `rust-default-final-independent.log`。因此区分“业务流程通过”与“原外层附加审计脚本退出失败”，不把该失败抹掉。

Rust 最终稳定归档含 117 个文件，Host 已停止，复制的认证目录及 auth.json 已清理。所有轮次共 9 份归档通过文件摘要、Artifact、SQLite 完整性及表计数复核；另保留 11 份历史不完整导出及实际原因。9 份完整归档也包括准备失败和业务拒绝轮，并不表示 9 次工程通过。部分准备轮目录重命名后的原索引路径保留，独立核对同时记录原路径和实际归档位置。

| 矩阵 | 当前状态与证明边界 |
|---|---|
| A01 分层 | 通过依赖门禁与独立构建；职责分析见 ASSESSMENT 和 S1 EXECUTION |
| A02 配置演进 | 通过精确历史修订/中间修订 UI 与真实五场景后继创作；旧任务按实际历史合同恢复 |
| A03 组合原子性 | 实际 SQLite 集成测试通过反向顺序创作、整体拒绝、陈旧预演、精确修订和隔离候选 |
| A04 运行资源 | 两份真实配方（相同 Codex 0.167.0 基础镜像、不同 LABEL 和内容摘要）完成 initialize/thread-start/工具注册探针；标签无关、内容与能力伪造拒绝。它验证独立配方与缓存保留，未证明两个不同 Harness 发行版的完整推理兼容 |
| A05 Rust 自动 | 通过：全新默认业务图另选格式说明 Skill，42 次派发、42 次接纳、2 次父子完成均为 Driver；零经理、零人工审批/豁免。PR #7/#8 直接查询为 MERGED，SHA 与实测候选一致。首轮有效拒绝保留 |
| A06 Agent 调度 | 同库恢复验收通过；实际经理原生调用驱动父子图，PR #4/#6 与实测候选完全一致。保留首轮失败，不记作一次无人干预通过 |
| A07 切换恢复 | 集成测试通过模式切换、人工门禁、停止恢复和迟到回执；实际旧库重启和根经理身份/预算采用已发生 |
| A08 经理创作 | 五项真实自然语言操作与独立审计通过，另有运行中计划格式说明的预演/采用 |
| A09 知情范围 | 无 Feature 工作台、授权隔离、分页/大输入读取有集成/浏览器覆盖；实际经理已查询跨父子图及配置，Agent/Rust 两轮的大产物完整续读均已独立核对 |
| A10 规范检查 | 原语、Schema、共同 lint/接纳、伪造位置拒绝通过；真实错误 quote 也被拒绝，不能据此宣称模型语义质量已机器证明 |
| A11 跨库故障 | 集成测试通过隐藏 staging 和重启恢复单一提交；没有逐一杀真实进程覆盖所有跨库故障时点，保留该限制 |
| A12 独立包 | 前期包源码目录外通过 HTTP/57 项浏览器；最新包通过 HTTP 与被测主 Host 源码隔离下全套 57 项浏览器；完整 Rust 图在源码隔离及宿主工具屏蔽下完成并通过离线复核 |

未将 DSH 工具桥、Windows/Ubuntu 宿主、所有供应商或所有权限组合写成通过。新增执行原语或新供应商协议仍需要代码；已支持原语内的修订、组合、参数和兼容资源选择由配置完成。语义判断仍保存实际判断者与来源，结构及定位成立不等于语义必然正确。

最终清点纠正了一项早期误报：Debug 包 `4ae9adc09934ba92c74a6c3a7a46add63a7acda081ab97bc5a26428cca32cf17` 的原件仍在 `native-default-flow/devdesk-host` 和 `native-default-flow-retry/devdesk-host`，两份副本逐字节摘要一致。旧包索引只扫描集中 `packages/`，遗漏这些运行目录副本；最终扫描已补齐，并复制到内容地址目录。所有归档声明的可执行包摘要现均有本地原件对应，见 `package-cross-check.json`。旧窄范围索引保留为历史记录，不代表最终结论。

完整浏览器套件的主 Host 使用上述独立 Release 包并受源码读取拒绝限制；连接切换 fixture 另启动辅助 Debug Host，其实际可执行文件也已冻结，不能把主 Host 的隔离约束推广到这些辅助进程。具体对应见 `BROWSER_PACKAGE_CONTEXT.json`。

本地完整证据的导航为证据根下 `EVIDENCE_INDEX.md`；固定文件校验清单为 `EVIDENCE_MANIFEST.json`，另有每份项目归档自身的 `MANIFEST.json`。原始可重新打开的数据集保留在本地，SDK 缓存与原始可变数据库不冒充最终固定快照；用于审计的原生 coordinator 日志另行计算摘要。
