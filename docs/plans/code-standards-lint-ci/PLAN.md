# 代码规范、Lint 体系与 CI 建设计划

状态：已完成（分支 `code-standards-lint-ci`，2026-11-15）。执行报告见 [EXECUTION.md](EXECUTION.md)；执行中发现的存量问题（与本次改动无关、按用户指示未修改）见 [ISSUES.md](ISSUES.md)。

## 背景与现状结论

探索确认当前项目**没有任何 lint 基础设施**：无 `[workspace.lints]`、无 rustfmt.toml/clippy.toml、无 ESLint/Biome（prettier 已在 devDependencies 但零配置零调用）、`check.sh` 不含任何 fmt/clippy 步骤、`.github/` 目录不存在（无任何 CI）。历史验收文档曾记录存量格式差异与 clippy 告警；本次执行实测 `cargo fmt --all -- --check` 已干净，clippy `-D warnings` 存量约 190 项（domain 11、core 104、adapters/host/credential-helper 其余）。

代码本身有稳定的事实惯例（注释/文档英文、用户可见文案中文、domain 单一 `Error(code,message)` 模式、serde 全 snake_case、web 侧 React19+hooks+全局 CSS），但无成文标准且存在漂移（web 文件命名 PascalCase/kebab 混用、`Row` 与 `RecordData` 重复别名、23 处 src 显式 `any`）。

用户决策：**① 立即修复存量（严格门禁从第一天绿色）② Web 用 ESLint + Prettier ③ e2e 作为独立 workflow，依赖 rust 和 web 通过后运行。**

## 交付物总览

1. 两份代码规范文档（参考公开规范 + 固化本仓库事实惯例）
2. Rust/Web lint 脚本与配置 + 存量清零
3. 三个 GitHub Actions workflow：`rust.yml`、`web.yml`、`e2e.yml`（workflow_run 依赖）

## 一、Rust 侧

**配置（已完成）**
- 根 `Cargo.toml`：`[workspace.lints]`——`clippy::all = warn` + `rust` `unused = warn`（本地 warn、CI 与 check-style.sh 以 `-D warnings` 拒绝）
- 五个 crate 各加 `[lints] workspace = true`
- domain/core/host 源码入口 `#![forbid(unsafe_code)]`；adapters/credential-helper 有 5 处 libc 调用保持允许
- 不新增 rustfmt.toml/clippy.toml（显式声明"rustfmt/clippy 默认即标准"）

**存量修复（已完成）**
- `cargo fmt --all`；实测原本已干净
- clippy 清零约 190 项：机械项（collapsible_if/needless_borrow/redundant_closure 等）以 `cargo clippy --fix` 批量处理；其余手工（let-chains 合并、derivable_impls、byte_char_slices、cmp_owned、useless_conversion、credential-helper 平台门控 dead_code）
- 阈值型 lint（too_many_arguments ×8、type_complexity ×1、large_enum_variant ×4、field_reassign_with_default ×1、duplicate_mod ×2）采用**定点 `#[allow]` + 一行理由注释**（serde DTO/协议字段一一映射/自包含测试支持文件架构），不做整体 workspace 豁免
- workflow.rs 测试模块移至文件末尾（items_after_test_module）

## 二、Web 侧

**配置（已完成）**
- `web/eslint.config.js`（ESLint 10 flat）：`@eslint/js` recommended + `typescript-eslint` recommended + react-hooks（显式规则声明，版本无关）+ react-refresh + eslint-config-prettier + globals；hooks 规则限定 `src/**`（Playwright 测试的 `use*` 命名不是 React）；测试目录豁免 `no-explicit-any`（规格文件搬运内核 JSON 行）；`no-empty` allowEmptyCatch；`no-unused-vars` `^_` 前缀豁免
- `web/.prettierrc.json`（纯默认值即标准）+ `.prettierignore`
- `web/tsconfig.test.json`：填补 tests/playwright.config.ts 从未被类型检查的缺口
- `web/package.json` scripts：`lint`（errors 拒绝；warning 为可见建议）、`lint:fix`、`format`、`format:check`、`typecheck`（两个 project）；devDeps：eslint 系全家桶 + @types/node
- 根 `.editorconfig`

**存量修复（大部分完成）**
- `prettier --write`（20 文件机械格式化）
- 逗号运算符语句 `(a(), b());` 展开 49 处（TypeScript parser codemod，处理嵌套括号）
- 未使用 import 清理 18 处（codemod）+ 手工删除死代码（main.tsx 的 fmt/roleRevision/snapshotA/B/configurationLock/openArtifact/RunControls、RoleConversation installations prop、tests 无用 locator）
- 杂项：prefer-const、no-useless-escape ×2、no-useless-catch（纯重抛 try/catch）、no-empty-pattern ×2
- src `any` 清零：统一以 configuration-ui 的 `Row`/`Request` 为唯一逃生舱（带 eslint-disable 与理由注释），删除各文件重复本地定义（AgentExperience/RepositorySettings/RoleConversation/PermissionResources/PermissionSettings/RuntimeSettings/DshProviderSettings）；main.tsx `RecordData` 别名整体并入 `Row`；JsonField onChange `unknown`；RevisionSelect onChange 收窄 `string | string[]` 后适配约 14 处单选/多选调用方（`as string`/`as string[]` 断言，调用方知晓 multiplicity）

