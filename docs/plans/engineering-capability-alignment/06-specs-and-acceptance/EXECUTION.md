# S6 执行与初步验收

已增加配置驱动的 Unicode 长度、ATX 章节、JSON Pointer 原语，以及绑定冻结输入的 JSON Pointer/行/字符证据定位。草稿 lint、Workflow/子流程输入、材料导入与输出接纳共享检查链；既有 JSON Schema 结构工具复用并补充本地引用展开、线性 pattern、深度/节点/大小边界。输出保存实际 checker/Spec/Delivery/内容摘要；机器结论与语义判断明确分开，不信任模型自填检查成功。

新增派发时的输入证据 manifest，保存实际来源/Artifact 链；报告要求定位时，非空文字不能替代真实引用/摘要/范围，引用文字也必须完全匹配。机器断言的评审结论由内核复算。未声明定位要求的原有语义 Spec 不被虚报为机器事实，保留独立演进方式。

出厂目录新增一个可机器核验 Markdown Delivery、相应结构 Spec 和定位评审 Spec，先作为可采用的独立资源，不整体改写所有旧 Workflow/Spec。经理 Capability/Agent 发布后继以提供 spec.lint。UI 可编辑原语和证据要求，并对未发布内容样本自检，提示语义质量仍待判断。工厂目录专用测试的数量从 manifest 读取。

本地证据导出保存一致项目 SQLite、登记产物及 SHA256 清单、运行资源事实；排除认证目录/worker 状态。真实流程 runner 支持用户明确指定专用仓库，并在停止后导出本地产物/结果。实际端到端调用留到合入 main 后。

初步验收通过：

- 纯原语/Unicode/代码围栏/位置与摘要/伪造机器结论：4 项。
- lint 与实际输出接纳给出同一机器结论；违规输出不封存；重试后正确内容携带同样的检查事实：1 项。
- 实际原生报告中的伪造位置拒绝；重试后真实冻结位置接受；拒绝历史保留：1 项。均为限定 primitive 图、明确 Harness fixture。
- 非递归本地 Schema 引用、线性正则及数据中的关键字区分：2 项。
- 配置变更/组合/Schema 回归 34 项；经理 Rust 模式回归；全部 workspace test target 编译（no-run）。
- 2 项指定浏览器测试：草稿结构自检与现有 Delivery 精确 Spec 预览；Web build、分层/目录自洽。
- 证据导出独立复核：删除源目录后，SQLite/产物/文件摘要仍一致，认证状态不在导出内容。

日志 `/tmp/devdesk-s6-*`。本 worktree 没有运行完整 Feature/Task 开发端到端流程。下一步提交、合入 main，再在 main 对 `xfork24/devdesk-project-1` 运行真实验收并保存本地结果/报告。A01–A12 的最终状态以随后 main 验收报告为准；供应商/平台、全部故障组合和语义质量尚不能仅凭这些初步检查宣告全部通过。
