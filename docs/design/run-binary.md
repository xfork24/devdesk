# DevDesk v0.5 运行指南

Host、Web Core 和领域 ZIP 独立发行。运行已打包的制品无需 Python、Rust、宿主 Node 或数据库服务。

## 启动

解压与操作系统和 CPU 匹配的 Host。API 客户端可以直接连接仅启动 Host 的工作台。

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --port 4317
```

运行 Web 工作台时解压 Web Core，并加上 `--web-dir`。终端会显示连接地址，默认是 `http://127.0.0.1:4317`。通过 Web 首次初始化时，可省略 `--data` 并在页面选择空目录。macOS 使用可访问 Keychain 的签名 Host。见[签名与凭据](../crates/devdesk-adapters/signing-and-keychain.md)。

## 领域工作台独立发行

解压与操作系统及 CPU 匹配的 `devdesk-core-host-0.5.0-<target>.tar.gz` 和 `devdesk-core-web-0.5.0.tar.gz`。启动后，在工作台「领域管理」页导入并激活 `software-development-0.5.5.zip`。发行渠道提供的 SHA-256 校验文件可用于核对下载内容。

```bash
./devdesk-host --data /path/to/new-dataset --web-dir /path/to/devdesk-core-web-0.5.0
```

空领域工作台提供核心配置、历史记录和设置。Git 查询通过核心 RPC 提供。领域包支持运行时导入、激活和卸载。切换更新所有客户端的活动代次，并检查在途工作流和待核对的仓库操作。只要该领域没有正在执行的工作流，即可卸载。同一领域的不同版本共享项目目录和历史，打开同一 GitHub 仓库及主分支会复用已有项目。运行领域处理器需要可用的 Docker 及下文的固定 Node 镜像。

打开数据集时，如果原活动领域设置的数据目录找不到，Host 会进入空领域。可以在「领域管理」导入并激活该包，再到领域工作台的「设置 → 数据与运行」指定并验证可用目录。若原项目数据库已移到所选目录，Host 会核对项目 ID 和仓库归属，修复当前 Git 工作树路径，并按原项目 ID 打开。保留原项目数据库的目录继续承担该项目的存储。

```bash
docker pull node@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f
```

| 工件 | 合同和兼容性 |
| --- | --- |
| Core Host 0.5.0 | JSON-RPC 2.0、领域协议 1、领域包清单格式 1。支持既有贡献投影及按原摘要回切 |
| Web Core 0.5.0 | 同时支持 `web.react` API 1 与 API 2。API 1 使用早期动态模块接口。API 2 提供共享 React 实例、组件、布局、项目 RPC 和事件作用域。模块从 `/assets/devdesk-ui.mjs` 获取运行时，类型声明和 JSON Schema 随 Web Core 放在 `sdk/` |
| 软件开发 ZIP 0.5.2 | 角色讨论使用 Role 与 Agent 定义，按角色汇总经验。需要支持 `query.role_experience` 的 Host |
| 软件开发 ZIP 0.5.3 | 可选跨 Harness 软件工程 Standard，要求 Host 已接入 Pi、DSH、OpenCode 与 mcode |
| 软件开发 ZIP 0.5.4 | 跨 Harness Standard 使用本机准备的 ZCode 执行 Feature 最终交付评审 |
| 软件开发 ZIP 0.5.5 | 新的跨 Harness 交付评审 Agent 使用从官方 CLI 源码本地构建的 ZCode |
| 软件开发 ZIP 0.5.1 | 使用 API 2 呈现绑定和工作流显式交付证据合同。需要支持 `operation_evidence` 的 Host。可与同一领域的旧版 ZIP 一起安装 |

非浏览器客户端先调用 `POST /session` 建立 Host 会话，随后从 `query.domain.active` 读取活动代次和贡献目录，并监听 `GET /domain-events`。项目事件流和已有项目 RPC 携带 `project_id` 及 `domain_epoch`。声明 `projection: true` 的贡献用 `query.domain.contribution` 读取，其他贡献的包处理器沿用 `query.domain.invoke`。

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "query.domain.contribution",
  "params": {
    "epoch": 4,
    "contribution": "software-development.branches",
    "input": {
      "project_id": "b510137f-93ce-48d4-927e-d069e39a6701",
      "feature_id": "d66ba20f-74b6-4439-8498-bd7c7c10d3ac",
      "task_offset": 0
    }
  }
}
```

请求示例中的代次、项目和 Feature 来自当前活动目录和项目数据。前端可选择适合自身平台的呈现绑定并使用同一贡献目录及 RPC。激活包的 Web 插件与 Web Core 共享当前浏览器会话和 DOM。

## 数据与退出

同一数据集只能由一个 Host 管理。生产数据标记为 `devdesk-profile-runtime-v2`，全局布局为 `single-host-domain-packages-v1`，数据库基线为 global 25 / project 62。领域平台的 global 24 数据集须先执行离线升级。首次从单体安装切换时，请为 v0.5 指定新的数据目录并保留原数据。兼容的工程库存储修订 60、61 可按下述方式显式迁移。

停止该数据集的 Host 后，可以执行显式维护命令。

```sh
devdesk-host storage upgrade-global --data /path/to/data
devdesk-host storage migrate --data /path/to/data --project <project-id>
devdesk-host storage compact --data /path/to/data --project <project-id>
devdesk-host storage rebuild-workflow-display --data /path/to/data --project <project-id>
```

全局升级命令持有数据集独占锁，先在数据集外备份 global 24，再升级安装表并保留已有安装。输出包含源版本、目标版本、安装行数与备份路径。

项目迁移命令接受 project 60 和 61，回填源依赖索引并输出回填数量、事件位置、迁移前后的工程库大小和备份路径。全局讨论库使用项目参数 `00000000-0000-4000-8000-000000000000`。归档命令将仍被引用的对象合并，核验每条索引对应的正文后清理散文件。恢复旧版时使用迁移输出的备份。正常备份新版项目时，一起保存工程库、`artifacts/` 和 `payloads/`。

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

仓库接入期间可以继续保存项目配置。Host 核对仓库、主干和工作区绑定，并将已经保存的 Git 回执应用到最新项目修订。旧记录中因项目修订冲突而挂起的接入也会重新核对这些条件，复用原回执。

## Linux 权限策略

Host 内置权限运行资产。使用受限嵌套权限且启用 AppArmor 时，在 Docker 宿主上审阅、检查并安装内置专用策略。

```bash
./devdesk-host sandbox-policy --print
./devdesk-host sandbox-policy --check
sudo ./devdesk-host sandbox-policy --install
```

安装器只加载 DevDesk 专用策略，Host 会校验资产和权限。默认全权限绑定不要求该嵌套策略。缺少必需后端时应修复环境或明确选择适合的权限配置，不能把启动失败视为已隔离运行。

## 远程连接

默认只监听 loopback。远程监听前设置 `DEVDESK_ACCESS_TOKEN`，要求 32–512 个非空白 ASCII 字符，再运行：

```bash
./devdesk-host --host 192.168.1.10 --port 4317 --data "/完整路径/DevDesk 数据" \
  --web-dir ./devdesk-core-web-0.5.0
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

设计与开发入口收录在[仓库文档索引](../README.md)。
