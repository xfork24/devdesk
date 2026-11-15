# main 补充执行与验收

2026-11-15：main 上的补充实施与本地完整验收已完成。独立判断、原报告中不准确的结论及修复理由见 [ASSESSMENT.md](ASSESSMENT.md)。本文件记录本地验证与首轮远端观察；精确提交的远端 GitHub CI run 链接与结果以最终交付消息为准。

## 本地结果

| 检查 | 结果 |
|---|---|
| 干净前端依赖安装 | 在独立临时目录用最终 package.json/lock 执行 npm ci，通过 |
| rustfmt / clippy workspace all-targets locked | 通过，0 告警；本机 macOS 条件分支已修复 |
| ESLint | 0 errors，75 条已登记 warnings；数量不得增长；新增显式 any 与逗号表达式的注入反例均被拒绝 |
| TypeScript src + tests | 通过；包含 RevisionSelect 合法回调推断与非法单/多选组合的编译反例 |
| Prettier / Web build | 通过；还验证了缺少 web/dist 时先构建再运行 clippy 的路径 |
| 权限、出厂资源、架构检查 | 全部通过，权限 Schema 不再因缺包静默跳过 |
| 权限 Schema 回归 | 2 项通过；非法 source envelope 经相对引用被拒绝；从仓库外 cwd 运行也通过，缺依赖时明确失败 |
| CI 依赖门禁回归 | 4 项通过，涵盖成功、缺失/pending/失败/取消、错 SHA/事件/源仓库、较新失败、过时重跑事件 |
| GitHub Actions 静态检查 | actionlint v1.7.12 通过 |
| cargo test --workspace --locked | **291 passed、0 failed、17 ignored**；17 项是仓库原有的外部资源/专用仓库测试，本次未增加 ignore |
| Role 修订测试的无 Docker 验证 | Docker 命令一经调用就失败的独立 PATH 下通过，记录器确认 0 次 Docker 调用 |
| debug HTTP / connection | 认证、Host/Origin、静态资源、Unicode/空格路径、锁、幂等、Artifact、SSE、退出/重启持久化、项目数据库隔离全部通过 |
| debug 模板 / 数据协议 / 运行准备 | 通过，包括失败重试、未知克隆恢复、并发复用和独立运行配方 |
| debug Chromium | **57 passed、0 failed、0 skipped**（8.8 分钟） |
| release 独立运行 HTTP / connection | 复制二进制到独立目录后通过同样边界检查 |
| release 模板 / 数据协议 / 运行准备 | 全部通过 |
| release Chromium | **57 passed、0 failed、0 skipped**（2.5 分钟） |
| git diff --check | 通过 |

## 原失败项的验收依据

- **A**：慢轮询不重叠且卸载停止；暂停经理采用精确后继源；配置目录 pending/failed 时状态、代次和人工接管仍直接可见。配置修改先展开管理区，运行面不依赖配置读取成功。
- **B**：修改前的失败快照实际已有“计划与执行历史 · 7 条”。修复用例后，保留计划调整、已完成步骤复用、绕过、完成及移动刷新；额外比较 reload 前后的完整 RPC 历史，再核对面板记录可见。没有改动后端历史数据逻辑。
- **C-1**：组合测试使用已有的 ResourceFixture，阻断 Docker 后仍通过；测试冻结/引用语义不变，生产资源准备路径由独立测试验证。
- **C-2**：MCP 用例没有改为模拟成功、没有跳过。两种构建都通过真实 stdio 发现、精确工具授权、Docker 调用及结果 13 的历史回执。镜像构建缓存由实际选中的出厂源提前准备。
- **D**：原先单独重跑才绿的三个 authoring 用例，在两轮完整 57 项运行中全部通过。接入用例逐测试独立 Host/数据集，清理其进程和目录。

## 执行记录与身份

原始日志、截图和临时数据位于 `/tmp/devdesk-lint-ci-20261115-acceptance/`，不提交到仓库：

- `execution-baseline.log`：修改前复现及 locator 失败；对应快照已证明历史标题带计数。
- `previous-rust-ci.log`：原提交 GitHub rust run [43683740129](https://github.com/xfork24/devdesk/actions/runs/43683740129) 的失败日志，实际错误为权限 Schema 相对引用未解析。
- `check.log`：完整 Rust 测试结果，291/0/17。该轮外层 Shell 因执行中修改脚本而在 Rust 完成后读取中断，不能把它的退出码写成完整入口通过。
- `check-final-source.sh` / `check-prefix.log` / `check-tail.log`：固定最终 check.sh 内容后，重新执行前置检查并从 Host 构建继续执行后半段，均退出 0；Rust 无行为改动后不重复整套测试。后续仅涉及 TypeScript 环境和 Schema 自校验的调整均另有针对性复验。
- 最终 check.sh SHA-256：`e4ac6c0bd9347c2cab381eaee1e8c39bce33b0c950b5c32455f8dafccaac9b4e`。
- `gate-identities.json` / `binary-identities.json`：检查入口、workflow 与实际 debug/release 二进制身份。
- `debug-browser-artifacts/` / `release-browser-artifacts/`：两轮截图；已目视检查阶段/子流程布局和 390×844 移动端历史面板。
- `permission-schema.log`、`role-without-docker.log`、`ci-gate.log`、`npm-ci-final.log`、`typecheck-final.log`、`format-final.log`：补充边界与类型验证。

HTTP 运行准备专项使用明确的 Docker CLI 协议夹具；MCP 浏览器专项实际使用 Docker。默认测试未调用付费模型或真实业务 GitHub 仓库，也没有把上述结果写成全部原生工程能力已经验收。

## 首轮远端观察与补充修复

提交 `1e634db` 的 [Rust](https://github.com/xfork24/devdesk/actions/runs/43686769199)、[Web](https://github.com/xfork24/devdesk/actions/runs/43686769208)、[实际 e2e](https://github.com/xfork24/devdesk/actions/runs/43687740806) 均成功：Ubuntu Rust 290 / macOS Rust 291 项通过；浏览器 debug 57 项（6.7 分钟）、release 57 项（2.2 分钟）通过，HTTP/数据协议/环境准备全部通过。较早的 [e2e](https://github.com/xfork24/devdesk/actions/runs/43686867520) 仅检查前置并跳过浏览器，未计入通过证据。

继续审读完整日志发现：setup-rust-toolchain 内置缓存和显式 rust-cache 重复执行，Linux post 清理报 `Unable to locate executable file: cargo`，但 action 没有将其传播为失败。补充配置在两个 Rust 工作流中关闭内置缓存，并设置显式缓存 `cache-bin: false`；只缓存构建/依赖，不处理 rustup 的代理。该项以补充提交重新运行后的实际 post 日志为准。原日志保存在 `rust-ci.log` 与 `e2e-ci.log`。

## 远端 CI 验收合同

`rust.yml` 必须在 Ubuntu 和 macOS 都成功，`web.yml` 必须成功；随后 e2e 前置门禁核对同一提交与事件的最新运行，实际 e2e job 必须完成 debug/release Host 与全部浏览器检查。较早因前置尚未齐全而跳过浏览器的 workflow 不能作为 e2e 通过证据。远端若暴露新问题，修复后继续提交并核对最终版本，不用本地通过替代远端结果。
