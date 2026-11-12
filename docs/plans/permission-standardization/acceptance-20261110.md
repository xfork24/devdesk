# 权限改造验收（2026-11-10）

> 归档说明：本文件自顶层 `docs/acceptance.md` 迁入（2026-11-12 按计划目录归档拆分）。内容为当时执行事实，不作为当前支持范围；当前验收见 [Harness 编排执行报告](../harness-workflow-orchestration/EXECUTION.md)。

## 历史：2026-11-10 权限改造与当时开发数据集清理

已迁移实际数据集 `/Users/hengliu/workspace/devdesk/data`，服务仍使用 `http://127.0.0.1:4318`。唯一项目为 `devdesk-project-1`（`7bb3ae37-523f-4e52-b9fa-f587807651dc`）。项目没有 Feature、Task、Session、Run、Execution 或 Snapshot 历史，因而没有旧执行需要保留或重建。下列数量按全局与项目镜像去重，不把同一配置计算两次。

| 对象 | 迁移或清理结果 |
|---|---|
| 6 个 Agent | 需求分析师、架构师、项目经理、开发、测试、评审全部移除旧 `permissions/approval` 字段；显式声明 Harness/Sandbox `follow_target`。保留现有身份、Role、Capability 和用户选择的 Model/Harness 引用。 |
| 全局默认与资源 | Codex、DSH、Docker Sandbox 三个目标均使用当前 v2 全权限活动绑定；旧 Harness/Sandbox 元数据中的权限引用改为完整解析策略。全权限的文件/网络及可用工具策略保持明确，Harness 审批为 never。 |
| v1 权限配置 | 删除 container-full-access、read-only、task-implementation、test-author、isolated-tests、legacy-restricted 共 6 个旧预设、旧 `permissions` 表、全局覆盖配置和项目覆盖列；删除 2 个兼容 Profile、4 个兼容 Binding、6 条转换记录、6 条旧文件来源及 2 条旧预设重载回执。仅保留 3 个当前 v2 Profile、3 个有效 Binding、3 个活动分配及 3 条当前预设来源。 |
| Standard / Spec / Skill | 1 个 Standard 转为显式 `baseline_rules`；删除 planning/implementation/merge 三个旧 Spec 和 5 个未被当前组件引用的旧 Skill。最终保留 8 个阶段 Spec、9 个 Skill。 |
| 默认生成链 | 删除 1 条 defaults_bundle 和 1 条 catalog_compaction；代码直接嵌入当前目录，不再依次生成旧 Agent、Capability、Workflow 或兼容投影。当前目录包含 6 个 Agent、7 个 Capability 和 2 个 v3 Workflow。 |
| Workflow / 冻结任务 | 仅接受 Workflow schema v3、compiler v3 与 agent-task/v3。2 个 Workflow 清除旧空讨论目标字段；删除 v3 转 v2 校验、旧提示词、旧任务输出合同、旧无任务元数据推导和旧失败意图修复分支。 |
| 运行偏好与资源 | 运行偏好只保留精确组件引用及 provider；删除原始模型名、思考深度和安装 ID 回退。删除模拟 Harness/Sandbox 元数据及未使用的旧默认 Harness 元数据；用户自建的当前 Model、接入源和退役记录保持原意。 |
| 凭据接入 | DeepSeek 配置键迁到 `credentials.dsh.deepseek`，与其他服务商一致；移除旧固定账号探测和助手协议 v1 接受路径。当前已安装助手协议为 v2，三个服务商的显式凭据引用均可用，未读取或重写密钥。共享助手未替换。 |
| 数据库与协议 | 使用全局 v10、项目 v41 两份当前 SQL 基线；删除 49 份历史逐级 SQL、原型单库拆分入口和旧迁移夹具。移除 legacy 交互回答与 simulated 来源；旧数据结构被明确拒绝。 |
| Web / 旧执行接口 | 删除旧权限设置、角色权限覆盖、原始 Binding 编辑器和旧导航别名；移除 v1 权限 RPC、旧扁平 Agent 创建和历史 Git 回执/测试执行器回退。真实验收脚本使用当前合同，旧权限与原型恢复探针删除。 |

独立的 HTTP/SDK 协议、原生工具组和后端资产仍按其当前版本编号；标识中的 `v1` 不等于权限 schema v1。没有仅为消除字符串而改写当前资源身份或已验证后端资产。

