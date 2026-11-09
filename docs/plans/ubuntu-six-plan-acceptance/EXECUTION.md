# Ubuntu 六项改造验收报告

日期：2026-11-10。**结论：按用户最终确认的“Codex 完整闭环跑通即可通过”标准，本轮验收通过。** MiniMax 的模型输出限制单独保留，不阻断此结论。

## 环境与交付

Ubuntu 24.04.5 LTS / x86_64，Rust 1.99.1、Node 24.13.0、Docker Engine 29.9.3。源码起点 `0b13778da7b519c9561745a474552570a3731d56`，初始工作区干净；本轮修复、回归和文档保留在当前工作区。

已阅读六个原计划目录全部 Markdown，包括 PLAN、契约、补充说明及实施报告。`ole-capability-project-configuration` 按实际目录 `role-capability-project-configuration` 处理；导航采用最新 settings 补充，工作流使用 v3，新项目默认资源显式保存。

独立数据集：`data/ubuntu-six-plan-20261109T162357Z`。原 51751 Host 与原数据集保持运行，没有升级或推进其工程。本轮 Host 使用动态端口，最后以独立包内 Host 重开数据完成核验，现已停止。

临时日志、JSON、截图、协议与探针输出在 `/tmp/devdesk-ubuntu-six-plan-7n3oj417`，这些路径只记录本机现场，不承诺长期存在。计划目录只保存本计划和 Markdown 报告。

## 修复的问题

| 问题 | 修复与回归 |
|---|---|
| v3 正式任务只列输出字段名，真实模型返回另一种检查结构 | 新冻结使用 `agent-task/v2`，output_schema 明确 summary、key/passed/evidence、四个唯一检查项、工作完成和文档类型；历史 v1 合同、输入和摘要不重写。新增回归修复前失败、修复后通过，原失败 Feature 保留。 |
| v3 候选评审缺少单父关系与目标基线证据 | 新增 Git 端口 commit_parents，只读查询精确候选；核验唯一父节点等于冻结目标，冻结 Merge 和对应真实仓库回执。Core 及真实 Git 回归拒绝错误候选、错误目标和多父关系。 |
| 普通实现说明错误替代 Structured Task Plan，返工 Attempt 因缺规划输入自行取消 | 两类 doc 分开修订；历史跨链 supersedes 保留原记录，只修正新输入投影。新增回归修复前复现，修复后新 Attempt 实际包含原 Task Plan。 |
| MCP 已成功且回执落盘，正常调度仍停在 needs_reconciliation | 每轮调度核对迟到回执，SQLite 调度提示保留待核对资格；未知调用不重新派发。回归覆盖无回执保持未知、有回执自动接纳、派发次数仍为 1、完成后恢复空闲，修复前失败、修复后通过。 |

验收入口同步修正：v3 task_instructions/子流程与系统节点字段、实际平台报告、DSH 服务商/模型/预算参数和原生工具组；浏览器初始化读取 capabilities.setup_required，消除首屏 count 竞态；Linux 凭据探针覆盖独立服务商和工具凭据命名空间。旧 node_modules 缺依赖通过 npm ci 恢复，锁文件和依赖版本没有变化。

完整场景补充明确测试文件路径、把交接说明保留在领域文档，并让 Feature 配方覆盖两套测试。所有调整形成新配置或正式业务返工，没有直接改写执行状态、放宽输出解析或扩大权限。

## 验收矩阵与结果

