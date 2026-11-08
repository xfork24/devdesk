# 阶段报告索引

日期：2026-11-08。自检由实现者完成；独立评审分别由 review_f0、review_storage_host、review_web 执行，未将自检冒充评审。

| 阶段 | 设计/执行/验证报告 | 独立评审 | 状态 |
|---|---|---|---|
| F0 | [F0](F0.md) | review_f0：领域/协议/架构 | 通过 |
| F1 | [F1](F1.md) | review_storage_host；[macOS 补充评审](macos-independent-review.md) | Linux / macOS 通过 |
| F2 | [F2](F2.md) | review_f0：规划与版本交付 | 通过 |
| F3 | [F3](F3.md) | review_f0：调度与完整闭环 | Linux / macOS 验证通过 |
| F4 | [F4](F4.md) | review_f0、review_storage_host：异常/恢复 | 通过 |
| F5 | [F5](F5.md) | review_web；[macOS 补充评审](macos-independent-review.md) | Linux / macOS 通过 |

保留 [初次 F0 退回记录](F0-review-initial.md) 与 [原 Linux 独立评审记录](independent-review.md)，不回写历史评语。本轮补齐 macOS 实机待办，详见 [macOS 设计/执行/验证报告](macos-validation.md)、[独立评审](macos-independent-review.md) 与 [测量原始数据](macos-measurements.json)。

- [领域与工程接入基础补强](domain-hardening.md)：本轮模型、迁移、回归和真实适配器边界。
- [每项目数据库与全局配置库](project-databases.md)：活动数据库切换、全局配置、旧库拆分及隔离验收。
