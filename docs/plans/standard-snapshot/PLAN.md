# Standard 不可变规范快照改造方案

日期：2026-11-11。状态：实施中，完整回归与验收统一在合入 main 后执行。审查基线：`b70cd10`。范围：Standard 领域、源配置、工作采用关系、运行计划、Task 调用、差异查询、页面、Schema、默认资源与验收。

Standard 收敛为 Workflow、Delivery、Spec 及其声明关系的不可变快照。它回答“采用了哪套流程和验收定义，以及与另一套定义有何变化”，不再提供独立覆盖规则、规范注入或规则豁免。

Standard 只有一种领域类型，不按 Project、Feature、Task 分类，不存在 FeatureStandard、TaskStandard 或“Feature 专属 Standard”。Project、Feature、Task 是使用者，可以引用同一个 Standard，也可以显式采用不同 Standard。工作之间的包含关系不要求它们使用相同 Standard。

本方案承接[源配置与执行解析](../../source-configuration.md)的当前存储边界：只支持新数据集，保留不可变源修订和真实执行选择，按需重建派生视图。此次直接切换当前合同，不增加旧基线 Standard 的转换器或兼容执行路径；既有数据目录不自动删除或重建。

## 1 目标与领域边界

| 概念 | 改造后的职责 |
|---|---|
| Workflow | 声明阶段、步骤、任务、依赖、输入输出、子流程、完成条件与人工确认。 |
| Delivery | 声明交付格式、结构 Schema，以及所关联的确切 Spec 修订。 |
| Spec | 声明内容要求与检查项，是规范正文的唯一来源。 |
| Standard | 固定一个或多个 Workflow 入口及其可达的 Workflow、Delivery、Spec 修订和声明关系。 |
| 工作配置 | 记录某个使用者采用的 Standard、入口 Workflow、执行者绑定和授权选择。 |
| 运行计划及执行证据 | 记录何时采用什么配置、实际执行了什么、产生了什么结果及例外。 |

必须成立的边界：

1. Standard 不保存独立规范正文、步骤选择器、必需 Spec 列表或最低匹配数量。需要改变要求时，修订相应 Workflow、Delivery 或 Spec，再生成新快照。
2. Standard 的规范内容不含 `project_id`、`feature_id`、`task_id`、业务 owner 或适用工作类型。源库访问元数据与使用者记录分别保存，名称也不决定其适用对象。
3. Standard 的内容不因引用它的工作增加、删除、改名、运行或完成而变化。同一快照可重复使用。
4. Standard 不包含运行实例、材料正文、验收结果、成员 Task 清单、Agent 实例或当前项目默认值。
5. Standard 相同表示规范定义及声明版本相同，不证明执行完整或验收通过。检查是否通过，继续读取真实执行和评审证据。
6. Project 默认配置更新只影响之后显式采用它的工作。已有工作不追随最新 Standard，也不在重试、重启时重新读取默认值。

## 2 当前实现与改造入口

| 当前入口 | 当前行为 | 改造方向 |
|---|---|---|
| `crates/domain/src/workflow.rs` | `ConfiguredStandard` 为正文加 `baseline_rules`；规则通过 Role、Delivery、标签选择节点，并要求 Spec 和最低数量。 | 替换为只持有确切 Workflow 入口引用的快照源定义。 |
| `crates/core/src/configured_workflows.rs` | 创建和修订基线；项目配置、可用性和预览调用 `check_configured_baseline`。 | 分离规范快照解析与执行配置编译，核对入口和引用一致性。 |
| `ConfiguredWorkflowPlan.specification_snapshot` | 编译时生成逐步 Spec、输出与完成条件清单。 | 改为统一快照解析器的只读视图；补足 Workflow 输入、Delivery 自身 Spec、子流程和关系差异。 |
| `crates/core/src/workflow_changes.rs` | 根据 `feature_workflow_configuration` 检查原 Standard；失败时可要求 `$workflow/coverage` 豁免。 | 删除独立基线检查，变更后根据实际规范源引用采用新快照。 |
| `crates/core/src/configured_runtime.rs` | Feature 复制项目配置；子流程复制父编译闭包，`owner_id` 沿用父工作；`workflow.start` 不直接固定 Standard。 | 所有入口固定 Standard；业务工作引用与流程调用归属分开；Task 可独立选择。 |
| `crates/core/src/orchestration.rs` | `freeze_plan` 发布变更后的 Workflow 源修订，并冻结计划引用。 | 同时固定对应 Standard，保证实际 Workflow 与声明快照一致。 |
| `crates/adapters/src/source_store.rs` 与 `source_views.rs` | 存源修订和执行选择，展开 Plan、任务等按引用重建。 | 沿用该机制，保存快照源和实际采用引用，不存第二份规范正文或展开图。 |
| `web/src/components/WorkflowSpecifications.tsx` | Standard 编辑器维护选择器、必需 Spec 和最低步骤数。 | 删除 Standard 分支和相关提示，单独实现快照查看与比较页面。 |
| `AgentCatalog.tsx` 与 `ProjectManager.tsx` | 将 Standard 当通用可编辑资源和可选基线；项目页显示覆盖检查。 | 使用独立 Standard 视图；项目页选择或生成快照，再配置执行者与权限。 |
| Schema、Host、模板与原生工具 | 发布 Standard v3、Feature 配置 v3、当前工作流工具参数。 | 同步新类型、统一工作采用关系、调用选择和差异结果。 |

