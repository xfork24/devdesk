# 运行时权限

Profile 是文件、网络、工具、审批与平台操作权限的唯一规则来源。Agent/Capability 表示具备的完整能力；实际调用仍须满足选定 Profile 与可信执行边界。Role、任务文字或工具需求不能扩大权限。后端资产与平台矩阵见 [权限后端](../../permissions/README.md)。

## 选择与默认绑定

每次执行采用一个 Harness Profile 和一个 Sandbox Profile。项目选择 Sandbox，可直接选择各层 Profile；Workflow 步骤的 `harness_permission_ref`、`sandbox_permission_ref` 可进一步指定确切修订。显式选择替换同层默认选择；没有显式选择时才解析该目标修订的活动默认绑定。

默认绑定与 Profile 都可版本化管理，绑定含确切目标、Profile 及执行合同，CAS 更新活动分配。直接选 Profile 无需先建立默认绑定，但仍检查目标类型、作用域和真实后端支持范围。两种选择都会冻结实际目标、Profile 与内容身份，不能绕过后端兼容性。

不存在独立执行授权配置。Workflow 步骤只保存 Profile 引用，不保存另一套可写目录或工具规则。派生 StepAccess、ExecutionScope 和 PermissionLock 是执行视图及事实，不是第二份可编辑源。

## Profile 字段与两层执行

Profile schema v2 要求 Harness policy 包含 filesystem、network、tools、approval；Sandbox policy 只处理文件与网络，不接受工具与审批。文件模式为 full_access/read_only/scoped，网络为 enabled/disabled/allowlist，原生工具为 all_available/restricted，审批为 never/on_request。无法落实的组合返回 `permission_unsupported`，不静默放宽。

Harness 的 `policy.tool_refs` 限制宿主管理工具的确切源修订：省略表示允许 Capability 中的全部此类工具，显式空数组表示全部拒绝。原生工具的限制由 `policy.tools` 和实际驱动支持决定，不能用平台工具引用假装限制任意原生命令。所有引用进入有类型源图校验。

文件和网络访问同时满足 Harness 与 Sandbox 两层边界。规范路径如 `/work/tests` 由 Profile 表达，可信挂载解析器映射宿主项目路径；执行器只挂载实际输入，容器以普通用户运行，不授予宿主 HOME 或 Docker socket。显式 Harness Profile 不替换 Sandbox 边界。

默认工程步骤直接引用专用 Profile：tests 只可写 tests/；red_baseline 与 implementation 可写仓库而 tests 只读；documentation 和评审使用只读 Profile 及确切工具范围。未特别选择的步骤使用项目选择或目标默认绑定。TDD、独立评审和业务确认条件仍由 Workflow 表达。

## 平台操作与调度

Harness Profile 的 `policy.platform` 包含 allowed_operations、human_operations、waivable_requirements、allow_task_revision；省略时不授予平台控制权限。它约束经理、Agent 控制器与 Rust 驱动。共享目录写入另需 configuration.global.write。内核的人类决定保护独立存在，宽松 Profile 也不能代答人类或自行批准受保护计划。

调度预算不属于权限。并发、每轮动作、租约、技术重试与 Agent 唤醒预算位于 Workflow 的 scheduling，跟随工作流修订和 Standard 冻结。平台能力、Profile 权限、运行 epoch/租约与预算须同时满足。

## 冻结、恢复与切换

执行开始固定双层目标与 Profile。存储 codec 保存确切源引用及选择，不持久化第二份展开权限正文；恢复按原源重建，默认改绑不改变已冻结工作。同一 Agent 在不同步骤可以采用不同 Profile，锁按工作流和步骤保存。

讨论发送时检测当前选择变化；变化需静止校验、取得实际停止与写撤销证明、保存快照并创建后继 Session。原会话、输入和回执保持历史。持久化意图保证崩溃后核对再恢复，不能在未知执行仍可能写入时直接换绑。

## 工作台与预设

基础组件的 Permission 管理 Profile 创建、修订、导入导出、预设目录和历史；Harness/Sandbox 详情管理目标默认绑定与兼容性预检。实际范围预览从两层 Profile 自动推导，无需手填另一份执行范围。保存 Profile 或重新加载预设不会自动改绑已有目标。

预设优先使用发行包旁的 resources/configuration/permissions/profiles；无外部目录时从包内资源恢复到数据集的 packaged-permission-presets。自定义来源目录可持久化选择，旧代际与历史修订保留。

实现依据：[Profile 与选择](../../crates/domain/src/permission_profile.rs)、[默认步骤 Profile](../../resources/configuration/permissions/profiles)、[双层编译](../../crates/adapters/src/permission_profiles.rs)、[冻结 codec](../../crates/adapters/src/source_views.rs)、[权限切换](../../crates/core/src/permission_switch.rs)。
