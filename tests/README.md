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
| `tests/browser/` | 浏览器编辑/保存/发布/重载、任务上下文、输入输出引用、阶段与真实子流程、确认规则、配置导航、权限、接入和历史。 |
| `tests/entrypoints/` | 真实 Host 的 HTTP、安全、SQLite 分库、幂等、模板重置、延迟环境准备、退出与重启。 |
| `tests/entrypoints/test_docs_checker.py` | 离线文档检查器回归：独立文件索引、增删检测、生成稳定性、忽略本地构建产物、二进制资源和链接/锚点。 |
| `tests/entrypoints/lifecycle_contract.py` | 100 次同版本初始化竞争、Host 实例围栏、原回执重放、失败重试及响应丢失恢复；独立临时数据，不代表 100 前端容量验收。 |
| `tests/fixtures/` / `tests/support/` | 协议样本、固定数据和只编译进测试的接口替身。业务示例不是产品限制。 |
| `plugins/software-development/runtime/node-reporter.mjs` | 软件开发包的 Node 测试报告器，返回真实分类、错误消息/堆栈/原因链及隔离测试的 stdout/stderr；测试输出始终作为数据封装，不能伪造顶层分类。 |

## 默认检查

先激活 Python 虚拟环境，安装固定的 Schema 校验依赖；缺少依赖时检查会失败，不再静默跳过。完整浏览器回归还需要 Docker 与 Chromium。

```bash
python3 -m pip install -r scripts/check/requirements.txt
./scripts/check/check.sh
```

入口执行 Permission/出厂/架构检查、CI 依赖门禁的离线测试、风格/类型/Web 构建、Rust、debug/release Host HTTP/数据集/环境准备与 Chromium。浏览器测试位于 `tests/browser/`，测试工作台源码位于 `tests/fixtures/workbench/`。工作台构建输出和 Playwright 截图分别写入 `target/web/fixture/` 和 `target/web/test-results/`。接入浏览器用例使用逐测试独立 Host，避免运行准备状态污染其他规格。可使用 `DEVDESK_E2E_PORT=4329` 避免占用日常开发端口。HTTP standalone 检查会将二进制复制到临时空目录，验证运行不依赖源码。

工作流专项：

```bash
cargo test -p devdesk-adapters --features test-fixtures,fixture-catalog --test configured_workflows --test conversations --test schema_contracts --locked
npm --prefix web run build:fixture
cargo build --locked -p devdesk-host --features fixture-catalog --target-dir target/fixtures
DEVDESK_E2E_PORT=4329 npm --prefix web run test:e2e
```

删除的 v3 动作/配方测试由当前执行合同回归替代。当前测试不创建占位 Agent/Session，不将端口替身结果宣称为真实模型或外部系统验收。

## 领域制品验收

```bash
export DEVDESK_RELEASE_OUT="$(mktemp -d)"
./scripts/release/package.sh host
./scripts/release/package.sh web
./scripts/release/package.sh software-development
node --test tests/entrypoints/domain_delivery_evidence.mjs
node tests/entrypoints/domain_release.mjs "$DEVDESK_RELEASE_OUT"
```

