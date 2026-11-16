# 测试组织与入口

测试按边界组织。默认回归使用临时数据集和明确的原生端口替身，不调用付费模型或真实 GitHub 服务。MCP 浏览器用例实际运行 Docker 容器：需要 Linux Docker Engine，首次构建运行资源需要访问镜像与 npm/apt 包源。

| 位置 | 验证内容 |
|---|---|
| `crates/*/src/` | 领域规则、协议解析、权限落实、进程/凭据边界、Host 装配及项目隔离。 |
| `crates/adapters/tests/configured_workflows.rs` | 自由 Role、Workflow v6 发布和重载、确切任务派发、动态 Spec、结构化产物、人工确认、补充输入、重试、返工、子流程集合、取消、祖先暂停、平台调用防伪及真实 Git 工作空间/候选。 |
| `crates/adapters/tests/standard_snapshots.rs` | 不可变规范闭包与关联、通用工作采用、同一 Feature 的不同 Task 实际执行不同 Standard、运行切换与旧依据恢复。 |
| `crates/adapters/tests/conversations.rs` | 原生轮次、消费及停止证明、不可变绑定、未知结果不重发、通用材料提案与输入引用。 |
| `crates/adapters/tests/schema_contracts.rs` | 离线编译全部 Schema / `$defs`，检查当前配置、Serde、实际冻结计划及 StepExecution。 |
| 其他 `crates/adapters/tests/` | SQLite、Artifact、配置修订、Git、运行安装、Tool/Skill、权限及 Docker 资源边界。 |
| `web/tests/` | 浏览器编辑/保存/发布/重载、任务上下文、输入输出引用、阶段与真实子流程、确认规则、配置导航、权限、接入和历史。 |
| `tests/entrypoints/` | 真实 Host 的 HTTP、安全、SQLite 分库、幂等、模板重置、延迟环境准备、退出与重启。 |
| `tests/fixtures/` / `tests/support/` | 协议样本、固定数据和只编译进测试的接口替身。业务示例不是产品限制。 |
| `tests/reporters/node-reporter.mjs` | 随产品嵌入的 Node 测试报告器，返回真实分类、错误消息/堆栈/原因链及隔离测试的 stdout/stderr；测试输出始终作为数据封装，不能伪造顶层分类。 |

## 默认检查

先激活 Python 虚拟环境，安装固定的 Schema 校验依赖；缺少依赖时检查会失败，不再静默跳过。完整浏览器回归还需要 Docker 与 Chromium。

```bash
python3 -m pip install -r scripts/check/requirements.txt
./scripts/check/check.sh
```

入口执行 Permission/出厂/架构检查、CI 依赖门禁的离线测试、风格/类型/Web 构建、Rust、debug/release Host HTTP/数据集/环境准备与 Chromium。接入浏览器用例使用逐测试独立 Host，避免运行准备状态污染其他规格。可使用 `DEVDESK_E2E_PORT=4329` 避免占用日常开发端口。HTTP standalone 检查会将二进制复制到临时空目录，验证运行不依赖源码。

工作流专项：

```bash
cargo test -p devdesk-adapters --test configured_workflows --test conversations --test schema_contracts --locked
DEVDESK_E2E_PORT=4329 npm --prefix web run test:e2e
```

删除的 v3 动作/配方测试由当前执行合同回归替代。当前测试不创建占位 Agent/Session，不将端口替身结果宣称为真实模型或外部系统验收。

## 配置加载性能

配置加载性能使用 `tests/performance/configuration_loading.py`，显式传入冻结的 Host 测试二进制、源数据库副本、全新输出目录和样本数。该入口在线备份 SQLite 并添加 `.performance-copy` 标记，只装配查询，不运行调度器、模型或 Git；普通配置与 SQLite 路径为真实实现。浏览器模式使用明确的实时模型目录夹具，配置查询不模拟，保存原始请求、导航、主线程/空闲 CPU 证据。构建与测量分开执行，基线与优化采样顺序运行。导航同时记录 RPC 数据完成时间和可操作控件出现后的两帧展示时间，避免把缓存命中的零 RPC 误报为零渲染耗时。

