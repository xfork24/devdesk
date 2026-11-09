# Role、Capability 与项目配置实施记录

日期：2026-11-09。P0–P7 已完成本轮 macOS 实现与验收。自动化检查及专用测试仓库的真实 Codex 工程闭环通过；Ubuntu 原生验收未在本轮重跑。

## 已实现

- 五组配置导航：项目管理、工作流程、角色与 Agent、规范、基础组件。Role/Capability/Agent、Spec/Standard 及基础组件分别使用页内 tab；Permission 留在设置。Workflow 列表与两列编辑器分开，支持草稿、复制、历史修订、阶段/步骤/子流程维护、深链接和按对象恢复未保存内容。
- Role 的职责 Markdown、Global/多项目可见范围独立维护；共享角色由全局目录索引同一身份和修订历史，保留原数据库归属。编辑 Role 不派生 Skill 或 Agent。停用检查在用引用，不改写历史。
- Capability 固定一个 Model、一个 Harness 和多个 Tool/Skill。命名 Agent 固定 Role/Capability 修订，执行字段由服务端展开并验证；同一执行职责下不同 Role 身份不能混用。Harness/Sandbox 两层 Permission 各自保留精确引用。
- Workflow v2 的 Step 与执行配方节点声明 Role；项目为所需 Role 单选 Agent。Step 显式绑定 Spec，Standard 是可选覆盖基线。未选 Standard 仍执行工程门禁；基线按动作、上下文、精确 Spec 和最低节点数量校验覆盖，不注入规范。
- 规范审计记录调用路径、Stage 意图、动作、Role、配方、Spec/检查方式、依赖、条件、输入和输出契约。执行、评审及复用 Session 的每个轮次收到冻结的 Spec；后续步骤或定义变化不改变已排队轮次的输入。
- 独立 `workflow.call` 支持递归编译、输入/输出别名、真实内部节点、入口条件/人工确认、阻塞/失败传播和返工。父调用未进入时内部节点不执行；返工重启该调用的内部节点；深度最多 8、展开最多 1000 节点。
- 项目配置显式保存、Feature 冻结完整依赖闭包。仓库接入、项目配置和运行环境分别呈现；可见范围收窄阻止新使用，原 Feature 仍能据配置锁创建对应 Agent 实例继续执行。
- 追加全局 006、项目 035 迁移。旧 Agent 生成可追溯 Capability 关联，保持 Agent/实例身份；旧 Workflow 与各自 Standard 的组合派生独立 v2 规范绑定。迁移幂等，旧源码修订和历史配置锁保留；无法证明的映射标记需要配置。

## 两次用户补充

