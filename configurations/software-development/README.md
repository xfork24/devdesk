# 软件开发领域 ZIP

软件开发包 0.5.1 提供 Feature Development、Task Development、Engineering Delivery Standard、执行角色、Agent、Skill、Spec、Delivery 和工程工具。当前发布修订和默认选择在 [catalog/manifest.json](catalog/manifest.json) 与 [catalog/defaults.json](catalog/defaults.json) 中。 0.5.1 更新 Web 呈现，沿用 0.5.0 已发布的配置修订和运行处理器。

- `catalog/` 保存软件开发源定义及正文。Feature 和 Task 通过 `operation_evidence` 绑定已接纳的源发布、评审、测试、批准和同步结果。
- `runtime/worker.mjs` 提供仓库接入、Git 快照上的测试入口识别、分支命名、PR 文案及交付证据判断。测试报告器也随包发布。
- `web/` 声明分支队列、交付工作台和工程设置，使用 `web.react` API 2 和 Web Core 的共享 SDK。

```bash
./scripts/release/package-core.sh software
```

将生成的 ZIP 导入已运行的 Host，使用可信发行摘要启用运行端和 Web 资产。核心目录、处理器和页面随包的具体摘要冻结。安装、激活和部署命令见[运行指南](../../docs/design/run-binary.md)。协议和跨端扩展见[领域包架构](../../docs/design/domain-packages.md)。
