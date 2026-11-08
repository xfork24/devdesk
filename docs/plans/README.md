# 计划与验收索引

每个计划使用 `docs/plans/<计划名>/PLAN.md`；对应报告、评审、日志、截图和其他验收证据均保存在同一目录。重复验收可在计划目录下建立独立运行子目录。新增计划沿用此结构，不再建立独立的 `docs/reports/`。

| 计划 | 报告与证据 | 状态与范围 |
|---|---|---|
| [架构设计 v0.2](devdesk-v0.2/PLAN.md) | 实施证据见下面各计划 | 原始领域与架构设计基线 |
| [原型实施](devdesk-prototype/PLAN.md) | [F0–F5 与平台报告索引](devdesk-prototype/README.md) | 原型 Linux/macOS 验收记录 |
| [领域与工程接入基础补强](domain-hardening/PLAN.md) | [验收](domain-hardening/domain-hardening.md)、[日志](domain-hardening/domain-hardening-validation.txt) | H1–H4 已完成，保留实际平台边界 |
| [每项目数据库与全局配置库](project-databases/PLAN.md) | [验收](project-databases/project-databases.md) | 已实施；独立计划文件为本次按原需求补录 |
| [真实工程接入](real-engineering-integration/PLAN.md) | [R0–R8 最终验收](real-engineering-integration/R8-acceptance.md)及同目录证据 | macOS Web 真实闭环通过 |
| [Web 平台与系统凭据](web-platform-and-credentials/PLAN.md) | [补充验收](web-platform-and-credentials/web-platform-and-credentials.md)及同目录证据 | macOS/Linux 系统凭据、Ubuntu 双 Harness Web 闭环与发布包通过 |
| [Ubuntu Web 验收](ubuntu-web-acceptance/PLAN.md) | [Ubuntu 本轮报告](ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)及独立运行证据 | U0–U6 通过；真实 PR、恢复、发布包与批准的命名策略均有实机证据 |
| [Permission 领域模型与权限标准化](permission-standardization/PLAN.md) | [执行报告](permission-standardization/EXECUTION.md)及同目录证据 | macOS 开发、真实闭环、权限覆盖/恢复及发布包验收通过；Ubuntu 按用户要求顺延 |
| [Workflow 领域与工程工作台调整](workflow-and-workbench/PLAN.md) | 实施后记录在同目录 | 待实施；Scope 与配置版本、Project 动态绑定及 Feature 冻结、工作流驱动、Agent 经验和前端工作台 |

## 历史证据的路径

本次仅整理归属和位置，不重新执行历史验收。Markdown 导航、说明中的路径及验收工具默认输出路径已更新；原始 JSON、日志、截图和测量文件保持原字节，以保留命令历史及日志 SHA256。历史 JSON 内的绝对路径/命令参数是当时执行记录，按下表定位迁移后的文件。

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
