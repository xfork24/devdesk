# 工具和接口

DevDesk 有三个容易混淆的调用面：浏览器访问 Host 的接口，Agent 经 Harness 调用的工具，以及 Host 内部调度所用的端口。它们的调用者、授权和用途不同。

## 三种调用面

| 调用面 | 调用者与入口 | 用途 | 身份来源 |
|---|---|---|---|
| HTTP / JSON-RPC / SSE | 浏览器访问 Host | 配置、工作项、查询、人工决定、运行控制 | Host 会话及请求作用域 |
| 动态平台工具 | Harness 协议中的工具请求 | Agent 读取项目、派发步骤、调用工程能力 | 关联的 Agent、Session、Turn 和工具定义 |
| Rust 内部端口 | Engine 和后台调度 | 启动/轮询/停止 Harness、执行 Git/测试、读写存储 | 应用流程准备好的执行请求 |

Agent 不能把任意 RPC 方法作为平台工具执行。平台工具有明确注册表和发布定义；调用还必须匹配该轮可用工具及权限。Docker 本身不“调用 Host 工具”，它运行的 Harness 或工具进程才是协议参与者。

## Agent 能看到什么

可用工具由已采用的 Capability、工具定义、驱动能力和运行权限共同决定。Role 描述职责，但单靠角色名不会授予工具。

平台处理器注册表示代码能够处理某个方法；manifest 发布表示这个工具定义可用于配置；某个会话的工具集合才表示模型本轮可以调用什么。这三份集合不应画等号。

另外，Harness 原生工具由供应商执行，例如命令和文件操作；外部 HTTP/MCP 工具经接入执行器运行；经验读取通过专门的上下文接口提供。它们的执行和结果来源不同，历史中应保留这些区别。

## 经理派发工具与内部启动原语

`workflow.step.dispatch` 表达“为这个流程派发某一步”。它包含业务检查，并创建执行记录。`Harness::begin_session`、`dispatch_turn` 和 Docker 的 `create/start` 则负责执行其中的具体动作。

Host 还需要 `poll_session` 读取结果、`answer_tool_call` 回复工具、`stop_session` 停止会话，以及 inspect/cleanup 核对和清理资源。只提供 start/stop 无法支持可靠恢复，因为超时并不说明操作没有发生。

完整分类、参数入口和实现位置见[工具与调度原语参考](reference/tools-and-primitives.md)。

## 同步响应与后台执行

查询通常返回当前已提交视图。修改命令可能只完成“接收并记录请求”，耗时的进程和网络操作随后在后台运行。客户端应结合返回状态和后续查询判断结果，不把 HTTP 成功直接解释为任务完成。

SSE 提示某个作用域的数据发生变化，浏览器再查询当前状态。Agent 的经理唤醒走持久化工作流事件和会话输入，不依赖浏览器连接。

接口细节：[RPC 参考](reference/rpc.md)。代码入口：[Host 请求服务](../crates/devdesk-host/http-and-security.md)、[平台工具](../crates/devdesk-core/manager-and-platform-tools.md)。
