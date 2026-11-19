# devdesk-tools：独立工具程序

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

本包集中存放 DevDesk 的独立工具代码，使用 `src/bin/<可执行文件名>.rs` 组织各工具入口。当前只有 `devdesk-credential-helper`，负责在 macOS 上访问 DevDesk 管理的 generic password。Host 可以频繁重建，固定安装的助手身份保持稳定；系统授权因此不会随每次 Host 构建改变。

它不是通用密码库代理：父进程签名、service 命名空间、账户前缀、操作和输入大小都受限制。普通操作禁止系统交互，只有显式 authorize 可以出现授权 UI。Host 经私有 stdin/stdout 调用，安装、manifest 和证书核对在 adapters 的 [macOS 凭据适配器](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

当前包里只有 Cargo.toml 和 src/bin/devdesk-credential-helper.rs。协议专题是凭据助手的逐文件说明；签名专题把两边校验、构建和系统 Keychain 行为连成完整路径。

## 工具组织与构建

新增工具时在 `src/bin/` 下添加独立入口，并同步补充本手册与需要的构建、测试或打包入口。工具内部模块按职责拆分；出现实际复用需求时再提取共享模块。业务领域规则与应用用例仍分别归 domain 和 core。

凭据助手的安装指纹覆盖其源码、tools 的 Cargo.toml、解析后的包依赖与签名证书；其他工具的独立源码不参与指纹。若将助手逻辑拆入新模块，须同步把这些源码加入 `scripts/py/credential-helper-fingerprint.py` 的输入清单。

Cargo 包名为 `devdesk-tools`；凭据助手的可执行文件名仍为 `devdesk-credential-helper`，安装路径、签名标识与请求协议保持原合同。构建和测试时明确选择目标：

```bash
cargo build --release --locked -p devdesk-tools --bin devdesk-credential-helper
cargo test --locked -p devdesk-tools --bin devdesk-credential-helper
```

macOS 签名发布由 `scripts/platform/macos/install-macos-credential-helper.sh --bundle <输出目录>` 完成，该入口同时设置可信 Host 证书并生成安装清单。普通 Cargo 构建不等于完成签名或系统授权。

## 专题阅读

- [一次请求的协议、校验与错误](request-protocol.md)
- [父进程签名、固定安装与 Keychain 访问](signing-and-keychain.md)

## 完整文件索引

由 `python3 scripts/py/check-docs.py --write` 更新。专题解释主要合同，表格覆盖当前全部文件。

<!-- crate-files:start -->

| 文件 | 阅读入口或源码职责 |
|---|---|
| [Cargo.toml](../../../crates/tools/Cargo.toml) | [独立助手版本与 macOS 条件依赖](overview.md#file-cargo-toml) |
| [src/bin/devdesk-credential-helper.rs](../../../crates/tools/src/bin/devdesk-credential-helper.rs) | [严格命名空间、签名调用者和单次 Keychain 操作](request-protocol.md#file-src-bin-devdesk-credential-helper-rs) |

<!-- crate-files:end -->

<a id="file-cargo-toml"></a>

## Cargo.toml：独立助手版本与 macOS 条件依赖

源码：[crates/tools/Cargo.toml](../../../crates/tools/Cargo.toml)。

包版本独立固定为 1.0.0，edition 复用 workspace。serde/serde_json 负责一次性 JSON 协议，security-framework 与 libc 仅在 macOS 引入，用于 Keychain 和签名父进程核验。

它不依赖 core/adapters，也不链接业务 Engine。非 macOS 构建保留受控 unsupported 返回，不能把“能编译”当作跨平台凭据能力。

关联阅读：[tools/src/bin/devdesk-credential-helper.rs](request-protocol.md#file-src-bin-devdesk-credential-helper-rs)、[adapters/src/credentials_macos.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
