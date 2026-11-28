# Web 与 Host 的代码衔接

Web 是 Rust 服务的客户端。本页补齐从界面动作到 RPC、SSE 和查询缓存的阅读路线，不将前端当作另一个 Rust crate。

## 请求入口

[index.html](../../web/src/core/index.html) 是 Web Core 的 HTML 源码入口，由 [core-main.tsx](../../web/src/core/core-main.tsx) 挂载界面。[DomainWorkbench.tsx](../../web/src/core/DomainWorkbench.tsx) 读取活动代次并加载贡献。[软件领域入口](../../plugins/software-development/web/entry.tsx) 用 [SDK](../../web/src/domain-sdk.ts) 获取项目上下文，在共享布局中渲染 [App.tsx](../../web/src/core/App.tsx) 的业务页面。组件通过传入的 request 调用 Host，统一处理超时、错误和作用域。

请求和未完全类型化的数据边界定义在 [rpc-types.ts](../../web/src/sdk/rpc-types.ts)。Row/Request 是现有宽类型边界；新组件已知的 Props、状态和返回结构应使用具体类型，不能把 Row 当作已经通过校验的证明。

## 配置与运行查询

| 模块 | 作用 |
|---|---|
| [configuration-client.ts](../../web/src/sdk/configuration-client.ts) | 配置/模型目录查询、版本观察、缓存失效与会话隔离 |
| [runtime-state.ts](../../web/src/sdk/runtime-state.ts) | 工程命令和事件使相关运行查询失效 |
| [query-cache.ts](../../web/src/sdk/query-cache.ts) | 有界查询缓存 |
| [use-read-request.ts](../../web/src/sdk/use-read-request.ts) | 组件读取请求与取消处理 |
| [useRuntimeRefresh.ts](../../web/src/kit/useRuntimeRefresh.ts) | 运行页按事件和作用域刷新 |

configuration 的目录版本、runtime 的显示 generation 和业务修改 expected_revision 不能互相替代。迟到响应需要核对请求身份，切项目后旧响应不能写进新项目页面。

## 从页面到业务

[ProjectManager.tsx](../../web/src/core/ProjectManager.tsx) 与 [WorkflowProjectManager.tsx](../../web/src/kit/WorkflowProjectManager.tsx) 提供经理视图；[WorkflowInstanceView.tsx](../../web/src/kit/WorkflowInstanceView.tsx) 展示流程；[WorkflowStepInput.tsx](../../web/src/kit/WorkflowStepInput.tsx) 处理步骤输入；[ManagerInteraction.tsx](../../web/src/kit/ManagerInteraction.tsx) 组织交互决定。

这些组件提交对象和原版本，Host 记录可信来源并执行 core 规则。不要在组件中以本地状态推导永久终态，也不要在冲突后自动更新原版本再重发。

## 修改和测试

新增查询时确认数据范围与大小；新增 mutation 时确认版本来源、草稿保留、忙碌和失败显示；新增事件处理时确认不会使每条 token 消息都触发全页面查询。

测试入口：[project-isolation.spec.ts](../../tests/browser/project-isolation.spec.ts)、[workspace-refresh.spec.ts](../../tests/browser/workspace-refresh.spec.ts)、[settings-concurrency.spec.ts](../../tests/browser/settings-concurrency.spec.ts)、[workflow-approvals.spec.ts](../../tests/browser/workflow-approvals.spec.ts)。风格与异步约定见[Web 规范](standards/web.md)。
