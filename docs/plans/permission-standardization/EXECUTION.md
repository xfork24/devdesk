# Permission 标准化执行报告

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

后续平台补验：2026-11-09 已在 Ubuntu 24.04.5 x86_64 完成本机回归、AppArmor、双供应商原生权限及 Linux 发布包验证，见 [Ubuntu 补验报告](UBUNTU.md)。下文保留首轮 macOS 的实际执行范围和历史记录。

日期：2026-11-08–09。源码基线：`44fb59296866b86c41ad5961b05398ba8e23e97d`。状态：P0–P5 已实施，本轮 P6 macOS 验收全部通过。用户指定复用 `data/real-integration`、本轮只验证 macOS；Ubuntu 实机项顺延，未记为通过。工作区同时出现的 GitHub 项目接入、自动准备运行时与新版 Web 改动已按用户要求保留并整合。

## 实施结果

- `Permission` 是独立、不可变修订的领域实体；Harness/Sandbox 分别绑定。支持完整策略、精确修订引用、字段 Patch、项目/角色/本次执行覆盖、来源、执行范围、审批、隔离与资源额度。规则和支持范围见 [CONTRACT.md](CONTRACT.md)。
- 新建配置默认容器内全权限、工具网络开启、无需运行时审批。旧数据迁移保留 legacy 默认；修改配置不会重写已有 Session/Run 的运行锁。全局 v4、项目 v32/v33 顺序迁移，项目复制引用的共享修订，RuntimeExecution 保存冻结权限。
- Codex 0.167.0 使用原生 externalSandbox 或命名 permissions profile；DSH 0.2.2-rc.1 使用原生 permission preset/sandbox-policy/approval。工具过滤扩展读取同一冻结策略，默认全权限不加载旧白名单或禁网 runner。
- `permissions/` 统一六个预设、四个后端和七个摘要登记文件。seccomp、AppArmor、Moby 基线及许可证保留原字节；DSH runner 保留原字节，工具扩展改为消费标准策略。镜像构建、嵌入身份、预检、安装器、探针和运行包已更新。
- Docker 根据冻结计划生成网络、只读/可写层、可执行临时空间和资源限制。正式代码写范围、只读 Review、测试保护和隐藏 Git/配置元数据仍由 ExecutionScope 与挂载落实。独立测试使用无凭据、无网络、只读代码导出，并记录 node-test Harness 和权限快照。
- Web 提供预设、不可变修订、Harness/Sandbox 绑定、字段覆盖、项目/角色覆盖、预检及冻结会话展示；后端使用 command_id 幂等和配置 CAS，能力不足在派发前拒绝。

## 验证证据

| 验证 | 当前结果与证据 |
|---|---|
| 原始完整基线 | 通过：baseline/check.log（历史路径：`baseline/check.log`），含 Rust、debug/release HTTP 和各 6 项 Chromium |
| 固定版本、原资产摘要 | baseline/context.json（历史路径：`baseline/context.json`）、Codex 实际协议和 DSH 实际 SDK profile；未读取凭据内容 |
| 完整回归 | final-check.log（历史路径：`final-check.log`） 通过 Rust、自动运行时准备、HTTP 及新版 Web debug/release 各 6 项 Chromium；后续 Rust 改动再由 regression-rust.log（历史路径：`regression-rust.log`） 与权限专项验证 |
| 权限专项 | docker-permissions.log（历史路径：`docker-permissions.log`）：8 项通过，含三项真实 Docker/原生执行器测试；覆盖 CAS/幂等、不支持配置、不可变存储、新旧默认及 legacy Codex 读取边界 |
| 最新包内版本真实工具 | macos/permission-native-tools.json（历史路径：`macos/permission-native-tools.json`）：4 个实际工具轮次全部通过；Codex、DSH 全权限均写入 `/tmp` 并命中受控 HTTP，只读＋工具禁网轮次均实际拒绝写入/网络且模型通信成功。结果来自工具输出及 HTTP 收据，不依赖模型自述；二进制 SHA 与包内文件一致 |
| 受限真实执行 | Codex 编译 profile 允许指定目录写入、拒绝其他 worktree 写入；DSH SDK 实际 shell 允许授权写入、拒绝测试目录写入，隐藏假状态凭据与假环境变量，且只见内部网络接口。见权限专项日志 |
| 假凭据边界 | macos/nested-sandbox.json（历史路径：`macos/nested-sandbox.json`）：Codex 原生沙箱写入成功且假 `/state` 凭据读取被拒绝；全权限容器测试则明确证明假状态可见，不宣称同容器凭据隔离 |
| Schema 与最终数据 | schema-validation.log（历史路径：`schema-validation.log`）、schema-validation-extra.log（历史路径：`schema-validation-extra.log`）、live-schema-validation.log（历史路径：`live-schema-validation.log`）：jsonschema 4.26.0 校验预设、后端 manifest、RPC Schema 及全部 41 个 RuntimeBinding、68 个执行权限快照，包含独立测试 |
| Web 实际界面 | 权限设置截图（历史路径：`permission-settings-release.png`），Chromium 覆盖保存、重载、创建修订和能力不足提示 |
| 最新 release | standalone-http.log（历史路径：`standalone-http.log`）、standalone-preparation.log（历史路径：`standalone-preparation.log`）、final-release-web.log（历史路径：`final-release-web.log`）：源码外 HTTP、自动准备与 6 项 Chromium 通过 |
| 真实测试身份与安装复用 | macos/runtime-permission-proof.json（历史路径：`macos/runtime-permission-proof.json`）、macos/installation-reuse.json（历史路径：`macos/installation-reuse.json`）：实际测试请求使用 node-test/禁网绑定，多个角色与项目复用固定 installation ID |
| 完整真实闭环 | full 总报告（历史路径：`macos/full/live-check-full-git-docker-harness-github-web-workflow-recovery.json`）：19 个步骤通过，涵盖双供应商会话、浏览器采纳、仓库、正式规划/实施/Review、PR、生命周期与重启；独立复核（历史路径：`macos/full/live-check-verify-existing-git-docker-harness-github-web-workflow-recovery.json`） 同样 `passed: true` |
| 最终封存与清理 | macos/final-validation.json（历史路径：`macos/final-validation.json`）：285 个已提交 Artifact 摘要一致；67 个执行已停止并封存协议。另 1 个创建 worker 前失败的执行已显式取消，保留协议缺失及未消费事实，不伪造日志。专用验收 Host 已正常停止 |
| 发布包 | package.log（历史路径：`package.log`）、源码外包验收（历史路径：`macos/package-validation.json`）：归档及二进制摘要已记录；7 个资产、6 个预设、独立权限 API、修订不可变、绑定幂等、项目 schema v33 全部通过，无模型调用或自动下载。源代码快照入口及输出见 source-package.log（历史路径：`source-package.log`） |