当前逐步清单只能作为实现基础，不能直接更名后视为完整 Standard。它依赖执行编译上下文，也没有表达全部规范关系。

## 3 Standard 源定义与不可变身份

沿用源协议 v2 的统一封套，以 `identity:revision` 引用某个不可变 Standard 修订。Standard 的 `data` 升级为 v4：

```json
{
  "kind": "standard",
  "identity": "standard-engineering",
  "revision": 1,
  "name": "工程规范第一版",
  "scope": { "kind": "global" },
  "data": {
    "schema_version": 4,
    "root_workflow_refs": [
      "workflow-delivery:1",
      "workflow-change:1"
    ]
  }
}
```

示例中的两个入口没有 Feature 或 Task 类型标记。使用者从快照中的 Workflow 选择本次采用的 `workflow_ref`；一个 Standard 可以服务多种工作，也可以只包含一个入口。

根引用只表示解析锚点，不构成工作类型或执行入口白名单。闭包内已经冻结的子 Workflow 也可以作为某个工作的入口，避免为相同规范另建快照。

`root_workflow_refs` 非空、去重并规范排序，只接受已发布的确切 Workflow 修订。子流程、Delivery 和 Spec 由解析器沿声明引用收集，用户不手工维护第二份集合或关联表。生成快照不自动采用各资源的“最新版本”，不替换 Workflow 已固定的任何引用。

只保存根引用仍能构成不可变快照，前提是完整依赖都是不可变的确切源修订，且按引用永久可恢复。发布时核验完整闭包；同一源引用不得对应不同正文。Markdown 文件内容在源修订发布时固定，恢复不能重新读取后来被改写的外部文件。

### 存储可见性与使用关系

源封套现有 `scope` 只用于源库的维护位置、依赖可见性和访问校验，不表示 Standard 属于哪种工作。允许 Global 或 Project 源可见边界，不允许 Feature 或 Task 源边界；页面不提供“Project Standard”“Feature Standard”“Task Standard”分类。

全局快照只能引用全局可见源；某项目可见的快照可引用全局或本项目源。引用项目私有源的快照不能因此让其他项目读取这些源。跨项目复用应发布可共享的源闭包，或显式导入对应源及来源关系。这是访问边界，不构成另一套 Standard 语义。

创建自某个 Feature 或 Task 的操作，只把该工作当前采用的 Workflow 源引用交给快照生成器；生成结果不带工作 ID。生成来源由调用事件记录，不写入规范内容。

### 修订与生命周期

Standard 发布即不可变，没有可变正文草稿。生成前可以调整入口选择和名称，但未生成前只是表单状态。更新现有系列时发布新修订，原修订继续可读、可引用。

同一命令重复提交返回同一结果；同一身份和修订携带不同内容时拒绝。对同一可见范围内完全相同的入口集合，默认复用已有快照；不同命名系列可以引用同一规范闭包，不强行建立跨库全局唯一约束。

归档只影响默认选择列表，不影响历史恢复。被工作、运行计划或其他源闭包引用的 Standard 及其依赖不得硬删除。依赖保留检查覆盖传递引用，不只检查直接引用。

## 4 快照闭包与关系解析

新增独立的规范解析器，例如 `resolve_standard`。它只读取源修订，不要求选择 Agent、模型、工具连接、Sandbox 或 Permission，也不产生执行意图。

| 纳入快照的内容 | 必须保留的关联 |
|---|---|
| 所有入口和递归可达的静态子 Workflow | 入口关系、调用节点、确切子流程修订、集合调用声明。 |
| Workflow 的完整业务源定义 | Stage、稳定节点、任务、依赖、输入来源、输出映射、条件、完成规则、确认和预算。 |
| Workflow 输入、Step 输出及可达端口的 Delivery | Workflow 或 Step 的具体端口到 Delivery 修订的关系。 |
| 端口结构 | 当前端口统一引用 Delivery，结构 Schema 由对应 Delivery 修订冻结；此次不新增内联端口类型。 |
| Delivery 的格式与内容 Schema | Delivery 的确切修订及其内容合同。 |
| Step 直接引用和 Delivery 引用的 Spec | 区分 Step 检查依据和交付内容要求，不把两条关系合并成一份 Spec 集合。 |
| Spec 正文和检查项 | 确切修订、正文、检查 key、名称与要求。 |

所有端口、子流程路径和节点关系都必须可定位。例如 Spec 集合相同，但从“设计交付”移到“实现交付”，必须报告关联变化；父流程其他内容不变、只修订静态子流程引用，也必须报告变化。

Workflow 中声明的 Role、Skill、Tool 引用保留在其原始业务定义里，因此修改这些引用会表现为 Workflow 定义变化；解析器不继续把 Role 正文、Capability、模型、Harness、Skill 或工具实现扩展成 Standard 的规范节点。它们的版本及实际绑定继续由执行配置和回执追溯。

