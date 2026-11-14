# DevDesk 0.2 原型阶段历史验收

> 归档说明：本文件自顶层 `docs/acceptance.md` 迁入（2026-11-12 按计划目录归档拆分）。本文保留原型阶段的历史验收事实，不代表当前支持模拟执行。当前产品仅使用原生供应商，验证入口见 [测试索引](../../../tests/README.md)；当前新能力的 Ubuntu x86_64 验收见 [Permission 补验](../permission-standardization/UBUNTU.md) 和 [Workflow/工作台补验](../workflow-and-workbench/UBUNTU.md)。

日期：2026-11-08。执行模式：simulation。原 Linux 验收为 Ubuntu 24.04.5 LTS x86_64；本轮补充 macOS 26.7.2（25G332）Apple Silicon arm64 实机验收。Linux 结果沿用原交付证据，本轮未在 Linux 重新运行。

## 验证结果

| 检查 | 原 Linux 结果 | 本轮 macOS 结果 |
|---|---|---|
| `cargo test --workspace --locked` | 32 项通过 | 32 项通过（29 集成/契约/调度/存储，3 领域） |
| TypeScript / Vite 构建 | 通过 | 通过 |
| debug host HTTP | 通过 | 通过 |
| release host HTTP | 空工作目录运行通过 | 复制二进制到临时目录运行通过 |
| Chromium Web E2E | 3 项通过，24.2 秒 | debug 3 项通过，31.4 秒；release 3 项通过，23.0 秒 |
| release 构建 | 通过 | `aarch64-apple-darwin` 本机构建通过 |
| 独立评审 | 原阶段评审通过 | 独立 release HTTP 与 Web E2E（3 项，22.0 秒）通过 |

本轮完整检查入口为 `./scripts/check/check.sh`，先构建 Web，再测试和构建 host，干净检出无需预先生成 `web/dist`。新增 HTTP 检查覆盖会话鉴权、Host/Origin、JS/CSS 实际读取、平台能力、中文及空格 data 路径、跨进程单写锁、幂等、Artifact 未知 ID/未鉴权访问、SSE 连接下退出与重启持久化。真实 Artifact 内容由 Web E2E 打开并断言来源标记，文件哈希与 symlink 边界由 Rust 测试验证。

浏览器使用真实 Rust API、SQLite 和本地文件，没有假后端。主流程与异常回路均断言无 pageerror。Web 截图：原 Linux（历史截图已移除）、本轮 macOS（历史截图已移除）。作者验证与独立运行的范围分别保存在 [macOS 执行报告](macos-validation.md) 和 [独立评审](macos-independent-review.md)。

## 原计划验收矩阵

| 项目 | 证据 | 结果 |
|---|---|---|
| A1 需求到 mainline | a1_full_loop；Web 主闭环含人工需求、双 Task、收尾、Merge、报告/历史 | Linux / macOS 通过 |
| A2 串并行/额度/写许可 | a2_parallel_runs、logical_write_permit；真实后端同时 running 记录 | 两平台 Rust 测试通过 |
| A3 技术失败重试 | a3_technical_retry；Web 异常回路的 Task 与 Merge 重试 | 两平台通过 |
| A4 退回新 Attempt | a4_rejection；Web 退回后新 Attempt，旧版本保留 | 两平台通过 |
| A5 输入失效 | a5_input_change；inflight 失效不阻塞调度器 | 两平台 Rust 测试通过 |
| A6 目标变化重验 | a6_target_change；旧候选阻塞，新候选重新验证/Review | 两平台 Rust 测试通过 |
| A7 规范升级/替换/继承 | a7_spec_upgrade、alternative_spec；旧锁不回写，Task 独立绑定 | 两平台 Rust 测试通过 |
| A8 暂停/取消/Override | pause/cancel、a8_quality_override、取消不可复活与 Completed 冻结 | 两平台 Rust 测试通过 |
| A9 刷新/重启/幂等 | a9_restart、merge_command_replay、HTTP 重启、Web 刷新 | 两平台通过 |
| A10 Context/Snapshot | a10_context；冻结 Attempt 上下文、Snapshot JSON、Web 比较入口 | 两平台 Rust 测试通过 |
| A11 模拟边界 | a11_hard_tdd、simulated_never_live；UI/报告来源标记 | 两平台通过 |
| A12 Linux/macOS 实机 | 两平台后端与完整 Web 主流程均实际运行 | 通过 |

