# 项目经理事件自主推进实施记录

状态：实现和真实默认 Feature 验收完成。实施基线 main aa7328daa8e1d94d47fb4c44e0506d59aefec690；分支 codex/event-driven-project-manager；工作区 /Users/hengliu/.codex/worktrees/event-driven-project-manager/devdesk。方案始于 2026-11-16，真实运行及最终复核跨至 2026-11-17。debug 浏览器全量结果为 83/84，保留的一项 Sandbox 配置定位超时见下文。

## 实现

- 复用现有 Store 事务和 events 日志，在工作流状态、确切人类决定和 writer 停止变化时登记结构化事实；根范围索引采用 project 50，global 保持 19，不迁移用户数据。
- Controller 使用一次初始目标、持久化源事件/处理游标、投递身份及检查点。后续只发送有界事实、真实用户输入引用和期限信息；删除每轮固定编排 instructions 与全图摘要驱动。已有 Store 的提交机制保持原有边界。
- workflow.controller.wait 原子保存检查点、确认当前投递并检查未处理事件。只有收到确切原生回复的本方动作可合并，不越过同时到达的外部结果。waiting_for 是经根范围校验的实例或决定引用；所有新根事件仍可唤醒，不用引用过滤掉人类决定。正常轮次结束却未登记等待时暴露 needs_attention。
- 空闲 SQL 提示跳过无待办的等待，仍监测登记等待后继续运行、失败或取消的经理 Turn，保留超时、租约、停止及未知结果保护。未收到首条消息的独立工作台不因 bootstrap 代次不同而空转。
- 事件按 32 条及 64 KiB 分批；大于 16 KiB 的单条通知保留来源及执行引用，明细从原记录读取，原事件不改写。事件数量、检查点和本方动作不刷新业务进展预算。
- workflow.controller.notify 和 decision.request 的 interaction 保存事项、需要人类做什么、相关材料、已做工作、审核结果及执行依据。内核核对当前产物、冻结上游材料、根范围和执行版本，补全实际 owner/Session/Turn。常规审批先满足除人类确认外的接纳条件；真实 Human 签署继续使用原入口。
- 项目经理页展示完整交互卡片、确切材料及每份材料的上下文/执行记录链接、意见和确认操作；删除“调度与经理管理”“查看确切请求”和最近工具调用流水。过期决定可读但不可签署；故障与恢复入口保留。
- 冷启动阅读页在工作区详情尚未返回时使用 URL 的 Feature 范围，材料、上下文、执行记录一致处理，避免先退回项目范围而误报不存在。
- 出厂 Codex 经理 Agent 10、Capability 9、编排 Skill 3、decision-request Tool 2，以及 wait/notify Tool 1 通过 manifest/defaults 采用。GLM/Minimax 经理仅更新共享 Skill 引用，未扩展其根编排支持。历史数据库的不可变源不原地覆盖，manual/rust 驱动、Profile 权限和人工治理继续共用内核。

## 自动化验证

| 范围 | 实际结果与证据 |
|---|---|
| Rust workspace | 391 通过、0 失败、26 项原有外部/性能入口 ignored；日志 /tmp/devdesk-event-manager-workspace-complete.log。最后补充的独立工作台空闲边界所在 runtime_queries 模块 6/6，通过日志 /tmp/devdesk-event-manager-idle-final.log。 |
| 事件及编排专项 | 核心事件 8/8、编排集成 51/51；覆盖等待竞态、持久化恢复、确切回复、撤权、人工门禁、跨根积压、字节分页、超大通知、材料来源及原生经理只凭真实决定继续。 |
| 独立证据校验器 | 工程证据 23/23、自主性审计 10/10，含篡改指导、Human 代经理推进、伪造批准来源、删除门禁/节点/完成规则、改独立评审指令及伪造分块读取的拒绝。日志 /tmp/devdesk-event-manager-evidence-batch.log、/tmp/devdesk-event-manager-autonomy-delivery.log。 |
| release Chromium | 完整 84/84；最终材料来源链接改动后，经理阅读专项 11/11 再次通过。日志 /tmp/devdesk-event-manager-release-final-all-r2.log、/tmp/devdesk-event-manager-provenance-ui-final.log。 |
| 风格及合同 | rustfmt、clippy -D warnings、ESLint、TypeScript、Prettier、Web build 通过；默认目录、权限、Schema、架构、RPC 入口清单通过。最终风格日志 /tmp/devdesk-event-manager-style-provenance-final.log。 |
| 跨进程 | 最终 release HTTP 认证、Origin、静态资源、数据集锁、幂等、SSE/Unicode、退出重启和隔离检查通过；模板重置、数据集 19/50、运行准备及 Node 报告器入口通过。日志 /tmp/devdesk-event-manager-http-final-r2.log、/tmp/devdesk-event-manager-release-contracts-final.log。 |

