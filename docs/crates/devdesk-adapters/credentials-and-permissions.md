# 凭据、登录与权限落地

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

密钥值保留在系统凭据库或私有认证目录，冻结计划使用不透明引用。权限源经过实际后端编译后成为可执行合同；声明一个授权名称，不能让不支持该策略的后端自动获得能力。

## 本页文件

- [src/credentials.rs](#file-src-credentials-rs)：数据集隔离的系统密钥引用
- [src/credentials_macos.rs](#file-src-credentials-macos-rs)：固定助手的安全安装、校验和调用
- [src/codex_auth.rs](#file-src-codex-auth-rs)：数据集私有的 Codex 登录生命周期
- [src/permission_profiles.rs](#file-src-permission-profiles-rs)：权限预设导入、编译与内容身份
- [src/permissions.rs](#file-src-permissions-rs)：把可移植授权变成 Codex/DSH 和容器策略

<a id="file-src-credentials-rs"></a>

## src/credentials.rs：数据集隔离的系统密钥引用

源码：[crates/adapters/src/credentials.rs](../../../crates/adapters/src/credentials.rs)。

CredentialVault 用规范化数据集根计算 service 命名空间；模型账户前缀来自已发布 ProviderFacts 的 credential_account，再加不透明 UUID，集成账户则使用 integration-key-。模型密钥与集成秘密分开；数据库及冻结会话只保存引用，不保存密钥值。模型密钥轮换会产生新引用，旧会话继续固定旧引用，避免新设置悄悄改变已启动执行。OAuth token 刷新可更新同一集成秘密引用，不能把模型轮换规则套到所有秘密上。

macOS 委托固定签名助手，Linux 使用 secret-tool 并经私有 stdin 传值。status 只描述配置事实，check 检查实际读取能力，authorize 才是显式交互授权。普通轮询不应弹系统授权窗口；Ollama 是无需该 API key 的独立路由。

模型引用读取核对当前数据集 service、UUID 形状和冻结 provider/connection 的账户前缀；集成引用核对 integration-secret:UUID。macOS 助手另有明确账户前缀白名单，新增 provider 不能仅靠配置绕过它。错误返回不附带原始密钥或任意系统凭据详情。调用者仍须先完成项目、工具与账户授权，知道一个引用并不代表有权使用。

代码定位：[`CredentialVault`](../../../crates/adapters/src/credentials.rs#L15)、[`CredentialStatus`](../../../crates/adapters/src/credentials.rs#L5)、[`integration_account`](../../../crates/adapters/src/credentials.rs#L35)、[`configure_integration`](../../../crates/adapters/src/credentials.rs#L42)。

关联阅读：[adapters/src/credentials_macos.rs](credentials-and-permissions.md#file-src-credentials-macos-rs)、[credential-helper/src/main.rs](../devdesk-credential-helper/request-protocol.md#file-src-main-rs)、[adapters/src/integration_oauth.rs](external-integrations.md#file-src-integration-oauth-rs)、[host/src/model_providers.rs](../devdesk-host/rpc-and-configuration.md#file-src-model-providers-rs)。

<a id="file-src-credentials-macos-rs"></a>

## src/credentials_macos.rs：固定助手的安全安装、校验和调用

源码：[crates/adapters/src/credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs)。

助手安装在当前用户 Library/Application Support/DevDesk/credential-helper/v1。路径需为用户所有、权限安全的真实目录/文件，拒绝符号链接。manifest 的协议版本为 2，并校验实际内容摘要、helper 标识和与 Host 相同的证书。

初次缺失时从 Host 附近的已签名 bundle 安全发布；并发安装不能覆盖现有对象。助手更新是显式动作，不通过读取 Keychain 偷偷触发。普通 Host 重建不会改写固定助手，从而保留系统授权所绑定的稳定主体。

每次调用通过 stdin 发送一份 JSON，经受管进程获取脱敏结果；普通调用 8 秒，显式 authorize 30 秒。助手内部还独立核验父进程签名，因此“能运行这个二进制”不等于可以读它管理的密钥。

代码定位：[`verify`](../../../crates/adapters/src/credentials_macos.rs#L8)、[`verify_manifest`](../../../crates/adapters/src/credentials_macos.rs#L58)、[`directory`](../../../crates/adapters/src/credentials_macos.rs#L77)、[`helper_path`](../../../crates/adapters/src/credentials_macos.rs#L91)、[`update_bundled_helper`](../../../crates/adapters/src/credentials_macos.rs#L154)。

关联阅读：[credential-helper/src/main.rs](../devdesk-credential-helper/request-protocol.md#file-src-main-rs)、[adapters/src/credentials.rs](credentials-and-permissions.md#file-src-credentials-rs)、[adapters/src/process.rs](harness-and-protocols.md#file-src-process-rs)。

<a id="file-src-codex-auth-rs"></a>

## src/codex_auth.rs：数据集私有的 Codex 登录生命周期

源码：[crates/adapters/src/codex_auth.rs](../../../crates/adapters/src/codex_auth.rs)。

CodexAuthentication 持有共享 AuthWorker，处理登录开始、状态、取消、凭据引用和模型目录。重复开始同一个待完成登录复用现有操作；worker 结束后读取真实结果，失败重试创建清晰的新尝试，不把旧状态假装成成功。

认证组件与数据集私有目录配套，避免复用或修改用户全局 CODEX_HOME。登录得到的引用可供冻结会话采用，但登录程序本身不是执行工作任务的容器，也不证明 Linux worker 能访问模型服务。

并发与取消围绕实际进程管理，状态查询不发起新的推理。Host 把进度暴露给界面，正式 Session 仍需安装、协议、权限和完整绑定的共同校验。

代码定位：[`CodexAuthentication`](../../../crates/adapters/src/codex_auth.rs#L79)、[`AuthWorker`](../../../crates/adapters/src/codex_auth.rs#L7)、[`start_login`](../../../crates/adapters/src/codex_auth.rs#L47)、[`credential_reference`](../../../crates/adapters/src/codex_auth.rs#L107)、[`models`](../../../crates/adapters/src/codex_auth.rs#L156)。

关联阅读：[adapters/src/session_resources.rs](runtime-preparation.md#file-src-session-resources-rs)、[adapters/src/runtime_preparation.rs](runtime-preparation.md#file-src-runtime-preparation-rs)、[host/src/runtime_setup.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-runtime-setup-rs)。

<a id="file-src-permission-profiles-rs"></a>

## src/permission_profiles.rs：权限预设导入、编译与内容身份

源码：[crates/adapters/src/permission_profiles.rs](../../../crates/adapters/src/permission_profiles.rs)。

此模块将可移植 Profile 与实际后端合同组合，先 preflight 再 compile，计算权限语义摘要和 enforcement contract。实际运行安装、recipe 和权限资产参与校验，不能只靠 Profile 名称判断已落实。

权限目录只有一个持久化位置。发行包附近的预设优先使用，缺失时可由内嵌资源恢复到数据集的内容寻址目录；恢复只补齐不可变资源，不自动替用户改变绑定。导入/导出与目录 reload 保留精确内容身份，避免同名文件悄悄替换冻结选择。

core 管谁可以修改哪个绑定和何时建立后继会话；这里负责策略可行性和实际文件安装。修改权限资产时需同时更新内容清单与相应的真实执行测试。

代码定位：[`PermissionPresetSource`](../../../crates/adapters/src/permission_profiles.rs#L298)、[`permission_contract`](../../../crates/adapters/src/permission_profiles.rs#L4)、[`permission_semantics_digest`](../../../crates/adapters/src/permission_profiles.rs#L45)、[`compile_permission_profiles`](../../../crates/adapters/src/permission_profiles.rs#L66)、[`preflight_profile`](../../../crates/adapters/src/permission_profiles.rs#L166)、[`verify_permission_installation`](../../../crates/adapters/src/permission_profiles.rs#L267)。

关联阅读：[adapters/src/permissions.rs](credentials-and-permissions.md#file-src-permissions-rs)、[core/src/permission_resources.rs](../devdesk-core/session-resources-and-permissions.md#file-src-permission-resources-rs)、[host/src/permission_resources.rs](../devdesk-host/rpc-and-configuration.md#file-src-permission-resources-rs)、[adapters/tests/permission_profiles.rs](testing.md#file-tests-permission-profiles-rs)。

<a id="file-src-permissions-rs"></a>

## src/permissions.rs：把可移植授权变成 Codex/DSH 和容器策略

源码：[crates/adapters/src/permissions.rs](../../../crates/adapters/src/permissions.rs)。

capabilities 根据实际 recipe 和受管资产描述后端能落实的权限。compile_resolved_permissions 将冻结 grant 编译为 Codex sandbox/config、DSH profile/tool 边界和 Docker 参数；工作目录、状态、Skill 与网络各有对应限制。

full、read-only、scoped 需要不同机制。后端无法表达的策略（例如不具备相应隔离机制的网络白名单）应明确拒绝，不能宽松降级。嵌套沙箱只能收窄外层容器授权，不能扩大挂载可写范围。

validate_runtime_version 的名字保留历史叫法，实际检查 recipe/内容身份，并非按发行版本字符串决定兼容。权限资产通过清单与摘要核对，已冻结计划再次验证实际安装和编译结果，防止文件变化绕开原授权。

代码定位：[`capabilities`](../../../crates/adapters/src/permissions.rs#L5)、[`asset_digests`](../../../crates/adapters/src/permissions.rs#L39)、[`compile_resolved_permissions`](../../../crates/adapters/src/permissions.rs#L70)、[`validate_execution_permissions`](../../../crates/adapters/src/permissions.rs#L166)、[`codex_sandbox_policy`](../../../crates/adapters/src/permissions.rs#L290)。

关联阅读：[domain/src/permission.rs](../devdesk-domain/runtime-and-permissions.md#file-src-permission-rs)、[adapters/src/docker.rs](engineering-resources.md#file-src-docker-rs)、[adapters/src/permission_profiles.rs](credentials-and-permissions.md#file-src-permission-profiles-rs)、[adapters/tests/permissions.rs](testing.md#file-tests-permissions-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
