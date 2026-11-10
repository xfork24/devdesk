# Repository Guidelines

## 项目结构与模块职责

- `crates/domain`：领域类型、状态规则与验收约束，不执行 I/O。
- `crates/core`：命令、查询、调度与执行接口；`crates/adapters`：SQLite、文件、Git、Docker 及原生供应商实现。
- `crates/host`：本地 HTTP/SSE 服务并嵌入 Web；`crates/credential-helper`：macOS 共享凭据助手。
- `web/src`：React/TypeScript 界面，样式在 `style.css`；`web/tests`：浏览器测试。
- `sql/project/schema.sql`：项目库当前结构，`sql/global/schema.sql`：全局库当前结构；结构基线见 `sql/README.md`。`schemas/`：协议；`permissions/`：权限预设和执行后端；`tests/entrypoints/`：跨进程检查入口；`tests/fixtures/`：夹具；`tests/reporters/`：嵌入式测试报告器；`docs/`：设计、操作说明及验收记录。
- `scripts/dev/`：构建和开发；`scripts/check/`：检查和诊断；`scripts/release/`：打包；`scripts/platform/{macos,linux}/`：平台配置；`scripts/py/`：由入口调用的 Python 辅助实现。

## 构建、测试与本地开发

准备 Rust 1.99.1、Node 24、npm、Python 3 和 C 编译工具链。macOS 构建前按 `docs/system-credentials.md` 配置签名身份。以下命令均从仓库根目录执行：

- `./scripts/dev/build.sh`：安装前端依赖，构建 Web 与 release Host。
- `./scripts/dev/dev.sh --data ./data --port 4317`：构建并启动开发 Host；访问打印的 `127.0.0.1` 地址。
- `npm --prefix web run build`：TypeScript 检查与 Vite 构建；界面改动后重新构建并重启 Host。
- `cargo test --workspace --locked`：运行 Rust 默认测试。
- `./scripts/check/check.sh`：运行 Web 构建、Rust、HTTP 契约及 debug/release Chromium 回归。首次先执行 `npm --prefix web ci` 和 `(cd web && npx playwright install chromium)`。
- `./scripts/release/package.sh`：生成 `dist/` 运行包及 SHA-256 校验文件。

## 编码风格与命名

Rust 使用 rustfmt、4 空格缩进，函数和模块采用 `snake_case`，类型采用 `PascalCase`；使用 `cargo fmt --all -- --check` 检查。前端保持所在文件风格，常规缩进为 2 空格，组件采用 `PascalCase`，变量采用 `camelCase`；可用已安装的 Prettier 格式化修改文件。目前没有独立 lint 脚本。数据库使用当前结构基线；变更需要显式迁移，不添加旧协议或旧行为回退。

## 测试规范

Rust 使用内置 `#[test]`，单元测试位于模块内，集成测试位于 `crates/adapters/tests/`；测试函数用描述行为的 `snake_case`。HTTP 契约位于 `tests/entrypoints/http_contract.py`，运行环境准备回归位于 `tests/entrypoints/runtime_preparation.py`，Playwright 用例采用 `web/tests/*.spec.ts`；完整职责见 `tests/README.md`。修复缺陷时补充对应回归，重点验证幂等、恢复、取消和项目隔离。目前未设置覆盖率百分比门槛。真实 Docker/GitHub 用例通常标记为忽略，须准备专用资源后单独执行；真实验收入口见 `scripts/check/check-live.sh` 与 `docs/real-engineering.md`。

## 提交与合并请求

历史提交以简短英文祈使句为主，例如 `Isolate project databases and introduce injectable execution adapters`，未统一要求 Conventional Commits。每次提交聚焦一个主题。合并请求应说明变更目的、行为影响、验证命令与结果，并关联适用的问题；界面变更附截图。涉及协议、迁移或恢复行为时同步更新 `schemas/` 和相关文档。

## 架构与配置约束

业务终态由 core 接纳，前端和适配器不得直接设置。外部调用遵循“持久化意图、事务外执行、接纳回执”，保留冻结输入与真实来源；系统只支持当前协议与数据结构，不保留历史原型执行路径。凭据接入遵循 `docs/system-credentials.md`；不要提交密钥、运行数据或构建产物，也不要通过删库恢复或升级数据集。

## 计划与执行记录

`docs/plans/<计划名>/` 只保留 Markdown 计划、契约和 execution report。日志、JSON/JSONL、截图、临时验收脚本和测试输出写入临时目录，不作为计划交付物保存。源码回归测试保留在 `crates/`、`tests/`、`web/tests/`。历史 Markdown 中的原始路径与 SHA 仅记录当时执行，清理后不得声称相应中间文件仍可读取。
