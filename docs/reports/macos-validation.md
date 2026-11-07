# macOS 补充交付：设计、执行与验证

日期：2026-11-08。基线：`plans/prototype-v0.2-plan.md` F1/F5、A12 及第 14 节独立评审要求。本轮关闭原 Linux 交付遗留的 macOS Apple Silicon 实机待办，不扩展真实适配器或 Archive/迁移执行。

## 环境与构建

- macOS 26.7.2（25G332），Darwin 25.6.0，Apple Silicon arm64。
- Rust 1.99.1（58cf8f9c3）、Cargo 1.99.1，遵循 `rust-toolchain.toml` 与 `Cargo.lock`。
- Node 24.13.0、npm 11.6.2，遵循 `.nvmrc` 与 `web/package-lock.json`。
- Xcode 工具链 Apple clang 21.0.0（clang-2100.4.1.1）；Python 3.15.0。
- Chromium for Testing 158.0.8127.18，Playwright Chromium revision 1254，mac-arm64。

从无 target、node_modules、dist 的本地仓库执行 `./scripts/build.sh`，安装锁定依赖并成功生成 release。SQLite bundled 随本机编译；`file` 确认 Mach-O 64-bit executable arm64，`otool -L` 仅依赖系统 libiconv 与 libSystem。

## 补齐内容

1. 检查脚本先构建 Web，避免首次 `cargo test` 的 RustEmbed 因 `web/dist` 缺失失败。该问题由独立评审发现并已修复。
2. `check.sh` 对 debug/release 分别运行真实 HTTP 与完整 Web E2E。Playwright 通过 `DEVDESK_E2E_PROFILE` 选择程序，拒绝非法值且不复用已有服务。
3. HTTP 契约新增真实 JS/CSS、平台能力、中文及空格 data 路径、跨进程写锁；release 探针将二进制复制到临时目录再启动、退出和重启。
4. 能力接口将已验收平台标记为 `runtime_verified_by_reports`，新增实际 `runtime.os/arch`；启动信息显示当前平台。Web 设置页与交付文档同步，保留原 Linux 评审原文作为历史。
5. 新增 `scripts/package.sh`，生成本机独立运行包、文档和 SHA-256 文件，附解压运行说明。无需制作额外客户端或真实外部适配器。

## 作者实际验证

`./scripts/check.sh` 退出 0：

| 检查 | 结果 |
|---|---|
| TypeScript / Vite | 通过；JS266.63kB，CSS14.64kB |
| Rust workspace | 32 项通过，0 失败 |
| debug HTTP | 会话、Host/Origin、资源、能力、路径、锁、幂等、SSE退出与重启通过 |
| debug Chromium Web E2E | 3/3 通过，31.4 秒 |
| release locked build | 通过 |
| release standalone HTTP | 复制二进制到临时目录后全部通过 |
| release Chromium Web E2E | 3/3 通过，23.0 秒 |
| `cargo fmt --all -- --check` | 通过 |
| `bash -n scripts/build.sh scripts/check.sh scripts/package.sh` | 通过 |
| `git diff --check` | 通过 |

两组 Web E2E 均包含：需求与规划的正式 Delivery/Review → 双 Task → Task Merge → Feature 收尾 → mainline → 刷新/Artifact/历史；技术失败/重试、Review退回/新Attempt、Merge重试与手动继续；51 Tasks 分页和 revision 冲突保留输入。主闭环与异常回路断言无 pageerror。Web 打开真实 Artifact；HTTP 的 Artifact 测试覆盖未鉴权与未知 ID，Rust 覆盖哈希和 symlink 边界。

小样本通过现有 `small_sample_pagination_and_event_query` 测试的 `--nocapture` 单次运行测量：10 Projects / 100 Features / 1,000 Tasks，列表+详情+事件 **41.396083ms**。release **5,740,880 bytes**，空目录启动到 HTML 五次中位数 **5.647ms**，非 SLA；见 [原始测量](macos-measurements.json)。[实际 Web 截图](../assets/workbench-macos.png) 已检查。

`./scripts/package.sh` 实际生成运行包与校验文件后，核对 SHA-256、解压至含中文及空格的临时目录，确认程序与当前 release 构建一致、说明及演示/评审文档齐全；以空 `PATH` 环境运行解包程序，HTTP/JS/CSS、数据锁、退出与重启验证均通过。归档 **2,542,928 bytes**，详见 [最终归档验证记录](macos-package-validation.json)。该记录在归档生成后保存，不包含在被校验的归档内。启动测量数据的 SHA-256 对应测量时二进制；最终打包再次构建，使用归档验证记录中的哈希核对当前交付。

## 独立评审与交付结论

独立 Agent `review_macos` 另行实际运行 release HTTP 和 Chromium E2E，评审脚本、平台信息与文档；其结论单独保存于 [macOS 独立评审](macos-independent-review.md)。作者自检不冒充独立评审。

结合原 Linux 证据，本期 F0—F5 与 A1—A12 的待办已关闭。只交付 simulation 模式；Intel Mac、旧 macOS、签名安装器、真实 Git/Test/Codex 与数据生命周期不是本轮验收范围。Linux 未在本轮重新运行。
