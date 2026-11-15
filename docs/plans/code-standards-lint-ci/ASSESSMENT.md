# 原方案与实施的独立评估

评估范围：`87a99c31bafc069efe7f3cec842ee975d62fb394..64a358a` 的六个提交、对应源码、原失败定位、完整检查入口与 GitHub Actions 实际日志。补充工作在 main 上进行，验收结果见 [ACCEPTANCE.md](ACCEPTANCE.md)。

## 结论与评估方法

建设方向合理，原来的“已完成”“零行为变更”“门禁全绿”结论证据不足。格式化、静态检查、类型检查、功能测试各自有边界；失败在旧提交复现，只能证明不是这批提交新引入，不能证明失败可以忽略，也不能证明整批改动语义等价。

本次以去除空白差异后的源码 diff 核对 Rust 的条件合并、Default 派生、引用简化与测试模块移动；核对 Web 的逗号表达式展开、未使用代码删除、类型别名迁移及组件回调；重新构建真实嵌入 Web 的 Host，复现失败并检查页面快照和 RPC；另外读取原提交实际 GitHub CI 失败日志。没有根据 ISSUES.md 中的根因推测直接改后端。

## 原方案哪些合理、哪些需要收紧

| 项目 | 独立判断 | 处理 |
|---|---|---|
| rustfmt 默认格式、固定 Rust 工具链、workspace clippy::all、CI `-D warnings` | 合理，工具所有权明确，没有必要额外引入 pedantic 或自定义格式争论 | 保留；补上 macOS 实际分支和双平台 CI |
| domain/core/host 禁 unsafe | 符合分层边界，adapters 的进程/系统接口需要单独审查 | 保留，不为了统一形式放宽边界 |
| 参数数量、DTO 枚举体积等定点 allow | 已有理由与边界，强制 boxing 或参数对象重构会扩大行为风险 | 保留定点豁免；不增加 workspace 全局豁免 |
| ESLint + typescript-eslint + hooks + Prettier | 适合当前 React/TypeScript 项目，但这套显式选取不是“全部默认推荐规则” | 文档准确列出启用范围；真正接入原来只安装未使用的 eslint-config-prettier |
| 清除 Web 显式 any | 原实施主要是把 any 集中到 Row/Request，any 仍会传播，不构成 RPC 类型安全 | 新显式 any 改为 error；两处边界豁免与规格测试豁免明确保留，文档不再宣称类型安全已完成 |
| RevisionSelect 的 `string \| string[]` 回调 | 比 any 表面上窄，但丢失了 multiple 与回调值的关系，调用方新增断言掩盖问题 | 使用判别联合，移除该批调用方的补偿断言；动态编辑器明确分支渲染 |
| Node 类型与运行环境 | 原实施使用 @types/node 26，运行基线却是 Node 24；添加该依赖还会让 Node 全局类型泄漏到浏览器源码 | 对齐 @types/node 24，浏览器 tsconfig 显式限定 vite/client，测试 project 单独启用 Node 类型 |
| 75 条 warnings 全部视为刻意模式 | 不准确：实际为 49 条 effect 依赖、26 条 refresh；Row 已豁免，不在计数内。effect 缺依赖可能有陈旧闭包，不能只凭 lint 输出认定无害 | 保留计划已记录的建议级策略，用数量预算拒绝增长；如实保留技术债，修复依赖需有行为证据，不能盲目加依赖引起循环请求 |
| 文档禁止逗号表达式，但未启用对应 lint | 实施不完整，ProjectManager 的 finally 中还漏了一处 | 启用 no-sequences（包括括号表达式），拆开遗漏点 |
| “只准 ok_or_else”“所有 DTO 都 deny_unknown_fields”“禁止所有 CSS 选择器/inline style”等 | 把局部惯例绝对化，部分与外部协议兼容性或动态 UI 的实际需要冲突；工具也没有实现这些禁令 | 改成有适用边界的审查规范，区分硬门禁与偏好；修正文档的错误链接、警告计数与夹具目录 |
| `check-style.sh` 直接运行 clippy，完整检查在此之后才构建 Web | 在已有 dist 的工作区会掩盖依赖，干净目录不能保证通过；完整入口也没跑测试 project 类型检查 | 风格入口先检查类型并构建 Web，再运行 clippy；完整入口去掉重复构建 |

75 的预算只是总量限制，不能识别同量替换，因此仍需要核对具体 warning。没有把这些存量告警变成静默 disable，也没有把“没有新告警”写成“全部告警已清零”。

## 未通过验收逐项重新判定

### A：经理面板同时存在产品问题和测试过时

折叠管理配置本身有合理目的，不能因为四个旧测试失败就把所有配置永久展开。但是运行原因、代次、等待条件和人工接管属于执行面，不应被配置折叠隐藏。修复将这些内容及沟通入口移到折叠区外，去掉重复的驱动原因；配置区继续折叠。恢复经理修订的测试显式展开配置后操作，慢轮询和配置读取失败测试继续验证运行面直接可见。

因此，没有照原报告的首个建议简单添加 `open`；产品与测试各修应修的部分。

### B：原报告“历史为空”的判断错误

