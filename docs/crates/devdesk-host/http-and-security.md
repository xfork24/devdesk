# HTTP、RPC、认证与事件

[main.rs](../../../crates/host/src/main.rs) 定义 HTTP 路由和来源/认证检查。请求通过检查后进入 Workspace，后者根据方法和项目选择对应的应用流程。

## 一次请求怎样进入应用

1. 检查 Host、Origin 和连接会话，拒绝不符合监听/公开地址配置的来源。
2. 解析 JSON-RPC 请求，要求明确 id 和对象参数。
3. 按方法的作用域选择配置、全局讨论或项目 Engine。
4. 调用 core 用例或专用查询；把业务错误转换成 RPC 错误，成功时附带必要提交元数据。

`dispatch_with_receipt` 保留业务结果和提交信息；显示读取有独立路径，避免读请求顺便修改配置或执行恢复。

## 请求如何按功能分组

| 模块 | 主要服务 |
|---|---|
| [governance.rs](../../../crates/host/src/governance.rs) | 定义与配置写入口 |
| [configuration_read.rs](../../../crates/host/src/configuration_read.rs) | 配置列表、正文和查询 |
| [runtime_reads.rs](../../../crates/host/src/runtime_reads.rs) | 运行、历史及工具读取 |
| [runtime_setup.rs](../../../crates/host/src/runtime_setup.rs) | 明确的运行准备操作 |
| [permission_resources.rs](../../../crates/host/src/permission_resources.rs) | 权限资源与目标绑定 |
| [model_providers.rs](../../../crates/host/src/model_providers.rs) | 模型目录和连接设置 |
| [integrations.rs](../../../crates/host/src/integrations.rs) | 工具发现、导入与认证入口 |
| [github_projects.rs](../../../crates/host/src/github_projects.rs) | GitHub 项目接入 |

新增方法时同步核对 core/rpc_contracts.json、Schema、访问分类和前置条件。不能因为请求使用 POST 就假定它一定是修改，也不能用 query 前缀掩盖实际写入。

## SSE 与产物

[event_hub.rs](../../../crates/host/src/event_hub.rs) 的 EventHub 提供按作用域共享的有界事件尾流。它订阅存储变化并管理缓存窗口；游标不连续或预算不足时发送 reset，客户端重新查询。

产物请求按项目和产物 ID 查找内容并核验摘要，不接收任意宿主路径。SSE 的作用域也不能沿用另一个项目的 Last-Event-ID。

## 验证入口

[http_contract.py](../../../tests/entrypoints/http_contract.py) 验证协议与来源，[event_tail_contract.py](../../../tests/entrypoints/event_tail_contract.py) 验证事件流，[project-isolation.spec.ts](../../../tests/browser/project-isolation.spec.ts) 验证浏览器项目隔离。完整参数见[RPC 参考](../../design/reference/rpc.md)。
