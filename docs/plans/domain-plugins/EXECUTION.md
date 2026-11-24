# 领域插件化实施与验证

实现工作位于 `codex/domain-plugins`。计划基线为 `2458a06`。首条可验收路径覆盖 P0 的本地基线、P1 空领域和 P2 真实运行时及双端协议。P3 的切换屏障已开始接入。

## 已接通的路径

- 核心 Host 的无默认 feature 构建以独立清单加载 Model、Provider、Catalog、Harness、Sandbox、权限预设和通用工具。核心数据集使用独立布局标记。独立 Web 入口和构建脚本可从核心 Host 的 `--web-dir` 提供静态文件。
- ZIP 清单、内容摘要、路径边界、源闭包和呈现绑定在安装时检查。激活以全局代次切换，SSE 驱动 Web 与无浏览器客户端重新发现。已激活的 React 组件动态进入同一个 React 根。
- 领域读处理器在固定 Node 镜像的临时 Docker 容器中通过 JSON-RPC 执行。Git 读取由平台适配器执行。文稿 Workflow 的工具步骤走既有 Rust 调度、持久化调用和回执接纳，Web 可按无浏览器端返回的执行 ID 读回状态和输出。
- 切换及卸载核对包绑定项目中的活动根流程。正在工作的包需要先结束在途流程。包停用期间，其绑定项目不领取后台调度。重装相同摘要后可读取原有执行。

## 验收入口

```bash
cargo build --locked -p devdesk-host --no-default-features
npm --prefix web ci
npm --prefix web run build:core
node tests/entrypoints/domain_packages.mjs target/debug/devdesk-host web/dist/core
```

验收使用独立数据目录、自建 Git 仓库、固定 Linux Docker 镜像、两个 Chromium 标签页和 Python 标准库 HTTP 客户端。记录真实提交和执行 ID，检查空领域中新增的模型、真实 Codex Harness/Docker 准备回执、核心 Git 读取、同一提交的双端读取、文稿 Workflow 持久化输出、未结束流程阻塞卸载、取消后卸载、SSE 代次、旧代次拒绝以及重装后的原运行。容器内的文稿处理和本地 Git 查询为实际执行。现有 Agent 定义用于配置发现，模型调用和 GitHub 写入属于后续验收。

独立发行件解压后的完整脚本通过。该次记录的镜像为 `sha256:328e96db04cc428bec67d1e8ef06837041c3c147ce5940c7e86c251f9809b84b`，流程执行为 `d3eab70c-c437-db47-e175-329274fc50d2`。证据位于 `/tmp/devdesk-domain-release-verification.B1Vzff/result.json`。

## 基线和后续范围

原基线 `2458a06` 的 `actual_git_docker_workflow_handoffs` 在启动 Docker 前停于 `builtin-tool-workspace-snapshot:2` 的配置读取。探针把项目级源用于全局草稿，并引用了退役 Harness 的测试工具修订。只修正探针的草稿作用域和工具引用后，真实 Git/Docker 流程在 18.210 秒内完成。证据存于 `/tmp/devdesk-domain-p0.O6iSOB/evidence`。

首批 ZIP 使用 Host 启动参数提供的可信摘要。当前每次只读调用使用临时容器。通用工作项、三驱动联合验收、带副作用处理器的停止和核对、完整软件开发包、跨平台兼容矩阵及容量验收仍按 P3–P6 推进。[完整计划](PLAN.md)
