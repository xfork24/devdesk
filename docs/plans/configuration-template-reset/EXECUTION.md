# 默认模板资源与重置验收记录

日期：2026-11-10。状态：已实施，Ubuntu 当前合同的 Codex 验收通过。macOS 实现曾 rebase 到 main `8a16686` 并完成复验，以快进方式整合本地 main，不产生 merge commit。当时实现分支为 `codex/configuration-template-reset`，独立工作区为 `/Users/hengliu/workspace/devdesk-template-reset`。

## Ubuntu 当前合同复验（仅 Codex Harness）

2026-11-10，在 main `ea74bd5` 的当前 Workflow v4、global 13 / project 44 合同上验收。宿主为 Ubuntu 24.04.5 LTS / x86_64，Rust 1.99.1、Node 24.13.0、Docker 29.9.3。本轮按用户要求仅验收 Codex Harness，不运行 DSH/Ollama 的真实供应商探针。全部使用隔离临时数据集，日常数据和源仓库保持原值。

| 检查 | 本轮结果 |
|---|---|
| `DEVDESK_E2E_PORT=45330 ./scripts/check/check.sh` | 完整通过；Rust 154 项通过、0 失败、15 项环境用例默认忽略，debug/release 各 41 项 Chromium 通过。 |
| 模板后端与浏览器 | 每类定义恢复、名称修改、MCP 绑定归档、跨项目一致、旧绑定、事务回滚、过期预览与页面、命令重放、无变化重置及资源文件摘要不变均通过。 |
| debug/release 独立二进制 | 两种构建均从源码外临时目录通过重置、不可变历史、偏好保留、重放、重启及无变化重置。 |
| 真实 Codex 的重置与重启 | 自建 Role/Skill/Agent/Delivery/Workflow 完成后保存项目配置，再将目录切到 generation 2。旧冻结任务、输出、项目配置、运行安装和凭据内容均保留；重启后仍能按归档绑定完成新 Feature，实际平台工具及后继消费通过。 |
| Linux 发布包 | `probe_permission_package.py` 通过：7 个权限资产、3 个预设、Schema 44、编辑/发布/预览与实际独立工具流程；0 次模型调用、0 次自动下载。 |
| 格式与空白 | `cargo fmt --all -- --check`、`git diff --check` 通过。 |

真实 Codex 使用固定 0.167.0 镜像。重置前 Workflow 为 `b049c6b7-133a-4578-ba5d-d7ef39121814`；重置、重启后新 Workflow 为 `16bbbf7d-658c-4a8a-b6e7-c7f4d937278f`，Feature 为 `70d3a25c-9dc4-42d8-a949-66253206b120`。新旧执行均得到降序 `[3,3,2,1]` 与总和 `9`，实际调用 `materials.capture`，后继消费同一材料。原冻结任务及完整运行视图前后相等；全局与项目库的 `integrity_check`、`foreign_key_check` 通过。探针退出后删除临时登录副本。

发布包为 `dist/devdesk-0.2.0-x86_64-unknown-linux-gnu.tar.gz`，SHA-256 为 `ce36fc6927e025d432d2790885c0e06dc1cb74ac2997637cff81dbae21f48625`；程序 SHA-256 为 `2c00400cc5e6b92bb267706316eda17f1eb83a0f2172dcd94d0ab813354b7576`。

本轮临时证据位于 `/tmp/devdesk-ubuntu-acceptance.Cbt2NL/`，包含 `native-custom/reset-retention-report.json`、原生任务及产物、`package-report.json`；完整默认检查日志为 `/tmp/devdesk-ubuntu-acceptance-check.log`。这些临时产物不提交，也不承诺清理后仍可读取。默认工程的真实 Codex/Git/Docker/GitHub 验收另见 [Workflow 执行报告](../workflow-business-configuration/EXECUTION.md)。下列 rebase 与首轮记录保留其当时的数量和 Schema，旧库迁移结论不代表当前版本仍支持迁移。

## 最新 main 的 rebase 与复验

