# Agent 完整能力与经验访问实施验收

日期：2026-11-15。分支：`codex/agent-capabilities-discussion-experience`。基础提交：`804a51a`。状态：实施与本轮验收完成。

## 实施范围

- 独立 worktree：`/Users/hengliu/workspace/devdesk-agent-context`。原工作区及用户数据未用于实施写入；真实运行使用专用临时数据集。
- Capability 增加 Agent 自身 `spec_refs`，进入确切引用、组合、冻结、原生指令和编辑界面；缺省不补推历史规范。
- 自由讨论、步骤及 Controller 装配保留完整业务工具和 Skills。步骤授权及 Controller 策略只约束实际调用。
- 自由讨论允许缺少项目、工作流、材料和外部账户；未配置或停用的接入保留工具并保存不可用原因，调用回传明确失败。
- 所有 Agent 通过冻结的 `devdesk-agent-context/v1` 使用 `agent.experience.list/read`，按当前实例检索并分块读取现有快照、文档和原始证据。
- 项目实例可在全局讨论命名空间继续使用原身份及私有配置闭包；历史来源和当前活动项目分别处理。
- UI 支持完整能力、Agent Specs、来源项目、工作流筛选、当前讨论引用和实际经验读取记录。
- 平台只读资料和规范检查不再统一依赖运行中的 Workflow Step；控制与写入仍由已有实际授权和持久化执行机制处理。
- Codex 及 DSH 动态工具接入使用同一 Host 调用链，保存意图、结果、错误及交付状态。DSH 新增真实 SDK 双向工具桥。
- 修复创建/发送装配差异及权限后继的接入锁比较；后继消息幂等性同时核对正文和经验引用。
- 以新修订发布 21 个 Capability、18 个 Agent 和工程上下文 Skill；未改写数据库旧修订，也未增加 SQLite 迁移。

## 已完成的真实验证

### Codex

通过真实 Host → Docker → Codex 模型 → 工具回执 → SQLite → 重启核对完整链路。

- Agent：`builtin-agent-sol-high-analyst:6`；实例：`agent-instance.f32486e9-a3a5-4a03-adc1-e4dd92cc8436:1`。
- 原无项目讨论：`b62e08ee-a3b5-45a7-a10c-1c4ff05c5ad6`；来源项目：`5bddd75f-1796-4b61-9b6c-ca8fa069065a`。
- 实际历史正文读取 8 次。Agent 返回来源讨论中随机生成的记号，未切换或恢复来源会话。
- 第一轮确认数据集没有 Project，实际列表调用成功；后续在原 Session 读取项目经验，活动项目保持不变；重启保留同一实例、协议和读取记录。
- 验收二进制 SHA-256：`d2608c21719d1017f2e187455734c51e50a0dcd687e1a92bd1e84b75f889b629`。
- 原始证据：`/private/tmp/devdesk-agent-context-evidence/native-codex-02/`。

### DSH SDK 协议验证

- 实际固定 DSH 镜像、官方 SDK、真实工具注册器和 JSON-RPC；推理为本机回环夹具，容器使用 `--network=none`。
- 两次真实上下文回调、三次模拟推理成功，读取历史内容后继续产生最终回答。该结果独立于供应商真实推理验收。
- 原始证据：`/private/tmp/devdesk-agent-context-evidence/dsh-bridge.log`；可重复入口为 `docker_dsh_agent_context_bridge_reads_detached_history_with_real_sdk` ignored test。

### MiniMax / DSH 真实推理

- 用户指定的 MiniMax，通过系统凭据库配置到专用测试数据集，未写入仓库。
- Agent：`builtin-agent-minimax-analyst:2`；实例：`agent-instance.8e46bfc6-c236-445e-a885-39011cf59750:1`。
- 原无项目讨论：`efd02fff-9a9a-4b0b-960a-1c6924ebe9d0`；来源项目：`aee35f4e-7c18-4399-aca5-e3452c25c36a`。
- 实际历史正文读取 2 次，返回来源讨论的随机记号；原会话、活动项目及重启后记录核对均通过。
- 验收二进制 SHA-256：`332893377accd97d9b60440144eac2974f08c6da9a95edc39806372776d50817`。
- 原始证据：`/private/tmp/devdesk-agent-context-evidence/native-minimax-01/`；执行日志 `native-minimax-04.log`。

## 最终验证记录

