# Workflow Step 任务指令实施记录

日期：2026-11-09。状态：实现与本机验收完成，合入本地 main。

开发位于独立管理 worktree `/Users/hengliu/.codex/worktrees/workflow-step-task-instructions/devdesk`，分支 `codex/workflow-step-task-instructions`。起点为 `4cca5d8`，后续整合已进入 main 的工具与 Skill 接入提交至 `f530e83`，保留其 HTTP/MCP、静态 Skill、凭据和权限合同。任务迁移编号从临时 037 调整为正式 038，不覆盖工具接入的 037；全局库 v8、项目库 v38。原 main 工作区的计划及索引改动已移入本 worktree，未提交其他 worktree 的未提交修改。

## 实施结果

| 阶段 | 结果 |
|---|---|
| P0 声明与执行者 | Workflow/编译器 v3、Agent 动作合同 v2；共享动作目录明确 Agent、配方和系统执行者。Agent Step 与六类 Agent 节点任务必填，系统测试和 merge.prepare 不接收 Agent 任务 |
| P1 编译与冻结 | 任务原文、渲染器、阶段、精确修订及完整路径进入 Feature 闭包和差异；旧 v1/v2 与旧锁不补写字段，升级产生显式草稿和诊断 |
| P2 组装与派发 | 共享预览/执行任务段；逐轮持久化 execution_task 与 Artifact，统一编码给 Codex/DSH。冻结有效 Agent、声明顺序 Skill、Spec、Context、材料和授权；历史 Snapshot 有来源及截取标记。摘要与来源关系、正文、输出合同、Spec 和授权均校验 |
| P3 默认资源 | 22 份 Markdown 任务编译嵌入新的 Feature/Task Workflow v3 修订；讨论、验证、测试编写、实现、文档和两类评审都有独立正文。旧字符串只服务旧编译锁；迁移清单见 resources/configuration/README.md |
| P4 UI 与查询 | Step 公共任务与 Agent 节点任务 Markdown 编辑、保存、发布、只读预览、实际 Turn 任务查询；Role/Agent、Skill、Spec、阶段及输出合同分开展示。缺绑定显示具体诊断，不准备环境或调用模型 |
| P5 验证 | 真实 SQLite/Artifact、RPC Schema、浏览器、HTTP、DSH 原生 SDK、真实 Codex 与独立包检查；具体覆盖和边界如下 |

单字段最多 32 KiB UTF-8，Feature/Task 展开任务合计最多 1 MiB，冻结任务与基础上下文合计最多 512 KiB。任务与 Role/Skill/Spec 不静默截断。任务只在明确采用的新配置中生效；发布修订不会在线替换已有 Feature 的任务。

v3 步骤默认独立 Session；接受输入时冻结当前 Context。排队输入、续轮和技术重试不读取最新 Workflow；权限后继保留任务/Spec 并更新活动步骤的 session 输出。Source 以完整路径映射回编译 key，避免 Feature 中名为 task 的子流程与真实 Task 计划出现相同编译 key 时串用任务。

## 自动化验证

- Rust：最终全量 152 项通过、16 项默认忽略；任务专项含 9 项默认测试和 1 项显式环境测试，Schema 11 项。覆盖讨论、三种验证、Task/Feature 测试编写、实现、文档、交付和候选评审；正式 Run 入队与技术重试；逐轮 Context 冻结；权限后继、重放、篡改、超限、两个调用点及两个 Task 的身份隔离。
- 新增的 Task 回归刻意让 Feature 调用与 Task 计划产生相同编译 key，核对两类来源、两个 Task、十次记录端口派发；没有以该夹具声称实际 Docker 写入、Git 或模型推理。
- Chromium：debug 与 release 各 20 项通过，包含原工具/Skill 接入页面、任务原文往返、公共/节点任务、发布、实际 HTTP 和 SQLite 持久化。
- debug/release HTTP 独立复制二进制检查通过：鉴权、Host/Origin、资源、幂等、SSE、退出、重启和项目隔离；按需运行准备、嵌入配方检查通过。
- DSH 0.2.2-rc.1 原生 SDK：ZAI/Minimax/Ollama 三路由，各三次请求，完整任务/Spec 与 Role/Skill persona 进入原生模型请求；真实 bash 工具结果、原生恢复和 Minimax thinking 回放通过。容器 `--network=none`，模型响应来自本地夹具，真实 DSH 推理为 0。
- 已按 React 检查表复核状态更新、请求触发、过期预览、轮次切换与可访问标签；Web TypeScript/构建和 diff 空白检查通过。

