# devdesk-tools：独立辅助程序

这个包存放需要独立运行的工具，按 `src/bin/` 组织。当前只有 `devdesk-credential-helper`，它为 macOS Host 提供稳定的 Keychain 访问主体。

独立进程的原因是 Host 经常重编，而 Keychain 授权需要可识别的签名程序。共享助手不会因每次 Host 构建而被任意替换。

| 主题 | 阅读入口 |
|---|---|
| 请求、校验和错误 | [助手协议](request-protocol.md) |
| 构建、签名、安装和验证 | [平台运行](signing-and-keychain.md) |
| 所有文件 | [files.md](files.md) |

包名是 devdesk-tools，现有二进制名仍为 devdesk-credential-helper。Linux 不用这个助手访问凭据，使用 adapters 的 Secret Service 实现。

新增工具在 src/bin 下提供独立入口，并说明输入、输出、退出行为、权限与构建方式。普通业务逻辑继续放在 domain/core/adapters，不因这个目录叫 tools 就迁入所有辅助函数。
