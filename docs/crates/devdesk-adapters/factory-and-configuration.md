# 出厂资源嵌入与配置后端

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

资源从 manifest 管理的配置包进入二进制，不依靠运行时存在源码 checkout。加载器提供资源，core 决定怎样发布和采用；外层配置后端只提供实际可行性与资源事实。

## 本页文件

- [build.rs](#file-build-rs)：编译时生成资源表与 Schema 校验器
- [src/factory_bundle.rs](#file-src-factory-bundle-rs)：读取嵌入配置并一次性注入 core
- [src/configuration_backend.rs](#file-src-configuration-backend-rs)：为配置预演提供实际后端事实

<a id="file-build-rs"></a>

## build.rs：编译时生成资源表与 Schema 校验器

源码：[crates/adapters/build.rs](../../../crates/adapters/build.rs)。

构建脚本先读取 manifest，校验 template-manifest/source-definition Schema，并核对每个声明源的 type 与分类相符；随后递归收集 resources/configuration 下的 JSON/Markdown，排序生成 OUT_DIR/template_files.rs，以 include_str 嵌入二进制。被嵌入不等于被发布：运行 loader 仍按 manifest.types 和 defaults 选择资源。资源变化通过 Cargo 重建提示进入产物，运行时无需读取开发机 checkout。

构建期校验器复用 scripts/rust/schema_validation.rs；它与包内的离线 Schema 功能都不抓取远端 Schema。新增出厂源应进入 manifest 和对应合同，单纯放进目录不代表已成为默认资源。

代码定位：[`collect`](../../../crates/adapters/build.rs#L7)、[`main`](../../../crates/adapters/build.rs#L20)。

关联阅读：[adapters/src/factory_bundle.rs](factory-and-configuration.md#file-src-factory-bundle-rs)、[adapters/tests/schema_contracts.rs](testing.md#file-tests-schema-contracts-rs)。

<a id="file-src-factory-bundle-rs"></a>

## src/factory_bundle.rs：读取嵌入配置并一次性注入 core

源码：[crates/adapters/src/factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs)。

TEMPLATE_FILES 来自 build.rs。资源路径仅接受安全的普通相对路径；loader 读取 manifest、源包装与 body_file 等受控正文引用，构建 ConfigurationBundle、默认映射和内容身份。

`initialize` 通过 OnceLock 安装 bundle，同时注册当前实际 Harness 驱动能力。core 后续从注入资源读取默认，不在代码中按业务名字硬编码一套源。packaged_runtime 与 packaged_permission_presets 为运行准备和权限目录恢复提供同一发行包资源。

加载器校验“包里是什么”，配置用例校验“当前允许怎样采用”。目录恢复不会让旧运行改用这份包的新默认，冻结引用仍保留自己的修订。

代码定位：[`resource`](../../../crates/adapters/src/factory_bundle.rs#L5)、[`initialize`](../../../crates/adapters/src/factory_bundle.rs#L19)、[`load`](../../../crates/adapters/src/factory_bundle.rs#L29)、[`packaged_runtime`](../../../crates/adapters/src/factory_bundle.rs#L111)、[`packaged_permission_presets`](../../../crates/adapters/src/factory_bundle.rs#L135)。

关联阅读：[core/src/templates.rs](../devdesk-core/configuration-and-catalog.md#file-src-templates-rs)、[core/src/driver_capabilities.rs](../devdesk-core/session-resources-and-permissions.md#file-src-driver-capabilities-rs)、[adapters/src/runtime_preparation.rs](runtime-preparation.md#file-src-runtime-preparation-rs)。

<a id="file-src-configuration-backend-rs"></a>

## src/configuration_backend.rs：为配置预演提供实际后端事实

源码：[crates/adapters/src/configuration_backend.rs](../../../crates/adapters/src/configuration_backend.rs)。

ConfigurationBackend 实现 core 的配置后端：权限绑定预演先检查 Profile 能否针对实际 Harness/执行安装编译，再计算 enforcement contract。这个准备阶段不启动工作，也不借预演给调用者提权。

GlobalStore 的 ProjectInventory 实现汇集安装、协议报告与权限资产事实，并分页读取工程只读快照。它向 core 提供确切事实而非“环境大概可用”的布尔猜测；跨库发布、经理工作台和会话装配据此进行各自授权与提交校验。

代码定位：[`ConfigurationBackend`](../../../crates/adapters/src/configuration_backend.rs#L2)、[`prepare_permission_binding`](../../../crates/adapters/src/configuration_backend.rs#L4)、[`runtime_facts`](../../../crates/adapters/src/configuration_backend.rs#L18)。

关联阅读：[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)、[adapters/src/permission_profiles.rs](credentials-and-permissions.md#file-src-permission-profiles-rs)、[core/src/configuration_changes.rs](../devdesk-core/configuration-and-catalog.md#file-src-configuration-changes-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)
