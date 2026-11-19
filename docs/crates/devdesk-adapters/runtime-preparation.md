# 运行环境准备与验证

运行准备将确定的 Harness/Sandbox 配置变成可使用的安装。它发生在会话执行前，准备成功只证明选定环境和协议满足要求。

## 从配置到镜像

[runtime_preparation.rs](../../../crates/adapters/src/runtime_preparation.rs) 的 `default_execution_recipe` 取得有效默认选择，`prepare_execution_recipe` 验证配方、保存准备状态、检查 Linux Docker Engine，并复用或构建相应安装。

配方由 Harness 程序安装输入与 Sandbox 基础环境组合。不同来源即使使用相同显示版本，也可能形成不同配方摘要。不能用“镜像名字相同”跳过完整性检查。

## 主要模块

| 文件 | 责任 |
|---|---|
| [runtime_images.rs](../../../crates/adapters/src/runtime_images.rs) | 按安装配方协调镜像准备和复用 |
| [runtime_registry.rs](../../../crates/adapters/src/runtime_registry.rs) | 登记实际安装及其状态 |
| [runtime_verification.rs](../../../crates/adapters/src/runtime_verification.rs) | 核对实际资源和协议验证结果 |
| [session_resources.rs](../../../crates/adapters/src/session_resources.rs) | SessionResourceAdapter 实现 core 的 SessionResources；RuntimePreparation 管理准备进度 |
| [model_providers.rs](../../../crates/adapters/src/model_providers.rs) | 解析服务连接并生成供应商 profile |
| [codex_auth.rs](../../../crates/adapters/src/codex_auth.rs) | Codex 本机认证准备 |

Host 通过这些服务展示进度和错误。读安装列表与主动安装是不同操作；读取不应自动下载、登录或替换运行资源。

## 兼容性怎样判断

检查需要考虑配方、实际镜像、驱动身份、权限资产和协议报告。已有会话绑定原安装及连接；技术重试不能换成最新驱动再声称仍是同一执行。

模型目录读取、凭据可读、安装协议通过、容器能连接模型以及模型正确执行工具，是不同验证层。错误消息应指出失败边界，避免把全部问题都归成“模型不可用”。

相关验证：[runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs)、[runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs)、[model_providers.rs](../../../crates/adapters/tests/model_providers.rs)、[runtime_preparation.py](../../../tests/entrypoints/runtime_preparation.py)。
