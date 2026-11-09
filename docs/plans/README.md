# 计划与验收索引

每个计划使用 `docs/plans/<计划名>/PLAN.md`，同目录只保留 Markdown 契约、执行报告和验收说明。日志、JSON、截图、补丁和临时测试文件不保存为计划交付物；源码回归测试仍在 crates、tests、web/tests。清理数量及范围见 [中间产物清理记录](ARTIFACT-CLEANUP.md)。

| 计划 | 报告与证据 | 状态与范围 |
|---|---|---|
| [DeepSeek Harness 多模型服务商接入](dsh-model-providers/PLAN.md) | [实施记录](dsh-model-providers/EXECUTION.md) | 已实现；阶段回归、固定 DSH 模拟协议与独立包真实 Codex 通过；DSH 真实订阅及 Ubuntu 宿主待验 |
| [工具与技能接入](tool-skill-integrations/PLAN.md) | 接入设计与分阶段执行计划 | 待实施；独立接入页、HTTP API/MCP 可调用工具、Skill 资源、Capability 绑定、权限冻结与真实调用回执；原生工具组 Schema 表单先行修正 |
| [Permission 资源与运行时版本绑定](permission-runtime-bindings/PLAN.md) | [实施记录](permission-runtime-bindings/EXECUTION.md) | 已实施；统一文件/UI、确切目标绑定与后继 Session 生效，macOS 自动化、双供应商原生边界和真实 Codex 改绑通过；Ubuntu、DSH 真实模型与完整崩溃矩阵待补验 |
| [Role、Capability、Workflow 与项目配置重构](role-capability-project-configuration/PLAN.md) | [实施记录](role-capability-project-configuration/EXECUTION.md)、[契约](role-capability-project-configuration/CONTRACT.md) | 五组配置导航、Role/Capability/Agent、显式规范、可选基线、独立子流程及按阶段区分能力的默认配置；macOS 自动化与真实 Codex 工程闭环通过，Ubuntu 本轮未重跑 |
| [内建默认套件与 Role](builtin-defaults-and-roles/PLAN.md) | [执行记录](builtin-defaults-and-roles/EXECUTION.md) | 已实施的历史方案；后续入口与自动绑定重构见 Role、Capability、Workflow 与项目配置计划 |
| [架构与实现基线 v0.3](devdesk-v0.3/PLAN.md) | [本轮执行记录](workflow-and-workbench/EXECUTION.md) | 当前领域、Web 工作台、存储与能力边界 |
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
