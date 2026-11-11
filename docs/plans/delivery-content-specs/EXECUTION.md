# Delivery 分类与内容规范实施记录

日期：2026-11-11。工作分支：`codex/delivery-content-specs`。起点：`b70cd10`。状态：独立 worktree 实施，合并 main 后统一测试与验收。

## 已完成的实现

- 发布 21 种 Delivery，全部关联内容 Spec；采用 Review Report、Task Doc、Test Doc、Execution Report、Task Report，按内容替换全部 25 个通用对象输出端口及 Task 单项输入，纠正 PRD 在基线、实现及验收总结中的复用。
- 原源修订保持不变，新增 Delivery、Spec、Workflow、Tool、Capability、Agent、Permission Context 和相关 Standard 修订，更新当前发布清单及默认选择。
- 创建、导入、模板加载、领域存储和流程编译统一要求有效内容 Spec，拒绝空、重复、悬空、错误作用域及没有检查项的规范。
- 平台产出声明允许的工具来源；编译和接纳核对实际工具、当前执行、真实回执和输出映射，保存不可变 provenance。提案和材料导入不能创建平台或子流程产出。
- 快照增加可信范围基线比较，输出相邻 Diff 与 Task 整体 Diff；候选输出独立 Diff。测试、基线、实现、候选 Diff 显式进入端口，评审与交接消费确切 patch。
- Git 比较保留完整对象 ID、重命名及二进制 patch；产物保存比较端点、来源与摘要，界面显示完整 Diff 及关联内容标准。
- 大输入使用带摘要的显式产物引用和完整分块读取；业务 Agent 的读取范围限制到当前输入产物及字段，Capability 和 Permission 同时授权。
- Test Doc 增加 scope、cases、recipe，测试工具消费 `/recipe`。Test Report 依据现有真实测试执行器的请求和日志形成，Execution Report 保留 Agent 原始执行记录的来源区别。
- Feature 收尾先准备候选并测试，再形成 Feature Report 和独立评审；Task Report 核对子流程完成、成员与顺序。
- Delivery、Workflow 和产物界面展示关联规范、固定修订、平台来源、测试报告及 Diff。

## 验证安排

按用户最新要求，worktree 内进行构建和针对性验证，完整测试合并 main 后统一执行。验证使用独立临时新数据集，不修改用户既有工程数据，不执行远端发布。

worktree 阶段已经通过 Web 构建、workspace cargo check、配置与 Workflow 集成定向回归（11 项配置测试和 34 项 Workflow 测试）、大产物完整分块读取回归。真实 Git 回归包括快照与候选 Diff 的独立持久化及评审输入，并通过二进制、重命名、模式、符号链接和空差异的真实比较。

合并与统一测试的最终结果在本记录后续补充。
