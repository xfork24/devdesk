# Repository Guidelines

## 项目结构与模块职责

- `crates/domain`：领域类型、状态规则与验收约束，不执行 I/O。
- `crates/core`：命令、查询、编译、调度与权威终态裁定；对适配器的依赖全部经 `ports.rs` trait 注入。
- `crates/adapters`：SQLite、Artifact、Git、Docker、Codex/DSH Harness 及系统凭据的实际实现；集成测试集中在 `crates/adapters/tests/`。
- `crates/host`：HTTP/SSE 服务、RPC 总路由、跨项目调度并嵌入 Web；`crates/credential-helper`：macOS 共享凭据助手。
- `web/src`：React/TypeScript 界面，样式在 `style.css`；`web/tests`：浏览器测试。
- `resources/configuration/`：出厂默认配置目录（源协议 v2，`manifest.json` 声明发布内容），构建期内嵌并校验。
- `sql/`：SQLite 当前结构基线（global 16 / project 47），只建新库不迁移；`schemas/`：领域合同 JSON Schema；`permissions/`：权限预设与执行后端资产；`containers/`：Harness Docker 配方。
- `tests/`：`entrypoints/` 跨进程检查入口、`fixtures/` 夹具、`live/` 真实验收探针、`reporters/` 嵌入式测试报告器、`support/` Rust 测试共享支持。
- `docs/`：现行系统文档（根目录）与 `docs/plans/` 历史计划/归档验收；`review/`：评审产出（项目文档与改进计划提案）。
- `scripts/`：`dev/` 构建和开发、`check/` 检查和诊断、`release/` 打包、`platform/{macos,linux}/` 平台配置、`py/` Python 辅助实现。

## 构建、测试与本地开发

准备 Rust 1.99.1、Node 24、npm、Python 3 和 C 编译工具链。macOS 构建前按 `docs/system-credentials.md` 配置签名身份。以下命令均从仓库根目录执行：

- `./scripts/dev/build.sh`：安装前端依赖，构建 Web 与 release Host。
- `./scripts/dev/dev.sh --data ./data --port 4317`：构建并启动开发 Host；访问打印的 `127.0.0.1` 地址。
- `npm --prefix web run build`：TypeScript 检查与 Vite 构建；界面改动后重新构建并重启 Host。
- `cargo test --workspace --locked`：运行 Rust 默认测试。
- `./scripts/check/check.sh`：运行 Web 构建、Rust、HTTP 契约及 debug/release Chromium 回归。首次先执行 `npm --prefix web ci` 和 `(cd web && npx playwright install chromium)`。
- `./scripts/release/package.sh`：生成 `dist/` 运行包及 SHA-256 校验文件。

## 编码风格与命名

Rust 使用 rustfmt、4 空格缩进，函数和模块采用 `snake_case`，类型采用 `PascalCase`；用 `cargo fmt --all -- --check` 检查。前端保持所在文件风格，常规缩进为 2 空格，组件采用 `PascalCase`，变量采用 `camelCase`；可用已安装的 Prettier 格式化修改文件。目前没有独立 lint 脚本。数据库使用当前结构基线；变更需要显式迁移，不添加旧协议或旧行为回退。

## 测试规范

Rust 使用内置 `#[test]`，单元测试位于模块内，集成测试位于 `crates/adapters/tests/`；测试函数用描述行为的 `snake_case`。HTTP 契约位于 `tests/entrypoints/http_contract.py`，运行环境准备回归位于 `tests/entrypoints/runtime_preparation.py`，Playwright 用例采用 `web/tests/*.spec.ts`；完整职责见 `tests/README.md`。修复缺陷时补充对应回归，重点验证幂等、恢复、取消和项目隔离。目前未设置覆盖率百分比门槛。真实 Docker/GitHub 用例通常标记为忽略，须准备专用资源后单独执行；真实验收入口见 `scripts/check/check-live.sh` 与 `docs/real-engineering.md`。

## 文档规范

- 现行系统文档在 `docs/` 根目录（领域、协议、架构、工作流与编排、权限、存储、讨论与经验、Web 工作台、各接入与运维主题），描述当前系统状况；改动系统行为时同步更新对应文档，入口清单见根 `README.md` 文档节。
- `docs/plans/<计划名>/` 是历史计划、合同与执行/验收记录（含归档验收），只增不改、不做回填修订。
- 版本相关表述（Workflow schema、渲染器合同、SQL 基线、数据集标记等）以代码与 `schemas/`、`sql/`、`resources/configuration/` 为准；文档不维护独立的版本对照机制。

## 提交与合并请求

历史提交以简短英文祈使句为主，例如 `Isolate project databases and introduce injectable execution adapters`，未统一要求 Conventional Commits。每次提交聚焦一个主题。合并请求应说明变更目的、行为影响、验证命令与结果，并关联适用的问题；界面变更附截图。涉及协议、迁移或恢复行为时同步更新 `schemas/` 和相关文档。

## 架构与配置约束

- 依赖方向严格单向：`host → adapters → core → domain`；domain 零 I/O；core 不 import 适配器具体类型，只依赖 `ports.rs` 的 trait。
- 业务终态由 core 接纳，前端和适配器不得直接设置；外部调用遵循"持久化意图、事务外执行、接纳回执"，保留冻结输入与真实来源；未知结果进入核对，绝不盲目重发。
- 一切配置为不可变源修订（源协议 v2，引用 `identity:revision`）；数据库只存源修订与执行选择/意图/回执，不建投影表，派生视图加载时按需重建。
- 权限独立于能力：Agent 与 Capability 不含权限字段；Harness/Sandbox 的 v2 Profile 与按流程步骤授权的 Permission Context 分开维护。
- 真实会话必须运行在 Linux 容器；来源只接受 External（拒绝模拟证据）；Agent 绝不作为人类批准。
- 系统只支持当前协议与数据结构，不保留历史原型执行路径；不通过删库恢复或升级数据集。
- 凭据接入遵循 `docs/system-credentials.md`；不要提交密钥、运行数据（`data/`、`dist/`）或构建产物。

## 计划与执行记录

`docs/plans/<计划名>/` 只保留 Markdown 计划、契约和 execution report。日志、JSON/JSONL、截图、临时验收脚本和测试输出写入临时目录，不作为计划交付物保存。源码回归测试保留在 `crates/`、`tests/`、`web/tests/`。历史 Markdown 中的原始路径与 SHA 仅记录当时执行，清理后不得声称相应中间文件仍可读取。
