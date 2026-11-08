# DevDesk 0.2 本地运行包

解压后，在本目录执行：

```bash
./devdesk-host
```

在浏览器打开终端显示的 `http://127.0.0.1:4317`，输入空目录或已有 DevDesk 数据集路径。路径可包含中文和空格。程序实际保存 SQLite、文档、报告和历史；运行 Host/Web 不需要 Rust、Node、数据库服务或源码目录；真实工程的外部运行依赖另行安装。

也可以直接打开已有数据集：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

同一数据集只能有一个可写程序。退出可在 Web 设置页操作，或在终端按 Ctrl+C。重新执行相同命令可读取已有数据；未确认完成的执行会标记中断，可显式重试。更换数据目录需退出后重新启动，不会隐式迁移文件。

包名中的目标架构应与机器匹配。macOS Apple Silicon 为 `aarch64-apple-darwin`；本轮 macOS 验证版本为 26.7.2。当前提供本地命令行程序；没有制作签名、公证的 `.app` 安装器。

模拟项目保留全部 Sim 流程。真实工程使用独立登记的 Codex/DSH、Git/GitHub、Docker；需额外安装 Docker Engine/Desktop、Git、GitHub CLI，并配置 OAuth/API key。运行包不按角色内嵌运行时；`containers/` 提供固定构建配方。真实流程及配置见 `docs/real-engineering.md`。Archive 与迁移只预览。完整步骤见运行包内 `docs/demo.md`，平台证据见 `docs/acceptance.md`。

macOS 真实 DSH 凭据由运行包中的一份固定签名助手访问，首次运行安装到用户私有共享目录，Host 更新复用该助手。普通访问不会弹出 Keychain 授权；旧密钥可在设置页显式授权一次。Linux 使用 Secret Service，需安装 `libsecret-tools`、GNOME Keyring 或兼容服务，并在可用且已解锁的桌面 D-Bus 会话运行。Ubuntu 24.04.5 / x86_64 包已通过源码外 HTTP/SSE、系统凭据、双 Harness 各两轮原生续聊、完成项目与 Snapshot 重启检查；模型工程需要下述专用 AppArmor 配置；当前完整结果见 `docs/plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md`。详见 `docs/system-credentials.md`。

Ubuntu 的 Docker 若启用 AppArmor，真实 Harness 还需要经审阅安装包内的专用嵌套沙箱策略：`sudo ./scripts/install-linux-sandbox-policy.sh --install`。安装器只加载 `devdesk-harness-v1`，不修改 Docker 默认策略。Host 校验已安装文件的 root 归属、权限和固定摘要；缺失/漂移会明确阻塞。策略与隔离边界见 `containers/security/README.md`。
