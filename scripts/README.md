# 操作脚本索引

从仓库根目录使用以下入口。脚本会按自身位置定位仓库；平台安装器打进运行包后也保留相同目录结构。

| 目录 | 入口 | 用途 |
|---|---|---|
| `dev/` | `build.sh` | 安装前端依赖、校验权限资产、准备 macOS 凭据助手、构建 Web 和 release Host、签名 |
| `dev/` | `dev.sh` | 构建 Web 和 debug Host、签名并启动；macOS 从独立签名副本运行，参数传给 Host |
| `dev/` | `run-host.py` | 启动已构建的 Host；macOS 使用私有临时副本，后续重建不影响运行中签名，退出时清理 |
| `check/` | `check.sh` | 默认离线回归：权限资产、Web、Rust、HTTP、运行环境准备、debug/release Chromium |
| `check/` | `check-live.sh` | 调用 `tests/live/check_live.py`，执行专用资源上的真实验收或已有证据复核 |
| `check/` | `check-permissions.sh` | 校验权限预设、后端 manifest 与不可变文件摘要 |
| `check/` | `doctor.sh` | 检查宿主环境、Docker、GitHub CLI 和所选权限后端 |
| `release/` | `package.sh` | 先构建，再生成本机独立运行包与 SHA-256 文件 |
| `release/` | `package-source.sh` | 打包当前工作区源码与文件清单，排除运行数据和构建产物 |
| `platform/macos/` | `setup-macos-signing.sh` | 准备或导入 DevDesk 本地稳定签名身份 |
| `platform/macos/` | `install-macos-credential-helper.sh` | `--ensure` 校验并复用兼容助手供启动/构建使用；更新既有安装需显式 `--update` |
| `platform/macos/` | `sign-macos.sh` | 对指定 Host 或助手二进制应用配置好的签名身份 |
| `platform/linux/` | `install-linux-sandbox-policy.sh` | 检查或安装受限嵌套沙箱的命名 AppArmor 策略 |
| `py/` | `doctor.py`、`check-permissions.py`、`package-source.py` | 对应 Shell 入口的 Python 实现 |
| `py/` | `credential-helper-fingerprint.py` | 计算凭据助手自身源码及依赖的指纹，由 macOS 安装器调用 |

常用命令：

```bash
./scripts/dev/build.sh
./scripts/dev/dev.sh --data ./data --port 4317
./scripts/check/check.sh
./scripts/release/package.sh
./scripts/release/package-source.sh
```

macOS 首次配置见 [系统凭据](../docs/system-credentials.md)，真实验收见 [工程接入](../docs/real-engineering.md)，测试分层见 [测试索引](../tests/README.md)。旧版 `scripts/<文件名>.sh` 已移动到上表目录，历史执行报告中的旧命令保留原样；运行新命令请使用当前路径。