真实验收只写入专用仓库：[加法 Task PR #10](https://github.com/xfork24/devdesk-integration-test/pull/10)、[乘法 Task PR #12](https://github.com/xfork24/devdesk-integration-test/pull/12)、[Feature mainline PR #13](https://github.com/xfork24/devdesk-integration-test/pull/13)。三者均核对远端合并状态与精确候选 SHA；旧候选因目标变化被拒绝。最终浏览器采纳截图见 R7-browser-final.png（历史路径：`macos/full/R7-browser-final.png`）。

## 失败、修正与保留证据

1. 初次 Codex 全权限探针仍被旧角色 `tool_policy: read_only` 提示阻止。已从标准化运行提示中移除旧权限标签，改为区分冻结工具权限与正式代码视图；随后双供应商实际写入和网络探针通过。初次报告保存在 `macos/permission-native-tools-before-*.json`。
2. 默认临时区最初只移除 `noexec`，真实容器仍不可执行。已显式使用 `exec`，真实可执行文件测试通过。
3. 用户恢复专用数据集的 Codex 登录后，原失败完整验收通过 `--resume` 继续；原认证失败报告保存在 `macos/full/history/`，不把它算成通过。DSH 凭据读取也已由实际会话确认。
4. Docker Desktop 一度无法直接挂载新建深层目录；用无凭据诊断确认并重新核对路径后恢复。该失败另暴露“准备失败、尚无协议进程”被当成日志读取错误的问题：现在保留日志缺失事实并允许停止/封存，不伪造模型输出，也不重发未知输入。对应回归和失败报告保留。
5. 新自动准备逻辑会保留多份不可变安装。旧规划探针错误要求目录中只有一份 Codex 镜像；已改取设计 Session 的冻结安装，保留已采纳提案继续验收。原失败日志与报告保存在 `macos/full/history/`。
6. 专项并发运行曾一次在立即重开临时数据集时触发 `dataset_locked`；原日志保存于 `macos/validation-history/`。真实资源专项改为串行执行，8 项通过；没有删除数据或绕过锁。
7. DSH 实施回答在 JSON 前添加 prose，被严格阶段契约拒绝。原 Task 唯一的未封存模块先保存在路径限定 Git stash 和本目录历史文件中，再以 `run.retry` 发起新 Run；新轮次通过实际 Green/verify，原失败不改写。读取持久化记录确认重试完全保留原运行时与 Permission 绑定，见 技术重试证据（历史路径：`macos/full/permission-technical-retry.json`）。
8. DSH 对同一实施交付的两次 Review 也因 prose 前缀被拒绝；重新发起独立 Codex Review 后正式通过。后续真实合并候选的 DSH Review 已直接通过。没有放宽 JSON 解析、改动接受条件或人工伪造 Review。
9. 发布构建与 macOS 重签名改变了二进制摘要。最终以与包内文件 SHA 完全一致的二进制重新跑完 4 个模型工具探针和 6 项 Chromium；前次探针结果保留，未把不同文件摘要混为同一验收对象。

## 边界与未支持能力

网络 allowlist 当前没有强制代理执行器，返回 `permission_unsupported`。Docker 不理解 Harness 工具名；Harness 容器整体禁网会阻断模型连接，配置返回冲突。DSH 工具禁网要求受限文件模式和 `bash`/`ask_user_question` 工具范围；其原生 workspace-write 不能缩小会话 cwd，不能由 ExecutionScope 挂载落实的 scoped 规则拒绝。

FullAccess 的信任范围是整个容器，同容器工具可以访问该执行的状态、环境和模型凭据。宿主 HOME、系统凭据库、Docker socket 和其他项目不挂载。独立测试和受限 canary 结果不能推导为全权限容器中的凭据不可读。

实施前已用 SQLite backup 备份原全局库和 14 个项目库至本地 `data/permission-standardization-backup-20261108/`，不包含凭据目录。数据集、构建产物和密钥不进入 Git。本轮只操作专用验收 Host/容器与 `xfork24/devdesk-integration-test`；本仓库工作区按用户最终指示在验收通过后以英文 commit message 提交并推送。
