# 执行报告：代码规范、Lint 体系与 CI 建设

分支 `code-standards-lint-ci`，2026-11-15 执行完毕。计划原文见 [PLAN.md](PLAN.md)；执行中发现的存量问题（与本次改动无关、未处理）单独记录在 [ISSUES.md](ISSUES.md)。

## 做了什么

| 提交 | 内容 |
|---|---|
| `0a90a01` Adopt rustfmt/clippy workspace gates and clear existing findings | `[workspace.lints]`（clippy::all + unused，warn 级、CI/门禁 -D 拒绝）+ 五 crate `[lints] workspace = true`；domain/core/host `#![forbid(unsafe_code)]`；清零全仓 clippy 存量约 190 项（92 项 `cargo clippy --fix` 机械修复，其余手工：edition-2024 let-chains 合并、derivable Default、byte-char-slices、useless conversion、credential-helper 平台门控 dead-code）；阈值型 lint 定点 `#[allow]` + 一行理由（参数个数 ×8、DTO 枚举体积 ×4、type_complexity、field_reassign、duplicate_mod 测试支持文件自包含引入）；workflow.rs 测试模块移至文件末尾 |
| `6ec7714` Add ESLint/Prettier toolchain and clear web findings | ESLint 10 flat config（Vite react-ts 公开基线：@eslint/js + ts-eslint recommended；react-hooks 限定 src 使 Playwright 辅助函数不被误判；测试目录豁免 no-explicit-any）；Prettier 纯默认值 + ignore；`tsconfig.test.json` 补齐 tests 从未被类型检查的缺口；npm scripts（lint/lint:fix/format/format:check/typecheck）；根 `.editorconfig`。存量清理：20 文件格式化；49 处逗号运算符语句展开（TS parser codemod，处理嵌套括号）；18 处未使用 import（codemod）；死代码删除（main.tsx 的 fmt/roleRevision/snapshotA-B/configurationLock/openArtifact/RunControls、RoleConversation installations prop 等）；23 处 src 显式 any 全部收拢到 configuration-ui 的 `Row`/`Request` 唯一逃生舱（带豁免注释）；main.tsx `RecordData` 别名并入 `Row`；RevisionSelect onChange 收窄 `string | string[]` 后约 14 处调用方按单选/多选断言适配 |
| `30797bc` Document code standards and wire the style gate | `docs/code-standards-rust.md`、`docs/code-standards-web.md`（基线、惯例固化、定点豁免协议、警告收紧路径）；`scripts/check/check-style.sh`（fmt → clippy -D → eslint → prettier）接入 `check.sh`；AGENT.md 风格段、README.md 文档表、scripts/README.md 索引更新 |
| `8314eb5` Add rust, web and e2e GitHub Actions workflows | `rust.yml`（读 rust-toolchain.toml 的 1.99.1 + rustfmt/clippy、Swatinem 缓存、先构建 web/dist 供 RustEmbed 嵌入、fmt/clippy/test + 三个可移植配置检查）；`web.yml`（lint/typecheck×2/format:check/build）；`e2e.yml`（workflow_run 依赖 rust+web：head_sha 并发组去重、guard 校验触发方成功、轮询另一方同 commit 结论，双绿后构建 debug host + Playwright chromium；文件内注明 fork PR 不触发） |
| `e304993` Keep Playwright's fixture destructuring under the empty-pattern lint | `base.extend` fixture 首参必须解构模式：恢复 `({}, use)` 规范写法 + 定点 no-empty-pattern 豁免（`(_, use)` 能过 lint 但破坏 Playwright 签名校验） |

## 验证结果

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过（改动前实测已干净，历史文档所述差异已被先前提交消化） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过（存量约 190 项清零） |
| `cargo test --workspace --locked` | 除 1 个 Docker 网络依赖项外全绿；该项在未含本次改动的 main 上同样失败（ISSUES.md C-1） |
| `npm run lint` | 0 errors；75 warnings 为建议级（刻意省略的 effect 依赖、共享模块混导出、Row 定义点），策略见 code-standards-web.md |
| `npm run format:check` / `typecheck`（src+tests）/ `build` | 全部通过 |
| `./scripts/check/check-style.sh` | 端到端通过 |
| Playwright e2e（57 用例，debug host） | 48 过 9 败；9 个失败全部为存量或环境问题（ISSUES.md），其中 3 个经单独重跑确认是全套运行时 Docker 失败状态的连锁污染，非独立缺陷 |

**零行为变更的证据**：所有失败用例经 `git worktree` 干净 main 基线对照复现；authoring 3 个失败用例单独重跑通过。Rust 修复全部为语义等价变换（let-chains 合并、派生替换、死代码 cfg 门控），web 修复全部为格式化/类型层/编译器验证无引用的死代码删除。

## 与批准方案的偏差

1. `lint` 脚本不含 `--max-warnings 0`：实测 185 个存量 warning 绝大多数是刻意模式（Row 逃生舱、刻意省略的依赖、共享模块），全量清零需 UI 层重构超出本任务范围。改为 errors 严格拒绝 + warnings 可见并在规范文档约定收紧路径。
2. rust workflow 需先 `npm run build`：host 以 RustEmbed 嵌入 web/dist，缺失则 clippy/test 无法编译（与 check.sh 同序）。
3. 追加第 5 个提交（fixture 语法）：e2e 验证阶段发现 no-empty-pattern 的机械修复破坏 Playwright fixture 签名，恢复规范写法。

## 遗留

存量问题见 [ISSUES.md](ISSUES.md)，按用户指示本次不修改任何前后端逻辑与 UI 行为，仅记录。
