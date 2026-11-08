# macOS 补充独立评审

日期：2026-11-08。评审者：独立 Agent `review_macos`，未参与本轮业务代码或构建脚本实现。范围：计划 v0.2 中 F1/F5 的 macOS 实机待办、验证入口和本地运行交付。

## 结论

本轮评审发现的阻塞已关闭，macOS Apple Silicon 的 F1/F5 补充验收通过。该结论结合原有 Linux 验收，补齐 A12 双平台实际运行要求；没有把编译成功或作者自检单独当作独立验收。

本次实际环境为 macOS 26.7.2（25G332）、arm64。结论限于本轮验证环境和 simulation 原型，不扩展到未验证的 macOS 版本、Intel Mac、签名公证安装器或真实外部适配器。

## 评审者独立执行

| 检查 | 实际结果 |
| --- | --- |
| `python3 tests/http_contract.py target/release/devdesk-host --standalone` | 通过。复制 release 程序到临时目录，验证 HTML/JS/CSS、会话鉴权、Host/Origin、能力信息、中文及空格 data 路径、跨进程写锁、命令幂等、SSE 连接期间退出和重启持久化 |
| `cd web && DEVDESK_E2E_PROFILE=release npx playwright test` | **3 passed (22.0s)**，真实 Rust 后端与 Chromium；完整主闭环 8.9s、异常回路 9.1s、详情分页及 revision 冲突 3.1s |
| `file target/release/devdesk-host` | Mach-O 64-bit executable arm64 |
| `otool -L target/release/devdesk-host` | 仅系统 `libiconv.2.dylib` 和 `libSystem.B.dylib`，未依赖 Node 或外部 SQLite 动态库 |
| `bash -n scripts/build.sh scripts/check.sh scripts/package.sh` | 通过 |

Web 主闭环实际完成需求输入、规划两个 Delivery/Review、双 Task、Task Merge、Feature 收尾、mainline Merge、刷新后读取、产物打开与历史查看；异常场景实际验证技术重试、Review 退回后新 Attempt、Agent Review、Merge 重试和手动继续。主闭环与异常用例均包含浏览器 `pageerror` 检查。

HTTP 契约中的 Artifact 检查覆盖未鉴权和不存在 ID；实际已保存产物的打开由 Web 主闭环覆盖。评审没有将前者描述为完整 Artifact 内容读取验证。

## 发现与关闭

1. 原 `check.sh` 先运行 workspace Rust 测试，再构建 Web；干净检出时 RustEmbed 所需 `web/dist` 尚不存在。已调整为先构建 Web，再运行 Rust 验证。
2. 原验证仅使用 debug 后端。本轮检查脚本保留 debug 流程，并增加 release HTTP 独立复制运行及 release Web E2E；Playwright 的 profile 仅允许 `debug` 或 `release`，不复用已有服务。
3. 新打包脚本调用的子脚本加载 Cargo 环境后，不会改变父进程的 PATH。已在 `package.sh` 父进程加载 Cargo 环境，保证后续 `rustc -vV` 可用。
4. 运行说明复制为包根 README 时，原 Markdown 相对链接指向不存在文件。已改为运行包内实际 `docs/demo.md`、`docs/acceptance.md` 路径。
5. host 能力接口、启动信息、设置页和当前验收文档原有 macOS pending 状态已同步；旧 Linux 独立评语作为历史保留，本轮结论另存。

上述脚本及交付修复已静态复审。最终归档生成与解包运行由实现者执行，具体证据保存于 [macOS 验证报告](macos-validation.md)。

## 作者验证与范围

作者另报告 `./scripts/check.sh` 全部通过：32 项 Rust 测试、debug HTTP、debug Web E2E 3/3（31.4s）、release 构建、release standalone HTTP、release Web E2E 3/3（23.0s）。作者也记录小样本查询、启动时间和包体积；这些不是评审者独立重跑的数据，见 [验证报告](macos-validation.md) 与 [测量数据](macos-measurements.json)。

本轮评审没有重新审计全部既有领域实现，也没有在 Linux 再次运行修改后的脚本；Linux 依据原有独立评审和验收历史。本轮新增修改集中在验证、平台状态和交付，未改变领域状态机。真实 Git/Test/Codex、Archive/迁移执行等仍按计划延期，不列为本轮未完成项。
