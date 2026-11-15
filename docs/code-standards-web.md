# Web（React / TypeScript）代码规范

与 Rust 侧相同的原则：Prettier 默认值负责格式；ESLint flat config 以 `@eslint/js`、typescript-eslint recommended 为基础，显式配置 hooks、refresh、受限 any 和逗号表达式规则，并由 eslint-config-prettier 关闭格式冲突。该配置是本仓库的选取，不等同于启用所有 React 推荐规则。

- Lint：`npm --prefix web run lint`（errors 与 warnings 均必须为零，`--max-warnings 0`）
- 格式：`npm --prefix web run format:check`（`prettier --check .`，必须全过；`npm run format` 一键修复）
- 类型：`npm --prefix web run typecheck`（`src` 与 `tests` 两个 project 都检查）
- 公开依据：[typescript-eslint 推荐规则](https://typescript-eslint.io/rules/)、[eslint-plugin-react-hooks](https://www.npmjs.com/package/eslint-plugin-react-hooks)、[Prettier 选项文档](https://prettier.io/docs/options)、[Playwright 最佳实践](https://playwright.dev/docs/best-practices)

## 命名

- **新组件文件 PascalCase**（`WorkflowController.tsx`）；**非组件工具模块 kebab-case**（`template-catalog.ts`、`runtime-installations.ts`）。存量混用文件（`configuration-ui.tsx`、`reading.tsx`）不改名，避免无谓 churn；新文件必须二选一正确
- 组件是 `function` 声明 + **具名导出**；唯一默认导出是 `MarkdownPreview.tsx`（`React.lazy` 按需加载），不新增默认导出
- Props 用 `type Props = { ... }` 别名（不写 `interface`、不写匿名内联大对象）；hook 一律 `use` 前缀
- hook 使用 `use` 前缀；普通工具避免以 `use` 加大写字母命名，防止被识别为 hook。测试辅助函数不受 React hooks 规则约束

## TypeScript

- type alias 与字符串字面量联合优先；只有需要运行时枚举对象时再评估 `enum`，这属于代码审查约定，并非已启用的 lint 禁令
- **唯一 any 逃生舱**是 `rpc-types.ts` 的 `Row = Record<string, any>` 与 `Request = (method, params?) => Promise<any>`（带 eslint-disable 与注释）。其他文件一律 `import { type Row, type Request }`，不重复定义本地别名；它们只标记尚未建立本地类型的 RPC 数据边界，不能证明返回值已通过类型或运行时校验；新组件的已知 Props、状态和查询结果应定义具体类型
- 确需无形状值用 `unknown` + 窄化（参考 `WorkflowManager.replacePort`）
- `@typescript-eslint/no-explicit-any` 为 error，测试规格目录有明确豁免。集中 Row/Request 仅减少重复别名，不等于消除了 any 传播
- 单选/多选组件通过 `multiple` 判别联合关联 value 和 onChange；调用方无需 `as string` / `as string[]` 来绕过编译器
- 浏览器 project 只启用 `vite/client` 全局类型；Node 类型限定于测试/配置 project，并与 Node 24 运行基线对齐
- 状态默认 `useState`；跨模块共享可变状态用模块级 store + `useSyncExternalStore`（参考 `template-catalog.ts`），不引入 Context/全局状态库

## React 模式

- 竞态防护沿用 generation-ref 模式：`const ticket = ++epoch.current;` 请求回来后核对（`WorkflowController.tsx`）
- 变更统一走 busy/error 协议：`setBusy(true); setError(""); try { ... } catch (e) { setError(String(e)); } finally { setBusy(false); }`
- 浮动 promise 显式 `void poll()` 或 `.catch(...)` 收尾；**禁止逗号运算符语句** `(a(), b());`——写成两条语句
- effect 依赖完整性（exhaustive-deps，warn 级并由零警告门禁阻断）：声明实际读取的依赖；加载只依赖项目 ID 时使用 ID 参数，不用整个项目对象；稳定回调显式列入依赖。不要通过省略依赖控制请求次数

## 样式与可访问性

- 全局 `src/style.css`，语义化 kebab 类名（`panel`、`feature-row`、`modal-backdrop`），状态类用模板串拼接（`` `badge status-${value}` ``）
- 静态样式使用 CSS 类；确需由运行数据计算的样式可使用行内样式或 CSS 变量，并说明必要性
- 交互元素优先语义标签 + ARIA（`role`、`aria-label`、`aria-selected`）

## 请求与异步

- 所有内核调用经 `App.tsx` 的 `rpc()`（统一超时分级、错误归一化）；组件只接收 `request: Request` prop，不直接 `fetch`
- SSE 用 `EventSource` + 去抖刷新；轮询用 `setTimeout` 递归并在卸载时停止

## 测试（web/tests）

- Playwright e2e；文件 kebab-case `*.spec.ts`，顶层 `test()` 扁平结构
- 标题描述用户可见行为，新增标题优先中文；定位优先 `getByRole`/`getByLabel`，必要时使用稳定结构选择器，避免依赖偶然的文案或层级
- 每文件本地 `setup(page)` + `rpc()` 辅助；浏览器共享夹具放 `web/tests/support/`；规格文件允许 `any`（内核 JSON 行无本地形状合同），新代码不得在 `src` 沿用该放宽

## 警告策略与模块边界

`lint` 使用 `--max-warnings 0`，hooks 和 refresh 规则保留启用，不设置数量预算或新的行内豁免。Row/Request 原有的两条 any 豁免保留在独立 RPC 类型边界；它们与 hooks/refresh 警告无关。

组件模块只导出组件；非 JSX 状态、hooks、导航和规范化函数分别放在 `action-feedback.ts`、`configuration-state.ts`、`reading-state.ts`、`kernel-address.ts`。`main.tsx` 只负责挂载，应用放在 `App.tsx`。当前 Vite 没有启用 React Refresh 插件，refresh 规则在这里约束模块边界，不能据此声称已经验证 React 状态热保留。

effect 必须核对实际状态语义：稳定函数可以直接作为依赖；派生数组用稳定输入推导，避免每次渲染重新触发请求；编辑器只消费一次初始路由，后台刷新不能重置草稿。请求序号 ref 用于隔离迟到响应，清理显式失效；仅属于一个 effect 的请求也可用局部 active 标记。修复依赖时检查会话身份、取消、浏览器前进后退和草稿保留，不能机械追加整个对象。

## 检查与测试边界

`check-style.sh` 包含 lint、两个 TypeScript project、Prettier 与 Web 构建，然后执行 Rust clippy；`check.sh` 继续执行 Rust、HTTP/数据集/环境准备和 debug/release Chromium。MCP 的浏览器用例实际使用 Docker，首次构建所选运行资源需要镜像/包源网络；不调用付费模型。接入类浏览器测试使用独立 Host 和临时数据集，运行准备状态不能泄漏给其他用例。纯 Rust 组合测试使用显式端口夹具，真实 Docker 协议测试保持单独分层。

## 提交前自查

```bash
npm --prefix web run lint
npm --prefix web run format:check
npm --prefix web run typecheck
npm --prefix web run build
```

或直接 `./scripts/check/check-style.sh`。
