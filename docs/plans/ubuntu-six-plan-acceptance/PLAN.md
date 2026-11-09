# Ubuntu 六项改造验收与修复

日期：2026-11-10。状态：已执行并通过用户最终验收判据。起点：`0b13778da7b519c9561745a474552570a3731d56`，初始工作区干净。

## 目标与依据

重新验收 settings-navigation-usability、workflow-step-task-instructions、tool-skill-integrations、dsh-model-providers、permission-runtime-bindings、role-capability-project-configuration 六项目录的 PLAN、契约、补充与实施记录。用户输入的 ole-capability-project-configuration 对应最后一项。最终导航以 settings 补充为准；新项目预填默认资源但显式保存，实际新工作流使用 v3。不能沿用历史 macOS/Ubuntu 通过结论。

本计划和 EXECUTION.md 保存本地；计划目录只放 Markdown。日志、JSON、截图及临时脚本写入独立 /tmp 目录，工程数据放 data/ubuntu-six-plan-<UTC时间>。修复保留在源码及正式回归目录；更新计划索引和 Ubuntu 验收入口。

## 环境和授权

- Ubuntu 24.04.5 / x86_64，Rust 1.99.1、Node 24；Docker、Chromium、Secret Service 已发现。AppArmor 专用策略与仓库字节一致，实测边界而不放宽权限。
- 独立动态端口和数据集；保留现有 51751 Host、项目、原始凭据。只停止或清理具有本轮归属证据的进程、容器和 worktree。
- 用户选择真实 Codex 完整验收，专用 GitHub 仓库 xfork24/devdesk-integration-test，允许分支、PR、确认与合入；当前账号权限 ADMIN。
- Codex 登录仅复制 data/ubuntu-permissions-workflow-20261109/credentials/codex/auth.json，权限 0600，结束删除副本。
- **用户执行阶段追加**：DSH 尝试 MiniMax 国内账户和 MiniMax-M3.1-Flash。密钥仅经独立数据集的 Secret Service 保存，不进入源码、报告、日志、argv。配置失败或额度不足则跳过 DSH 真实验收并继续其余检查。固定 SDK 本地模拟回归仍执行。
- 本机镜像使用完整不可变 ID，核对架构、版本和当前配方；不复制 macOS 安装身份，不把旧镜像当当前构建。

2026-11-10 用户最终补充：MiniMax 能力可能无法完成任务，Codex 完整闭环跑通即可认定验证通过。执行结果遵循此判据，MiniMax 原始失败和未完成范围单独保留。

## 阶段与通过条件

### U0 保存计划并修正验收入口

将完整 Workflow 探针升级为 task_instructions/v3 子流程，系统调用节点清除不适用 Agent 字段。平台报告由实际 OS/架构生成。核对 DSH 工具、模型、权限引用和 schema 假设。Linux 凭据探针使用独立合成条目覆盖模型服务商及工具凭据，Ollama 免密；清理本轮条目。探针适配和产品缺陷分开记载，不删断言取通过。

### U1 完整基线

执行 cargo fmt --all -- --check、scripts/check/check.sh、git diff --check。覆盖 Web/Rust/Schema/权限资产、debug/release HTTP、独立二进制、runtime preparation、双 profile Chromium；另跑 release runtime preparation。记录 ignored 明细，相关外部项在 U3 显式执行。

### U2 六项矩阵

