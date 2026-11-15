# Rust 代码规范

基线工具链之外没有第二个风格仲裁者：**rustfmt 默认值就是格式标准，clippy 默认告警集（`clippy::all` + `unused`）是静态检查基线**。本仓库不提供 rustfmt.toml / clippy.toml 覆盖；如需调整风格，改的是这份文档和 workspace lints，不是个人编辑器配置。

- 格式检查：`cargo fmt --all -- --check`（必须零差异）
- 静态检查：`cargo clippy --workspace --all-targets --locked -- -D warnings`（必须零告警）
- 本地开发看到的是 warn 级提示；CI 与 `scripts/check/check-style.sh` 直接拒绝
- 公开依据：[Rust Style Guide（rustfmt 实现的规范）](https://doc.rust-lang.org/nightly/style-guide/)、[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)、[Clippy lint 目录](https://rust-lang.github.io/rust-clippy/master/index.html)

## 分层不变量

| 规则 | 说明 |
|---|---|
| `crates/domain` 纯函数层 | 无 I/O、无时钟/随机数/环境读取；错误与合同在此定义 |
| 依赖方向 | `host → adapters → core → domain`；core 只依赖 `ports.rs` trait，不导入具体适配器 |
| unsafe 边界 | domain/core/host 源码入口 `#![forbid(unsafe_code)]`；libc 调用只出现在 adapters 与 credential-helper |
| 副作用协议 | "持久化意图 → 事务外执行 → 核对接纳回执"；不在数据库事务内等待模型/镜像/网络 |

## 命名

遵循 Rust API Guidelines 的 C-CASE 与 C-CONV：

- 模块与文件 `snake_case`，类型与 trait `CamelCase`，常量 `SCREAMING_SNAKE_CASE`，函数与字段 `snake_case`；执行动作的函数优先用动词，查询和字段按含义命名
- 端口 trait 以能力命名（`Harness`、`Git`、`EngineeringStore`），一律 `Send + Sync`
- 测试函数名陈述被验证的不变量，不加 `test_` 前缀：`revision_conflicts_and_pagination_are_enforced`、`artifact_symlinks_cannot_escape_dataset`
- 跳过真实环境测试用带理由的 `#[ignore = "explicit local Docker protocol acceptance; no model input"]`

## 语言分工（本仓库最重要的约定）

- 新增代码注释与 `///`/`//!` 文档优先英文；`//!` 模块契约可以使用多行，说明该模块是什么、边界在哪里（"GitHub is the remote PR system. This adapter never invokes a platform merge."）
- 面向工作台用户的文案使用中文：错误消息、`require(...)` 的说明文本。外部协议原文、机器输出、工具名和开发检查日志保留相应格式。`err("not_found", "Project 不存在")` 是标准形态
- 错误码是 snake_case 机器字符串（`blocked`、`revision_conflict`、`dataset_locked`）；单一 `domain::Error { code, message }` + `Result<T>` 别名贯穿全仓，不引入 anyhow，不新建每 crate 错误枚举

## 惯用法

- 错误构造有分配或计算时使用 `ok_or_else`，廉价常量可用 `ok_or`；使用 `let ... else` 早退，按 clippy 建议合并 let-chains，保持求值顺序和副作用不变
- 外来错误用小型适配函数转换：`.map_err(storage)?`；`require(cond, msg)?` 取代手写 if-return
- 自有持久化/RPC 合同使用 snake_case；外部协议按其字段名显式映射。拒绝未知字段与默认值由具体合同决定：受控写入 DTO 通常使用 `deny_unknown_fields`，外部只读响应可能需要兼容新增字段，不能全仓机械添加。`#[serde(default)]` 必须符合缺省值语义。派生顺序优先沿用文件惯例
- lib.rs 组织：`mod x; pub use x::*;` 为默认，少量模块走 `pub mod x;` 显式路径
- crate 内模块顶部 `use crate::*;`（adapters 用 `use super::*;`）是既定风格，不算风格违规
- `.unwrap()` 只出现在测试与"已验证不变量"点（如序列化已知 JSON）；生产不可达路径用带理由的 `.expect("...")`
- 压缩单行 `json!({...})` 构建查询响应是既定风格；rustfmt 不会展开它们，不强求

## 豁免协议

clippy 阈值型 lint（参数个数、serde DTO 枚举体积、Default 后逐字段装载、自包含测试支持文件重复引入）允许**定点豁免**：

```rust
// Record fields map one-to-one onto the persisted governance row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record(...)
```

规则：**只允许逐处 `#[allow]` + 一行英文理由，禁止 workspace 级放宽、禁止无注释豁免**。新增豁免必须在 PR 说明中给出理由。

## 测试

- 单元测试内联 `#[cfg(test)] mod tests { use super::*; ... }`，且**必须位于文件末尾**（clippy::items_after_test_module）
- 集成测试在 `crates/adapters/tests/`；跨 crate 共享夹具经 `#[path]` 引入 `tests/support/`（其自包含重复引入已定点豁免 duplicate_mod）
- 临时目录用 `tempfile`；夹具保持纯离线——真实 Docker/模型/GitHub 测试必须 `#[ignore]` + 专用入口
- 平台相关代码（macOS Keychain、unix 权限位）用 `#[cfg(...)]` 门控，Linux 上的门控死代码用 `cfg_attr(not(...), allow(dead_code))` 而非删除测试

## 门禁覆盖范围

`rust.yml` 在 Ubuntu 与 macOS 上运行 fmt、clippy、workspace 测试和可移植配置检查，避免 `cfg` 分支只在一个平台上合格。工具检查不代替协议/副作用/持久化的行为验收。`check-style.sh` 先检查并构建 Web，随后运行 clippy，支持尚无 `web/dist` 的干净源码目录；执行前需要 `npm --prefix web ci`。

## 提交前自查

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

或直接 `./scripts/check/check-style.sh`（见 [脚本索引](../scripts/README.md)）。
