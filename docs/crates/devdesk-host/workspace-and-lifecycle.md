# 多项目装配、生命周期和 GitHub 工程

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

Workspace 把目录、工程库与真实端口组合为服务。后台持续推进所有有待办工作的已打开工程，不依赖哪个项目当前显示在界面。准备、调度、关闭各自保持明确的锁和停止边界。

## 本页文件

- [src/workspace.rs](#file-src-workspace-rs)：全局与工程 Engine 装配、请求选择和共享配额
- [src/github_projects.rs](#file-src-github-projects-rs)：GitHub 项目身份与异步仓库准备
- [src/runtime_setup.rs](#file-src-runtime-setup-rs)：执行安装端口的薄装配层

<a id="file-src-workspace-rs"></a>

## src/workspace.rs：全局与工程 Engine 装配、请求选择和共享配额

源码：[crates/host/src/workspace.rs](../../../crates/host/src/workspace.rs)。

Workspace 持有 GlobalStore、全局配置 Engine、已打开工程表、active 选择、模板门和 RuntimePreparation。open_with_adapters 是可注入的装配入口，生产 open 则创建真实执行适配器。每个 Engine 使用自己的存储/Artifact 根，配置后端与原生执行器可以共享。

打开时装配全局配置 Engine，并分页打开目录内所有工程、恢复配置 transfer/工具回执及工作流事实。项目 reserve/publish 与加载分开；尚未发布的预留项目不能用于普通请求。`project_engine(project_id)` 按显式 ID 从 Engine 表查找；缺少 ID 返回 project_required，未知项目返回 not_found。Host 不保存或恢复活动项目，选定 Arc<Engine> 后，本次处理固定到该库。`project.open` 已撤销，Web 选择只改变本标签页的 URL。跨项目只读比较等专用入口继续校验各自范围。

全局讨论通过支持该范围的方法加 scope=global 路由，与前端所选项目无关；它拒绝同时指定 project_id。首次访问时才打开 conversations/global/engineering.sqlite3 的独立 Engine，以 GLOBAL_DISCUSSION_NAMESPACE 放入 opened 表，随后参与调度，不创建 Project 行。全局配置 Engine 则使用 global.sqlite3；query.history 的 scope=global 和未指定 project_id 的 /events 指向这份配置库，不能把它们当作全局讨论消息查询。重启打开 Workspace 不会立即装配全局讨论 Engine，相关请求到来后才载入其旧状态。

`tick` 先处理共享工具，再筛选 has_pending_work 的已打开 Engine，按总并发预算推进 workflow、repository、conversation 和 Engine 后台工作。没有前端查看的工程及已经装配的全局讨论继续执行；不同工程已有会话占用同一配额。长时间安装准备不持有项目创建的旧协调锁。每阶段检查停机信号；stop_owned_runtime 遍历已打开 Engine，核对停止其 Harness writer，未启动输入继续排队。退出不是 workflow.cancel，也不表示已撤销外部工具或远端副作用。

开发用 `scripts/dev/run-host.py` 在 macOS 保存独立签名副本，但默认仍是前台启动器，不会脱离调用它的终端或工具进程组。需要跨对话轮次持续运行时，应由独立进程会话或系统服务托管，并持久保存标准输出和错误日志。启动器记录 launcher/Host PID、进程组、收到的信号与子进程退出码，便于区分外部回收和应用崩溃。服务存活须通过 HTTP 核对；SQLite 中的 running 只表示最后保存的状态。恢复中断会话时先核对 writer 停止证明和已发生的工具回执，再建立后继经理会话，不能盲目重发原输入。

HTTP 退出、SIGINT 以及 Unix SIGTERM/SIGHUP 都停止调度准入并广播关闭通知，SSE 连接不会抢走唯一的停机唤醒。退出记录 shutdown_requested 的来源，完成所属 Harness writer 清理后记录 runtime_stopped；清理错误不记录成功终点。SIGKILL 无清理机会，重开后仍须核对，不伪造停止证明。

原生 worker 的 Docker/SDK 启动由适配器后台完成，核心只轮询已经占用额度的原 execution。Host 退出同样停止这些 Prepared/Dispatching 执行；启动线程不写数据库或发送业务输入，停止与创建通过同一 worker 锁协调。专用真实探针可在容器已启动而输入尚未发送时终止 Host，验证未消费、未重发及后继恢复。该边界不表示强制 SIGKILL 的在途 Docker 子进程已被验证安全回收，也不将所有外部工具 I/O 声明为异步。

文件末集成测试覆盖项目创建/选择失败、请求隔离、恢复、共享预算和关闭等行为。`workflow_live.rs` 提供显式、默认忽略的实际 Git/Docker 自动工作流，不使用模型或生产数据。新增入口应复用明确的 global/project 分派，避免 handler 直接操作 active 内部状态。

代码定位：[`Workspace`](../../../crates/host/src/workspace.rs#L41)、[`open_with_adapters`](../../../crates/host/src/workspace.rs#L96)[`dispatch`](../../../crates/host/src/workspace.rs#L385)、[`event_engine`](../../../crates/host/src/workspace.rs#L1159)、[`tick`](../../../crates/host/src/workspace.rs#L1201)、[`stop_owned_runtime`](../../../crates/host/src/workspace.rs#L1528)。

关联阅读：[adapters/src/catalog.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-catalog-rs)、[adapters/src/session_resources.rs](../devdesk-adapters/runtime-preparation.md#file-src-session-resources-rs)、[host/src/governance.rs](rpc-and-configuration.md#file-src-governance-rs)、[host/src/github_projects.rs](workspace-and-lifecycle.md#file-src-github-projects-rs)、[core/src/lib.rs](../devdesk-core/engine-and-ports.md#file-src-lib-rs)。

## src/lifecycle.rs：Host 实例版本

`/setup` 和 `/shutdown` 校验 `instance_id`、`expected_revision`、`command_id`。短临界区仅登记命令与回执，生命周期切换通过 CAS；初始化 I/O 在工作线程执行，不持有 workspace 锁。完成任务由 Host 持有，客户端断开不会丢弃完成处理；停止改变代次，迟到初始化不能接入。重启产生新的实例身份，旧页面请求被拒绝。进程内回执不代替数据库业务回执。

源码：[crates/host/src/lifecycle.rs](../../../crates/host/src/lifecycle.rs)。

<a id="file-src-github-projects-rs"></a>

## src/github_projects.rs：GitHub 项目身份与异步仓库准备

源码：[crates/host/src/github_projects.rs](../../../crates/host/src/github_projects.rs)。

repository_name 严格解析 owner/repo 和分支输入；open_github_project 先诊断 gh 与仓库事实，再按目录中稳定项目身份创建或复用工程。项目公开与实际仓库 clone/准备由不同阶段完成。

仓库操作保存 durable intent，后台推进并在重启后 reconciliation。仓库尚未 ready 时不能创建需要其实际内容的 Feature。重复请求复用原项目和操作，不因为界面重试留下重复目录。

Host 负责把 gh 事实、目录和 Engine 命令接起来，实际 Git 执行在 adapters，仓库准备终态由 core 接纳。外部认证失败应保留原错误，不用一个空本地仓库掩盖。

代码定位：[`repository_name`](../../../crates/host/src/github_projects.rs#L4)、[`open_github_project`](../../../crates/host/src/github_projects.rs#L27)。

关联阅读：[adapters/src/github.rs](../devdesk-adapters/engineering-resources.md#file-src-github-rs)、[core/src/repository_runtime.rs](../devdesk-core/integrations-and-repositories.md#file-src-repositories-rs)、[host/src/workspace.rs](workspace-and-lifecycle.md#file-src-workspace-rs)。

<a id="file-src-runtime-setup-rs"></a>

## src/runtime_setup.rs：执行安装端口的薄装配层

源码：[crates/host/src/runtime_setup.rs](../../../crates/host/src/runtime_setup.rs)。

此文件只有三个包装入口：ensure_runtime 通过 SessionResources 准备 provider 安装；ensure_codex_auth 准备独立登录组件；materialize_harness 调用 Engine 的完整资源装配用例。

策略、冻结选择和 durable 回执不在这里复制。新增准备能力应先归入 core 用例或 adapters 端口实现，再由 Host 组合，避免此小模块变成第二套运行状态机。

代码定位：[`ensure_runtime`](../../../crates/host/src/runtime_setup.rs#L19)、[`ensure_codex_auth`](../../../crates/host/src/runtime_setup.rs#L26)、[`materialize_harness`](../../../crates/host/src/runtime_setup.rs#L29)。

关联阅读：[adapters/src/session_resources.rs](../devdesk-adapters/runtime-preparation.md#file-src-session-resources-rs)、[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)、[host/src/governance.rs](rpc-and-configuration.md#file-src-governance-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
