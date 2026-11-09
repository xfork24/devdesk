# 分阶段实施记录

日期：2026-11-09。状态：实现与分阶段验收完成。

开发始终位于独立 worktree `/Users/hengliu/workspace/devdesk-tool-skill-integrations`，分支 `codex/tool-skill-integrations`。最初使用临时依赖快照保留主干未提交的 Permission 接口；主干完成 Permission 与模型服务商工作后，已 rebase 到 `4cca5d8`，移除临时快照提交，保留主干的新权限、凭据助手兼容及 DSH 多模型实现。没有修改或提交主干工作区中的并行改动。

## 阶段结果

| 阶段 | 结果 | 验证 |
|---|---|---|
| P0 契约与协议 | 通过 | 固定 Docker Codex 0.167.0 实际接受实验 dynamicTools；明确 Tool/Skill/Integration/Binding/调用合同；发布 Schema 离线解析与 Serde 回归通过 |
| P1 资源与冻结 | 通过 | 不可变修订、Global/Project、确切动作授权、项目映射、Capability 依赖、Feature/Session 锁、引用检查与普通/紧急停用；额外 Host 回归验证跨项目拒绝和全局写入归属 |
| P2 HTTP 与执行 | 通过 | 真实 Docker HTTP 读/写、参数和结果校验、私网拒绝、重复派发及执行器重启不重发、写请求超时副作用未知、文件完整性读取及敏感参数脱敏；core 验证审批、取消、恢复和人工核对 |
| P3 MCP | 通过 | 本地真实服务：2025-11-25/2026-07-28 × stdio/Streamable HTTP；初始化/逐请求协议、分页、调用、未选修订拒绝、描述漂移拒绝。浏览器另完成真实 stdio 发现、选择、发布、授权和 Docker 测试调用 |
| P4 Skill | 通过 | Markdown/ZIP/固定 Git commit 导入、路径/链接/脚本检查、原文与依赖冻结、材料维护和导出；真实 Codex 读取独立参考 marker，Shell 对 `/skills` 的写入实际返回 `Read-only file system` |
| P5 独立页面 | 通过 | HTTP API、MCP、Skills、原生工具、调用记录五个内页；基础组件仅 Model/Harness；旧链接、Markdown、修订/重载、项目选择和调用回执均有浏览器覆盖。已检查 HTTP 与 MCP 回执页面截图 |
| P6 认证 | 通过已声明范围 | Bearer/Header API key 系统引用及轮换；OAuth 公共客户端 PKCE、一次性交换、刷新、scope/账户约束、撤销与中断保护。协议测试使用真实 Docker token 服务及内存凭据替身；系统助手输入/账户格式编译回归通过，未替换共享助手或连接真实第三方 OAuth 账户 |
| P7 综合验收 | 通过 | Rust 143 项；debug/release 各 18 项浏览器；双 profile HTTP（含独立复制二进制）、按需运行准备和权限资产检查；release Host 的两轮真实 Codex HTTP/MCP/Skill/重启闭环通过，DSH 推理调用为 0 |

默认 Rust 测试忽略 15 项依赖环境的用例。本计划的 4 项 Docker 测试已另行显式执行并全部通过，其余原有真实供应商/专用资源测试没有冒充已完成。Python 可选 jsonschema 未安装；发布 Schema 的编译、引用和实例校验由 Rust jsonschema 回归覆盖。

## 真实模型验收

使用独立 release Host 与新临时数据集，完整创建来源、动作、Skill 包、接入绑定、Capability、Agent、项目 Role→Agent 和来源→绑定映射，再执行真实对话。

- Model：`gpt-6.1-sol`，`reasoning_effort=high`。
- Harness：Codex `0.167.0+docker.5f8731ec42de`。
- Docker：`sha256:5f8731ec42de5c5d542d5398147c3348c200ad7a66c6cfc99a5f52e0b9776ffc`。
- 适配器：`codex-app-server/v1+build.8707d453c953427a`。
- Project：`07166956-7cc7-49c2-bca4-81fa406f6332`。
- Session：`b206c9ee-3052-4349-9557-71f8b4ae9dab`。
- 第一 Turn：`99a709ce-3b2a-463b-9c93-74f45ac8dc7a`。
- 重启后 Turn：`7577a53c-1044-4db9-93e5-25d6e1eea733`。

第一轮调用 HTTP canary 与 MCP sum，读取 Skill 静态参考文件并正确返回两个不可猜测 marker。HTTP 服务只收到一次请求。Host 重启后保持同一冻结目录与原生上下文，再调用 MCP 得到 42，并恢复先前 HTTP marker；累计 3 条工具调用均有成功的真实外部回执。写 Skill 的 Shell 命令退出码为 1，原生输出证明只读挂载实际生效。

扫描非凭据运行文件未发现本次真实 OAuth token。验收探针结束时删除了测试中的 auth.json 副本；原始登录文件与共享 Keychain 助手保持原状。没有读取或调用真实 DSH 凭据和推理服务。

可复验入口：`tests/live/probe_tool_integrations.py`，必须显式提供专用 Codex 登录文件。具体命令及接入操作见 [工具与技能接入](../../tool-skill-integrations.md)。

## 本机证据位置

日志与运行数据留在临时目录，不提交到计划文件夹：

- `/tmp/devdesk-tools-rust-final.log`：143 项 Rust 通过。
- `/tmp/devdesk-tools-final-docker.log`：4 项真实 Docker HTTP/MCP/OAuth 协议测试通过。
- `/tmp/devdesk-tools-debug-browser-final.log`、`/tmp/devdesk-tools-release-browser-final.log`：各 18 项通过。
- `/tmp/devdesk-tools-debug-http-final.log`、`/tmp/devdesk-tools-release-http-final.log`、`/tmp/devdesk-tools-runtime-final.log`。
- `/tmp/devdesk-tools-release-native.json` 与 `/tmp/devdesk-tools-release-native.log`。
- 真实数据集：`/private/tmp/devdesk-tool-integrations-native-zry26xiz`；登录副本已清理。
- HTTP 与 MCP 页面截图在隔离 worktree 被忽略的 `web/test-results/`。

临时文件可能由系统清理；上述路径记录本次验收位置，不作为运行依赖。

## 交付边界

实现覆盖计划声明的首版子集。静态 Skill 材料目前要求 UTF-8；脚本、二进制包安装、任意转换脚本、远程 Schema 引用、复杂 OpenAPI 序列化和 MCP 扩展明确拒绝。OAuth 交付公共客户端授权码 + PKCE，未连接真实第三方账号，也未在本机进行新的 Keychain 交互授权或 Ubuntu Secret Service 实机验收。

macOS 使用新接入凭据时需显式升级签名助手；正常构建复用已授权兼容助手，已有 Codex/DSH 账号不需要重新写入。本次没有自动替换用户共享助手。
