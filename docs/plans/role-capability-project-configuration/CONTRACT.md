# 配置与工作流 v2 实施契约

日期：2026-11-09。对应 PLAN.md，实施中；以代码回归与 EXECUTION.md 记录实际完成范围。

## 身份与存储

沿用 governance_records 的 definition_id、不可变 revision ID、版本、内容摘要和幂等命令。新增 Capability 是独立定义。Role 增加 visibility={kind:global}|{kind:projects,project_ids:[...]}；未声明时从旧记录 Scope 解释。共享 Role 在全局目录保存一次，visibility 不改变工程材料的所有权。项目引用校验所有依赖的可见范围；Agent 的可见范围不能超出 Role/Capability。

## 创建与修订

capability.create/revise 使用通用配置 envelope，data={model_ref,harness_ref,tools,skills}。agent.create/revise 的新输入为 data={role_ref,capability_ref,permissions?,approval?}；服务端根据精确引用展开兼容的 AgentComposition 执行字段，并把 capability_ref 一起封存。新建/编辑 Role 只产生 Role 修订，不生成 Skill、Agent、实例或项目配置。历史 Agent 的有效指令与权限保持原状。

query.configurations 支持 capability，query.roles 保持历史兼容；新查询提供定义引用、可用资源、Workflow 所需角色、规范覆盖与审计快照。删除/停用检查在用定义及项目配置引用，保留不可变历史。所有写入使用 command_id 和 expected_revision。

## Workflow v2

schema_version=2。需要模型的 Step 使用 role_ref（稳定 Role 身份）与 spec_refs（精确 Spec 修订），不保存 Agent 引用。执行配方节点也使用显式 Role 身份。系统节点可以不需要 Role/Spec。Workflow 子流程调用引用确切 Workflow 修订；Task 集合迭代保留 tasks.foreach 语义。发布前递归校验引用、端口、作用域、循环和资源上限，最大深度 8，展开节点不超过 1000。

编译计划冻结所有子流程和引用。工作流调用节点以实际子流程的输入、输出和终态推进；暂停、重试、取消和恢复沿用持久化意图协议。v1 锁不重新编译为 v2。

## Spec、Standard、审计

Spec={body,applicable_actions:[{kind,phase?,delivery_kind?}],rules:[supported_checker]}。无 applicable_actions 的旧 Spec 由迁移时的用途确定适用位置。

Standard={body,baseline_rules:[{selector,required_spec_refs,min_matches}]} 是可选基线。每条规则至少匹配 min_matches 个节点，并要求所有匹配节点覆盖指定 Spec 修订。空匹配不得通过。基线不注入 Spec，改基线不改变 Step。人工 Markdown、人工 Review 和已有自动检查器分别显示。

审计快照展开所有调用路径，记录 Step、动作上下文、Role、Spec 修订、输出契约和检查方式。差异包括规范版本、落点、角色、动作和检查变化，不能只比较 Spec 集合。

## 项目与冻结

ProjectConfiguration v2={schema_version:2,workflow_ref,standard_ref:null|baseline_revision,role_agent_bindings:{role_identity:agent_revision},specification_snapshot,baseline_check}。角色需求包含所有子流程与执行配方。每个绑定的 Agent 必须属于同一 Role 身份，职责类型相同不足以证明可替代。

新项目登记后处于待配置状态，允许保存项目作用域 Role；未配置不得创建 Feature 或派发正式运行。配置校验完整后追加修订，Feature 创建时冻结精确闭包。项目默认更新不改写已有 Feature。仓库就绪、配置完整和凭据/运行环境就绪分别展示。

## 迁移与接口兼容

追加全局 006、项目 035 迁移，扩展配置类型唯一索引与转换记录，不修改旧 SQL。旧 Agent 四组件组合生成有稳定内容身份的 Capability，Agent 保留独立身份与实例履历。旧项目按 (WorkflowRevision,StandardRevision,已证明的 Role 身份映射) 派生等价显式 Spec 绑定，不能覆盖共享原流程。转换幂等且事务化；无法证明的配置标为待补，不能填入猜测值。

既有 default_bundle 记录仅供历史读取；新启动与新项目不创建或追随套件。参考配置作为普通条目保留。正常页面不调用 prepare_defaults 或隐式常量选择 Agent。

## 页面

项目管理、工作流程、角色与 Agent、规范、基础组件五组。组内 tab 与对象选择可深链接，编辑草稿按组/tab/对象保存。Permission 在设置。Workflow 编辑两列：阶段/步骤/子流程树与所选节点表单。项目是 Role→Agent 的唯一配置入口。

## 用户补充后的默认目录

新项目页提供可一键采用的已存在默认配置；仍显式保存项目修订，已有项目/Feature 不追随默认变化。默认 Feature 分为需求、架构、任务规划、测试设计、方案交付、Task 开发、Feature 收尾和候选交付八阶段。Stage.description 为可选 Markdown；执行 Role/Spec 仍绑定在实际 Step。六个默认职责含独立项目经理 Role；其 Capability 与架构师不同，底层执行职责复用 architect。七个 Markdown Skill、八个阶段 Spec 嵌入二进制，目录见 resources/configuration。当前真实模型验收只调用 Codex，DSH 不调用。

## AgentID 经验上下文的归属修正

AgentID 经验用于检索、定位 Feature/Task 问题和恢复上下文，不属于 Tool。Tool 列表、Capability 选择和新默认配置不再提供 experience.lookup；原生 worker 不再因历史 Tool 引用投影或挂载 /experience。应用的 query.agent_experience 与 experience.restore 保留原有作用域、完整性验证和来源约束。旧 Tool/Capability/Agent 及配置锁保持不可变，旧经验 Tool 引用只作兼容元数据，不触发执行。新 Tool 与 Capability 写入拒绝将经验定义为工具；旧 Capability 在 UI 保存新修订时移除该引用。默认 Skill/Capability/Agent 追加新修订，不改写历史或自动切换项目既有绑定。
