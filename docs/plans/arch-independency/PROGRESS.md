# 执行进度报告

本文件是[改造计划](PLAN.md)实施至今的进度总览，按阶段列出已完成与待做的工作项。每项的详细验收命令与结果见[执行报告](EXECUTION.md)；两份文件的结论对应提交时的工作区。

状态截至 2026-11-28，分支 `arch-independency`，起点 `main@e84e2218`。

## 总览

计划的 61 个工作项中，**30 个已完成并验收提交**，31 个待做。P0 与 P1 两个阶段的门禁已通过；P2 完成了 WS5 的前两项；P3、P4、P5 尚未开始。

| 阶段 | 工作项 | 已完成 | 待做 | 门禁 |
|---|---|---|---|---|
| P0 守卫与基线 | G-1、G-2、G-4、G-5 | 4/4 | 0 | 门禁 0 通过 |
| P1 协议核心 | G-3、K-1~K-7、N-1~N-8、S-1~S-5、W-5、D-1 | 21/21 | 0 | 门禁 1 通过 |
| P2 Web SDK、UI 迁出与测试改造 | W-1~W-4、W-6~W-9、T-1~T-6、S-6、D-1 | 2/17 | 15 | 未运行 |
| P3 第二个领域 | E-1~E-9、D-1 | 0/10 | 10 | 未运行 |
| P4 工程扩展模块迁移 | M-0~M-6、N-9、D-1 | 0/8 | 8 | 未运行 |
| P5 收尾 | D-2、D-3 | 0/2 | 2 | 未运行 |

## P0 守卫与基线 — 已完成

| 工作项 | 提交 | 内容 |
|---|---|---|
| G-4 | `ec2221a6` | 内核测试执行器改用 `tests/fixtures/reporters/` 的冻结报告器 |
| G-1 | `116dd939` | `check-architecture.py` 增加内核与领域包的边界守卫 |
| G-2 | `6fee45bc` | `check-web-boundaries.mjs` 对照基线检查 Web import 边界 |
| G-5 | `a335547f` | 平台处理器数量修订与文档对齐 |

门禁 0 通过。完整检查通过，唯一例外是 `prepare_browser_runtime.py` 的 Docker Hub 拉取（网络限制，镜像本地已有，剩余步骤补跑通过）。

## P1 协议核心 — 已完成

| 工作项 | 提交 | 内容 |
|---|---|---|
| G-3 | `a338bfe0` | 移除 core 对六个单体时代角色标识的归档逻辑；架构检查例外清空 |
| N-8 | `2d4d54ca` | 验证结论：无仓库项目的 Agent 步骤可直接运行 |
| K-5 | `7778ac9a` | 工程钩子移入 `extensions`，按内核登记的扩展组绑定 |
| K-1~K-3 | `3dcf5e3f` | 清单格式 2：`domain_protocol`、`requires`；能力目录；安装期协商 |
| K-4 | `dd90f4ab` | 领域包的配置包清单在安装时套用出厂 Schema |
| W-5 | `5cc4a703` | 呈现目标 `sdk`/`kit`/`home`；Web Core 版本校验 |
| N-6 第一部分 | `f8d4a8f0` | 内核输入更名为 `engineering.git.head`/`engineering.git.snapshot` |
| N-3 + N-6 第二部分 | `ea7fbd35` | `query.domain.entities` 对象列举；内核输入 `domain.entities` |
| N-5 | `d98707a8` | 处理器入口改为纯模块 + 内核引导模块；结构化错误传回 |
| N-1 | `daebdb7c` | 领域工具一等化（`implementation: "domain"`）；移除 `domain.read` |
| N-7 | `bd2e40d4` | 工作流输入声明 `provided_by`（扩展组提供） |
| N-4 | `97fcceaa` | 项目接入信息对内核不透明；`domain.project.remove` 通用化；工程库 v63 |
| K-7 | `efe9d88f` | 版本 0.6.0；兼容矩阵更新 |
| N-2 | `20f2a9bf` | 通用证据门禁：按操作名守护、合同声明 context、完成前询问验证器 |
| K-6a | `ab0b839e` | `plugins/core` 改名为 `kernel/core` |
| K-6b | `68ec9041` | 工具定义随提供者发布：`kernel/core/tools` 43 个 + `kernel/engineering/tools` 8 个 |
| S-1~S-5 | `057b655a` | SDK 制品 `sdk/domain/`、`devdesk-domain` CLI（init/check/build/verify）、声明式软件包、CI 独立构建作业 |

每项各自跑过初步验收（typecheck、lint、三个构建、合同、完整 workspace 测试、Docker/浏览器端到端）。

**P1 阶段门禁**（`check.sh`）：60 个套件 808 通过、0 失败；debug 浏览器 159 通过、release 160 通过；release 夹具 Host 六组合同通过。唯一例外是 `docker pull` 连不上 Docker Hub（与 P0 相同），镜像本地已有，剩余步骤补跑全部通过。

## P2 Web SDK、UI 迁出与测试改造 — 进行中（2/17）

### 已完成

