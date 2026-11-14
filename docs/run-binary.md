# DevDesk 0.2 本地运行包

## 启动与依赖

解压与当前机器架构匹配的运行包，在包目录执行：

```bash
./devdesk-host
```

在浏览器打开终端显示的地址，默认是 `http://127.0.0.1:4317`。首次选择空目录或兼容的 DevDesk 数据集，也可显式指定：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

路径可含中文和空格。Host 嵌入 Web 和出厂资源，运行工作台不需要 Rust、宿主 Node、数据库服务或源码。实际工程执行需要已启动的 Linux Docker Engine/Desktop、Git 与已登录的 GitHub CLI；模型认证在设置中完成。登录组件及 worker 运行资源按实际所选配置下载/校验、构建或拉取，不要求手工登记镜像。

本机运行包由 `scripts/release/package.sh` 构建，名称为 `devdesk-0.2.0-<rust-host-target>.tar.gz`，附 SHA-256 文件。macOS Apple Silicon 的 target 为 `aarch64-apple-darwin`；其他机器使用对应平台产物。macOS 包是带凭据助手的签名命令行程序，不是已公证的 `.app` 安装器。

## 数据集与恢复

数据集标记为 `devdesk-source-flat-v1`，SQLite 基线为 global 18 / project 48。兼容数据集可直接重开；旧协议/旧结构明确拒绝，不自动迁移、删除或重建。验证新代码使用独立空目录，不把清空现有数据当作恢复手段。

初始化先取得 `.devdesk.lock` 独占锁。同一数据集只能有一个可写 Host；非空非 DevDesk 目录不初始化。仅剩 `.devdesk-dataset`、`.devdesk.lock` 和 `.DS_Store` 的无数据目录可初始化当前结构，含数据库、认证或产物的目录不能按空目录处理。

退出可用 Web 设置页或 Ctrl+C。重开时保留历史，未确认终态的副作用先进入核对，不能直接当作可重发的失败。切换项目或关闭浏览器不取消后台工作；更换数据路径需退出 Host 后重新启动，不隐式搬迁。

## 连接远程内核

设置、初始化页及连接失败页提供“连接内核”。输入 `http(s)://ip:port` 后先探测 `/connection`，确认产品与协议，再导航到目标内核的同源工作台。数据、凭据、Docker 和 Git 操作都位于目标内核机器。

远程监听须先通过进程环境设置 `DEVDESK_ACCESS_TOKEN`，值为 32–512 个非空白 ASCII 字符，再启动：

```bash
./devdesk-host --host 192.168.1.10 --port 4317 --data "/完整路径/DevDesk 数据"
```

客户端输入同一连接密钥，Host 建立 HttpOnly、SameSite=Strict 会话。密钥不进入 URL 或浏览器持久存储；Host 与有值的 Origin 必须匹配配置。默认仅监听 loopback，配置公开地址或非 loopback 监听时要求密钥。

HTTPS 可由反向代理提供，并设置 `--public-url https://desk.example`；代理须传递匹配的 Host/Origin。监听 `0.0.0.0` 或 `::` 时必须指定公开地址。HTTP 仅用于可信网络或隧道。

## 模型、权限与平台设置

工程从“打开 GitHub 项目”接入仓库与分支，后台克隆并登记工作空间。项目选择 Workflow、Standard、Role→Agent、Permission Context 及 manual/rust/agent 驱动。Codex 当前提供经理/编排原生工具桥；DSH 支持所配置的普通模型会话，不能承担需要该桥的控制会话。

运行资源来自不可变配置源，以配方、实际镜像和 driver/权限资产核验；准备成功只证明协议探针通过，不证明模型推理或实际工具执行成功。已有运行和技术重试保留原冻结资源。

macOS API key 和工具 token 由固定签名 `devdesk-credential-helper` 访问 Keychain。包提供一份助手，首次需要时安装到用户私有共享目录；当前要求协议 2、内容摘要及同证书签名。普通访问不自动弹窗，设置页提供显式钥匙串授权。兼容助手复用，不随 Host 构建替换；需要显式升级时使用包含对应助手的包执行：

```bash
./devdesk-host --update-credential-helper
```

Linux 需要 `secret-tool`、可用且解锁的 Secret Service/D-Bus 会话，例如 `libsecret-tools` 与 GNOME Keyring；不使用 macOS 助手，不降级为明文 API key 文件。Codex OAuth 另用数据集私有认证文件。

默认权限将当前 Harness 容器作为信任范围，步骤授权进一步限制文件与工具；宿主系统凭据库、其他项目和未挂载路径在容器外。包旁权限预设优先加载，未附带时由内嵌资源物化到数据集缓存。修改预设请使用自己的目录；生成缓存不是编辑源。

Linux 启用 AppArmor 且选择受限嵌套权限时，需经审阅安装随包专用策略：

```bash
sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install
```

安装器只加载 `devdesk-harness-v1`，Host 校验归属、权限与资产摘要；缺失或漂移明确阻塞。默认全权限绑定不要求该嵌套策略。

本页也复制为运行包根目录的 README。包根目录下 `docs/real-engineering.md`、`docs/system-credentials.md`、`docs/permissions.md`、`docs/storage.md` 和 `permissions/README.md` 提供详细操作与边界；`docs/plans/` 是历史记录，不代表当前机器已通过同样验收。

开发源码依据：`crates/host/src/main.rs`、`crates/host/src/workspace.rs`、`crates/adapters/src/runtime_preparation.rs`、`crates/adapters/src/credentials_macos.rs`、`scripts/release/package.sh`。