## 三、Lint 脚本与审查机制（待办）

- 新增 `scripts/check/check-style.sh`（遵循现有惯例：`set -euo pipefail` + cd 根目录 + 纯文本输出）：fmt --check → clippy -D warnings → web lint → web format:check
- 接入 `scripts/check/check.sh`（插在 check-architecture.sh 之后）
- 更新 scripts/README.md、AGENT.md 风格段、README.md 当前文档表

## 四、代码规范文档（待办）

- `docs/code-standards-rust.md`：基线（rustfmt 默认 = Rust Style Guide、clippy::all）；命名（引 Rust API Guidelines）；固化惯例——`//!` 单行英文模块契约、注释英文/用户文案中文、domain `Error(code,message)` 与 snake_case 错误码、serde 只用 snake_case + `deny_unknown_fields` + 显式 default、`ok_or_else`/`let-else`/crate 内 wildcard import、测试命名陈述不变量 + `#[ignore] "理由"`、domain/core/host 禁 unsafe、定点 allow + 理由注释的豁免协议
- `docs/code-standards-web.md`：基线（ESLint flat + ts-eslint recommended + react-hooks + Prettier 默认，引 typescript-eslint/Prettier 官方文档）；命名（新组件 PascalCase、非组件工具 kebab-case，存量不改名）；函数组件 + 具名导出 + `type Props`；type alias 优先、禁 enum、`Row`/`Request` 唯一 any 豁免；hooks 模式（generation ref 防竞态、useSyncExternalStore）；全局 CSS + kebab 类名 + 禁 inline style；rpc() 统一请求、mutation busy/error 协议、`void` 浮动 promise、禁逗号运算符语句；测试中文标题 + flat 结构 + getByRole；警告（exhaustive-deps 等）为建议级，PR 审查应关注新增

## 五、GitHub Actions（待办）

**`rust.yml`**（push main + pull_request）：checkout → `actions-rust-lang/setup-rust-toolchain@v1`（读 rust-toolchain.toml）→ `Swatinem/rust-cache@v2` → 先 `npm --prefix web run build`（host 嵌入 web/dist 依赖）→ fmt --check → clippy --workspace --all-targets --locked -D warnings → cargo test --workspace --locked → check-architecture/check-defaults/check-permissions

**`web.yml`**（触发同上）：checkout → `actions/setup-node@v5`（node 24 + npm cache）→ `npm --prefix web ci` → lint → typecheck → format:check → build

**`e2e.yml`**（依赖 rust+web 的独立 workflow）：`on.workflow_run: [rust, web] completed` + `concurrency: e2e-<head_sha>` cancel-in-progress 去重；guard 检查触发方 conclusion == success；`gh run list --commit <head_sha>` 轮询另一方结论（上限 ~15 分钟）；双绿后：checkout（指定 head_sha）→ Rust 工具链 → `cargo build --locked -p devdesk-host`（debug）→ node + npm ci → `npx playwright install --with-deps chromium` → `(cd web && npx playwright test)`。注释说明 workflow_run 对 fork PR 的限制。

## 提交划分（单一主题）

1. **Adopt rustfmt/clippy workspace gates and clear existing findings**（Rust 配置 + fmt + clippy 修复）
2. **Add ESLint/Prettier toolchain and clear web findings**（web 配置 + 格式化 + any/别名/hooks 修复）
3. **Document code standards and wire the style gate**（规范文档 + check-style.sh + 索引更新）
4. **Add rust, web and e2e GitHub Actions workflows**

## 验证清单

1. `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets --locked -- -D warnings` 全绿（已达成）
2. `cargo test --workspace --locked` 全过（运行中）
3. `npm --prefix web run lint && run typecheck && run format:check && run build` 全绿
4. `./scripts/check/check-style.sh` 退出码 0
5. `(cd web && npx playwright test)` 全过（web 修复不破坏 UI）
6. 推送分支后观察三个 workflow：rust/web 绿 → e2e 自动触发并绿

## 与批准方案的偏差记录

- `lint` script 不使用 `--max-warnings 0`：实测存量 185 个 warning 中绝大多数是刻意模式（Row 逃生舱体系、刻意省略的 effect 依赖、共享模块的组件+工具混导出）。errors 级严格拒绝、warnings 保持可见并由规范文档约定"新增需在 PR 中说明"；全量清零需要 UI 层重构，超出 lint 基建范围。
- rust workflow 需先构建 web/dist：host crate 以 RustEmbed 嵌入 web/dist，缺失则 clippy/test 无法编译（check.sh 同序）。
