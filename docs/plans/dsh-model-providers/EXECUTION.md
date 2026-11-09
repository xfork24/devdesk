# DeepSeek Harness 多模型接入实施记录

日期：2026-11-09。状态：实现与本轮验证完成。真实模型仅使用 Codex；DSH 使用本地模拟端点，不访问真实推理服务，也不读取真实 DSH key。

## 工作区与整合

独立 worktree 为 `/Users/hengliu/workspace/devdesk-dsh-model-providers`，分支为 `codex/dsh-model-providers`。最初以临时 Git 索引生成基线快照 a91a815，未修改主干索引和工作文件；随后更新到主干 651b7fe，保留其权限实现、工具表单及计划提交。临时基线快照不进入最终主干提交链。

## 分阶段结果

| 阶段 | 交付与验证 | 结果 |
|---|---|---|
| M0 固定运行时 | 离线核验既有 `@deepseek-ai/dsh@0.2.2-rc.1` 镜像，确认多服务商插件随包提供；固化目录、协议与端点 | 通过，未升级 DSH |
| M1 领域与解析 | 复用 Model/Harness/Capability/Agent，增加容量、连接和 profile 摘要；拒绝非法组合、端点、预算、缺失快照及伪造路由 | Rust 与 Schema 回归通过 |
| M2 凭据 | 独立账号命名空间、数据集/服务商归属校验、旧 DeepSeek 引用兼容、免密 Ollama、助手 protocol=2 | 隔离与格式测试、编译、签名及运行包清单通过；未操作真实 DSH 凭据 |
| M3 DSH 与容器 | 受控 profile、动态 initialize、Ollama 宿主映射、权限合成、清理、连接检查 | 固定 DSH 的模拟流式/工具/恢复与容器目录检查通过 |
| M4 Web 与默认配置 | 独立凭据入口、Model/容量/候选、服务商声明、新内建 DSH Harness、完整 Agent 检查入口及连接摘要 | debug/release 浏览器流程通过 |
| M5 回归与真实模型 | 最新主干整合后完整检查，独立运行包、Codex 真实工具调用和恢复、token 扫描 | 本轮通过；DSH 真实推理与 Ubuntu 宿主保留为待验 |

新增服务商仍属于原有 dsh Harness。模型适配使用 DSH 内置插件，没有引入 Pi CLI、其他 Agent/Harness 框架或 Rust 厂商推理实现。新增内建 DSH Harness 采用固定配方物化；旧修订与执行保持原引用。

## 自动化和离线证据

在最终整合代码上执行 `scripts/check/check.sh`：132 项 Rust 回归通过，HTTP/Schema、按需准备和独立复制二进制检查通过，debug/release 各 15 项浏览器测试通过。默认忽略 11 项依赖环境的测试，其中本计划的两项 DSH/Docker 离线测试另行显式执行并通过。Python 可选 jsonschema 未安装；发布 Schema 的离线解析与实例校验由 Rust jsonschema 测试覆盖。

三条新路由分别在固定 DSH 镜像及 `--network=none` 容器中访问模拟服务，每条完成流式文本、一次原生 Bash 调用、工具结果消费、停止与原生恢复，共 9 次模拟推理请求。MiniMax thinking 内容回放通过。生产 profile 由正式适配器生成，只有测试中的 endpoint 被替换为容器 loopback；没有真实服务商 key 或外部推理请求。

另一测试查询本地模拟 Ollama 目录，分别验证 Host 与目标容器结果，并在服务关闭后确认先前目录成功不能使容器检查误报成功。临时凭据文件、目录和容器正常清理。完整 Host 链同时验证 Model → Capability → Agent → Session → 固定镜像物化、免密连接、Model 新修订和重启后的冻结，见 [Host 离线验收](HOST-OFFLINE.md)。端到端检查中发现的新建全局 Agent 后项目目录同步问题已修复。

## 独立运行包与 Codex

运行包为 worktree 下 `dist/devdesk-0.2.0-aarch64-apple-darwin.tar.gz`。已解包核验 Host/助手签名、助手 protocol=2 与内容摘要，并使用包内 release Host 完成实际验收，见 [真实验收](NATIVE.md)。

真实模型为 `gpt-6.1-sol`，通过完整 Agent 提交工具验证，确认原生工具结果被消费并返回 marker；Host 重启后继续原生 Session，保持模型、凭据及运行绑定，正确恢复此前 marker。扫描非凭据持久化文件，未发现 Codex OAuth token。验收数据集与运行包解包目录均为独立临时目录，验收容器已释放。

打包在独立目录构建、签名随包助手，没有替换本机已授权共享安装；已比较打包前后助手二进制和清单，内容未变。源码安装器和运行包分别提供显式升级入口，正常 Host 构建/打包不自动覆盖已授权安装。

## 实际边界

本轮未执行真实智谱、MiniMax、DeepSeek 或 Ollama 推理，也未保存、读取或授权真实 DSH key。新服务商的实际订阅权限及 macOS 新助手的首次 Keychain 授权，需要用户按设置流程完成，不能由模拟调用或签名检查代替。Ubuntu 宿主与 Linux Secret Service 运行验证未在本机执行。现有 DSH 真实模型和跨平台待验矩阵继续保留，不影响本轮按用户指定范围完成实现与验证。

用户配置入口及助手升级说明见 [模型服务商](../../model-providers.md)。

## 启动兼容修正

源码启动和构建入口使用助手安装器的 --ensure 模式。兼容的 protocol 1/2 助手在内容和同证书签名校验通过后继续复用，源码/依赖身份变化不再中断 Host 启动。手工替换仍要求 --update，智谱/MiniMax 的新账号格式仍要求新版助手。

已执行 `python3 tests/entrypoints/credential_helper_startup.py --launch`：验证兼容旧助手复用、显式更新门禁、未知协议/内容篡改/外部签名程序拒绝，并通过实际 dev.sh 启动和 HTTP setup 检查。比较前后共享助手二进制与清单，确认未改变；没有模型调用或 Keychain 读写。