debug 最终全量 83/84：permission-resources.spec.ts 的 Sandbox 修订在 reload 后定位超时，原始日志和截图保留在 /tmp/devdesk-event-manager-debug-final-all-r5.log 及对应输出目录。其他轮次另有配置失效 AbortError 和布局测量瞬态。Standard 两项测试补齐按需目录的焦点操作及初始采用配置加载完成的等待。main aa7328d 临时副本的 4 项及重复 9 项对照通过，未复现相同瞬态，不能据此断言问题来自基线；目前原因尚未定位。对照完成后移除临时副本并重建本分支。debug 初轮从错误目录启动造成的夹具路径错误也单独保留，未计作通过。

自动无人工门禁的原生经理协议夹具验证自行派发、接纳和完成；真实默认 Feature 则保留原人工门禁。协议夹具与真实模型验收分别报告。本次未执行 100 前端性能验收。

## 真实完整 Feature

场景目录 /tmp/devdesk-event-manager-live-20261117/run-04，独立专用空数据集及 xfork24/devdesk-integration-test；原生 Codex、Docker、Git 和 GitHub 均实际执行。主工作树及用户已有运行数据未改动。当前默认 Feature builtin-workflow-feature-sol-high:8 的 22 个节点及实际 Task builtin-workflow-task-sol-high:7 的 20 个节点全部完成，未删除节点、确认、完成条件、独立评审或原始证据要求。

