# R0 固定接入基线

日期：2026-11-08。依据 [实施计划](PLAN.md)。本报告是作者自检；工程内的原生 Reviewer 独立评审各自的交付，不构成本实现代码的第三方评审。

## 固定环境和产物

| 项目 | 实际版本 / 标识 |
|---|---|
| 起始提交 | `01c0708f0c0c8c985a620477b9dcbffb49954a36` |
| 宿主 | macOS / aarch64，Node 24.13.0，Rust 1.99.1 |
| Codex | CLI / Linux npm 0.167.0，许可证 Apache-2.0 |
| DSH 参考源码 | 本机 `5aeed699a011563980a9597f9fc8bc3ee2889066` |
| DSH Linux 分发 | 官方 npm `@deepseek-ai/dsh@0.2.2-rc.1`，MIT；不以版本号相同推定与本机源码构建相同 |
| DSH npm integrity | `sha512-ijR9UEVYkG15Q/Q79JzvzLBzQOQfc63VLr9n6flpwHPhr5IKn2UxthnSoto/P9l7qGd7pPvI1Ra6TW7LH3oKpg==` |
| Docker | Engine 29.9.2；经用户授权将 Desktop 磁盘上限从 8 GiB 扩至 16 GiB，未删除用户容器、镜像或卷 |
| 共享基础镜像 | `node:24.13.0-bookworm-slim@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f` |
| Codex Linux 镜像 | `sha256:6d8a4432dd04977728fd94f8b77ffd6b3ee4d9e9f90128ba96250691d31246e1` |
| DSH Linux 镜像 | `sha256:0ceade127a6a2f5f420e493759f0baeefe2daf77e841f2bf59ea7efe9611e00e` |
| 实际模型 | Codex 默认模型由 `model/list.isDefault` 解析为 `gpt-6.1-sol`；DSH `deepseek-official` / `deepseek-v4-pro` |

旧报告中的其他镜像摘要属于较早探针或失败构建，保留其原始记录；当前受控 worker 使用上表固定镜像和包含权限包装的适配器构建身份。

## 实际能力与证据

- 原生握手：[R0-protocols.json](R0-protocols.json)。握手与真实模型调用分别记录。
- 两套 Linux Harness 的真实两轮与结构化输出：[Codex](R0-codex-turns.json)、[DSH](R0-dsh-turns.json)。随机 marker 跨轮保留；不是只观察 enqueue/idle。
- 销毁并重建 worker 后原生续用第三轮：[Codex](R0-codex-resume.json)、[DSH](R0-dsh-resume.json)。各自独立状态目录、不同 worker 生命周期、相同原生 Session。
- 完整 HTTP → Rust → 容器 → 持久化会话：[Codex](R3-codex-application.json)、[DSH](R3-dsh-application.json)。每轮绑定输入、消费证明、终态、writer revoked 和脱敏协议 Artifact；同 client message ID 重放返回同 Turn。
- 实际受控 shell 工具：[Codex](R3-codex-tools.json)、[DSH](R3-dsh-tools.json)。Docker 实测覆盖指定目录写入、越界拒绝、隐藏私有状态、私有/空 proc、归属核对、重复创建/停止/清理。
- 专用私有仓库 [PR #1](https://github.com/xfork24/devdesk-integration-test/pull/1) 的间接 merged、单父候选、CAS 与精确 lease 拒绝旧值：[R0-github.json](R0-github.json)。未调用平台 merge、修改规则或管理员绕过。
- 实际策略检查：专用仓库兼容；对 `openai/codex` 只读诊断识别活动规则并阻塞。见 [R4-repository.json](R4-repository.json)。

## 协议和隔离约定

Codex 使用固定版本生成的 JSON Schema、initialize/initialized、thread/start/resume、turn/start、对应 thread/turn/item 事件与 request 回调。细粒度权限需要 experimentalApi 能力。讨论和工程阶段使用独立的角色输入与冻结配置；代码工具需要受控 code-mode host。停止区分原生 interrupt ACK 与容器最终停止，不能把提交 interrupt 当作撤销写权限。

DSH 使用公开 sdk profile：initialize 选择 provider/model/cwd；session/prompt 返回 durable message ID，仅表示入队。必须观察同一 root Session 的 inbox/spliced 实际消费、随后 turn/end，以及 session/wait 回执的静止边界。恢复使用 session/resume；session/query 是输入派发，不当作历史查询。SDK 不提供 per-session cancel/close，本轮停止专属容器并创建显式后继 Session，未知输入不自动重发。

容器非 root、rootfs 只读、cap-drop ALL、no-new-privileges，具备 CPU/内存/PID 限额；无宿主 HOME、Docker socket 或其他 worktree。嵌套沙箱使用从 Moby default 派生的固定 seccomp，保留默认拒绝，只开放必要 namespace/mount 调用。DSH 的原生 proc 挂载与 Docker masked paths 不兼容，包装为私有 PID 空间中的空 `/proc`，保留外层安全配置；不使用特权容器或 unconfined fallback。

## 失败与修复记录

- slim 镜像缺 CA 导致 Codex 账户路由失败，补齐系统 CA 后真实两轮通过；保留 [首次失败](R0-codex-turns-first-failure.json)。
- DSH native addon 默认复制至 noexec tmp，改用供应商支持的 `NARB_DISABLE_NATIVE_CACHE=1` 直接加载镜像内产物，保留 tmp noexec。
- OAuth 登记卡顿通过 debug SHA 优化、绝对 CODEX_HOME、超时与同步打开浏览器窗口解决；用户已完成独立网页登录。
- unsigned 开发构建的 Keychain 读取可能等待系统授权；状态查询改为仅查元数据，密钥读取通过有超时的系统工具。用户已授权，密钥未进入报告。
- 首次正式写工具测试发现 code-mode host 未启用及 DSH proc 挂载失败；禁止越权的策略确实拒绝了重试。保留 [R6-tool-boundary-failure.json](R6-tool-boundary-failure.json)，修复后采用新 Attempt，双 Task 的实际 Red/Green/Review 见 [R6-implementation.json](R6-implementation.json)。

整轮完成状态以最终 A01–A22 报告为准，不能仅凭本阶段兼容路径宣称 R8 已通过。接入与当前限制见 [运行说明](../../real-engineering.md)。