[PLAN.md 第 12 节](PLAN.md#12-实施期间补充可直接采用的默认配置与阶段能力)已纳入实现：保留完整、可直接采用的默认配置，同时保持独立领域对象。项目页预填并允许保存或调整，不自动追随默认资源后续修订。

默认目录包含六个职责 Agent、相应 Capability、七个专用 Markdown Skill、八个阶段 Spec、分阶段 Feature/Task 工作流，使用 GPT-6.1 Sol High、固定 Codex Harness、现有原生工具及 Docker 默认权限。架构师产出 Design Doc，独立项目经理负责 Task Plan；两者身份和能力不同。需求产出 PRD，测试/开发/评审按阶段能力和 Spec 执行。

资源正文见 [默认配置目录](../../../resources/configuration/README.md)。本轮真实模型只调用 Codex，未调用 DSH。

## 自动化与实机检查

- `./scripts/check/check.sh`：Web 构建、Permission 资产、Rust workspace、debug HTTP/准备回归/Chromium、release HTTP standalone/Chromium 通过。默认 Rust 108 项通过、8 项需额外外部资源而忽略；debug/release Chromium 各 10 项通过。新增草稿恢复及导航修复后，另跑 Web 构建和双 profile Chromium 验证。
- `configuration_resources` 15 项回归涵盖独立维护、引用闭包、错误 Role 身份、可选基线、规范落点审计、默认阶段内容、逐轮冻结输入、子流程确认/输出/失败返工和不同旧 Standard 的隔离迁移及旧自定义 Role 的身份保留。
- Schema 回归 10 项通过；新增 v2 默认工作流、阶段 Markdown、Capability、Agent、Spec/Standard 及项目配置契约覆盖。所有 Schema 与引用由 Rust 离线编译；Permission 脚本中的可选 Python jsonschema 未安装，不影响 Rust Schema 回归。
- Host 回归 11 项通过，包括跨项目目录可见性、两层 Permission 分别解析，以及 Scope 收窄后新使用被阻止而冻结 Feature 保留实例创建能力。
- 真实 Docker：`actual_node_red_green_environment_failure_and_stopped_receipts`、`real_container_ownership_idempotence_stop_and_cleanup` 单独执行通过；不调用模型。
- 真实 Codex：默认 PRD Agent/Capability/Skill 的独立轮次完成并提取 requirement 提案；随后专用 GitHub 测试仓库的完整 Workflow 通过，21 个正式 Run 成功、6 次独立评审通过、Task/Feature 两次接纳完成，远端 HEAD 与最终提交一致。详细记录见 [NATIVE.md](NATIVE.md)。
- `cargo fmt --all -- --check`、`git diff --check` 通过。

原生闭环初次因 GitHub 443 不可达停止；用户恢复网络后 `ls-remote` 成功，并沿原仓库接入操作核对恢复。恢复后的原生验收已通过：可复用 PRD 子流程、需求/设计/任务规划/测试提案、真实 TDD、独立评审与候选接纳。最终主干提交为 `814de73354fe67412ef90d2638a194e92768d0aa`。

日志、截图、数据库和原生协议均位于本轮 `/tmp/devdesk-v2-*` 或独立临时数据集中，不提交为计划交付物；文中路径仅说明本轮执行位置，不承诺临时文件长期存在。没有使用夹具结果冒充真实模型、Docker 或远端接纳。

## 2026-11-09 补充：AgentID 经验归属于上下文

按用户修正，AgentID 经验用于检索、定位 Feature/Task 问题和恢复现场，不再作为 Tool。全局与项目 Tool 查询、UI 列表、实现选项和 Capability 的 Tools 选择排除经验；新 Tool/Capability 写入拒绝经验工具。旧不可变记录、AgentID、项目绑定及配置锁保留，旧引用只作兼容元数据，不启用模型工具。

删除原生 worker 的经验目录生成、`experience_tool` 输入、Docker 挂载及额外目录权限。应用的经验目录查询、快照完整性验证、定位和恢复接口保留。默认工程上下文/阶段 Skill 与 Capability 追加 v2 修订，职责 Agent 追加 v3 修订；默认 Tools 仅为 `harness.native`。已有自定义修订不会因默认资源升级发生版本冲突，项目绑定不自动改写。

`./scripts/check/check.sh` 全部通过：Rust 110 项通过、8 项忽略，debug/release Chromium 各 10 项通过，HTTP、准备与独立二进制检查通过。新增回归覆盖全局/项目目录过滤、旧精确修订与 Agent 可用性、默认修订追加、用户已有版本的升级冲突和禁止新增经验工具。Web、debug/release Host 已重建并签名；`cargo fmt --all -- --check`、`git diff --check` 通过。

本次检查日志在 `/tmp/devdesk-experience-context-*`；不调用真实模型。此前 [NATIVE.md](NATIVE.md) 仍是修正前那一轮原生闭环的历史记录。

## 2026-11-09 补充：阶段折叠与 Markdown 编辑体验

- Workflow 阶段标题支持整体展开/折叠，隐藏该阶段所有步骤和子流程引用，不影响其他阶段。展开状态按流程保存在本机草稿；折叠时切换到阶段配置，添加步骤时自动展开其所属阶段。
- Role 职责、Spec/Standard/Skill 正文、阶段意图、讨论目标与必谈主题默认渲染 Markdown，点击“编辑源码”后编辑原文，“完成编辑”返回预览。主题逐项保留完整 Markdown，不再按逗号或换行切碎。切换对象与重载恢复文本时默认回到预览。
- 共享渲染器支持标题、强调、列表、表格及代码块，按需加载。Tool 的 JSON Schema 等结构化字段继续使用原有编辑方式。
- 修复“编辑流程”请求未携带 `base_revision_id`。已发布流程（包括子流程）从准确来源建立新草稿，保留当前所选阶段/步骤；编辑、保存和发布均不改写原有版本。

验证：Web 构建、Rust workspace（110 项通过、8 项忽略）、HTTP、准备回归、release 独立二进制检查通过；debug/release Chromium 各 12 项通过。新增浏览器回归实际保存并重载 Spec/Standard/Skill 的 Markdown，校验讨论目标/主题的完整原文和阶段折叠状态，并从父流程进入子流程完成新修订发布、核对旧版不变。debug/release Host 已重新构建并签名，格式及差异检查通过。本次不调用真实模型；日志在 `/tmp/devdesk-workflow-ui-*`，截图为未提交的浏览器临时产物。

## 2026-11-09 补充：原生工具组隐藏占位 Schema

`harness.native` 的工具表单隐藏输入/输出 JSON Schema，说明沿用 Harness 原生协议，实际权限另行解析。保存忽略旧界面遗留的输入/输出 JSON 源码缓冲区，保留已有修订的元数据，新建使用兼容占位对象。没有修改原生工具执行或宣称 JSON 字段可以定义可用工具。

后续 API/MCP/Skill 独立接入页与完整执行通路见 [工具与技能接入计划](../tool-skill-integrations/PLAN.md)，本轮仅编写计划，尚未实施接入功能。

本次 Web 构建、`git diff --check` 与 debug/release 原生工具组定向浏览器回归均通过：Schema 不显示，旧 JSON 源码缓冲不会阻止新建保存，提交数据不携带表单缓冲字段。debug/release Host 已构建并签名。没有运行真实模型或外部 API/MCP 调用；日志位于 `/tmp/devdesk-native-tool-schema-*`。
