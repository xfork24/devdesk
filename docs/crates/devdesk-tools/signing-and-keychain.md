# 凭据助手的构建、签名与安装

助手需要稳定签名、受控内容和匹配的 Host 调用者。普通 Cargo 构建只证明代码可编译，不能证明它能够通过真实 Keychain 授权链。

## 开发构建

在仓库根目录执行：

```bash
cargo build --locked -p devdesk-tools --bin devdesk-credential-helper --release
cargo test --locked -p devdesk-tools
```

二进制输出在 target/release。macOS 正式使用还需要匹配的签名和安装流程，不能直接以未签名测试程序替代受信任 Host。

## 安装流程

首次源码开发可先准备并导入本地开发签名身份，再安装助手：

```bash
./scripts/platform/macos/setup-macos-signing.sh --prepare
./scripts/platform/macos/setup-macos-signing.sh --import
./scripts/platform/macos/install-macos-credential-helper.sh
```

这些步骤由开发者在本机执行，导入只针对 DevDesk 开发证书及用户域的 code signing 信任。已有身份可通过 `DEVDESK_CODESIGN_IDENTITY` 指定，必要时同时设置 `DEVDESK_CODESIGN_CERTIFICATE_SHA1`。证书私钥不进入源码或发布包。

[install-macos-credential-helper.sh](../../../scripts/platform/macos/install-macos-credential-helper.sh) 提供 ensure、update 和 bundle 等入口。ensure 复用兼容安装；update 是明确升级；bundle 构建随运行包分发的助手，不替换本机共享安装。

Host 的 [credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) 核对当前要求的协议、内容摘要和证书；助手反向核对父 Host 的签名身份。目录名称 v1 不表示仍使用协议 1，当前协议要求为 2。

共享安装位于 `~/Library/Application Support/DevDesk/credential-helper/v1/`。Host 使用 `dev.devdesk.host` 标识，助手使用 `dev.devdesk.credentials` 标识。普通 Host/Web/core 修改不重建这个已授权安装。

开发启动使用 `scripts/dev/dev.sh` 或 [run-host.py](../../../scripts/dev/run-host.py) 创建私有签名副本，避免后续 Cargo 构建覆盖正在运行的二进制。直接运行 target 中的 Host 再覆盖文件，可能导致原进程的签名核验失败；应停止旧进程并通过稳定副本入口重启，不必因此重存密钥。

修改助手源码、协议或允许的账户范围时，同时检查指纹脚本 [credential-helper-fingerprint.py](../../../scripts/py/credential-helper-fingerprint.py)、安装验证和 Host 兼容检查。不可通过放宽签名或忽略内容摘要让本机验证“通过”。

## 平台验证范围

| 验证 | 能证明什么 |
|---|---|
| Cargo build/test | 编译与参数规则 |
| [credential_helper_startup.py](../../../tests/entrypoints/credential_helper_startup.py) | 启动及安装检查相关路径 |
| [credential_helper_runtime.py](../../../tests/entrypoints/credential_helper_runtime.py) | 在具备前置条件的机器上验证运行边界 |
| 设置页显式授权后的真实访问 | 当前签名和当前用户 Keychain 的授权结果 |

缺少签名身份时报告该限制；编译成功不能写成凭据功能已完成实机验收。运行包操作见[运行指南](../../design/run-binary.md)。
