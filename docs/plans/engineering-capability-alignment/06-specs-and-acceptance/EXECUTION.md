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

## 合入 main 后的验收

六阶段在 `f01dec6` 合入本地 main 后才开始真实工程。验收结果与具体限制见 [最终验收记录](../FINAL_ACCEPTANCE.md)，完整本地证据根为 `/Users/hengliu/workspace/devdesk-acceptance/engineering-capability-alignment-2026-11-14`。

Agent 默认图已通过同库恢复验收：父 Feature、唯一 Task 及两层 Workflow 均 completed；PR #4/#6 的远端合并 SHA 与实测候选一致，原始两轮 TDD、独立评审及大输入续读证据通过独立核对。首轮无人干预失败和全部恢复记录保留，不能记成一次无人干预成功。经理五项真实配置创作也已通过，另有 22 项持久化断言和五次 Native apply 与精确预演/通信记录的关联审计。

main 实测修复涵盖镜像保留、完整定位 Schema、历史实际任务恢复、根经理后继组合采用，以及独立包权限预设的源码目录依赖。归档进一步修正了生产 Artifact 路径、SQLite 临时 sidecar 清单及实际复制字节校验。最终 Agent stable 包含 413 个文件，独立核对摘要、登记产物、SQLite 完整性和表计数，复制认证资料已清理；旧的不完整归档不覆盖。

最新完整 Rust 回归为 272 通过、0 失败、17 默认忽略；前期 Release 独立包 57 项浏览器通过，权限预设修复后在 OS 拒绝源码访问时通过受影响 UI 场景。完整 Rust 默认工程正在同一源码隔离、开发工具屏蔽的独立包上验收；额外 Reviewer Skill 仅澄清既有证据格式，业务图/Spec/门禁保留，Driver 启动后观察脚本只读。该项结束前仍保持进行中。


### main 可信运行事实修复与复验

真实 Rust 默认图在 Task 评审处正确拒绝，实际运行镜像符合要求，但 Agent 输入缺少可信运行事实。`7ef4008` 增加白名单投影、确切生产者 Turn/operation 停止证明及冻结任务字段，并在驱动诊断中显示未满足的完成门禁；没有批准、覆盖或放松原检查。274 Rust 测试通过（17 默认忽略）、新包 HTTP 通过、OS 拒绝源码读取时 4 项浏览器回归通过。旧拒绝与稳定归档保留，新的完整自动复验使用 `rust-default-flow-retry`。最终状态及证明边界以 [FINAL_ACCEPTANCE](../FINAL_ACCEPTANCE.md) 为准。


### main 最终验收完成

最新代码 `7ef4008` 的全套 Rust 274 项通过（17 默认忽略），独立 Release 包在被测主 Host 拒绝读取源码目录时通过全套 57 项浏览器，HTTP/认证/重启通过。真实 Rust 默认 Feature/Task 父子图全部完成，42 次派发、42 次接纳、2 次完成均为 Driver，零经理 Controller/人工审批/豁免；PR #7/#8 与实测候选一致。真实 Agent 图经辅助恢复通过（PR #4/#6），经理五项配置创作通过。

原附加离线审计误读 source codec，保留非零退出日志，仅修正重跑审计；业务流程未重放或改动。最终核对 16 份原生任务、29 条生产者停止证明、两轮增量 Red/Green、两份大输入完整续读。9 份完整归档和 11 份历史不完整导出均逐项记录，认证副本已清理。阶段结论、具体包、故障记录及 A01–A12 覆盖边界见 [FINAL_ACCEPTANCE](../FINAL_ACCEPTANCE.md)。全部工程端到端在合入本地 main 后进行；产品仓库未推送远端。
