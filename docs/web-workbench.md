# Web 工作台指南

Web 是构建后嵌入 Host 的 React 单页应用：默认打开 `http://127.0.0.1:4317`，与内核同源（会话 Cookie 鉴权），全部操作经 `/rpc` JSON-RPC 与 `/events` SSE 完成。本文按导航区说明操作入口；运行语义见 [工作流与编排](workflow-orchestration.md)，部署与远程连接见 [运行包说明](run-binary.md)。

## 初始化与连接内核

首次打开时先探测 `/capabilities`：需要初始化则显示设置页选择数据集目录；受密钥保护的远程内核显示密钥登录。设置页的"连接内核"可切换到其他 `http(s)://ip:port` 内核：先无凭据探测 `/connection` 确认产品与协议，再整页跳转目标 origin；连接密钥不进入 URL 或浏览器持久存储。

## 九大导航区

| 导航 | 内容与操作 |
|---|---|
| 工作总览 | 指标卡（Features/Tasks/运行中/待确认/阻塞）、Feature 工作队列、当前流程阶段、新建 Feature（含流程输入与前序关联） |
| 执行与交付 | 工作流实例按阶段逐步操作：派发/接纳/批准/补充输入/重试/返工/核对；项目经理协作；调整运行计划；材料与上下文 |
| 角色讨论 | 与 Agent 会话、工具审批与澄清、结构化提案、经验与快照（见 [讨论与经验](conversations.md)） |
| 项目管理 | 项目列表与 GitHub 接入；项目经理独立工作台；项目配置（Workflow/Standard/Role→Agent/执行授权/编排角色与驱动）；项目接入账户 |
| 流程与规范 | Workflow 编辑器、Delivery、Spec 与结构 lint、Standard 快照/比较/采用；配置依赖与精确修订选择 |
| 角色与 Agent | Role 职责编辑；Capability（Model+Harness+Tools+Skills）；Agent（仅 Role+Capability 精确组合） |
| 基础组件 | Model、Harness、MCP、Skills、HTTP 工具、Harness 原生工具（见 [工具与技能接入](tool-skill-integrations.md)） |
| 历史记录 | 当前项目或 Global 的事件与工具调用统一时间线；审批/取消/核对入口；文件结果下载 |
| 设置 | 内核连接、数据路径、仓库接入状态、权限定义与绑定、模板重置、运行时与凭据、全局并发、退出内核 |

页面路由写入 URL（tab/project/feature/子页/所选对象），重开时按当前可访问对象恢复。使用草稿持久化组件的编辑器将草稿保存在本机；这些草稿不等于已发布源修订，旧目录代次或失效引用仍需重新核对。连接密钥不进入 URL 或草稿存储。

## 刷新模型

SSE `/events` 通知变化后全量刷新工作区快照；会话详情（1 秒）、编排面板（2.5 秒）、仓库接入与运行准备（1.5 秒）另有定向轮询。编排状态与配置选项分别读取；目录加载缓慢或失败时仍展示真实经理和调度状态，首次状态查询完成前显示读取提示，不推断为人工调度。写操作统一封装命令信封（command_id、目标、expected_revision 乐观锁），模板类定义写入另带目录代次校验。

## 典型旅程

1. **接入**：工作总览或项目管理页"打开 GitHub 项目"，填仓库与主分支；后台验证、克隆并准备 worktree，接入完成前阻止工程操作（项目管理页显示接入状态与核对入口）。
2. **配置**：项目管理页采用默认配置或自选：入口 Workflow、Standard 快照、Role→Agent、执行授权、编排角色及未来工作的 manual/rust/agent 驱动。独立经理工作台可先查询项目、预演和创作配置，无需已有 Feature。
3. **执行**：执行与交付页选择 Feature：人工逐步派发/接纳，或由已选择的 Rust/Agent 驱动推进。经理协作面显示当前根驱动、真实会话和等待事项，可通过 CAS 切换当前根驱动；项目默认修改只影响未来工作。默认 automatic 步骤无需人类确认；有 human 门禁或人工例外时才签署确切输入输出。
4. **调整**：运行计划变化时在"调整当前运行计划"编辑变更（改任务/增删/绕过/豁免），先预演受影响范围再应用；静止等待后原子切换。
5. **发布**：integration 阶段的候选、候选测试与接纳在页面逐步确认；发布报告与 PR 链接显示在产物与历史中。

## 设置页导览

- **仓库接入**：GitHub 仓库状态、未完成操作核对、worktree 列表。
- **权限**：权限定义（JSON 文本域，创建/修订/导入/导出）、目标版本绑定（预检后应用）、预设目录选择与重载（见 [运行时权限](permissions.md)）。
- **模板**：目录指纹与重置预览/确认（自建定义退出当前目录，绑定与运行保留，见 [存储与数据布局](storage.md)）。
- **运行时与凭据**：默认 Harness 切换、Codex OAuth 登录、DSH 各服务商 key（写入系统凭据库）、共享安装列表（见 [模型接入](model-providers.md) 与 [系统凭据](system-credentials.md)）。
- **并发与退出**：全局并发额度（1–32）、项目编辑、退出内核（优雅停机）。

经理状态与资源目录分开加载，目录错误不会把活动 Agent 或 Rust 调度误显示为人工模式。大产物通过摘要和分块读取展示；任务预览只做配置解析，实际执行任务另含冻结输入及 core 提供的运行事实，预览不能证明已经执行。

实现依据：[路由与导航](../web/src/main.tsx)、[项目管理](../web/src/components/ProjectManager.tsx)、[经理与驱动面板](../web/src/components/WorkflowController.tsx)、[规范 lint](../web/src/components/SpecLint.tsx)、[目录与草稿](../web/src/components/configuration-ui.tsx)。
