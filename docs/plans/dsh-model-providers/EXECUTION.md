# DeepSeek Harness 多模型接入实施记录

日期：2026-11-09。状态：实施中。真实模型仅使用 Codex；DSH 使用本地模拟端点，不访问真实推理服务。

## 工作区

独立 worktree：`/Users/hengliu/workspace/devdesk-dsh-model-providers`；分支 `codex/dsh-model-providers`。基线快照 `a91a815` 保留主干当时未提交的配置依赖，不修改主干索引。合入仅提交本计划增量。

## M0 固定运行时

已离线读取现有镜像 `devdesk/managed-dsh:0.2.2-rc.1-09644ad904fab573`（image ID 前缀 `0f15dc59cc94`）中的发行包、base composition 和 pi-ai 模型目录，确认 `@deepseek-ai/dsh-llm-pi-ai@0.2.2-rc.1` 随包提供，支持 `providers` 字典、显式协议、模型容量及自定义端点。保留上游版本。固化非秘密模型目录供运行校验；MiniMax 端点显式覆盖为官方当前国内端点。未读取凭据、未调用模型。

## M1 到 M4

已实现服务商目录、Model 容量声明、统一解析、冻结连接与 profile 摘要、按服务商隔离的 vault 引用、旧 DeepSeek 兼容、受控 DSH patch、Ollama 宿主映射、模型目录、完整 Agent 连接/推理验证入口及 Web 表单。DSH 使用原有 0.2.2-rc.1，新增内建 Harness 通过固定配方物化；未增加其他 Harness 框架。

Domain/Core/Host/Schema 校验覆盖非法服务商组合、端点、容量、请求参数、跨服务商/数据集引用与缺失快照。Ollama 无需 vault。Host 完整链检查修复了新建全局 Agent 后项目配置目录未同步的问题；Model 新版本和重启不改旧连接，见 [Host 离线验收](HOST-OFFLINE.md)。

固定 DSH 镜像在 --network=none 内分别访问三种模拟路由，每条完成流式文本、原生 Bash 工具结果消费、重启与原生恢复；MiniMax thinking 回放通过。共 9 次模拟推理请求，没有外部模型请求。另以本地模拟 Ollama 目录验证宿主/目标容器分别检查与资源清理。

macOS 助手支持新的有限账号命名空间，协议 1 的旧助手继续支持 DeepSeek，协议 2 用于新服务商。打包在独立目录构建、签名并校验新版助手，不替换已授权共享安装；运行包提供显式 --update-credential-helper。已比较共享助手二进制和清单，确认打包前后未变。

## M5 本地验证

已执行完整 scripts/check/check.sh：Rust、HTTP/Schema、运行时准备与独立二进制检查通过，debug/release 各 15 项浏览器测试通过。新增浏览器测试验证独立凭据表单、保存后清空、免密 Ollama 和不可变 Model 保存。更新后的 Rust 回归 132 项通过，11 项环境测试默认忽略；两项 DSH/Docker 离线环境测试另行显式执行通过。Python 可选 jsonschema 未安装，发布 Schema 的离线解析与实例校验由 Rust jsonschema 测试覆盖。

运行包已生成，随包凭据助手 protocol=2、内容摘要匹配；最终基线整合和 Codex 真实模型验收正在进行。Ubuntu 宿主及 DSH 真实订阅推理按本轮约束保留为待验，不以模拟结果替代。
