# 计划与验收索引

每个计划使用 `docs/plans/<计划名>/PLAN.md`，同目录只保留 Markdown 契约、执行报告和验收说明。日志、JSON、截图、补丁和临时测试文件不保存为计划交付物；源码回归测试仍在 crates、tests、web/tests。

待实施方案必须明确标注状态，不能作为已实现的 API 或已通过的验收；核心设计约束同时记录在 [架构设计](../design/overview.md) 和 [仓库指南](../../AGENT.md)。

| 计划 | 报告与证据 | 状态与范围 |
|---|---|---|
| [工作流故障恢复与升级](workflow-recovery/PLAN.md) | 实施中 | Agent 自修、经理接手、实际重跑预算、历史与人工升级 |
| [工程库存储改造与百万级容量评估](project-payload-storage/PLAN.md) | [实施与验收](project-payload-storage/EXECUTION.md)、[容量与 I/O 报告](project-payload-storage/PERFORMANCE.md) | 正式工程库已迁移至 project 61，达到 120 MB 容量目标。部分相对 P95 探针仍有差距，百万级使用现有数据推算 |
| [架构与代码文档重写](architecture-documentation-rewrite/PLAN.md) | [实施与验证](architecture-documentation-rewrite/EXECUTION.md) | 2026-11-19：P0–P5 完成；重写架构和代码手册，24 篇旧页合并删除，文件索引独立；文档/架构检查与 8 项检查器回归通过 |
| [精简版性能基准与后续优化](streamlined-workflow-performance/PLAN.md) | [基准](streamlined-workflow-performance/BASELINE.md)、[执行](streamlined-workflow-performance/EXECUTION.md) | 2026-11-19：详细版出厂退役，精简版完整交付 22 分 54.790 秒；两项中断修复已回归，新性能计划待实施 |
| [默认 Feature / Task 开发流程精简](default-workflow-simplification/PLAN.md) | [实施](default-workflow-simplification/EXECUTION.md)、[本轮验收](default-workflow-simplification/ACCEPTANCE.md)、[变基合入](default-workflow-simplification/MERGE.md)、[main 续验](default-workflow-simplification/FOLLOW-UP.md)、[性能](default-workflow-simplification/PERFORMANCE.md)、[退役清单](default-workflow-simplification/RETIREMENT.md) | 已合入本地 main；续验单 Task/Feature 完成真实交付，最终 Rust 627、浏览器 128 项覆盖通过。新默认及通知合并已定向验证；本轮原生含返工用时 29 分 18.6 秒，五分钟目标未达成 |
| [工作流性能与可靠性修复](workflow-performance-remediation/PLAN.md) | [接续清单](workflow-performance-remediation/FOLLOW-UP.md)、[合入验证](workflow-performance-remediation/MERGE.md)、[阶段验收](workflow-performance-remediation/ACCEPTANCE.md)、[实施](workflow-performance-remediation/EXECUTION.md)、[第二批](workflow-performance-remediation/CONTINUATION.md)、[性能](workflow-performance-remediation/PERFORMANCE.md)、[存储诊断](workflow-performance-remediation/STORAGE-DIAGNOSTIC.md)、[预算边界](workflow-performance-remediation/BUDGET-DESIGN.md) | 两批局部修复已合入本地 main。S0 完成，S1–S7 剩余修复和验证见接续清单。完整 Feature 和正式容量矩阵暂缓 |
| [评审拒绝后的经理决策、有限返工与人工批准](review-rejection-and-approval/PLAN.md) | [实施与验证](review-rejection-and-approval/EXECUTION.md)、[合入验证](review-rejection-and-approval/MERGE.md) | 已合入本地 main；三种返工与共享额度、人工例外、根经理页子审批及流程定位；Rust/Chromium 回归通过，真实模型及 4318 显式采用未执行 |
| [main 三项改动联合验收与性能改进](integrated-workflow-performance-acceptance/PLAN.md) | [阶段验收](integrated-workflow-performance-acceptance/ACCEPTANCE.md)、[性能基准](integrated-workflow-performance-acceptance/PERFORMANCE.md)、[后续修复计划](integrated-workflow-performance-acceptance/FOLLOW-UP-PLAN.md)、[实施记录](integrated-workflow-performance-acceptance/EXECUTION.md) | 已按用户要求暂停测试并交付阶段结果；最新后台回归/子流程审批通过，完整 Feature 和正式100浏览器容量未通过；未解决问题另列计划，未继续执行 |
| [项目经理通过事件自主推进工作流](event-driven-project-manager/PLAN.md) | [实施与验证](event-driven-project-manager/EXECUTION.md)、[main 合入验收](event-driven-project-manager/MERGE.md) | 已完整合入本地 main；Rust 392、debug/release 各 85、证据回归 33 项通过；真实 Feature 42 步已复核，feature worktree 和分支已清理 |
| main 剩余改动整理 | [内容与定向验证](main-uncommitted-review/EXECUTION.md) | 运行诊断、权限恢复与执行页改动已复核；定向回归通过，未运行完整 workflow/Feature 交付回归 |
| [Workflow 执行性能与运行稳定性](workflow-runtime-performance/PLAN.md) | [分工边界](workflow-runtime-performance/SCOPE.md)、[真实运行验证](workflow-runtime-performance/LIVE.md)、[验收](workflow-runtime-performance/ACCEPTANCE.md) | 独立 worktree 实施中；真实 Git/Docker、Codex 原生衔接与 Host 中断恢复已验证，完整 SLA 未通过；公共存储、调度、执行页与 100 前端容量待联合 |
| [单 Host SQLite 底层并发重构](single-host-concurrency/PLAN.md) | [工作流运行时 100 前端验收合同](single-host-concurrency/ACCEPTANCE.md) | P0/P1 实施中，P2–P6 待实施、待验收；独立前端项目上下文、并发读、所有修改入口的资源级乐观校验、SQLite 短事务、最终一致视图与事件分发 |
| [配置加载优化](configuration-loading-performance/PLAN.md) | [性能分析](configuration-loading-performance/ANALYSIS.md)、[W0–W3 验收](configuration-loading-performance/EXECUTION.md)、[W4–W7 验收](configuration-loading-performance/W4-W7.md) | W0–W7 已完成；轻量索引/正文、版本缓存与取消、后台模型目录、编译与成员索引；debug/release 同条件各场景 30 次对比及回归通过 |
| [配置归属、权限简化、本地安装与讨论展示](configuration-permissions-runtime-discussion/PLAN.md) | [实施与定向验收](configuration-permissions-runtime-discussion/EXECUTION.md)；已实施 | 整合六项需求及两项调整：项目工具接入说明、Profile 直用并删除执行授权、Harness/Sandbox 拆分及运行资源删除、本地安装与登录反馈、Workflow 内嵌调度、讨论原始信息新标签页及增量消息简化 |
| [Agent 完整能力与自由讨论及经验访问](agent-capabilities-discussion-experience/PLAN.md) | [实施验收](agent-capabilities-discussion-experience/EXECUTION.md)、[合并回归](agent-capabilities-discussion-experience/MERGE.md) | 已合并本地 main，合并回归通过；统一 Agent 完整能力装配；Workflow 只负责调度；支持无项目对话及 Agent 在当前对话中读取现有经验，修复工具裁剪引发的权限冲突 |
| [代码规范、Lint 体系与 CI 建设](code-standards-lint-ci/PLAN.md) | [独立评估](code-standards-lint-ci/ASSESSMENT.md)、[补充验收](code-standards-lint-ci/ACCEPTANCE.md)、[原执行报告](code-standards-lint-ci/EXECUTION.md)、[原问题记录](code-standards-lint-ci/ISSUES.md) | main 补充实施完成；Rust 291 项、debug/release 浏览器各 57 项通过；已修正原问题分类、门禁缺口与 CI 依赖判断 |
| [源配置信封扁平化](source-envelope-flattening/PLAN.md) | [评审](source-envelope-flattening/REVIEW.md)、[实施与验收](source-envelope-flattening/EXECUTION.md)、[验证报告](source-envelope-flattening/VALIDATION.md) | 已完成；151 源、Rust 236 项、浏览器 debug/release 各 51 项通过；真实 Codex Feature 42 节点完成，两次精确发布，证据复核与现场封存完成 |
| [合同版本去硬编码](contract-version-decoupling/PLAN.md) | [实施与验收](contract-version-decoupling/EXECUTION.md) | 部分实施（2026-11-12）；W01-W05+W08+W09 完成：结构识别、白名单枚举化、读取机制外置、normalize-plan 删除、check-defaults 脚本；W06 大文件重构等后续工作保留；W07 展示过滤与残留版本门禁已在源信封扁平化中清理 |
| [出厂配置精简与 dsh/minimax/glm 栈补全](configuration-slimming-dsh-stack/PLAN.md) | [实施与验收](configuration-slimming-dsh-stack/EXECUTION.md) | 已完成（2026-11-12，W4 门禁能力化当晚按用户决定回滚）；61 个历史修订删除、dsh 双栈补全（PM 为普通 agent 形态）；出厂 codex 修订链维持 main 原版本；测试全绿 |
| [DSH 工具执行桥](dsh-tool-bridge/PLAN.md) | 提案·挂起（源自 review/plans p14） | 用户决定暂维持仅 codex 调度；SDK 协议扩展或容器内插件旁路方案已论证，等上游发版意愿恢复时重启 |
| [现行系统文档补全](current-documentation/PLAN.md) | [实施与验收](current-documentation/EXECUTION.md) | 已完成；5 篇新文档（工作流与编排/运行时权限/存储/讨论与经验/Web 工作台）、8 处漂移修复与 --normalize-plan v6 化；host 32 项测试通过；版本锚点机制按用户决定撤回 |
| [Standard 不可变规范快照](standard-snapshot/PLAN.md) | [实施与验收](standard-snapshot/EXECUTION.md) | 已完成；独立 worktree 合入 main 后统一验证，真实双 Task / Feature 62 节点闭环、切换历史与独立发布包通过 |
| [Delivery 分类与内容规范](delivery-content-specs/PLAN.md) | [实施与验收](delivery-content-specs/EXECUTION.md) | 已合并 main；21 种 Delivery 全关联 Spec，真实 Diff 接入交付与评审；214 项 Rust、debug/release 各 44 项浏览器及真实 Docker 验收通过 |
| [源配置与执行解析](../design/configuration.md) | [实施验收](source-configuration/EXECUTION.md) | 已合入本地 main；源协议 v2、权限独立、文档规范复用、按需视图；207 项 Rust、debug/release 各 43 项浏览器及独立 macOS 包通过；只支持新数据 |
| [Harness 驱动工作流改造](harness-workflow-orchestration/PLAN.md) | [实施验收](harness-workflow-orchestration/EXECUTION.md)、[main 合并验收](harness-workflow-orchestration/MERGE.md)、[合同](harness-workflow-orchestration/CONTRACT.md) | P0—P8 已完成并合并 main；202 项 Rust、debug/release 各 43 项浏览器、实际 Git/Docker/权限、真实旧库升级及独立 macOS arm64 包通过；追加无人值守 Feature 22/Task 20、两轮 TDD 完整证据与 PR #35/#36 通过，0 人工 approve/旁路/豁免；编排当前支持 Codex |
| [Workflow 通用步骤与业务配置改造](workflow-business-configuration/PLAN.md) | [执行记录](workflow-business-configuration/EXECUTION.md)、[合同](workflow-business-configuration/CONTRACT.md)、[硬编码审查](workflow-business-configuration/AUDIT.md) | 已实施；Ubuntu 当前合同仅 Codex 验收通过：154 项 Rust、debug/release 各 41 项浏览器、独立 Linux 包，真实 Feature 21 步 / Task 18 步及 PR #29/#30 完成；不迁移旧开发数据 |
| [默认模板资源与重置模板定义](configuration-template-reset/PLAN.md) | [实施验收](configuration-template-reset/EXECUTION.md)、[合同](configuration-template-reset/CONTRACT.md) | 已实施；Ubuntu 当前 global 13 / project 44 合同通过完整检查与独立 Linux 包；重置、重启后真实 Codex 按归档项目绑定继续运行，旧任务、产物及凭据保留 |
| [架构与实现基线 v0.4](devdesk-v0.4/PLAN.md) | [基线核对与验证记录](devdesk-v0.4/EXECUTION.md) | 当前源码 e4fbcae；Role/Capability、Workflow v3/任务配置、模型与人类 Approve、引用恢复、接入及全局讨论；macOS 179 项 Rust、双 profile 各 33 项浏览器通过，Ubuntu 历史验收与新增待验边界分别保留 |
| [Ubuntu 六项改造验收](ubuntu-six-plan-acceptance/PLAN.md) | [执行记录](ubuntu-six-plan-acceptance/EXECUTION.md) | 已通过用户最终判据；165 项 Rust、双 profile 各 31 浏览器、Codex 完整闭环、独立 Linux 包；修复四项产品问题，MiniMax 模型能力限制单列 |
| [设置、配置与执行工作台改进](settings-navigation-usability/PLAN.md) | [实施报告](settings-navigation-usability/REPORT.md)、[Ubuntu 补验](ubuntu-six-plan-acceptance/EXECUTION.md) | 已实施；Ubuntu 导航、反馈、连接、AgentID、阶段布局与完整 Codex 工程通过 |
| [Workflow Step 任务指令改造](workflow-step-task-instructions/PLAN.md) | [实施记录](workflow-step-task-instructions/EXECUTION.md) | 已实施；可编辑任务、Role/Skill/Spec 逐轮冻结、权限后继与完整路径隔离；152 项 Rust、双 profile 浏览器、真实 Codex 与独立发布包通过，DSH 为原生 SDK 模拟模型验证 |
| [DeepSeek Harness 多模型服务商接入](dsh-model-providers/PLAN.md) | [实施记录](dsh-model-providers/EXECUTION.md) | 已实现；阶段回归、固定 DSH 模拟协议与独立包真实 Codex 通过；历史 macOS 结果保留；Ubuntu 补验和 MiniMax 实测边界见本轮六项报告 |
| [工具与技能接入](tool-skill-integrations/PLAN.md) | [实施记录](tool-skill-integrations/EXECUTION.md) | 已实施；Ubuntu 本轮补验见六项改造验收报告，历史 macOS 结果保留 |
| [Permission 资源与运行时版本绑定](permission-runtime-bindings/PLAN.md) | [实施记录](permission-runtime-bindings/EXECUTION.md) | 已实施；统一文件/UI、确切目标绑定与后继 Session 生效，macOS 自动化、双供应商原生边界和真实 Codex 改绑通过；Ubuntu 原生边界、真实 Codex 与五个进程退出边界补验见本轮六项报告 |
| [Role、Capability、Workflow 与项目配置重构](role-capability-project-configuration/PLAN.md) | [实施记录](role-capability-project-configuration/EXECUTION.md)、[契约](role-capability-project-configuration/CONTRACT.md) | 五组配置导航、Role/Capability/Agent、显式规范、可选基线、独立子流程及按阶段区分能力的默认配置；macOS 自动化与真实 Codex 工程闭环通过，Ubuntu 补验见本轮六项报告 |
| [内建默认套件与 Role](builtin-defaults-and-roles/PLAN.md) | [执行记录](builtin-defaults-and-roles/EXECUTION.md) | 已实施的历史方案；后续入口与自动绑定重构见 Role、Capability、Workflow 与项目配置计划 |
| [架构与实现基线 v0.3](devdesk-v0.3/PLAN.md) | [当轮执行记录](workflow-and-workbench/EXECUTION.md) | 2026-11-09 历史基线；当前领域、配置与运行合同见 v0.4 |
| [架构设计 v0.2](devdesk-v0.2/PLAN.md) | 实施证据见下面各计划 | 原始领域与架构设计基线 |
| [原型实施](devdesk-prototype/PLAN.md) | [F0–F5 与平台报告索引](devdesk-prototype/README.md) | 原型 Linux/macOS 验收记录 |
| [领域与工程接入基础补强](domain-hardening/PLAN.md) | [验收](domain-hardening/domain-hardening.md) | H1–H4 已完成，保留实际平台边界 |
| [每项目数据库与全局配置库](project-databases/PLAN.md) | [验收](project-databases/project-databases.md) | 已实施；独立计划文件为本次按原需求补录 |
| [真实工程接入](real-engineering-integration/PLAN.md) | [R0–R8 最终验收](real-engineering-integration/R8-acceptance.md)及同目录证据 | macOS Web 真实闭环通过 |
| [Web 平台与系统凭据](web-platform-and-credentials/PLAN.md) | [补充验收](web-platform-and-credentials/web-platform-and-credentials.md)及同目录证据 | macOS/Linux 系统凭据、Ubuntu 双 Harness Web 闭环与发布包通过 |
| [Ubuntu Web 验收](ubuntu-web-acceptance/PLAN.md) | [Ubuntu 本轮报告](ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)及独立运行证据 | U0–U6 通过；真实 PR、恢复、发布包与批准的命名策略均有实机证据 |
| [Permission 领域模型与权限标准化](permission-standardization/PLAN.md) | [首轮执行报告](permission-standardization/EXECUTION.md)、[Ubuntu 补验](permission-standardization/UBUNTU.md) | macOS 及 Ubuntu x86_64 验收通过；Ubuntu 含固定 AppArmor、debug/release 双供应商真实权限与独立发布包 |
| [Workflow 领域与工程工作台调整](workflow-and-workbench/PLAN.md) | [首轮执行记录](workflow-and-workbench/EXECUTION.md)、[原生记录](workflow-and-workbench/NATIVE.md)、[Ubuntu 补验](workflow-and-workbench/UBUNTU.md) | macOS 首轮与 Ubuntu Codex 验收通过；修复评审失败状态投影，Ubuntu DSH 按用户后续要求暂停 |

