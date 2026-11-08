# Web 平台与系统凭据补充计划

日期：2026-11-08。状态：P1 macOS 实际验收通过；P2 Linux 后端/入口已实现，Ubuntu 宿主待切换验证。依据用户在 R0–R8 执行期间的追加要求；在 [真实工程接入验收](../reports/real-integration/R8-acceptance.md) 后执行。目标为 macOS/Linux Web UI，macOS native 客户端后续再做。

## P1：macOS 稳定身份与无后台提示

- 使用直接的 Security.framework 读取/保存，后台访问禁止自动显示系统授权 UI，权限不足返回可读错误，不通过 security CLI 读取秘密。
- 为开发/发行 Host 提供稳定 codesign identifier 与证书配置；debug/release 重建使用相同 designated requirement。当前本机没有可用代码签名身份，准备一次性本地签名证书及导入说明；用户协助系统导入/授权，不收集登录密码。
- 实测发现固定证书仍受 Keychain cdhash 分区影响；增加一份不可变共享凭据助手，Host 验证助手签名/摘要，助手验证对应签名 Host。Host 重编译复用助手，不为项目/角色复制。
- 旧凭据保留，提供显式的一次性授权入口；不使用允许所有程序的 ACL、不批量改用户钥匙串、不绕过锁定策略。需要系统认证时仅由显式操作触发。
- 实测两次不同构建/重启后可读同一凭据且无后台提示；未授权/锁定返回错误，设置页仍响应。

## P2：Linux Web 运行

具体执行步骤及验收清单见 [Ubuntu Web 真实工程验收计划](ubuntu-web-acceptance.md)，当前仍待实际 Ubuntu 宿主执行。

- Linux Secret Service 后端与设置页输入，秘密经私有 stdin/系统凭据库传递，不进 SQLite、argv、报告或浏览器持久存储；缺依赖明确阻塞。
- 去除真实资源测试中的单平台镜像 ID 假设，按显式登记或本机固定镜像解析不可变 ID；保持会话冻结的旧摘要不变。
- macOS/Linux 统一 Web 构建、Host、容器配方、Git/GitHub 与验收入口；预检列出宿主平台、依赖和凭据能力，不把 Linux 容器当作 Linux 宿主验收。
- 准备 Ubuntu 操作说明及验收报告模板。macOS 回归与包验证后，请用户切换 Ubuntu；在 Ubuntu 实际执行离线检查、镜像构建、双 Harness、Web 闭环/恢复及运行包验证。

## 完成条件

macOS 实际重建/重启不重复弹钥匙串提示；Ubuntu 实际 Web 与工程链路通过；两平台证据分别保存。证书导入/首次 OS 授权和 Ubuntu 切换是明确外部前置条件，等待时继续不依赖该步骤的实现与检查，不将其记作已通过。
