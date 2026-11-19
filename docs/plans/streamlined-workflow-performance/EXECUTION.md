# 出厂退役与本轮执行

日期：2026-11-19。源码基线 `ab7bc628389fd57ff0c7e29e6f4f1759fc1938df`，起始工作区干净。详细版清理、精简版基准及用户追加要求的两项故障修复属于本轮执行；[新性能计划](PLAN.md) 留待后续实施。旧计划及历史验收保持原文。

## 退役范围

以当前 manifest 中全部确切源引用建立正反向图，从详细版 Standard、Feature/Task 与六类原 Codex Agent 出发计算闭包；再保护默认选择、全部保留源及其传递依赖。闭包 146 项，其中 80 项仍被其他配置使用，实际退役 66 项：

| 类型 | 移除数 |
|---|---:|
| Standard / Workflow | 1 / 2 |
| Agent / Capability | 6 / 6 |
| Delivery | 16 |
| Permission Profile | 16 |
| Skill / Spec | 1 / 12 |
| Tool | 6 |

同时移除 13 份只被退役源使用的正文；出厂源总数 243→177。发布标签为 `2026.11.19-streamlined-only-v1`，重新生成反向引用索引。精简版默认 Standard `simplified-standard-engineering:3`、Feature `simplified-workflow-feature:3`、Task `simplified-workflow-task:2` 及 Agent/权限/模型选择保持原引用。DSH 等保留栈仍引用的通用 Skill、Spec、Role 和工具继续发布。

旧源正文内联到 `tests/fixtures/configuration/retired-engineering.json`，由历史合同、旧平台工具及出厂更新回归显式装入隔离测试库；生产 manifest 和构建资源目录均不包含它。正常默认测试改为精简版 Agent，新增退役源在新库中不存在且精简闭包仍可编译的回归。出厂更新回归保留一份真实旧源并核对更新前后内容一致。

未删除或迁移用户数据、历史冻结源、已有项目选择；新默认不会偷偷替换旧工作的执行依据。退役详细版原生执行入口及精简探针的 detailed 选择，当前文档指向精简入口；旧 Standard 历史探针按原版证据用途保留。

## 基准入口

专用证据根目录 `/tmp/devdesk-streamlined-baseline-20261119-NRBdi5`。在测试仓库 `xfork24/devdesk-default-workflow-test-20261118` 创建独立目标分支 `devdesk/baseline-20261119`，从初始提交建立无延迟的小型 Node 工程，加入 `package.json`、一个既有函数及其回归。基线提交 `a0d68f2`；实际完整 SHA、源/二进制输入及测试输出保存在目录中。

准备时对仓库现有 main 的检查发现旧验收 `tests/money/money.test.mjs` 含 90 秒人为等待。两次本地准备检查已停止并保留日志，未将旧场景作为正式基准，也未改写远端 main。独立分支的无延迟基础回归通过后才启动 Feature。

使用新建专用数据集、真实 Codex、Docker、Git/GitHub、当前精简版 Standard 和自动发现的 `npm test` 项目入口。探针只观察和保存确切 Human 材料，不代替经理派发/接纳。开启 `--performance`；二进制冻结后运行期间不重建或替换。新增审批只读诊断在设计确认时因缺少直接上游 PRD 链接异常退出，导致探针拥有的 Host 停机；已修正为保存诊断、持续观察，不自动批准。以相同二进制、数据集、根实例和已完成步骤重启，通过正常经理 takeover/resume 恢复。核对完整冻结 PRD、设计与评审后，按本轮用户明确授权签署确切版本；没有代替经理派发/接纳。这段维护时间及原异常全部保留。完整结果、限制与性能分解见 [BASELINE.md](BASELINE.md)。

## 验证与已知失败

出厂文件/正文归属及引用自洽检查通过；架构检查通过；审批观察器 13 项通过。首轮编译发现本轮新增测试路径缺少 `crate::`，已修正。第二轮 workspace 只有新增“保留旧源但不再属于当前目录”的断言失败：现有 preserve_custom 合同保留旧选择，不能将出厂退役解释为删除历史目录成员，因此移除该错误断言，继续核对旧源内容完整保留。原失败日志分别为 `/tmp/devdesk-retirement-rust.log`、`/tmp/devdesk-retirement-rust-v2.log`。

初始完整 Rust 回归与需求步骤重叠 46.583 秒，基准报告保留该干扰范围，不将本轮当作独占资源的正式性能认证。最终检查均在业务 complete 后运行。


## 本轮追加故障修复

