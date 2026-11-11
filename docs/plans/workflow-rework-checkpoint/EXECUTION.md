# 实现返工检查点执行记录

真实仓库 `xfork24/devdesk-project-1` 的验收于 2026-11-12 00:41 阻塞。恢复前现场保存在 `/Users/hengliu/workspace/devdesk-acceptance/20261111-230248-project-1-workflow/manual-review-checkpoints/20261112-first-block/`，完整业务现场在同一根目录的 `run/data`。

独立评审拒绝了历史开发执行缺少指定镜像关联的证据，PM 组织 implementation 返工。Add 新执行仍消费旧 Red 快照，而同一 Task 工作树已由上一轮 implementation_snapshot 推进，原生 writer 校验拒绝启动。源码改动在独立 worktree `/Users/hengliu/workspace/devdesk-worktrees/workflow-rework-checkpoint`、分支 `codex/workflow-rework-checkpoint` 实施。

内核为写入返工冻结同一实例、同一工作区的真实平台快照检查点；仅接受已登记代码头且有成功平台快照回执的版本。替代执行与后继封存使用该检查点，原比较基线保留。新的实际 Agent 输入显式记录原绑定、检查点、返工原因和本步骤旧产物引用。产物读取仅扩展到显式提供的本步骤历史；旧执行、计划、Standard、测试及拒绝回执不变。

定向回归：configured_workflows 37、orchestration 30、schema_contracts 11、standard_snapshots 5，全部通过。新真实 Git 用例同时验证未知代码头拒绝、writer 启动时冻结版本校验、返工后的真实快照、原始比较基线、历史产物不变和新输入来源。日志位于验收根目录 `rework-related-regression.log`，格式与 diff 检查通过。开发过程中新增用例先暴露平台工具独立参数解析分支未使用检查点，修正后通过；初始失败日志保留。

下一步合入 main，构建签名 Host，通过公开停止、核对、返工命令在原数据集恢复。完整闭环通过前不得宣称本轮验收成功。