| 检查 | 结果 | 临时证据（统一根目录见下） |
|---|---|---|
| `cargo test --workspace --locked` | 300 passed，18 ignored，无失败；ignored 涉及显式 Docker/供应商环境 | `workspace-final.log` |
| 新增历史产物与旧经验来源回归 | 两项新增测试分别通过；最终 Agent 能力专项 4 项、会话 8 项、经验治理 5 项通过 | `history-outputs-head.log`、`provenance-head.log`、`conversation-head.log`、`catalog-head.log` |
| Rust/TS/样式门禁 | rustfmt、ESLint、TypeScript、Prettier、Web build、clippy `-D warnings` 通过；最后源码补充再次通过 clippy | `style-head.log`、`clippy-delivery.log` |
| 权限资产、独立 Permission Schema、出厂闭包与架构 | 全部通过；bundle `2026.11.15-agent-context-v1`；SQL 基线未变 | 命令输出及 `ci-gate.log` |
| HTTP / SSE / 退出 / 认证 | debug 与 release 通过；debug HTTP/SIGINT 退出约 1.140 / 0.902 秒 | `http-debug-optimized.log`、`http-release.log` |
| 模板重置、数据集兼容、运行准备 | debug/release 全通过；不兼容数据原样保留，空目录初始化与重启正常 | `template-*.log`、`dataset-*.log`、`preparation-*.log` |
| Chromium 全量 | debug 63 passed；release 63 passed | `browser-debug-clean.log`、`browser-release.log` |
| 最终界面与目录检查 | 布局调整后的 release 专项 3 passed，最终产物再跑无项目/项目来源实例 2 passed；截图已检查 | `browser-release-head.log`、`browser-delivery.log` 及对应 artifacts 目录 |
| DSH 官方 SDK 工具桥 | 显式 immutable image 的 ignored test 单独执行通过；真实 SDK，回环模拟推理，禁外网 | `dsh-bridge-head.log` |
| Codex 真实推理 | 无项目会话、实际 list/read、来源记号、原会话及重启保留通过 | `native-codex-02.log` |
| MiniMax / DSH 真实推理 | 同一完整场景通过，使用用户指定服务商凭据 | `native-minimax-04.log` |

上述日志根目录为 `/private/tmp/devdesk-agent-context-evidence/`，日志和截图未提交。临时 OAuth/环境凭据副本已清理；源码及报告不包含密钥。

300 项全量之后新增的两项测试是历史产物只读访问和旧经验计划来源重建；分别执行并通过。最终源码的相关专项、clippy 和 debug/release 构建通过，没有将未重新执行的整套 Rust 结果改写成另一个测试数量。

### 验收矩阵对应

| 场景 | 本轮依据 |
|---|---|
| A01–A04：无项目、完整能力、原实例和跨项目上下文 | 双供应商真实会话；无项目及来源项目实例浏览器用例；`governance` 跨项目只读查询及私有闭包导入回归。三条入口的场景过滤均已移除，业务配置保留确切引用。 |
| A05：实例隔离 | 实例参数伪造被输入合同拒绝；其他实例读取快照或历史工作流产物被实际 Reader 拒绝。 |
| A06–A08：平台工具、缺少对象、账户和权限 | 无工作流的 `spec.lint` 真正返回检查结果；历史产物 Reader 返回原封存值及原计划，读取前后状态摘要相同；缺少实例目标、跨 Scope 规范和缺失/停用接入回传失败，讨论继续运行。既有权限及 Controller 回归保留。 |
| A09–A12：历史、异常、分页、UI/Agent 一致性 | 完成态工作流产物读取；离线来源、篡改证据及索引验证；UI/Agent 内容一致与稳定分块摘要；讨论引用保存及客户端消息重复输入/引用核对。 |
| A13–A16：原错误、后继、恢复及任务授权 | 新会话创建/发送完整锁一致，真实项目首条消息通过；权限后继、未知执行、重启和原始任务字节合同回归通过；步骤授权仍作用于调用。用户原 `devdesk-project-2` 数据未修改。 |
| A17：供应商分别验证 | Codex 与 MiniMax/DSH 均实际推理并调用工具；SDK 模拟协议验证单独记录。 |

### 验证发现与处理

- 原始 `permission_conflict` 来自创建时清空平台工具、发送时冻结完整工具；后继校验又不接纳真正的接入锁变更。现统一装配，并在保持 Agent/运行身份的前提下明确允许接入锁及当前上下文协议采用。
- 真实 Codex 揭示无项目会话结束后日志查找仍只扫描项目目录；已增加全局讨论查找及缺少项目目录的回归。
- 真实 DSH 揭示准备探针用了服务商显示 ID、全权限未加载桥、工具归属校验错用 Codex turn ID；现分别采用源声明的原生路由、统一加载插件，以及 DSH 原生 message ID。
- Web Capability 保存遗漏根级 `spec_refs`，已修复公共信封转换；保存不再按当前工具目录裁剪已有引用。
- 新增空轮询的额外完整快照加载曾导致 debug 退出超过四秒；已复用既有快照，仅在存在待交付调用时进入上下文驱动。未放宽退出时限，最终 HTTP 检查通过。
- 经验快照对照完整封存内容核验，证据只来自快照实际引用；旧来源按确切执行记录重建，冲突时保持未知。同一源引用的跨库内容冲突明确拒绝。
- 早期 debug 浏览器运行有两项页面等待超时，原日志保留在 `browser-debug-final.log`。重新构建后的独立完整运行 63 项通过，未删除或放宽相应断言。

## 运行边界

- 旧业务 Agent 修订继续固定其原 Skills、Tools 和规范声明；通用经验协议按运行绑定采用。真正改变适配器或运行环境的旧会话，仍需使用显式后继入口。
- 工作流控制、项目经理写入、Git 和人类决定需要对应目标和授权。自由讨论不冒用 Controller 或 Human 身份。
- GLM 未进行真实供应商推理；DSH 实际推理使用用户指定的 MiniMax。
- 未合并或部署到 main；工作区和运行证据保留在上述独立位置。