## 历史证据的路径

历史 Markdown 保留其执行日期的结论，不代表本轮重新验收。按用户要求，2026-11-09 已移除计划目录下所有非 Markdown 中间文件。历史报告中的临时路径、命令参数和 SHA 仅是当时记录；相应原始 JSON、日志和截图不再保留，不可据此声称仍有机器证据可重放。下面路径表只用于理解旧记录归属。

| 原路径 | 当前路径 |
|---|---|
| `docs/plans/<计划名>.md` | `docs/plans/<计划名>/PLAN.md` |
| 其余系统凭据、`credentials-*`、`macos-credentials*`、helper、preflight、runtime-base 与 Web package 文件 | `docs/plans/web-platform-and-credentials/` 中的同名文件 |
| `fixtures/` | `tests/fixtures/` |
| `scripts/doctor.py`、`scripts/credential-helper-fingerprint.py` | `scripts/py/` 中的同名文件；入口见 [脚本索引](../../scripts/README.md) |
| `scripts/package-source.py` | `scripts/py/package-source.py`，当前入口为 `scripts/release/package-source.sh` |
| `scripts/build.sh`、`scripts/dev.sh` | `scripts/dev/` 中的同名文件 |
| `scripts/check.sh`、`scripts/check-live.sh`、`scripts/check-permissions.sh`、`scripts/doctor.sh` | `scripts/check/` 中的同名文件 |
| `scripts/package.sh`、`scripts/package-source.sh` | `scripts/release/` 中的同名文件 |
| `scripts/setup-macos-signing.sh`、`scripts/install-macos-credential-helper.sh`、`scripts/sign-macos.sh` | `scripts/platform/macos/` 中的同名文件 |
| `scripts/install-linux-sandbox-policy.sh` | `scripts/platform/linux/install-linux-sandbox-policy.sh` |
| `sql/NNN_*.sql` | `sql/project/<职责>/` 中的同编号文件，见 [迁移索引](../../sql/README.md) |
| `containers/tests/node-reporter.mjs` | `tests/reporters/node-reporter.mjs`，仍为产品测试执行器的嵌入资产 |
| `tests/http_contract.py`、`tests/runtime_preparation.py` | `tests/entrypoints/` 中的同名入口 |
| `docs/demo.md` | `docs/plans/devdesk-prototype/demo.md`，仅保留为历史原型说明 |
| `containers/security/README.md` | 已删除跳转页，直接阅读 [权限说明](../../permissions/README.md)；策略资产仍在 `permissions/backends/docker/` |

领域、协议、适配器契约、运行说明等长期参考文档统一收录在 [系统设计文档](../design/README.md)；它们不是阶段执行计划或验收报告。
