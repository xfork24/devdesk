# 实施与验收记录

日期：2026-11-13。基准：`58bb4d2`。状态：实现、离线验收、真实 Codex Feature 业务交付及证据复核完成。[完整验证报告与留存入口](VALIDATION.md)。

## 已完成

- 151 个出厂源、领域对象、命令参数、存储编解码、RPC/源 Schema、Web 读写、默认检查与验收探针对齐 `type`、字符串 Scope、根级引用。
- 引用白名单、单值/列表形状、规范修订、去重与空 Agent/Capability 正文约束；Scope 编码与可见性验证；内容摘要覆盖 refs + data。
- 修正服务商配置 Host 接线、目录最新修订选择、精确 Harness 引用保留，以及完整配方摘要门禁。详见 [三次改动评审](REVIEW.md)。
- 清理业务配置在 Rust、Web、发布 Schema 中残留的版本号门禁；增加未来元数据版本不改变结构接受的回归。
- 数据集验证全部使用独立空目录；未迁移、删除或改写用户既有数据集。

## 最终离线结果

| 检查 | 结果 |
|---|---|
| 全工作区 Rust | `RUST_TEST_THREADS=2 cargo test --workspace --locked`：236 通过、15 按原标记忽略，退出码 0；日志 `/tmp/devdesk-flatten-workspace-final.log` |
| 默认并发 Workflow 套件 | 37/37，166.37 秒；日志 `/tmp/devdesk-flatten-git-concurrency.log` |
| Web 构建 | TypeScript + Vite 通过；日志 `/tmp/devdesk-flatten-web-final.log` |
| Chromium debug | 51/51，7.7 分钟；日志 `/tmp/devdesk-flatten-browser-debug.log`，截图 `/tmp/devdesk-flatten-browser-debug/` |
| Chromium release | 51/51，2.4 分钟；日志 `/tmp/devdesk-flatten-browser-release.log`，截图 `/tmp/devdesk-flatten-browser-release/` |
| Host / 连接安全 | debug 与 standalone release HTTP、进程锁、幂等、项目隔离、SSE、停止/重启、认证及 Origin/CORS 通过 |
| 模板与初始化 | 数据库副本、重置、不可变历史、重放、偏好、重启、全新目录初始化及运行环境准备通过 |
| 出厂资产 | 权限资产完整性通过；151 源定义自洽；bundle `2026.11.13-source-flat-v1` |

包含 Scope / 引用形状 / refs+data 摘要、完整配方摘要、服务商创建修订接线和新目录/旧 Harness 精确引用、项目间接采用、未来元数据版本以及晚到配置读取的回归。

首轮浏览器 46/50，补齐字段遗漏后定向 5/5；完整 debug 随后复现已有规范比较竞态，修复并增加可控延迟回归后，最终两个档位均为 51/51。`check.sh` 的最后一次整段记录停在该竞态；表中成功结果来自修复后相同组件的独立完整复核，不把早先失败退出码记作成功。

并行构建及多套件同时运行期间，默认并发的工作流回归出现两个真实 Git 快照待核对结果；逐个复测、限制并发全量运行以及完成构建后的默认并发 37/37 复跑均通过，未再复现。保留失败日志，不改动副作用核对及接纳条件。

## 真实 Codex Feature

测试仓库：`xfork24/devdesk-integration-test`。认证仅使用既有登录文件的临时副本；退出时清理。没有调用真实 DSH 模型。

首次独立启动 `/tmp/devdesk-flatten-feature-20261113` 在项目经理启动前复现摘要长度不一致，未产生业务交付或发布。修复后在新数据集 `/tmp/devdesk-flatten-feature-fixed-20261113` 从头启动。

- Project：`36580b2e-1914-4981-ab3f-97cd80e88357`（首次失败数据集；保留诊断）。
- 当前 Project：`47db41e8-5bff-4889-ab10-fb5cba654c6d`。
- 当前根实例：`ff98500d-51d4-4dea-bdc3-49957851af86`。
- 当前 Feature：`802e30c6-7db8-49c6-80df-f5bc44ae44d0`。
- 需求独立评审拒绝一次文件授权边界冲突，项目经理按正常流程返工并复审通过，没有覆盖拒绝。
- 在设计评审期间受控退出并以修复后的构建 `--resume` 接管同一根实例，保留已接纳输出，核对旧执行者停止后采用新执行身份继续。此为包含重启恢复的完整工程验收，不宣称无人值守首轮一次通过。

Task 的 20 个步骤已完成，Docker Red/Green、源码复验、候选测试及两项独立评审通过。[Task PR #40](https://github.com/xfork24/devdesk-integration-test/pull/40) 已合入，测试候选、PR head 与 merge commit 均为 `104d83e8eb38cec239a7027575121de34f61d68b`。Task 完整原始证据已交接给 Feature。

Feature 的 22 个节点和 Task 的 20 个节点均完成，业务状态均为 completed。[Feature PR #41](https://github.com/xfork24/devdesk-integration-test/pull/41) 合入 main，测试候选、PR head 与 merge commit 均为 `ee30393a4e2f709555bba83dc135a893334d650a`。两次发布均另用 GitHub 查询核实。

实际运行留下 5 条开发原始记录、2 对独立增量 Red/Green、6 份平台 Docker 测试回执；Task 候选 8/8、Feature 候选 13/13。核验器将每条开发记录与实际原生命令输出匹配，并逐字节核对最终 Git blob。两项 Feature 评审均在确切 Session/Turn 中读完 118,296 字节交接，全部分块与摘要、封存 JSON 一致。没有豁免、旁路或人工批准。

业务完成后修复核验器对冻结计划源编码和大输入引用的错误假设，已完成实例只读复核退出码 0，未接管、派发或重复发布。封存副本的离线证据复核和 9 项核验器回归通过，最终候选在同 Docker 镜像中复跑 13/13。

原探针停止 Host 时 PM 控制 Turn 尚未自然结束，保留的控制器状态为 needs_reconciliation；不将其记为 closed，业务实例和全部节点仍已完成。已为后续探针增加业务完成后等待控制器关闭，并保留每次启动前的 Host 日志。这段等待逻辑没有在本次完成实例上重新触发业务流程。完整停机状态及恢复边界见验证报告。

长期现场为 `/Users/hengliu/workspace/devdesk-validation/source-envelope-flattening-20261113`，包含数据库、Git 仓库与工作区、原生 wire 日志、全部产物、浏览器证据、Docker 镜像、源码快照和校验清单。原 `/tmp/devdesk-flatten-*` 也保留。计划目录只保存 Markdown；未提交当前修改。
