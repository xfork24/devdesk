# DevDesk 运行指南

Host 已嵌入 Web 工作台和出厂配置，启动工作台无需 Python、Rust、Node 或单独的数据库服务。

## 启动

解压与操作系统、CPU 架构匹配的包，在包目录运行：

```bash
./devdesk-host
```

打开终端给出的地址，默认是 `http://127.0.0.1:4317`。首次选择空数据目录，也可直接指定：

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

工程执行还需要可用的 Linux Docker Engine/Desktop、Git，以及 GitHub CLI。GitHub 和模型认证可在设置中完成。执行安装由所选 Harness/Sandbox 配置准备，不必手工猜测镜像标签。

macOS 压缩包只包含签名后的 `devdesk-host` 命令行程序。

## 数据与退出

同一数据集只能由一个 Host 管理。当前数据标记为 `devdesk-profile-runtime-v2`，布局为 `single-host-resource-v2`，数据库基线为 global 24 / project 62。程序兼容打开 project 60 和 61，旧工程库在显式迁移前沿用保守的模板重置扫描。

停止该数据集的 Host 后，可以执行显式维护命令。

```sh
devdesk-host storage migrate --data /path/to/data --project <project-id>
devdesk-host storage compact --data /path/to/data --project <project-id>
devdesk-host storage rebuild-workflow-display --data /path/to/data --project <project-id>
```

迁移命令接受 project 60 和 61，回填源依赖索引并输出回填数量、事件位置、迁移前后的工程库大小和备份路径。全局讨论库使用项目参数 `00000000-0000-4000-8000-000000000000`。归档命令将仍被引用的对象合并，核验每条索引对应的正文后清理散文件。恢复旧版时使用迁移输出的备份。正常备份新版项目时，一起保存工程库、`artifacts/` 和 `payloads/`。

流程展示回填使用固定的项目事件位置，从已提交的流程实例和步骤版本批量恢复展示表。回填事务提交前核对数量及精确引用，可用于 project 60、61 和 62。

遇到不兼容错误时保留原数据。验证新版本请使用独立空目录，不以清空旧目录作为恢复步骤。切换数据目录需退出后重新启动。

退出可用工作台设置或 Ctrl+C。Host 停止新的调度工作，并核对它管理的 worker 是否停止。关闭浏览器、切换项目或退出 Host 都不会回滚已经发生的 Git/远端操作；重开后系统会核对未确认的执行。

## 模型与运行准备

在设置中配置模型认证，选择 Agent、Harness、Sandbox 和权限。安装检查成功说明程序与协议准备完成，仍需实际会话验证模型连接和工具调用。

当前 Codex 支持根经理编排。DSH 可以运行所支持的普通模型会话和步骤，不能仅凭工具桥存在就选择它承担需要根编排能力的会话。

Ollama 可填写本机服务根地址。容器必须实际访问到该服务；Linux 的 host-gateway 不会自动开放只监听 loopback 的进程。更改地址后保存并采用新的 Model/Agent 组合，原会话继续使用原连接。

## 系统凭据

macOS 由签名后的 `devdesk-host` 直接访问钥匙串。旧版创建的条目可以继续使用。若 macOS 要求确认新的访问者，可在模型服务商设置中执行钥匙串授权。GitHub 和其他集成的显式授权命令见 [Host 签名与钥匙串](../crates/devdesk-adapters/signing-and-keychain.md)。

Linux 需要 `secret-tool` 和已解锁的 Secret Service/D-Bus 会话，例如 libsecret-tools 与 GNOME Keyring。不降级为明文 API key 文件。Codex OAuth 使用数据集私有认证文件，Ollama 默认免密。

在“设置 → GitHub 连接”中检查账号或保存个人访问令牌。DevDesk 验证账号后，将 Token 写入当前数据集对应的系统凭据库。未保存 Token 时使用 Host 的 `GH_TOKEN`、`GITHUB_TOKEN` 或 GitHub CLI 登录。如果认证在克隆开始前失败，更新凭据后可在原项目中点击“核对并重试原操作”。

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
| GitHub 仓库接入失败 | 设置中的 GitHub 连接状态、目标仓库权限和网络 |
| 步骤等待而无进展 | 是等待输入、等待人工确认、预算阻止，还是外部结果待核对 |
| 远程工作台拒绝连接 | 连接密钥、公开地址与 Host/Origin 是否匹配 |

记录对应项目、执行、会话和错误信息后再处理。不要用反复创建新执行或删除数据掩盖原故障。

包内 `docs/design/` 提供架构说明，`docs/crates/` 提供开发阅读路线。代码链接需要源码仓库；本页的运行步骤不依赖这些链接。
