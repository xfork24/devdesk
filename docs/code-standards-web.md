# Web（React / TypeScript）代码规范

与 Rust 侧相同的原则：**工具链默认值即标准**。ESLint flat config 采用 Vite react-ts 官方模板基线，Prettier 使用纯默认值；本仓库不维护第二套风格参数。

- Lint：`npm --prefix web run lint`（errors 必须为零；warnings 是可见建议，见下文"警告策略"）
- 格式：`npm --prefix web run format:check`（`prettier --check .`，必须全过；`npm run format` 一键修复）
- 类型：`npm --prefix web run typecheck`（`src` 与 `tests` 两个 project 都检查）
- 公开依据：[typescript-eslint 推荐规则](https://typescript-eslint.io/rules/)、[eslint-plugin-react-hooks](https://www.npmjs.com/package/eslint-plugin-react-hooks)、[Prettier 选项文档](https://prettier.io/docs/options)、[Playwright 最佳实践](https://playwright.dev/docs/best-practices)

## 命名

- **新组件文件 PascalCase**（`WorkflowController.tsx`）；**非组件工具模块 kebab-case**（`template-catalog.ts`、`runtime-installations.ts`）。存量混用文件（`configuration-ui.tsx`、`reading.tsx`）不改名，避免无谓 churn；新文件必须二选一正确
- 组件是 `function` 声明 + **具名导出**；唯一默认导出是 `MarkdownPreview.tsx`（`React.lazy` 按需加载），不新增默认导出
- Props 用 `type Props = { ... }` 别名（不写 `interface`、不写匿名内联大对象）；hook 一律 `use` 前缀
- 自定义 hook 命名与其文件保持一致；非 React 模块禁止 `use*` 命名（hooks 规则只对 `src` 生效，正是为了 Playwright 辅助函数不被误判）

## TypeScript

- **type alias 优先**，禁 `enum`（字符串字面量联合 + `as const` 替代）
- **唯一 any 逃生舱**是 `configuration-ui.tsx` 的 `Row = Record<string, any>` 与 `Request = (method, params?) => Promise<any>`（带 eslint-disable 与注释）。其他文件一律 `import { type Row, type Request }`，不重复定义本地别名；内核 JSON 形状由内核合同拥有
- 确需无形状值用 `unknown` + 窄化（参考 `WorkflowManager.replacePort`）
- 提示：`@typescript-eslint/no-explicit-any` 为 warn；**新增 warning 需在 PR 中说明**，积累会阻塞后续收紧
- 状态默认 `useState`；跨模块共享可变状态用模块级 store + `useSyncExternalStore`（参考 `template-catalog.ts`），不引入 Context/全局状态库

## React 模式

- 竞态防护沿用 generation-ref 模式：`const ticket = ++epoch.current;` 请求回来后核对（`WorkflowController.tsx`）
- 变更统一走 busy/error 协议：`setBusy(true); setError(""); try { ... } catch (e) { setError(String(e)); } finally { setBusy(false); }`
- 浮动 promise 显式 `void poll()` 或 `.catch(...)` 收尾；**禁止逗号运算符语句** `(a(), b());`——写成两条语句
- effect 依赖完整性（exhaustive-deps，warn 级）：刻意省略依赖属于审查关注点，确有理由时在行内注释说明

## 样式与可访问性

- 全局 `src/style.css`，语义化 kebab 类名（`panel`、`feature-row`、`modal-backdrop`），状态类用模板串拼接（`` `badge status-${value}` ``）
- **禁 inline style**（现存唯一一处是历史遗留，不新增）
- 交互元素优先语义标签 + ARIA（`role`、`aria-label`、`aria-selected`）

## 请求与异步

- 所有内核调用经 `main.tsx` 的 `rpc()`（统一超时分级、错误归一化）；组件只接收 `request: Request` prop，不直接 `fetch`
- SSE 用 `EventSource` + 去抖刷新；轮询用 `setTimeout` 递归并在卸载时停止

## 测试（web/tests）

- Playwright e2e；文件 kebab-case `*.spec.ts`，顶层 `test()` 扁平结构
- **标题是描述用户可见行为的中文句子**；定位用 `getByRole`/`getByLabel`，不写 CSS 选择器
- 每文件本地 `setup(page)` + `rpc()` 辅助；共享夹具放 `tests/support/`；规格文件允许 `any`（内核 JSON 行无本地形状合同），新代码不得在 `src` 沿用该放宽

## 警告策略与收紧路径

当前 75 个 warning 集中在三类：刻意省略的 effect 依赖（49）、共享模块组件+工具混导出（react-refresh，27 之外的 Row 类）与 Row 定义点。它们是**建议级**：本地与 CI 都可见但不阻断。规则：

1. 新增代码不得增加 warning 数量；PR 审查对照 `npm run lint` 输出
2. 触碰含 warning 的文件时顺手消除或注释说明
3. 中期目标：exhaustive-deps 逐文件清零后切换 `--max-warnings 0`

## 提交前自查

```bash
npm --prefix web run lint
npm --prefix web run format:check
npm --prefix web run typecheck
npm --prefix web run build
```

或直接 `./scripts/check/check-style.sh`。
