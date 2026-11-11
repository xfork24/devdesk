# Delivery 分类与内容规范实施记录

日期：2026-11-11。工作分支：`codex/delivery-content-specs`。起点：`b70cd10`。状态：P0—P5 已完成，独立 worktree 实施后合并 main 并统一验收。实现提交 `b66e37a`，首次合并 `3d86102`；最终测试版本 `d16c21a`，后续仅补充验收文档。

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

## 验证顺序

按用户最新要求，worktree 内进行构建和针对性验证，完整测试合并 main 后统一执行。验证使用独立临时新数据集，不修改用户既有工程数据，不执行远端发布。

worktree 阶段已经通过 Web 构建、workspace cargo check、配置与 Workflow 集成定向回归（11 项配置测试和 34 项 Workflow 测试）、大产物完整分块读取回归。真实 Git 回归包括快照与候选 Diff 的独立持久化及评审输入，并通过二进制、重命名、模式、符号链接和空差异的真实比较。

## main 统一测试结果

验证环境为 macOS arm64，真实测试容器为本地已有的固定 Node 24.13.0 镜像。Delivery 配置、RPC、源存储、Git 与测试产出、评审输入及 Web 呈现分别沿实际接口验证。

| 范围 | 结果与依据 |
|---|---|
| Rust 自动测试 | 214 项通过。首次完整运行发现 1 处权限夹具继承新增平台工具但没有冻结接入锁，已将该手写绑定夹具明确为原生工具能力并在 main 复验全部 28 项 Host 测试；其余 186 项通过，Doc tests 通过 |
| debug Web | 44 项通过；初次 42 项通过，2 处旧夹具分别遗漏内容 Spec、使用旧 Agent 修订，修正后失败项单独复验通过 |
| release Web | 完整 44 项通过，包含 Delivery 规范关联、确切修订、保存重载、流程编辑、执行、权限与模板重置 |
| HTTP 与连接 | debug/release 均通过，覆盖认证、Host/Origin、中文与空格路径、进程锁、幂等、Artifact 读取、SSE、重启和项目分库；release 从独立临时目录执行复制后的二进制 |
| 模板与数据集 | debug/release 模板重置、不可变历史、重放及 no-op 通过；数据集初始化、隐藏残留、Web setup、重启及旧数据保留通过 |
| 运行时准备 | 中断恢复、延迟准备、并发复用、失败重试和嵌入配方的独立包检查通过 |
| 真实 Git | 快照、整体和候选比较、来源核验、独立 Diff Artifact 和评审输入通过；新增、删除、重命名、二进制、模式、符号链接、空差异及非法端点通过 |
| 真实 Docker | 使用产品 CommandTests 执行器完成业务断言 Red、实现 Green、候选测试；3 份真实 Test Report、Test Doc、固定对象 Diff 的 Schema、内容、Artifact 摘要及读取一致性通过，重复轮询保持原回执 |
| 大产物与来源 | 原始执行内容超过派发内联阈值时形成显式引用，分块读回完整 UTF-8 JSON 并与原始 Artifact 一致；无关输出不进入任务。平台产出通过编译约束及原始回执映射校验，不能由 Agent 或导入材料冒充 |

统一检查遵循 `scripts/check/check.sh` 的全部阶段。发现夹具问题后按失败边界修复并继续后续阶段，避免重复已经通过且未受修改影响的整套测试。最终 release 构建及完整浏览器检查均在 main 执行。

真实 Docker 用例为 [delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs)，需显式提供已存在的 `DEVDESK_ACCEPTANCE_NODE_IMAGE=sha256:…`，再运行 `cargo test --locked -p devdesk-adapters --test delivery_acceptance -- --ignored --nocapture`。真实 Git 和端口传递回归见 [configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs) 和 [git_resources.rs](../../../crates/adapters/tests/git_resources.rs)。

## 验收边界

本轮通过配置、结构与来源、真实 Git、真实 Docker、独立评审端口与工作流状态、完整 Web 入口的本地验收。模型执行路径使用仓库明确标识的原生端口夹具；没有发起真实模型调用或远端 GitHub 发布，不能据此宣称已重跑在线的默认 Feature 22 步与 Task 20 步发布闭环。14 个原有环境专用手动用例未自动运行，新增 Docker 手动验收已单独通过。

按本计划采用临时新数据集验证，未变更用户既有工程数据，也未提供旧数据升级器。原工作区同期 Standard 计划和其他未提交文件保留。
