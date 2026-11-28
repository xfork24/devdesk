# 内核目录

| 目录 | 发布方式 | 内容 |
|---|---|---|
| [core/](core/manifest.json) | 随 Rust Host | Model、ModelProvider、Harness、Sandbox、权限预设、内核处理器的工具定义及默认选择 |

内核目录不是领域包：它由 Host 在构建时内嵌，不经过领域 ZIP 的安装与协商。已发布源按 `identity:revision` 引用，使用 `builtin-` 前缀。领域包在自己的 `requires` 中声明依赖的内核能力，见[领域包](../docs/design/domain-packages.md)。

```bash
./scripts/check/check-defaults.sh
./scripts/check/check-permissions.sh
```
