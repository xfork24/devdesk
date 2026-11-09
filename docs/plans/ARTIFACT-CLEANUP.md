# 计划中间产物清理记录

日期：2026-11-09。按用户要求，所有计划目录只保留 Markdown 文件。
本次移除 430 个中间产物，共 7,940,686 字节。源码回归测试、产品运行数据库、运行期 Artifact、凭据和构建源文件不在清理范围。

| 计划 | 移除文件数 |
|---|---:|
| devdesk-prototype | 2 |
| domain-hardening | 1 |
| permission-standardization | 102 |
| real-engineering-integration | 39 |
| ubuntu-web-acceptance | 273 |
| web-platform-and-credentials | 13 |

删除类型：.json × 193, .jsonl × 2, .log × 206, .mjs × 2, .patch × 3, .png × 13, .py × 9, .txt × 1, .yaml × 1。

保留各 PLAN.md、执行/验收/评审 Markdown。历史报告中的命令、路径和 SHA 是当时的记录；原日志、JSON、截图、补丁及临时测试脚本已经移除，不能再据这些路径读取中间文件或把旧报告当成本轮复验。

此后测试临时输出放在系统临时目录或工具的忽略输出目录，最终执行记录只保留 Markdown；源码测试继续保存在 crates、tests、web/tests。
