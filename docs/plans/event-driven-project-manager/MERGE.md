# 本地 main 合入与验收

日期：2026-11-17。状态：完整合入、验收通过，feature 分支及 worktree 已清理。主工作区：/Users/hengliu/workspace/devdesk。

## 合入完整性

- main 从 aa7328daa8e1d94d47fb4c44e0506d59aefec690 快进至原 feature 提交 d9c14e1bd074f61268001b031b16d80431d988d5，合入时文件树完全一致。
- 验收发现并修复模型目录手动刷新竞态，代码验收提交为 e6be48bb8fb214ac6f68c8d10e50d9ab655fa77f。除该界面修复和新增回归外，与 feature 文件完全一致；feature 没有未合入 main 的提交。
- 原主工作区只有本任务早期计划草稿，正文已进入最终方案；原文件及补丁备份在 /tmp/devdesk-main-acceptance-20261117/pre-merge。未改动其他 worktree 或既有运行数据。

## 验收

从 main 执行 scripts/check/check.sh，使用 Node 24.22.0、仓库固定 Rust 工具链和已安装 Schema 依赖。完整入口最终退出 0。

| 范围 | 结果 |
|---|---|
| Rust workspace | 392 通过、0 失败；26 项显式外部/性能入口 ignored。 |
| Chromium debug / release | 两套分别 85/85，均包含新刷新竞态回归。 |
| 权限、Schema、默认目录、架构、RPC、Node、风格 | 全部通过。 |
| HTTP、安全、退出重启、模板、数据集 19/50、运行准备 | debug/release 全部通过。 |
| 工程证据与自主性负向回归 | 33/33。 |
| 已完成的真实 Feature | 用 main 校验器再次离线核验默认 Feature 22 + Task 20 步、原生记录、真实 TDD、确切 Human 签署和 Git/PR 版本，通过。未重新执行模型或增加调度输入。 |

首轮 main 浏览器验收为 83/84，失败于模型目录首次失败后手动刷新未发出第二次请求；此前实施记录中的 Sandbox 用例在本轮通过。可控回归阻塞刷新时的账号身份读取，再用真实配置提交和 SSE 替换请求，修复前稳定失败、修复后通过。修复把刷新标记的消费移到真正开始目录查询前，保留被取消的身份读取所对应的刷新意图和未保存草稿。原始失败、截图及 Red/Green 均保留，断言超时不变。

完整日志及校验身份在 /tmp/devdesk-main-acceptance-20261117：check.log/check.exit 为首轮失败，check-r2.log/check-r2.exit 为修复后的完整通过；refresh-red.log、refresh-green.log、acceptance.json、completed-feature-revalidation.log 保存独立证据。真实工程原运行的详细边界见 [实施记录](EXECUTION.md)。

## 清理

验收通过并再次核对提交包含关系、工作区干净后，使用 git worktree remove 删除 /Users/hengliu/.codex/worktrees/event-driven-project-manager/devdesk，再用 git branch -d 删除 codex/event-driven-project-manager。两者均已确认不存在，main 的 node_modules 原目录保持不变；其他 worktree 保留。清理前后列表及最终结果保存在同一证据目录。
