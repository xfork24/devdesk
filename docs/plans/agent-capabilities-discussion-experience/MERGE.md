# Agent 能力与经验访问合入 main

日期：2026-11-15。目标：`/Users/hengliu/workspace/devdesk` 的本地 `main`。状态：冲突与衔接问题已处理，本轮回归通过，合入本地 main。

## 合并范围

- main 合并前：`1c5d3bc4ebf95fdb70452fcf60f4aa324debb7c6`。
- 功能分支：`codex/agent-capabilities-discussion-experience`，提交 `aa70f9b`。
- 合并保留 main 的项目规范覆盖、显式默认工作流、权限配置入口和表单布局，同时接入完整 Agent 能力、无项目讨论及经验访问。
- 合并、修复和回归均在上述本地 main 工作区完成。验证使用临时数据集，未打开用户运行数据。

## 冲突与衔接

| 位置 | 处理 |
|---|---|
| 计划正文与索引 | 保留已实施状态和原验收记录；本轮合并结果单独记录。 |
| 出厂 defaults | Capability 采用新修订 `:4`，Standard 保留 main 的 `:6`；采用功能分支的最新 Agent 修订。 |
| 出厂 manifest | 同一 Standard 仅发布 v6，移除出厂目录中的旧 v5 文件；既有数据库历史源不变。 |
| 角色讨论 | 保留 main 的下拉列表新建实例、简化名称及缩短实例编号；保留项目来源名称、完整能力和经验引用。 |
| 样式 | 合并权限详情、输入对齐和 Agent 完整能力网格规则。 |
| 回归夹具 | main 新增经理角色边界用例采用现行分析师修订；原生模型探针从出厂默认选择解析 Agent；Rust、Host 和浏览器的 Standard 比较夹具显式发布不同入口的连续修订，避免依赖已移除的旧出厂快照。 |

自动合并的核心、Schema 和配置图另外核对：Capability 自身规范保持根级确切引用；项目/工作标准、Controller 角色边界与实际工具调用授权继续有效。

## 本轮验证

按 `scripts/check/check.sh` 的检查范围分段完成全部验证。原始日志保存在 `/private/tmp/devdesk-agent-merge-evidence/`，不提交生成文件。

| 检查 | 结果 |
|---|---|
| 权限资产 / Permission Schema / 出厂配置 / 架构 / CI 门禁 | 通过 |
| rustfmt、Clippy `-D warnings`、ESLint、TypeScript、Prettier、Web build | 通过；夹具修复后重新执行相应门禁 |
| Rust 工作区全部默认目标 | 311 passed，18 ignored；已通过目标与修复后剩余目标分别保存结果，无遗漏 |
| DSH 官方 SDK 工具桥 | 显式固定镜像单独运行 ignored test：1 passed；回环模拟推理，无外网推理 |
| debug / release 构建与 macOS 签名 | 通过 |
| debug / release HTTP、SSE、认证、退出与进程锁 | 通过；release 使用独立复制的二进制 |
| debug / release 模板重置、数据集兼容、运行准备 | 通过 |
| debug Chromium | 67 passed |
| release Chromium | 67 passed |
| 合并界面核对 | 无项目 Agent 能力、实例选择、权限新入口、项目规范覆盖与比较通过；能力面板截图已检查 |

Rust 分段日志对应：`check-main.log` 中中断前成功目标 193 项，`project-standards.log` 4 项，`check-main-continued.log` 其余成功目标 81 项，`check-main-final.log` 修复后的 Host 全部 33 项，共 311 项。失败运行不计入成功数量，未放宽原断言。

独立 SDK 结果见 `dsh-sdk.log`；最终 HTTP、数据集、构建和两轮浏览器结果见 `check-main-final.log`。

首次全量 Rust 在 Standard 比较夹具处失败；修复后该组 4 项通过。后续 Host 的同类夹具也显式建立不同元数据的修订，两项跨项目比较通过；浏览器对应夹具增加入口元数据的明确断言。已通过的 Rust 目标保留结果，继续执行剩余目标和后续完整检查，原失败日志保留。

工作区的未跟踪 `data2/` 原样保留，不纳入提交。

本轮不重复付费模型推理；分支此前的 Codex/MiniMax 真实调用结果见 [实施验收](EXECUTION.md)，不将其标记为合并后重跑。

验证二进制 SHA-256：

- debug: `0930428619952b91afd35669af5b7b99e8242527b5017a17fb77eda57e3f25ac`
- release: `257f4d30f05930758498ab565a71acb77a8e015d685b0c336ce96e5473cccfb6`