| 工作项 | 提交 | 内容 |
|---|---|---|
| W-1 | `de719983` + `8c6c6659` | `web/src` 分成 `sdk/`（稳定外壳契约：领域上下文、请求边界与缓存失效状态、布局、操作反馈、存储设置插槽、共享助手）、`kit/`（内核概念 UI：配置编辑器与状态、工作流视图与经理组件、运行时/接入/模板/权限设置、阅读与 Markdown 助手）、`core/`（工作台外壳：App、核心页、领域工作台外壳、入口、样式、浏览器兼容）。ESLint 边界错误级：SDK 只 import 自己，kit 加 SDK，core 加两者 |
| W-2 | `b2090176` | kit 入口输出 `assets/devdesk-kit.mjs`，与 `assets/devdesk-ui.mjs` 并列；类型声明覆盖两个入口 |
| 文档 | `496976ab`、`22acfeda` | `docs/crates/web-integration.md` 链接更新到新布局 |

### 待做

| 工作项 | 内容 | 风险 |
|---|---|---|
| W-3 | `useDomain()` 的目标能力；统一请求边界；移除方法名特判和投影贡献对领域请求的拦截 | M |
| W-4 | 插槽组件：数据目录设置、项目操作、项目选择、核心导航项；删除 DOM 探测与 `MutationObserver` | M |
| W-6 | 设计令牌首批（`--devdesk-` CSS 自定义属性） | S |
| W-7 | 28 个领域文件迁入 `plugins/software-development/web/src/`；48 个共享模块归入 SDK 或 kit（W-1 已完成归类）；`App` 的双模式删除 | L，最高风险 |
| W-8 | 领域 UI 自带 `package.json`、Vite、TypeScript、ESLint 配置；清理 `web/` 中的领域配置 | M |
| W-9 | 核心样式表移出领域选择器；领域样式加前缀 | M |
| T-1~T-6 | 测试改造：单体模式退役（移除 `fixture-catalog`、`build:fixture`）；浏览器用例改在正式制品上运行；`native_acceptance.py` 去除 `fixture-catalog` 依赖 | L |
| S-6 | CI 增加独立构建作业：只有 SDK 制品和领域目录 | 已随 WS4 提前完成 |
| D-1 | 实施中发现的每个抽象缺口回写契约或登记 | 持续 |

**门禁 2 要求**：Web Core 产物 import 图不含领域目录；领域 UI 只依赖 SDK 制品；无 DOM 探测与 `MutationObserver`；单体模式移除；浏览器用例在正式制品上 debug 与 release 各通过一遍。

### 执行报告记录的教训

机械批量改写 import 经历四轮才收敛，教训已写入[执行报告](EXECUTION.md)：按文件名（含扩展名）建立旧→新映射，逐条重写后统一剥离扩展名。W-3 起改为逐文件手工改写。

## P3 第二个领域 — 未开始

10 个工作项（E-1~E-9、D-1）：按[电商出库领域设计](DOMAIN-ecommerce-outbound.md)用 `devdesk-domain init` 创建第二个领域包，7 种对象类型、13 个处理器、3 个流程、3 个页面，验证内核抽象的完备性。`devdesk-domain` CLI 和 SDK 制品（P1 S-1~S-5）已就绪。

## P4 工程扩展模块迁移 — 未开始

8 个工作项（M-0~M-6、N-9）：把工程扩展（`tests.*`、`git.*`、`workspace.*` 处理器）从内核 crate 迁到独立扩展模块，Feature、Task 从内核类型变为软件开发包的对象类型，新增通用平台工具 `entities.create`。M-0 的设计契约先行。

## P5 收尾 — 未开始

2 个工作项（D-2、D-3）：设计文档与实现一致；发布流水线产出五种制品。

## 关键提交索引

| 提交 | 阶段 | 工作项 |
|---|---|---|
| `b0b249c9` | P0 | 门禁 0 |
| `ec2221a6`~`a335547f` | P0 | G-4、G-1、G-2、G-5 |
| `a338bfe0` | P1 | G-3 |
| `2d4d54ca` | P1 | N-8 |
| `7778ac9a` | P1 | K-5 |
| `3dcf5e3f` | P1 | K-1~K-3 |
| `dd90f4ab` | P1 | K-4 |
| `5cc4a703` | P1 | W-5 |
| `f8d4a8f0` | P1 | N-6 第一部分 |
| `ea7fbd35` | P1 | N-3 + N-6 第二部分 |
| `d98707a8` | P1 | N-5 |
| `daebdb7c` | P1 | N-1 |
| `bd2e40d4` | P1 | N-7 |
| `97fcceaa` | P1 | N-4 |
| `efe9d88f` | P1 | K-7（0.6.0） |
| `20f2a9bf` | P1 | N-2 |
| `ab0b839e` | P1 | K-6a |
| `68ec9041` | P1 | K-6b |
| `057b655a` | P1 | WS4（S-1~S-5） |
| `753e4b6f` | — | 合并 main（双语 README 0.6.0） |
| `de719983` | P2 | W-1 |
| `8c6c6659` | P2 | W-1 基线记录 |
| `b2090176` | P2 | W-2 |
| `496976ab`、`22acfeda` | P2 | 文档链接 |
| `01ff6c2d` | P2 | 执行报告 |
| `9c4f1bd7`、`302f2303`、`34f31ec5` | P1 | 门禁结果与状态 |
