# 对 review/plans 的独立评估

本评估基于当前代码及用户四项要求。`review/plans` 是设计参考，不能作为新权限、固定产品策略或实施完成的事实源。它的索引仍混有已撤回的版本锚点依赖、已回滚的能力门禁与旧路径，实施必须重新核实。

| 原计划 | 合理部分 | 修正、不采纳或暂缓 | 本路线落点 |
|---|---|---|---|
| [P1](../../../review/plans/p1-controller-workflow-proposals/PLAN.md) 流程提案 | 经理创作配置、预演编译、内容摘要、发布与应用分离、留下来源 | 范围仅 Workflow，覆盖不了用户要求；“发布永远仅人类”没有从用户要求推出，授权内可自动发布；ReplacePlan/UpsertStep 已存在，不能先认定必须增加 UpsertSubflow。流程修订、采用和运行变更也不能无条件在同一跨库事务中完成 | S2 通用变更集、S5 策略驱动创作 |
| [P2](../../../review/plans/p2-configuration-layering/PLAN.md) 配置分层 | 降低配置成本、统一解析/冻结、共用表单与错误 | 不先决定抹平 Project 默认选择与具体 Work 冻结选择的区别；不新增 `schema v5` 门禁；不以“离线转换”字面绕过迁移职责；术语表不必变成 domain 核心依赖 | S2 先统一语义和服务，S5 任务式 UI |
| [P3](../../../review/plans/p3-document-acceptance-hardening/PLAN.md) 文档验收 | 机器可判定结构由代码检查，预览/提交共享检查器，证据可定位 | 章节标题、AC 正则、默认 21 类数量属于配置；不全部塞进 Rust。结构成立不等于语义正确。优先复用 Spec，避免 Delivery 与 Spec 各建一套检查 DSL；正则用有界引擎/输入限制，不随意承诺同步检查可超时中断 | S6 |
| [P4](../../../review/plans/p4-reproducibility-baseline/PLAN.md) 可重现性 | 区分命令幂等、环境重建、证据复核和模型输出；导出确切引用与摘要有用 | “表在就读、缺列留空”的旧库尽力投影会掩盖证据缺失；新历史兼容子系统不属于当前主线。环境/网络变化仍可使旧测试重跑不同，不能把环境 pin 当作结果保证 | S6 明确验收和来源清单；完整归档另立计划 |
| [P5](../../../review/plans/p5-operator-identity-audit/PLAN.md) 身份审计 | 命令记录发起主体，人工确认与 Controller 动作可区分，时间线有用 | 不能由客户端随意填写 operator/actor 就视为可信；“同一操作者提交后不能批准”不是本次要求，且以 Agent 所属人推断产物作者不充分。不默认添加账号/口令/双人审批系统 | S1 主体上下文、S4/S5 留痕 |
| [P6](../../../review/plans/p6-budget-throughput/PLAN.md) 预算吞吐 | 预算可配置、唤醒去重、无变化零模型调用、并发取消测试 | 并发固定改 3 没有来自需求或实测；预算不强制三档常量。同容器连续步骤须先证明冻结/权限/独立性，暂缓；不能以性能优化绕过新计划和会话身份 | S4 有界调度；preset 可后续配置演进 |
| [P7](../../../review/plans/p7-doc-version-governance/PLAN.md) 文档治理 | 现行文档补齐、历史计划与事实区分 | 大部分已实施，版本锚点已撤回。新路线不恢复版本对照表/强制版本一致脚本，也不依赖不存在的 check-docs | 各阶段同步现行文档 |
| [P8](../../../review/plans/p8-frontend-engineering/PLAN.md) 前端工程 | 类型化 API、局部组件拆分、受影响交互回归 | 全量前端迁移不是主线前置；不以文件行数目标代表架构完成。API 生成若采用需明确唯一源和完整性，不能留下大量 `any` 却声称类型安全 | S2/S5 相关界面局部完成 |
| [P9](../../../review/plans/p9-least-privilege-defaults/PLAN.md) 权限 | 需求与后端可执行权限求交，真实受限流程验收，拒绝虚假能力 | 默认权限整体切换不是本次授权的必选改造；program/args glob 不能充分限制通用 shell、解释器、脚本和子进程，不能宣称形成安全边界；不以 DSH 尚无的 tools/execute 作为既有能力 | S3 验证真实权限能力，S5 禁止配置自举提权 |
| [P10](../../../review/plans/p10-ecosystem-extension/PLAN.md) 生态扩展 | 按协议接服务商，平台处理器按用例拆分注册，新 Harness 协议需要实现 | 当前 ProviderFacts 已配置化，旧白名单分析需更新；inventory 只改善代码组织，不产生运行时动态扩展。已实现协议内的端点/模型应可配置，新协议仍需新 adapter；无需一次引入动态插件 | S1 工具目录、S3 协议/能力边界 |
| [P11](../../../review/plans/p11-observability/PLAN.md) 可观测性 | 关联 command/intent/execution、健康状态、事件通知加速 | 固定 P95 50ms、空闲 60s 零轮询等指标缺基线；不手写一套监控框架作为先决。SSE 通知不是事实源，仍靠持久化游标补读 | S4 最小诊断、S6 验收记录；完整指标另案 |
| [P12](../../../review/plans/p12-contract-version-decoupling/PLAN.md) 去版本硬编码 | 最终修正的“结构与规则识别”、测试/出厂配置解耦、Web 不复制版本门禁正确 | 文档里仍留 current/supported/range/normalize 的废弃代码和风险段，不可照搬；当前很多项已完成。将外部 runtime pin 和唯一 recipe 摘要一概认定永远合理，与本次 UI 选兼容运行时不符；“加新领域字段但不改 Rust”也不是合理验收 | S2 结构接受、S3 真正运行时解耦、S6 演进测试 |
| [P13](../../../review/plans/p13-configuration-slimming-dsh-stack/PLAN.md) 精简与 DSH 栈 | 出厂目录不堆未发布历史、模型栈补齐、如实声明 DSH 边界 | 已完成的资源精简保留；不能把“发布包每身份一条”推成运行库只能一条修订。仅把常量集中不等于运行时可选；能力门禁回滚是历史决定，应由新能力模型解决 | S2 区分出厂目录/运行库，S3 |
| [P14](../../../review/plans/p14-dsh-tool-bridge/PLAN.md) DSH 桥 | 明确工具注册、请求/应答、取消恢复缺口；不能配置伪造工具桥；如重启优先单一协议通道 | 当前挂起决定继续生效，不自动跨仓库改 SDK。桥接方案和 upstream 版本需实施时重新验证；FIFO/socket 旁路引入额外生命周期与审计责任，不作为快捷替代。挂起不妨碍先去除 core 厂商判断并保持能力拒绝 | S3 能力事实，DSH 真实支持独立计划 |

本路线重排优先级：**分层应用边界、配置图和原子创作先行，运行资源与双调度随后，经理全项目工具在共有用例稳定后开放，最终以规范与完整工程验收收口。**不沿用原索引的批次依赖和完成状态。
