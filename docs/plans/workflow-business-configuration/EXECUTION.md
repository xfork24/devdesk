# Workflow 通用步骤改造执行记录

日期：2026-11-10。状态：实施、硬编码审查、开发数据重建、macOS 验收及 Ubuntu Codex 验收完成。本轮按用户要求仅验收 Codex Harness；平台限制见文末。

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

## Ubuntu 当前合同复验（仅 Codex Harness）

2026-11-10，以 main `ea74bd5` 为源码基线，在 Ubuntu 24.04.5 LTS / x86_64 宿主验收，使用 Rust 1.99.1、Node 24.13.0、Docker 29.9.3。按本轮用户要求，真实供应商验收仅使用固定 Codex 0.167.0，不运行 DSH/Ollama 真实探针。默认检查中的多供应商协议替身不计作真实模型验收。

| 检查 | 本轮结果 |
|---|---|
| 默认完整检查 | `DEVDESK_E2E_PORT=45330 ./scripts/check/check.sh` 通过；Rust 154 项通过、0 失败、15 项环境用例默认忽略；debug/release 各 41 项 Chromium 通过。 |
| HTTP 与运行准备 | 双 profile 的鉴权、连接、SSE、项目分库、退出/重启、独立二进制模板重置均通过；运行准备的显式 Docker CLI 替身检查通过。 |
| 真实 Git/Docker/Codex 权限 | `git_resources`、`command_tests`、`docker_resources`、`permissions` 共 13 项通过，包含 Node Red/Green、停止/清理、任意目录局部写及嵌套沙箱越界拒绝；显式排除 DSH 容器探针。宿主既有 AppArmor 策略摘要核验通过。 |
| 自由业务原生模型 | 自建数据研究 Role、分析 Skill、Agent 与 JSON Delivery，实际 Codex 输出 `[3,3,2,1]` / `sum=9`，真实 `materials.capture` 回执及后继材料一致；canonical 任务为 `agent-task/v4`。Workflow `b049c6b7-133a-4578-ba5d-d7ef39121814`。 |
| 重置后按旧绑定原生执行 | 目录恢复至 generation 2，原项目绑定、冻结任务、产物、凭据和运行安装保持原值；重启后新 Feature 按归档配置实际完成相同业务及平台工具调用，Workflow `16bbbf7d-658c-4a8a-b6e7-c7f4d937278f`。详见 [模板重置复验](../configuration-template-reset/EXECUTION.md#ubuntu-当前合同复验仅-codex-harness)。 |
| 独立 Linux 发布包 | 源码外启动并通过权限资产、配置编辑/发布/预览、实际独立工具流程及 Schema 44 检查，0 次模型调用、0 次自动下载。包及程序摘要见模板重置报告。 |

真实 Git/Docker/Codex 权限检查命令：

```bash
cargo test --locked -p devdesk-adapters \
  --test git_resources --test command_tests --test docker_resources --test permissions \
  -- --include-ignored --skip dsh_runner_masks_private_state_and_proc_without_expanding_docker_privileges
```

## Ubuntu 默认工程原生验收

完整工程探针使用隔离数据集、私有二进制副本和既有专用测试仓库 `xfork24/devdesk-integration-test`。实际模型、Git、Docker 测试与 GitHub 结果分别核对，不用默认回归的替身代替。

Feature 21/21 步、Task 18/18 步全部完成，实际 Task 状态为 completed。根实例 `476a3e96-1df9-46f3-a687-5aaa3b247557`，Task 子实例 `828fd3e8-47d5-41e6-a351-9072e7308de5`，Feature `3ff2c208-31f5-48ba-bab6-8da8e57c4a80`，Task `ce5417bf-a779-456b-8cee-4bc900ecc9b6`。三次确切版本确认分别接纳已通过的规划、Task 候选和 Feature 候选评审，没有覆盖失败条件或模型拒绝。

| 事实 | 本轮确切版本 | 实际结果 |
|---|---|---|
| Task Red | `86a59b33cf57892285a3d61f17d596aad868ac4a` | 1 项实际断言失败，exit 1；预期失败成立。 |
| Task Green / 文档后 verify | `479154e35119d2bc0211636edc4057f032188b07` | 各 6 项通过、0 失败、0 跳过，exit 0；具有独立 Docker 回执。 |
| Task 候选测试与发布 | `9bd0306cf3d81857319f0d9c828e378c6f3459e1` | 6 项通过；[PR #29](https://github.com/xfork24/devdesk-integration-test/pull/29) merged，head / merge_commit 均等于该候选 SHA，另经 GitHub API 核对。 |
| Feature 整体测试 | `496fed16ad0966deeb850e63a5cf62023105e753` | Task 与独立 Feature 套件共 12 项通过、0 失败、0 跳过，exit 0。 |
| Feature 候选测试与发布 | `3d165e4635e82119788ccf625007e6a3e8bcd231` | 12 项通过；[PR #30](https://github.com/xfork24/devdesk-integration-test/pull/30) merged，head / merge_commit / 远端 main 均为该候选 SHA。 |
| 显式 Task 完成 | Task `ce5417bf-a779-456b-8cee-4bc900ecc9b6` | `tasks.complete` 实际回执 completed，完整 18 步 Task 子流程通过。 |

候选均核对真实父提交、diff、同 SHA 测试、输入和产物；PR #29/#30 及远端 main 另经 `gh api` 独立核对。两份原生数据集的 global 13 / project 44 均通过 SQLite `integrity_check`、`foreign_key_check`。退出后测试 Host 与 worker 已停止，无运行中的测试容器，临时 auth.json 为 0；日常数据、系统策略和原始登录文件未修改。

复现使用本报告文末的两个原生探针，为各探针传入独立 `--report-directory`，工程探针另指定 `--github-repo xfork24/devdesk-integration-test`。本轮证据根目录为 `/tmp/devdesk-ubuntu-acceptance.Cbt2NL/`，含 `native-custom/`、`native-engineering/`、`codex-git-docker-permissions.log`、`package-report.json`、`database-checks.json`；默认完整检查日志为 `/tmp/devdesk-ubuntu-acceptance-check.log`。日志、数据库、JSON 和临时补验脚本不提交，清理后不保证可读取。

本轮未发现需要修改产品代码的缺陷；提交补充两个计划的 Ubuntu 验收事实、当前状态及平台边界。`cargo fmt --all -- --check`、`git diff --check` 与新增文档链接检查通过。以下保留 macOS 首轮实施与提交前复查的历史结果，其数量和摘要不替代本轮 Ubuntu 证据。

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

真实模型探针只在显式调用时使用账户和专用测试资源。macOS 完整检查、自由业务原生验收、默认工程图与独立包已通过；Ubuntu 当前合同的 Codex 自由业务、模板重置后继续执行、完整默认工程图和独立 Linux 包本轮也已通过，确切结果见前文。按用户要求，本轮不验收 DSH/Ollama；真实 DSH 云模型工程运行仍未执行，DSH 的动态平台工具桥当前明确不支持，独立 Tool Step 不受此限制。不能将 SDK 本地夹具或历史版本报告当作这些范围的实机证明。


## 2026-11-11：显式证据交付与手动 Approve

现场复盘：Task 的实现评审已通过显式输入收到 implementation、documentation、Red、Green 与 verify；并非历史 delivery 没有传入。开发 Agent 将自行补跑的逐次 TDD 日志及源码快照留在隔离执行的 `/tmp/parity-evidence`，仅交付路径和摘要。文档步骤已说明未读取、未确认持久化，评审据此拒绝证明历史 TDD 顺序。另一处配置问题是测试设计 Skill/Spec 允许把实现缺失包装成受控断言，平台只据断言分类接受 Red；开发 Agent 已识别该 Red 不具有业务意义。

修复：

- 默认 Task 添加 `red_baseline`，由开发职责准备最小可调用接口和必要元数据，保留只读独立测试；测试与最小接口一并固定后再执行平台 Red。Skill/Spec 明确禁止将导入、模块或导出缺失改写为业务断言失败。
- 添加带 JSON Schema 的“原始执行证据”Delivery；implementation 的必需 `evidence` 输出交付逐次命令、目录、起止时间、退出码、完整 stdout/stderr 及当时源码和测试的内容快照、SHA-256。只交临时路径不能通过结构校验。
- documentation、review、candidate_review 的 `inputs.evidence` 和 handoff 参数分别显式引用该输出。沿既有 workflow_output/Artifact 路径密封存储；规范任务记录 source_output_id 与 value_digest，不注入未绑定输出或任意历史目录。
- `workflow.step.approve` 提供独立人工例外决定，非空理由及确切 execution_id/input_digest/outputs 为必需；拒绝旧轮次、不同输入和不同产物，允许无关步骤推进造成的实例修订过期。原报告、冻结配置及实际测试回执保持原值。普通 confirm 继续遵守原覆盖策略。
- 执行页对所有 waiting_confirmation 显示审批或校验未通过说明，并提供返工；手动理由逐执行保存。批准后显示人工例外及理由。

验证：配置执行回归 28 项、Schema 合同 11 项、core 3 项通过；Chromium 覆盖自动/人工确认步骤的手动批准、页面重载、独立理由及连续批准；既有 Workflow 编辑与确切输入确认 9 项通过。跨步骤夹具实际执行生产和消费任务，检查原始记录、源输出 ID、摘要、未绑定输出隔离及存储重新打开；临时路径缺少内容被拒绝。夹具记录不作为真实 TDD/模型执行证据。

当前项目已发布并采用 Task Workflow v5、Feature Workflow v6（包含更新后的开发/测试 Skill、Capability 与 Agent 绑定），配置编译和任务预览通过。既有 Feature 与子流程继续引用原冻结 v4，原始 reject 保留，未执行人工放行或自动返工。Host 已重新构建、签名并在原 4317 端口恢复。

限制：本次验证了证据交付、最小可执行基线的配置闭包及审批链路，未替用户重跑新版本的完整真实模型/GitHub 交付。执行证据由 Agent 据真实工具输出整理，其身份不同于平台测试回执；Schema 保证交付结构，内容真实性与 Red/Green 对应关系仍需独立检查。规范任务沿用 512 KiB 大小上限，超过时会明确拒绝，不放宽为任意文件读取。

首次真实复跑在 `red_baseline` 暴露快照顺序缺陷：独立测试已写入 Task 工作树，但还未封存，第二个写入 Agent 因工作树存在未封存修改而被拒绝启动；原进程已停止，未取得模型执行结果，不计为业务通过。流程已修正为“独立测试 → tests_snapshot → red_baseline → red_snapshot → Red”，基线准备消费测试快照，平台 Red 与实现消费新的 Red 快照，两次快照分别保存独立测试及接口准备结果。现有 Git 清洁度、确切修订及只读测试保护规则保持不变。补充依赖及输入来源回归；配置执行 28 项、Schema 11 项再次通过。真实验收探针移除把模块缺失包装成断言失败的旧提示，并新增原始证据内容摘要、规范任务输入及源输出 ID 检查。

第二次真实复跑已证明证据传递有效：文档与独立实现评审的规范任务均收到约 26 KiB 的同一完整证据，2 条真实记录及源码、测试、README 快照摘要逐一匹配平台 Red/Green 的实际 Git 对象，平台独立测试从 13 项断言失败变为 13 项通过。但冻结 Task 要求两轮增量，开发 Agent 只执行一轮并把多轮要求改称“设计建议”；文档如实披露缺口，独立 Reviewer 因 AC-007 未满足正确拒绝。未执行人工例外批准。实现 Step、Skill 和证据 Delivery 说明已补强：逐项消费冻结计划的每个增量，区分独立只读全量套件与开发职责自有的增量测试，中间增量允许全量套件尚未通过；证据结构的 minItems=2 是单增量下限，不是多增量任务只交两条记录的许可。缺少任一要求的历史记录必须阻塞，不能用 README 解释或最终 Green 替代。