解析使用原始 Workflow 源定义，不使用已注入权限、运行输入或节点状态的编译文档。外部权限分配变化不得改变 Standard。

### 摘要与确定性

解析结果在内存生成节点、关系和内容摘要。规范摘要采用带版本前缀的稳定编码，例如 `standard-snapshot/v1`，输入包含入口集合、节点种类、确切源引用、源业务内容及声明关系。

对象键、集合型引用和关系集合进行规范排序；步骤、检查项等具有顺序含义的数组保留顺序。编码算法及空值处理固定为合同，不能直接依赖任意容器遍历顺序或运行 Plan 的现成摘要。

Standard 自身的名称、身份、发布时间、存储位置和使用者不进入规范摘要。源封套的展示名称也不进入；Workflow 业务正文中的名称或说明变化仍作为定义变化展示，可标为文字调整。

确切源修订参与摘要。因此相同正文发布为不同源修订仍显示“引用版本变化”，并可以说明正文未变；不自行判定两个不同修订在业务上等价。

`snapshot_digest`、依赖图和逐步规范视图都是可重建查询结果，不作为新的持久化投影。进程内可按不可变 Standard 引用和解析算法版本做有界缓存。数据库保留的源引用、计划采用关系和真实回执足以重建历史。

### 引用与图校验

生成与导入时核验引用类型、修订存在性、可见性、完整闭包、无非法循环、端口和规则引用，以及现有深度和规模上限。源文件里的输入不能伪造已解析摘要或绕过领域验证。

结构合法性继续由 Workflow、Delivery、Spec 自身的校验器负责。移除 Standard 规则，不等于取消这些源定义的合法性校验，也不引入新的规范覆盖门禁。

发布 Spec 新修订不会改变原 Delivery 或 Workflow。采用新要求时应明确发布相关 Delivery 修订、更新 Workflow 的对应引用，再生成新 Standard；未被入口闭包引用的新版 Spec 不会自动进入快照。静态子流程升级同样要求发布父侧引用变化。

## 5 使用者与采用关系

工作配置保存 `standard_ref` 与所选入口 `workflow_ref`。后者必须属于该 Standard 冻结的 Workflow 闭包；固定子流程则通过已解析的调用路径使用闭包中的修订。不能独立选择一份不相关 Workflow，再附上某个 Standard 名称。

| 使用位置 | 默认选择与冻结时机 | 后续变更 |
|---|---|---|
| Project 默认配置 | 保存时选择已有 Standard，或由已发布 Workflow 生成快照；同时选择入口、Role→Agent 和权限。 | 发布新的项目配置修订，不批量修改已有工作。 |
| 新 Feature | 显式选择优先，否则采用创建时 Project 的确切默认配置。 | 运行前可修订采用选择；运行后走计划变更。 |
| 新 Task | 显式选择优先，否则固定父工作在创建或首次绑定时已采用的确切配置及合适入口。 | 允许与父工作、兄弟 Task 不同；已有运行走计划变更。 |
| 独立工作流启动 | 在 `workflow.start` 时传入或解析已冻结的采用选择。 | 重试、恢复、后继会话沿当时计划引用。 |

持久化并开始运行的配置必须有确切 `standard_ref`。表单可以暂不选择 Standard，保存时提供“从当前流程生成快照”；不能存成“有实际规范但没有快照”的长期状态。快照生成不改变既有 Workflow 行为。

Task 不能在每次派发时重新读取 Project 或父工作当前默认值。创建时尚无合法配置的工作可保留“待配置”状态，但派发必须拒绝；配置完整后才固定首次采用关系。

### 通用工作配置记录

保留 `project_configuration` 作为项目默认配置源。用通用 `work_configuration` 替代仅服务 Feature 的 `feature_workflow_configuration`，供 Feature、Task 的采用关系使用；其工作引用明确指向真实使用者。

该记录表达“谁采用了什么”，可以包含工作引用、来源配置引用、所选 Standard 与入口、初始计划引用及创建依据；不复制整个项目配置正文、展开规范图或源正文。执行者和权限选择按原有确切源引用保存。

活动计划是运行中采用依据的权威来源。`WorkflowPlanRevision` 必须记录 `standard_ref`、确切 `workflow_ref` 和来源工作配置引用；实例通过 `active_plan_revision_id` 定位它，避免再维护一份可独立更新的 Standard 状态。

StepExecution、材料和回执通过自己的计划引用追溯当时 Standard，不读取当前实例或父工作的最新引用。未受影响结果的复用保留原计划和原 Standard 来源。

### 示例

| 使用者 | 采用的 Standard | 结果 |
|---|---|---|
| Project 默认配置 | S1 | 作为之后新工作的默认选择。 |
| Feature A | S1 | 沿用创建时选择。 |
| Feature B | S2 | 显式采用另一套规范，无需创建“Feature Standard”。 |
| Feature A 下的 Task A1 | S1 | 使用 S1 中适合本次工作的入口。 |
| Feature A 下的 Task A2 | S3 | 使用 S3 的入口，校验与父调用的输入输出合同兼容。 |

