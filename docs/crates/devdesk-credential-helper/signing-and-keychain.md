# 父进程签名、固定安装与 Keychain 访问

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

先读 [请求实现](request-protocol.md#file-src-main-rs)，再按本页连接 Host 侧安装与系统行为。Host 侧使用的 helper manifest 协议版本是 2，与本包 Cargo 版本 1.0.0 分别管理。

## 两边各自核对身份

Host 侧 [credentials_macos](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs) 核对固定目录的用户所有权、权限、非符号链接、manifest 和内容 SHA，再核对 helper identifier `dev.devdesk.credentials` 与签名证书。helper 侧按 getppid 取得实际父进程，要求 identifier `dev.devdesk.host` 且 leaf certificate 等于编译时 DEVDESK_ALLOWED_HOST_CERTIFICATE；缺失或非法的证书 pin 直接拒绝，不能退回仅检查进程名。

构建/打包入口参见 [平台脚本目录](../../../scripts/platform)，操作说明见 [系统凭据说明](../../system-credentials.md)。固定助手由显式升级动作替换，普通 Host 重建不会顺手覆盖。更新后需要授权时，通过设置中的 authorize 操作完成。

## 交互和秘密的生命周期

除 authorize 外，execute 通过 SecKeychain 的 RAII guard 禁止交互，返回时恢复原状态。status 只查属性是否存在；check 实际读取但不返回密钥；read 才把值通过私有管道交回 Host。store 和读取后的缓冲区在使用后清零；JSON/字符串仍经过进程内分配，因此这不是对内存所有副本的绝对清零保证。

无匹配条目、需要交互和其他系统拒绝映射为受控错误，普通轮询不会因系统对话框阻塞。Linux 不执行此助手，凭据由 [CredentialVault](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-rs) 的平台实现处理。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
