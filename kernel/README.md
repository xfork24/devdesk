# 内核目录

| 目录 | 发布方式 | 内容 |
|---|---|---|
| [core/](core/manifest.json) | 随 Rust Host | Model、ModelProvider、Harness、Sandbox、权限预设、内核处理器的工具定义及默认选择 |
| [engineering/](engineering/manifest.json) | 随 Rust Host，按需提供 | 工程扩展的工具定义（`tests.*`、`git.*`、`workspace.*` 处理器），标识用 `engineering-` 前缀；领域包声明了对应扩展组时，它的项目才得到它们 |

内核目录不是领域包：它由 Host 在构建时内嵌，不经过领域 ZIP 的安装与协商。已发布源按 `identity:revision` 引用，使用 `builtin-` 前缀。领域包在自己的 `requires` 中声明依赖的内核能力，见[领域包](../docs/design/domain-packages.md)。

```bash
./scripts/check/check-defaults.sh
./scripts/check/check-permissions.sh
```
