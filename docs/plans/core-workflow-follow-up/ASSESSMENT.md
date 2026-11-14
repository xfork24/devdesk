# 旧提案对核心能力的实际价值评估

日期：2026-11-14。代码基线：`1669f86c1fc142f46853122c4b439ec5267553b4`。结论与实施安排见 [PLAN](PLAN.md)。本次只形成计划，没有修改实现。

## 评估依据与取舍

本评估覆盖 `review/plans` 的全部 14 个提案及其中两份执行记录。提案、旧评估和历史验收都是线索；实现是否存在以当前代码为准，是否值得继续做以核心功能和工程约束为准。

采用[工程能力对齐总体计划第二节](../engineering-capability-alignment/PLAN.md)的八项工程约束，以及本次用户明确的范围：**基本执行能力、工作流调度、任务分配和交付验收**。一个工作项必须能指出实际功能缺口，或直接验证受本次修改影响的核心规则。通用工程化、运维、账号、导出界面和生态扩张不因此进入计划。

旧总体计划中的“待实施”、旧提案中的“未实施”以及旧索引的依赖图均不作为现状结论。当前已存在配置变更集、独立项目经理工作台、Rust 驱动、可执行 Spec 和内容地址运行资源；它们无需重新建设。审查期间观察到现行 `docs/*.md` 有其他工作区改动，本次未改写这些文件，也未把文档更新当作代码证据。

## 全部提案的处置