完整基准完成后，用户明确要求修复首次评审定位错误和审批诊断导致 Host 停机两项问题，后续未另开付费完整 Feature。

1. 首次 PRD 为 63 行，模型报告引用到 65 行，且没有预先 lint。默认自检 Skill 发布第 3 修订，明确普通文档免 lint 不适用于 content_checks 报告，要求使用本步骤精确 Delivery 调用 spec.lint，通过结构与冻结证据检查才最终提交。六类 Capability/Agent 发布第 4 修订，更新真实默认绑定，旧修订在已有数据集和历史快照保留；Standard/Feature/Task 引用及业务门禁不变。当前包标签为 `2026.11.19-located-review-preflight-v1`，仍为 177 源。
2. 原生报告 lint 复用接纳的冻结输入和检查器，返回 content_report_sources：确切源/摘要、实际行数与 Unicode 字符数、最多八份有界章节定位及截断标记，不重复文档正文；越界错误包含实际边界。报告可在同一 Turn 校正并重新 lint，接纳仍拒绝伪造来源、摘要、越界或不匹配引用。未改冻结任务渲染协议，不使旧准备输入因新增元数据而失效。
3. 共享 observe_human_request 将诊断失败作为显式结果返回；先持久化原件和诊断，再继续观察，不调用业务控制或代签。精简版和子审批探针都采用它。重启端口变化不改写原始请求/归档 URL，来源、输入或输出变化仍被识别，非法路径拒绝。

定向结果：16 个审批观察器回归通过；两个领域定位指导测试通过，覆盖 CRLF/Unicode、截断及正文不复制；原生工具桥+真实 SQLite 定向场景通过，复现 63→65 越界，第一次 lint 拒绝、依指导修正后第二次 lint 通过，在原 Turn 完成接纳，只有一次 Agent dispatch、没有重试 execution。已有冻结来源/权限和草稿不落产物的负向回归继续保留。这是确定性协议验证，不冒充修复后付费模型全流程成绩；模型仍须遵守已发布自检政策，结构与来源门禁负责拒绝未遵守的输出。


## 最终检查与保留项

| 检查 | 结果 |
|---|---|
| Rust workspace | 668 通过，0 失败，34 项按既有显式条件忽略 |
| 定位修复 | 包含上述 Rust 整轮中的两个纯领域回归及一个真实 SQLite/生产工具桥同 Turn 修正场景 |
| 审批观察器 | 16 通过，覆盖诊断失败继续观察、原件与重启端口及目录逃逸边界 |
| 风格/构建 | rustfmt、clippy、ESLint、TypeScript、Prettier、Web 构建、release 构建通过；最新测试断言另做 Web 静态复核 |
| 架构/出厂 | 架构、源/文件/正文归属及反向索引自洽通过 |
| release Host | 生命周期、SSE/event-tail、HTTP/认证/连接、数据集、运行准备通过 |
| 出厂更新/重置入口 | 通过；原入口把显式破坏性 reset 误作历史归档，按当前合同区分 preserve_custom 更新与破坏性 reset，没有修改产品重置语义 |
| release Chromium 全量 | 首轮 139 项：129 通过、10 失败；配置清理涉及的六项断言适配当前资源后定向复核，六项全部通过；其余四项保留 |

首轮浏览器原始失败与截图在 `browser.log`、`browser-results/`，没有删除。退役资源相关断言改用保留的 DSH Capability、精简版测试 Delivery/Agent 和当前 Sandbox；界面行为未因此修改。

其余四项保留并未扩入本轮修复：运行计划追加场景出现 catalog_clock 版本冲突（未进一步区分夹具时序和界面处理）；一个共享数据集场景含之前测试创建的额外 Sandbox，唯一数量断言失败；旧 Standard 页面文字断言已不匹配当前新版本编辑器；替换下拉框 324 px 超出该测试自身的文字宽度+32 px 上界约 2.6 px。不能把首轮或整体浏览器记为全部通过，也未据此宣称全工作台放行。两项本次实际中断问题及完整业务交付、数据/版本/权限门禁分别判定。

最终候选 Host SHA-256 `6a696cbce2cfcf73a7ee3460a9ac41be014989c40be6c17b82f4b433a352c418`；运行基准仍使用 BASELINE 中的原冻结 SHA。最终一致性快照验证 233 个注册 Artifact，49 个工作者 released、登记容器无运行残留、两库完整性/外键通过，已删除 15 份临时认证副本。没有提交运行数据、原始认证或临时 JSON；本轮未做 Git 提交或合入。
