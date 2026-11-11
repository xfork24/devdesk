# 执行记录

真实现场 `/Users/hengliu/workspace/devdesk-acceptance/20261111-230248-project-1-workflow` 的网络记录 `audit/evidence/screenshots/16-feature-history-network-diagnosis.json` 显示：Add 历史查询约 12 秒成功返回两条记录；目录、项目配置及持续新增的项目管理轮询在 30 秒超时。已有历史被同组 Promise.all 的失败丢弃；2.5 秒周期轮询也会在旧读取未完成时重复派发。

修复在独立 worktree `/Users/hengliu/workspace/devdesk-worktrees/workflow-rework-checkpoint`、分支 `codex/workflow-read-isolation` 实施。项目管理轮询改为读取结算后再等待下一周期，卸载停止后继轮询并继续通过原代次隔离迟到结果；切换实例时清空旧控制者数据。规范历史独立接纳自身查询，目录与项目默认授权仍各自完整读取，授权初始化不降级；历史加载期间显示“加载中”。

3 项定向浏览器回归在旧 main 程序上全部失败，在修复后的 debug 程序上全部通过。覆盖慢查询首个结果可见、轮询不重叠与卸载后停止；目录或项目授权读取失败时历史仍可见。日志在现场根目录的 `read-isolation-red.log`、`read-isolation-green.log`。Web 构建通过。

React 检查：Hook 不含条件调用；效果依赖使用项目/实例标识；异步读取均处理错误；卸载清除定时器和失效代次；成功历史不依赖目录错误；没有引入新库、权限或业务状态修改。受控响应仅证明 UI 行为，不冒充真实模型、测试或 Git 回执。

原工程流程随后达到父级 7200 秒有效执行预算，按合同失败并取消未完成子流程。该失败现场独立保留，不因 UI 修复改写为通过。后续完整闭环需要新执行目录；本修复不复活已取消实例或放宽业务检查。
