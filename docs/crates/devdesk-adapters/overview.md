# devdesk-adapters：把端口落实为可核对的外部操作

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)

adapters 实现 core 的端口：把工程状态写入 SQLite，把交付物封存成文件，把冻结任务交给真实容器和 Harness，并处理 Git、GitHub、系统凭据和外部工具。它负责证明“外部实际发生了什么”；是否接纳为业务成功，仍由 core 决定。

包里既有生产实现，也有跨层集成测试。测试可以注入 Harness/工具夹具来精确制造时序，但生产装配 [execution_adapters](storage-and-source-codecs.md#file-src-lib-rs) 使用实际执行器。测试目录中的普通离线测试、真实本地 Git 测试、显式 ignored 的 Docker/GitHub 测试应分开理解。

实现上的共同约束是：固定归属与内容身份，在操作前保存意图，在事务外执行，保留原操作回执；结果未知时核对已有资源，不换 ID 重试。存储、容器、原生协议各自有不同的“停止”含义，不能把杀掉一个客户端进程当作所有远端副作用已撤销。

## 专题阅读

- [SQLite、Artifact 与源视图恢复](storage-and-source-codecs.md)
- [出厂资源嵌入与配置后端](factory-and-configuration.md)
- [运行安装、准备意图与协议验证](runtime-preparation.md)
- [Harness、JSONL 与受管进程](harness-and-protocols.md)
- [凭据、登录与权限落地](credentials-and-permissions.md)
- [Git、GitHub、Docker 与测试执行](engineering-resources.md)
- [模型路由、外部工具与 OAuth](external-integrations.md)
- [集成测试的场景、夹具与真实边界](testing.md)

## 完整文件索引

每个文件只有一个主要讲解位置；相关专题通过链接引用它。

| 文件 | 职责与实现说明 |
|---|---|
| [Cargo.toml](../../../crates/adapters/Cargo.toml) | [外部实现所需依赖和测试边界](overview.md#file-cargo-toml) |
| [build.rs](../../../crates/adapters/build.rs) | [编译时生成资源表与 Schema 校验器](factory-and-configuration.md#file-build-rs) |
| [src/catalog.rs](../../../crates/adapters/src/catalog.rs) | [全局配置、项目目录与运行设置](storage-and-source-codecs.md#file-src-catalog-rs) |
| [src/codex_auth.rs](../../../crates/adapters/src/codex_auth.rs) | [数据集私有的 Codex 登录生命周期](credentials-and-permissions.md#file-src-codex-auth-rs) |
| [src/configuration_backend.rs](../../../crates/adapters/src/configuration_backend.rs) | [为配置预演提供实际后端事实](factory-and-configuration.md#file-src-configuration-backend-rs) |
| [src/credentials.rs](../../../crates/adapters/src/credentials.rs) | [数据集隔离的系统密钥引用](credentials-and-permissions.md#file-src-credentials-rs) |
| [src/credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) | [固定助手的安全安装、校验和调用](credentials-and-permissions.md#file-src-credentials-macos-rs) |
| [src/docker.rs](../../../crates/adapters/src/docker.rs) | [归属明确的容器与不扩权的挂载](engineering-resources.md#file-src-docker-rs) |
| [src/experience.rs](../../../crates/adapters/src/experience.rs) | [按 Agent 身份只读汇集跨项目经验](storage-and-source-codecs.md#file-src-experience-rs) |
| [src/factory_bundle.rs](../../../crates/adapters/src/factory_bundle.rs) | [读取嵌入配置并一次性注入 core](factory-and-configuration.md#file-src-factory-bundle-rs) |
| [src/git.rs](../../../crates/adapters/src/git.rs) | [受管仓库、候选提交与精确引用推进](engineering-resources.md#file-src-git-rs) |
| [src/github.rs](../../../crates/adapters/src/github.rs) | [稳定 PR 身份、仓库政策与回执核验](engineering-resources.md#file-src-github-rs) |
| [src/harness.rs](../../../crates/adapters/src/harness.rs) | [原生 worker、私有会话目录与真实停止证明](harness-and-protocols.md#file-src-harness-rs) |
| [src/integration_oauth.rs](../../../crates/adapters/src/integration_oauth.rs) | [PKCE、固定账户和未知交换结果](external-integrations.md#file-src-integration-oauth-rs) |
| [src/integrations.rs](../../../crates/adapters/src/integrations.rs) | [离线合同校验和持久化 HTTP/MCP 执行](external-integrations.md#file-src-integrations-rs) |
| [src/jsonl.rs](../../../crates/adapters/src/jsonl.rs) | [有界 JSONL 通道、请求关联与事件日志](harness-and-protocols.md#file-src-jsonl-rs) |
| [src/lib.rs](../../../crates/adapters/src/lib.rs) | [工程数据库、封存文件和生产端口装配](storage-and-source-codecs.md#file-src-lib-rs) |
| [src/model_providers.rs](../../../crates/adapters/src/model_providers.rs) | [冻结模型连接与只读模型目录](external-integrations.md#file-src-model-providers-rs) |
| [src/permission_profiles.rs](../../../crates/adapters/src/permission_profiles.rs) | [权限预设导入、编译与内容身份](credentials-and-permissions.md#file-src-permission-profiles-rs) |
| [src/permissions.rs](../../../crates/adapters/src/permissions.rs) | [把可移植授权变成 Codex/DSH 和容器策略](credentials-and-permissions.md#file-src-permissions-rs) |
| [src/process.rs](../../../crates/adapters/src/process.rs) | [受管进程组、私有 stdin 和输出边界](harness-and-protocols.md#file-src-process-rs) |
| [src/runtime_preparation.rs](../../../crates/adapters/src/runtime_preparation.rs) | [按冻结配方准备镜像与认证组件](runtime-preparation.md#file-src-runtime-preparation-rs) |
| [src/runtime_registry.rs](../../../crates/adapters/src/runtime_registry.rs) | [注册不可变安装并发现内容漂移](runtime-preparation.md#file-src-runtime-registry-rs) |
| [src/runtime_verification.rs](../../../crates/adapters/src/runtime_verification.rs) | [按实际驱动与权限资产验证原生协议](runtime-preparation.md#file-src-runtime-verification-rs) |
| [src/session_resources.rs](../../../crates/adapters/src/session_resources.rs) | [把运行准备、凭据和权限接成会话资源端口](runtime-preparation.md#file-src-session-resources-rs) |
| [src/source_store.rs](../../../crates/adapters/src/source_store.rs) | [不可变源与运行治理记录分开落盘](storage-and-source-codecs.md#file-src-source-store-rs) |
| [src/source_views.rs](../../../crates/adapters/src/source_views.rs) | [无损压缩冻结视图，保留原始事实](storage-and-source-codecs.md#file-src-source-views-rs) |
| [src/tests.rs](../../../crates/adapters/src/tests.rs) | [持久化测试作业与结构化 Node 结果](engineering-resources.md#file-src-tests-rs) |
| [tests/command_tests.rs](../../../crates/adapters/tests/command_tests.rs) | [真实测试容器、停止与代码导出防漂移](testing.md#file-tests-command-tests-rs) |
| [tests/configuration_changes.rs](../../../crates/adapters/tests/configuration_changes.rs) | [完整候选图、原子提交和跨库故障恢复](testing.md#file-tests-configuration-changes-rs) |
| [tests/configuration_resources.rs](../../../crates/adapters/tests/configuration_resources.rs) | [资源源化、默认闭包与自检配置](testing.md#file-tests-configuration-resources-rs) |
| [tests/configured_workflows.rs](../../../crates/adapters/tests/configured_workflows.rs) | [从冻结计划到产物接纳的完整用例](testing.md#file-tests-configured-workflows-rs) |
| [tests/conversations.rs](../../../crates/adapters/tests/conversations.rs) | [会话隔离、消费证明与不可变历史](testing.md#file-tests-conversations-rs) |
| [tests/delivery_acceptance.rs](../../../crates/adapters/tests/delivery_acceptance.rs) | [真实 Red/Green、候选 diff 与交付合同对齐](testing.md#file-tests-delivery-acceptance-rs) |
| [tests/docker_resources.rs](../../../crates/adapters/tests/docker_resources.rs) | [容器归属、挂载预检和嵌套沙箱](testing.md#file-tests-docker-resources-rs) |
| [tests/git_resources.rs](../../../crates/adapters/tests/git_resources.rs) | [本地真实 Git 的 CAS 与故障恢复](testing.md#file-tests-git-resources-rs) |
| [tests/github_resources.rs](../../../crates/adapters/tests/github_resources.rs) | [真实 PR 丢失回执后的稳定恢复](testing.md#file-tests-github-resources-rs) |
| [tests/governance.rs](../../../crates/adapters/tests/governance.rs) | [Agent 实例范围、经验来源与目录治理](testing.md#file-tests-governance-rs) |
| [tests/manager_workspace.rs](../../../crates/adapters/tests/manager_workspace.rs) | [经理原生调用链与共享写入边界](testing.md#file-tests-manager-workspace-rs) |
| [tests/model_providers.rs](../../../crates/adapters/tests/model_providers.rs) | [冻结 provider profile 与目录发现边界](testing.md#file-tests-model-providers-rs) |
| [tests/orchestration.rs](../../../crates/adapters/tests/orchestration.rs) | [驱动一致性、阻塞调度与生命周期场景](testing.md#file-tests-orchestration-rs) |
| [tests/permission_profiles.rs](../../../crates/adapters/tests/permission_profiles.rs) | [权限源发布、冻结绑定和会话切换恢复](testing.md#file-tests-permission-profiles-rs) |
| [tests/permissions.rs](../../../crates/adapters/tests/permissions.rs) | [权限编译拒绝与真实写入隔离](testing.md#file-tests-permissions-rs) |
| [tests/runtime_registry.rs](../../../crates/adapters/tests/runtime_registry.rs) | [安装去重、实际文件摘要和设置原子性](testing.md#file-tests-runtime-registry-rs) |
| [tests/runtime_resources.rs](../../../crates/adapters/tests/runtime_resources.rs) | [配方身份、实际驱动与跨数据集镜像保留](testing.md#file-tests-runtime-resources-rs) |
| [tests/schema_contracts.rs](../../../crates/adapters/tests/schema_contracts.rs) | [离线 Schema 与 Rust 序列化合同对齐](testing.md#file-tests-schema-contracts-rs) |
| [tests/standard_snapshots.rs](../../../crates/adapters/tests/standard_snapshots.rs) | [规范闭包、不同 Task 选择和历史恢复](testing.md#file-tests-standard-snapshots-rs) |
| [tests/support/mod.rs](../../../crates/adapters/tests/support/mod.rs) | [共享出厂装配、固定镜像与平台夹具](testing.md#file-tests-support-mod-rs) |
| [tests/tool_integrations.rs](../../../crates/adapters/tests/tool_integrations.rs) | [工具合同、OAuth 与未知副作用恢复](testing.md#file-tests-tool-integrations-rs) |
| [tests/workflows.rs](../../../crates/adapters/tests/workflows.rs) | [工程存储、锁、分页和 Artifact 基础合同](testing.md#file-tests-workflows-rs) |

<a id="file-cargo-toml"></a>

## Cargo.toml：外部实现所需依赖和测试边界

源码：[crates/adapters/Cargo.toml](../../../crates/adapters/Cargo.toml)。

本包依赖 core/domain，并引入 rusqlite、sha2、JSON Schema、ZIP/YAML 等外层实现库。系统凭据使用平台条件依赖；测试用 tempfile 创建独立数据与仓库。`build.rs` 参与编译，将配置资源和 Schema 校验入口嵌入产物。

依赖应留在实际使用的边界：SQLite 和协议细节不应反向进入 domain/core。包中的 `tests.rs` 是生产测试执行适配器，`tests/` 才是 Rust 集成测试目录。二者名称相近，职责不同。

关联阅读：[adapters/build.rs](factory-and-configuration.md#file-build-rs)、[adapters/src/tests.rs](engineering-resources.md#file-src-tests-rs)、[core/src/ports.rs](../devdesk-core/engine-and-ports.md#file-src-ports-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md)
