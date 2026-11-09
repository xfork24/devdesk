# Permission 标准化 Ubuntu 补验

日期：2026-11-09。源码基线：`b0e881928358b0cb82767cfcfd657c194262f6b3`，另含 Workflow 补验中记录的评审失败状态修复。本报告补充首轮 macOS 验收，使用当前 Ubuntu 本机重新执行，不复用历史平台结论。

结论：按用户最终指定的仅 Codex 范围，Ubuntu 验收通过。修复前已完成双供应商 debug/release 权限轮次；修复后最终 release 的 Codex 全权限和受限轮次通过。DSH 后续复验遇到 HTTP 402 余额不足，用户要求暂停，未将该次复验记为通过。

## 环境与数据

- Ubuntu 24.04.5 LTS / x86_64，Linux 7.0.0-42-generic，glibc 2.39。
- Rust 1.99.1、Node 24.13.0、npm 11.8.0、Docker Engine 29.9.3、Git 2.43.0、jsonschema 4.26.0。
- Docker 启用 AppArmor、seccomp 和 cgroup namespace。已安装的 `devdesk-harness-v1` 与仓库固定策略字节一致，SHA-256 为 `3af35216df62da5ff6bfbe54b47506b219f26ccc99c944d84b826d66870cdb2c`；本轮没有安装或放宽系统策略。
- 独立数据集：`data/ubuntu-permissions-workflow-20261109`，验收端口 51762。既有 51751 服务和旧数据集保持运行。
- 临时证据目录：`/tmp/devdesk-ubuntu-validation-20261109-prcVxM`。原始日志、JSON 和截图仅保存在本机临时目录；计划目录只交付 Markdown。原生协议、执行权限、Artifact 和工程事实保存在独立数据集中。临时目录不是永久下载地址。

## 执行结果

| 检查 | 结果 |
|---|---|
| `./scripts/doctor.sh --permission-backend docker-nested-v1` | 全部通过，含 Docker、GitHub 登录、Secret Service 可用且已解锁、固定 AppArmor 策略 |
| `./scripts/check.sh` | 首轮 Rust 131 项通过；Workflow 修复后完整复验 Rust 132 项通过，8 项外部测试默认 ignored；两轮 debug/release HTTP、运行时自动准备、debug/release Chromium 各 10 项均通过 |
| `cargo test --locked -p devdesk-adapters --test permissions --test docker_resources --test command_tests --test git_resources -- --include-ignored --test-threads=1` | 17 项通过、0 ignored；包含三个原生权限测试、实际 Docker 隔离、Node Red/Green、Git worktree/CAS/恢复 |
| `probe_nested_sandbox.py --apparmor-profile devdesk-harness-v1` | 固定镜像及本轮实际 managed Codex 镜像分别通过；授权目录可写，假 `/state` 凭据读取被拒绝，容器清理已核验 |
| `probe_permissions.py --project …` | 修复前 debug/release 各 4 个真实模型轮次通过，覆盖 Codex/DSH 默认全权限与只读＋工具禁网；修复后最终 release 以 `--providers codex` 再验证 2 个轮次通过 |
| `probe_linux_credentials.mjs` | 实际 Chromium → Host → Secret Service 保存、更新、读取、重启、不可用 D-Bus 和独立假条目清理通过；不锁定用户凭据库 |
| `probe_oauth_recovery.mjs` | 实际原生 OAuth start/cancel、丢失本地 HTTP 响应后的同 login 恢复、刷新及新登录身份通过；未替用户完成新 OAuth 登录 |
| `python3 tests/runtime_preparation.py target/release/devdesk-host` | release 的独立运行、嵌入构建配方、失败/重试、并发复用和 Git 恢复通过；外部 Docker/GitHub 使用明确夹具 |
| `./scripts/package.sh` 与 `probe_permission_package.py` | 本机 Linux 包在源码外运行通过；7 个资产摘要、6 个预设、不可变修订、绑定重放、项目库 v34 和不自动下载均通过 |
| `cargo fmt --all -- --check`、Python 探针语法、`git diff --check` | 通过 |

## 原生权限事实

实际 managed 安装：

