# 软件开发领域包

从仓库根目录构建。

```bash
python3 configurations/software-development/build.py dist/software-development-candidate.zip
```

候选包以固定 Git 提交归属运行仓库预检。Host 读取包声明的跟踪文件、路径及文本匹配结果，Docker 内的 JavaScript 处理器决定测试入口。预检将结果作为工作流产物提交。对已接入的项目，Host 的项目测试入口发现也使用包声明的处理器。

包可提供 Feature、Task 和候选分支的命名策略。Rust 在领取原仓库操作时冻结策略结果，沿用 Git 工作副本写入和原回执核对。已失去回执的操作可以核对原 worktree 和 OID。

候选包包含软件开发的 Feature 和 Task 工作流、标准、角色、Agent、权限、Spec、Skill 和各个交付合同。产品源码中的精确修订在构建时映射到软件包命名空间，核心模型和 Harness 继续按原修订引用。新的空领域 Host 可以安装这套配置，无需内置工程目录。

连接项目时，调用 `domain.project.create` 并传入 `connection`，其中包含 GitHub `repository` 和目标分支 `mainline`。Docker 内的包处理器解析连接坐标，Host 记录这些坐标并执行仓库操作。活动包的项目默认值可通过 `query.domain.active` 读取。

包提供 Node 与 Python 测试报告器。工具冻结报告器摘要和原始内容，Docker 从确切 Git 修订执行测试并保留结构化回执。包内处理器还生成候选分支和 GitHub PR 创建请求，Host 保存这些请求以便中断后核对，并执行远端保护、目标校验和同步。[领域插件化计划](../../docs/plans/domain-plugins/PLAN.md)
