# 软件开发领域候选包

从仓库根目录构建。

```bash
python3 configurations/software-development/build.py dist/software-development-preflight.zip
```

候选包以固定 Git 提交归属运行仓库预检。Host 读取包声明的跟踪文件、路径及文本匹配结果，Docker 内的 JavaScript 处理器决定测试入口。预检将结果作为工作流产物提交。对已接入的项目，Host 的项目测试入口发现也使用包声明的处理器。

包可提供 Feature、Task 和候选分支的命名策略。Rust 在领取原仓库操作时冻结策略结果，沿用 Git 工作副本写入和原回执核对。已失去回执的操作可以核对原 worktree 和 OID。

当前候选包覆盖仓库预检和工作副本分支策略。完整发布仍需迁移仓库接入、任务编排、测试运行、返工、源发布、候选、GitHub PR、合入与同步。[领域插件化计划](../../docs/plans/domain-plugins/PLAN.md)
