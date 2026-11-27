# 软件开发领域 ZIP

软件开发包 0.5.2 提供 Feature Development、Task Development、Engineering Delivery Standard、执行角色、Agent、Skill、Spec、Delivery 和工程工具。当前发布修订和默认选择在 [catalog/manifest.json](catalog/manifest.json) 与 [catalog/defaults.json](catalog/defaults.json) 中。 0.5.2 的角色讨论按 Role 选择 Agent，按角色查看经验，需要支持 `query.role_experience` 的 Host。配置修订和运行处理器沿用 0.5.1。

- `catalog/` 保存软件开发源定义及正文。Feature 和 Task 通过 `operation_evidence` 绑定已接纳的源发布、评审、测试、批准和同步结果。
- `runtime/worker.mjs` 提供仓库接入、Git 快照上的测试入口识别、分支命名、PR 文案及交付证据判断。它是纯模块，只导出处理器表 `handlers`，由内核的 worker 引导模块加载和调用；拒绝时抛出带代码的 Error。测试报告器也随包发布。
- `web/` 声明分支队列、交付工作台和工程设置，呈现目标为 `web.react`、Web SDK 主版本 1，使用 Web Core 的组件套件。

```bash
./scripts/release/package.sh software-development
```

在已运行的工作台中导入并激活生成的 ZIP。配置、处理器和页面随包的具体摘要冻结。安装、激活和部署命令见[运行指南](../../docs/design/run-binary.md)。协议和跨端扩展见[领域包架构](../../docs/design/domain-packages.md)。
