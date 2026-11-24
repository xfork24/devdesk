# 修改功能与选择测试

测试应覆盖改动经过的边界。许多 core 行为测试位于 adapters/tests，因为它们需要真实 SQLite 或明确的端口夹具；测试所在包不等于被测责任全部属于 adapters。

## 按改动选择验证

| 改动 | 先读什么 | 主要测试入口 |
|---|---|---|
| 领域规则或格式 | domain、对应 Schema | domain 内联测试、[schema_contracts](../../crates/adapters/tests/schema_contracts.rs) |
| 配置发布/编译 | configuration_*、configured_workflows | [configuration_changes](../../crates/adapters/tests/configuration_changes.rs)、[configured_workflows](../../crates/adapters/tests/configured_workflows.rs) |
| 派发/接纳/返工 | workflow_* | [workflows](../../crates/adapters/tests/workflows.rs)、[workflow_resource_concurrency](../../crates/adapters/tests/workflow_resource_concurrency.rs) |
| 经理/工具权限 | controller_*、platform_* | [manager_workspace](../../crates/adapters/tests/manager_workspace.rs)、[orchestration](../../crates/adapters/tests/orchestration.rs) |
| SQL 和查询 | resource_commits、专用读模型 | [resource_commits](../../crates/adapters/tests/resource_commits.rs)、[advisory_read_bounds](../../crates/adapters/tests/advisory_read_bounds.rs) |
| Harness/Docker | harness、jsonl、docker | 内联协议测试、[docker_resources](../../crates/adapters/tests/docker_resources.rs)、[native_acceptance](../../tests/entrypoints/native_acceptance.py) |
| 外部工具/认证 | integrations、tool-worker、integration_oauth | [tool_integrations](../../crates/adapters/tests/tool_integrations.rs)、[integration_operations](../../crates/adapters/tests/integration_operations.rs) |
| HTTP/生命周期 | main、workspace、lifecycle | [http_contract](../../tests/entrypoints/http_contract.py)、[lifecycle_contract](../../tests/entrypoints/lifecycle_contract.py) |
| Web 交互 | App、查询客户端和组件 | tests/browser 下对应行为用例 |
| 文档/索引 | 当前说明与 check-docs | 文档检查及检查器定向测试 |

## 新增平台工具

1. 明确工具是查询还是修改、谁可调用、输入输出及副作用。
2. 在 core 增加处理路径和必要的端口调用，保留调用身份、权限与原版本检查。
3. 在 configurations 发布 ToolDefinition，更新 manifest，并由需要它的 Capability 明确采用。
4. 核对 Harness 工具 Schema 和实际驱动能力，不能只增加 handler 就认为模型可调用。
5. 覆盖未授权、错误参数、重复调用、版本冲突及外部结果未知的情况；更新工具参考。

## 新增适配器或规则

新增适配器先确定 core 端口能否表达所需能力；实现开始、结果读取、停止和核对语义，再在工厂与 Host 装配。生产失败不能回退到测试替身。

新增纯规则从 domain 开始，用明确输入验证规则，再检查 core 如何采集事实和提交状态。不能让 domain 为方便测试而读取数据库、环境、随机数或时钟。

新增 RPC 同时维护方法登记、Schema、访问分类、作用域和前置条件，再接入页面。测试至少确认项目隔离和旧版本冲突不会产生额外副作用。

## 常用命令

在仓库根目录执行，按改动选择必要项目：

```bash
python3 scripts/py/check-docs.py
python3 tests/entrypoints/test_docs_checker.py
./scripts/check/check-architecture.sh
cargo test --workspace --locked
npm --prefix web run typecheck
./scripts/check/check-style.sh
```

完整检查入口为 `./scripts/check/check.sh`，包含更多 HTTP、数据集和浏览器验证；部分准备依赖 Docker 和包源网络。详细前置条件见[测试索引](../../tests/README.md)。

真实 Docker 协议验收使用 `tests/entrypoints/native_acceptance.py --docker-only --prepare`，准备当前配置要求的安装。真实模型或 GitHub 交付需要明确的专用认证和仓库，不由普通回归自动代替。

## 怎样报告结果

记录运行的命令和平台、通过范围及未验证的边界。协议夹具不代表真实账户可用，编译通过不代表签名助手已授权，局部性能测试不代表容量目标达成。

日志、截图、JSON 和临时证据放临时目录；计划目录只保留可阅读的说明和结论。测试失败应保留原因，不能删掉断言或放宽安全规则以取得绿灯。
