# 架构与 ADR

## 模块

- domain：类型、不可变版本、DAG、绑定、来源与验收规则；无 I/O。
- core：命令/查询、调度与交付协调；定义 EngineeringStore、ArtifactStore、HarnessPort、GitPort、TestRunnerPort、SandboxPort。
- adapters：SQLite 关系表、文件封存、模拟执行能力；不得绕过 core 改业务终态。
- host：loopback HTTP/SSE、会话与同源验证、数据集生命周期、静态 Web 装配。
- web：React/TypeScript 展示、编辑和调用 API；不设置业务状态。

## ADR-001：完整 Web

Web 是长期客户端，Rust 编译时嵌入 Vite 产物。Node 仅构建依赖，成品不需要 Node。未来 Native 客户端使用相同协议。

## ADR-002：跨平台单机

Rust stable 固定版本、bundled SQLite、普通路径及文件锁；不引入平台业务 API。Linux x86_64 与 macOS Apple Silicon 均已执行后端及 Web 实机验收，分别保存平台证据，见 [验收记录](acceptance.md)。

## ADR-003：真实规则、模拟外部能力

所有 Project 均为 simulation。Git/Harness/Tests/Sandbox 只产生有来源的能力结果；核心决定状态。适配器结果先校验结构，支持固定故障、取消和 hold/continue。后续真实适配器复用 ports 与契约测试，不需要重写 Review/Attempt 规则。

## ADR-004：持久化与幂等

规范化关系表保存高频标量列、外键和索引；只有确定类型的数组、绑定锁和供应商结果用 JSON 列。初版每次工作单元加载类型化工程状态并写回变化，优先事务一致性；不把整个项目存成 JSON。查询返回有界结果。后续可在同一 port 下优化增量 SQL，不变更领域语义。

命令结果及其派发意图在同一事务保存；operations 唯一键保存指纹与原结果。模拟 Git 目标推进、Merge 完成及事件在接纳回执的同一事务保存。单 host 数据集文件锁，进程内写协调。未提交的 Artifact 可成为孤儿文件，但不得成为可见交付；只读时验证 hash。数据库迁移按 user_version 执行，不以删库升级。

## ADR-005：恢复与范围

重启将运行未确认记录标记 interrupted。通过原适配器核对终态后才释放资源并允许显式重试，结果未知时保留资源与阻塞理由。供应商具体进程管理留在适配器中，内核恢复协议已由故障夹具验证。Archive/迁移只预览，不写成功事实。只有自己初始化的空目录才能建数据集；不修改用户仓库。

## 阶段评审

每阶段报告列出实现、测试、限制和评审。独立 Agent 审查与作者自检分别保存；本轮 macOS 补充验收及独立结论见 [阶段报告索引](reports/README.md)。

## ADR-006：持久化意图、回执与短事务

执行分为登记、调用、接纳三个阶段。core/effects.rs 管理 Harness、Cancel、Test 与 Cleanup 的 EffectRecord；core/advances.rs 管理 MergeAdvance。Run/Session/Resource 意图和操作 ID 先提交，再调用供应商；供应商调用不持有 SQLite 事务或单写协调器。收到结果后先保存原始回执，再在短事务内核对 Run 状态、输入版本、绑定、取消和目标版本。迟到结果保留为 discarded，不成为交付证据。

SQLiteStore 的 writer 只协调本地状态变更及文件封存工作单元；查询在工作单元执行期间仍读取上一个已提交状态。Harness/Test/Git 推进/清理已移到该协调器外，取消可在慢轮询期间提交。状态工作单元仍加载类型化状态，SQL 只 upsert 实际变化的记录；前端和事件查询有界分页，未实现大规模历史库压测。

Run 完成时封存文档与日志，随后同事务提交终态及 Cleanup 意图；在提交后以原 Resource ID 调用清理。导出失败复用已保存的回执，不重复执行供应商；清理失败保留历史并允许显式重试。崩溃后重复 delete 必须按同一 Resource ID 幂等，只有确认终止/释放才解除旧 worktree 的写许可。

## ADR-007：目标推进不是跨系统事务

merge.apply 只验证并登记推进意图，返回 operation_id；调度器调用 GitPort.advance。适配器身份、源交付、候选与预期目标固定。dispatched 表示已发出但尚未确认；重启必须向同一适配器查询，禁止盲目再次 advance。回执保存后，目标比较、逻辑 head 更新、Merge/Task/Feature 完成及事件同事务提交。

AdvanceObservation 区分 applied、not_applied 和 unknown。确认已推进时按同候选/同验收补记；确认未推进时记录观察到的目标、使旧候选失效，重新构造验证；结果未知时保持 needs_reconciliation。若输入或目标在调用期间变化，保存外部回执但不伪造完成。SQLite 回滚不能撤销外部副作用。

## ADR-008：锁定输入与阶段输出

Attempt.documents、scope、binding、context 与 baseline 是冻结输入；Attempt.outputs 保存本尝试生成的文档修订。后续 Run 明确接收 frozen_inputs、stage_outputs、锁定的定义正文及运行实例，不从可变 Feature 默认配置重新解释旧执行。Reviewer 和 Merge 验证继承源交付的 Standard/执行锁；最终 Snapshot 保存已接受 Attempt 实际使用的 Context，Context 更新另存追加修订事件。

Archive/迁移执行、环境重建和真实供应商安装、账号/凭据接入仍按计划延期；实际供应商应实现已落地的操作与核对契约并通过隔离真实用例，再开放 live。参见 [适配器契约](real-adapter-contract.md)。

## ADR-009：明确的仓库与测试请求路由

Attempt.git_context 在启动时冻结 Workspace、Project 和源/目标分支/worktree。MergeAdvance.context 从源交付复制；candidate、advance 和 reconcile_advance 均显式接收它。派发前再次校验输入、目标和 Review，失效意图记为未执行，不调用供应商。

TestRequest 附冻结的定义正文、GitContext 和 ExecutionInstance（包含执行回传的环境身份）；Harness 输入也携带代码定位。提供者不以进程全局 cwd 或可变 Project 当前设置定位工程。历史缺少定位的数据保持 null，不能自动根据当前配置倒填。

并行 DAG 的请求在首次派发时固定到 EffectRecord。后续轮询不重新吸收兄弟节点输出，避免 Green/Docs 或 Feature Tests/Docs 并行时改变供应商输入。评审独立性比较 Agent 修订链的身份根，而非仅比较 revision ID。