| 原提案 | 当前代码事实与工程判断 | 本次处置 |
|---|---|---|
| [P1 流程提案](../../../review/plans/p1-controller-workflow-proposals/PLAN.md) | `configuration_changes.rs` 已有 Create/Revise/SelectProject/SelectWork，`manager_workspace.rs` 已向经理开放预演和应用；`WorkflowChangeOperation` 已有 ReplacePlan、携带完整 StepDefinition 的 UpsertStep。发布源修订和改变运行中实例已有不同用例。 | 核心目标已有实现。不另造 workflow_proposal、workflow.propose、UpsertSubflow 或“永远仅人类发布”规则；后者违背策略驱动授权。相关冻结、改派与接纳边界进入 C4 回归。 |
| [P2 配置合并](../../../review/plans/p2-configuration-layering/PLAN.md) | 项目未来默认选择与具体工作冻结选择承担不同职责；当前配置变更集已统一候选验证，UI 也已有采用默认和精确修订选择。没有证据证明合并两类记录能修复核心功能。 | 不做 work_configuration v5、移除项目命令、迁移 CLI、术语表或新向导。保留两种选择语义，C4 验证新默认不改旧执行。 |
| [P3 文档验收](../../../review/plans/p3-document-acceptance-hardening/PLAN.md) | `spec_checks.rs` 已有三类机器断言和证据定位；`spec_lint.rs` 已共用 lint/接纳。但机器断言和强制定位主要停留在新增示例 Spec，默认工程交付仍未采用；普通工作步骤还没有完整的 spec.lint 执行路径。 | **部分采纳为 C3**：补普通生产步骤自检，并让默认流程采用确有判定价值的现有检查。拒绝 Delivery.structure_checks 第二套 DSL、固定“21 类”覆盖指标和把结构通过等同质量正确。旧 baseline_rules/min_matches 已不适用于当前 Standard 快照模型，不恢复 RequiredNodes 子系统。 |
| [P4 可重现与保全](../../../review/plans/p4-reproducibility-baseline/PLAN.md) | 冻结计划/输入、真实测试与 Git 回执、运行环境事实和本地归档脚本已经存在。`tests/live/export_evidence.py` 及其入口回归覆盖 SQLite 一致备份、登记 Artifact 和文件摘要。环境指纹不能保证外部环境可重建或测试必然同结果。 | 不建旧库“尽力投影”、历史挂载、新导出产品或完整复现子系统。C4 复用现有证据工具核对真实执行，不扩建归档能力。 |
| [P5 身份审计](../../../review/plans/p5-operator-identity-audit/PLAN.md) | `InvocationContext` 已由可信入口创建，命令保存 command_context；WorkflowActor 已区分 Human/Controller/Driver/Segment。Human 尚未区分具体个人，这是当前单机产品边界，不等于核心调度缺失。 | 不做账号、口令、操作者切换、双人批准、审计页面或新导出。C4 只检查实际调度主体、授权及人工门禁，不能把 Agent 归属人推断为产物作者。 |
| [P6 预算吞吐](../../../review/plans/p6-budget-throughput/PLAN.md) | Controller 已在状态摘要不变时跳过 conversation.send，workspace-only 无用户输入也直接等待；foreach、根驱动、项目和全局额度已有机制。当前真正缺口是共同就绪判定与 foreach 派发门禁不一致，以及重试错误来源、次数口径不完整。 | **修正后采纳为 C1/C2**。不再实现无变化零模型调用；不把默认并发改 3，不强制三档预算、不提高默认超时、不复用步骤会话/容器。配置已有能力的可用性优先于默认提速。 |
| [P7 文档治理](../../../review/plans/p7-doc-version-governance/PLAN.md) | 五篇系统说明已存在；版本锚点已撤回，normalize-plan 已删除。旧方案保留了不再成立的依赖。 | 原建设目标无需重做，不恢复锚点/版本对照表/check-docs。实施时只同步实际改变的工作流与验收语义。 |
| [P8 前端工程](../../../review/plans/p8-frontend-engineering/PLAN.md) | Request 仍是 string/Promise<any>，若干大组件也仍存在；问题确实存在，但类型桥、全面拆分、lint/Vitest 是通用维护工作，不直接补本次核心能力。RPC Schema 也尚不是完整可枚举的请求/响应方法目录，不能直接生成后宣称全类型安全。 | 移出本计划。只允许 C1/C3 所需的等待原因、自检调用和结果展示局部调整，不做前端基础设施项目。 |
| [P9 默认权限](../../../review/plans/p9-least-privilege-defaults/PLAN.md) | 已有 v2 Profile、真实挂载和拒绝不可执行组合的预检；Never 下拒绝审批是正确策略，受限 OnRequest 的部分组合也有明确拒绝测试。Codex 凭据路径已有 0700/0600 创建逻辑，不能照搬“没有保护”的旧描述。 | 不切默认权限、不做 command_allowlist/glob、不加凭据巡检或通知中心。命令名/glob 不能限制解释器和子进程，不能当安全边界。C4 保留现有真实权限和人工确认门禁。 |
| [P10 生态扩展](../../../review/plans/p10-ecosystem-extension/PLAN.md) | ProviderFacts、model_provider 源和 DSH route patch 已配置化，新增 OpenAiCompatible 枚举没有必要。macOS helper 的自定义新账户前缀仍受限，但凭据存储命名空间不等于模型服务商原语；不能据此断言所有新服务商都需要改 Rust。平台处理器仍集中，但注册框架本身不增加执行能力。 | 不做新 provider、新 Harness、inventory/plugin 或全量处理器拆分。只补 C3 需要的现有 spec.lint 通路；新协议仍须实现 adapter，不伪装为改配置即可。 |
| [P11 可观测性](../../../review/plans/p11-observability/PLAN.md) | Host 仍主要使用 eprintln 和 250ms SSE 补读；这是维护/性能议题。核心已有 driver.waiting、execution.reason 和持久回执，可直接承载业务等待原因。 | 不建 tracing 全链、healthz、metrics 或 EventBus。C1/C2 的结构化阻塞/失败原因属于控制内核事实，应随核心修复完成，不另起监控系统。 |
| [P12 去硬编码](../../../review/plans/p12-contract-version-decoupling/PLAN.md) | 结构接受、源信封、运行资源选择和驱动能力已在后续实现；旧计划还混有已撤回的 Contract Registry/range/normalize 设计。测试中的默认产品合同与通用引擎测试也不能一概视为相同耦合。 | 不重复重构版本机制或整个大测试文件。C1–C3 的新回归用小型自建图验证通用规则；保留专门验证出厂业务合同的测试，不把 builtin/hash/文件行数清零作为验收。 |
| [P13 配置精简与 DSH 栈](../../../review/plans/p13-configuration-slimming-dsh-stack/PLAN.md) | 出厂目录精简、DSH 模型/Agent 栈已存在；运行库可保存多条历史修订。当前能力检查使用实际驱动事实，已不等于旧报告中的四处 core 厂商判断。 | 不重做清理、不删除运行库历史、不自动重绑默认角色。C3 若发布后继，只更新出厂选择并保留历史执行内容。 |
| [P14 DSH 工具桥](../../../review/plans/p14-dsh-tool-bridge/PLAN.md) | 当前 DSH 驱动没有 Codex 式动态工具往返，缺桥不能靠能力标记补出；后续通用能力模型并未实现这条桥。原跨仓库方案还涉及工具注册、取消、停止证明和真实验收。 | 维持挂起，不把“聚焦基本能力”解释为恢复这个已暂停的大型新后端项目。不改上游 SDK，不做 FIFO/socket 旁路；不支持的组合继续明确拒绝。 |

## 需要继续做的三项核心工作

### C1 就绪判定与 foreach 并发门禁不一致

代码位置：

- [orchestration.rs](../../../crates/core/src/orchestration.rs)：`workflow.inspect` 在约 1140 行生成 dispatchable，没有纳入父 foreach 槽位条件；`workflow.step.dispatch` 在约 778–795 行却会检查该条件。
- [workflow_driver.rs](../../../crates/core/src/workflow_driver.rs)：约 435 行按子实例 ID 排序，约 508 行选 dispatchable，约 573–589 行在选中动作失败后结束该根的本轮处理。

