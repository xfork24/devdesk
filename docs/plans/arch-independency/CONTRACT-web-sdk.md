# 目标 Web SDK 契约

| 项 | 内容 |
|---|---|
| 状态 | **待实施**。本文是目标契约，不是当前已实现的接口 |
| 日期 | 2026-11-26 |
| 所属计划 | [架构独立性改造](PLAN.md) |
| 改造前的事实 | [Web 嵌入基线](BASELINE-web-embedding.md) |

## 1. 原则

1. **核心拥有外壳，领域拥有内容。** 文档、路由外壳、布局骨架、主题、请求边界、事件、会话、错误边界、生命周期属于 Web Core。页面内容、领域导航项、领域路由参数、领域样式属于领域。
2. **两者只经 SDK 交往。** 领域 UI 不 import Web Core 的源码，不读写 Web Core 的 DOM，也不依赖核心类名的内部结构。
3. **插槽显式化。** 核心要放进领域页面的东西，由 SDK 以组件形式交给领域渲染。
4. **约束由工具保证。** import 边界、样式前缀、入口形态由 lint 与类型检查执行。
5. **不做兼容。** 不保留 DOM 探测、`api: 1` 与 `api: 2` 两套渲染、按文件名匹配的别名。软件开发包随 Web Core 重发。

## 2. 两个入口

基线中名义 SDK 只有 4 个模块，另外 48 个核心模块被领域直接编译。把这 52 个共享模块按稳定性分成两层：

| 入口 | 运行时路径 | 内容 | 版本 |
|---|---|---|---|
| `@devdesk/web-sdk` | `/assets/devdesk-ui.mjs` | React 本体、上下文、布局、插槽组件、请求与事件、反馈组件、设计令牌、类型 | `web_sdk` 主版本，独立于 Web Core |
| `@devdesk/web-kit` | `/assets/devdesk-kit.mjs` | 内核概念的通用界面：配置编辑器、工作流视图、运行时设置、接入、权限 | 与 Web Core 同版本 |

分成两个入口的原因是变化速度不同。外壳契约应当很少变化，可以给领域一个稳定的目标；配置编辑器和工作流视图随内核功能演进，把它们放进 SDK 会让 SDK 无法稳定，不让领域使用又要各自重写一遍。

领域源码中两者都是普通的包名，由构建配置重定向到运行时路径并设为 external：

```ts
import { DomainLayout, useDomain } from "@devdesk/web-sdk";
import { WorkflowInstanceView } from "@devdesk/web-kit";
```

## 3. 清单声明

```json
"ui": {
  "contributions": "ui/contributions.json",
  "targets": [{
    "renderer": "web.react",
    "sdk": 1,
    "kit": "0.6",
    "styles": ["ui/web/outbound.css"],
    "bindings": [
      { "contribution": "ecommerce-outbound.orders", "entry": "ui/web/outbound.mjs", "export": "OrderQueue", "home": true },
      { "contribution": "ecommerce-outbound.fulfillment", "entry": "ui/web/outbound.mjs", "export": "FulfillmentWorkbench" }
    ]
  }]
}
```

| 字段 | 含义 |
|---|---|
| `sdk` | 必填。与 Web Core 的 `web_sdk` 主版本一致才加载 |
| `kit` | 可选。使用了组件套件时填写所依赖的 Web Core 次版本，不一致时不加载 |
| `bindings[].home` | 可选。标记“领域工作台”首页，取代基线中的“第一个绑定” |
| `styles` | 路径规则由渲染器自己描述，不再写进通用 Schema |

版本不匹配时页面区域显示“领域包与 Web Core 版本不兼容”，导航与领域管理保持可用。

## 4. 契约条目

**W1 入口。** 领域 UI 是 ES 模块。每个绑定指向的导出必须是 React 组件，不接受必填 props。模块顶层只允许定义导出。

**W2 模块来源。** 领域只能 import 三类模块：`@devdesk/web-sdk`、`@devdesk/web-kit`、自己包内的模块。React、ReactDOM 与全部 Hooks 必须来自 SDK。其他 npm 依赖可以打包进领域包。

**W3 上下文。** 页面经 `useDomain()` 取得全部能力：

| 成员 | 含义 |
|---|---|
| `epoch`、`contribution` | 当前代次与贡献 |
| `project` | 当前项目及其切换方法。由核心持有并写入 URL |
| `route` | 领域自己的查询参数读写 |
| `request` | 与核心页面是同一条请求边界：超时、错误格式、缓存、失效、配置目录代次 |
| `invoke` | 调用领域处理器，结构化错误原样抛出 |
| `events` | 事件流，绑定当前代次与项目 |
| `navigate` | 前往另一贡献或核心页面 |

