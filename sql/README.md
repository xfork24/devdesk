# 当前 SQLite 结构

当前运行只执行新合同；旧 v4 数据通过显式离线转换保留。

| 文件 | 数据库 | user_version |
|---|---|---:|
| `global/schema.sql` | `<data>/global.sqlite3`，共享配置、安装与项目索引 | 14 |
| `project/schema.sql` | `<data>/projects/<project-id>/engineering.sqlite3`，材料、会话与流程运行 | 45 |

空库在事务中直接创建当前结构。其他版本会被明确拒绝；旧数据必须显式转换，禁止通过删库升级。不要在运行路径加入隐式迁移或旧动作回退。

Workflow 修订、冻结计划、StepExecution、实际输出、确认、历史轮次与工具回执保存于版本化治理记录；材料使用显式 `input_refs`，Feature 使用 `source_materials`。旧 Attempt、Run、Delivery/Review、Merge 等业务调度表及旧配置索引已删除。原生 Session、Turn、RuntimeExecution、仓库操作、Artifact 与幂等记录继续承载实际执行事实。

模板目录使用身份、代次及不可变成员索引。重置只切换当前目录，正常运行中的冻结引用保持确切修订。凭据独立保存于数据集凭据目录或系统凭据库，不写入业务配置与运行数据库。

当前结构已删除对象上的空 BindingLock、项目运行时偏好、Definition 的旧 slots/rules，以及旧 execution_task 索引。每个 Step 独立冻结 Session 运行配置。

`orchestration_wakeups` 保存持久化唤醒及消费状态，`orchestration_leases` 以唯一约束维护根实例单写者及 epoch。计划、决定、材料和 permit 保存为治理记录。

离线升级（先停止 Host，并停止/核对全部外部执行者）：

```bash
python3 scripts/upgrade/harness-v5.py /absolute/data --binary /absolute/devdesk-host
python3 scripts/upgrade/harness-v5.py /absolute/data --binary /absolute/devdesk-host --apply --backup-directory /absolute/backup
```

入口锁定数据集，备份每个 SQLite 文件，保存不可变 upgrade_archive 和确定性 ID 映射。跨库 journal 完成前 Host 拒绝打开；中断后重跑同一命令继续，不覆盖备份。可用 `--max-databases 1` 在数据库提交边界建立检查点。原 Artifact、凭据和仓库不改写。恢复备份前必须停止新的 worker，整组恢复一致备份，不能将计划切换当作 Git 回滚。
