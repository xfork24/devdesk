# 持续工作流事件下的页面刷新

在 `devdesk-project-1` 的真实验收中，入口记录一次 `query.workspace` 40 秒读超时；页面早期截图显示加载状态。独立源码检查发现 SSE 每次刷新都会增加请求代次，当事件比查询返回更快时，前一次结果不断失效。该缺陷使用 200ms 事件和 800ms 查询延迟的浏览器回归稳定复现：修复前可见项目标题始终未出现。

页面现在记录进行中的工作区读取，将这段时间内的事件合并为一次后续刷新。项目切换和命令完成后的强制读取继续使用原有代次隔离，不复用旧项目或内核退出前的结果。没有调整工作流、Standard、测试要求或业务状态。

Web 构建通过。修复前的相同用例失败；修复后在 2.7 秒内显示可见项目标题，并断言同时仅有一次工作区请求。另 9 项 workbench 和 kernel-connection 回归通过，包括内核切换、关闭后迟到响应隔离和设置命令刷新。初版新增用例选中了隐藏的 option，修正为可见 banner 标题；早期截图等待也有同样定位问题，不能据此宣称真实页面持续 45 秒未加载。所有原始失败、修正与截图保留。

完整日志和运行现场位于 `/Users/hengliu/workspace/devdesk-acceptance/20261111-230248-project-1-workflow/`：`ui-refresh-red-corrected.log`、`ui-refresh-green-corrected.log`、`ui-refresh-green.log`、`recovery-driver-2.log`。受控浏览器用例只验证刷新调度，不冒充真实工程运行或 Git 回执。40 秒超时的完整因果仍须以恢复后的真实运行确认。