迁移前分别备份全局与项目数据库到 `/Users/hengliu/workspace/devdesk-data-backups/permission-v2-20261110-140441`；补充字段清理前的备份在同目录下 `current-config-20261110-141537`。备份位于活动数据集外，权限为私有，未复制凭据文件。一次性迁移脚本和测试输出位于系统临时目录，不进入产品运行路径。

迁移后逐表检查已删除权限 ID、兼容 Profile/Binding、旧覆盖字段和固定账号引用；核对全部治理记录内容摘要、Agent/Capability/Spec 引用、三个目标的活动绑定、SQLite integrity_check 和 foreign_key_check。实际服务重启后全局/项目各 73 条治理记录及全部摘要一致，没有再生成旧配置。原项目、6 个 Agent 和 5 个 AgentID 保持可用。

验证结果：

| 检查 | 结果 |
|---|---|
| Rust 默认回归 | 174 项通过；15 项依赖外部条件的用例按默认配置忽略。 |
| 当前 Web Chromium 回归 | debug / release 各 38 项全部通过，使用真实 Rust API 和临时 SQLite 数据集。 |
| Web / Host 构建 | TypeScript、Vite、debug 与 release 构建通过；Host 按现有证书签名。 |
| HTTP / 运行环境 | debug 与独立复制的 release HTTP、鉴权、项目隔离、SSE 退出/重启及运行环境准备回归通过。 |
| 原生权限边界 | 显式运行真实 Docker 中的 Codex/DSH v2 文件和网络边界测试，1 项通过。 |
| 凭据助手 | 协议 v2 复用通过；旧协议、改动内容和外来签名均拒绝，共享安装未替换。 |
| 权限资产 / 代码格式 | 3 份当前预设、4 个后端和 7 份不可变资产检查、rustfmt 与 diff 空白检查通过；Schema 离线回归 12 项通过。 |
| 实际数据集 | 内容摘要、引用、活动绑定、数据库完整性与重启幂等审计全部通过。 |

全部验证使用独立临时数据集或只读检查；未调用真实模型、未修改远端仓库。

## 提交前全面复查（2026-11-10）

二次审计继续覆盖代码分支、完整 Schema、默认目录、实际数据库 DDL 和引用、脚本入口、本地文档及图片链接。复查补清了以下内容：

- 删除两份 SQL 基线中的旧转换索引，并在实际全局/项目库中删除该索引。修改前的数据库备份保存在活动数据集外的 `second-pass-20261110-142948`。
- 默认选择仅按当前目录的明确类型和身份寻找最新已发布修订，删除旧 ID 后缀和 prose 命名推断；保留已发布旧修订作为当前版本化对象的历史，不再误选它们作为新默认。新增回归覆盖旧修订仍存在时的默认选择。
- 删除合并式 planning 执行阶段、宿主及领域分支、内置节点配方回退、旧阶段交付拆分及其 Web 入口。执行只能使用当前冻结 Workflow 的明确阶段和节点配方。业务状态及可自定义的阶段 key 中的 planning 仍是当前含义。
- FrozenAgentTask 的 Schema 只声明 agent-task/v3；正式证据输出直接使用 evidence-checks/v2，不再先生成旧合同。旧渲染器的 Schema 与领域拒绝均有回归。
- 修正选定 Standard 后冻结配置没有保存其引用的问题，确认 Standard 引用、覆盖检查及定义摘要一致；删除缺失 ModelConnection 时的旧服务商特例。
- 删除真实检查入口中已无用途的 root 参数，修正旧分组/mode 说明。28 个 Python 模块语法检查通过，真实讨论探针按明确 Role 身份选择 Agent。
- 清除 218 个失效本地文档链接；原路径改为明确的历史说明，不重新制造旧报告或旧源码。剩余本地文档/图片链接及 6 处本地锚点均有效，当前源码和指南不再引用已删除文件。
- 讨论身份切换立即清空旧会话列表并作废在途请求，避免迟到响应恢复旧身份；删除旧项目 agent_refs 读取分支。浏览器夹具使用明确角色绑定及完整当前配置元数据，身份切换专项重复 6 次、运行偏好专项重复 3 次通过。

实际项目目录与数据库目录一致，没有孤立项目库、原型根数据库或独立旧配置文件。全局与项目的内容摘要、AgentID、权限活动分配、文件来源、物化资源及退役记录的引用均已逐项核对；Feature/Task 两个默认流程和当前 Standard 可完整编译。代码提交不包含数据集、凭据、构建产物或临时日志。
