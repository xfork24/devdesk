# DevDesk 0.2 本地运行包

解压后，在本目录执行：

```bash
./devdesk-host
```

在浏览器打开终端显示的 `http://127.0.0.1:4317`，输入空目录或已有 DevDesk 数据集路径。路径可包含中文和空格。程序实际保存 SQLite、文档、报告和历史；运行 Host/Web 不需要 Rust、Node、数据库服务或源码目录；Harness 组件与镜像由 DevDesk 按需准备；宿主需可用的 Docker、Git、GitHub CLI。

也可以直接打开已有数据集：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

设置中的“连接内核”接受 `http://ip:port` 或 HTTPS 地址。连接前检查产品和协议，成功后打开目标内核的工作台；连接失败保留原工作台，退出内核后也可连接其他地址。数据目录、运行时及工程操作均位于所连接的内核主机。

要部署到其他机器，先在内核进程环境中配置 `DEVDESK_ACCESS_TOKEN`，值为 32–512 位随机非空白 ASCII 连接密钥，再显式指定该机器的 IP：

```bash
./devdesk-host --host 192.168.1.10 --port 4317 --data "/完整路径/DevDesk 数据"
```

客户端首次连接时输入同一密钥；密钥不通过 URL 传递，也不保存在浏览器持久存储中。默认本机启动仍可直接使用。HTTP 部署适用于可信网络或隧道；需要 HTTPS 时可通过反向代理提供，并用 `--public-url https://desk.example` 指定公开地址，代理保留 Host/Origin。监听 `0.0.0.0` 或 `::` 时必须同时指定 `--public-url`。

同一数据集只能有一个可写程序。退出可在 Web 设置页操作，或在终端按 Ctrl+C。重新执行相同命令可读取已有数据；未确认完成的执行会标记中断，可显式重试。更换数据目录需退出后重新启动，不会隐式迁移文件。

当前运行合同为 Workflow v5（global 14 / project 45）。旧 v4 数据集需先停止原 Host 并核对活动执行，再使用包内离线入口；第一次运行只预演：

```bash
python3 scripts/upgrade/harness-v5.py "/完整路径/DevDesk 数据" --binary ./devdesk-host
python3 scripts/upgrade/harness-v5.py "/完整路径/DevDesk 数据" --binary ./devdesk-host --apply --backup-directory "/数据集外的空备份目录"
```

转换保留原记录、产物、凭据和采用的业务图；模板目录曾重置时也保留其代次及逻辑身份。新版默认七阶段模板可单独采用，转换不会隐式替换旧实例的业务步骤。转换中断后重跑同一 `--apply` 命令续接 journal；未完成时 Host 会阻止启动。当前 Codex 支持项目经理原生编排，DSH 在补齐工具桥前不能承担编排会话。

包名中的目标架构应与机器匹配。macOS Apple Silicon 为 `aarch64-apple-darwin`；本轮 macOS 验证版本为 26.7.2。当前提供本地命令行程序；没有制作签名、公证的 `.app` 安装器。

Web 统一使用 GitHub 项目：点击“打开 GitHub 项目”填写仓库与分支，自动检查并克隆；账号 OAuth/API key 在设置中配置。DevDesk 根据讨论或任务实际选中的 Harness 自动下载、校验并准备共享运行时，配方已嵌入程序，无需手动构建镜像或登记安装。宿主需安装并启动 Docker Engine/Desktop、Git、GitHub CLI，并完成 GitHub 登录。已有模拟历史保留在后端，不进入 Web 项目列表。流程与配置见 `docs/real-engineering.md`。

macOS 真实 DSH 凭据由运行包中的一份固定签名助手访问，首次运行安装到用户私有共享目录，Host 更新复用该助手。普通访问不会弹出 Keychain 授权；旧密钥可在设置页显式授权一次。Linux 使用 Secret Service，需安装 `libsecret-tools`、GNOME Keyring 或兼容服务，并在可用且已解锁的桌面 D-Bus 会话运行。Ubuntu 24.04.5 / x86_64 包已通过源码外 HTTP/SSE、系统凭据、双 Harness 各两轮原生续聊、完成项目与 Snapshot 重启检查；模型工程需要下述专用 AppArmor 配置；当前完整结果见 `docs/plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md`。详见 `docs/system-credentials.md`。

Ubuntu 的 Docker 若启用 AppArmor，选择受限嵌套 Permission 的 Harness 还需要经审阅安装包内的专用嵌套沙箱策略：`sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install`。安装器只加载 `devdesk-harness-v1`，不修改 Docker 默认策略。Host 校验已安装文件的 root 归属、权限和固定摘要；缺失/漂移会明确阻塞。策略与隔离边界见包内 `permissions/README.md`。

运行包内 `permissions/` 保存规范化预设及后端，`schemas/` 提供结构定义。默认全权限不要求安装嵌套 AppArmor；旧数据集继续使用原受限绑定，可在设置页预检并选择后续会话的权限。
