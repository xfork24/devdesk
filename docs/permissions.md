# 运行时权限

本文描述权限的**系统语义**：策略如何表达、授权如何叠加到步骤、执行如何冻结、切换为何等价于后继会话。资产清单与后端矩阵见 [权限说明](../permissions/README.md)，命令合同见 [本地协议](protocol.md)。

## 五层模型

```text
PermissionProfile（不可变源修订，schema v2：文件/网络/工具/审批策略）
  → PermissionTarget（Harness / Sandbox 的确切目标修订，含配方摘要与执行合同）
    → VersionedPermissionBinding + Assignment（按目标修订 CAS 的活动绑定）
      → PermissionLock（执行开始冻结的双端锁）
PermissionContext（源修订：sandbox_ref + 双端选择 + step_grants + Controller 控制策略）
```

核心原则：**Agent 与 Capability 不含任何权限字段**。技术能力（Capability 声明具备某工具）与本次调用授权（Permission 授予）分别表达；Role 名称、任务指令或提示词都不能扩大权限。

## 权限预设来源

默认预设优先使用运行包旁的 `resources/configuration/permissions/profiles`；没有附带该目录时，从包内资源按内容摘要生成到当前数据集的 `packaged-permission-presets/`，不读取编译时的源码目录。生成缓存可恢复，旧内容代际保留；修改预设应选择自己的目录，避免把生成缓存当作编辑源。

用户选择的预设目录持久化保存，重新加载只读取该目录；“重新加载内置预设”恢复运行包提供的来源。加载产生不可变 Profile 修订，不自动改绑 Harness 或 Sandbox，也不改变历史执行的冻结权限。

## Profile 职责矩阵

Profile schema v2 强制：Harness profile 必须含 filesystem / network / tools / approval 四键；Sandbox profile 不得含 tools 与 approval（审批语义只属于 Harness 层）。可移植词汇为：

- filesystem：full_access / read_only / scoped（≤128 条规范绝对路径）；
- network：enabled / disabled / allowlist（1–128 个域名目标）；
- tools：all_available / restricted（+deny ≤256）；
- approval：never / on_request。

可移植策略由 `permission-compiler/v2` 编译为固定后端合同（Codex 配置、DSH profile 与工具边界、Docker 旗标与 seccomp/AppArmor 资产）；后端资产按 sha256 钉死并与 manifest 比对防漂移。无法落实的策略组合在编译期明确拒绝，不静默放宽。

## 授权叠加：step_grants

存储态 Workflow 只描述任务上下文（工作空间输入、工作目录）；读写目录与工具授权全部放在 Permission Context 的 `step_grants`：按 `workflow_ref + step key` 精确声明可写目录、只读目录与工具修订。加载执行视图时由同一解析器叠加到匹配步骤——授权是"目录 ∩ 工具"的交集，只能收窄。

默认工程上下文（`builtin-permission-context-engineering:3`）的示例：Task 的 `tests` 步骤只可写 `tests/`；`red_baseline` 与 `implementation` 可写全仓但 tests 只读（保护独立断言不被实现改写）；`documentation` 全仓只读；评审步骤无文件授权，只携带 outputs-read 工具。

## 执行冻结与切换

运行开始时解析双端（Harness/Sandbox）锁并冻结进 RuntimeBinding；**重试与恢复不追随新的活动分配**——绑定变更不改变已冻结执行。

权限切换等价于**后继会话**，三段式完成：静止校验 → 逐执行取得停止与写撤销证明（首停时导出协议日志）→ 会话快照 + 强制新建后继 Session（继承 Agent 实例）。全过程 durable intent，任意崩溃点重试收敛；原会话与输入保留历史，不被改写。

讨论会话发送消息时自动检测改绑：锁未变直接执行；变化则走上述切换。`permission_status` 状态机为 unconfigured / pending / ready_to_switch / adopted / needs_reconciliation，待恢复时可用 pending_command 原样重试。

## 默认绑定与收紧方向

默认配置绑定 `builtin-permission-harness-full-access` 与 `builtin-permission-sandbox-full-access`，把整个容器视为信任范围（宿主凭据库、其他项目、未挂载路径仍在容器外；Agent 拿不到 Docker socket）。最小权限的精细化由 step_grants 表达；需要收紧 Harness 层时可发布 read_only 或 scoped profile 并经 preflight 预检后应用绑定（`permission.bindings.apply`，同作用域原子批次、CAS 更新、可回退）。

## Controller 操作策略

Permission Context 内嵌 WorkflowControlPolicy：允许的编排操作集合、必须留给人类的操作、可豁免的要求类别（完成/确认/测试/独立性/覆盖）、是否允许修订任务、唤醒预算（轮次/每轮工具调用/超时）。Controller 的每个控制工具调用都经过策略、epoch、租约与预算围栏；留给人类的操作必须持有逐参数匹配的人工决定。
