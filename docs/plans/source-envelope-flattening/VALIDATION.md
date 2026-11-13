# 验证报告与现场留存

日期：2026-11-13。验证范围：`48603ff5b8acde5b6e2bc827fdb1290309141c92` 之后的 `5bec145`、`58bb4d2`，以及当前源配置信封扁平化工作区。真实模型仅测试 Codex Harness。

## 结果

计划实现完成；三次改动方向合理，跨层遗漏和真实运行暴露的问题已修复，详见 [改动评审](REVIEW.md)。离线测试与实际 Feature 业务交付通过。**这次是包含需求返工和受控恢复的完整流程，不能记为无人干预、首次一次通过。**

- Rust 全工作区：236 通过、15 个按原标记忽略；工作流套件另以默认并发复跑 37/37。
- Web：TypeScript + Vite 通过；Chromium debug / release 各 51/51。
- HTTP、连接安全、初始化、模板重置、重启和运行准备通过；151 个源定义自洽。
- 真实 Feature：22/22 节点完成；唯一 Task：20/20 节点完成；两者业务状态均为 `completed`。
- 两次真实 TDD 增量，有 5 条逐次原始执行记录；6 次平台 Docker 测试回执；两份精确候选发布到 GitHub。
- 实际 Host 读取验证退出码 0；封存现场在不启动 Host、模型或网络的情况下重新核验通过；封存最终候选在同镜像中再次运行 13/13。

没有迁移、清理或改写用户原有数据集，也没有增加旧格式兼容层。所有数据验证使用独立新目录。

## 保留位置

长期现场：[/Users/hengliu/workspace/devdesk-validation/source-envelope-flattening-20261113](/Users/hengliu/workspace/devdesk-validation/source-envelope-flattening-20261113)。该目录位于源码仓库之外，以权限 0700 保存。原始目录 `/private/tmp/devdesk-flatten-feature-fixed-20261113` 与首次失败目录 `/private/tmp/devdesk-flatten-feature-20261113` 也未删除。

| 路径（相对长期现场） | 内容 |
|---|---|
| `native-completed/` | 成功业务流程完整数据集、Host 二进制、SQLite、托管 Git 仓库/工作区、Codex 会话与原生 wire 日志、Workflow 视图、任务正文和发布证据 |
| `native-startup-failure/` | 配方摘要不一致导致首次启动失败的独立现场 |
| `logs/` | 本轮完整离线测试、构建、浏览器、真实流程、恢复、失败及复核日志；失败日志照实保留 |
| `browser-debug/`、`browser-release/` | 浏览器验收截图与测试产物 |
| `ARTIFACT-INDEX.md`、`artifact-index.json` | 全部 42 个节点、执行身份、Session/Turn、输出端口和 Output ID 索引 |
| `docker-test-receipts.json` | 六份平台测试回执，含完整测试报告、容器、镜像、配方、候选和执行快照 |
| `delivered-files/` | 从最终 Feature 候选精确导出的模块、开发自有增量测试、Task 独立测试、Feature 独立测试 |
| `docker-test-runtime.tar.gz`、`docker-image-inspect.json` | 本次实际使用的不可变 Docker 镜像及其元数据 |
| `reproduction/` | 离线核验器、9 项回归测试、真实探针与客户端的冻结副本 |
| `source/` | 当前源码快照、Git 历史、两次提交及当前未提交差异、构建二进制摘要与环境信息 |
| `MANIFEST.json`、`verify-manifest.py` | 现场文件的 SHA-256 与长度清单及核验入口 |

认证临时副本已清理；不归档用户登录凭据。测试 Host 已停止。保留的 SQLite 与 Git 工作区包含原运行目录的绝对路径，因此归档用于读取和核验；需要继续运行时先另建工作副本，保留本封存目录。

## 真实需求、流程与产物

专用测试仓库：`xfork24/devdesk-integration-test`。需求为新增无外部依赖的 ESM 函数 `greeting(name)`：字符串先 trim，再返回 `Hello, <name>!`；空白字符串和非字符串抛出 `TypeError`。限定一个 Task，冻结两个 TDD 增量：问候与 trim；无效参数拒绝。Task 测试和 Feature 测试由各自独立职责编写，开发仅逐步扩展其自有增量测试。

