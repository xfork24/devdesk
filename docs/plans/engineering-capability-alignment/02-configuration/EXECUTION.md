# S2 执行与阶段验收

日期：2026-11-13。结论：针对性验收通过，可进入 S3。沿用用户要求，不运行全量 Feature/Task 开发流程。

## 实现

- domain 提供 ConfigurationEdge，按声明路径识别根级引用、Workflow 角色/端口/Spec/Skill/子流程、权限授权、Catalog、Integration、实例与工作选择。纯正文、示例和常量不形成依赖。核心反向查询与冻结计划闭包已改为使用这些边。
- 普通发布与批量候选共用每种对象的既有验证规则；新增候选完整图验证及带来源/字段/目标的诊断。发布不比较配置版本元数据，未知执行原语仍拒绝。
- ConfigurationChangeSet 支持 Create/Revise/Retire/SelectProject/SelectWork。源先全部进入候选，再校验依赖和选择；同库整体提交，head、作用域、目录和预演指纹有 CAS 约束。停用检查身份全部修订的当前消费者，历史保留；同批显式移除的消费者可一并停用。
- 新 Work 选择分配稳定身份；预演摘要排除审计时间，修复了预演/提交因时间或随机选择 ID 不一致而误拒的问题。发布与未来绑定/当前执行变更保持不同用例。
- 新增跨库 stage/apply/cancel：隐藏候选、共享目录写入屏障、项目原子复制/采用、持久回执、共享目录最终发布。Host 打开项目时先恢复 applying 操作；项目提交后共享提交失败可以重启完成，无需重做项目选择。
- RPC 发布新的变更/依赖边结构。公共修订选择器和 Agent 实例、工作流角色、项目编排角色入口可主动选择历史版本；编辑列表仍显示最新对象。完成相关 React 列表键、状态、只读排序与表单可访问性检查。
- 扩展编译检查发现 S1 中移动 ID 分配后遗漏的旧测试 helper/import，已补齐；全部 workspace 测试目标现在能编译。

## 验收证据

| 检查 | 结果 |
|---|---|
| configuration_changes | 7 通过：反序 Spec/Delivery、反序 Agent/Capability/Skill、非法整体拒绝、过期预演、历史和幂等、跨修订停用/文本非依赖、稳定 Work 选择、跨库中断恢复（多个情景合并在同一用例） |
| configuration_resources | 14 通过 |
| governance | 5 通过 |
| schema_contracts | 13 通过，包含新增变更集/边合同 |
| domain/core 单元 | 21 通过（domain 18、core 3） |
| 冻结选择恢复、递归子流程拒绝、Integration 删除引用保护、Host 过期预演回滚 | 各 1 通过 |
| Playwright 指定三例 | 3 通过：Capability/Agent 精确组合、阶段角色继承、主动选择未采用过的历史中间修订 |
| Web build、架构/默认资源检查、格式与 diff 检查 | 通过 |
| cargo test --workspace --no-run --locked | 全部测试目标编译通过；未执行其中长流程 |

合计 64 项上述 Rust 针对性测试及 3 个指定浏览器用例通过。跨库用例在项目提交完成、共享提交前注入失败，重开两个数据库，核对暂存不可见、项目引用自足、一次采用及最终共享发布。

## 边界

跨库共享阶段只发布新/后继源，不把共享删除与多个项目采用伪装成一个原子操作。全局停用通过项目目录逐一检查使用方；有无法读取的项目时不声称无引用。Source/Project 默认/Work 冻结的表示没有被强行合并。运行资源就绪、真实 Harness 支持与受管 Agent 写工具分别属于后续 S3/S5。

未运行全量 Feature/Task 开发、真实模型工程流程或全部 Playwright。日志位于临时目录，未提交。
