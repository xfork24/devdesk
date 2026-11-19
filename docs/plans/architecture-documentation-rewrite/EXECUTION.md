# 架构与代码文档重写：实施与验证

日期：2026-11-19。状态：P0–P5 完成。

源码核对基于 `e925b739` 及本轮工作区。已采用用户更新的顶层 `configurations/` 与 `crates/adapters/runtime/tool-worker.mjs` 路径。未修改 Rust、运行配置或产品业务行为。

## 交付结构

- [架构入口](../../design/README.md)：按系统全貌、分层、概念、配置、执行、通信、数据恢复、安全与工程交付组织；接口细节放 reference。
- [代码入口](../../crates/README.md)：仓库地图、跨模块调用链、五个包的专题、Web 衔接、测试与扩展、开发规范。
- 架构目录 19 篇，代码目录 40 篇，其中 5 篇是独立自动文件索引。正文不再混入完整文件清单。
- 24 篇旧页在内容迁移后删除。保留路径的页面也重新编写，未保留另一套当前文档或跳转空页。
- 根 README、AGENT、脚本/测试/Schema 索引、源码中的文档提示同步更新。历史计划只机械更新链接和已失效的章节定位，不改变原验收结论。

## 内容核对与取舍

| 主题 | 核对依据 | 处理结果 |
|---|---|---|
| 分层与装配 | Cargo.toml、core/ports.rs、adapters 的 execution_adapters、host/workspace.rs 的 engine | 重写职责；分别说明编译依赖和运行调用 |
| PM 派发与 PRD 返回 | workflow_dispatch、workflow_runtime、conversation_runtime、harness/jsonl/docker、workflow_outputs、controller_runtime | 写成连续过程和时序图，补充异步派发、输出校验与事件唤醒 |
| 工具能力 | manifest 发布源、PLATFORM_HANDLERS、CONTROL_TOOLS、MANAGER_TOOLS、agent_context | 单独列 49 个发布平台工具；区分 54 个处理器、2 个原生能力声明与经验接口 |
| 工作流与规则 | domain/workflow、workflow_evaluation、workflow_execution_controls、当前 Feature/Task 资源 | 区分模型完成、输出接纳、评审、Human 确认与流程完成 |
| 配置和数据 | source/configuration_graph、configuration_*、source_store、catalog、SQL | 保留修订、分库、原版本检查与恢复约束，删除重复定义 |
| 运行和凭据 | runtime_preparation、permissions、credentials_macos、独立助手、平台脚本 | 保留真实权限、初次签名、稳定副本、安装与升级操作 |
| 外部接入 | integrations、integration_oauth、runtime/tool-worker.mjs | 保留支持范围及未知远端效果，移出架构主流程 |
| Web | App、configuration-client、runtime-state、query-cache、event_hub | 按请求、查询、事件与草稿保护组织 |
| 历史补充与测试清单 | 原文档和对应测试入口 | 当前机制重写；逐次补丁叙事删除，测试按功能汇总，历史证据仍在计划目录 |

## 旧页迁移记录

下列文件完成有价值内容迁移，重复段落和过时的组织方式删除。表中的“主入口”是更新旧链接时的默认目标；拆分主题还由主入口链接到其他章节。

| 删除的旧页 | 新主入口 |
|---|---|
| `docs/design/design.md` | [docs/design/overview.md](../../design/overview.md) |
| `docs/design/domain.md` | [docs/design/domain-model.md](../../design/domain-model.md) |
| `docs/design/contracts.md` | [docs/design/reference/contracts.md](../../design/reference/contracts.md) |
| `docs/design/source-configuration.md` | [docs/design/configuration.md](../../design/configuration.md) |
| `docs/design/workflow-orchestration.md` | [docs/design/workflow.md](../../design/workflow.md) |
| `docs/design/conversations.md` | [docs/design/agent-runtime.md](../../design/agent-runtime.md) |
| `docs/design/storage.md` | [docs/design/data-and-recovery.md](../../design/data-and-recovery.md) |
| `docs/design/protocol.md` | [docs/design/reference/rpc.md](../../design/reference/rpc.md) |
| `docs/design/permissions.md` | [docs/design/security.md](../../design/security.md) |
| `docs/design/system-credentials.md` | [docs/design/security.md](../../design/security.md) |
| `docs/design/model-providers.md` | [docs/design/reference/integrations.md](../../design/reference/integrations.md) |
| `docs/design/tool-skill-integrations.md` | [docs/design/reference/integrations.md](../../design/reference/integrations.md) |
| `docs/design/real-adapter-contract.md` | [docs/design/layers.md](../../design/layers.md) |
| `docs/design/real-engineering.md` | [docs/design/engineering-delivery.md](../../design/engineering-delivery.md) |
| `docs/design/code-standards-rust.md` | [docs/crates/standards/rust.md](../../crates/standards/rust.md) |
| `docs/design/code-standards-web.md` | [docs/crates/standards/web.md](../../crates/standards/web.md) |
| `docs/crates/devdesk-domain/agents-and-conversations.md` | [docs/crates/devdesk-domain/runtime-and-permissions.md](../../crates/devdesk-domain/runtime-and-permissions.md) |
| `docs/crates/devdesk-domain/integrations.md` | [docs/crates/devdesk-domain/runtime-and-permissions.md](../../crates/devdesk-domain/runtime-and-permissions.md) |
| `docs/crates/devdesk-core/workflow-compilation.md` | [docs/crates/devdesk-core/configuration-and-catalog.md](../../crates/devdesk-core/configuration-and-catalog.md) |
| `docs/crates/devdesk-core/orchestration-and-drivers.md` | [docs/crates/devdesk-core/workflow-execution.md](../../crates/devdesk-core/workflow-execution.md) |
| `docs/crates/devdesk-core/conversations-and-history.md` | [docs/crates/devdesk-core/session-resources-and-permissions.md](../../crates/devdesk-core/session-resources-and-permissions.md) |
| `docs/crates/devdesk-adapters/testing.md` | [docs/crates/testing-and-extension.md](../../crates/testing-and-extension.md) |
| `docs/crates/devdesk-host/rpc-and-configuration.md` | [docs/crates/devdesk-host/http-and-security.md](../../crates/devdesk-host/http-and-security.md) |
| `docs/crates/devdesk-host/testing.md` | [docs/crates/testing-and-extension.md](../../crates/testing-and-extension.md) |

