# 源配置整体重构执行记录

工作分支：`codex/source-configuration`。独立 worktree：`/Users/hengliu/.codex/worktrees/source-configuration/devdesk`，起点 main `082c081`。原 worktree 的报告未改动；实施期间 main 新增的验收记录会在合并时一并保留。

## 已确认边界

- 源配置、Schema、数据库新基线、执行解析和界面一起替换。
- 只支持新协议和新数据，不提供旧库升级或兼容执行分支。
- Agent 仅引用 Role、Capability；权限独立归属 Permission。
- 引用为 identity:revision。派生执行视图在内存中生成，不持久化。
- PRD/Design 规范正文由 Spec 唯一拥有，Delivery 引用。
- 默认 Feature 22 步、Task 20 步的任务、依赖、端口、门禁和 TDD 顺序保持不变。

## 实施

源封套和引用解析已替换；静态投影和初始实例资源已删除。SQLite 使用 source_definitions 及源引用选择，没有 definitions/harnesses/sandboxes 投影表。权限预设与步骤授权统一到 resources/configuration/permissions。

存储编码保存引用和实际选择，读取时恢复任务、Plan、权限、模型和工具视图。流程修改接纳为新的 Workflow 源修订，执行事实保留。文档和打包脚本同步更新。

## 验证进度

规范化源合同与 main 082c081 对比：两份默认 Workflow 业务字段完全一致；仅转换引用和迁移授权字段。配置源数据、新库重启及默认执行流程回归已通过，完整回归与浏览器检查仍在进行。

日志和测试输出仅位于系统临时目录，不作为交付物提交。最终验证结果在任务完成前更新。

用户追加确认：完成独立 worktree 修改后合并本地 main，并在 main 上继续完整验收。
