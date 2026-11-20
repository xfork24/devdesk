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

## 运行依赖与 Git 工作树

源码仅位于容器 `/work`。运行时依赖使用独立的宿主机目录，通过 Docker bind volume 挂载到 `/tmp/devdesk-runtime`，不受普通 `/tmp` 的小容量、禁止执行挂载限制。`DEVDESK_RUNTIME_DIR` 指向当前会话的依赖安装、虚拟环境、配置和临时文件目录。

依赖缓存默认位于项目执行根下的 `runtime-cache/<project-id>`，同一项目的容器可以复用；跨项目共享默认关闭。`downloads` 保存项目下载缓存，`images/<image digest>` 保存同一确切镜像兼容的浏览器和已安装工具。会话私有安装环境、配置、凭据、构建输出和临时文件位于各自的 `state/runtime`，移除容器不会删除项目缓存。

只有显式设置 Host 的 `DEVDESK_SHARED_DEPENDENCY_CACHE=/absolute/path` 才跨项目共享下载与工具缓存；这与现有镜像安装根目录 `DEVDESK_RUNTIME_CACHE` 独立。容器内 `DEVDESK_RUNTIME_CACHE` 指向已挂载的工具目录。Cargo 只复用 registry/git 缓存，保留私有 `CARGO_HOME` 配置；模型只复用下载缓存，保留私有 `HF_HOME` 凭据。

每次创建业务容器前，检查宿主机可用空间，再通过临时、无网络容器，以真实运行 UID 验证每个运行时挂载的可用空间、创建/写入和执行权限。默认要求至少 4 GiB 空闲，`DEVDESK_RUNTIME_MIN_FREE_BYTES` 可提高下限。失败返回具体的空间、权限或挂载错误，并标记业务操作尚未执行；不使用 `chmod 777` 兜底。检查证明启动时可用，不代表磁盘空间在整个任务期间被独占预留。

平台配置 Node/npm、Python/pip/uv、Rust/Cargo、Go、Java/Maven/Gradle、Ruby、NuGet 和模型下载缓存等目录，并将目录约束放入 Agent 的冻结运行上下文。npm 的本地安装仍须使用明确的外部 prefix；只设置下载缓存不能阻止包管理器创建工作区内的安装目录。依赖清单和锁文件、代码、测试及最终验收证据可以进入 Git，下载包、安装环境及构建临时目录不作为证据入库。

运行上下文同时要求优先复用项目已有测试入口，区分正式回归代码与实现者不可改写的独立流程断言。浏览器及服务检查须设置超时，并在失败路径清理页面、浏览器和子进程，关闭上一轮后再启动下一轮；成功验收仍须保留实际命令观测和源码身份。运行目录隔离不替代测试程序自身的资源回收。

快照遵循仓库 `.gitignore`，并为常见运行目录提供低优先级 `info/exclude` 规则。为拦截未知目录中的整套运行环境，提交前检查未跟踪文件：单个新增文件超过 8 MiB 或未声明新增文件合计超过 64 MiB 时，保留现场、拒绝提交并给出具体路径；业务确需的大资产可在 `allowed_paths` 中明确列出。此检查发生在暂存和提交之前，失败被标为未应用，不会伪装成未知 Git 副作用。

大快照首先读取提交间的完整文件/对象清单，仅限量展开小文件补丁；未展开内容带有确切路径与对象身份，明确标记 `patch_complete: false`，不会作为完整审阅证据。正常大小的差异继续返回完整补丁，核对只读取原快照回执，不重新执行原写操作。
