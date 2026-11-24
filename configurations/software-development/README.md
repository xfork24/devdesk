# 软件开发领域包

从仓库根目录构建。

```bash
./scripts/release/package-core.sh software
```

软件开发包以固定 Git 提交归属运行仓库预检。Host 读取包声明的跟踪文件、路径及文本匹配结果，Docker 内的 JavaScript 处理器决定测试入口。预检将结果作为工作流产物提交。对已接入的项目，Host 的项目测试入口发现也使用包声明的处理器。

包可提供 Feature、Task 和候选分支的命名策略。Rust 在领取原仓库操作时冻结策略结果，沿用 Git 工作副本写入和原回执核对。已失去回执的操作可以核对原 worktree 和 OID。

软件开发包包含 Feature 和 Task 工作流、标准、角色、Agent、权限、Spec、Skill 和各个交付合同。产品源码中的精确修订在构建时映射到软件包命名空间，核心模型和 Harness 继续按原修订引用。新的空领域 Host 可以安装这套配置，无需内置工程目录。

连接项目时，调用 `domain.project.create` 并传入 `connection`，其中包含 GitHub `repository` 和目标分支 `mainline`。Docker 内的包处理器解析连接坐标，Host 记录这些坐标并执行仓库操作。活动包的项目默认值可通过 `query.domain.active` 读取。

包提供 Node 与 Python 测试报告器。工具冻结报告器摘要和原始内容，Docker 从确切 Git 修订执行测试并保留结构化回执。包内处理器还生成候选分支和 GitHub PR 创建请求，Host 保存这些请求以便中断后核对，并执行远端保护、目标校验和同步。[领域插件化计划](../../docs/plans/domain-plugins/PLAN.md)

正式 ZIP 的实仓证据、候选树和同步结果见[执行记录](../../docs/plans/domain-plugins/EXECUTION.md)。


0.4.0 同时提供 Web 呈现目标 `web.react` API 2。Web Core 从 ZIP 加载 React ESM 模块及工作台样式。分支队列、交付和工程设置的入口在 `web/contributions.json` 声明。浏览器和独立客户端使用 `query.domain.contribution` 读取相同的项目查询结果。连接 GitHub 项目时，Web 通过 `domain.project.create` 使用当前包的连接处理器及默认配置。

0.3.0 的流程配置和 JavaScript 运行端在 0.4.0 中逐文件保持一致。使用 `--legacy-runtime-only` 可重建 0.3.0，以复验独立升级和版本回切。

0.5.0 使用 Feature Development 第 6 修订、Task Development 第 5 修订和 Engineering Delivery Standard 第 6 修订。交付流程在 `delivery-evidence.json` 声明证据输出、平台生产者和不可豁免的人工批准。Rust 在固化的执行意图上核对并绑定执行 ID、输入摘要、接纳结果和实际平台回执。Docker 内的验证器检查源版本、候选回归和适用的人工批准或独立 Green，Host 留存决定后执行受约束的 Git 操作。流程完成条件引用已接纳的平台合入与同步回执。

旧 ZIP 和冻结工作流继续按其原配置读回，`--legacy-web` 可重建 0.4.0 用于兼容性检查。验证新流程使用独立领域项目和专用 GitHub 测试分支。
