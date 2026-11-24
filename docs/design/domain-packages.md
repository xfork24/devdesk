# 领域包与客户端扩展

DevDesk v0.5 独立交付 Rust Host、Web Core 和领域 ZIP。Host 带有 Model、Harness、Docker Sandbox、Git 和领域无关的工具、权限预设及领域模型。业务部署可从空领域开始，在运行时安装所需的 ZIP。

## ZIP 内容

| 内容 | 职责 |
|---|---|
| `manifest.json` | 包 ID、版本、标记、文件 SHA-256、配置与可选运行时或呈现目标 |
| `configurations/` | 不可变源定义和默认选择，以及实体 Schema、工作流绑定和引用索引 |
| `runtime/` | 可选 JavaScript 处理器、输入输出合同关联和运行辅助文件 |
| `ui/` | 可选客户端贡献声明与目标专属资产 |

格式由[领域包 Schema](../../schemas/domain-package.schema.json)、[对象定义](../../schemas/domain-entity-types.schema.json)和[客户端贡献](../../schemas/domain-contributions.schema.json)共同描述。核心与领域目录在源码中独立管理。软件开发包的 Feature、Task 使用冻结证据授权，仓库识别、分支命名、PR 文案、测试报告和页面入口也随包发布。

## 安装、激活和版本

安装核对清单、逐文件摘要、引用和包定义。活动目录记录包 ID、摘要和活动代次，整个工作台共享一个活动领域。切换更新代次，客户端重新发现贡献。停用保留已安装的版本，卸载撤掉对应版本的运行入口。处于活动执行的包受到切换屏障保护。

项目及执行保存包摘要。切回所需版本后可读到它原有的项目和执行。更新 ZIP 使用新版本；同 ID、同版本的不同内容会被拒绝。现有早期包的离线回切检验使用真实历史 ZIP 夹具。

## 运行端与前端

领域 JavaScript 处理器由 Host 调度到受限的 Docker worker 进程，使用 JSON-RPC 处理调用。Host 验证输入、范围、返回值和运行证据。核心 Git、持久化模型、调度与发起外部操作的权限仍由平台管理。

Web Core 使用 `web.react` API 2 将受信包的 ES 模块装进同一 React 树。组件获取当前项目、导航、RPC 和订阅上下文。领域页共用布局和主题，以包提供的样式表扩展。执行脚本与核心前端共用浏览器上下文，必须校验信任摘要。

非浏览器客户端通过 `query.domain.active` 和 `/domain-events` 读取相同代次及贡献目录。它可以用通用动作、字段和数据查询自行呈现，也可以实现包声明的专属呈现目标。项目操作与事件订阅使用相同的作用域规则。原生客户端无需加载 Web 资产。

协议示例见[RPC 参考](reference/rpc.md)，安装与兼容矩阵见[运行指南](run-binary.md)，跨端制品验收见[测试索引](../../tests/README.md)。