| 标识 | 值 |
|---|---|
| Project | `47db41e8-5bff-4889-ab10-fb5cba654c6d` |
| Feature | `802e30c6-7db8-49c6-80df-f5bc44ae44d0` |
| Feature Workflow | `ff98500d-51d4-4dea-bdc3-49957851af86` |
| Task | `8b1c8fe0-63c0-4001-bfb4-c09a6bf9c788` |
| Task Workflow | `7170e8de-d817-46aa-9b5e-21ed32831289` |
| 测试隔离后缀 | `b55d65d7` |

Feature 流程完整经过：需求 → 需求独立评审 → 源工作区和工程约束 → 设计及独立评审 → 计划及独立评审 → Task 物化与子流程 → Feature 工作区 → 发布测试配方 → 测试快照 → 实际验证 → 收尾文档 → 主线查询 → 候选 → 候选测试 → 收尾独立评审与候选独立评审 → 推进 → 发布。根实例经原生工具明确完成。

Task 流程完整经过：工作区 → 独立测试 → 测试快照 → 最小可调用 Red 基线 → Red 快照 → 平台 Red → 两次开发 TDD 增量 → 实现快照 → 平台 Green → 文档交付 → 源码复验 → 目标查询 → 候选 → 候选测试 → 实现独立评审与候选独立评审 → 推进 → 发布 → 证据交接 → Task 完成。

全部执行节点与 Output ID 见长期现场的 [42 节点索引](/Users/hengliu/workspace/devdesk-validation/source-envelope-flattening-20261113/ARTIFACT-INDEX.md)。每份交付正文可以在 `native-completed/<Workflow UUID>.json` 的 `outputs[Output ID].value` 找到；Session/Turn 与实际 Harness 请求、响应可从索引定位到 `data/projects/<Project UUID>/execution/sessions/<Session UUID>/coordinator/*.jsonl`。

最终产物：

- `src/workflow-v5-b55d65d7/greeting.mjs`
- `test/workflow-v5-b55d65d7-increments.test.mjs`
- `tests/workflow-v5-b55d65d7-task/behavior.test.mjs`
- `tests/workflow-v5-b55d65d7-feature/behavior.test.mjs`

文档通过 Workflow 输出端口交付，保留在节点产物中。

## 测试与发布证据

开发原始记录为 Red(1) → Green(2) → Red(3) → Green(4) → Regression(5)。每对 Red/Green 执行相同命令和同一组测试正文，Red 为业务断言失败，Green 为退出码 0。逐次记录包含 stdout/stderr、时间、工作目录、文件内容与摘要、Node 环境和终止状态；核验器将每条记录与原生 `commandExecution` 的实际工具输出逐项相等比较。

| 平台测试 | 精确 Git 候选 | 结果 |
|---|---|---|
| Task Red | `648ab730fa61645d5168dc87efeea46c3201aa3a` | 8 项业务断言失败，预期退出码 1 |
| Task Green | `f0ca465a4fc141f36ceb6c0b9eff3aef296d9554` | 8/8，退出码 0 |
| Task 源码复验 | `f0ca465a4fc141f36ceb6c0b9eff3aef296d9554` | 8/8，退出码 0 |
| Task 候选测试 | `104d83e8eb38cec239a7027575121de34f61d68b` | 8/8，退出码 0 |
| Feature 验证 | `f489278092ca9779eb6fd0813502b6a19b7ffae0` | 13/13，退出码 0 |
| Feature 候选测试 | `ee30393a4e2f709555bba83dc135a893334d650a` | 13/13，退出码 0 |

六次使用相同镜像 `sha256:5f8731ec42de5c5d542d5398147c3348c200ad7a66c6cfc99a5f52e0b9776ffc`，`linux/arm64`，Node `v24.13.0`。配方为 `node_test`，超时 60 秒；Task 运行 Task 测试文件，Feature 运行 Task 与 Feature 两份测试文件。独立测试从 Red 到 Green 未被修改。