```bash
cargo test --locked -p devdesk-host --no-run
python3 tests/performance/configuration_loading.py --test-binary target/debug/deps/devdesk_host-实际摘要 --dataset /独立数据库副本 --output /全新证据目录 --samples 30
# 同进程切页加 --browser；每次重载浏览器配置缓存加 --cold-browser。
# 慢/失败模型目录同条件夹具加 --model-conditions；每条件 samples 次。
# release 使用 cargo test --release --no-run 产生的测试二进制。
# 优化后 RPC 采样增加 --assert-configuration-isolation，检查完整快照与事务次数为零。
node --test tests/entrypoints/configuration_query_cache.mjs
cargo test -p devdesk-adapters --test configuration_catalog configuration_source_scaling -- --ignored --exact --nocapture
```

## 真实资源检查

```bash
./scripts/check/check-live.sh --groups git,docker,permissions
```

真实 Docker 用例要求预先存在固定镜像，验证 Node Red/Green、环境故障、停止/清理、资源归属、嵌套权限和私有状态。Git 用例创建真实临时仓库，验证单父候选、CAS、过期目标、幂等与恢复；不需要 GitHub。

完整 Docker 外部验收包含 16 项原本 `#[ignore]` 的测试：命令 Red/Green、Delivery、Docker 资源、DSH 服务商、权限、运行资源、HTTP/MCP/OAuth。它们保留显式入口以免便携单元测试隐含要求 Docker；E2E CI 实际执行这些测试，不只编译或列出它们。

```bash
# AppArmor Linux 宿主先加载已有的命名策略，不修改 docker-default 或全局限制。
sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install
# 从 manifest 选定的不可变出厂资源准备镜像，检查并冻结本平台 image ID。
python3 tests/entrypoints/native_acceptance.py --docker-only --prepare --reports /tmp/devdesk-native
# 全部 17 项：恢复专用仓库的既有 PR；request 为 PullRequestRequest JSON。
python3 tests/entrypoints/native_acceptance.py --prepare --github-request /专用/pr-request.json --github-repo owner/dedicated-test-repo --reports /tmp/devdesk-native-all
```

GitHub request 的 repository/number 必须指定已存在的验收 PR；测试先读取它，再按原 marker 两次恢复同一回执，并核对 number/head/marker/merged。此项需要本机 `gh` 授权，只访问专用仓库；不要用业务仓库或凭据文件代替 request。入口按测试二进制核对实际执行数量，失败不会被 skip 或重试掩盖，日志、镜像身份与源内容摘要写入指定临时证据目录；执行期间源码输入改变也会使验收失败。已有准备好的镜像时可省略 `--prepare`，但必须提供 `DEVDESK_TEST_CODEX_IMAGE`、`DEVDESK_TEST_DSH_IMAGE`、`DEVDESK_DSH_MOCK_IMAGE`、`DEVDESK_TOOL_TEST_IMAGE`、`DEVDESK_ACCEPTANCE_NODE_IMAGE` 五个确切 `sha256:` ID。外部服务与模型响应使用明确的协议夹具；这不等于通过真实付费模型的工程验收。

DSH 原生 SDK 夹具使用不可变镜像和禁外网容器内的本地端点，不调用真实模型：

```bash
DEVDESK_DSH_MOCK_IMAGE=sha256:… cargo test -p devdesk-adapters --test model_providers pinned_dsh_uses_local_fixture_streams_tools_and_native_resume --locked -- --ignored
```

`tests/live/probe_workflow_workbench.py` 使用当前配置与真实执行器，在专用 GitHub 测试仓库中创建或继续运行。它不按固定业务节点推导行为。需预先配置相应 Agent；可传入精确绑定、输入及补充文本。人工确认默认保留等待，显式 `--confirm` 才确认当前版本，永远不自动覆盖失败完成条件。该入口会调用实际模型和修改测试仓库，只能显式执行；历史 v3 验收报告不代表 v4 已通过同样的原生工程验收。

