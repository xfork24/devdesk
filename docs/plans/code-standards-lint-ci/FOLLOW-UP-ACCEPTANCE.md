# Web 警告与外部验收补充验收

基线：ef37b778343b805a202fc2213fd41eaa320b088f。逐条判断见 [补充评估](FOLLOW-UP-ASSESSMENT.md)。原始日志、镜像身份、完整 ESLint JSON 和输入摘要位于 `/tmp/devdesk-web-native-20261115/`，只提交 Markdown 报告。

## 已执行结果

| 范围 | 实际结果 |
|---|---|
| ESLint | 0 errors、0 warnings；原 49 条 hooks 与 26 条 refresh 全部消除；lint 收紧为 `--max-warnings 0`，规则没有关闭 |
| 既有行内抑制 | 修改前后均为 4 条：2 条 RPC any，2 条 Playwright 空对象形参；RPC 的两条原样移至 rpc-types.ts，未新增抑制 |
| TypeScript / Prettier / Web build | src 与 tests 类型检查、全量格式、构建通过 |
| Rust 风格 | 最终 rustfmt 与 clippy workspace/all-targets `-D warnings` 通过 |
| 便携 Rust | 全量快照 291 passed、17 ignored；最终新增的带密钥本地服务回归另行通过，model_providers 最终 5/5（含外部测试） |
| 外部 Rust | 最终入口 9 个测试二进制、17 项全部实际通过，0 failed、0 ignored；不是将默认套件的 ignored 计作 passed |
| debug Host | HTTP/认证/Origin/SSE/幂等、数据集、模板重置、运行准备通过；Chromium 58/58 |
| release Host | 独立目录运行的 HTTP 合同、数据集、模板重置、运行准备通过；Chromium 58/58 |
| 新增/加强浏览器回归 | 同一 Session 更新绑定同步 Agent；切换身份隔离迟到历史；前进后退保留 Role 草稿，5 项聚焦回归全过 |
| CI 工作流静态检查 | actionlint 通过；既有 4 项精确 SHA/事件/重跑/分叉门禁测试通过 |
| 应用模块迁移 | TypeScript AST 对比确认原 main.tsx 的 10 个既有函数主体保持一致，仅调整导入与挂载边界 |

共享工作区执行期间出现了另一个治理任务的未完成 Rust 变更。全量 `check.sh` 因此在独立源码快照 `validation-source/` 执行，没有覆盖或收进该任务的变更。全量浏览器使用的 Web 源码与最终 Web 文件逐字节一致。快照构建后补充的带密钥本地连接保护、权限镜像选择和外部验收证明强度，均在 main 的最终源码上通过 model_providers、全量 17 项外部入口、rustfmt 与 clippy 重新验证。最终提交的全量远端 CI 按精确 SHA 观察，运行链接由交付消息提供。

## 最终外部验收

命令：

```bash
python3 tests/entrypoints/native_acceptance.py --github-request /专用/pr-request.json --github-repo xfork24/devdesk-integration-test --reports /tmp/devdesk-web-native-20261115/native-complete
```

镜像由 `--prepare` 按当前 manifest 的出厂选择预先构建、检查并冻结；最终一轮使用冻结的 5 个 image ID 环境变量，不依赖旧发行标签。最终报告 `native-complete/native-acceptance.json` 为 `passed: true` / `scope: all-17`。记录的源内容摘要为 `d399d1b36cfd1ecebd5bbe8e321cd6e53b27e14fb63aa00ae634acb638a63785`，结束后重新计算一致；入口会拒绝在源码输入变化的情况下宣布通过。

- Codex：`sha256:8d5419bb30ddbd009ae64775f2eb31acb1d33b7876d1e90efaaab5fb7f441a6f`
- DSH / DSH SDK fixture：`sha256:a4d3b22bc2f2daa5e7e48563be970934b936c01a86039c3947a94f4cd3797fdf`
- Node：`sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f`
- HTTP/MCP/OAuth 工具使用上述 Codex ID。

DSH 实际运行固定 SDK，四组本地协议响应覆盖智谱、MiniMax、免密 Ollama 和带密钥自定义本地服务的流式响应、原生工具结果消费与会话恢复；不调用真实付费模型。新 profile 不生成空 `apiKeyEnv`；公开占位头仅供免密本地连接使用，带密钥连接继续解析其密钥引用。

HTTP 验收不再用任意失败冒充私网拒绝：负例使用私网 IP 字面地址并断言 `endpoint_denied`；正例、幂等、恢复、输出 Schema 与请求计数均保留。权限测试全部使用声明的冻结镜像。跨数据集验收实际生成不同镜像，再验证旧安装与两个内容身份 tag 均保留。

GitHub 使用专用仓库既有 PR #41，先读回执，再两次按原 marker 恢复，number/head/marker/merged 一致。没有创建新 PR；执行期间核对的 PR 编号集合也保持一致。

## CI 与边界

E2E 在精确提交的 Rust/Web 前置门禁成功后，先加载已有命名 Linux 沙箱策略，再执行 `native_acceptance.py --docker-only --prepare` 的 16 项，随后执行 debug/release HTTP 与浏览器回归；成功与失败均归档原始 native 日志、源内容摘要和镜像身份，供远端逐项核对。GitHub 第 17 项保留需要专用仓库认证的显式入口，不向普通 CI 添加业务凭据。17 项的默认 `#[ignore]` 保留外部资源分层，专用入口按实际执行数量失败即停。

Vite 仍提示入口分块约 503.86 kB，属于构建体积提示，未通过提高阈值隐藏；它不属于本次 75 条 ESLint 警告。当前 Vite 未启用 React Refresh 插件，模块边界合格不等于已经验收 Fast Refresh 状态保留。RPC 的动态 any 边界也仍存在，不能将零警告写成完全强类型。

旧错误 Ollama profile 的冻结摘要不会被静默替换；相关旧会话需要重新创建绑定/会话。最初 HTTP `ECONNRESET` 没有足够证据归因于产品执行器；未改执行器的重复实跑通过，随后采用标准 HTTP 夹具并加强断言，最终完整入口通过，未用自动重试掩盖失败。