Task [PR #40](https://github.com/xfork24/devdesk-integration-test/pull/40) 合入 Feature 分支，Feature [PR #41](https://github.com/xfork24/devdesk-integration-test/pull/41) 合入 main。GitHub 实时查询结果另存 `native-completed/github-pr-{40,41}.json`。两者 `candidate.revision == pull_request.head == merge_commit`，分别为上表两个候选测试 SHA；不是只核对“PR 已合并”。

独立核验同时检查：不可变原始 Workflow 修订仍可解析；原强制节点、依赖、完成条件、确认策略和职责未被删除或放松；无旁路、无豁免、无人工批准；最终候选中的源码及测试文件与原始执行快照逐字节一致；候选改动没有越过冻结文件范围。

Task 向 Feature 交接为 **118,296 UTF-8 字节**。Feature 的 `closure_review` 和 `candidate_review` 均使用平台分块读取引用。核验严格限定本节点的 execution/session/turn 与输出 ID，匹配实际原生工具响应和数据库成功回执，重组区间 `0–32768–65536–98304–118296`，核对 SHA-256 `4631eaf02914fd98e467704a5afeb4973c8c9521699d20c1636ab98a10c072f0`，并确认重组 JSON 与封存交接完全相同。证明保存为 `reviewer-input-reads.json`；任务正文确实发到原生 `turn/start` 的证明为 `reviewer-wire-deliveries.json`。

## 失败、恢复与实际边界

1. 首次新目录在 PM 启动前暴露运行时注册摘要 16 位、权限门禁比较 64 位的不一致，未产生业务发布。修复为完整摘要相等后，用另一全新目录从头开始。首次失败现场完整保留。
2. 需求独立评审拒绝过一次文件授权边界冲突。PM 经正常返工和复审通过，没有覆盖拒绝。
3. 设计评审期间受控停止，采用修复后的构建恢复同一根实例；旧执行者停止后以新身份重试被中断的 Agent，保留已接纳的业务产物。PM 控制 epoch 为 3，输入账本为 2 条。因此不宣称本次满足 `--unattended` 的无接管条件。
4. 业务完成后，旧证据脚本分别误读源支持的冻结计划编码、将大输入引用误认为内联正文。已按当前合同修正并增加 9 项回归，缺分块、错误源/Turn、摘要篡改、缺原生响应及错误交接均会拒绝。随后只重读已完成实例，没有接管、派发或再次发布。
5. 本次探针在根业务完成后停止 Host，PM 最后一个原生控制 Turn 留下 `needs_reconciliation`；Feature/Task 及全部 42 个业务节点仍为 `completed`，没有未核对业务工具副作用。原状态保存在 `controller-at-shutdown.json`，没有改写为 `closed`。探针现已增加业务完成后等待 PM 自然关闭、以及重新启动时保留既有 Host 日志；新增等待逻辑未在本次已完成实例上重新进行业务运行。该停机细节作为现场边界保留。
6. 最后一次整段 `check.sh` 在浏览器竞态失败；修复后各组件分别完整复核成功。并行构建期间两项 Git 回归出现待核对状态，随后单项、限制并发全量及默认并发 37/37 均通过；保留失败日志，不把旧退出码算作成功。详细日志对应关系见 [实施记录](EXECUTION.md)。

## 离线复核

在长期现场中执行，不需要认证、模型或外网：

```sh
cd /Users/hengliu/workspace/devdesk-validation/source-envelope-flattening-20261113
python3 verify-manifest.py
python3 reproduction/verify_engineering_evidence.py native-completed
python3 -m unittest discover -s reproduction -p test_engineering_evidence.py
```

使用保留的实际镜像重放最终候选测试（Docker 必须运行；不调用模型或 GitHub）：

```sh
docker load -i docker-test-runtime.tar.gz
docker run --rm --network none \
  --mount "type=bind,source=$PWD/delivered-files,target=/work,readonly" \
  --workdir /work --entrypoint node \
  sha256:5f8731ec42de5c5d542d5398147c3348c200ad7a66c6cfc99a5f52e0b9776ffc \
  --test tests/workflow-v5-b55d65d7-task/behavior.test.mjs \
  tests/workflow-v5-b55d65d7-feature/behavior.test.mjs
```

已实际执行封存副本的离线证据核验、9 项核验器回归和最终候选 Docker 重放，结果保存在 `logs/retained-*.log`。完整 SHA-256 清单可验证报告与现场后续是否发生变化。