| 检查 | 本轮结果 |
|---|---|
| doctor / Ubuntu 前置条件 | 全部通过；Secret Service 可用且已解锁，专用 AppArmor 策略与源码字节一致。 |
| `./scripts/check/check.sh` 最终完整检查 | **165 项 Rust 通过**；16 项默认忽略另行显式执行。Web 构建、Schema、权限资产、debug/release HTTP、按需准备、standalone 通过。 |
| Chromium | **debug/release 各 31 项通过**；导航、连接、资源/权限、任务编辑预览、项目配置、工具和阶段布局均使用真实 Host。 |
| 全部默认忽略项 | 16 项均已显式通过：任务原生协议 1、Node 真实测试 1、Docker 资源 3、GitHub 回执恢复 1、模型路由 2、v2 profile 1、原生权限 3、HTTP/MCP/OAuth 执行 4。 |
| Docker 原生专项 | 授权写入、越界拒绝、禁网、私有状态隔离、真实 Red/Green、停止及清理；四项 HTTP/MCP/OAuth/未知副作用回归通过。 |
| DSH 模拟协议 | 固定 SDK 三路由流式/工具/恢复、MiniMax thinking、冻结任务/Role/Skill/Spec 载荷均通过；模拟容器 network=none，不冒充真实模型。 |
| Linux 专项 | 实际 Secret Service 保存/更新/读取/删除、重启、不可用 D-Bus、独立服务商及工具引用、Ollama 免密；OAuth start/cancel 和 AppArmor 嵌套沙箱通过。 |
| 权限切换进程退出 | 五个子进程退出边界通过：意图提交、停止回执丢失、停止记录提交、后继提交前、输入提交后。重启不双 writer、不重复输入、不采用后续新绑定。派发回执未知由既有 coordinator 回归覆盖。 |
| 真实 Codex 专项 | 两份任务的文件/HTTP 回执；权限热切换允许/拒绝；HTTP/MCP/只读 Skill/重启；完整模型组合/原生恢复均通过。输出合同修复后四组复跑通过。 |
| 独立发布包 | 源码外启动；7 个后端资产、9 个预设、默认 v3 任务编辑发布/预览、热加载、绑定与项目 schema v39 通过；最终包内 Host 的真实 HTTP/MCP/Skill 两轮及重启通过，3 次真实工具回执、HTTP 仅 1 次。 |
| 真实工程浏览器 | 完成的 Codex Feature 在 1440/390 宽度均无横向溢出，无 pageerror。 |
| 静态检查 | git diff --check、Python 语法及修改的 12 个 Rust 文件格式检查通过。全局 cargo fmt --check 在起点已有的 10 个未修改文件仍有格式差异，未列为通过，也未为格式清理修改这些文件。 |

AppArmor SHA-256：`3af35216df62da5ff6bfbe54b47506b219f26ccc99c944d84b826d66870cdb2c`。本轮未安装或放宽系统策略，未使用 privileged/unconfined。基础镜像 Linux/amd64：Codex `sha256:b03dc81902ff96bb95978d92648a4953bc16028d089873e3a530377eb67d592e`，DSH `sha256:b0e73cb529ab2bf3e5bb4dcd5a3344070bafadf1cccf4eb315607cf4a59dea80`；真实工程采用本机物化并冻结的 managed 镜像。

六项计划分别有当前证据：settings 的最终导航/反馈/AgentID/阶段行/内核连接；workflow tasks 的逐轮冻结/权限后继/真实任务；integrations 的实际 HTTP/MCP/Skill/恢复；model providers 的路由与 Linux 网络及 MiniMax 实际调用；permission bindings 的确切绑定/真实边界/恢复；role/capability 的配置、Spec、基线、子流程和完整工程。历史 macOS 结果没有当作本轮通过证据。

## Codex 完整闭环

Project：`87c12c08-c1cc-4207-8680-947a5f9d5acb`；Feature：`b5063703-8544-4b6f-b671-b090093cd4fc`；Task：`9c92a2b2-84b3-408e-9ff9-448187a0254b`。Codex 0.167.0 / gpt-6.1-sol，Workflow v3 / ProjectConfiguration v2。

实际完成 PRD 子流程、需求/设计/Task Plan/Test Plan、正式验证、Task assertion Red/实现/Green/文档/verify、独立 Review、候选验证、Feature 收尾、主干确认及合入。收尾返工后的同镜像/同修订正式测试为 **72 通过、0 失败、0 跳过**，保留原 Task 断言及 Feature 独立套件。