| 身份或事实 | 核验结果 |
|---|---|
| Project / Feature | bfabcd67-7911-46d9-aa7f-1ae825a5a989 / 990ba4cf-cf21-4399-b8e8-2bfe8a360c35 |
| Root / Task instance | 6ade3cc7-e78b-4664-bf71-f7af6254f263 / 61a794f2-f29d-4404-b74a-52dc964d6b65 |
| Controller | 3f01a95f-be4e-4e6d-9c28-c1b767d84d76，epoch 1，最终 closed；期间未 takeover/resume。 |
| 初始输入与后继 | 一次 Human 正式启动，61 个经理 Turn，仅一个 bootstrap；controller_input 和补充调度指令均为 0。 |
| 实际控制来源 | 86 次后续业务决定全来自经理或其授权段；276 次成功原生控制调用。经理显式完成 Task、处理父调用并完成根流程。 |
| 事件 | 实际投递 98 条，无跨根或重复投递；最大事件批次 2520 字节，符合最终 32 条/64 KiB 约束。 |
| 人工确认 | PRD、Design Doc、Task Plan 三次真实请求、初审和确切 Human 签署；按用户“本次实施均批准”的授权在页面提交，保留本轮执行、输入摘要和输出身份。 |
| TDD 与测试 | 两个真实增量 Red/Green 对、原始命令/退出码/源码快照；平台 Red 为预期失败。独立 Task/Feature 测试及最终候选测试保留，Feature 最终配方 14/14，通过且无跳过。大材料两处经原生工具完整分块读取并核对全部字节。 |
| 发布 | Task PR [42](https://github.com/xfork24/devdesk-integration-test/pull/42) 对应 dcd198faf8ab2e9472eb250ccffebcb39fad74ae；Feature PR [43](https://github.com/xfork24/devdesk-integration-test/pull/43) 对应 045cdec561ab49fa83b5fefe04b3b6d6e52277a1，候选、测试文件和发布观察精确一致。 |
| 最终人类反馈 | 经理原生发布“greeting 功能已交付并合入 main”，附交付报告、最终测试、独立评审、发布回执和实际执行记录。 |

自主性依据 autonomous-orchestration-validation.json；独立工程依据 raw-evidence-validation.json、raw-tdd-evidence.json、reviewer-wire-deliveries.json、reviewer-input-reads.json 和 publication-evidence.json。完整步骤与来源见 EXECUTION_SCENE.md、step-ledger.json、evidence-index.jsonl 及项目一致快照，不以模型摘要替代执行证明。

原完整运行二进制 SHA-256 为 02a0a26dc63d0fbbfaf23aa65197528d21e37aa586e71693ea7b451d57144b21，原探针和源码副本保存在该场景。后续补齐字节边界、独立工作台空闲提示及阅读页面修复后，最终构建 df121927b5ab333fa1439f3a01345181372c163475ec40c0d6128d2bdd9c05ff 只打开已完成场景复核。新增边界由定向测试验证，真实轨迹再次接受最终审计；不能把最终构建的只读复核说成重新跑了一遍完整业务流程。

最终页面复核从经理页面实际点击链接并使用浏览器返回：三张已签署卡片分别通过 14、13、6 个精确链接，最终交付通知通过 10 个，共 43 个；桌面及 390 像素窄屏截图保留。final-ui/validation.json 记录复核前后总 Turn 均为 77、业务决定记录均为 87（含最初 Human start）、epoch 均为 1，未新增模型输入、签署或业务推进。当前核验器又对保存的完成视图进行离线复核，通过日志 /tmp/devdesk-event-manager-offline-completed-final.log。

## 保留的尝试与校验修复

- run-01 探针遗漏 start_workflow=false，创建后再次启动被内核拒绝；保留，未计成功。
- run-02 真实经理已自行派发需求、接纳 PRD、安排独立审核和准备审批，期间自己修正等待引用及遗漏上游材料的请求。为切换掉终端审批路径，在签署前停止，保留为部分场景。
- run-03 的 PRD 链接与页面通过，按用户明确授权签署后经理自行进入工程约束及设计。发现等待后仍需监测当前经理 Turn 的保护边界，修正并测试后停止此部分场景，未计完整通过。
- run-04 从新空数据集重新开始，探针仅观察；临时页面操作脚本核对经理真实请求及全部确切链接后，依据用户授权签署三次 Human 决定，不创建申请或派发/接纳/返工。完整业务过程持续到最终完成，未由外部逐步补推。
- 规划审批首次打开材料时曾因冷启动采用项目范围误报不存在。核对确切 Feature 后端及全部链接后才签署；保留首次失败，App 的 Feature 范围修复有确定性延迟加载回归。
- 原运行完成后的独立校验报 Large input read has a missing chunk at 0，原探针退出 1，failure.txt 保留。调查证实 closure_review 第一块通过合法批量 IDs 读取，API 按 ID 数分配字节预算，旧校验器仅识别单 ID，因此漏计首块。修复后仍核对完整原生回执、请求输出列表、每份预算、重叠字节、UTF-8 边界及 SHA-256；23 项回归、已完成场景在线只读复核及离线复核全部通过。未修改原产物或通过补读救助业务 Agent。
- 最终页面核验初轮连续创建并提前关闭阅读标签页，下一张卡片的 query.workspace 在 15 秒内未返回，capabilities 正常；时序和失败截图保留。核验器改用真实链接点击及浏览器返回，三次审批和最终通知的全部链接通过；未调高断言超时。日志 /tmp/devdesk-event-manager-final-ui-revalidation-first-failure.log、/tmp/devdesk-event-manager-final-ui-revalidation-r2.log、/tmp/devdesk-event-manager-final-ui-revalidation-r3.log。

所有日志、数据库、原生上下文、截图、执行源码副本和认证均留在临时证据目录，不进入提交。专用测试登录副本已清理，最终只读 Host 已退出。当前兼容边界为 global 19 / project 50，旧项目数据需继续使用匹配构建；本次未执行迁移、删库或转换。
