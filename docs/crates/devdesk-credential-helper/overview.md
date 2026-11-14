# devdesk-credential-helper：稳定签名的 macOS Keychain 主体

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

这是一个很小的独立可执行包，负责在 macOS 上访问 DevDesk 管理的 generic password。Host 可以频繁重建，固定安装的助手身份保持稳定；系统授权因此不会随每次 Host 构建改变。

它不是通用密码库代理：父进程签名、service 命名空间、账户前缀、操作和输入大小都受限制。普通操作禁止系统交互，只有显式 authorize 可以出现授权 UI。Host 经私有 stdin/stdout 调用，安装、manifest 和证书核对在 adapters 的 [macOS 凭据适配器](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

包里只有 Cargo.toml 和 src/main.rs。协议专题是源码的主要逐文件说明；签名专题把两边校验、构建和系统 Keychain 行为连成完整路径。

## 专题阅读

- [一次请求的协议、校验与错误](request-protocol.md)
- [父进程签名、固定安装与 Keychain 访问](signing-and-keychain.md)

## 完整文件索引

每个文件只有一个主要讲解位置；相关专题通过链接引用它。

| 文件 | 职责与实现说明 |
|---|---|
| [Cargo.toml](../../../crates/credential-helper/Cargo.toml) | [独立助手版本与 macOS 条件依赖](overview.md#file-cargo-toml) |
| [src/main.rs](../../../crates/credential-helper/src/main.rs) | [严格命名空间、签名调用者和单次 Keychain 操作](request-protocol.md#file-src-main-rs) |

<a id="file-cargo-toml"></a>

## Cargo.toml：独立助手版本与 macOS 条件依赖

源码：[crates/credential-helper/Cargo.toml](../../../crates/credential-helper/Cargo.toml)。

包版本独立固定为 1.0.0，edition 复用 workspace。serde/serde_json 负责一次性 JSON 协议，security-framework 与 libc 仅在 macOS 引入，用于 Keychain 和签名父进程核验。

它不依赖 core/adapters，也不链接业务 Engine。非 macOS 构建保留受控 unsupported 返回，不能把“能编译”当作跨平台凭据能力。

关联阅读：[credential-helper/src/main.rs](request-protocol.md#file-src-main-rs)、[adapters/src/credentials_macos.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
