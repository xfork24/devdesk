# devdesk-domain：对象与纯规则

这个包定义系统讨论的对象以及不依赖 I/O 的判断。它可以检查工作流是否成环、输出类型是否合理、完成规则是否成立，但不启动容器、不查询数据库，也不读取当前时间或环境。

## 阅读顺序

| 专题 | 关键内容 |
|---|---|
| [工作与产物](work-and-materials.md) | Project/Feature/Task、Artifact 与结果归属 |
| [配置和引用](configuration-and-sources.md) | 源修订、依赖图、Agent 组合与执行配方 |
| [工作流规则](workflows-and-checks.md) | 步骤、输入输出、CompletionRule 与检查依据 |
| [会话与权限类型](runtime-and-permissions.md) | Session/Turn、运行身份、Profile 与接入类型 |

core 构造规则的输入并处理结果；adapters 将这些类型编码或转换成外部协议。类型能够反序列化，不等于调用者已获授权。

入口：[lib.rs](../../../crates/domain/src/lib.rs)。完整文件查找：[files.md](files.md)。架构背景：[核心概念](../../design/domain-model.md)。
