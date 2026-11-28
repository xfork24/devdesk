# 软件开发领域 ZIP

软件开发包 0.5.5 提供 Feature Development、Task Development、Engineering Delivery Standard、执行角色、Agent、Skill、Spec、Delivery 和工程工具。当前发布修订和默认选择在 [catalog/manifest.json](catalog/manifest.json) 与 [catalog/defaults.json](catalog/defaults.json) 中。「跨 Harness 软件工程」是可选的 Standard。

在项目设置选择「跨 Harness 软件工程」和 Agent 调度，再按下表为各岗位选择随包提供的 Agent。新 Feature 入口调用同一 Standard 中的跨 Harness Task 子流程。

| 岗位 | Harness | 模型 |
|---|---|---|
| 项目经理、拆解和调度 | Codex | GPT-6.2-Sol |
| 需求分析 | Pi | MiniMax-M3.1-Flash |
| 架构设计 | OpenCode | MiniMax-M3.1-Flash |
| Task 实现 | mcode | MiniMax-M3.1-Flash |
| 独立测试及阶段、Task 评审 | DSH | MiniMax-M3.1-Flash |
| Feature 独立交付评审 | ZCode | MiniMax-M3.1-Flash |

同一 MiniMax M Plan key 可按各 Harness 的服务商配置分别保存。ZCode 配方从固定官方 CLI 源码在本机 Docker 中构建，发布包携带配方和源码摘要。跨 Harness 流程继承 Feature v6、Task v5 的交付合同、人工确认、独立 Red/Green 与回归门禁。

- `catalog/` 保存软件开发源定义及正文。Feature 和 Task 通过 `operation_evidence` 绑定已接纳的源发布、评审、测试、批准和同步结果。
- `runtime/worker.mjs` 提供仓库接入、Git 快照上的测试入口识别、分支命名、PR 文案及交付证据判断。测试报告器也随包发布。
- `web/` 声明分支队列、交付工作台和工程设置，使用 `web.react` API 2 和 Web Core 的共享 SDK。

```bash
./scripts/release/package.sh software-development
```

在已运行的工作台中导入并激活生成的 ZIP。配置、处理器和页面随包的具体摘要冻结。安装、激活和部署命令见[运行指南](../../docs/design/run-binary.md)。协议和跨端扩展见[领域包架构](../../docs/design/domain-packages.md)。
