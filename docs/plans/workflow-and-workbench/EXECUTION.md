# Workflow 与工程工作台执行记录

后续 Ubuntu 补验：2026-11-09 已完成当前源码完整回归、Codex 原生 Feature、实例经验和工作台验证，并修复评审 Run 失败后步骤仍为 active 的问题。按用户最终范围，DSH 暂停且不计为完整闭环通过；详见 [Ubuntu 补验报告](UBUNTU.md)。下文保留首轮 macOS 的执行记录。

日期：2026-11-09。状态：已完成。

## 实现结果

| 阶段 | 结果 |
|---|---|
| W0 | Scope、不可变修订、完整 Agent 组合、实例范围及流程/执行边界已落实；见 CONTRACT.md |
| W1 | 全局 v5 / 工程 v34 迁移、SQL 不可变约束、Project 默认修订、Feature 配置闭包和 legacy_feature_lock |
| W2 | Workflow 草稿保存、校验、预演、发布、精确子流程引用、有限动作及端口、五种规范职责编译 |
| W3 | 持久化 intent 驱动讨论、采纳、分阶段交付、TDD、Review、候选和合入；有限重试、确认、返工与固定 TaskGroup |
| W4 | AgentID 经验目录、三项主要筛选、跨库只读、离线可用性、完整性检查、原生只读经验目录和来源恢复 |
| W5 | 新导航、项目统计卡片、Feature/Task 定位、六个工作台页签、冻结配置、讨论推荐、步骤控制与证据整合 |
| W6 | 工作流结构化编辑、保存/发布/单独绑定，Model/Harness/Tool/Skill/Spec/Standard/Agent 及实例管理 |
| W7 | Rust/HTTP/Web/真实 Git 回归、原生专用仓库验收和文档整理；外部结果以本报告后续及 NATIVE.md 为准 |

## 已执行检查

- 首轮完整 `./scripts/check.sh` 通过：Rust 126 项通过，8 项外部资源用例 ignored；debug/release HTTP、运行环境准备、debug/release Chromium 各 8 项通过。
- 新增治理定向测试覆盖：实例独立 Scope、不可变发布和重放、旧 Feature 不换绑、非法图/Scope、需求先行及精确失效、合入前确认、完整模拟工作流和固定成员汇合、跨库 AgentID 查询及篡改拒绝、旧 live Feature 保守冻结。
- 完整 `./scripts/check.sh` 复验通过：Rust 129 项通过、8 项外部资源测试 ignored；debug/release HTTP、运行环境准备、debug/release Chromium 各 8 项通过。结束流程只读的新增断言另跑完整工作流定向回归，通过。原生模型/GitHub 结果单列，不计入离线测试数。
- `cargo fmt --all -- --check`、`git diff --check`、TypeScript/Vite 构建与新增 Python 验收脚本语法检查通过；新文档本地文件链接有效，计划目录非 Markdown 文件数为 0。
- 默认 Python 未安装可选 jsonschema，权限脚本明确跳过该项；另使用隔离的 uv/jsonschema 环境验证 9 个 Schema，以及真实原生数据集的 13 个 Workflow/Model/Agent/Tool 正文，通过。编译器动作目录与 Schema 枚举的一致性另有 Rust 回归。

## 原生与页面验收

- [Codex 完整 Feature 闭环](NATIVE.md)：实际需求与方案两段 Delivery/Review、Task 断言 Red/Green、候选测试/独立评审、人工确认、主干接纳和最终快照/清理完成。GitHub PR #15 的 head.sha / merge_commit_sha 均为 faf9142a61cc037570c0b4b9b94c122e6f1d23a2，且 merged=true。
- [跨项目实例经验](EXPERIENCE.md)：同一精确 Harness 下 gpt-6.1-sol / gpt-6.1-terra 两个组合及实例隔离；原生工具索引后读具体快照，Global 实例跨项目恢复、Project 实例越界拒绝，当前 Project 不被历史覆盖。
- [DSH 完整组合](DSH.md)：deepseek-v4-pro 真实消费输入并完成原生 Turn；拒绝旧角色入口和独立 Model 覆盖。此项是组合及原生对话验收，不宣称重新执行了 DSH 的完整 Feature。
- 在真实验收 Host 上检查 1440px 桌面及 390px 窄屏：七个导航项、完整流程节点、直接 Feature 地址正常，无浏览器 pageerror 或横向溢出；结束后的实际 Workflow 无暂停/返工控件，直接 RPC 返工被拒绝且持久化历史不变。截图只作临时视觉检查，未保留在计划目录。

## 验收矩阵对应位置

