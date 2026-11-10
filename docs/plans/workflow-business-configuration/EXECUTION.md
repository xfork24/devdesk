# Workflow 通用步骤改造执行记录

日期：2026-11-10。状态：实施、硬编码审查、开发数据重建及本机最终验收完成。平台限制见文末。

用户批准 [PLAN.md](PLAN.md)，并明确无需迁移旧开发数据。工作分支为 `codex/workflow-business-configuration`，保留开始前的 README、界面导航、设置和历史相关修改。

| 阶段 | 实施结果 |
|---|---|
| P0 审查与合同 | 完成 H01—H21 分类与处理，见 [AUDIT.md](AUDIT.md)；当前合同见 [CONTRACT.md](CONTRACT.md)。 |
| P1 领域与配置 | Workflow v4，Spec/Standard v3，自由 Role、明确执行能力、端口、Schema、来源与预算。 |
| P2 执行闭环 | 四种执行器统一调度，canonical `agent-task/v4`，确切产物、补充输入、确认、重试、返工和子流程集合。 |
| P3 平台工具 | 实际工作空间、快照、Task、Node 测试、候选、CAS 与发布；无占位 Agent/Session。 |
| P4 完成规则 | 有界事实规则、动态内容检查、人工确认与显式覆盖；输入、输出和回执不可篡改。 |
| P5 默认资源 | 工程方法迁入可编辑配置；清理旧提案约定和固定检查项，补齐代码基线、测试快照及 Task 证据传递。 |
| P6 编辑器与工作台 | 通用步骤与子流程、Role 继承、任务预览、输入输出、目录授权、Spec/Standard、动态执行视图及草稿恢复。 |
| P7 清理与验收 | 旧开发数据已删除重建，凭据及源仓库保留；本机回归、原生完整流程和独立包均通过。 |

## 首次实施验收

- 首次统一检查 `DEVDESK_E2E_PORT=4330 ./scripts/check/check.sh` 通过：155 项 Rust 测试通过、0 失败、15 项真实资源用例按默认配置忽略；debug / release 各 43 项 Chromium 回归全部通过。提交前清理删除两项过时运行偏好用例，当前检查规模与结果见文末。
- Web 构建、实际 Host 保存、发布、重载、材料引用、子流程、任务预览、确切确认、覆盖规则及两项异步选择/加载竞态均已验证。独立包从空目录启动、编辑/发布、任务预览及实际自定义工具流程均通过；不依赖源码、不调用模型。
- HTTP 鉴权、Host/Origin、SSE、退出、分库、SQLite、幂等、模板重置及运行环境准备入口通过。
- 真实 Git 验证工作空间、实际文件快照、单父候选、CAS 与恢复；平台调用直接驱动实际 Git。
- 真实 Docker 验证 Node Red/Green、环境故障、停止/清理、权限、任意目录局部写与只读覆盖，包括只读根目录。
- DSH 固定 SDK 在禁外网 Docker 内通过本地端点的流式、工具及恢复回归；这是原生 SDK 夹具，不是实际 DSH 云模型推理。
- 真实 Codex 完成自由业务 Workflow：自建“数据研究员”Role、分析 Skill、JSON 交付类型，结果为降序 `[3,3,2,1]` 与总和 `9`，后续 Step 消费确切结果。实际任务、SQLite 与 Artifact 已核验。对应 Workflow ID：`b1661777-8ac4-4c9a-be2e-aeb62fd7d352`。
- 补齐 Agent 平台工具桥后再次通过实际 Codex 验收，Workflow ID：`57e7cdac-ac3e-40fe-bdbf-badb046e1fa1`。Agent 实际调用 `materials.capture`，收到平台回执后提交结构化结果；查询核对真实 Turn、调用身份、封存值和后继消费。
- 原生恢复验收通过，根实例 `16923fe9-816c-4d20-abe8-40850c791617`；PR #24 / #25 按确切 SHA 合入，验证失败派发后的子流程重试，以及保留原拒绝后的候选返工。后续中断和拒绝记录保留，不算作最终图通过。
- 最终默认图从空数据集通过实际 Codex、Git、Docker 和 GitHub 全流程：Feature 21 步、Task 18 步全部完成，1 个实际 Task 完成。根实例 `97ba6269-286f-458d-8139-5d0128fb0182`，子实例 `b8033377-e58d-4874-84d2-53e58c5a8d05`，Feature `1f47ec81-806d-4687-ba41-21b6de4b2b55`。三次确切确认接口完成规划、Task 候选与 Feature 候选门禁；未覆盖模型拒绝。

## 完整工程方法验收证据

