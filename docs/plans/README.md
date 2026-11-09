# 计划与验收索引

每个计划使用 `docs/plans/<计划名>/PLAN.md`，同目录只保留 Markdown 契约、执行报告和验收说明。日志、JSON、截图、补丁和临时测试文件不保存为计划交付物；源码回归测试仍在 crates、tests、web/tests。清理数量及范围见 [中间产物清理记录](ARTIFACT-CLEANUP.md)。

| 计划 | 报告与证据 | 状态与范围 |
|---|---|---|
| [架构与实现基线 v0.3](devdesk-v0.3/PLAN.md) | [本轮执行记录](workflow-and-workbench/EXECUTION.md) | 当前领域、Web 工作台、存储与能力边界 |
| [架构设计 v0.2](devdesk-v0.2/PLAN.md) | 实施证据见下面各计划 | 原始领域与架构设计基线 |
| [原型实施](devdesk-prototype/PLAN.md) | [F0–F5 与平台报告索引](devdesk-prototype/README.md) | 原型 Linux/macOS 验收记录 |
| [领域与工程接入基础补强](domain-hardening/PLAN.md) | [验收](domain-hardening/domain-hardening.md) | H1–H4 已完成，保留实际平台边界 |
| [每项目数据库与全局配置库](project-databases/PLAN.md) | [验收](project-databases/project-databases.md) | 已实施；独立计划文件为本次按原需求补录 |
| [真实工程接入](real-engineering-integration/PLAN.md) | [R0–R8 最终验收](real-engineering-integration/R8-acceptance.md)及同目录证据 | macOS Web 真实闭环通过 |
| [Web 平台与系统凭据](web-platform-and-credentials/PLAN.md) | [补充验收](web-platform-and-credentials/web-platform-and-credentials.md)及同目录证据 | macOS/Linux 系统凭据、Ubuntu 双 Harness Web 闭环与发布包通过 |
| [Ubuntu Web 验收](ubuntu-web-acceptance/PLAN.md) | [Ubuntu 本轮报告](ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)及独立运行证据 | U0–U6 通过；真实 PR、恢复、发布包与批准的命名策略均有实机证据 |
| [Permission 领域模型与权限标准化](permission-standardization/PLAN.md) | [执行报告](permission-standardization/EXECUTION.md)及同目录证据 | macOS 开发、真实闭环、权限覆盖/恢复及发布包验收通过；Ubuntu 按用户要求顺延 |
| [Workflow 领域与工程工作台调整](workflow-and-workbench/PLAN.md) | [执行记录](workflow-and-workbench/EXECUTION.md)、[原生记录](workflow-and-workbench/NATIVE.md) | 本轮实现及验收完成，平台和能力边界见记录；Scope 与配置冻结、Workflow 编辑存储及执行、Agent/Model/Harness/Skill/Tool/Permission 组合、实例经验和工作台 |

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
| `scripts/doctor.py`、`scripts/credential-helper-fingerprint.py` | `scripts/py/` 中的同名文件，原 shell 入口继续使用 |
| `scripts/package-source.py` | `scripts/py/package-source.py`，当前入口为 `scripts/package-source.sh` |

`docs/` 下的领域/协议/适配器契约、运行说明等长期参考文档仍保留原位置；它们不是阶段执行计划或验收报告。
