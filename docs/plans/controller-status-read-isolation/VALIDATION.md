# main 上的状态读取回归验收

2026-11-14，独立 worktree 的 8092bf1 通过 6a123c8 合入 main 后验证。合并保留并行经理配置选择改动；未提交其他任务的工作文件。

## 问题与证据

真实工程验收中，旧页面等待55秒仍显示 manual / 代次0，但同一Host的公开编排及driver查询分别已返回 agent / 代次1与活动经理。配置目录仍等待使旧 Promise.all 未更新运行状态。旧程序对新增两条回归均按预期失败。

## main 检查结果

- 浏览器相关回归7 passed、0 failed：目录持续等待、目录失败、慢轮询/卸载、后继经理选择、Standard默认/历史读取和迟到默认值保持比较。
- 编排集成35 passed、0 failed，包含并行经理配置接替约束。
- Web构建、release Host构建及签名成功。
- 同一真实数据集在新Host重新打开，页面显示“当前调度：agent · 代次1”和“协作中”，与实际查询一致；目录继续等待不再遮蔽运行状态。

原始日志、旧/新DOM、截图及网络时序保存在本地 `/Users/hengliu/workspace/devdesk-acceptance/20261114-013802-standard-main-final/`：`controlled-old-ui-regression.log`、`main-controller-ui-regression.log`、`main-integrated-orchestration-regression.log`、`main-controller-ui-build.log`、`audit/evidence/screenshots/13-execution-settled.*`、`14-main-resumed-manager-state.*`、`ui-manager-fix-validation.json`。

此记录证明该修复的定向回归与实际页面复查；完整Standard工程流程的最终结论以独立保留现场的62步、真实测试/评审/Git账本为准，不据此宣称剩余业务步骤完成。
