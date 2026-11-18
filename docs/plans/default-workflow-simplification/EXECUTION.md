# 默认工作流精简实施记录

状态：本轮已收尾，综合验收未通过，未合入本地 main；结果见 [ACCEPTANCE](ACCEPTANCE.md)。日期：2026-11-18。

## 隔离与范围

基线为本地 main `e3957f3`；实施位于独立 worktree `/home/hengliu/.codex/worktrees/default-workflow-simplification/devdesk`，分支 `codex/default-workflow-simplification`。原始 PLAN 保持不变。测试只使用本次专用目录及用户授权创建的 `xfork24/devdesk-default-workflow-test-20261118`，未改动用户工程运行数据。

用户明确取消详细版原生测试；保留详细版资源和既有行为，仍运行仓库兼容回归并静态比较其执行正文。用户授权助手代为签署项目经理建议批准的阶段，原生业务 Agent 仍无 Human 签署权限；每次外层代签保留材料、确切版本、授权原文和实际回执。

## 实现

- 新默认 Feature 14 个业务步骤、19 个执行节点、5 阶段；Task 10 个业务步骤、14 个执行节点、5 阶段，foreach 上限 4。移除独立 documentation、verify、closure 和重复 candidate_review 交接，文档随实现维护。
- 项目冻结 Node、Python 或 command 回归配置，版本化独立 Red/Green、代码评审、源版本和合并结果两类项目回归；平台回执、原始命令及文件版本按引用定位。
- Task 通过实际 PR 自动合入所属 Feature；Feature 经确切 Human 决定后合入配置主分支。严格远端保护、候选父提交/tree、源发布、目标 lease/CAS、脏目录、重启和未知副作用均保留门禁。
- 同步恢复只在原 PR 已合入的正面证明后产生新的同步授权；原已取消/未知执行及物理回执保留，重试不再次远端 merge。目标竞争只重新准备目标、候选回归与确切人工决定，保留未变源的审查和回归。
- 项目编辑和出厂默认更新保留草稿及自定义选择。根经理页面显示 Task 槽位、依赖、PR、远端合入和本地同步，子流程的审批留在对应子卡片。
- 修复真实验收发现的独立测试报告 runner/超时不匹配，以及父流程仍引用旧 Task Report 结构的问题。精简版分别使用专用独立测试报告和实际 Task 合入同步回执集合合同，并以实际默认编译测试校验。
- 运行计划影响分析按权限内容比较，避免新 Workflow 地址将局部变更扩大为整条重做；输出合同修复可在完整子实例证明下重新校验集合，原失败另存历史。

## 真实验收进展

主场景为 A/B 并行，C 只依赖 A，B 保留完整 90 秒负载。已证明 A/B 同时运行，以及 A 完成真实合入同步后 C 在 B 尚未完成时启动。A、B、C 均已完成独立 Red/Green、两类回归、PR 合入和同步。

已注入并核对 PR 创建响应丢失、远端 merge 成功响应丢失、脏 Feature worktree 和 ref CAS 后索引更新失败。重启后保留原失败；未删除未知用户文件，只清理本次注入且内容核对一致的哨兵；A 的真实 PR merge 只发生一次。

C 的独立代码评审未发现实现缺陷，但因 Reviewer 无权读取未声明的 A 证据而拒绝。外层完整读取 A 独立审查、源回归、合并回归及合入同步链后，按用户授权及经理建议签署仅限证据可见性缺口的人工例外。原 approve=false 报告保留；C 两类回归与真实合入仍实际执行。

另在移除详细版专用出厂资源的隔离副本中，运行无人工等待负载的单 Task slugify Feature。Task 已完整完成真实交付；父汇总旧合同失败已保留，修复经正式计划变更生效，未重新运行完成的子 Task。这两条格式 59 场景随后完成 Feature 整体评审、两类回归、确切人审及真实交付。集成新 main 后另建格式 60 目录，其最终目标与复用需求不一致，经理建议拒绝，本轮已按用户要求拒绝并取消，没有修改输入重跑。

## 验证与证据

最后一轮完整回归为 Rust 622 通过、debug Chromium 117 通过及 7 失败，release 浏览器门禁未执行；不合并本地 main。详见 [验收与遗留问题](ACCEPTANCE.md)。完整回归入口为 `scripts/check/check.sh`；真实 Git/Docker/权限入口已通过。原生流程的 Host、探针、脚本内容身份，原始失败、协议产物、确切审批材料和计时均保存在 `/tmp/devdesk-default-workflow-simplification-20261118`。

性能分项见 [PERFORMANCE](PERFORMANCE.md)，实际执行合同见 [CONTRACT](CONTRACT.md)，详细版专用资源及反向引用见 [RETIREMENT](RETIREMENT.md)。单 Task 场景在汇总修复前已明显超过五分钟，不能宣称性能目标达成，也不把人为负载和外层维护等待归咎于 Harness。
