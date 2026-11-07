# DevDesk 0.2 本地运行包

解压后，在本目录执行：

```bash
./devdesk-host
```

在浏览器打开终端显示的 `http://127.0.0.1:4317`，输入空目录或已有 DevDesk 数据集路径。路径可包含中文和空格。程序实际保存 SQLite、文档、报告和历史；运行不需要 Rust、Node、数据库服务或源码目录。

也可以直接打开已有数据集：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

同一数据集只能有一个可写程序。退出可在 Web 设置页操作，或在终端按 Ctrl+C。重新执行相同命令可读取已有数据；未确认完成的执行会标记中断，可显式重试。更换数据目录需退出后重新启动，不会隐式迁移文件。

包名中的目标架构应与机器匹配。macOS Apple Silicon 为 `aarch64-apple-darwin`；本轮 macOS 验证版本为 26.7.2。当前提供本地命令行程序；没有制作签名、公证的 `.app` 安装器。

外部 Git、Agent、项目测试和环境均为 **simulation**；完成表示“模拟流程已完成”。Archive 与迁移只预览。完整步骤见运行包内 `docs/demo.md`，平台证据见 `docs/acceptance.md`。