Project 改为 S4 后，上述已固定引用保持不变。Task A2 采用 S3 不会改写 S1，也不会把 Feature A 和 Task A1 批量切到 S3。

## 6 Task 独立选择与父子流程合同

此项是本轮必做范围。当前默认 Task Workflow 在 Feature 的 `foreach` 子调用里运行，仅增加一个 Task 页面下拉框不能使实际执行采用另一套规范。

### 区分工作身份与调用身份

子流程实例记录自己的调用父执行，同时可以绑定真实业务工作引用。`owner_id` 不再一律从父实例复制；由平台创建 Task 的真实结果提供可校验的工作引用，运行时确认其项目及父工作关系。

集合成员使用稳定工作身份建立关联，不能以数组下标、标题、节点名称或任意 JSON 中碰巧存在的 `task_id` 推断归属。重新排序集合不能把另一个 Task 的 Standard 或执行结果绑过来。

### 对 Workflow 增加显式工作配置调用

保留四种执行器。为 `executor.workflow` 增加可选 `work_binding`，复用现有有界输入引用机制，从本次调用输入或集合成员中取得类型化工作引用。该机制可指向受支持的工作对象，执行引擎不按“Task”节点名称选择行为。

- 没有 `work_binding` 时，按 `workflow_ref` 的确切静态子流程执行，不允许通过工作默认配置暗中替换。
- 有 `work_binding` 时，从该工作的已固定 `work_configuration` 读取 Standard 和入口 Workflow，作为本次调用的执行选择；声明的 `workflow_ref` 提供默认入口和父侧接口依据。
- 工作尚未配置时，可用父工作已固定的 Standard 和声明入口完成首次绑定；如该 Workflow 不属于对应快照则明确报缺口，不回退到最新项目默认值。
- 每次实际调用固定工作配置引用、Standard、Workflow、父执行、集合成员稳定身份及输入摘要，形成真实调用事实。

默认工程 Workflow 在创建 Task 和展开 Task 调用之间补齐这些显式关系。同一调用的重复派发只关联原实例；Task 已在其他根流程或调用中活动时拒绝重复启动，不能自动迁移其父调用、Controller 或执行历史。

子工作按自己所选入口、执行者和权限引用独立编译闭包，不能继续复制父 Plan 后只替换 Workflow 文档；否则新 Standard 的 Spec、Delivery 和资源可能缺失或串用。编译缓存以实际采用引用和执行绑定为键。

调用祖先关系继续决定根 Controller、epoch、租约和控制范围，业务 `owner_id` 切到 Task 不改变这组控制事实。实际操作同时通过调用控制权限与子工作执行授权校验，不因为更换工作配置扩大原 Controller 的权限。

### 不同 Standard 的兼容校验

Task 可以采用不同 Standard，但父调用声明的必需输入、输出、Delivery 修订、材料来源和完成合同仍然有效。首版只接受确切端口合同兼容，不通过名称相同、Spec 相似或结构子集推断可替代。

内部步骤、Spec 和内部 Delivery 可以不同。若对外 Delivery 或父侧验收合同发生变化，必须先预演并修订父侧 Workflow，生成对应新 Standard，再执行；不能把同一产物重新标成父流程期望的类型。

选择另一 Standard 不自动提供新 Role 的 Agent、工具能力或写权限。缺少执行者、Permission Context 的确切步骤授权或资源访问时，分别报配置缺口。

父侧评审读取产物自身的 Delivery、Spec、计划和 Standard 来源。子工作按自己的标准通过，不等于父侧验收自动通过；父侧声明的检查仍需真实执行。

### 声明快照与实际调用树

Standard 冻结 Workflow 声明，包括静态子流程和允许按工作配置调用的机制；运行时某个 Task 的选择是调用事实，不把 Task ID 和实例边写入 Standard。

因此 Feature A 自身采用 S1、Task A2 采用 S3，可以同时成立。工作详情显示自身采用记录以及子工作的实际采用清单；不能只显示 S1 就宣称整棵调用树都使用 S1。

两个 Feature 的自身 Standard 相同，但其 Task 选择不同，工作对比应显示“自身规范相同，子工作采用不同”。不为整棵实际调用树自动创建“Feature 聚合 Standard”。

## 7 运行中变更与快照切换

沿用 `preview → apply → 等待受影响执行静止 → 原子切换`，把 Standard 的采用纳入同一变更，而非增加旁路修改接口。

### 两种规范变更

1. 采用已有 Standard：ChangeSet 增加 `AdoptStandard`，携带确切 `standard_ref` 与入口 `workflow_ref`。解析目标定义，计算节点、输入输出、执行者和权限缺口，再进行影响分析。
2. 修改当前规范定义：继续使用现有 Workflow 类型化变更。预演解析候选业务定义；应用时发布新的 Workflow 源修订，并生成对应 Standard 修订或复用相同快照。

若当前 Standard 包含多个入口，只替换本次修改对应的入口及其明确修订的静态依赖，保留其他入口。修改某一处子流程调用不自动升级其他调用或所有兄弟工作。

