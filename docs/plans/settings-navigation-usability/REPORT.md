# 设置、配置与执行工作台实施报告

日期：2026-11-09。状态：已完成。范围见 [计划](PLAN.md)，不包含 target/data 清理工作。

## 当前交付

已完成设置生命周期预览移除、连接内核、全局就近反馈、按资源类型筛选权限、明确运行时与模型选项、工具删除及 DSH 原生工具、资源创建入口、默认正文规范化、Feature 前序下拉、权限合同修复、全局 AgentID 讨论及阶段行布局。导航合并与最终验证均已完成。

## 关键行为说明

- 前序 Feature 是工程关联，不改变新 Feature 从项目主线建立代码基线的行为。
- macOS Codex 安装用于登录与账号模型目录；项目执行使用经过验证的 Linux Docker worker。安装选择展示原因，不再以模糊的自动准备项替代固定安装。
- Codex 模型来自账号目录；DeepSeek 官方 API key 读取账号 `/models`；Ollama 读取本机目录；其他已支持 DSH 服务商展示固定版本的支持目录并说明来源，不宣称已验证推理。
- 权限合同不再绑定整个适配器构建摘要。仅对没有执行 Session/Attempt 的 Feature 重新验证相同权限和目标，并追加新记录；原始不可变记录和已开始执行的冻结合同保留。
- 全局讨论不创建伪 Project。工作、消息及完成快照按 AgentID 保存，切换身份不会继续向原会话发送消息。
- 默认正文采用追加修订升级；用户修改的正文和历史引用不被覆盖。

## 导航与交互交付

- 左侧“流程与规范”：Workflow、Spec、Standard。
- 左侧“基础组件”：Model、Harness、MCP、Skills、Tools、Harness原生工具，顺序固定。Tools 复用 HTTP API 创建、管理、删除能力。
- “历史记录 → 历史范围 → Tools调用记录”：保留筛选、分页、真实调用详情及回执，并可切换当前项目和 Global 接入。
- 旧工作流、规范、工具接入及 Skill/原生 Tool 链接迁至对应入口，保留资源定位；子 Tab 与历史范围支持重载恢复。
- 执行与交付每行一个阶段，显示状态和进度；节点、编译子流程及 Task 子流程均在所属阶段内展开。

## 验证结果

| 检查 | 结果 |
| --- | --- |
| `npm --prefix web run build` | TypeScript 与 Vite 构建通过 |
| `cargo test --workspace --locked` | 23 个 suite；162 通过，16 项既有环境依赖测试忽略，0 失败 |
| debug / release host 构建与 macOS 签名验证 | 通过 |
| `DEVDESK_E2E_PROFILE=release npx playwright test`（web 目录） | 最终 31 项全部通过，58.3 秒 |
| `python3 tests/entrypoints/http_contract.py target/release/devdesk-host --standalone` | HTTP、静态资源、独立二进制、持久化、退出、项目隔离及远端连接安全均通过 |
| 运行时准备与权限资产检查 | 前序轮次通过；本轮未更改安装配方或权限资产 |
| `git diff --check` | 通过 |

浏览器覆盖：两组新导航与 Tab 顺序、旧链接迁移、Tools 回执实际 Docker 调用、HTTP/MCP 删除、就近反馈、固定版本下拉、Codex/DSH 模型目录选择、全部 125 项前序选项、AgentID 切换与全局讨论、阶段行和内嵌 Task 子流程、折叠及草稿恢复。React 改动依本轮已应用的 react-best-practices 技能检查 hooks、组件边界、请求清理与控件可访问名称。

## 本地数据与权限核验

已启动最新 debug 内核 `http://127.0.0.1:4317`。两个现有项目均迁移到工程库 v39，SQLite 外键检查通过；全局和项目原有不可变配置记录逐条与更新前备份相同。两个项目原有 Feature、Session、Turn、Run、Snapshot、Artifact 数据逐行核对未改变。

现有 Feature 保留，6 份权限合同重新验证记录已追加，并通过真实 `permission.binding.preflight` 解析及编译核验。需求讨论仍停留在原阻塞位置，重试和返工入口均可用；刷新页面后由用户继续测试。未发送真实模型消息或自动推进该工作流。

数据库备份通过 SQLite backup API 创建于本机临时目录 `devdesk-before-update-lsm6i541`，保存 global.sqlite3 和两个项目的更新前 v38 数据库；未复制凭据。原有 macOS Codex 与 Linux Docker Codex 两个安装保留。

## 实际模型目录读取

- Codex 账号目录实际返回 8 个模型，包含 `gpt-6.1-sol`、`gpt-6-astra`、`gpt-6.1-terra`、`gpt-6-sol`、`gpt-6-luna` 及三个 gpt-5.6 变体。
- 智谱国内编程服务商支持目录返回 4 个模型；MiniMax 国内服务商支持目录返回 4 个模型。来源为固定 DSH 版本目录，界面明确说明账号推理权限仍需单独验证。
- DeepSeek key 已配置，但本机凭据助手尚未获准读取。用户需在设置选择 DeepSeek 并点击“授权已保存的钥匙串凭据”，随后刷新模型目录。此次未绕过系统授权、未发送推理请求。
- Ollama 默认服务地址当前不可达，界面给出明确错误。启动本机 Ollama 服务或填写实际服务地址后刷新目录。

## 提交

代码、Schema、数据库迁移、默认正文、回归测试、计划及本报告一并提交到当前 main 分支。提交标题为 `Improve configuration workflows, runtime selection and kernel connections`；确切提交号由本次提交后的 `git log -1` 给出。未推送远端。

## 验证边界

未替用户运行真实模型推理，未替用户重试或返工真实 Feature 的下一步骤。当前用户新建项目与 Feature 保留。数据迁移前已通过 SQLite backup API 备份数据库，不复制或打印凭据。
