# 一次请求的协议、校验与错误

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

助手每进程处理一个 JSON 请求并输出一行 JSON，不提供网络监听，也不接受任意 Keychain 查询。协议输入来自匹配签名的 Host；业务工具不会直接持有这个进程接口。

## 本页文件

- [src/bin/devdesk-credential-helper.rs](#file-src-bin-devdesk-credential-helper-rs)：严格命名空间、签名调用者和单次 Keychain 操作

<a id="file-src-bin-devdesk-credential-helper-rs"></a>

## src/bin/devdesk-credential-helper.rs：严格命名空间、签名调用者和单次 Keychain 操作

源码：[crates/tools/src/bin/devdesk-credential-helper.rs](../../../crates/tools/src/bin/devdesk-credential-helper.rs)。

Request 使用 deny_unknown_fields，字段为 operation、service、account 和可选 secret。service 必须为 devdesk. 加 64 位小写十六进制摘要；account 只接受 deepseek-api-key-、dsh-zai-coding-cn-api-key-、dsh-minimax-cn-api-key-、integration-key- 加 UUID 形状，不能指定任意系统账户。

操作仅 status/store/read/check/authorize。store 必须带 secret：模型密钥为 8..4096 字节可见 ASCII，集成秘密为 1..65536 字节且不含换行、回车和 NUL；其他操作不能夹带 secret。macOS 先核对实际父进程签名，再读取最多 393216 字节 stdin，超过上限或 JSON/语义不合法返回 invalid_params。

execute 对普通调用禁交互。status 返回 configured；store 写入 generic password 并返回 configured；read 返回 secret；check 与 authorize 实际读取并返回 authorized，只有后者允许系统 UI。每次 run 输出 `{ok:true,result:...}` 或 `{ok:false,code:...,message:...}`，main 写一行 JSON 后退出。

Keychain 无条目映射 credentials_required，需要交互/认证或用户取消映射 authentication_required，其他错误为 credential_store；输出使用固定说明，避免泄露系统详情。非 macOS 返回 unsupported。输入/密钥缓冲区尽量清零，限制及身份检查不能由调用方跳过。

包内测试核对账户白名单、外来 service、额外 secret 拒绝；macOS 另检查禁止交互 guard 的恢复行为。签名和安装两端的关系见 [签名与 Keychain](signing-and-keychain.md)。

代码定位：[`Request`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L12)、[`valid_account`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L19)、[`validate`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L41)、[`authorized_parent`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L61)、[`execute`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L81)、[`run`](../../../crates/tools/src/bin/devdesk-credential-helper.rs#L132)。

关联阅读：[adapters/src/credentials.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-rs)、[adapters/src/credentials_macos.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
