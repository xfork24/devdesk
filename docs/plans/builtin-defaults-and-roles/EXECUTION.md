# 默认套件与 Role 执行记录

日期：2026-11-09。状态：实现、回归及全新数据初始化完成。

## 实现

| 对象 | 内建实现 |
|---|---|
| Model | gpt-6.1-sol，reasoning_effort=high |
| Harness | Codex 0.167.0；编译期固定 Docker 配方 SHA256，运行时固定已核验镜像 |
| Role | 需求分析师、架构师、开发、测试、评审；Global/Project Scope、不可变修订及删除记录 |
| Agent / AgentID | 每个 Role 有完整组合和默认实例，全部使用 Sol High / Codex；全局默认实例按各自 ID 跨项目积累经验 |
| Workflow | 完整 Feature 流程及独立 Task 子流程，需求先行、TDD、独立 Review、主干接纳前确认 |
| Spec / Standard | planning、implementation、merge 三种用途的 Spec 和工程 Standard |
| Skills / Tools | 上下文与 worktree 方法、角色工作方法；原生 Codex 工具组与 AgentID 经验读取 |
| Permission / Sandbox | 版本化权限预设及 Linux Docker 默认 Sandbox；实际工作范围继续收窄执行权限 |

默认套件查询和配置管理不依赖 Docker 或账号授权。正常 Web 创建 GitHub Project 时即建立默认绑定；Feature 保存 defaults_bundle_ref 和完整依赖锁。底层模拟夹具仍可显式走原离线契约，不出现在正常工程入口。

Role 的 UI 位于“规范与身份 → 角色 Role”。新增、编辑和删除使用真实 RPC 与 SQLite；Role 创建自动生成默认 Agent/实例，Agent 编辑器选择 Role 精确修订。Role 职责和 Agent 额外指令共同构成执行指令。历史 role 字段作为工程执行职责兼容保留，实际角色身份是 role_ref。

## 数据重置

用户明确要求清空所有 data 和数据库。旧 Host 已停止；工作区 data/ 的旧主库、项目库、备份、原生会话、日志和验收数据全部删除。源码回归测试、系统 Codex 目录和远端仓库不在清理范围。旧报告中的本机运行 ID 仅保留为历史记录，原始本机数据不再可读取。

重新初始化后仅有内建配置与新全局库；不恢复旧 Project、Feature 或 Agent 经验。账号授权属于被清理的应用数据，Codex 执行需在设置重新登录。

## 验证

- 后端定向：完整默认依赖、初始化幂等、角色及实例固定组合、Role CRUD/Scope/历史、默认角色替换、旧 Feature 锁不变。
- HTTP/SQLite：自动 Project 默认配置、惰性环境准备、固定物化安装、缺失镜像不换绑。
- Chromium：完整默认套件展示、角色默认组合，以及 Role 新增、编辑、删除和刷新持久化。

### 实际结果

| 验证 | 结果 |
|---|---|
| `./scripts/check.sh` | 通过：Web 构建，Rust 132 通过 / 8 ignored，debug/release HTTP 与两组各 10 项 Chromium 回归 |
| Role 与默认套件定向回归 | 通过：最后界面样式调整后重新构建，debug/release 的 4 项相关浏览器用例均通过 |
| rustfmt、diff whitespace | 通过 |
| Permission 资产及 JSON Schema | 通过：6 个权限预设、4 个后端、7 个不可变资产；10 份 Schema 文档合法；实际默认套件 21 条类型化记录通过对应 Schema |
| 全新 release 数据集 | 通过：0 Project、0 Feature、0 experience，5 Role；SQLite integrity_check=ok，无外键错误 |
| 实际 Docker | 已物化固定镜像；重复准备复用同一安装，runtime.integrity verified；镜像内执行 `codex --version` 返回 codex-cli 0.167.0 |
| 实际界面 | release 服务中的五个角色、编辑回填正常；1440px/390px 人工查看，390px 无横向溢出，无页面 JavaScript 异常 |

HTTP 环境准备回归使用明确的假 Docker CLI 和 GitHub API 夹具，其中 Git 操作、HTTP、SQLite 为实际调用；覆盖 Project 自动绑定、并发惰性准备、物化幂等、原镜像缺失拒绝换绑、失败重试。该回归与下表的实际 Docker 核验分开记录。

实际新数据集为 `/Users/hengliu/workspace/devdesk/data`，服务地址 `http://127.0.0.1:4317`。内建套件为 `builtin-defaults-sol-high-codex-v1`。

| 实际默认运行时 | 固定值 |
|---|---|
| RuntimeInstallation | 008a2ca2-2233-406b-9506-62013cc04a89 |
| RuntimeKey | codex / linux / aarch64 / 0.167.0+docker.5f8731ec42de |
| 镜像 | sha256:5f8731ec42de5c5d542d5398147c3348c200ad7a66c6cfc99a5f52e0b9776ffc |
| 配方摘要 | e38e18dcedc55380 前缀；完整 SHA256 保存在 HarnessRevision 和物化回执 |

本轮没有恢复旧账号授权，也没有发起新模型调用或远端工程合入；不能把默认配置和 Docker 核验视为新一轮完整模型工程闭环。需要外部资源的 8 项 ignored 测试没有计为通过。

计划目录仍只保留 Markdown；浏览器截图、测试输出和 Python 缓存等本轮临时产物已清理，源码回归测试保留。

模型参数依据 [OpenAI 官方 GPT-6.1 Sol 文档](https://developers.openai.com/api/docs/models/gpt-6.1-sol)核对；Codex 版本以本机 `codex --version` 和项目固定安装配方核对，均为 0.167.0。