预演为只读操作，不发布 Workflow、Standard 或工作配置。候选引用根据候选内容稳定计算；应用时重新核对候选摘要、活动计划、确切源引用、授权和实际影响集合。

### 原子生效与历史

规范变更的成功提交同时完成新源修订登记、新 Standard 选择、新计划冻结及活动计划切换。需要等待执行静止时先记录待处理变更；不能提前把界面和实例切到尚未生效的 Standard。

`StepExecution` 的旧输入、合同、产物和回执保持原引用。受影响节点重新执行或等待明确接纳；未受影响节点继续按既有 carry-forward 规则复用，并保留原始来源。不能只凭新旧 Standard 摘要相同就认定执行结果可复用。

变更历史记录旧、新 Standard、旧、新计划、变化原因、实际生效位置和操作者。中断、重启和幂等重放依据这些确切引用恢复，不重新生成不同的候选身份。

已完成工作保持终态；进一步修改建立后继工作或按既有允许的返工路径办理。回退只在现有受限条件内恢复旧选择，追加回退事实，不删除已经发生的新计划和副作用记录。

### 不需要新 Standard 的情况

补充输入、技术重试、恢复 Session、替换兼容 Agent 绑定、权限绑定调整和仅修改实际代码或材料内容，均不改变 Workflow、Delivery、Spec 源引用时，保持原 Standard；仍按各自规则生成执行或计划记录。

修改 Workflow 中的任务、预算、依赖、端口、Spec 引用、检查规则或静态调用修订，则属于定义变化，需要新快照。判定由规范源闭包变化决定，不按“这是 Task 修改”或“这是 Feature 修改”决定。

### 覆盖规则与运行例外

删除 `check_configured_baseline`、`ConfiguredBaselineRule`、仅用于该机制的 `StepSelector`、`baseline_rules`、`baseline_check` 和 `$workflow/coverage` 基线豁免路径。

保留 Workflow 自身合法的 `CompletionRule::Covers`、明确的检查规则以及相应运行例外。不能全库删除 `coverage` 字符串，因为它同时可能表示业务完成条件。Permission 中的可豁免项按实际目标清理，不能连带移除仍被 Workflow 使用的能力。

跳过阶段或豁免检查若改写了 Workflow 源合同，应生成新 Standard，并展示删减的要求；仅记录运行例外而未改写定义时，保留原 Standard，同时显式显示未执行或被豁免项。两者都不能展示为原检查已通过。

## 8 差异比较与查询语义

### Standard 之间的比较

支持任意两个可访问 Standard 的比较，不要求来自同一个项目、同一种工作或同一个命名系列。

| 差异类别 | 例子 |
|---|---|
| 入口变化 | 增加或移除可选 Workflow 入口。 |
| 流程变化 | 增减步骤、依赖和子流程；任务、条件、确认或预算变化。 |
| 交付合同变化 | 格式、JSON Schema或端口绑定变化。 |
| 规范内容变化 | Spec 正文、检查项或检查要求变化。 |
| 关联变化 | 同一 Spec 改绑到另一个 Delivery 或 Step。 |
| 引用变化 | 源修订变更但业务正文相同。 |
| 文字变化 | Workflow 中的名称、说明等展示内容变化。 |

比较先按稳定源身份、节点身份、端口和调用路径配对，再展示字段差异；无法可靠配对时明确报告新增与移除，不按同名合并。共享子流程出现多次时，既展示源变化，也列出受影响调用位置。

摘要一致可快速判定规范一致；摘要不同则返回结构化变化，不仅给一条“版本不同”。第一版不自动判断“更严格”“更宽松”或“新旧等价”，避免把结构差异包装成业务结论。

### 工作之间的比较

工作对比接受两个真实工作引用，读取各自选定的计划或当前活动计划，不读取最新项目默认配置。返回：

- 双方自身采用的 Standard、入口 Workflow 和计划时点；同一 Standard 的不同入口也应显示使用差异。
- 完整 Standard 的规范差异，以及按双方实际入口投影的相关差异，避免无关入口掩盖本次工作变化。
- 子工作和实际调用采用的 Standard 分布；分别标记规范变化与单纯成员、材料或执行状态变化。
- 历史切换记录和例外摘要。验收结果作为证据关联，不能混入 Standard 内容摘要。

对历史执行允许指定确切 `plan_revision_id`。调用树查询分页、有规模上限，重复子调用按稳定身份表达，不在浏览器一次展开所有历史。

### 查询结果与错误

快照查询返回确切引用、摘要算法版本、入口、分页节点和关系、使用位置索引。查询图是按需视图，不作为可提交的源数据。

结构化诊断至少区分：`invalid_standard`、`missing_source_revision`、`source_scope_violation`、`workflow_not_in_standard`、`work_configuration_required`、`invocation_contract_mismatch`、`stale_plan` 和 `execution_configuration_required`。界面不通过解析错误文案决定行为。

## 9 页面与操作流程

### Standard 列表

保留“流程与规范 → Standard”入口。列表显示名称、修订、入口 Workflow、摘要简写、生成时间和使用数量，支持搜索、历史修订、归档与两个快照的比较。不按 Project、Feature、Task 建立 Standard 类型筛选。