基线中对具体方法名的特判全部移除：`query.agent_revision`、`project.open_github`，以及投影贡献对领域页面请求的拦截。投影贡献只服务于通用呈现的客户端。

**W4 布局与插槽。** 页面根必须是 `DomainLayout`，导航由它送入核心侧栏。核心提供的插槽改为 SDK 组件，由领域在自己的页面里渲染：

| 组件 | 内容 | 基线中的做法 |
|---|---|---|
| `DomainStorageSection` | 领域数据目录设置 | 探测 `data-domain-storage-outlet` |
| `DomainProjectActions` | 移除项目等核心操作 | 探测类名、属性和按钮文案 |
| `ProjectSelector` | 当前项目选择与新建 | 领域自己实现，核心从它的下拉框读项目 |
| `CoreLinks` | 通往基础组件、历史记录、设置、领域管理的导航项 | 领域重复一份 |

Web Core 不再探测领域 DOM，不再使用 `MutationObserver`，不再需要知道领域的类名和文案。

**W5 路由。** 核心拥有 `view` 与 `project_id`。领域只能经 `route` 读写其余参数，不直接访问 `location`。

**W6 样式。** 领域选择器必须带领域前缀，前缀在 `domain.json` 中声明。只能使用 SDK 文档化的设计令牌与组件类，不得改写核心类的规则。核心样式表不含任何领域的专有选择器。

**W7 生命周期。** 页面在切换 `view`、代次变化、领域停用时被卸载，核心以 `key` 保证重新挂载。页面负责清理自己的定时器、订阅和观察器；在途请求在卸载时中止。

**W8 失败隔离。** 页面的渲染错误只影响页面区域。页面不得覆盖核心的认证、数据集和领域管理状态。

**W9 数据。** 领域页面与核心页面共用同一个请求边界和同一份缓存实例。基线中两个 bundle 各持一份模块级状态的情况随重复编译一起消失。[Web 开发规范](../../crates/standards/web.md)对草稿、版本冲突和刷新的要求同样适用于领域页面。

**W10 构建。** 领域 UI 随领域包构建，构建配置由 SDK 提供。`web/` 下不再有针对具体领域的配置、类型检查范围或 npm script。

**W11 信任。** 领域 UI 与核心同源、同权限执行，领域包被视为可信代码。这一点写进 SDK 文档。不可信领域的隔离是另一个设计。

**作者清单。** 上面的条目落成领域页面作者可以直接对照的做法：

| 应该 | 不应该 |
|---|---|
| `import { ... } from "@devdesk/web-sdk"` | import `web/src` 下的任何路径 |
| 用 `useDomain()` 取项目、请求、事件 | 自己 `fetch("/rpc")` |
| 用 `<DomainLayout>` 包住页面 | 查询或依赖核心的 DOM 结构 |
| 样式选择器带领域前缀 | 改写 `.panel`、`.sidebar` 等核心类 |
| 经 `route` 读写领域参数 | 直接读写 `location.search` |
| 在卸载时清理副作用 | 在模块顶层执行代码 |
| 使用 SDK 的类型，缺类型时向 SDK 提需求 | 自行复制核心模块或类型 |

**入口示例。**

```tsx
import {
  DomainLayout,
  DomainProjectActions,
  DomainStorageSection,
  useDomain,
} from "@devdesk/web-sdk";

export function Overview() {
  const { project, request } = useDomain();
  return (
    <DomainLayout navigation={<WorkbenchNavigation />}>
      <BranchQueue project={project.id} request={request} />
    </DomainLayout>
  );
}

export function Settings() {
  return (
    <DomainLayout navigation={<WorkbenchNavigation />}>
      <section className="sd-settings">
        <DomainStorageSection />
        <EngineeringSettings />
      </section>
    </DomainLayout>
  );
}
```

与改造前的 [entry.tsx](../../../plugins/software-development/web/entry.tsx) 相比，差别只有两点：全部依赖来自 SDK，核心的插槽组件由领域显式渲染。

## 5. 设计令牌

SDK 文档化一组以 `--devdesk-` 开头的 CSS 自定义属性，覆盖颜色、间距、圆角和字号。它们从核心现有样式中的重复取值提取，首批只保证领域页面实际需要用到的那些。领域样式只能引用令牌，不复制核心的具体数值。

