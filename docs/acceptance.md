# DevDesk 验收记录

当前 Harness 驱动 Workflow v5 的验收与限制见 [执行报告](plans/harness-workflow-orchestration/EXECUTION.md)。以下保留此前版本的历史执行事实，不作为当前支持范围。

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

## 0.2 原型阶段历史验收

当前新能力的 Ubuntu x86_64 验收见 [Permission 补验](plans/permission-standardization/UBUNTU.md) 和 [Workflow/工作台补验](plans/workflow-and-workbench/UBUNTU.md)。以下保留 0.2 原型的历史验证规模与结果。

本文保留原型阶段的历史验收事实，不代表当前支持模拟执行。当前产品仅使用原生供应商，验证入口见 [测试索引](../tests/README.md)。

日期：2026-11-08。执行模式：simulation。原 Linux 验收为 Ubuntu 24.04.5 LTS x86_64；本轮补充 macOS 26.7.2（25G332）Apple Silicon arm64 实机验收。Linux 结果沿用原交付证据，本轮未在 Linux 重新运行。

## 验证结果

| 检查 | 原 Linux 结果 | 本轮 macOS 结果 |
|---|---|---|
| `cargo test --workspace --locked` | 32 项通过 | 32 项通过（29 集成/契约/调度/存储，3 领域） |
| TypeScript / Vite 构建 | 通过 | 通过 |
| debug host HTTP | 通过 | 通过 |
| release host HTTP | 空工作目录运行通过 | 复制二进制到临时目录运行通过 |
| Chromium Web E2E | 3 项通过，24.2 秒 | debug 3 项通过，31.4 秒；release 3 项通过，23.0 秒 |
| release 构建 | 通过 | `aarch64-apple-darwin` 本机构建通过 |
| 独立评审 | 原阶段评审通过 | 独立 release HTTP 与 Web E2E（3 项，22.0 秒）通过 |

本轮完整检查入口为 `./scripts/check/check.sh`，先构建 Web，再测试和构建 host，干净检出无需预先生成 `web/dist`。新增 HTTP 检查覆盖会话鉴权、Host/Origin、JS/CSS 实际读取、平台能力、中文及空格 data 路径、跨进程单写锁、幂等、Artifact 未知 ID/未鉴权访问、SSE 连接下退出与重启持久化。真实 Artifact 内容由 Web E2E 打开并断言来源标记，文件哈希与 symlink 边界由 Rust 测试验证。

浏览器使用真实 Rust API、SQLite 和本地文件，没有假后端。主流程与异常回路均断言无 pageerror。Web 截图：[原 Linux](assets/workbench.png)、[本轮 macOS](assets/workbench-macos.png)。作者验证与独立运行的范围分别保存在 [macOS 执行报告](plans/devdesk-prototype/macos-validation.md) 和 [独立评审](plans/devdesk-prototype/macos-independent-review.md)。

## 原计划验收矩阵

| 项目 | 证据 | 结果 |
|---|---|---|
| A1 需求到 mainline | a1_full_loop；Web 主闭环含人工需求、双 Task、收尾、Merge、报告/历史 | Linux / macOS 通过 |
| A2 串并行/额度/写许可 | a2_parallel_runs、logical_write_permit；真实后端同时 running 记录 | 两平台 Rust 测试通过 |
| A3 技术失败重试 | a3_technical_retry；Web 异常回路的 Task 与 Merge 重试 | 两平台通过 |
| A4 退回新 Attempt | a4_rejection；Web 退回后新 Attempt，旧版本保留 | 两平台通过 |
| A5 输入失效 | a5_input_change；inflight 失效不阻塞调度器 | 两平台 Rust 测试通过 |
| A6 目标变化重验 | a6_target_change；旧候选阻塞，新候选重新验证/Review | 两平台 Rust 测试通过 |
| A7 规范升级/替换/继承 | a7_spec_upgrade、alternative_spec；旧锁不回写，Task 独立绑定 | 两平台 Rust 测试通过 |
| A8 暂停/取消/Override | pause/cancel、a8_quality_override、取消不可复活与 Completed 冻结 | 两平台 Rust 测试通过 |
| A9 刷新/重启/幂等 | a9_restart、merge_command_replay、HTTP 重启、Web 刷新 | 两平台通过 |
| A10 Context/Snapshot | a10_context；冻结 Attempt 上下文、Snapshot JSON、Web 比较入口 | 两平台 Rust 测试通过 |
| A11 模拟边界 | a11_hard_tdd、simulated_never_live；UI/报告来源标记 | 两平台通过 |
| A12 Linux/macOS 实机 | 两平台后端与完整 Web 主流程均实际运行 | 通过 |

