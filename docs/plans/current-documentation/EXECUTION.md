# 实施与验收记录

日期：2026-11-12。结果：W01–W07、W12 完成；**W08–W11（版本锚点提取器、check-docs 校验器、version-baseline 生成物、check.sh 挂接）按用户决定撤回，不实施**——版本相关内容后续将从代码中移除，文档只负责梳理总结当前系统状况，不做硬编码版本比对工具。撤回前曾实现并通过注入实验验证，现已全部删除。

## 交付清单

### 新增文档（5 篇，共 259 行）

| 文档 | 行数 | 覆盖 |
|---|---|---|
| `docs/workflow-orchestration.md` | 69 | 定义与发布、三种推进模式、步骤生命周期与产物接纳、四层批准语义、运行中计划修改、TDD 证据链与 ActionPermit、默认流程概览、预算与并发 |
| `docs/permissions.md` | 48 | 五层模型、Profile 职责矩阵与编译、step_grants 授权叠加、执行冻结与后继会话切换、默认绑定与收紧方向、Controller 操作策略 |
| `docs/storage.md` | 57 | 数据集结构、三库分工、source_definitions 唯一存储、事实表族、幂等与恢复、模板目录代次、升级策略 |
| `docs/web-workbench.md` | 43 | 初始化与连接内核、九大导航区、刷新模型、典型五步旅程、设置页导览 |
| `docs/conversations.md` | 42 | 会话范围与工作类型、正式会话边界、运行时交互、提案防陈旧、权限改绑检测、经验只读语义 |

### 漂移修复（PLAN §三清单全部 8 项）

- real-engineering.md：Workflow v5→v6、agent-task/v5→v6、global 15/46→16/47；现行合同引用改指 workflow-orchestration.md（历史合同降为存档链接）。
- real-adapter-contract.md：agent-task/v5→v6。
- run-binary.md：global 15/46→16/47。
- system-credentials.md：v1 旧预设名 `container-full-access` → v2 档案 `builtin-permission-harness-full-access`/`builtin-permission-sandbox-full-access` 并链接 permissions.md。
- tool-skill-integrations.md：导航描述更新为现行结构（基础组件六类 + 历史记录调用记录）。

### 索引

- 根 README 文档段重构：现行系统说明（15 篇入口）与历史计划/验收分开列示。

### --normalize-plan 修复（crates/host/src/main.rs，W12）

- 逻辑抽离为 `normalize_plan_document`：读取输入 schema，超过当前版本明确拒绝；≤ 当前版本则升级到当前 `WORKFLOW_SCHEMA_VERSION`，重新分配 node_id、重算 contract digest，并新增 `document.validate()` 整体校验（旧实现写死 5、无任何校验）。
- `--help` 补充用法行；新增 3 个单元测试（v5 升级 / 当前版本原位规范化 / 更高版本拒绝）。

## 验证

| 检查 | 结果 |
|---|---|
| `cargo test -p devdesk-host --locked` | 32 项通过（含新增 normalize_plan 3 项），0 失败 |
| `cargo fmt --all -- --check` | 通过 |
| 受管文档本地链接完整性（一次性内联校验） | 通过；修复 web-workbench 一处缺 `.md` 链接 |
| 撤回前注入实验（版本错值、死链脚本路径） | 两项均被当时的校验器检出后恢复；工具已删除，仅作过程记录 |

## 限制与待办

- 新读者走查（仅读 docs/ 根回答系统问题集）未执行，作为待办保留。
- 本轮未运行全量 `./scripts/check/check.sh`（Web 构建、workspace 全量测试、双档 Playwright）；提交前建议完整执行一次。
- 文档中的版本表述以撰写时代码为准（Workflow v6、agent-task/v6、orchestration-task/v2、global 16 / project 47、数据集标记 devdesk-source-v2-standard-v1）；版本相关内容从代码移除后，相关表述随下一次文档修订一并调整。