本次以 `8a16686 Remove legacy configuration and finalize current runtime contracts` 为基线解决冲突。保留 main 的当前协议、严格执行合同、凭据隔离和 SQL 基线；默认资源重新提取其完整有效目录，共 83 个条目、52 个逻辑定义。原 `catalog.json` 拆入 manifest 指定的各类型文件，不保留两个默认来源。旧 v1 Permission、旧 Workflow、旧正文升级和旧数据库转换链均不重新引入。

全局 10、项目 41 的现有库通过事务追加索引迁移到 11、42。新增回归已证明迁移与首次目录登记保留 main 的已有编辑，且不会生成历史默认版本。重置和启动共用当前 Permission 绑定逻辑；原子创建恢复后的目标、Profile 和 assignment，历史绑定保持原值。Capability 校验、权限预检和新项目预填按当前代次解析依赖。

main 已修改的内置 Harness 保留原有的自动权限初始化行为；新增回归覆盖其首次目录登记和绑定。逐项展开资源后，与 main `8a16686` 的 83 个原始目录条目比较，正文、名称、Scope、ID 和精确引用完全一致。

| 本轮复验 | 结果 |
| --- | --- |
| Rust 全量 `cargo test --locked --workspace --no-fail-fast` | 181 项通过，15 项既有环境依赖测试跳过 |
| 最终 Host 全量 `cargo test --locked -p devdesk-host` | 24 项通过，含新增 main 已编辑 Harness 回归；与全量去重合计 182 项 |
| main 当前数据库迁移 | 全局 10→11、项目 41→42；定义和编辑保留，测试通过 |
| Web 构建、Rust 格式、diff 空白、文档链接 | 通过 |
| Permission 资产 | 3 个预设、4 个后端、7 个不可变资产通过；Python 可选 jsonschema 模块未安装，Rust 离线 Schema 校验通过 |
| Debug HTTP、连接及运行环境准备 | 通过 |
| Debug 浏览器全量 | 41 项通过 |
| 最终 Debug 模板浏览器补验 | 3 项通过 |
| Debug/Release 最终构建、签名与独立二进制重置/重启 | 通过 |
| Release HTTP 与连接 | 通过 |
| Release 最终浏览器全量 | 41 项通过 |

完整检查入口首次在旧断言将目录元数据计作业务定义时停止；修正后分别运行以上组件完成全量复验，未跳过失败项。最终 Host 改动后重新跑 Host 全量、双 profile 构建与独立包、Debug 模板浏览器和 Release 浏览器全量。

本轮使用 45083、45084 独立端口及临时数据集，未操作主工作区运行数据。15 个忽略项沿用当前 main 的环境条件；本轮不声称覆盖真实 Docker/专用 GitHub/供应商运行环境的付费外部执行。

以下为 rebase 前 `2824bdf` 的首轮验收，默认数量、迁移编号及环境跳过数属于当时版本，当前结果以本节为准。

## 首轮实现结果

默认清单包含 148 个资源条目，覆盖发布定义、旧格式投影、不可变权限依赖和兼容记录；当前默认目录为 69 个逻辑定义。Role、Skill、Spec、Standard、Workflow、任务、Delivery、Capability、Agent、Model、Harness、Sandbox、Tools 与 Permission 默认均由资源加载。MCP/HTTP 清单明确为空；可以添加真实的可移植定义，凭据不进入资源。

空库初始化建立数据库副本，后续编辑只保存数据库修订或草稿。设置页提供数量预览与二次确认，重置原子创建新的默认依赖图并切换目录代次。自建、自定义和草稿退出当前目录，历史记录、项目绑定、Feature 锁、会话、经验、凭据、安装和运行偏好保留原值。

目录元数据复用治理账本，全局迁移 010、项目迁移 041 仅追加索引。普通启动不重新播种默认；旧库登记、项目延迟同步和显式权限重载遵守目录代次。恢复后的默认 Agent 在同一事务内取得完整权限绑定。

## 整合基线

开始时主工作区的 128 个未提交文件原样复制到独立 worktree，保存为临时基线提交 `515688d`，主工作区未被修改。用户允许将这些已有改动一并整合。

