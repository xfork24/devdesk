# Ubuntu Web 验收入口

本轮 macOS 的真实工程报告在 [R8](plans/real-engineering-integration/R8-acceptance.md)。此文是 Ubuntu 待执行步骤；当前不能据此宣称 Linux 宿主已通过。Web UI 是本轮客户端，macOS native 客户端延后。

完整执行顺序、独立报告目录、逐阶段通过条件和收尾要求见 [Ubuntu Web 真实工程验收计划](plans/ubuntu-web-acceptance/PLAN.md)。切换 Ubuntu 后按该计划执行；下面保留简要命令入口。

## 准备

安装仓库固定的 Rust/Node 工具链、Git、GitHub CLI、Docker Engine，以及 `libsecret-tools` 和桌面 Secret Service（例如 GNOME Keyring）。用户需具备 Docker 权限，并在当前桌面会话使用已解锁的凭据库。Headless SSH 不自动具备桌面 D-Bus/Secret Service。

```bash
npm --prefix web ci
./scripts/check.sh
# 在本机 Docker 平台构建；同版本各角色共用镜像，不复制 macOS 二进制
 docker build -t devdesk/codex:0.167.0 containers/codex
 docker build -t devdesk/dsh:0.2.2-rc.1 containers/dsh
 gh auth login
./scripts/doctor.sh --output docs/plans/ubuntu-web-acceptance/preflight.json
cargo build --locked -p devdesk-host
./target/debug/devdesk-host --data /absolute/path/to/dedicated-live-data --port 51751
```

打开终端显示的 Web 地址，在设置页登记本机 Codex、完成官方 OAuth；通过密钥输入框保存 DeepSeek key 到本机 Secret Service。登记两套 Linux Docker 镜像，检查默认 Codex/角色偏好、凭据访问和 GitHub 仓库诊断。不要把密钥发送到聊天或写进验收脚本。

macOS/aarch64、Linux/aarch64、Linux/x86_64 是不同安装键。不能复制旧的本机安装 ID 或用另一平台的固定镜像 ID 代替本机产物。真实资源测试默认解析本机固定版本镜像，再冻结不可变 ID；也可显式设置 `DEVDESK_TEST_CODEX_IMAGE`、`DEVDESK_TEST_DSH_IMAGE`。

## 实际验证

```bash
cargo test --locked -p devdesk-adapters --test command_tests --test docker_resources --test git_resources -- --include-ignored
./scripts/check-live.sh --mode full --providers codex,dsh \
  --root /absolute/path/to/dedicated-live-data \
  --github-repo owner/dedicated-test-repo \
  --url http://127.0.0.1:51751 --allow-host-restart
./scripts/package.sh
```

完整 live 模式使用新报告目录和随机源码前缀，执行模型调用、创建真实 Feature/Task、PR 并接纳至专用仓库主干，还会 SIGKILL/重启唯一匹配数据集和端口的验收 Host。只使用专用测试数据集/仓库，不能以 DevDesk 自身仓库验收。缺配置、权限、镜像或恢复授权以非零退出，不把 skip 视为成功。

按组诊断可使用 `--groups git,docker,harness,github,web,workflow,recovery` 的子集；完整工程验收需要全部组和两供应商。`--mode verify-existing --reports <报告目录>` 是对已经执行过的外部对象/产物重新核验，不替代首次真实执行。

报告需另存 Ubuntu 实际 OS/CPU、依赖、命令退出状态、双供应商原生 Session、资源/停止证明、PR/候选 SHA、浏览器截图和打包 SHA256。旧 macOS 数据保留历史，不改写其平台/凭据引用来伪造 Ubuntu 实例。
