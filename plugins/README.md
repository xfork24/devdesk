# 领域包

| 目录 | 发布方式 | 内容 |
|---|---|---|
| [software-development/](software-development/README.md) | 独立领域 ZIP | Feature/Task 工作流、角色与 Agent、Skill、Spec、Delivery、Standard、工程工具、运行处理器和 Web 贡献 |

每份目录通过 manifest 明确列出发布范围。已发布源按 `identity:revision` 引用。软件开发包的源使用 `software-development.` 命名空间，直接维护当前修订，并可引用已发布的[内核能力](../kernel/README.md)。

```bash
./scripts/check/check-defaults.sh
./scripts/check/check-permissions.sh
```

前者核对文件归属和引用，后者检查权限 Schema。[内核目录](../kernel/README.md)的改动随 Host 构建，领域改动随新版本 ZIP 发布。工作台运行时以已安装的目录及执行冻结快照为准。详见[源配置](../docs/design/configuration.md)和[领域包](../docs/design/domain-packages.md)。
