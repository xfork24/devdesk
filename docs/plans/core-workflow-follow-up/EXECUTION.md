# 核心工作流补强执行记录

日期：2026-11-14。实施基线为本地 main `8fef081`（包含计划之后的现行文档更新），独立 worktree `/Users/hengliu/workspace/devdesk-worktrees/core-workflow-follow-up`，分支 `codex/core-workflow-follow-up`。本报告随阶段追加，最终结论以合入 main 后验收为准。

## 本轮授权与证据保留

用户明确要求按 C1、C2、C3、C4 顺序实施，合入本地 main 后进行最终验收，不扩展核心范围；本轮专用远端仓库已明确选择 `xfork24/devdesk-project-1`。平台只创建测试分支、提交和 PR，不调用 GitHub 合并接口。用户追加要求保留最终测试实施文件、每步实施者、决策和产物及完整工程现场，供人工验收。

本轮证据根为 `/Users/hengliu/workspace/devdesk-acceptance/core-workflow-follow-up-2026-11-14`。最终验收另设 main 目录，保留实际二进制/脚本摘要、输入、逐步 Actor/执行来源、回执、失败/恢复与停止后的 SQLite/Artifact 一致快照和证据索引；认证资料不写入分享包。文档目录仅保存 Markdown。

## C1 实施与阶段验收

先加入 foreach 槽位回归，在改动前实际失败：查询 dispatchable 为 true，预期 false。共享资格进入 core，实际命令继续核对精确计划、输入/输出与控制权，不放宽并发或人工门禁。阶段完整结果待实际回归后补录。

共享动作资格已接入 workflow.inspect、driver.waiting、Rust 候选选择及 dispatch/accept/retry/complete 实际用例。foreach 槽位和独立人工等待回归通过；原停止证明、完成条件、人工确认和事务 CAS 保留。陈旧候选只在有界 tick 内排除/重读，受阻结果不再阻塞其他动作；UI 只展示共同资格返回的原因。

C1 验证：编排 37 项通过，Schema 14 项通过，domain/core 单元通过（数量见原始日志），Web production build、架构/出厂目录及 diff 检查通过；最终 foreach 定向复验通过。Web 初次构建捕获一处可空 data 访问，改为显式空数组后重建通过，失败日志保留。未执行真实模型工程；最终 main 验收尚未开始。阶段日志在证据根 stages/c1-*。