默认 Rust 测试保留依赖专用环境的忽略项；本任务的 DSH 原生环境测试另行显式执行。原 v2 回归夹具明确采用 v2，新任务专项明确采用 v3，未用旧合同测试冒充新派发验证。

## 真实 Codex 验证

最终 release Host、macOS arm64、Codex 0.167.0、`gpt-6.1-sol`、reasoning_effort=low。探针在临时私有数据集中创建专用 Role、Skill、Spec、Capability、Agent、两份 Workflow 修订与两个 Feature，仅复制明确给定的 Codex 登录文件，不打开原项目数据库；结束删除整个测试数据集和登录副本。

两份任务分别要求原生 shell 写入不同的 scratch 文件，并请求受控 HTTP 服务。两轮均取得 exitCode=0 的实际工具输出和对应 HTTP 回执；Role 有效正文只出现一次，Skill/Spec 原文和任务来源均由持久化快照核对。第二版发布后第一轮快照保持不变；原 conversation.send 命令重放返回相同结果，没有重复业务输入。证据不依赖模型自述。

| 任务 | Turn | 定义摘要 |
|---|---|---|
| A | `5f5a8300-38a3-4eb3-8fa3-484f1e1e0376` | `a31f889a5aee5a37bf2f9a89277d467c30752eb8e4aa4f9e7eeea0a885a84f3a` |
| B | `ed785739-a6a7-4c5c-9cdc-bc3255f4c9bc` | `c5bb8fcb275a7ebd4ff994b7bfe0ada2d8c74cb69dfd592a7cb67ef14929bc96` |

这两轮专门验证新任务合同。Host 重启后的排队任务由 Rust 专项验证；本轮没有把真实模型探针描述为完整 GitHub/PR/TDD 工程验收或全部崩溃点矩阵。

## 独立发布包

`devdesk-0.2.0-aarch64-apple-darwin.tar.gz` 已构建、签名并解包验证。Host 从独立临时目录启动，不依赖源码，能够加载嵌入默认 v3 任务、原文编辑发布、保持原修订、解析 Role/Skill/Spec 预览和建立项目 schema v38；未自动下载运行时或调用模型。最终真实 Codex 探针使用与包内相同的 release Host。

- 包 SHA256：`77fc1ae50073dcf16cfe303588f71ba9ff94b3d3fe0dde373dbf90e98d4feab4`。
- Host SHA256：`4a9a1eb45052019a3a783fb0700274a4b4b674cb5e50b99bc26be3ac515ab028`。

## 本机证据和复验

以下均为可清理的本机临时文件，不是产品运行依赖，也未把 JSON、日志或凭据提交到计划目录：

- `/tmp/devdesk-task-delivery-rust.log`：最终全量 Rust。
- `/tmp/devdesk-task-verified-debug-browser.log`、`/tmp/devdesk-task-verified-release-browser.log`：各 20 项浏览器。
- `/tmp/devdesk-task-verified-debug-http.log`、`/tmp/devdesk-task-verified-release-http.log`、`/tmp/devdesk-task-verified-runtime.log`。
- `/tmp/devdesk-task-delivery-dsh.log`：固定原生 SDK 与模拟模型响应。
- `/tmp/devdesk-workflow-task-delivery-real.json`、`/tmp/devdesk-workflow-task-delivery-real.log`：两轮真实 Codex。
- `/tmp/devdesk-task-delivery-package.json`、`/tmp/devdesk-task-delivery-package-check.log`、`/tmp/devdesk-task-delivery-package.log`。

复验命令见 [测试说明](../../../tests/README.md)；独立包位于本 worktree 被忽略的 dist 目录。

本轮未在 Ubuntu 实机重跑，未调用真实 DSH 模型或真实第三方 OAuth 账户。并行工具/Skill 接入的完整验收仍见其独立实施记录。任务正文不能扩大 Permission、Capability 或工程写范围，不能代替真实测试、独立 Review、显式采纳和 Git 门禁。
