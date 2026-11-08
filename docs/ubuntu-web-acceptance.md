# Ubuntu Web 验收入口

本轮 macOS 的真实工程报告在 [R8](plans/real-engineering-integration/R8-acceptance.md)。Ubuntu 已执行 [本轮实机检查](plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)：U0–U6 全部通过，包含双 Harness 工程闭环、真实 PR merged、系统凭据、安装复用、取消/崩溃恢复和独立发布包；AppArmor Docker 宿主使用经批准的专用策略。Web UI 是本轮客户端，macOS native 客户端延后。

完整执行顺序、独立报告目录、逐阶段通过条件和收尾要求见 [Ubuntu Web 真实工程验收计划](plans/ubuntu-web-acceptance/PLAN.md)。下面保留新一轮验收的简要命令入口；本轮失败及显式续跑历史均在独立报告中保留。

## 准备

安装仓库固定的 Rust/Node 工具链、Git、GitHub CLI、Docker Engine，以及 `libsecret-tools` 和桌面 Secret Service（例如 GNOME Keyring）。用户需具备 Docker 权限，并在当前桌面会话使用已解锁的凭据库。Headless SSH 不自动具备桌面 D-Bus/Secret Service。

```bash
npm --prefix web ci
./scripts/check.sh
# 在本机 Docker 平台构建；同版本各角色共用镜像，不复制 macOS 二进制
 docker build -t devdesk/codex:0.167.0 containers/codex
 docker build -f containers/dsh/Dockerfile -t devdesk/dsh:0.2.2-rc.1-permissions-v1 .
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

## 无模型凭据的 Linux 诊断

```bash
# 从本机固定镜像解析不可变 ID；不自动拉取，不扩大安全权限
python3 tests/live/probe_nested_sandbox.py --output /tmp/devdesk-nested-sandbox.json
# 仅创建独立随机测试条目，验证保存/更新/删除、重启和不可用 D-Bus
node tests/live/probe_linux_credentials.mjs --output /tmp/devdesk-linux-credentials.json
# 真实原生 OAuth start/cancel；注入本地 HTTP 响应丢失，不完成官方登录
# 需保证没有其他专用 Codex 登录流程占用官方 callback 端口
node tests/live/probe_oauth_recovery.mjs --output /tmp/devdesk-oauth-recovery.json
```

Secret Service 预检使用 Ubuntu 的 `busctl` 只读查询默认集合元数据，不读取或解锁秘密。嵌套沙箱失败时保存实际 AppArmor 策略和原始诊断；不要通过 privileged、unconfined 或关闭宿主策略来取得通过。

AppArmor 启用的 Ubuntu Docker 宿主需先审阅 `permissions/backends/docker/apparmor/devdesk-harness-v1.apparmor`，再以管理员权限执行 `sudo ./scripts/install-linux-sandbox-policy.sh --install`；它只安装/加载 DevDesk 专用策略。安装后可用 `probe_nested_sandbox.py --apparmor-profile devdesk-harness-v1` 验证，不使用默认策略下的失败结果代替该实测。

同一轮 full 因真实阶段失败退出后，先用领域命令显式新建修正 Attempt 或执行合法技术重试，再以原参数加 `--resume` 续跑。入口会将此前报告原字节存入 `history/`，为新执行使用独立日志，校验复用成功步骤的日志哈希，并重新执行最终事实检查。`--resume` 不能改变数据集、仓库、providers/groups，也不能把失败阶段自动计为通过；新的独立验收轮次仍使用新报告目录。
