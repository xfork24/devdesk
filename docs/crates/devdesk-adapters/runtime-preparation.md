# 运行安装、准备意图与协议验证

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

运行资源描述不可变配方和镜像身份。准备过程可以下载、构建和注册安装，协议验证则检查已安装对象是否具备当前驱动所需能力；这两步都不等于模型推理已通过。

## 本页文件

- [src/runtime_registry.rs](#file-src-runtime-registry-rs)：注册不可变安装并发现内容漂移
- [src/runtime_preparation.rs](#file-src-runtime-preparation-rs)：按冻结配方准备镜像与认证组件
- [src/runtime_verification.rs](#file-src-runtime-verification-rs)：按实际驱动与权限资产验证原生协议
- [src/session_resources.rs](#file-src-session-resources-rs)：把运行准备、凭据和权限接成会话资源端口

<a id="file-src-runtime-registry-rs"></a>

## src/runtime_registry.rs：注册不可变安装并发现内容漂移

源码：[crates/adapters/src/runtime_registry.rs](../../../crates/adapters/src/runtime_registry.rs)。

注册原生程序时检查规范化普通文件、声明的平台/架构、实际 SHA 与可执行内容，拒绝启动脚本冒充程序；Docker 安装记录本地不可变 image ID，OCI 引用要求固定 digest；登记形状合法后，还需 runtime_integrity 核对实际可用对象。发行标签和版本字符串仅是元数据，不能替代内容核验。

相同 provider/version/platform/recipe 键下，相同内容从不同路径或并发注册应去重，不同内容则报 build identity 冲突；完整性查询再次检测文件/镜像漂移。runtime_integrity 只证明安装对象仍匹配；是否支持当前协议另由 runtime_verification 判断。

Codex 认证组件与容器工作运行时分别发现和记录。发现本机认证程序不代表已经登录、容器具备模型连接或推理成功。

代码定位：[`configuration_digest`](../../../crates/adapters/src/runtime_registry.rs#L6)、[`file_digest`](../../../crates/adapters/src/runtime_registry.rs#L13)、[`checked_artifact`](../../../crates/adapters/src/runtime_registry.rs#L36)、[`read_installation`](../../../crates/adapters/src/runtime_registry.rs#L139)、[`discover_docker_runtime`](../../../crates/adapters/src/runtime_registry.rs#L145)。

关联阅读：[domain/src/agents.rs](../devdesk-domain/agents-and-conversations.md#file-src-agents-rs)、[adapters/src/runtime_verification.rs](runtime-preparation.md#file-src-runtime-verification-rs)、[adapters/tests/runtime_registry.rs](testing.md#file-tests-runtime-registry-rs)。

<a id="file-src-runtime-preparation-rs"></a>

## src/runtime_preparation.rs：按冻结配方准备镜像与认证组件

源码：[crates/adapters/src/runtime_preparation.rs](../../../crates/adapters/src/runtime_preparation.rs)。

默认版本、recipe 和下载信息来自内嵌 runtime_resource。准备先核对不可变资源与 Linux Docker daemon；已有安装只有经实际内容核对才复用。否则按 digest 拉取镜像，或用受控文件集合建立私有构建上下文，再检查实际 image ID、注册安装并验证协议。

准备过程保存 intent、阶段 checkpoint 与结果。完成镜像加上 devdesk/prepared-* 保留标签，供同一内容后续复用；标签本身不作为内容身份。断点或取消交由恢复路径核对，不因客户端断开就认定构建没有发生。

Codex 登录用的原生组件单独准备：按系统/架构选 HTTPS 发行文件，核对下载摘要和压缩包成员，提取到数据集私有运行目录。它不修改用户全局 CODEX_HOME，也不把登录组件当作 Linux worker 镜像。

代码定位：[`default_version`](../../../crates/adapters/src/runtime_preparation.rs#L6)、[`packaged_recipe`](../../../crates/adapters/src/runtime_preparation.rs#L9)、[`default_runtime_resource`](../../../crates/adapters/src/runtime_preparation.rs#L25)、[`runtime_directory`](../../../crates/adapters/src/runtime_preparation.rs#L190)。

关联阅读：[adapters/src/runtime_registry.rs](runtime-preparation.md#file-src-runtime-registry-rs)、[adapters/src/runtime_verification.rs](runtime-preparation.md#file-src-runtime-verification-rs)、[adapters/src/session_resources.rs](runtime-preparation.md#file-src-session-resources-rs)、[adapters/src/codex_auth.rs](credentials-and-permissions.md#file-src-codex-auth-rs)。

<a id="file-src-runtime-verification-rs"></a>

## src/runtime_verification.rs：按实际驱动与权限资产验证原生协议

源码：[crates/adapters/src/runtime_verification.rs](../../../crates/adapters/src/runtime_verification.rs)。

验证键覆盖实际安装内容、recipe、受管驱动身份和权限资产。使用前同时要求安装完整性和匹配的已验证报告；仅把数据库字段改成 verified 无法替代真实对象。

协议探针在无模型、禁网、只读的短生命周期容器里运行。Codex 检查 initialize、thread start 与动态工具注册；DSH 检查自身初始化协议。报告明确保留 inference/tool 实际验证范围，初始化通过不宣称完成了模型工具调用。

文件也提供准备意图的查询、取消、checkpoint、恢复与 reconciliation。重启后遇到未知结果要检查原容器/安装，而不是直接再建一套资源。

代码定位：[`verification_key`](../../../crates/adapters/src/runtime_verification.rs#L13)、[`runtime_verification`](../../../crates/adapters/src/runtime_verification.rs#L18)、[`require_runtime_verification`](../../../crates/adapters/src/runtime_verification.rs#L27)、[`verify_runtime_protocol`](../../../crates/adapters/src/runtime_verification.rs#L38)。

关联阅读：[adapters/src/runtime_preparation.rs](runtime-preparation.md#file-src-runtime-preparation-rs)、[adapters/src/harness.rs](harness-and-protocols.md#file-src-harness-rs)、[adapters/tests/runtime_resources.rs](testing.md#file-tests-runtime-resources-rs)。

<a id="file-src-session-resources-rs"></a>

## src/session_resources.rs：把运行准备、凭据和权限接成会话资源端口

源码：[crates/adapters/src/session_resources.rs](../../../crates/adapters/src/session_resources.rs)。

RuntimePreparation 为 Codex、DSH 和认证组件分别管理互斥准备任务与进度。下载/构建期间不长时间持有工程事务或当前项目选择锁，因此别的项目仍可响应请求和调度。

SessionResourceAdapter 实现 core 的 SessionResources，将 GlobalStore、Harness、权限资产、CredentialVault 和模型连接串起来。它检查实际安装和协议报告，编译冻结权限，提供受管 adapter 身份与不透明凭据引用；core 再固定完整 Session 选择。

“已有默认”“已安装”“已授权”“可完成推理”是不同状态。返回资源状态时应保留区别，不能为了让按钮可用而合并为一个 ready 标志。

代码定位：[`RuntimePreparation`](../../../crates/adapters/src/session_resources.rs#L7)、[`SessionResourceAdapter`](../../../crates/adapters/src/session_resources.rs#L41)、[`run`](../../../crates/adapters/src/session_resources.rs#L14)、[`status`](../../../crates/adapters/src/session_resources.rs#L36)、[`prepare_auth`](../../../crates/adapters/src/session_resources.rs#L47)。

关联阅读：[core/src/session_assembly.rs](../devdesk-core/session-resources-and-permissions.md#file-src-session-assembly-rs)、[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)、[host/src/runtime_setup.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-runtime-setup-rs)、[adapters/src/credentials.rs](credentials-and-permissions.md#file-src-credentials-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