浏览器自动化覆盖三个代表性场景，不声称每个后端异常都已逐一做浏览器自动化。全部异常命令均有 Web 入口，后端规则由上述集成测试覆盖。F0—F5 的本期范围与双平台实机待办已完成；原评语与本轮补充结论分别保留。

## 可重复小样本

同一规模：10 Projects / 100 Features / 1,000 Tasks。分页列表、详情和事件三个查询：原 Linux debug 单次约 **28.82ms**；本轮 macOS debug 单次 **41.396083ms**。均非性能 SLA。

原 Linux release 可执行文件 **6,604,784 bytes**（约 6.30MiB）；空目录首个 HTML 约 **6.17ms**。本轮 macOS release 为 **5,740,880 bytes**（约 5.48MiB）；空目录启动到首个 HTML 五次为 **20.996 / 6.128 / 5.360 / 5.558 / 5.647ms**，中位数 **5.647ms**。启动测量不含数据初始化和浏览器启动。Web JS 266.63kB（gzip83.35kB），CSS14.64kB（gzip4.09kB）。macOS 测量数据（历史路径：`plans/devdesk-prototype/macos-measurements.json`，文件已随历史证据清理）含二进制 SHA-256。

## 运行包

`./scripts/release/package.sh` 从锁定依赖构建，生成本机目标的 `.tar.gz` 和 `.sha256`。本轮 macOS 交付为 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`，含独立可执行程序、运行说明、演示及验收文档。构建后运行无需 Node/Rust 或源码；`otool -L` 只列出系统 libiconv 和 libSystem。详情见 [运行包说明](../../run-binary.md)。

## 已知限制与下一轮边界

1. macOS 实测基线为 Apple Silicon、macOS 26.7.2；未验证 Intel Mac 或更旧系统，不提供签名、公证的 `.app` 安装器。
2. Git/Test/Harness/Sandbox 只有模拟实现；没有真实代码合入、模型推理或被管理项目的测试。live 模式未开放。
3. Archive、迁移只预览；不执行导出、清理、冷数据恢复或搬迁。重启标记中断并显式重试，不透明重建真实环境。
4. 工程存储 port 当前每次工作单元加载类型化状态；查询输出分页，表有外键与索引。已覆盖小样本，未承担大量 Run 历史/百万级压测。后续可在同一 port 内改增量查询。
5. Spec 仅解释内置有限规则；模拟建议为确定性模板，可编辑，不是智能生成。Review 结果与 Override 保留原事实。
6. 当前 Delivery 列表的 Review 展示有界；多次不确定决定的大量历史可通过分页事件与报告读取，尚未提供独立全文检索。协议不支持 JSON-RPC batch/notification。

按 v0.2 停止线交付并等待实际使用反馈，真实 Git/Test/Codex 和数据生命周期继续留待后续讨论。

## 领域补强后续验收

本文件上方保留原 v0.2 平台交付记录。领域补强的最终实现、v1→v13 数据迁移、操作核对及本轮 macOS 验证见 [补强验收报告](../domain-hardening/domain-hardening.md)；不能将上方历史 Linux 验证视作补强代码的 Linux 复验。

## Ubuntu Web 真实工程复验（2026-11-08）

上方原型记录保留历史。本轮 Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过：完整离线基线、Linux 系统凭据、双 Harness Web 工程闭环、真实 PR #6/#8/#9 merged、安装复用、取消/跨项目崩溃恢复和独立 Linux 发布包。Docker 嵌套沙箱使用用户明确批准的专用 AppArmor 策略；首次失败及显式续跑历史保留。full、verify-existing、完整安装探针均退出 0，证据与实际/契约边界见 [Ubuntu 完成报告](../ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。
