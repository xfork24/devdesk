# Web Host 的系统凭据

macOS 与 Linux 使用相同 Web/Rust 工程接口；没有依赖 macOS 原生客户端。秘密不写入 SQLite、命令参数、报告或浏览器持久存储；工程库只保存数据集范围的不透明引用。

## Permission 与凭据信任范围

默认绑定使用 v2 权限档案 `builtin-permission-harness-full-access` 与 `builtin-permission-sandbox-full-access`（见 [运行时权限](permissions.md)），将整个 Harness 容器视为信任范围，同容器工具可以访问该执行的模型凭据和运行状态。宿主系统凭据库、其他项目和未挂载路径仍在容器外。Permission 审批与操作系统凭据授权分开：选择 `never` 不会授予 Keychain 或 Secret Service 权限。受限后端的状态隔离必须由实际执行器落实，能力和假凭据验证见 [权限后端](../permissions/README.md) 与 [执行报告](plans/permission-standardization/EXECUTION.md)。

## macOS：固定凭据助手

macOS 对本地自签名程序的 Keychain 分区使用构建的 cdhash。实际试验表明：固定证书和 designated requirement 后，重新构建 Host 仍可能失去访问权；新建测试条目也会复现。原因见 Apple 的 [client identity 实现](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp)。因此仅固定 Host 签名不能作为完成修复的证据。

当前由独立的 `devdesk-credential-helper` 通过 Security.framework 访问 Keychain。它在用户私有目录 `~/Library/Application Support/DevDesk/credential-helper/v1/` 安装一次，所有 Host 构建、项目和角色共用；Host/Web/Core 变化不会重建它。安装清单保存源/依赖身份与实际二进制摘要，内容或签名不匹配时阻塞。运行包只包含一份助手，首次启动可将其安装到该共享目录，不按项目或角色复制。

Host 在调用前检查助手归属、权限、内容和同证书签名；助手反向检查直接调用者为对应证书签名的 `dev.devdesk.host`。普通脚本或其他应用直接调用助手会在访问 Keychain 前被拒绝。服务名、账号、操作类型与输入长度均受限。秘密只通过私有 stdin/stdout 管道传递，Host 消费后不返回给 HTTP，也不进入协议日志。没有给通用 security CLI 或所有应用授予读取权限。

普通操作禁止 Keychain 自动显示授权 UI。未授权/锁定返回可读错误；只有设置页“授权已保存的钥匙串凭据”允许固定助手弹出系统授权，最长等待 30 秒。系统密码只在系统窗口输入。旧条目需要对固定助手选择一次“始终允许”；之后更换 Host 构建不改变访问 Keychain 的助手二进制。锁定、撤销权限或显式升级助手仍需相应系统处理。

一次性开发设置：

```bash
./scripts/platform/macos/setup-macos-signing.sh --prepare
./scripts/platform/macos/setup-macos-signing.sh --import
./scripts/platform/macos/install-macos-credential-helper.sh
./scripts/dev/dev.sh --data /absolute/path/to/data --port 51751
```

开发和发行 Host 使用稳定 identifier `dev.devdesk.host`；助手使用 `dev.devdesk.credentials`，并验证调用者证书。脚本仅生成/导入 `DevDesk Local Development` 开发证书，信任限定为用户域 code signing，私钥只授权 codesign。证书材料在 `~/Library/Application Support/DevDesk/signing/`（目录 0700、私钥 0600），不在源码或发行包中。这不是 Developer ID 发行签名或公证。

也可通过 `DEVDESK_CODESIGN_IDENTITY` 指定已有身份；必要时用 `DEVDESK_CODESIGN_CERTIFICATE_SHA1` 指定对应证书指纹。`scripts/dev/dev.sh` 与 `scripts/dev/build.sh` 以 `--ensure` 检查助手，并为 Host 签名。已有助手的协议、内容与同证书签名通过时，即使源码或依赖身份变化也继续复用，不阻塞普通启动。启用需要新版助手的功能时显式运行安装器 `--update`；该变化可能需要一次新的助手授权，不静默覆盖已授权安装。

## Ubuntu / Linux

Linux 使用 Secret Service/secret-tool，不需要 macOS 凭据助手。安装 `libsecret-tools`，并确保当前桌面会话提供已解锁的 Secret Service（例如 GNOME Keyring）。设置页通过 secret-tool 的私有 stdin 保存 API key；实际使用或显式检查时才读取，状态页只使用配置引用。

```bash
sudo apt-get install libsecret-tools gnome-keyring
```

缺少 secret-tool、D-Bus/桌面会话或锁定凭据库时明确阻塞，不降级为明文文件或 Sim。Headless/SSH 不自动具有桌面凭据库。行为依据 [libsecret](https://gnome.pages.gitlab.gnome.org/libsecret/) 与 [Ubuntu secret-tool 手册](https://manpages.ubuntu.com/manpages/noble/man1/secret-tool.1.html)。

Codex 仍使用数据集专属官方 OAuth 文件和受控 Session 凭据。平台和绝对数据目录变化会改变系统凭据命名空间；换机器后重新配置本机凭据，不能把 macOS 引用当作 Linux 上的秘密。

macOS 开发入口 `scripts/dev/dev.sh` 通过 `scripts/dev/run-host.py` 从私有临时目录中的签名副本运行 Host，退出后清理；程序旁的凭据助手和权限预设也随副本保留。后续 Cargo 构建、签名不会覆盖运行中的程序，因此不会使凭据助手的调用方核验失效。也可用 `python3 scripts/dev/run-host.py target/release/devdesk-host --data /absolute/path/to/data` 启动已构建的版本。

直接运行 `target/debug/devdesk-host` 后再覆盖其二进制，仍会使旧进程的签名核验失败。遇到“无法核验当前 Host 签名”时，停止该旧进程并通过上述入口启动；不需要重新授权助手或重新保存 key。

## DSH 多服务商凭据

DeepSeek、`zai-coding-cn`、`minimax-cn` 使用独立账号命名空间和当前引用，读取时校验数据集及服务商。DeepSeek 使用显式 UUID 凭据引用，未配置时不探测旧固定账号；换 key 不覆盖已冻结执行的引用。Ollama 本地服务免密，不创建 vault 条目。macOS 只接受协议版本 2 的助手，升级步骤见 [模型服务商](model-providers.md)；打包仅构建随包助手，不修改本机共享安装。
