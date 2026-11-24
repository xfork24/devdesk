# 操作脚本索引

从仓库根目录使用以下入口。脚本按自身位置定位仓库。

| 目录 | 入口 | 用途 |
|---|---|---|
| `dev/` | `build.sh` | 安装前端依赖、校验权限资产、构建 Web 和 release Host，并在 macOS 上签名 |
| `dev/` | `dev.sh` | 构建 Web 和 debug Host、签名并启动；macOS 从独立签名副本运行，参数传给 Host |
| `dev/` | `run-host.py` | 启动已构建的 Host；macOS 使用私有临时副本，后续重建不影响运行中签名，退出时清理 |
| `check/` | `check.sh` | 默认回归：权限资产、Web、Rust、HTTP、运行环境准备、debug/release Chromium |
| `check/` | `check-live.sh` | 调用 `tests/live/check_live.py`，执行专用资源上的真实验收或已有证据复核 |
| `check/` | `check-architecture.sh` | 检查层依赖、domain 环境访问、出厂资产与业务接纳边界 |
| `check/` | `check-style.sh` | 风格与构建门禁：rustfmt、clippy `-D warnings`、ESLint、TypeScript、Prettier 与 Web 构建；规范见 `docs/crates/standards/` |
| `check/` | `check-permissions.sh` | 校验权限预设、后端 manifest 与不可变文件摘要 |
| `check/` | `doctor.sh` | 检查宿主环境、Docker、GitHub CLI 和所选权限后端 |
| `release/` | `package.sh` | 按 `host`、`web`、`software` 分别生成独立制品及 SHA-256 |
| `release/` | `package-source.sh` | 打包当前工作区源码与文件清单，排除运行数据和构建产物 |
| `platform/macos/` | `setup-macos-signing.sh` | 准备或导入 DevDesk 本地稳定签名身份 |
| `platform/macos/` | `sign-macos.sh` | 对 Host 应用配置好的签名身份 |
| `platform/linux/` | `install-linux-sandbox-policy.sh` | 检查或安装受限嵌套沙箱的命名 AppArmor 策略 |
| `py/` | `doctor.py`、`check-permissions.py`、`package-source.py` | 对应 Shell 入口的 Python 实现 |

完整检查前，在已激活的 Python 虚拟环境中执行 `python3 -m pip install -r scripts/check/requirements.txt`；MCP 浏览器验收需要 Docker，`prepare_browser_runtime.py` 从已选出厂源预热镜像构建缓存。

常用命令：

```bash
./scripts/dev/build.sh
./scripts/dev/dev.sh --data ./data --port 4317
./scripts/check/check.sh
./scripts/release/package.sh host
./scripts/release/package.sh web
./scripts/release/package.sh software
./scripts/release/package-source.sh
```

macOS 首次配置见 [Host 签名与钥匙串](../docs/crates/devdesk-adapters/signing-and-keychain.md)，真实验收见 [工程交付](../docs/design/engineering-delivery.md)，测试分层见 [测试索引](../tests/README.md)。旧版 `scripts/<文件名>.sh` 已移动到上表目录，历史执行报告保留当时的命令。

文档检查：`python3 scripts/py/check-docs.py` 验证当前说明的本地链接和各 crate 的 `files.md` 索引；新增、删除或移动 crate 文件后运行 `python3 scripts/py/check-docs.py --write`。检查器回归为 `python3 tests/entrypoints/test_docs_checker.py`，已纳入 check.sh。原生配方导出由 `cargo run --quiet --locked -p devdesk-adapters --example export_runtime_recipes` 调用领域组合器，不复制另一份 Python 配方编译逻辑。