| 计划 | 必验行为 |
|---|---|
| settings-navigation-usability | 最终导航/旧链接、就近反馈、安装/版本/模型选择、工具引用保护删除、全部前序 Feature、AgentID/全局隔离、阶段行及刷新恢复、内核连接/认证/退出重连 |
| workflow-step-task-instructions | Markdown 往返、公共/节点任务、系统节点拒绝任务、预览与派发快照、完整路径/Task 身份隔离、队列/技术重试冻结、权限后继保留任务/Spec |
| tool-skill-integrations | HTTP/OpenAPI、两版 MCP × 两传输、选中工具授权、Skill 依赖/只读材料、账户/项目隔离、配置锁、审批/取消/未知副作用核对、调用历史及脱敏 |
| dsh-model-providers | 组合校验、凭据隔离、三路由模拟流式/工具/恢复、MiniMax thinking、Linux host-gateway、Host/容器分别检查、禁网冲突、连接冻结；追加 MiniMax 真实验证按用户条件执行 |
| permission-runtime-bindings | profile 导入导出/原子冲突/热加载、确切绑定/CAS/follow/pinned、Scope、原生文件网络、审批、后继/去重、旧 Feature/Run/重试冻结 |
| role-capability-project-configuration | 独立 Role/Capability/Agent、Scope、六职责默认配置、显式项目保存、可选 Standard/覆盖拒绝、逐 Step Spec、子流程输入输出/失败恢复、旧迁移幂等和冻结保留 |

复用正式 Rust/HTTP/Playwright，并为缺陷补回归。浏览器真实 Host 流程与工程数据检查桌面/窄屏、控制台、刷新持久化。迁移用历史夹具及独立副本验证全局 v8/项目 v39、外键、幂等和不可变内容，不打开旧运行数据进行迁移。

### U3 Linux 与真实 Docker

显式执行 permissions/docker_resources/command_tests 的忽略项、permission_profiles v2 原生边界、tool_integrations 四项 Docker、model_providers 两项模拟测试、agent_tasks 原生冻结任务。固定镜像环境变量使用完整 sha256 ID。

运行 nested sandbox、Linux Secret Service、OAuth start/cancel 探针；核对 AppArmor、非 root/只读根/挂载/清理。Ollama 使用本地模拟目录验证 Linux 容器可达和 loopback 失败，不下载模型。不锁用户 collection，以隔离 D-Bus 不可用验证错误路径并明确实测边界。

### U4 真实模型与恢复

顺序执行 release Host 的 workflow_tasks、permission_runtime、tool_integrations、model_providers 真实 Codex 探针，核对实际文件、HTTP 收据、外部工具、只读 Skill、后继会话和原生重启恢复。

核对权限切换五个边界：持久化意图、旧 writer 停止、后继提交、输入接受、派发回执丢失。已有回归不足时补可恢复测试；重启不双 writer、不重复消息、不重发未知副作用。只操作专用进程。

完整 v3 Feature 使用随机 greeting ESM 文件：需求/设计/Task Plan/Test Plan、PRD 子流程、独立 assertion Red、实现 Green、文档、独立 Review、精确候选确认、Task/Feature 合入与清理。独立 GitHub API 和 ls-remote 核对 PR/候选 SHA。先完成 Codex，再对配置成功且额度可用的 MiniMax 执行 DSH；失败按用户条件记录并继续，不掩盖非账户产品缺陷。

### U5 发布与收尾

生成 Linux x86_64 包，源码外运行，验证包/Host SHA256、Web/Schema/默认 v3 任务、权限资产、profile 热加载/绑定、编辑发布/预览及重启。包内 Host 与最终真实验收 release Host 一致。核对数据库、Artifact 和冻结身份；删除登录副本和本轮临时凭据，清理专属资源。

## 修复和最终判定

失败保留阶段、命令、退出码、复现及原证据；补有效回归，最小修复，重跑受影响及下游检查。最终源码再执行完整检查并生成发布包。不改已发布 SQL、不改历史冻结记录、不削弱门禁。所需环境缺失则继续独立项，待验不计通过。

EXECUTION.md 逐项写明通过/失败/阻塞/未覆盖、ignored 明细、修复、版本/镜像/包摘要、真实工程对象、清理和边界。六项能力需有 Ubuntu 当轮证据；MiniMax 账户失败按用户指定记为跳过，第三方真实 OAuth 账户及用户 collection 锁定不在范围。