## 6. Web Core 的内部结构

`web/src` 分成三个互不越界的目录：

| 目录 | 内容 | 允许 import |
|---|---|---|
| `web/src/sdk/` | SDK 入口 | 只限本目录 |
| `web/src/kit/` | 组件套件 | 本目录与 `sdk/` |
| `web/src/core/` | 工作台外壳 | 本目录、`sdk/`、`kit/` |

核心包的构建守卫从三个文件的黑名单改为白名单：核心包的 import 图不得离开 `web/src`。领域包的 lint 保证它不 import `web/src`。

## 7. 领域 UI 的目录与构建

领域 UI 是领域包的一部分，位于领域的 `web/` 目录：

```text
plugins/<id>/web/
  package.json        # 依赖 @devdesk/domain-sdk 与自己的第三方依赖
  vite.config.ts      # import { createDomainConfig } from "@devdesk/domain-sdk/vite"
  tsconfig.json       # extends "@devdesk/domain-sdk/tsconfig.web.json"
  eslint.config.js    # SDK 提供的边界与样式规则
  src/
  <entry>.tsx
```

`@devdesk/domain-sdk` 是一个 npm 包，就是[内核 SDK 契约](CONTRACT-kernel-sdk.md)第 10 节的 SDK 制品。对领域作者来说，Schema、命令行工具、处理器的类型、Web 的类型和构建配置都来自这一个包。

| 导出 | 用途 |
|---|---|
| `@devdesk/domain-sdk/vite` | `createDomainConfig()`，生成 Vite 配置：重定向两个入口、设为 external、输出 `workbench.mjs` 与分块 |
| `@devdesk/domain-sdk/tsconfig.web.json` | 领域 UI 的 TypeScript 基线 |
| `@devdesk/domain-sdk/eslint` | import 边界、样式前缀、入口形态的 lint 配置 |
| `@devdesk/domain-sdk/worker` | 处理器的类型声明 |
| `@devdesk/domain-sdk/schemas` | JSON Schema |

SDK 的绝对运行时路径由 `createDomainConfig({ basePath })` 的参数决定，默认是 `/assets/`。

## 8. 测试

单体模式移除后，浏览器用例运行在正式制品上：正式 Host、正式 Web Core、运行时安装的领域 ZIP。

| 用例类别 | 使用的领域包 | 位置 |
|---|---|---|
| 内核 Web 外壳与通用界面 | 测试自建的最小领域包，沿用 [tests/fixtures/domain-packages](../../../tests/fixtures/domain-packages/build.py) 的构建器 | `tests/browser/kernel/` |
| 软件开发界面 | 软件开发 ZIP | `tests/browser/software-development/` |
| 电商出库界面 | 电商出库 ZIP | `tests/browser/ecommerce-outbound/` |

测试引导提供安装与激活领域包的公共步骤，每个用例声明自己需要的领域。内核的用例不再依赖软件开发包的配置内容。

## 9. 与基线的对照

| 基线中的隐式契约 | 目标 |
|---|---|
| C1 入口是 ES 模块，导出是函数 | W1，加类型约束 |
| C2 React 必须来自 SDK | W2，按包名重定向 |
| C3 上下文经 `useDomain()` | W3，能力完整，领域不再需要核心的其他模块 |
| C4 `api` 的两种渲染 | 移除，只有一种 |
| C5 领域必须自带回到核心的导航 | W4，`CoreLinks` |
| C6 首页是第一个绑定 | 清单显式标记 |
| C7 双方各自读写 URL | W5 |
| C8、C9 请求路径不同且有方法名特判 | W3、W9 |
| C10 事件 | W3 |
| C11 全局样式无作用域 | W6 |
| C12、C13 DOM 探测插槽 | W4 |
| C14 生命周期靠约定 | W7 |
| C15 没有版本校验，路径写死 | 第 3 节，第 7 节 |
| C16、C17 资源与类型 | SDK 提供类型 |
| C18 信任前提未写明 | W11 |
| 48 个核心模块被重复编译 | 两个入口 |
| 28 个领域文件位于 `web/src` | 第 7 节 |
| 构建配置位于 `web/` | W10 |

## 10. 留到实施时确定

- 52 个共享模块逐一归入 SDK、组件套件还是领域，在迁移时按“是否外壳契约”判定，首批结果记录在执行报告里。
- 设计令牌的首批清单。
- `ProjectSelector` 中“新建项目”的领域自定义入口形态。
- 组件套件是否对外承诺稳定。首版只承诺给仓库内的领域使用。