生成时间来自发布事件或源登记元数据；不能把当前加载时间、缺省零时间当作真实生成时间。来源不可用时可省略该列。

### 生成快照

主要操作为“从 Workflow 生成快照”：选择一个或多个已发布 Workflow 入口，系统解析完整规范闭包，填写名称后生成。预览只展示入口、节点数量、规范和关系摘要，不编辑解析结果。

从项目、工作详情或 Workflow 页进入时可以预填确切入口；操作结果仍为普通 Standard。草稿 Workflow 必须先明确发布，再生成快照，不偷偷把未发布修改混入历史版本。

### 快照详情

默认只读，展示概览和流程树；逐步查看任务、输入输出、Delivery、Spec 和检查项。Spec 正文只读渲染 Markdown 并链接到对应源修订，避免在 Standard 下维护副本。

详情提供“比较”“查看使用位置”“基于这些入口生成新修订”和跳转源定义。修订 Workflow、Delivery、Spec 的操作回到其原页面；生成新的快照不会改写已有快照。

移除 Standard 正文编辑、Role/Delivery/标签选择器、必需 Spec、多寡阈值和添加/删除覆盖规则。Standard 不再复用通用 `ResourceEditor` 的正文表单。

### Project 配置

项目页选择 Standard 后，从其 Workflow 闭包选择本次入口；再配置 Role→Agent、编排者和权限。已有 Workflow 操作习惯可保留“选择流程后生成快照”入口，两条路径最终形成同一种配置。

显示规范概要和执行配置缺口，删除“可选覆盖基线”“覆盖检查通过”等状态。保存配置时未选快照可在同一提交中生成，保证保存后持有确切 Standard。更新后的提示明确只影响后续采用该配置的工作。

### Feature 与 Task 工作详情

使用相同的“采用的规范”组件，显示 Standard 名称、修订、入口、来源和切换历史。创建或开始前可显式选择；运行后操作为“预演规范变更”，展示影响后再按既有变更机制应用。

Task 明确显示其选择来自父工作还是显式采用；兄弟 Task 可以不同。父工作显示子工作的实际采用清单，能够进入任意两项比较。页面不使用“Feature Standard”或“Task Standard”作为对象名。

工作台将“规范定义一致性”“执行配置完整性”和“实际验收结果”分别表达，避免用一个绿色 Standard 状态替代真实检查结果。

## 10 API 与发布合同

| 接口 | 目标行为 |
|---|---|
| `standard.capture.preview` | 解析入口并返回候选摘要、闭包概览和诊断；只读。 |
| `standard.capture` | 核对候选、发布或复用不可变快照；支持幂等和系列修订。 |
| `query.standard` | 读取指定修订及按需规范视图，不回退到最新版本。 |
| `query.standard_diff` | 返回两个快照的完整或按入口筛选的结构化差异。 |
| `query.standard_usage` | 返回真实采用引用和可访问的使用位置，支持分页。 |
| `query.work_standard_diff` | 比较工作自身、指定历史计划及实际子工作采用关系。 |
| `project.configuration.update` | 保存 Standard、入口和执行配置；支持显式从入口生成快照的同事务路径。 |
| `work.configuration.update` | 运行前为 Feature、Task 保存显式采用选择；对已有活动执行要求走 ChangeSet。 |
| `workflow.start` | 接受确切工作配置引用或明确采用参数，固定 Standard 和入口。 |
| `workflow.change.preview/apply` | 支持 `AdoptStandard` 和规范修改后的候选快照；复用原有授权、CAS 和静止等待。 |

移除 `standard.create/revise` 接收任意 `data` 和 `body` 的通用编辑入口。源导入仍可接收合法 Standard v4，但必须经过同一个领域解析和校验器，不能绕过快照约束。

保留 `query.workflow_requirements`、`query.workflow_validation` 的流程与执行配置职责，移除 `baseline_check`。需要逐步规范视图时调用统一解析器，不保留第二套生成逻辑。

版本切换按内容合同处理：Standard v4、ProjectConfiguration v4、通用 WorkConfiguration v1；由于增加显式 `work_binding`，Workflow 升级为 v6，实例及相关 Schema 同步。Spec v3、Delivery、源封套 v2 保持各自现有合同。

Agent 和编排任务的来源若增加 Standard、工作与调用引用，分别升级对应 renderer 版本；同时更新存储 codec 的识别和重建规则。不能新增字段却沿用旧任务版本假装合同未变。

原生 `workflow.start`、`workflow.change.*` 工具、Host 分发、Controller 白名单和查询返回同步更新。Standard 查询不授予发布或工作变更权限；`standard.capture` 不因为存在编排会话而默认向其开放。

## 11 默认资源与数据切换

删除内建 Standard 的正文与 `baseline_rules`，由当前默认工程 Workflow 入口生成普通快照。默认资源只维护根引用，目录清单和 `defaults.json` 选择确切 `standard_ref`。默认两个入口服务现有工作类型，不给 Standard 添加工作类型字段。