## 读者问题验收

| 问题 | 可直接阅读的答案 |
|---|---|
| 各程序在哪里运行？ | [系统总览](../../design/overview.md) |
| core 不依赖 adapters，怎样调用它？ | [分层与依赖](../../design/layers.md)及 [Engine/ports](../../crates/devdesk-core/engine-and-ports.md) |
| domain、core、adapters 各决定什么？ | [分层职责表](../../design/layers.md) |
| PM 如何启动分析师并取得 PRD？ | [运行与通信](../../design/agent-runtime.md)及[调用链](../../crates/execution-flows.md) |
| 工具、内部原语与运行权限有什么关系？ | [接口说明](../../design/interfaces.md)及[工具参考](../../design/reference/tools-and-primitives.md) |
| 更新配置是否影响原运行？ | [配置生效](../../design/configuration.md) |
| 输出、评审和人工确认是什么关系？ | [工作流](../../design/workflow.md) |
| 中断、重试、返工和停止有什么不同？ | [数据与恢复](../../design/data-and-recovery.md) |
| 修改功能要读哪些文件和测试？ | [代码手册](../../crates/README.md)及[测试与扩展](../../crates/testing-and-extension.md) |
| 当前限制和验证范围是什么？ | [质量与限制](../../design/quality-and-limits.md) |

上述为内容与源码对照检查，不是对真实新读者开展的可用性实验。

## 检查器调整

自动索引从 overview.md 移到 files.md，保留人工专题链接。Git 工作区覆盖已跟踪文件和未被忽略的新源文件，排除被忽略的本地构建产物；没有 Git 元数据的源码包使用目录枚举。二进制资产纳入索引但不尝试按 UTF-8 解码。

保留本地链接和 Markdown 锚点检查，新增缺失索引及异常标记的可读错误。新增 8 项离线回归，并接入 scripts/check/check.sh。

## 验证结果与边界

| 检查 | 结果 |
|---|---|
| `python3 scripts/py/check-docs.py --write` 后再次检查 | 当前文档本地链接、锚点及五个包索引通过 |
| `python3 tests/entrypoints/test_docs_checker.py` | 8 项通过：缺失/异常索引、增删、生成稳定性、人工链接、二进制资源、Git 忽略规则、坏链接/锚点 |
| `./scripts/check/check-architecture.sh` | 分层、纯领域、资源边界和应用状态责任通过 |
| `git diff --check` | 通过 |
| 当前路径搜索 | 两个重写目录未残留 resources/configuration、resources/integrations 或被删除文档的当前链接 |
| 工具定义及关键符号核对 | 发布集合与 49 行工具参考一致，核心调用入口存在 |
| 发布包运行指南 | 保留 run-binary.md 路径；启动说明自包含，无需源码相对链接；打包脚本仍复制它为包根 README |

本轮未重建产品二进制、执行完整 Rust/Web 回归、启动模型或进行真实 GitHub 交付；这些行为不受本轮文档改写影响。检查脚本、Shell 引用及文档本身按实际改动验证。

历史计划仍有 5 个原有失效链接，指向已退役的 Schema/工作流文件或旧报告路径；它们不是本次重写引入的引用，保留原历史内容，未纳入当前文档通过声明。当前两个目录及其入口的链接检查通过。
