# Web 开发规范

前端使用 React 和 TypeScript，格式由 Prettier 管理，静态检查以仓库 ESLint/TypeScript 配置为准。页面应如实表达 Host 状态，并保护用户未提交的输入。

## 模块与类型

新组件文件采用 PascalCase，普通工具模块采用 kebab-case。组件使用具名导出；现有 MarkdownPreview 的 lazy 默认导出是特定用途，不扩散成默认模式。组件文件只导出组件，状态、hooks、导航和规范化函数放独立模块。

Props 使用明确 type，已知状态和结果使用具体结构。未知输入用 unknown 并收窄。现有 rpc-types.ts 集中定义 Row/Request 的宽类型边界，其他文件不复制本地 any 别名；这些类型不保证数据已通过校验。

字符串联合优先于不必要的运行时 enum。单选/多选等相关属性使用判别联合，避免通过断言绕过类型关系。浏览器与测试/配置的全局类型分别管理。

## 状态和异步

局部状态使用 useState；跨模块共享状态沿用模块 store 与 useSyncExternalStore。不要为单个功能引入另一套全局状态框架。

请求通过 App 的统一 request 边界，组件不各自实现 fetch。异步响应核对请求序号或有效标记，卸载和项目切换时取消或失效。effect 写出真实依赖；不能通过漏依赖控制请求次数，也不能机械加入每次变化的新对象导致循环查询。

保存操作明确显示 busy、错误和结果，finally 恢复状态。浮动 promise 明确 void 或捕获错误。轮询使用可停止的 setTimeout 链；不使用逗号运算符压缩多条语句。

## 草稿、版本与刷新

展示刷新只更新读取视图，不重置编辑草稿和原 expected_revision。版本冲突时保留草稿，让用户查看差异后决定。不能用自动读取新版本再保存来掩盖冲突。

SSE 用于使相关查询失效。不要对每个 token 或工具日志触发整页查询。查询缓存必须区分 Host、项目和必要的版本身份，旧请求不能污染新页面。

## 界面与可访问性

静态样式使用 style.css 中的语义类名；随数据计算的样式才使用行内样式或变量。交互优先语义标签与合适的 ARIA 属性，测试优先 getByRole/getByLabel 等稳定定位。

模型消息、平台结果和 Human 决定应有清晰的来源显示。前端不能自行判定持久化业务终态。

## 检查与测试

```bash
npm --prefix web run lint
npm --prefix web run format:check
npm --prefix web run typecheck
npm --prefix web run build
```

lint 使用零警告门禁；hooks/refresh 约束保持启用，不增加数量预算。typecheck 覆盖 src 与 tests。当前 refresh 规则约束模块边界，不据此声称运行时已经验证热更新状态保留。

Playwright 用例位于 web/tests，标题描述用户可见行为，夹具放 support。新增交互覆盖加载、失败、取消、项目切换和草稿保留中与改动相关的分支；浏览器测试不代替真实模型验收。

前后端路径见[Web 与 Host](../web-integration.md)，检查前置条件见[测试索引](../../../tests/README.md)。