| 事实 | 确切代码版本 | 实际结果 |
|---|---|---|
| Task Red | `b41942ab543c8f3b0e711a2da29db8f13857f787` | 1 项真实断言失败，exit 1，预期失败成立。 |
| Task Green / 文档后 verify | `f01305350d5216b46f5a3a8253e83c55a6e94bdc` | 各 7 项通过、0 失败、0 跳过，exit 0；均有独立 Docker 回执。 |
| Task 候选测试与发布 | `adf69088729c8cc1c065421c1ac07e774e4323ae` | 7 项通过；[PR #27](https://github.com/xfork24/devdesk-integration-test/pull/27) merged，head / merge_commit 均为此 SHA。 |
| Feature 整体测试 | `0c465920b70966c642702fd9d7940925ea2ede34` | Task 与独立 Feature 套件共 15 项通过、0 失败、0 跳过。 |
| Feature 候选测试与发布 | `5468f1fd6681cfefc6ffd85877f6b11de399af52` | 15 项通过；[PR #28](https://github.com/xfork24/devdesk-integration-test/pull/28) merged，head / merge_commit / 远端 main 均为此 SHA。 |
| 显式 Task 完成 | Task `c3228768-12a0-43ed-a1dc-1ed2a7fe3373` | `tasks.complete` 实际回执 completed，未依赖 Git 发布隐式修改状态。 |

GitHub 最终状态另经 `gh pr list` 与 `gh api .../git/ref/heads/main` 独立核对。候选父提交和 diff、同 SHA 测试、完整输入及实际产物由探针再次断言。

首次实施独立包（提交前复查已重新构建，最新摘要见文末）：`dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`。当时包 SHA-256 为 `0ff33f84f1e140713e2f08f25615df3d3da1562a6dac2739b9914447919c99e7`；程序 SHA-256 为 `574b4e6e070b6552c6c5a112deca2c5e99409c876778e0fe6b8fb497705d17f8`。核对 7 个权限资产、3 个预设、当前 Schema、配置修订、显式 Task 完成、评审前复验、Feature 测试写授权，以及实际独立工具流程。无自动下载或模型调用。

临时证据目录为 `/private/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-engineering-v4-gwmlase6`；自由业务及工具桥证据为同级 `devdesk-native-v4-c47fad68`。首次统一检查日志为 `/tmp/devdesk-final-check9.log`，独立包报告为 `/tmp/devdesk-final-package-current-report.json`。这些是本次临时执行证据，不承诺清理后仍可读取；计划目录只保存 Markdown。


## 原生验收发现并修复的问题

1. Host 必须在解析组合、补入执行字段前核对原始调度意图，避免将可信请求误判为篡改。
2. 完整 Agent 的模型参数只来自冻结 Agent 配置，不再同时写入独立参数覆盖字段。
3. Feature 在建立根流程时先进入运行状态，后续会话意图使用稳定的当前对象版本。
4. 默认 Role 聚焦职责，Skill/Spec 清理旧 requirement/design 提案 ID、独立采纳和固定四项检查约定。
5. 任务同时提供精确输入来源及值摘要，输出合同直接描述每个端口的值类型、动态检查项和完成时的必需端口。
6. Docker 的写入及只读覆盖均采用显式授权目录，不再强制 `tests`；只读根目录不生成无效的点路径挂载。
7. 默认流程显式固定 Feature 新测试版本；候选使用该版本。Task 将测试、文档、评审及发布的真实回执打包交接。
8. Task 创建保留完整业务元数据，依赖工作副本在前置完成后冻结当时的父级代码版本。
9. 代码输入根据冻结任务中的真实工作副本授权核对归属，不再用 Feature 会话 owner 与 Task owner 必须相等阻塞合法调用。
10. 轮次派发失败及时记录原错误；明确重试子流程同时恢复对应父调用，已完成的前置步骤保留。
11. Codex Agent 与独立 Tool Step 共用平台处理器，核对实际 worker / Session / Turn / 调用 ID、参数及权限。原生回复使用独立交付记录，不改写终态回执；参数拒绝、取消和输出合同拒绝均有回归。
12. 候选回执补齐从实际 Git 读取的父提交与 diff。原生评审先因缺失这些证据拒绝，补齐后返工通过，未覆盖原拒绝。
13. `git.publish` 不再隐式完成 Task；默认配置通过 `tasks.complete` 明确完成。纯材料 Task 可以不使用 Git，仍核对实际归属、依赖与停止。
14. 默认方法要求文档后独立交付复验；补齐实际 `tests.run` 步骤、实现评审输入和交接证据，未降低 Spec 要求。候选评审显式接收源评审及相关验证事实。
15. Feature 整体测试由独立 Step 编写，不由单个 Task 提前交付；显式声明 Skill 和测试目录写授权，后续再快照与实际运行。
16. 讨论身份切换使旧异步详情失效；接入绑定从当前目录加载默认执行配置，并在加载完成后开放表单。刻意延迟默认响应的浏览器回归通过。

## 开发数据清理

已通过 HTTP 正常停止原 4318 Host，确认其进程退出且没有所属 Docker worker 后，删除 `/Users/hengliu/workspace/devdesk/data` 内过时的 global 11、project 42 数据与托管项目目录。保留数据集标记、`credentials` 目录、系统凭据和外部源仓库。首次新基线为 global 12 / project 43；提交前复查清理后提升至 global 13 / project 44，无迁移器或双版本执行器。

临时原生验收使用单独的数据集、程序副本和登录文件副本。退出后删除临时登录文件。最终检查 14 个本次原生数据集，剩余临时 auth.json 为 0，原始登录文件仍在；业务执行证据保留在临时报告目录。

首次实施验收结束时，日常数据再次核对为无项目、无自建定义、无讨论消息且没有持有者后，重建到最新默认目录。`http://127.0.0.1:4318` 已运行最新开发程序，默认 Feature 为 21 步、Task 为 18 步。凭据与数据集标记保留；随后通过正常模板重置切换到当时目录 generation 2，确认 Feature 测试 Step 已获得明确测试目录写授权。

## 提交前复查及重新验证

用户要求再次深入 review 后提交。发现并清理固定角色运行时偏好、空 BindingLock、旧 Spec slots/rules、旧 Run 选择与 execution_task 运行锁覆盖、按 Task 共用会话、无效校验和旧字段名解析分支。生产业务身份/阶段/节点名和 builtin ID 分支重新扫描；处理细目见 [AUDIT.md](AUDIT.md#提交前独立复查)。

- `DEVDESK_E2E_PORT=4330 ./scripts/check/check.sh` 完整通过，日志 `/tmp/devdesk-review-full-check2.log`：155 项 Rust 通过、0 失败、15 项外部用例默认忽略；debug / release 各 41 项 Chromium 通过。删除两项过时运行偏好用例，新增/更新项目字段拒绝、无旧 SQL 列和同 Agent 不同 Task Step 活动会话隔离回归。HTTP、模板重置、运行环境准备与两种构建均通过。
- 真实 Codex、自由 Role / Skill、平台工具及后继输入重新验收通过。Workflow `fc78d289-529e-4eb4-8223-77cfcb851398`，实际排序 `[3,3,2,1]` / 总和 `9`，真实 `materials.capture` 调用回执及后继消费一致。证据 `/private/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-native-v4-0qz3j6x2`，日志 `/tmp/devdesk-review-native.log`。
- 该原生数据集的 global 13 / project 44 均通过 `integrity_check` 和 `foreign_key_check`，原生 worker 已停止，临时 auth.json 为 0。完整 21 / 18 步工程发布的证据来自前一次实施验收，本轮复查未重复创建工程验收 PR。
- 当前日常数据无项目；停止确切持有者、再次取得数据集锁后，仅删除已过时的空项目全局库，按 global 13 重建。原始凭据保留，`http://127.0.0.1:4318` 已启动当前程序，默认 Feature 21 步 / Task 18 步均从新目录读取（generation 1）。
- 重新构建并通过独立安装包探针（从源码目录外启动、0 次模型调用、0 次自动下载）：项目 Schema 44，7 个权限资产及 3 个预设、实际通用工具 Workflow 均通过。报告 `/tmp/devdesk-review-package-report.json`；当前包 SHA-256 `0b3ef57dbcfb3848bdf2fcc7befb037e1a53c28d85d3ad9471f9ec7b80760b05`，程序 SHA-256 `9726651bfd9c8ca4a244f9edafcf367d8b66fb20d730d657485271d00b6247be`。
- `cargo fmt --all -- --check`、`git diff --check` 与当前说明文档的本地链接核验通过。暂存范围为源码、配置、协议、SQL、测试与文档；不含运行数据、凭据、构建产物或临时证据。

## 可复现入口与限制

```bash
DEVDESK_E2E_PORT=4330 ./scripts/check/check.sh
./scripts/check/check-live.sh --groups git,docker,permissions
python3 tests/live/probe_configured_workflow.py --binary target/debug/devdesk-host --codex-auth-source /专用/auth.json
python3 tests/live/probe_default_engineering_workflow.py --binary target/debug/devdesk-host --codex-auth-source /专用/auth.json --github-repo owner/dedicated-test-repo
./scripts/release/package.sh
```

真实模型探针只在显式调用时使用账户和专用测试资源。本次 macOS 完整检查、自由业务原生验收、默认工程图与独立包已通过；提交前复查后重跑的具体范围见上节。Ubuntu 新版本实机验收及真实 DSH 云模型工程运行尚未执行；DSH 的动态平台工具桥当前明确不支持，独立 Tool Step 不受此限制。不能将 SDK 本地夹具或历史版本报告当作这些范围的实机证明。