| 范围 | 实施及验证位置 |
|---|---|
| A1–A4、A17–A22、A37–A44 身份、Scope、修订及锁 | governance.rs 集成测试、域校验、SQL 触发器；EXPERIENCE / DSH 原生记录补充实际 Model 和范围 |
| A5、A13、A16 渐进经验及恢复 | 跨项目只读/篡改/离线测试，原生索引与详情工具调用，跨项目新 Session 恢复 |
| A6–A10、A30–A36 工作流及工程门禁 | 治理完整流程、分阶段失效、确认/暂停/重试用例，既有 68 项工程集成测试；NATIVE 记录真实闭环及目标基线变化 |
| A11–A12、A23–A27、A35、A44 页面和定义 | Chromium 编辑/预演/发布/刷新、组合管理、项目隔离，直接定位与分页后端回归，真实桌面/窄屏检查 |
| A14–A15、A28–A29 持久化与恢复 | SQLite 旧库/新库、不可变发布/重放、HTTP debug/release 重启及分库隔离、验收 Host 原 operation 恢复 |

该表记录分层验收位置，不表示 A1–A44 每一项都单独运行过外部模型。真实、注入和模拟结果分别归档；现有 Ubuntu 报告不算本轮新能力的复验。

## 原生执行中的修复

1. 已完成/取消流程在 core 和运行视图均只读，禁止用返工或暂停/恢复重新激活。
2. 精确 Harness 构建匹配，避免复用同一基础版本的不同镜像。
3. 复用原生 Session 时，每个讨论步骤仍发送自己的目标和当前材料 ID，并仅采纳本轮 Turn 的产物。
4. Task 子流程等待异步 worktree/代码头及依赖就绪，不把正常等待当作失败派发。
5. Task 与 Feature 收尾的评审边界明确分开；保留错误边界导致的原退回，重新规划生成新 TaskGroup，而不改写 Review。
6. 接纳前发生网络超时保持 NotApplied/待核对事实，不假定本地或远端成功；明确重试沿用原候选及 lease。
7. 完成后的快照步骤引用已有权威最终快照，避免对已冻结 Task/Feature 发起可变保存。
8. 远端主干前进后增加显式快进同步意图，原候选失效；保留旧基线及全部回执，重新准备候选及确认。接纳前基线检查使用同次 GitHub 策略查询的精确 ref SHA，远端推送仍使用精确 lease。
9. 经验恢复导入有来源的历史上下文，新会话携带继承内容，不把源项目文档改成当前正式材料。

## 首版约束与未宣称能力

- 编译器 v1 是有限动作目录和五种规范职责 slot，Feature→Task 精确子流程；不是任意代码/BPMN/无限递归引擎。
- 原生映射为 Codex/OpenAI 和 DSH/DeepSeek，模型参数有限且显式校验；不支持任意 endpoint。
- 经验工具为当前 AgentID 专属的冻结只读文件目录，按 index/detail 渐进读取；不提供任意数据库接口。
- Scene 恢复是来源上下文及新 Session，不宣称重建旧模型内部状态、凭据或任意容器。
- 新能力本轮验证宿主为 macOS Apple Silicon；旧 Ubuntu 报告不作为 v0.3 复验结果。
- Archive 导出、冷数据删除/恢复、Swift 客户端和 Hugging Face 接入没有在本轮实现。

## 计划目录清理与新基线

已删除所有计划目录下 430 个非 Markdown 中间产物，共 7,940,686 字节，包括日志、JSON、截图、补丁及临时测试脚本。源码回归测试保留。详见 [清理记录](../ARTIFACT-CLEANUP.md)。后续默认测试证据使用临时目录，最终只保存 Markdown execution report。

已依据 v0.2 结构编写 [v0.3 架构与实现基线](../devdesk-v0.3/PLAN.md)，反映正式 Web 界面、多库架构、Workflow、完整 Agent/Model 组合、Scope/版本、实例经验、实际外部接入与能力边界；同时更新领域、ADR、协议与 Schema。

## 运行与交付说明

debug/release Host 已构建。验收只重启了本轮专用的 51752 Host；既有 4317 Host 没有被中断，它仍运行加载时的旧程序。重启该 Host 后才会加载新 Web 与迁移。当前源码回归和新能力原生验收宿主均为 macOS Apple Silicon。

数据可用性更新（2026-11-09）：用户随后明确要求清空本工作区全部 data/ 和数据库，相关旧本机验收数据已删除。本文记录当时的执行事实；其中运行 ID、Artifact 和快照不再代表可读取的当前本机数据。新初始化见 ../builtin-defaults-and-roles/EXECUTION.md。
