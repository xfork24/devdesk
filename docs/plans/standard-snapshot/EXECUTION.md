# Standard 不可变规范快照实施与验收记录

日期：2026-11-11。状态：实施中。实施工作区 `/Users/hengliu/.codex/worktrees/standard-snapshot/devdesk`，分支 `codex/standard-snapshot`，起点 `b70cd10`。原 main 中的其他计划与未跟踪文件保留。

## 已实施链路

Standard v4 保存 Workflow 根引用，规范闭包只包含 Workflow、Delivery、Spec 及声明关系。根引用是解析锚点，已包含的子 Workflow 也可作为工作入口，不形成适用工作类型或独立规则白名单。源封套 Scope 只用于访问边界。

工作采用记录统一为 work_configuration；运行计划固定 Standard，子工作通过显式 work_binding 独立编译自己的配置。新旧计划、执行与产物分别保留依据。计划变更删除独立基线覆盖检查，沿原有预演、授权、静止等待与原子切换路径采用新快照。

Standard 页面提供只读规范、生成、比较与归档；Feature 与 Task 共享采用组件。归档不删除源与历史，规范相同不代表真实验收通过。

## 实施期间验证

按用户最新要求，完整测试和验收统一在合入 main 后执行。worktree 阶段仅进行了 Rust workspace 编译检查、Web 构建和关键 Standard 回归。

最小回归 3 项通过：规范源及关系冻结、旧形状与错误入口拒绝、同一 Feature 两个 Task 通过实际材料工具分别执行不同 Standard，并在切换及 SQLite 重启后保留旧计划、原始输入和产物。测试中的 Harness 是明确的离线端口夹具，不作为真实模型验收证据。

新增浏览器回归覆盖生成、只读、差异、归档、Task 采用与运行切换。真实工程入口为 tests/live/probe_standard_snapshots.py，使用独立数据集、真实 Codex 和专用 GitHub 测试仓库；实际结果在 main 统一验收后补充。

## 整合与验收

main 上并行的 Delivery 内容标准改造已进入当前整合基线。合并时必须保留 Delivery 必需 Spec、有效输出检查、真实 Diff、Test Doc/Test Report 和大产物读取链路，并同步源修订及权限引用。此处只记录真实完成的验证结果。
