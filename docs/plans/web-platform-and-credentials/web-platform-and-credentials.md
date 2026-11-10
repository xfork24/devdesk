# Web 平台与系统凭据补充记录

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

依据 [补充计划](PLAN.md)。当前阶段：macOS 固定共享凭据助手已通过真实凭据、跨构建及重启验收。Ubuntu 24.04.5 / x86_64 的 U0–U6 已通过：Linux 凭据、双供应商工程闭环、真实 PR merged、安装复用、取消/崩溃恢复与源码外运行包；专用 AppArmor 策略经用户批准并实测允许/拒绝边界。详见 [Ubuntu 本轮报告](../ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。

## 已实现和验证

- 固定共享助手直接使用 Security.framework，并禁止普通操作交互 UI。未授权检查实际约 0.05 秒返回 authentication_required；不调用 security CLI 输出密钥。显式授权使用固定共享助手（30 秒），HTTP 不接收系统密码，也不返回秘密。
- 本机原先没有有效 signing identity；已生成/导入 `DevDesk Local Development`，仅信任用户域代码签名，私钥只授权 codesign。用户确认系统信任和“始终允许”。签名标识固定 `dev.devdesk.host`；debug/release 内容哈希不同、designated requirement 相同，但实际读取仍被 macOS 的 cdhash 分区拒绝，因此保留该失败并采用独立助手。元数据见 macOS 证明（历史路径：`macos-credentials.json`）。
- `scripts/dev.sh`、`scripts/build.sh` 和运行包构建在 macOS 使用已配置证书签名，并复用同一份不可变凭据助手；`setup-macos-signing.sh` 可重复执行，不改其他身份或钥匙串 ACL。证书私钥只在用户私有目录，不进入源码或发行包。
- Linux 通过 Secret Service/secret-tool 私有 stdin 保存 API key；普通状态页不读取/解锁秘密，缺依赖明确阻塞，无明文文件降级。Codex 继续使用独立 OAuth 文件。
- 真实资源测试从本机固定版本镜像解析并冻结不可变 ID/CPU，不再把 macOS Docker 的 arm64 image ID 写死用于 Ubuntu。
- 新增宿主预检、[系统凭据说明](../../system-credentials.md)、[Ubuntu Web 验收步骤](../../ubuntu-web-acceptance.md)，并准备不含运行数据/密钥的工作树源码快照，便于不提交当前改动也能搬到 Ubuntu 构建。

## 验证命令

`cargo test --workspace --locked` 通过。专项实际 Docker CommandTests 2/2、资源 4/4、adapter unit 5/5、固定助手 2/2 通过；单测验证无后台 Keychain UI 的设置与恢复。签名 release 的复制后二进制 HTTP 契约已通过；签名 release 的 6 个 Chromium 用例均通过。平台预检见 macOS（历史路径：`macos-web-preflight.json`）。

当前 R0–R8 工程闭环验收仍以 [原报告](../real-engineering-integration/R8-acceptance.md) 为准。本补充轮次不回写原模型调用、PR 或失败记录。

## 平台接续

1. macOS 已通过。原来直接读取的第一次签名 Host 已获授权（约 0.01 秒成功），但同证书的新构建和新建条目均复现拒绝。新方案已用无真实密钥的测试条目验证：不同 Host 构建、不同路径读取成功，助手摘要不变，非对应签名调用者在访问前被拒绝。用户已授权固定助手；两个不同 Host 构建、同一旧凭据引用、助手摘要不变的实际读取均通过，重启后读取仍通过。真实 DSH 两轮会话通过，重启后第三轮续用同一原生 Session，随机 marker 保留，执行者停止/封存/释放均确认。
2. Ubuntu 桌面宿主已实际完成编译、Secret Service 保存/读取、双 Harness Web 工程闭环/恢复与运行包检查；使用独立数据集和报告，未以 Linux worker 或旧原型报告替代。

共享助手的跨构建实际证明见 macos-helper-identity.json（历史路径：`macos-helper-identity.json`）。macOS 自签名分区行为依据 Apple 的 [clientid.cpp](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp)，没有修改整个钥匙串的 partition list 或授予通用 CLI 读取权。

完整 macOS 凭据证据见 macos-credentials.json（历史路径：`macos-credentials.json`）、真实 DSH（历史路径：`macos-credentials-dsh.json`）。保留首次直接读取失败和旧 DSH 启动失败，不将其覆盖为成功。Ubuntu 工程闭环、正式模型凭据与恢复的最终结果见上述独立报告，保留全部首次失败与续跑历史。

最终 `./scripts/check.sh` 退出 0：完整 Rust 工作区、debug/release HTTP 与各 6 个 Chromium 用例通过，见 完整回归（历史路径：`credentials-full-regression.log`）。共享助手未重建。回归重建覆盖在运行的 Host 后，旧进程会被调用方签名校验阻塞；该情况要求重启新构建，不要求重新授权。

最终运行包已解压到源码目录之外，实际读取同一旧凭据成功；随后重启标准 release Host 再读成功，均未重新授权。运行包只有一份共享凭据助手，摘要与已授权安装相同。原 R0–R8 的 GitHub/数据库/Artifact 全量复核也再次通过。