规范正文继续只保存在 Spec。当前默认 Standard 要求的 Spec 落点应逐项核对是否已经由 Workflow、Delivery 显式声明；缺失的实际工程要求在相应源定义中明确表达，不把旧规则搬进另一个隐藏策略字段。

发布新的 Workflow 源修订以表达工作配置调用；默认 Permission Context 的确切 Workflow/Step 授权同步重绑，核对授权范围和工作空间边界，不能因换修订丢失授权或扩大权限。

数据库继续使用 `source_definitions` 保存 Standard 源，不新增规范图、覆盖判定或展开快照表。工作采用和调用选择属于执行事实，按现有运行存储方式追加；使用查询按需或通过可重建索引完成。

实施时核对并递增 global/project 数据基线，更新数据集兼容标记和浏览器草稿命名空间。只接受新合同数据集，不将旧 `baseline_rules` 重新解释为新快照。遇到旧库明确拒绝并保留原文件；不自动清空用户数据。新库、重启和模板重置都必须固定相同源引用和快照。

旧计划和验收报告保留其历史结论；当前 `docs/domain.md`、`docs/design.md`、`docs/protocol.md`、`docs/source-configuration.md`、Schema 索引、README 和资源说明统一更新。

## 12 实施文件与职责

| 范围 | 主要入口与任务 |
|---|---|
| 领域 | `crates/domain/src/workflow.rs`、`source.rs`、`workflow_change.rs`；新增独立快照类型和确定性关系解析，移除基线类型，增加工作配置调用声明。 |
| 规范服务 | 新增 `crates/core/src/standards.rs` 或同职责模块，集中生成、验证、摘要、差异与采用查询；不依赖完整执行编译器。 |
| 配置与模板 | `configured_workflows.rs`、`governance.rs`、`templates.rs`；统一 Standard 发布、引用和默认配置。 |
| 运行与编排 | `configured_runtime.rs`、`orchestration.rs`、`workflow_changes.rs`、`controllers.rs`；所有启动、子调用、计划和变更固定采用依据。 |
| 存储与恢复 | `crates/adapters/src/source_store.rs`、`source_views.rs`、SQLite 基线与数据集标记；保存引用和事实，重启一致恢复。 |
| Host 与工具 | `crates/host/src/governance.rs`、模板/源导入入口、平台工具合同；处理 capture、查询、工作配置和实际调用选择。 |
| Web | 新增 `StandardManager`、`StandardDetail`、`StandardDiff` 和共享采用组件；改造 `WorkflowSpecifications`、`AgentCatalog`、`ProjectManager`、Workflow 编辑器与实例工作台。 |
| Schema | `standard.schema.json`、`workflow.schema.json`、`rpc.schema.json`、`orchestration.schema.json`、项目配置；用通用工作配置 Schema 替代 Feature 专用采用合同；任务来源与工具 Schema 同步。 |
| 默认资源 | `resources/configuration/standards/`、`workflows/`、`tools/`、`defaults.json`、`manifest.json` 和实际 Permission Context 资源。 |
| 回归 | `configuration_resources.rs`、`configured_workflows.rs`、`orchestration.rs`、`schema_contracts.rs`、Host 模板测试、HTTP/运行准备入口和 Web E2E。 |

删除旧实现前列出所有 `standard_ref`、`baseline_*`、`specification_snapshot` 和 `coverage` 消费点；以职责判断移除或重写，不用机械字符串替换代替领域收敛。

## 13 分阶段实施与完成条件

| 阶段 | 交付 | 完成条件 |
|---|---|---|
| P0 合同固定 | 领域/DTO、摘要编码、入口与调用规则、Schema、默认资源和全部消费点清单。 | 没有 Feature/Task Standard 类型；明确每种启动和变更如何确定 Standard；确定实际版本号和新数据标记。 |
| P1 快照领域 | Standard v4、纯源闭包解析、关系定位、摘要、生成与差异查询。 | 无 Agent/权限也能生成；关系变化可比较；错误引用和不可变冲突可诊断。 |
| P2 配置与持久化 | Project 默认、通用工作采用记录、计划引用、源存储及恢复、默认快照。 | 新工作必有确切采用依据；同一快照跨工作复用；重启不漂移且无重复正文。 |
| P3 Task 与子调用 | 真实工作绑定、显式 `work_binding`、逐成员选择和父子接口检查。 | 同一 Feature 下两个 Task 真正按不同 Standard 执行，且来源、权限、结果不串用。 |
| P4 运行变更 | AdoptStandard、候选快照、影响预演、静止等待、原子应用、历史与受限回退。 | 活动计划与 Standard 始终一致；旧执行可追溯；删除独立基线及其豁免。 |
| P5 页面 | 快照列表、生成、只读详情、差异；项目及工作共享采用入口。 | 无覆盖规则表单；用户能定位不同工作为何采用不同规范。 |
| P6 切换与验收 | 新数据基线、工具/Schema/文档、默认工程流程、独立发布包。 | 全部验收项通过，证据区分离线回归和真实工程结果；旧数据目录未被自动改写。 |

P3 与 P4 都属于本轮完成要求，不能以“先支持 Feature，Task 后续补充”或“只改页面”结项。阶段实现保持单一当前合同；不为分阶段开发留下旧基线执行器。

