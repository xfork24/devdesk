# Web 与 Host 的代码衔接

Web 是 Rust 服务的客户端。本页补齐从界面动作到 RPC、SSE 和查询缓存的阅读路线，不将前端当作另一个 Rust crate。

## 请求入口

[main.tsx](../../web/src/main.tsx) 挂载应用，[App.tsx](../../web/src/App.tsx) 组织页面并提供统一 rpc/request 边界。组件通过传入的 request 调用 Host，集中处理超时、错误和作用域，不各自拼出一套 fetch。

请求和未完全类型化的数据边界定义在 [rpc-types.ts](../../web/src/components/rpc-types.ts)。Row/Request 是现有宽类型边界；新组件已知的 Props、状态和返回结构应使用具体类型，不能把 Row 当作已经通过校验的证明。

## 配置与运行查询

| 模块 | 作用 |
|---|---|
| [configuration-client.ts](../../web/src/configuration-client.ts) | 配置/模型目录查询、版本观察、缓存失效与会话隔离 |
| [runtime-state.ts](../../web/src/runtime-state.ts) | 工程命令和事件使相关运行查询失效 |
| [query-cache.ts](../../web/src/query-cache.ts) | 有界查询缓存 |
| [use-read-request.ts](../../web/src/components/use-read-request.ts) | 组件读取请求与取消处理 |
| [useRuntimeRefresh.ts](../../web/src/components/useRuntimeRefresh.ts) | 运行页按事件和作用域刷新 |

configuration 的目录版本、runtime 的显示 generation 和业务修改 expected_revision 不能互相替代。迟到响应需要核对请求身份，切项目后旧响应不能写进新项目页面。

## 从页面到业务

[ProjectManager.tsx](../../web/src/components/ProjectManager.tsx) 与 [WorkflowProjectManager.tsx](../../web/src/components/WorkflowProjectManager.tsx) 提供经理视图；[WorkflowInstanceView.tsx](../../web/src/components/WorkflowInstanceView.tsx) 展示流程；[WorkflowStepInput.tsx](../../web/src/components/WorkflowStepInput.tsx) 处理步骤输入；[ManagerInteraction.tsx](../../web/src/components/ManagerInteraction.tsx) 组织交互决定。

这些组件提交对象和原版本，Host 记录可信来源并执行 core 规则。不要在组件中以本地状态推导永久终态，也不要在冲突后自动更新原版本再重发。

## 修改和测试

新增查询时确认数据范围与大小；新增 mutation 时确认版本来源、草稿保留、忙碌和失败显示；新增事件处理时确认不会使每条 token 消息都触发全页面查询。

测试入口：[project-isolation.spec.ts](../../web/tests/project-isolation.spec.ts)、[workspace-refresh.spec.ts](../../web/tests/workspace-refresh.spec.ts)、[settings-concurrency.spec.ts](../../web/tests/settings-concurrency.spec.ts)、[workflow-approvals.spec.ts](../../web/tests/workflow-approvals.spec.ts)。风格与异步约定见[Web 规范](standards/web.md)。