用 `domain_packages.mjs` 配合解压后的 Host、Web Core 目录和软件 ZIP，验证浏览器与 Python 协议客户端操作同一领域目录、对象、流程和代次，以及真正的 Docker 处理器调用。制品验收也覆盖既有领域版本回切和重启恢复。运行前准备[领域 worker 镜像](../docs/design/run-binary.md#领域工作台独立发行)。

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
cargo test -p devdesk-adapters --features test-fixtures,fixture-catalog --test configuration_catalog configuration_source_scaling -- --ignored --exact --nocapture
```

## 工作台与存储性能

[工作台基准与验收记录](../docs/plans/workbench-storage-performance/VALIDATION.md)给出了 Release Host、生产 Web 和检查点备份的复现入口。工作台基准保留 RPC、浏览器、事件重连和重置的原始样本，并按嵌套路径汇总 Host 计时。历史倍率夹具通过正常命令创建流程及其冻结计划，等待取消终态后封存数据库。浏览器使用 Chromium 和真实嵌入资源。 重置基准的 `--track-reset-progress` 记录全局受理后的首次可见状态。`workbench_reset_recovery.py` 用独立副本在全局提交和首个目标应用后中止进程，再校验重启后的重放回执。

## 单 Host 并发验收设计

[底层并发重构方案](../docs/plans/single-host-concurrency/PLAN.md) 和 [100 前端验收合同](../docs/plans/single-host-concurrency/ACCEPTANCE.md) 定义待实施的并发测试入口、数据规模、负载与门禁。目标是单 Host/SQLite 在工作流持续推进时支持 100 个活跃 Web 前端，覆盖同项目热点、多项目独立选择、持续混合读写、突发竞争与重连。现有 `configuration_loading.py` 关闭调度器，只能作为离线查询基准，不能代替该验收。

正式容量成绩使用固定 release Host/Web、独立数据集、独立负载发生器和实际工作流推进证据；可控原生端口夹具与真实 Docker/Git 执行分别出具结果。测试脚本、100 前端浏览器队列及正式验收尚待实施，不能将文档门禁记作通过。

## 真实资源检查

```bash
./scripts/check/check-live.sh --groups git,docker,permissions
```

真实 Docker 用例要求预先存在固定镜像，验证 Node Red/Green、环境故障、停止/清理、资源归属、嵌套权限和私有状态。Git 用例创建真实临时仓库，验证单父候选、CAS、过期目标、幂等与恢复；不需要 GitHub。

完整 Docker 外部验收覆盖原本 `#[ignore]` 的测试：命令 Red/Green、Delivery、Docker 资源、DSH 服务商、权限、运行资源、HTTP/MCP/OAuth。执行前从实际测试二进制枚举每个套件，并逐项核对执行数量；Codex/DSH 镜像须通过环境变量显式选择，不回退旧版固定标签。它们保留显式入口以免便携单元测试隐含要求 Docker；E2E CI 实际执行这些测试，不只编译或列出它们。

```bash
# AppArmor Linux 宿主先加载已有的命名策略，不修改 docker-default 或全局限制。
sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install
# 从 manifest 选定的 Harness 与 Sandbox 用 Rust 组合器生成配方并准备镜像，检查并冻结本平台 image ID。
python3 tests/entrypoints/native_acceptance.py --docker-only --prepare --reports /tmp/devdesk-native
# 加入 GitHub 专项，恢复专用仓库的既有 PR；request 为 PullRequestRequest JSON。
python3 tests/entrypoints/native_acceptance.py --prepare --github-request /专用/pr-request.json --github-repo owner/dedicated-test-repo --reports /tmp/devdesk-native-all
```

GitHub request 的 repository/number 必须指定已存在的验收 PR；测试先读取它，再按原 marker 两次恢复同一回执，并核对 number/head/marker/merged。此项需要本机 `gh` 授权，只访问专用仓库；不要用业务仓库或凭据文件代替 request。入口按测试二进制核对实际执行数量，失败不会被 skip 或重试掩盖，日志、镜像身份与源内容摘要写入指定临时证据目录；执行期间源码输入改变也会使验收失败。Pi 套件通过 Host `runtime.prepare` 准备并记录不可变镜像，再用本地模型夹具验收。Codex 和 DSH 已有准备好的镜像时可省略 `--prepare`，但必须提供 `DEVDESK_TEST_CODEX_IMAGE`、`DEVDESK_TEST_DSH_IMAGE`、`DEVDESK_DSH_MOCK_IMAGE`、`DEVDESK_TOOL_TEST_IMAGE`、`DEVDESK_ACCEPTANCE_NODE_IMAGE` 五个确切 `sha256:` ID。外部服务与模型响应使用明确的协议夹具；这不等于通过真实付费模型的工程验收。

DSH 原生 SDK 夹具使用不可变镜像和禁外网容器内的本地端点，不调用真实模型：

```bash
DEVDESK_DSH_MOCK_IMAGE=sha256:… cargo test -p devdesk-adapters --features test-fixtures,fixture-catalog --test model_providers pinned_dsh_uses_local_fixture_streams_tools_and_native_resume --locked -- --ignored
```

`tests/live/probe_workflow_workbench.py` 使用当前配置与真实执行器，在专用 GitHub 测试仓库中创建或继续运行。它不按固定业务节点推导行为。需预先配置相应 Agent；可传入精确绑定、输入及补充文本。人工确认默认保留等待，显式 `--confirm` 才确认当前版本，永远不自动覆盖失败完成条件。该入口会调用实际模型和修改测试仓库，只能显式执行；历史 v3 验收报告不代表 v4 已通过同样的原生工程验收。

两项隔离数据集的原生探针使用现有 Codex 登录文件的临时副本，退出时清理登录文件。前者验证自由业务 Role、Skill、结构化输出和后继消费；后者运行默认工程图，使用实际 Docker 测试与 GitHub PR。后者必须指定专用测试仓库，且会产生真实提交和发布：

```bash
python3 tests/live/probe_configured_workflow.py --binary target/debug/devdesk-host --codex-auth-source /专用/auth.json
python3 tests/live/probe_streamlined_workflow.py --binary target/release/devdesk-host \
  --domain-zip dist/software-development-0.5.2.zip --mode agent \
  --codex-auth-source /专用/auth.json --github-repo owner/dedicated-test-repo \
  --requirement-file /专用/requirement.md --report-directory /新的证据目录 --performance
```

两者都可用 `--report-directory` 指定临时证据目录。完整工程推荐使用 `target/release/devdesk-host`；通过同一证据目录的 `--resume` 可接管原根实例，先核对中断执行，再以新执行身份重试，保留已接纳成果。工程探针默认最多等待 30 分钟，可用 `--timeout-seconds` 调整验收入口的等待时限，不改变工作流本身的预算或门禁。运行程序先复制二进制，避免同时构建影响正在执行的程序。不要把登录文件或证据写入源码目录。

软件开发包支持 `--mode manual`、`--mode rust` 和 `--mode agent`。当前验收应核对完整状态、无绕过/豁免、确切人工确认、真实 Red/Green 与两类回归、原始文件/Git blob，以及 Task/Feature 两个 PR 的确切合入和同步版本。`probe_streamlined_workflow.py` 等待根流程完成且经理关闭后才停止自己启动的 Host，并清理复制的认证。历史详细版的完整证据仍可使用 `python3 tests/live/verify_engineering_evidence.py /临时证据目录` 复核；该工具按其原详细版节点合同工作，不用于精简版。

发布包中的核心权限预设由 `domain_release.mjs` 和 Rust 权限回归验证。其他 `tests/live/` 探针覆盖凭据、供应商协议、安装、经验和外部接入。运行前阅读其参数和外部依赖。

macOS 钥匙串的后台调用和显式授权隔离由 `cargo test -p devdesk-adapters --lib credentials_macos` 检查。签名后的 Host 可用独立测试数据集验证系统钥匙串访问。Linux Secret Service 与权限后端须在 Linux 实机单独验证。

日志、JSON/JSONL、截图和临时脚本写入临时目录；计划目录只保留 Markdown 计划、合同和执行报告。

Harness 编排回归由 `crates/adapters/tests/orchestration.rs` 验证显式驱动、版本、替代材料、绕过、有界委托和原生调用来源。`tests/entrypoints/dataset_protocol.py` 覆盖不兼容旧库保护及 v24 → v25 离线升级。

已有旧版 Host 时，可用本地模型重演旧讨论、权限漂移、后继会话与升级后的 Pi 工具链。探针创建隔离数据集并执行实际 Docker，无需系统凭据。

```bash
python3 tests/entrypoints/pi_harness.py --binary target/debug/devdesk-host \
  --v24-binary /path/to/v24/devdesk-host --reports /tmp/devdesk-pi-upgrade
```

同一入口以 `--provider opencode` 或 `--provider mcode` 核验正式 Runtime 配方、续跑、取消、冻结权限、两个依赖步骤的实际平台回执及项目 HTTP 回执。可加 `--v25-binary` 使用批次 A Host 先准备隔离数据集，验证批次 B 更新目录和显式重绑后的历史恢复。

```bash
python3 tests/entrypoints/pi_harness.py --provider opencode \
  --binary /path/to/release/devdesk-host --reports /tmp/devdesk-opencode-product
```

批次 B 的 `harness_g0.py` 在真实容器中运行固定版本的 OpenCode、mcode 或 ZCode，使用宿主回环上的测试模型和 MCP 服务。它在全新的报告目录保留协议与 HTTP 审计供 P0 核对。`--image` 接受预先准备的镜像。磁盘空间有限时，OpenCode 可选 `--executable` 挂载已校验的 Linux 二进制，mcode 和 ZCode 可选 `--installation` 挂载容器内安装后的 npm prefix。相关版本和结果见 [Harness 实施记录](../docs/plans/harness-shared-foundation/EXECUTION.md#macos-验证结果)。

```bash
python3 tests/entrypoints/harness_g0.py --provider opencode \
  --image node:24.13.0-bookworm-slim --executable /path/to/verified/linux/opencode \
  --reports /tmp/devdesk-opencode-g0
```

Linux 宿主需用 `--listen` 指向本地 Docker 网桥地址。mcode 可以在两次运行中分别验证项目配置发现和 `--mask-project-config` 下的只读覆盖。ZCode 的 `--zcode-account-probe` 在断网容器中验证账号请求关联、宿主拒绝供给的终态，以及夹具认证能进入模型请求阶段。

`tests/live/probe_harness_orchestration.py` 使用实际 Codex 项目经理和业务 Agent，验证运行时绕过、实际平台调用及暂停重启后的后继 Session。`probe_streamlined_workflow.py` 使用独立数据集、专用 GitHub 仓库与真实 Codex、Docker、Git 执行默认 Feature/Task；人工确认精确绑定当前材料，不覆盖拒绝。二者必须显式提供二进制与认证源；临时复制凭据在退出时清理。

`tests/live/probe_mixed_harness_workflow.py` 先验证软件开发 ZIP 中的跨 Harness Standard 可以被项目选择，再以单独的验收 ZIP 运行五步有依赖的 Agent 调度。通过标准输入提供 MiniMax key，并显式指定证书签名的 Host、真实 Codex 认证源、软件开发 ZIP 和新证据目录。它核对每步的冻结 Harness、模型、产物和实际 Codex 调度回执。`--feature-entry` 运行正式 Feature 入口并观察第一个人工门禁。

Standard 捕获、采用、历史冻结与项目隔离由 `crates/adapters/tests/standard_snapshots.rs` 覆盖。当前出厂精简版的真实完整工程使用 `probe_streamlined_workflow.py`；`probe_standard_snapshots.py` 和 `verify_standard_history.py` 保留用于旧版本独立两 Task 的历史证据复核，不作为当前出厂默认验收入口。

Workflow 执行性能的显式组件入口为 `crates/adapters/tests/workflow_performance.rs`，通过 `DEVDESK_WORKFLOW_PERF_OUTPUT` 指定全新输出文件，测量确定性隔离流程；`frozen_history_performance.rs` 只读取明确选择的备份，使用 `DEVDESK_HISTORY_DATASET`、`DEVDESK_HISTORY_PROJECT` 和 `DEVDESK_HISTORY_OUTPUT`。两者默认 ignored，不启动模型或恢复原 worker。它们不是 100 前端或完整原生往返验收。

实际执行链另有三种显式入口，不使用暂停的业务 Feature：

- `DEVDESK_WORKFLOW_LIVE_OUTPUT=/新的证据目录 cargo test --release -p devdesk-host actual_git_docker_workflow_handoffs -- --ignored --nocapture`：真实 Host 调度、SQLite、Git、Artifact、6 次 Docker Node 测试。初始化场景用实际本地 Git 回执登记工作空间，生产推进使用正常 Rust driver；不模拟 Docker/工具/接纳，不调用模型或 GitHub。
- `python3 tests/performance/workflow_runtime.py --binary /冻结的/devdesk-host --codex-auth-source /专用认证源/auth.json --output /新的证据目录`：实际 Codex 经理与 Agent 完成独立流程，输出原生收包到写入、经理/Agent 耐久登记、逐调用 native_phase 及启动时间线。需开启对应源码测点；认证只用于显式真实模型验收，退出删除临时副本。样本不足不视为 SLA/100 前端通过。
- `python3 tests/live/probe_native_shutdown.py --binary /冻结的/devdesk-host --codex-auth-source /专用认证源/auth.json --output /新的证据目录`：实际经理原生请求时发送 SIGTERM，验证 Host 清理、旧 writer 撤权、重启不重发原输入，随后由后继经理自主完成独立技术流程。加 `--during-startup` 在实际容器运行、首条输入尚未消费时停机，另核对 consumed=0。它不是业务 Feature 的恢复入口。

`python3 tests/entrypoints/host_launcher.py` 在 macOS 以一次性 worker 验证独立进程组、SIGTERM/SIGHUP 转发、退出日志和子进程清理；其他系统显式跳过。签名验证在此测试中替换，实际子进程和信号使用生产启动器。

## 项目经理自主 Feature 验收

默认工程探针以 start_workflow=false 保存草稿，再通过一次 workflow.start 启动 agent 根驱动。新场景不注入 business_facts、不发送编排指导。人类审批必须由经理先发起并准备材料，操作者在项目经理页面作出决定；探针只观察。严格 unattended 仍单独表示不能出现任何人类决定，不能通过删除默认确认点获得该成绩。

完成后 engineering_evidence 调用 autonomous_orchestration，审计原生 Turn 输入、Controller/Segment 来源、零补充指导、原始人类决定、计划和全部工程证据。其负向回归为 python3 -m unittest discover -s tests/live -p 'test_autonomous_orchestration.py'。真实页面的只读验证入口为 node tests/live/verify_manager_page.mjs /证据目录/human-request-ID.json，检查本轮材料、上下文、执行记录的真实导航及桌面/窄屏截图；不提交人类决定。


## 工作流读取与审批观察专项

`python3 -m unittest discover -s tests/live -p test_workflow_observer.py` 覆盖精简版和子流程探针审批请求先保存后检查、诊断失败继续观察、重启端口变化和原件不可覆盖、两类材料联合覆盖、确切版本及只读观察。生产命令合同另由 `configured_workflows` 中的 `human_interaction_uses_production_union_and_exact_execution_context` 验证；单独运行适配器测试时加 `--features test-fixtures,fixture-catalog`。这些入口不调用模型或推进真实 Feature。

`tests/performance/workflow_reads.py --test-binary <release测试入口> --dataset <专用兼容备份> --output <新证据目录> --samples 200` 通过关闭调度的实际 HTTP 服务测 control、状态页及执行详情，保留逐请求错误、bytes、内容摘要和 trace 关联。结果只用于本机定位，不等于原生往返或正式容量。`joint_frontends.py --browser` 的 1/2 页面诊断支持混合读、写和导航，较高并发仍分别记录动作数和真实 RPC；导航取消只对已观测到的只读请求单列，修改失败不豁免。

`joint_frontends.py` 的本机协议模式可用 `--scale 1 --projects 10 --seconds 120 --renew --trace --sse-configuration --sse-reconnect --reconnecting-clients 10`，让 10 个客户端各重连 10 次，剩余客户端维持连接。输出 `sse-reconnections.json` 记录重连游标、收到首条消息和读回项目状态的时延。

`DEVDESK_STORAGE_PROFILE_OUTPUT=/tmp/全新目录 cargo test --release --locked -p devdesk-adapters --features test-fixtures,fixture-catalog --test workflow_storage_scaling workflow_storage_growth_profile -- --ignored --exact --nocapture` 创建四个隔离数据集，分别测冷实例数、当前 DAG 及真实历史尝试的十倍增长；每项预热另存、30 个组件样本，保存版本回读、WAL/checkpoint、进程 I/O 及完整性结果。只测空正文 Control 和可信单步骤存储 patch，没有模型、外部 writer 或业务并发，不代表业务延迟、真实执行或容量通过。使用归档二进制时把构建与采样分开。

预算专项在 `configured_workflows` 中用可控 UTC/单调时钟、真实 SQLite 验证显式 time_limits v1 的 Human 截止、签署/补充输入、技术重试/重启、时钟倒退及计划采用；缺省旧预算的序列化由 schema_contracts 验证。新项目基线为 62，支持显式迁移 60 和 61。旧 59 数据集拒绝测试核对原文件字节未改写。

`DEVDESK_TRANSACTION_PROFILE_OUTPUT=/tmp/全新文件 cargo test --release --locked -p devdesk-adapters --test resource_commits transaction_observation_profile -- --ignored --exact --nocapture` 在新全局库交错执行启用/关闭 Trace 的同尺寸提交，预热另存，分别保存千样本。计时不含报告文件/日志输出，结果只定位内存观测开销；逐事务记录的上限、冲突/回滚、重放及内容排除另有默认回归。

`configured_workflows::out_of_bounds_review_is_repaired_by_lint_in_the_same_turn_before_admission` 使用真实 SQLite 和生产原生工具桥合同，验证 63 行文本的越界草稿先被 lint 拒绝、同一 Turn 依照冻结位置指导修正后接纳，没有新评审尝试或历史覆盖。该确定性端口测试不调用付费模型。