- [Task PR #22](https://github.com/xfork24/devdesk-integration-test/pull/22)：`eca36547d36a3d6d20950e007b4302498e5b9cb1`。
- [主干 PR #23](https://github.com/xfork24/devdesk-integration-test/pull/23)：`cdf0379c21248139ed408e55e82d409a36e94661`。

GitHub API 独立确认 merged、head.sha、merge_commit_sha 与候选一致；远端主干当轮核对一致，实际 git rev-list 核对单父目标。主干推进前有精确候选/输入确认。包内 Host 重开同一数据集后再次核验通过。

原始问题没有被覆盖：首轮 v1 渲染 Feature `958fa9e7-a40d-44b2-85e6-c899b9a0e032` 的 adapter_contract 失败保留；成功 Feature 共 32 个 Run（28 succeeded、4 cancelled）、8 次 Review（6 pass、2 changes_requested）。候选补证据通过候选准备返工；收尾范围和最终测试证据问题通过 closure 返工，原质量决定保留。中断候选 PR #21 经归属 marker/候选 SHA 核对后关闭，未合并。

## MiniMax 实测与用户最终判据

按用户指定使用 minimax-cn / **MiniMax-M3.1-Flash**，凭据仅存本轮 Secret Service。实际模型验证及需求提案生成可用，未观察到额度不足。

- `accd5d09-d953-4d1b-9095-2a0baf865c51`：8,192 token 预算达到 max-tokens，失败轮次保留并暂停。
- `14b635ca-1d0a-493d-ae32-a11c160b62bd`：显式新配置提高到 32,768 token，PRD 采纳后正式验证含 Markdown/JSON 混合输出，严格合同拒绝，暂停。
- `97b83233-297b-4a33-98b9-57b7762c6201`：另以 Agent 附加指令明确单 JSON 与阶段边界；仍在最终 JSON 前添加说明，adapter_contract 失败保留，暂停。

**MiniMax 完整工程闭环未通过，未将其写成余额不足或完整通过。** 用户最后明确：MiniMax 能力可能无法完成任务，Codex 完整闭环跑通即可认定验收通过。本报告采用该判据，DSH 模型能力限制不阻断本轮结论；没有为取得通过提取回复中的 JSON 片段或放宽解析器。

## 发布包、数据与清理

运行包：`dist/devdesk-0.2.0-x86_64-unknown-linux-gnu.tar.gz`。

- 包 SHA-256：`f256d6eeffe1fbb0c5c8fce4b55dfaac00a158e32987b12e055ab141dbee3e86`。
- Host SHA-256：`da8a21356a27f354f8c13c50659e1bf797d8546bbd99b0b1ca836a807f52bc7f`。
- 3 个 SQLite 库 integrity_check / foreign_key_check 通过；全局 v8、项目 v39。
- 226 个 Artifact 的路径边界和摘要通过；41 个执行均撤销 writer，41 个容器均停止或移除。
- 重启及切换项目配置后，先前保存的 15 条不可变任务/Feature 锁正文和摘要逐字一致。
- 扫描 17456 个非凭据文件及工作区 diff，未发现本轮真实 access/refresh/id token 或 MiniMax key。
- 删除 27 份测试 auth.json 副本及本轮 MiniMax vault 条目；原登录来源保持原样。Linux 合成凭据探针的测试条目也已清理。
- 独立验收 Host 已停止；原 PID 2209343 / 51751 服务保留。未清理用户其他镜像、卷或工作区。

未覆盖边界：真实第三方 OAuth 账户连接、锁定用户 collection、每条机器指令的崩溃压力矩阵及其他宿主架构；分别由已声明协议夹具、不可用 D-Bus、五个进程退出边界和当前 Ubuntu x86_64 证据限定。MiniMax 原生协议模拟通过与真实模型限制分别记录。