在修改前重新运行原用例，失败快照明确存在 **“计划与执行历史 · 7 条”**。`getByText("计划与执行历史", { exact: true })` 无法匹配带计数的真实标题；后续“原合同”文本也已不属于当前历史阅读界面。不能由 locator 超时推出后端丢数据。

修复只调整测试：匹配真实 summary，比较修改前后历史增长，reload 后直接 RPC 比较完整历史，再展开面板核对每条记录可见。保留移动视口、冻结步骤复用和最终完成断言，没有通过删掉历史验收来取绿。

### C-1：环境会暴露失败，但根本问题是单元测试跨越了资源边界

本机当前有镜像时，原测试能够通过；这并不意味着依赖合理。该测试要证明 Role 修订不会改写旧 Agent 引用，却通过一个仅供测试的 Host 包装函数调用真实 session_resources，从而触发 Docker 构建。

改用相邻组合测试已使用的 ResourceFixture，仍验证真实 Engine 的组合和冻结计划不变量；删除只供该测试使用、没有生产调用方的包装函数。不是跳过测试，也不是让生产路径使用夹具。

### C-2：真实 Docker 浏览器验收应保留，先声明并准备外部依赖

MCP 用例实际测试 stdio 发现、精确工具授权、容器调用回执和历史结果，不能用伪造成功响应替代。保留真实 Docker。新增准备入口从 manifest/defaults 选出的不可变运行资源写入临时构建上下文，提前预热 Docker 缓存；应用仍自行注册并验证实际镜像。

这样构建或网络失败会在明确的准备步骤报告，而不是被浏览器 90 秒查找工具标题的超时掩盖。原报告基于失败模式把该项归为环境问题，未做单独对照，原证据不足以宣称其已验收。

### D：不能假设 Docker 恢复后必然消失

共享 Host/数据集会让运行准备状态跨规格保留；成功状态同样可能增加页面 `role=status`，并非只有失败才有污染。接入规格改用逐测试独立 Host 与临时数据集，复用执行布局规格已有的隔离机制并清理进程/目录。完整运行继续覆盖后续 authoring 用例，不能只用单独重跑替代整套验收。

## 原报告未发现的问题

1. **macOS clippy 失败**：credentials.rs 的 macOS 条件分支残留 needless_return。本机重新运行即失败，原来的全平台“清零”没有成立。修复该分支，并为 Rust workflow 增加 Ubuntu/macOS 矩阵。
2. **权限 Schema 门禁环境不一致**：原提交远端 rust run `43683740129` 的实际失败点是 `source-definition.schema.json` 相对引用无法解析。本机 Python 缺包则直接跳过 Schema，原报告的本地绿色不能代表同一检查。新增固定 Python 依赖、显式仓库内 Registry、缺依赖明确失败及非法 source envelope 的回归测试。解析不访问外网；做法依据 [jsonschema 的 Registry 文档](https://python-jsonschema.readthedocs.io/en/stable/referencing/)。
3. **e2e 构建顺序错误**：原 workflow 在 npm ci/Web build 之前编译 RustEmbed Host，依赖工作区偶然已有的 dist。调整顺序后从真实 head_sha 构建。
4. **e2e 前置判断不够精确**：原查询只按 SHA 选一条结果，没有区分 push/PR、最新 run/attempt；15 分钟轮询上限还可能短于首次 Rust 构建。改为每次完成事件核对同 SHA、同事件、同仓库的最新 rust/web 运行，最后完成者自然触发；pending/失败时不占 runner 等待。针对成功、缺失、失败、错误事件/提交/fork、较新失败及过时重跑事件补离线测试。
5. **fork 注释错误**：workflow_run 使用默认分支定义，不等于自动排除 fork。增加显式源仓库 guard；前置检查读取默认分支门禁脚本，测试 checkout 禁止持久化凭据，测试步骤不继承 GH_TOKEN。事件语义依据 [GitHub workflow_run 文档](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)，查询过滤依据 [workflow runs REST API](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-workflow)。
6. **CI 验收覆盖不足**：原 e2e 只跑 debug 浏览器，缺 HTTP/数据集/运行准备和 release。补齐 debug/release 的真实 Host 边界与 Chromium，并保存失败产物。

7. **首轮远端绿色仍有缓存收尾错误**：完整日志显示 setup-rust-toolchain 默认自带 Swatinem 缓存，原方案又显式运行了一次 rust-cache；两个 post 步骤重复清理 cargo/bin，Linux 后执行者报找不到 cargo，却仍返回成功。根据 [setup-rust-toolchain 的 action.yml](https://github.com/actions-rust-lang/setup-rust-toolchain/blob/v1/action.yml) 核对默认值后，关闭其内置缓存，仅保留显式 rust-cache；本项目不 cargo install 工具，另外关闭 cargo/bin 缓存，避免处理 rustup 管理的代理。以实际 post 日志复验，不只看 workflow conclusion。

## 交付判断边界

原有机械修改的抽查与逐类 diff 核对未发现需要回滚的业务变化；最终是否可交付，以修复后完整 Rust、HTTP、debug/release 浏览器和远端 CI 结果为依据。格式化成功、旧基线同样失败、单个用例重跑成功，均不单独作为整批改动正确的证明。详细结果和实际 CI 链接统一记录在 [ACCEPTANCE.md](ACCEPTANCE.md)。
