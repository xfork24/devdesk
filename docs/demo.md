# 演示与故障回路

所有操作从 Web 完成，外部结果明确标注 origin=simulated。模拟场景选择只影响之后启动的执行，不修改已经冻结的 Run。

## 主场景

1. 初始设置选择空 data 目录。创建 Project，填写 Workspace 描述，然后创建 Feature。
2. 在“文档与上下文”新建 Requirement，填写 Markdown 需求与验收条件。也可以由规划生成模拟建议。
3. 在“执行与交付”选择正常执行，启动规划 Attempt。后端异步执行研究、需求、设计、Task/Test 计划。
4. 分别提交 Requirement、Design/Task/Test 计划 Delivery。阅读交付报告与冻结引用，逐项 Review；可以人工操作，也可请求独立 Agent 评审。接受两个交付后产生初始 Snapshot。
5. 创建“导出接口”“错误处理”两个独立 Task。各自可编辑绑定、设置依赖并查看继承快照。分别启动 Attempt，观察真实时间重叠的模拟 Runs 与写许可。
6. 等待 Red→实现→Green/Docs→verify。提交每个 Task 的实现 Delivery 并 Review。接受后保存最终 Snapshot。
7. 分别申请 Task Merge，等待候选验证，评审对应 Merge 候选 Delivery，再接纳模拟候选。如果两个候选基于相同目标，先接纳一个会使另一个失效；为后者重新申请、验证和评审。
8. 所有 Task 完成后启动 Feature 收尾 Attempt，完成完整 Tests/Docs/verify，提交并评审 closure Delivery。
9. 申请 Feature→mainline Merge，验证、Review、接纳。界面显示“模拟流程已完成”。
10. 刷新页面，查看持久化历史、报告、Context 和初始/最终 Snapshot；可比较不同 Snapshot。关闭浏览器不停止内核。

## 可重复异常

- **技术失败：** 启动 Task 或 Merge 前选择 technical_failure。失败 Run 上点击“重试”，新执行带 retry_of，旧失败保留；Attempt 中后续节点重新按依赖运行。
- **Review 退回：** 对实现 Delivery 退回，说明理由；同一 Delivery 不可再 pass。新 Attempt 生成新的交付与候选，旧版本仍可查看。
- **输入变化：** 修订 Requirement/Design 后查看 stale，旧 Merge 不能应用；重新规划当前输入，再执行受影响的 Task。已完成 Feature 冻结，后续通过关联的新 Feature 承接。
- **目标变化：** 两 Task 的 Merge 都验证并接受后，接纳一个，另一个必须重新申请候选、测试和 Review。
- **规范升级：** 在定义页克隆 Standard，把 planning/implementation/merge 的 Spec slot 指向 Internal Design Spec v2 或 Alternative Design Spec；显式更新 Feature/Task 绑定。新 Review 的检查项目改变，旧 Attempt 的锁保持原样。
- **暂停与取消：** 选择 hold，启动后暂停派发；在途 Run 保持许可。“继续下一步”释放受控等待点；取消须经适配器确认后释放环境。Merge Run 同样有继续、取消和重试入口。
- **质量例外：** assertion_failure 产生失败测试证据，仍可提交正式交付。人工 Override 必须填写理由，指定 review 和 test:RunID 条目。完整 RunID 可从冻结交付/Run 详情取得。不能豁免版本失效、目标竞态、权限或引用不完整。
- **供应商契约错误：** missing_fields 使规划研究 Run 失败；不得被当作成功。可技术重试。
- **资源清理失败：** cleanup_failure 保留 cleanup_failed Resource；不会伪造释放成功。
- **重启：** hold 运行时从设置退出，重新使用同一 data 启动。未确认执行标 interrupted，查看详情后创建重试；重复 Merge command_id 不会再次推进目标。

“历史记录”分页回看事件；各详情列表也分页，不需要手改数据库或浏览器控制台。Archive/data 迁移预览只显示范围与“预览结束”，不会导出、删除或搬迁。