本次用临时 crate 引用当前三个 crate，复用测试端口和真实 SQLite，只调用生产命令，构造了最小复现。没有模型、Docker、网络或真实工程执行：

1. 父流程 foreach 包含 A/B 两个子流程，集合并发上限为 1；每个子流程有两个依赖相连的 Control 步骤。
2. 人工先派发 ID 排序靠后的子流程第一步，使其进入 awaiting_acceptance；另一子流程保持 queued。这是合法的人工选择，随后切换 Rust 驱动也合法。
3. 查询 queued 子流程，得到 `dispatchable=true`；实际派发得到 `blocked: 子流程并发已满`。
4. 选择 Rust 驱动，连续运行 8 次 drive/evaluate；排序靠后的合法结果始终未被接纳，driver.reason 持续为子流程并发已满。

这是**已动态复现的核心推进缺陷**，而非性能猜测。即使默认并发不变，也会影响人工转自动后的任务推进。临时探针的成功退出表示断言到了当前错误行为，不表示流程通过了验收。

修复应统一共同内核的动作资格，保留提交时的事务内校验；调度器不能被一个暂时不可执行的候选饿死其他合法动作。不能放松 foreach 门禁、强行接纳或依赖 ID 恰好排序。

### C2 技术失败来源和重试计数没有形成完整原语

代码位置：

- [workflow_driver.rs](../../../crates/core/src/workflow_driver.rs) 约 484–493 行只通过 tool_call_id 获取 error.code，并用 execution.sequence 比较策略 max_retries。
- [configured_runtime.rs](../../../crates/core/src/configured_runtime.rs) 约 1628 行补充输入会增加同一个 sequence，约 1672 行又用它比较步骤 retry_limit；Agent 建会话/派发失败主要落为 reason 文本。
- [workflow_driver.rs 领域策略](../../../crates/domain/src/workflow_driver.rs) 已公开 retry_error_codes/max_retries；[默认策略](../../../resources/configuration/orchestration-policies/builtin-balanced-v1.json) 则明确关闭自动重试。

因此“有可配置重试”还不等于各执行方式都能按可信技术错误重试，补充输入轮次也可能消耗名义上的重试次数。这里是静态确认的表达和消费边界，尚未进行完整故障注入；实施先写行为回归，再决定最小的失败事实与计数表达。不能从中文 reason 或模型报告猜 retryable，也不能为了通过测试打开默认重试。

### C3 已有 Spec 原语尚未完整服务于默认交付验收

代码位置：

- [spec_checks.rs](../../../crates/domain/src/spec_checks.rs) 已提供 TextLength、RequiredSections、RequiredPointers 和精确 EvidenceLocation；[spec_lint.rs](../../../crates/core/src/spec_lint.rs) 共用机器检查；输出接纳已调用 enforce_delivery_value。
- [默认 PRD Spec](../../../resources/configuration/specs/builtin-spec-stage-prd-v1.json)、[设计 Spec](../../../resources/configuration/specs/builtin-spec-stage-architecture-v1.json) 等仍只有语义检查；带 assertion/evidence_required 的是 checked-markdown、located-review 示例资源。出厂共有 22 类 Delivery，但数量本身不是验收目标。
- [manager_workspace.rs](../../../crates/core/src/manager_workspace.rs) 有 spec.lint 分支，[platform_tools.rs](../../../crates/core/src/platform_tools.rs) 把它登记为平台工具，却未在普通 poll_platform_tool 中实现该分支，后者会落入 unsupported。默认也只有经理 Capability 挂载该工具。
- [standards.rs](../../../crates/domain/src/standards.rs) 当前 Standard 是精确 Workflow 源闭包的不可变快照，已没有旧 baseline_rules 数据模型。

因此应完成普通产出步骤的受控自检，并把现有原语用于实际默认文档/评审，而不是再建设检查框架。机器可检查结构和引用位置；是否答对问题、设计是否合理、实现是否满足需求，仍由配置规定的独立评审及真实测试/接纳决定。

## 本次验证边界

- `check-defaults.sh`：通过，出厂目录与精确引用自洽。
- `check-architecture.sh`：通过，现有分层门禁成立。
- 上述 C1 临时探针：确认查询/派发冲突与连续 8 tick 无推进；没有修复实现或把探针写入仓库。
- 未运行本轮全量 Rust/浏览器/真实 Feature–Task 工程。历史验收报告不代替 C1–C3 实施后的回归。

原提案中有价值但偏离用户指定核心范围的事项，本次均不排期；它们也不成为 C1–C3 的前置条件。
