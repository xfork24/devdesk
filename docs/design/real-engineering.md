# 工程接入与通用流程

DevDesk 使用配置驱动的 Workflow 执行工程工作。Role 是业务身份，Agent 组合 Role 与 Capability，后者选择 Model、Harness、Skill 和 Tool；Step 的 `task_instructions` 指定本次任务。运行语义见 [工作流与编排](workflow-orchestration.md)，出厂配置见 [默认目录](../../configurations/README.md)。

## 操作流程

1. 工程项目通过“打开 GitHub 项目”接入 `owner/repo` 与主分支，后台验证、克隆并登记受管理工作空间。实际 GitHub 操作需要 Git 和已授权的 GitHub CLI。仓库操作失败保留原操作 ID，供核对。
2. 在设置中完成所用供应商授权并确认 Docker 可用。运行环境在实际调用时准备；读取配置不安装运行时或调用模型。供应商与凭据边界见 [模型接入](model-providers.md) 和 [系统凭据](system-credentials.md)。
3. 在“流程与规范”编辑 Workflow：Stage 提供背景和可选默认 Role，Agent Step 可覆盖 Role，添加任务指令、必需 Skill、规范、输入输出、访问范围及预算。工具 Step 和子流程调用使用精确修订。保存草稿允许未完成引用，发布必须校验完整闭包。
4. 在“项目管理”选择 Workflow，将所有需要的 Role 身份绑定到相应 Agent，选择或生成 Standard，选择 Sandbox、可选双层 Profile 和 manual/rust/agent 驱动；调度参数由 Workflow 自身版本固定。项目经理独立工作台可以先查询资料和创作配置，它不自动取得根实例控制权。精简版还须保存测试语言、独立执行器及项目回归命令/镜像；新建 Feature 时冻结完整配置、权限和接入绑定。
5. 在“执行与交付”查看 Stage/Step、输出和等待事项：人工逐步操作，或由所选 Rust/Agent 驱动派发、接纳和完成根/子实例。Agent 控制目前需要 Codex 原生工具桥。运行计划修改先预演再应用；补充输入经当前 Step 提交。精简版 PRD/Design/Tasks 评审和最终 Feature 合入具有 Human 门禁，其余步骤 automatic；确认绑定当前输入摘要及输出 ID。技术重试保留输入并创建新执行，返工沿实际消费关系失效后继结果。未知副作用先核对。

普通分析流程可通过 API 在 Project 上启动，不需要 Task、Git 候选或固定文档类型。执行视图支持 Project 直属运行。材料采用类型、修订与显式输入引用，讨论提案采纳只建立材料，不隐式生成 Feature 或 Task。

## 实际执行边界

Agent 运行保存 canonical `agent-task/v6`、Session、Turn 和真实 worker。任务正文、职责、Skill、Spec、输入、输出合同与授权分别保留来源。权限由 Agent 能力上限、Step 访问声明、完整 Permission 与执行后端共同落实，业务 Role 不能扩大权限。写入必须使用受管理工作空间及明确路径。

平台工具执行真实工作空间查询、快照、Task 创建、测试、候选与发布；工具运行不借用占位 Agent。测试使用固定代码 SHA、明确 TestRecipe、固定镜像、无网络无凭据容器和结构化失败报告。独立测试支持 Node test 与 Python unittest，项目回归还支持显式 argv/cwd/environment 和准备命令；断言失败与环境故障分开。

精简版 Task 在独立测试之后由平台快照封存测试及声明的固定空接口，不允许模型先实现业务行为。实现快照核对测试保护路径；Green、独立代码评审与源回归针对同一源版本。Feature 在所有 Task 合入并同步后才进入整体评审。Task 和 Feature 均执行源版本与试合并结果两类项目回归。

Task 直接以开发分支创建 PR，使用 GitHub 试合并的确切源/目标双父提交及 tree 进行候选回归。Feature 在全部 Task 合入同步、源代码评审和回归通过后，按冻结声明剔除流程验证材料、保留正式项目测试，生成以确切目标为单父提交的代码投影及专用 candidates 分支；候选回归、Human 审核和合入绑定这份投影的 SHA/tree。PR 创建不要求新增保护规则；无保护目标通过 beforeOid 与 force=false 原子快进，已有保护目标遵守现有 PR 合入门禁。Task 自动合入所属 Feature，Feature 合入主分支须确切版本 Human 决定。核对最终父提交/tree 和本地同步后才登记完成。门禁不能由模型报告、普通批准或豁免替代；目标变化须重新回归和审核，未知效果先核对。历史详细版仍按原冻结合同恢复。

所有外部调用先持久化意图，再在事务外执行，最后接纳回执。取消必须停止实际 worker 或测试容器；未知调用不直接重放。重启后保留待核对状态，副作用按原操作 ID 恢复。切换项目和关闭浏览器不取消后台工作，Host 退出停止自己拥有的执行者。

## 验证与开发数据

`./scripts/check/check.sh` 验证 Rust、HTTP、安全、SQLite、模板重置及 debug/release Chromium。真实 Docker 验证可使用 `scripts/check/check-live.sh --groups git,docker,permissions`，不调用模型。真实模型与 GitHub 发布须使用专用数据集和测试仓库；历史版本的验收报告不代表新执行器已完成相同实机验收。

当前配置使用扁平源协议，数据库基线为 global 24 / project 60。不提供旧协议转换或升级；验证使用独立空目录。不兼容的既有目录会被明确拒绝，兼容数据集可正常重开，不通过删库恢复。

完整工程验收核对真实 DAG 并发、依赖基线、源版本、独立 Red/Green 与评审、两类回归、PR 归属、远端保护和本地同步。原生回执与模型记录分别读取；Human 确认通过可信入口绑定确切材料，经理不能代签。历史合同兼容性回归与精简版实际默认验收分别报告。

## 本地验收证据保留

`tests/live/probe_streamlined_workflow.py` 在独立数据目录观察精简版实际默认 Feature/Task 图，保存构建身份、自动识别的项目测试入口、状态时间线及确切待决定材料，不生成 Human 决定。它调用真实 Codex、Docker、Git 和专用 GitHub 测试仓库；需明确提供二进制、认证源、需求文件和证据目录。`--performance` 保存原生计时和观察查询样本；同一证据目录的 `--resume` 继续观察已有实例。详细版原生入口已退役。

`tests/live/export_evidence.py` 保存一致的项目 SQLite 快照、按登记摘要核验的产物、遮蔽凭据字段的记录投影、实际执行安装事实和逐文件 SHA256 清单。它不复制全局凭据设置、认证文件、worker 状态或进程环境。导出到新的本地私有目录；保留失败/未运行项，不能仅凭模型回复宣布验收通过。证据导出本身失败也必须报告。

验收保留实际使用的 Host 与各辅助脚本的内容身份，历史执行不从当前 PATH 或当前源码补推。`runtime_context.binding` 表示运行选择，`runtime_context.inputs` 关联上游真实 Turn 的外层镜像和停止证明；它们不证明任意嵌套或远程命令的环境。

实现依据：[工作流运行](../../crates/core/src/workflow_runtime.rs)、[根驱动](../../crates/core/src/workflow_driver.rs)、[平台动作](../../crates/core/src/platform_tools.rs)、[工程探针](../../tests/live/probe_streamlined_workflow.py)、[证据导出](../../tests/live/export_evidence.py)。