- Codex `0.167.0+docker.09fbe4ee9575`，镜像 `sha256:09fbe4ee95753eef3cb79788e14713ab64e7711ff9dd8f6f3dfa748cb6176125`，Model `gpt-6.1-sol`。
- DSH `0.2.2-rc.1+docker.abc4194553c0`，镜像 `sha256:abc4194553c025e834132a9f4e6a91887b4f72ca8686388588cc13897920a8b1`，Model `deepseek-v4-pro`。

| 策略 | Codex Turn | DSH Turn | 实际事实 |
|---|---|---|---|
| 默认全权限 | `7a53ad17-3bc2-41c1-b29c-589c3884c570` | `1afa6536-6903-4dc0-a8bd-f3a085f22c91` | 工具先写入 `/tmp`，再命中本机 HTTP canary；冻结策略为 full_access、approval=never、无需嵌套沙箱 |
| 只读＋工具禁网 | `35deb2bc-486d-4f92-8fb2-958da111b72b` | `e62f10c3-a205-4f71-8104-b2f06fdb1919` | 工具实际输出 `PERMISSION_DENIED_OK`；写入被拒绝、未命中 canary、模型 Turn 正常完成；Sandbox 的模型通信网络保持 enabled |

上述表格是 debug 的 4 个 Turn；修复前在与当时发布包二进制 SHA 完全一致的 release Host 上重新执行相同 4 个轮次，也全部通过，结果保存在临时证据目录的 `release-permissions/permission-native-tools.json`。

Workflow 失败投影修复后，以最终发布包的二进制再运行 Codex 全权限及受限两个轮次，结果在 `package-codex-permissions/permission-native-tools.json`；两侧工具结果和 canary 均重新核对。DSH 失败复验保留在 `fixed-release-permissions/`，暂停该供应商后没有继续调用。

真实资源专项另验证：Codex scoped 授权写入成功且越界失败；DSH 受限 SDK 看不到假状态和假环境变量、无外网接口、不能写入受保护测试目录；全权限容器 `/tmp` 可执行，假状态可见，Docker socket 不可见。容器未使用 privileged、unconfined、额外 capability 或关闭 AppArmor。

独立工程测试的权限固定为 node-test、只读代码导出、Docker network=none、无模型凭据。完整原生 Workflow 的 Red/Green、候选测试和工程收尾另见 [Workflow Ubuntu 补验](../workflow-and-workbench/UBUNTU.md)。

## 发现及修正

1. 旧权限探针使用已被新治理契约拒绝的角色入口和会话权限覆盖。现改为创建完整 Model/Harness/Tool/Permission/Agent/AgentID，通过精确组合执行；受限权限属于新不可变 Agent 修订。
2. 原探针把 macOS 的 `host.docker.internal` 当作 Linux 默认地址。首次实际工具输出 `fetch failed`，没有 canary 收据，正确判为失败。改为从本机 Docker bridge 查询网关后，四个真实轮次通过；初次失败 JSON 和日志保留。
3. 原生报告中的 macOS 平台文字改为读取真实 OS/架构，避免 Ubuntu 结果误标。
4. 发布包探针仍断言工程 schema=33。现从现行顺序 SQL 迁移计算应有版本，并核对新包实际 schema=34。

上述修正均位于验收探针；本轮未通过改变领域门禁、权限编译器或隔离策略来取得通过。

## 发布包与范围

- 包：`dist/devdesk-0.2.0-x86_64-unknown-linux-gnu.tar.gz`。
- 包 SHA-256 保存于同名 `.tar.gz.sha256`；发布包探针独立计算归档摘要并核对该文件。修复前的首轮归档和验证结果另存于临时证据目录。
- 最终 release 二进制 SHA-256：`ed0bc8ce96a89bd5bf4e3cdc4157f99614043f3d1b8bf2ff20d0f2659758523a`。

最终包的二进制与实际 Codex 权限轮次和真实浏览器检查使用的文件 SHA 一致。报告补齐后仅刷新归档中的 Markdown，没有再次编译程序。

本轮平台结论限于 Ubuntu x86_64。网络 allowlist 仍明确拒绝；DSH 工具禁网仍要求受限文件模式及 bash/ask_user_question 工具范围。FullAccess 继续以整个容器为信任范围，假凭据隔离结果不扩大为 FullAccess 中的凭据隔离保证。
