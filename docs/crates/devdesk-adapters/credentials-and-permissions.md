# 凭据与权限后端

这些模块把应用选择的凭据引用和权限规则落实到操作系统及运行环境。它们不决定某个业务角色是否有调度权；那是 core 的应用授权。

## 凭据读取路径

[credentials.rs](../../../crates/adapters/src/credentials.rs) 提供 CredentialVault 等平台访问。[credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) 核对 Host 的代码签名和账户范围，并调用 macOS Security Framework。

配置保存不透明引用，适配器按供应商和数据集范围访问凭据。macOS 由 Host 访问钥匙串。Linux 使用 Secret Service。Codex OAuth 通过 [codex_auth.rs](../../../crates/adapters/src/codex_auth.rs) 管理数据集私有认证材料。

显式授权独占进程内的钥匙串调用，后台读写期间关闭授权弹窗。需要用户确认时，读写操作返回错误，用户可从设置页或命令行发起[授权](signing-and-keychain.md)。

## 权限编译路径

[permission_profiles.rs](../../../crates/adapters/src/permission_profiles.rs) 处理 Profile 与后端合同，[permissions.rs](../../../crates/adapters/src/permissions.rs) 生成实际执行约束。权限资产来自 [permissions](../../../permissions/README.md)，安装和校验还涉及运行准备模块。

输入是固定 Profile、目标运行和上下文；输出包括实际可执行的策略、挂载/工具限制及校验结果。若当前平台不能完整实施选定规则，应拒绝执行，不能删除不支持部分后继续。

Harness 权限限制程序和原生工具，Sandbox 权限限制容器边界。替换其中一层不表示放开另一层。受限 DSH 等运行可能需要嵌套后端；Linux AppArmor 资产必须经过相应安装和摘要检查。

## 常见修改风险

新增服务商凭据需同步核对账户命名空间、允许的钥匙串账户、凭据版本检查及授权入口。

新增文件权限需同时检查路径规范化、符号链接、读写挂载和实际工具后端。UI 显示“只读”不能替代容器和程序实际的只读限制。

验证入口见 [permissions.rs](../../../crates/adapters/tests/permissions.rs)、[permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs)、[credential_updates.rs](../../../crates/adapters/tests/credential_updates.rs) 和 [credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs)。
