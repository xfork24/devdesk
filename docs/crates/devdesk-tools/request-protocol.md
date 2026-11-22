# 凭据助手的请求协议

实现位于 [devdesk-credential-helper.rs](../../../crates/tools/src/bin/devdesk-credential-helper.rs)。助手一次从 stdin 读取一个 JSON 请求，向 stdout 写一个 JSON 结果并换行。

## 请求结构

| 字段 | 要求 |
|---|---|
| operation | status、store、read、check 或 authorize |
| service | devdesk. 前缀加限定格式的数据集命名空间摘要 |
| account | 支持的服务商或接入凭据前缀，加合法 UUID |
| secret | 仅 store 接受；其他操作禁止携带 |

Request 使用 deny_unknown_fields，validate 检查命名空间、账户和内容限制。助手不是可以读取任意 Keychain 项目的通用接口。macOS 还会在读取请求前通过 authorized_parent 检查调用者签名。

## 操作差别

status 检查是否配置；store 保存秘密；read 返回秘密；check 检查能否读取但不返回秘密；authorize 允许显式交互授权。除 authorize 外禁用 Keychain 用户交互，避免后台轮询不断弹窗。

成功形状为 `{"ok":true,"result":...}`，失败形状为 `{"ok":false,"code":...}`。认证不足与凭据不存在分别使用 authentication_required 和 credentials_required 等代码。日志和错误不得包含秘密正文。

系统钥匙串失败可带 `os_status` 整数，Host 展示该状态和失败的操作。旧助手对有效的 `integration-key` 请求返回 `invalid_params` 时，Host 提示显式升级签名助手。签名校验、协议响应错误和钥匙串权限使用各自的诊断。

输入有大小上限；处理后主动清理持有秘密的可变缓冲区。非 macOS 构建返回 unsupported，不提供假凭据或明文回退。

## 验证

文件内测试覆盖允许的账户 namespace、服务格式、秘密参数和边界。Host 端协议与内容检查在 [credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs)。纯参数测试不证明签名父进程或真实 Keychain 已获授权，后者见[平台验证](signing-and-keychain.md)。
