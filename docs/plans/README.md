# 计划与验收索引

每个计划使用 `docs/plans/<计划名>/PLAN.md`，同目录只保留 Markdown 契约、执行报告和验收说明。日志、JSON、截图、补丁和临时测试文件不保存为计划交付物；源码回归测试仍在 crates、tests、web/tests。

| 计划 | 报告与证据 | 状态与范围 |
|---|---|---|
| [合同版本去硬编码](contract-version-decoupling/PLAN.md) | [实施与验收](contract-version-decoupling/EXECUTION.md) | 部分实施（2026-11-12）；W01-W05+W08+W09 完成：合同注册表、结构识别、白名单枚举化、读取机制外置、normalize-plan 删除、check-defaults 脚本；W06 大文件重构与 W07 展示过滤待后续批次 |
| [出厂配置精简与 dsh/minimax/glm 栈补全](configuration-slimming-dsh-stack/PLAN.md) | [实施与验收](configuration-slimming-dsh-stack/EXECUTION.md) | 已完成（2026-11-12，W4 门禁能力化当晚按用户决定回滚）；61 个历史修订删除、dsh 双栈补全（PM 为普通 agent 形态）；出厂 codex 修订链维持 main 原版本；测试全绿 |
| [DSH 工具执行桥](dsh-tool-bridge/PLAN.md) | 提案·挂起（源自 review/plans p14） | 用户决定暂维持仅 codex 调度；SDK 协议扩展或容器内插件旁路方案已论证，等上游发版意愿恢复时重启 |
| [现行系统文档补全](current-documentation/PLAN.md) | [实施与验收](current-documentation/EXECUTION.md) | 已完成；5 篇新文档（工作流与编排/运行时权限/存储/讨论与经验/Web 工作台）、8 处漂移修复与 --normalize-plan v6 化；host 32 项测试通过；版本锚点机制按用户决定撤回 |
| [Standard 不可变规范快照](standard-snapshot/PLAN.md) | [实施与验收](standard-snapshot/EXECUTION.md) | 已完成；独立 worktree 合入 main 后统一验证，真实双 Task / Feature 62 节点闭环、切换历史与独立发布包通过 |
| [Delivery 分类与内容规范](delivery-content-specs/PLAN.md) | [实施与验收](delivery-content-specs/EXECUTION.md) | 已合并 main；21 种 Delivery 全关联 Spec，真实 Diff 接入交付与评审；214 项 Rust、debug/release 各 44 项浏览器及真实 Docker 验收通过 |
| [源配置与执行解析](../source-configuration.md) | [实施验收](source-configuration/EXECUTION.md) | 已合入本地 main；源协议 v2、权限独立、文档规范复用、按需视图；207 项 Rust、debug/release 各 43 项浏览器及独立 macOS 包通过；只支持新数据 |
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
| `docs/reports/real-integration/<文件>` | `docs/plans/real-engineering-integration/<文件>` |
| `docs/reports/F0–F5.md`、原型评审和早期 `macos-validation/measurements/package-validation` 文件 | `docs/plans/devdesk-prototype/` 中的同名文件 |
| `docs/reports/domain-hardening*` | `docs/plans/domain-hardening/` 中的同名文件 |
| `docs/reports/project-databases.md` | `docs/plans/project-databases/project-databases.md` |
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

`docs/` 下的领域/协议/适配器契约、运行说明等长期参考文档仍保留原位置；它们不是阶段执行计划或验收报告。
