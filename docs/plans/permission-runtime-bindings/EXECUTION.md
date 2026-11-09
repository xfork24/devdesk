# Permission 资源与运行时版本绑定实施记录

日期：2026-11-09。实现已完成；macOS arm64 的自动化、Docker 原生边界、真实 Codex 改绑和独立发布包已验证。Ubuntu AppArmor、DSH 真实模型传输及完整进程崩溃注入矩阵未在本轮重跑，不能从历史报告推定通过。

## 交付行为

- `permissions/presets/` 与 UI 共用 v2 完整 JSON。Harness 声明文件、工具网络、工具集合及审批；Sandbox 声明文件和整个容器网络。未知字段、显式 null、不适用维度、无效范围均拒绝。
- 保存只增加不可变 profile 修订；导出包含前序闭包，导入检查连续修订与内容冲突，批次原子入库。文件与存储对象按相同的 JSON 规范形式计算摘要，v1 的原始顺序和历史摘要不改写。
- 加载来源为数据集、发布包或开发源码的明确目录，以及用户显式登记的目录。Host 启动后可以新增 JSON 并重新加载，无需重新编译或重启；加载不自动应用。逐文件来源保存规范路径和原始 SHA-256，不执行策略脚本，也不跟随符号链接。
- 绑定标识精确 Harness/Sandbox 配置修订及内容摘要，另锁定执行合同。应用按目标活动修订做 CAS，保存不可变绑定和分配记录；选择历史 profile 再应用即形成可审计回退。全局与项目目标分别事务提交，混合数据库的批次明确拒绝。
- Agent 使用命名的 `permission_selection`，两层分别 `follow_target` 或 `pinned`。新目标未设置活动绑定时不能冻结执行；固定 Agent 保持原授权。设置页展示关联 Agent 的追随或固定状态。
- 目标预检只验证结构兼容，明确 `scope_checked: false`。完整预检与实际派发共用解析编译器，后者使用协调器确定的正式 ExecutionScope。无法执行的规则在模型调用前拒绝。
- 追随目标的讨论在下一次输入前检查绑定。空闲时先持久化切换意图，取得旧 writer 停止证据，再原子创建带 Snapshot 的后继 Session、接受原输入并记录 `effective_session_id`。旧 RuntimeBinding、原生线程和证据不覆盖。UI 自动跟随后继，也可查看前序或后继讨论。
- 活动、排队、待审批或待核对的输入保持接受时的冻结权限。切换失败保留原命令和待核对状态，可恢复原输入；重试继续原切换意图，不悄悄回退或重新读取另一次目标更新。命令重放和跨后继的消息 ID 去重均有持久化记录。
- 新 Feature 冻结确切权限、绑定、目标和执行合同闭包；后续 Run 与技术重试不追随目标默认值。旧冻结继续可读，不伪造 v2 锁。
- 对可证明等价的旧 Agent 追加固定选择投影，不修改原 Agent 或 v1 Permission。无法等价转换的 legacy 动态规则与审批组合记录为 `legacy_fixed`，继续旧合同。旧设置收在兼容入口，v2 不再走数组位置和 provider 默认覆盖链。

## 实现位置与协议

| 层 | 入口 |
|---|---|
| 领域身份与冻结校验 | `crates/domain/src/permission_profile.rs`；`ExecutionPermissions.binding_lock` 与 `AgentComposition.permission_selection` 为兼容可选字段 |
| 修订、导入与 CAS | `crates/core/src/permission_resources.rs` |
| 讨论切换及恢复 | `crates/core/src/permission_switch.rs`、`crates/host/src/permission_resources.rs` |
| 文件加载、编译与旧选择投影 | `crates/adapters/src/permission_profiles.rs` |
| 工程权限闭包 | `crates/core/src/project_configuration.rs` 的 `feature_permission_lock` |
| UI | `PermissionResources.tsx`、`PermissionSettings.tsx`、`AgentCatalog.tsx`、`RoleConversation.tsx` |
| 持久化 | 全局追加 `007_permission_resources.sql`，项目追加 `036_permission_resources.sql`；不改旧 SQL |
| 契约与预设 | `permission-profile.schema.json`、`permission-binding.schema.json`、`sandbox.schema.json`、RPC Schema；三个 v2 预设与六个旧预设同时发布 |

RPC 包含 `query.permission_resources`、`permission.profile.create/revise/import/export/reload`、`permission.source.register`、`permission.binding.preflight`、`permission.bindings.apply`。Sandbox 通过普通 `sandbox.create/revise` 建立确切资源修订。Session 查询返回冻结、期望绑定和切换状态；原 `conversation.send` 在切换时增加有效 Session 标识。

会话切换使用受信 Host 到 Engine 方法，不向客户端开放任意 RuntimeBinding 注入。审批 `never` 在原生交互产生和回答两个入口再次拒绝；v2 不能借 legacy ID 例外跳过编译检查。`on_request` 仅在原生升级后仍可维持最大授权边界时接受。

## 本轮验证

环境：macOS arm64、Docker Linux arm64 worker；Codex 0.167.0、DSH 0.2.2-rc.1。实际编译合同另包含当前适配器构建摘要，安装包含不可变 Docker image ID；相同供应商版本不替代这些身份。