两项隔离数据集的原生探针使用现有 Codex 登录文件的临时副本，退出时清理登录文件。前者验证自由业务 Role、Skill、结构化输出和后继消费；后者运行默认工程图，使用实际 Docker 测试与 GitHub PR。后者必须指定专用测试仓库，且会产生真实提交和发布：

```bash
python3 tests/live/probe_configured_workflow.py --binary target/debug/devdesk-host --codex-auth-source /专用/auth.json
python3 tests/live/probe_default_engineering_workflow.py --binary target/debug/devdesk-host --codex-auth-source /专用/auth.json --github-repo owner/dedicated-test-repo
```

两者都可用 `--report-directory` 指定临时证据目录。完整工程推荐使用 `target/release/devdesk-host`；通过同一证据目录的 `--resume` 可接管原根实例，先核对中断执行，再以新执行身份重试，保留已接纳成果。工程探针默认最多等待三小时，可用 `--timeout-seconds` 调整验收入口的等待时限，不改变工作流本身的预算或门禁。运行程序先复制二进制，避免同时构建影响正在执行的程序。不要把登录文件或证据写入源码目录。

工程结束后可运行 `python3 tests/live/verify_engineering_evidence.py /临时证据目录`，不调用模型地复核完整状态、无绕过/豁免、精确人工确认、实际测试回执、原始文件/Git blob 以及两个 PR 的确切发布版本。

`probe_permission_package.py` 检查解包发布程序的权限资产、配置发布、当前任务预览、数据库结构及独立运行，不调用模型。其他有效的 `tests/live/` 探针分别覆盖凭据、供应商协议、安装、经验和外部接入，使用前阅读其参数和外部依赖。

macOS 凭据助手专项入口为 `tests/entrypoints/credential_helper_startup.py` 与 `credential_helper_runtime.py`；它们使用隔离签名探针，不读取或保存模型凭据，不替换共享助手。Linux Secret Service 与权限后端须在 Linux 实机单独验证。

日志、JSON/JSONL、截图和临时脚本写入临时目录；计划目录只保留 Markdown 计划、合同和执行报告。

Harness 编排回归：`crates/adapters/tests/orchestration.rs` 验证显式驱动、版本、替代材料、绕过、有界委托和真实原生调用来源（Harness 端口夹具）。`tests/entrypoints/dataset_protocol.py` 验证拒绝旧数据协议且不改写源数据。

`tests/live/probe_harness_orchestration.py` 使用实际 Codex 项目经理和业务 Agent，验证运行时绕过、实际平台调用及暂停重启后的后继 Session。`probe_default_engineering_workflow.py` 使用独立数据集、专用 GitHub 仓库与真实 Codex、Docker、Git 执行默认 Feature/Task；人工确认精确绑定当前材料，不覆盖拒绝。二者必须显式提供二进制与认证源；临时复制凭据在退出时清理。

Standard 的真实两 Task 工程验收入口为 `tests/live/probe_standard_snapshots.py`，使用独立数据集、真实 Codex、Docker 和专用 GitHub 测试仓库，验证不同快照、运行中切换、重启恢复、真实测试及 Git 回执。完整回归在整合到 main 后统一运行。通过同一 `--report-directory` 和 `--resume` 可以保留原实例、接管项目经理，并只恢复已核对的中断执行、修复后的只读目标查询，或对并行任务推进后过期的候选从 target 返工。父调用由子步骤恢复，不重新创建 Task。完成后运行 `python3 tests/live/verify_standard_history.py /临时证据目录`，离线核对 62 个步骤、唯一的规范切换、旧输入输出、零豁免、每项 Task 的两轮原生命令 Red/Green 与三个精确测试后发布版本。
