# S1 执行与阶段验收

日期：2026-11-13。结论：本阶段针对性验收通过，可进入 S2。测试边界遵循上层 EXECUTION.md 的用户追加要求，未运行全量 Feature/Task 开发流程。

## 实现

- core 的出厂资源 build.rs 已删除；资源构建校验、清单/Markdown 解析在 adapters，外层注入 ConfigurationBundle。选择在 adapters 打包而非增加 Host 业务 loader，保持库式集成/测试的装配入口一致。生产 schema 支持代码位于 scripts/rust，测试通过薄包装复用，不再由生产构建导入 tests 实现。
- Store/Artifact 与执行端口归集到 core/ports。domain 不再生成 UUID，应用 ID/时钟有可注入的作用域来源；异常退出恢复原来源。
- Session/Agent 组合、冻结解析、默认权限、经验导入、目录同步、目录重置和物化回执迁入 core 应用用例。实际安装、凭据、权限编译与完整性检查经 SessionResources 在 adapters 实现；Host 只路由和装配。
- Workflow 计划与任务的纯重建在 core/frozen_views。adapters/source_views 保留存储描述的编解码和后端纯派生，已无直接工作流编译或任务渲染。没有引入新的 SQLite 基线或双格式兼容路径，冻结选择与原 codec 往返等价。
- RPC 方法分类移出 domain；InvocationContext 由可信调用入口创建、不能由 RPC 反序列化。命令及原生 Controller 操作记录实际主体/请求指纹，人工决定仍受原控制内核限制。
- 增加 check-architecture 门禁并接入 check.sh：检查本仓库 crate 依赖、domain 环境访问、core 出厂构建脚本、Adapter 应用编译和 Host 业务集合直接写入。静态检查配合本阶段职责复核使用，不把正则检查当作完整架构证明。

## 验收

| 检查 | 结果 |
|---|---|
| 复制 domain/core、workspace manifest/lock/toolchain 至临时目录，不提供 resources/schemas/tests/adapters；离线构建 core | 通过；临时构建必须携带仓库 rust-toolchain，系统默认旧工具链不能构建锁定依赖 |
| domain/core 单元与 doc tests | 20 项单元通过（domain 17、core 3），doc tests 无用例 |
| configuration_resources | 14 通过 |
| governance | 5 通过 |
| schema_contracts | 12 通过 |
| adapters source_views codec 局部单元 | 4 通过 |
| standard_snapshots::project_execution_selections_rebuild_in_their_own_access_context | 1 通过 |
| permission_profiles::profile_revisions_preserve_the_source_owner_scope | 1 通过 |
| Host templates::tests | 4 通过，包含目录重置、历史绑定、幂等与过期预演 |
| Web TypeScript/Vite build | 通过 |
| check-defaults、check-architecture、cargo fmt、git diff --check | 通过 |

合计 61 项上述 Rust 针对性测试通过。测试使用专用临时数据及原有可控端口，不运行真实模型开发流程。

验收中修复了：未有 Project 记录的合法初始化/全局讨论被新 Actor 归属检查误拒；直接构造状态的测试漏注入外层默认资源。最终检查均使用修复后代码；受影响的治理、schema、Host 和单元检查已重跑。

## 实施边界

冻结后端权限与模型 profile 的纯生成仍在 Adapter codec，业务图/任务解释在 core。S3 将进一步拆开运行资源与能力事实；本阶段没有通过删掉内容核验来绕过该问题。

没有运行全量 check.sh、全量 Feature/Task 流程或真实模型/容器工程验收；这些不作为本阶段已验证结果。日志在临时目录，不提交。
