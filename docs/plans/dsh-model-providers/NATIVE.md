# 多模型接入 Host 验收

运行方式：独立运行包内的 release Host。真实模型仅使用 Codex；DSH 仅做本地模拟目录查询，不发推理请求。

- 完整 Model → Capability → Agent → Session → 固定 DSH 镜像物化与容器目录查询通过。
- Ollama 无凭据；连接和 profile 摘要冻结，Model 新修订与 Host 重启不改旧 Session。
- DSH 模拟服务只收到一次 GET /v1/models，没有推理请求。
- Codex 真实模型：gpt-6.1-sol；完整 Agent 实际验证、原生工具结果消费、Host 重启与原生会话恢复通过。
- Codex Session：`7634f1ea-47ff-4e97-8156-ac7b05078265`；恢复 Turn：`7f473d45-86cc-4bfc-827d-1bef3f6d9df7`。
- Codex OAuth token 扫描通过：非凭据持久化文件未发现 token，报告不包含秘密。

结果：通过。本次为 macOS 本机与 Linux Docker；Ubuntu 宿主和 DSH 真实推理未验收。
