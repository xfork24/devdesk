# DevDesk 运行指南

本页可在独立运行包中阅读。Host 已嵌入 Web 工作台和出厂配置；启动工作台不需要 Rust、Node 或单独的数据库服务。

## 启动

解压与操作系统、CPU 架构匹配的包，在包目录运行：

```bash
./devdesk-host
```

打开终端给出的地址，默认是 `http://127.0.0.1:4317`。首次选择空数据目录，也可直接指定：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

工程执行还需要可用的 Linux Docker Engine/Desktop、Git，以及按功能需要登录的 GitHub CLI。模型认证在设置中完成。执行安装由所选 Harness/Sandbox 配置准备，不必手工猜测镜像标签。

macOS 包是包含凭据助手的签名命令行程序，不是已公证的 App 安装器。

## 数据与退出

同一数据集只能由一个 Host 管理。当前数据标记为 `devdesk-profile-runtime-v2`，布局为 `single-host-resource-v2`，数据库基线为 global 24 / project 60。程序打开兼容数据集或初始化空目录，不自动迁移、删库或覆盖非 DevDesk 目录。

遇到不兼容错误时保留原数据。验证新版本请使用独立空目录，不以清空旧目录作为恢复步骤。切换数据目录需退出后重新启动。

退出可用工作台设置或 Ctrl+C。Host 停止新的调度工作，并核对它管理的 worker 是否停止。关闭浏览器、切换项目或退出 Host 都不会回滚已经发生的 Git/远端操作；重开后系统会核对未确认的执行。

## 模型与运行准备

在设置中配置模型认证，选择 Agent、Harness、Sandbox 和权限。安装检查成功说明程序与协议准备完成，仍需实际会话验证模型连接和工具调用。

当前 Codex 支持根经理编排。DSH 可以运行所支持的普通模型会话和步骤，不能仅凭工具桥存在就选择它承担需要根编排能力的会话。

Ollama 可填写本机服务根地址。容器必须实际访问到该服务；Linux 的 host-gateway 不会自动开放只监听 loopback 的进程。更改地址后保存并采用新的 Model/Agent 组合，原会话继续使用原连接。

## 系统凭据

macOS 通过固定签名的 `devdesk-credential-helper` 访问 Keychain。Host 检查助手协议、内容和签名；普通读取不会自动弹出授权窗口。需要授权时，在设置中执行明确的钥匙串授权操作。

若运行包携带了需要升级的助手，显式执行：

```bash
./devdesk-host --update-credential-helper
```

当前助手要求协议 2。普通构建和正常启动不会任意替换已授权安装。安装目录名称不代表协议版本。

Linux 需要 `secret-tool` 和已解锁的 Secret Service/D-Bus 会话，例如 libsecret-tools 与 GNOME Keyring。不降级为明文 API key 文件。Codex OAuth 使用数据集私有认证文件，Ollama 默认免密。

## Linux 权限策略

使用受限嵌套权限且启用 AppArmor 时，经审阅安装随包专用策略：

```bash
sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install
```

安装器只加载 DevDesk 专用策略，Host 会校验资产和权限。默认全权限绑定不要求该嵌套策略。缺少必需后端时应修复环境或明确选择适合的权限配置，不能把启动失败视为已隔离运行。

## 远程连接

默认只监听 loopback。远程监听前设置 `DEVDESK_ACCESS_TOKEN`，要求 32–512 个非空白 ASCII 字符，再运行：

```bash
./devdesk-host --host 192.168.1.10 --port 4317 --data "/完整路径/DevDesk 数据"
```

客户端通过“连接内核”探测并打开目标工作台，输入同一连接密钥。数据、Docker、Git 和凭据均位于目标 Host 机器。

反向代理提供 HTTPS 时配置 `--public-url https://desk.example`，并传递匹配的 Host/Origin。监听 `0.0.0.0` 或 `::` 必须指定公开地址。明文 HTTP 只用于可信网络或隧道。

## 按症状排查

| 症状 | 优先检查 |
|---|---|
| 数据集被占用 | 是否已有 Host 使用相同目录，避免并行启动 |
| 安装或容器启动失败 | Docker 是否运行、挂载是否允许、镜像及权限策略是否可用 |
| 模型连接失败 | 所选配置版本、系统凭据授权、容器网络及服务地址 |
| 步骤等待而无进展 | 是等待输入、等待人工确认、预算阻止，还是外部结果待核对 |
| 远程工作台拒绝连接 | 连接密钥、公开地址与 Host/Origin 是否匹配 |

记录对应项目、执行、会话和错误信息后再处理。不要用反复创建新执行或删除数据掩盖原故障。

包内 `docs/design/` 提供架构说明，`docs/crates/` 提供开发阅读路线。代码链接需要源码仓库；本页的运行步骤不依赖这些链接。