| 检查 | 结果与证据边界 |
|---|---|
| `cargo test --workspace --locked --no-fail-fast` | 123 通过、9 个外部依赖测试默认忽略；包含旧历史、配置、会话、工程冻结与 Schema 回归 |
| Host 权限专项 | 3 通过：绑定重放、不支持规则拒绝、项目目标隔离、完整发送入口后继采用与消息去重 |
| `permission_profiles` 默认回归 | 10 通过：规范摘要、v1 不变、导入原子性/闭包往返、运行时文件加载、目标 CAS/批次回滚、固定选择、Scope 冲突、迁移重启幂等、后继恢复、队列门禁、Feature 冻结和未知停止结果 |
| Schema | 10 通过；所有发布 Schema 离线编译，v1/v2 预设校验；可独立检查的长度/列表界限与领域一致，工具 allow+deny 合计上限另由领域检查 |
| Chromium 全量 | 14 通过；真实浏览器→HTTP→SQLite，不使用配置 API 替身。新增权限用例完成新建、保存新修订不改绑、预检、应用、回退、闭包导出/导入、登记目录和运行时加载，浏览器无异常 |
| Chromium 最终定向 | 最新 UI 的权限资源用例再次通过；包含关联 Agent 状态、构建/平台/安装登记和会话导航改动后的构建 |
| v2 原生双供应商边界 | 显式运行忽略测试并通过：真实 Codex sandbox 与 DSH SDK shell 的授权文件写入成功、越界写入失败、工具禁网。使用无害凭据标记，不调用模型 |
| 旧权限原生回归 | 3 个忽略测试显式运行并通过：容器全权限、Codex scoped、DSH 受限执行；保留旧执行行为 |
| 真实 Codex 两轮改绑 | 通过；详见下节。临时独立数据集，实际模型工具输出与受控 HTTP 收据，无模型自述替代 |
| HTTP 与独立二进制 | debug HTTP 合同与 release 源码外独立二进制合同通过 |
| 独立运行包 | 解包后从源码外启动，校验二进制、7 个后端资产、9 个预设和 Schema，v2 导入/导出、目标绑定与文件运行时加载通过，项目 Schema 36；无自动下载和模型调用 |
| 构建与静态检查 | Web TypeScript/Vite、Rust release、权限资产校验、rustfmt 与 `git diff --check` 通过。普通 clippy 通过；仓库既有告警使全局 `-D warnings` 不通过，未为本次权限实现修改无关代码 |

### 真实 Codex 改绑证据

`tests/live/probe_permission_runtime.py` 启动临时数据集，仅复制显式给定的现有登录文件；不打开原数据集数据库或修改其项目。登录副本与临时执行数据在结束时清理，不输出凭据。

1. 精确 Harness 与 Sandbox 都应用 v2 full-access，Agent 两层追随目标。真实 `gpt-6.1-sol` 调用原生 shell，在 `/tmp` 写入唯一文件并请求受控 HTTP 服务。原生输出 `PERMISSION_NETWORK_OK`，服务收到对应唯一收据。
2. 同一 Host 运行中，将同一 Harness 目标改绑为 `filesystem: read_only`、工具 `network: disabled`、`approval: never`。向原 Session 发送下一轮，返回不同的 `effective_session_id`。
3. 新原生线程完成实际 shell 调用：向同一 `/tmp` 目录的写入失败（独立于两轮都只读的 `/work` 挂载），请求另一个唯一 HTTP 地址失败；工具以退出码 0 输出 `PERMISSION_DENIED_OK`，服务没有收到该地址请求。模型轮次仍完成，说明工具禁网没有阻断模型通信。该对照证明 Harness 文件限制也在改绑后实际采用。
4. 核对后继 predecessor、不同原生线程、原 Session 冻结绑定和 Turn 列表不变；两次原发送命令重放结果完全相同。

复现命令（会调用真实模型）：

```bash
python3 tests/live/probe_permission_runtime.py \
  --binary target/release/devdesk-host \
  --auth-file /absolute/path/to/auth.json \
  --output /tmp/devdesk-permission-runtime-real.json
```

临时机器证据位于 `/tmp/devdesk-permission-runtime-real.json`、`/tmp/devdesk-permission-rust-final.log`、`/tmp/devdesk-permission-release-final.log`。这些路径只代表本轮现场记录，不作为仓库可长期保留的原始证据；计划目录只提交 Markdown。原生边界与协议故障用例保存在 Rust/Host 测试中，可独立复跑。

## 限制与待补验

- 当前能力合同只验证上述固定 Codex/DSH 版本。Codex 通用工具白名单、网络域名 allowlist、无法落实的任意读取 deny，以及不能维持最大授权范围的原生审批升级均明确拒绝。新增策略数据无需改镜像；增加执行能力仍需发布新后端合同。
- 本轮没有在 Ubuntu 宿主重新安装或核对 AppArmor，也没有运行 DSH 的真实模型传输。DSH 的原生文件、工具过滤与网络边界已在 Docker 中验证，不能把它解释为 DSH 真实模型闭环通过。
- 停止结果未知、队列门禁、后继原子提交、重启重放和后续改绑不改变待恢复意图已覆盖；没有在每一条机器指令边界做进程 kill 注入。更多崩溃、并行压力和不同宿主平台仍可继续补验。
- 改绑不即时撤销运行中或已排队的原生调用；它们保留冻结授权。已有工程锁与固定 Agent 不追随；要改变旧 Agent 的行为，需要在 Agent 编辑中明确改为追随或选择新的固定绑定。
- 跨全局库和项目库的混合批次不提供分布式原子性，接口拒绝该批次；单个同库多目标变更事务化。
