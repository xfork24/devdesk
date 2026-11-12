# 工程接入与通用流程

DevDesk 使用 Workflow v6 驱动工作。Role 是业务身份，Agent 加载自己的 Skill 和能力，Step 的 `task_instructions` 指定本次任务。运行语义见 [工作流与编排](workflow-orchestration.md)，出厂配置见 [默认目录](../resources/configuration/README.md)；历史改造合同存档于 [Harness 执行合同](plans/harness-workflow-orchestration/CONTRACT.md)。

## 操作流程

1. 工程项目通过“打开 GitHub 项目”接入 `owner/repo` 与主分支，后台验证、克隆并登记受管理工作空间。实际 GitHub 操作需要 Git 和已授权的 GitHub CLI。仓库操作失败保留原操作 ID，供核对。
2. 在设置中完成所用供应商授权并确认 Docker 可用。运行环境在实际调用时准备；读取配置不安装运行时或调用模型。供应商与凭据边界见 [模型接入](model-providers.md) 和 [系统凭据](system-credentials.md)。
3. 在“流程与规范”编辑 Workflow：Stage 提供背景和可选默认 Role，Agent Step 可覆盖 Role，添加任务指令、必需 Skill、规范、输入输出、访问范围及预算。工具 Step 和子流程调用使用精确修订。保存草稿允许未完成引用，发布必须校验完整闭包。
4. 在“项目管理”选择 Workflow，将所有需要的 Role 身份绑定到相应 Agent，可选 Standard。新建 Feature 时冻结完整配置、权限和接入绑定；后续修改不替换已有运行。
5. 在“执行与交付”与项目经理对话，通过 Harness 原生工具明确派发、接纳和完成根/子实例；工作台直接操作使用相同领域命令。查看真实 Stage/Step、输出和等待状态，运行计划修改先预演再应用。补充输入通过当前 Step 提交；确认绑定当前输入摘要及输出 ID。技术重试保留输入并创建新执行，返工沿实际消费关系失效后继结果。未知副作用先核对。

普通分析流程可通过 API 在 Project 上启动，不需要 Task、Git 候选或固定文档类型。执行视图支持 Project 直属运行。材料采用类型、修订与显式输入引用，讨论提案采纳只建立材料，不隐式生成 Feature 或 Task。

## 实际执行边界

Agent 运行保存 canonical `agent-task/v6`、Session、Turn 和真实 worker。任务正文、职责、Skill、Spec、输入、输出合同与授权分别保留来源。权限由 Agent 能力上限、Step 访问声明、完整 Permission 与执行后端共同落实，业务 Role 不能扩大权限。写入必须使用受管理工作空间及明确路径。

平台工具执行真实工作空间查询、快照、Task 创建、测试、候选与发布；工具运行不借用占位 Agent。测试使用固定代码 SHA、明确 Node TestRecipe、固定镜像、无网络容器和结构化失败报告，断言失败与环境故障分开。新增语言或 runner 需要实现新的适配契约。

默认 Task 使用显式快照衔接写入者：`tests_snapshot` 封存独立测试，Red 基线 Agent 使用该确切版本；`red_snapshot` 再固定 Red 源码。实现后也先封存再测试，不允许后一个写入者在来源未经核对的脏工作树上启动。

Git 候选以预期目标为父提交。默认接纳要求确切候选的真实通过测试、独立评审以及 Harness 项目经理的显式接纳；默认确认策略为 automatic，无需 Human 签署。配置为 human 的门禁仍要求确切执行、输入和输出对应的签署。当前计划和编排策略可明确豁免业务要求，并独立记录理由与未验证范围。core 核验当前 ActionPermit、实际 CAS/preflight 及来源，远端发布使用精确 lease，PR 接纳结果须与同一候选匹配。豁免不产生成功测试或人的签名。

所有外部调用先持久化意图，再在事务外执行，最后接纳回执。取消必须停止实际 worker 或测试容器；未知调用不直接重放。重启后保留待核对状态，副作用按原操作 ID 恢复。切换项目和关闭浏览器不取消后台工作，Host 退出停止自己拥有的执行者。

## 验证与开发数据

`./scripts/check/check.sh` 验证 Rust、HTTP、安全、SQLite、模板重置及 debug/release Chromium。真实 Docker 验证可使用 `scripts/check/check-live.sh --groups git,docker,permissions`，不调用模型。真实模型与 GitHub 发布须使用专用数据集和测试仓库；历史版本的验收报告不代表新执行器已完成相同实机验收。

当前配置使用源协议 v2，数据库基线为 global 16 / project 47。仅支持新数据集，不提供旧协议转换或升级；验证使用独立空目录。既有目录会被明确拒绝且不改写。

合并后的完整 Feature 验收还核对 Task documentation/review/candidate_review 和 Feature closure_review/candidate_review 的实际渲染任务封套：原始逐增量 TDD evidence 必须完整到达，输入来源必须指向相应的密封输出。默认 automatic 数据集中出现 Human 确认、人工批准、旁路或豁免即判失败；不允许通过手动 approve 完成探针。运行计划原始合同通过不可变修订祖先核对，不能将修订后的图当作自身验收基线。
