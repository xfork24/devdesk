# 测试组织与入口

测试按边界组织。默认回归使用临时数据集和明确的原生端口替身，不调用付费模型或真实 GitHub 服务。

| 位置 | 验证内容 |
|---|---|
| `crates/*/src/` | 领域规则、协议解析、权限落实、进程/凭据边界、Host 装配及项目隔离。 |
| `crates/adapters/tests/configured_workflows.rs` | 自由 Role、Workflow v4 发布和重载、确切任务派发、动态 Spec、结构化产物、人工确认、补充输入、重试、返工、子流程集合、取消、祖先暂停、平台调用防伪及真实 Git 工作空间/候选。 |
| `crates/adapters/tests/conversations.rs` | 原生轮次、消费及停止证明、不可变绑定、未知结果不重发、通用材料提案与输入引用。 |
| `crates/adapters/tests/schema_contracts.rs` | 离线编译全部 Schema / `$defs`，检查当前配置、Serde、实际冻结计划及 StepExecution。 |
| 其他 `crates/adapters/tests/` | SQLite、Artifact、配置修订、Git、运行安装、Tool/Skill、权限及 Docker 资源边界。 |
| `web/tests/` | 浏览器编辑/保存/发布/重载、任务上下文、输入输出引用、阶段与真实子流程、确认规则、配置导航、权限、接入和历史。 |
| `tests/entrypoints/` | 真实 Host 的 HTTP、安全、SQLite 分库、幂等、模板重置、延迟环境准备、退出与重启。 |
| `tests/fixtures/` / `tests/support/` | 协议样本、固定数据和只编译进测试的接口替身。业务示例不是产品限制。 |
| `tests/reporters/node-reporter.mjs` | 随产品嵌入的 Node 测试报告器，返回实际断言和环境错误分类。 |

## 默认检查

```bash
./scripts/check/check.sh
```

入口执行 Permission 资产检查、Web 构建、Rust、debug/release Host HTTP 与 Chromium。可使用 `DEVDESK_E2E_PORT=4329` 避免占用日常开发端口。HTTP standalone 检查会将二进制复制到临时空目录，验证运行不依赖源码。

工作流专项：

```bash
cargo test -p devdesk-adapters --test configured_workflows --test conversations --test schema_contracts --locked
DEVDESK_E2E_PORT=4329 npm --prefix web run test:e2e
```

删除的 v3 动作/配方测试由当前执行合同回归替代。当前测试不创建占位 Agent/Session，不将端口替身结果宣称为真实模型或外部系统验收。

## 真实资源检查

```bash
./scripts/check/check-live.sh --groups git,docker,permissions
```

真实 Docker 用例要求预先存在固定镜像，验证 Node Red/Green、环境故障、停止/清理、资源归属、嵌套权限和私有状态。Git 用例创建真实临时仓库，验证单父候选、CAS、过期目标、幂等与恢复；不需要 GitHub。

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

两者都可用 `--report-directory` 指定临时证据目录。运行程序先复制二进制，避免同时构建影响正在执行的程序。不要把登录文件或证据写入源码目录。

`probe_permission_package.py` 检查解包发布程序的权限资产、配置发布、当前任务预览、数据库结构及独立运行，不调用模型。其他有效的 `tests/live/` 探针分别覆盖凭据、供应商协议、安装、经验和外部接入，使用前阅读其参数和外部依赖。

macOS 凭据助手专项入口为 `tests/entrypoints/credential_helper_startup.py` 与 `credential_helper_runtime.py`；它们使用隔离签名探针，不读取或保存模型凭据，不替换共享助手。Linux Secret Service 与权限后端须在 Linux 实机单独验证。

日志、JSON/JSONL、截图和临时脚本写入临时目录；计划目录只保留 Markdown 计划、合同和执行报告。