## 14 验收场景

### 规范与不可变性

1. 同一 Standard 同时被不同项目、Feature 和 Task 引用，内容不增加任何使用者字段，摘要保持一致。
2. 一份快照包含多个入口；不同使用者选择各自入口，不产生 FeatureStandard 或 TaskStandard。
3. 同一 Workflow 身份的修订中，Delivery/Spec 引用、静态子流程或关联位置变化，必须生成不同快照并指出差异。相同 Spec 集合的重绑不能漏报，未采用的新版 Spec 不得自动进入旧快照。
4. 重复生成、重载和进程重启得到相同摘要；改变集合存储顺序不改变摘要，有业务含义的顺序变化可检测。
5. 未修改 Workflow 引用时，仅改变 Agent、模型、Permission 或工作材料不会改变 Standard。修改 Workflow 内的 Role/Skill/Tool 引用则报告对应 Workflow 变化。
6. 不同源修订但正文相同报告引用变化；只给 Standard 换名称或命名系列时，规范摘要可相同。
7. 缺失修订、引用错种类、循环、越界、不可见私有依赖和同引用异内容均被拒绝；源归档不破坏历史恢复。

### 采用与 Task 执行

8. Project 从 S1 更新到 S2，已有 Feature、Task、重试和后继 Session 仍使用已固定依据；新工作按显式选择或新默认采用。
9. 同一 Feature 的 Task A、Task B 分别采用 S1、S3，任务预览、实际 Harness 输入、检查项、材料和回执均对应各自修订。检查真实输入，不只检查数据库多了一个字段。
10. Task 创建后到派发前父工作默认选择变化，不改变该 Task 已固定的配置；集合成员重新排序不交换配置和结果。
11. 固定子流程不受外部工作默认选择影响；显式工作配置调用能采用另一 Standard，并独立解析所需闭包。同一调用重复派发返回原实例，其他活动根实例不会被重复启动或改挂控制者。
12. 子工作内部规范不同而父侧端口合同一致时可以运行；Delivery 或必需端口不兼容时，在派发前拒绝并指出具体合同。
13. 目标 Standard 增加 Role、工具或步骤时，缺少 Agent/权限准确报错；选择 Standard 不产生额外授权。
14. 父侧评审按产物真实 Standard 和 Spec 来源读取，仍执行父侧声明的检查；同一父工作可以如实展示多个子工作采用的不同快照。

### 变更与恢复

15. 预演不产生 Standard 或 Workflow 源；应用与活动计划切换一致，过期 preview、CAS 冲突和活动写入按现有机制拒绝或等待。
16. 运行中修改规范生成新快照，已有执行和产物仍引用旧计划；仅补充输入或重试保持原 Standard。复用结果可追溯到原标准，不重标历史。
17. 应用前、等待静止、原子提交和提交后分别中断重启，最终只产生一条有效采用链，没有实例指向尚未发布快照或丢失闭包。
18. 删除原 Standard 覆盖门禁后，无 `$workflow/coverage` 基线豁免；Workflow 的实际完成覆盖条件、人工确认和允许的业务例外仍正确工作。
19. 跳过或豁免检查明确显示未验证项；相同 Standard 不被呈现为所有检查已通过。已完成工作和受限回退继续遵守终态与外部事实约束。

### 页面与交付

20. Standard 页支持生成、只读展开、历史和比较；不存在正文/覆盖规则编辑器。Spec、Delivery、Workflow 仍是要求的唯一编辑入口。
21. Feature 与 Task 共用采用组件；工作对比区分自身定义、入口差异、子工作采用和验收事实，不凭一个 Standard ID 合并判断。
22. 所有新 Schema 与 Serde、资源和原生工具合同一致；错误版本明确拒绝；浏览器旧草稿不污染新表单。
23. SQLite 检查只发现源修订、采用引用和真实事实，没有展开规范图、复制 Spec 正文或覆盖判定表；独立包重启可完整恢复。
24. 默认工程流程的任务、TDD、真实测试、独立评审、人工确认策略和 Git 接纳逐项复验；新增不同 Task Standard 场景不能削弱真实来源、候选版本或权限校验。

## 15 验证与交付要求

领域和解析测试覆盖摘要稳定性、完整引用图、差异定位和非法输入；集成测试覆盖工作选择、调用兼容、计划变更和持久化恢复。Web E2E 覆盖生成、只读详情、比较、项目默认和两个 Task 的独立采用。按实际修改运行 Rust workspace、Schema、Web 构建、HTTP、debug/release 浏览器及独立发布包检查。

真实工程验收至少完成一次同一 Feature 下两个 Task 采用不同 Standard 的流程，并记录实际任务内容、所用检查、真实测试/Git 回执和父侧接纳依据。若某平台或 Harness 不能执行该链路，在执行报告中明确其实际边界，不把模拟端口或仅字段断言算作真实闭环。

计划目录只保存 Markdown 合同、执行记录与验收结论，日志、JSON、截图和临时数据集留在独立验收目录。方案阶段不修改运行数据、不提交代码，也不把尚未实施的检查写成通过。
