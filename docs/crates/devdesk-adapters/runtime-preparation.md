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

Host 通过这些服务展示进度和错误。读安装列表与主动安装是不同操作；读取不应自动下载、登录或替换运行资源。Pi 准备会登记所选 Harness 修订的物化回执。断网启动探针用 `get_state` 核对 RPC 会话身份，无需模型凭据。

## 兼容性怎样判断

检查需要考虑配方、实际镜像、驱动身份、权限资产和协议报告。已有会话绑定原安装及连接；技术重试不能换成最新驱动再声称仍是同一执行。

模型目录读取、凭据可读、安装协议通过、容器能连接模型以及模型正确执行工具，是不同验证层。错误消息应指出失败边界，避免把全部问题都归成“模型不可用”。

相关验证：[runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs)、[runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs)、[model_providers.rs](../../../crates/adapters/tests/model_providers.rs)、[runtime_preparation.py](../../../tests/entrypoints/runtime_preparation.py)。

## 运行依赖与 Git 工作树

源码仅位于容器 `/work`。运行时依赖使用独立的宿主机目录，通过 Docker bind volume 挂载到 `/tmp/devdesk-runtime`，不受普通 `/tmp` 的小容量、禁止执行挂载限制。`DEVDESK_RUNTIME_DIR` 指向当前会话的依赖安装、虚拟环境、配置和临时文件目录。

同一个数据集默认使用 `data/shared-cache/runtime`。该目录的 `downloads` 保存外部依赖下载缓存，`images/<image digest>` 保存同一固定镜像可复用的浏览器、字体和已安装工具。Host 在项目没有遗留 Docker 容器时迁入旧项目缓存。合并时保留发生冲突的旧文件。Host 的 `DEVDESK_SHARED_DEPENDENCY_CACHE=/absolute/path` 可显式选择另一个位置，容器内的 `DEVDESK_RUNTIME_CACHE` 指向已挂载的工具目录。镜像安装根仍由 Host 的 `DEVDESK_RUNTIME_CACHE` 单独配置。

会话私有安装环境、配置、凭据和构建输出留在各自的 `state/runtime`。worker 释放后清理临时目录和只读 Git 导出。测试完成或停止的原始回执入库后，清理测试容器、源码导出和临时文件。

`data/shared-cache/codex` 按文件内容和权限保存 Codex 插件、Apps 缓存、系统 Skills 及会话安装的 Node 依赖。闲置会话在协调器目录保存索引，启动时按索引生成私有的可写文件。worker 释放后写入索引，再清理已入库的会话缓存副本。远程插件目录按凭据作用域和 Codex 缓存键保留最新快照。模型目录按凭据、Codex 版本和模型目录身份保留最新快照，继续使用来源的刷新时间和 ETag。

会话线程、权限和凭据保存在独立目录。Host 打开数据集时执行 SQLite WAL checkpoint，也会整理已释放会话的 Codex 数据库。原始运行协议日志封存并入库后，Host 核对原日志和封存产物的 SHA-256，再清理重复运行副本。历史日志从封存产物读取。
