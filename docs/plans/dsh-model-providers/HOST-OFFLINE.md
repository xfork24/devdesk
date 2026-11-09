# 多模型接入 Host 验收

真实模型仅使用 Codex；DSH 仅做本地模拟目录查询，不发推理请求。

- 完整 Model → Capability → Agent → Session → 固定 DSH 镜像物化与容器目录查询通过。
- Ollama 无凭据；连接和 profile 摘要冻结，Model 新修订与 Host 重启不改旧 Session。
- DSH 模拟服务只收到一次 GET /v1/models，没有推理请求。

结果：通过。本次为 macOS 本机与 Linux Docker；Ubuntu 宿主和 DSH 真实推理未验收。