浏览器自动化覆盖三个代表性场景，不声称每个后端异常都已逐一做浏览器自动化。全部异常命令均有 Web 入口，后端规则由上述集成测试覆盖。F0—F5 的本期范围与双平台实机待办已完成；原评语与本轮补充结论分别保留。

## 可重复小样本

同一规模：10 Projects / 100 Features / 1,000 Tasks。分页列表、详情和事件三个查询：原 Linux debug 单次约 **28.82ms**；本轮 macOS debug 单次 **41.396083ms**。均非性能 SLA。

原 Linux release 可执行文件 **6,604,784 bytes**（约 6.30MiB）；空目录首个 HTML 约 **6.17ms**。本轮 macOS release 为 **5,740,880 bytes**（约 5.48MiB）；空目录启动到首个 HTML 五次为 **20.996 / 6.128 / 5.360 / 5.558 / 5.647ms**，中位数 **5.647ms**。启动测量不含数据初始化和浏览器启动。Web JS 266.63kB（gzip83.35kB），CSS14.64kB（gzip4.09kB）。macOS 测量数据（历史路径：`plans/devdesk-prototype/macos-measurements.json`） 含二进制 SHA-256。

## 运行包

`./scripts/release/package.sh` 从锁定依赖构建，生成本机目标的 `.tar.gz` 和 `.sha256`。本轮 macOS 交付为 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`，含独立可执行程序、运行说明、演示及验收文档。构建后运行无需 Node/Rust 或源码；`otool -L` 只列出系统 libiconv 和 libSystem。详情见 [运行包说明](run-binary.md)。

## 已知限制与下一轮边界

1. macOS 实测基线为 Apple Silicon、macOS 26.7.2；未验证 Intel Mac 或更旧系统，不提供签名、公证的 `.app` 安装器。
2. Git/Test/Harness/Sandbox 只有模拟实现；没有真实代码合入、模型推理或被管理项目的测试。live 模式未开放。
3. Archive、迁移只预览；不执行导出、清理、冷数据恢复或搬迁。重启标记中断并显式重试，不透明重建真实环境。
4. 工程存储 port 当前每次工作单元加载类型化状态；查询输出分页，表有外键与索引。已覆盖小样本，未承担大量 Run 历史/百万级压测。后续可在同一 port 内改增量查询。
5. Spec 仅解释内置有限规则；模拟建议为确定性模板，可编辑，不是智能生成。Review 结果与 Override 保留原事实。
6. 当前 Delivery 列表的 Review 展示有界；多次不确定决定的大量历史可通过分页事件与报告读取，尚未提供独立全文检索。协议不支持 JSON-RPC batch/notification。

按 v0.2 停止线交付并等待实际使用反馈，真实 Git/Test/Codex 和数据生命周期继续留待后续讨论。

## 领域补强后续验收

本文件上方保留原 v0.2 平台交付记录。领域补强的最终实现、v1→v13 数据迁移、操作核对及本轮 macOS 验证见 [补强验收报告](plans/domain-hardening/domain-hardening.md)；不能将上方历史 Linux 验证视作补强代码的 Linux 复验。

## Ubuntu Web 真实工程复验（2026-11-08）

上方原型记录保留历史。本轮 Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过：完整离线基线、Linux 系统凭据、双 Harness Web 工程闭环、真实 PR #6/#8/#9 merged、安装复用、取消/跨项目崩溃恢复和独立 Linux 发布包。Docker 嵌套沙箱使用用户明确批准的专用 AppArmor 策略；首次失败及显式续跑历史保留。full、verify-existing、完整安装探针均退出 0，证据与实际/契约边界见 [Ubuntu 完成报告](plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。