实施期间本地 main 新增 `0786ed1`，已包含这批已有改动和后续配置、Markdown、工作流改进。实现分支使用 `git rebase --onto main 515688d` 只重放本次实现，避免重复提交基线。冲突保留 main 的现有产品行为；默认业务数据迁入资源，替代重复的 Rust 播种及自动正文升级。

默认资源重新对齐 `0786ed1` 的有效初始目录。职责、方法、规范、任务和交付的 Markdown 引用按领域显式声明，即使文本相同也保持各自文件来源。已有旧库保留上一版本采用的精确默认引用。按用户最新指令，验收后只提交并保留实现分支，暂不更新本地 main。此项是首轮暂停整合的记录；本轮已按后续指令 rebase 并复验，整合方式见本报告首节。

## 已覆盖的回归

- 每类定义的数据库修订和名称修改均可恢复；MCP 定义与绑定、自建项目定义和 Workflow 草稿可归档。单独停用默认工具也会触发恢复，停用会使旧预览失效。
- 全局与两个项目在重置后使用同一目录；旧记录、已保存项目配置和 Feature 冻结内容保持原样，旧项目仍能按原配置创建后续 Feature。
- 新默认 Agent 解析恢复后的 Harness、Sandbox 和权限 Profile；新的 Workflow 子流程、Role、Spec 和交付引用全部可编译。
- 过期预览、过期页面、历史对象编辑被拒绝；相同命令重放、相同默认重复重置、失败事务回滚及重启恢复均通过。
- UI 编辑和刷新保留数据库修改；取消不修改数据；确认后恢复默认；旧浏览器草稿不会覆盖新目录；冲突可重新预览，失败可按原命令重试。测试对资源目录作前后摘要比较，证明这些操作未回写资源。
- 旧库迁移的失败重试使用确定性成员元数据，避免不可变记录冲突。显式目录整理不会在启动时重新引入旧默认。仅含旧 Permission 的数据库不会被当作空库覆盖；冲突的不可变默认 ID 会使重置失败。
- 已保存的运行偏好保留精确 Model/Harness 引用；重置和刷新后仍能显示原名称，并标记“重置前固定版本”，新的下拉选项来自当前目录。

## 自动化结果

| 检查 | 结果 |
| --- | --- |
| Rust 格式与 diff 空白检查 | 通过 |
| 资源构建 Schema、引用与领域闭包检查 | 通过 |
| `cargo test --workspace --locked` | 193 项通过，16 项环境依赖测试按既有标记跳过 |
| 新增模板后端回归 | 3 项通过，包含全部类型、名称、停用、跨项目与冻结引用 |
| `npm --prefix web run build` | 通过 |
| Permission 资产检查 | 通过：9 个预设、4 个后端、7 个不可变资产；Python 的可选 JSON Schema 检查缺少模块，Schema 另由 Rust 离线校验覆盖 |
| Debug HTTP、连接、运行环境准备 | 通过 |
| Debug 独立二进制模板重置与重启 | 通过 |
| Debug 浏览器回归 | 41 项通过 |
| Release 构建与签名 | 通过 |
| Release HTTP、连接与独立二进制模板重置、重启 | 通过 |
| Release 浏览器回归 | 41 项通过 |

完整入口为 `DEVDESK_E2E_PORT=45083 ./scripts/check/check.sh`。使用独立端口以保留原工作区已有服务。新增 `tests/entrypoints/template_reset.py` 在临时目录运行单独复制的 Host，验证默认副本、归档历史、重试、偏好保留、重启和幂等；debug 与 release 均纳入检查脚本。

本轮另人工检查了默认 52 项的确认框截图。首轮浏览器截图人工检查了设置页预览与恢复后的 Role 目录，确认文字、数量、确认/取消操作和表单布局正常。截图和原始日志位于临时目录及忽略的测试输出目录，不进入计划目录。

本轮未调用付费模型或创建真实 GitHub PR。16 个跳过项需要真实 Docker/固定镜像、原生 DSH/Ollama 环境或专用 GitHub 验收凭据；本轮的存储、HTTP、独立二进制和浏览器结果不代替这些外部资源测试。
