# 工程能力对齐：执行约束与进度

日期：2026-11-13。工作分支：`codex/engineering-capability-alignment`；独立 worktree：`/Users/hengliu/workspace/devdesk-worktrees/engineering-capability-alignment`。

基线快照 `3203ba181f75d7dda8fcae6cef7abe1793fb3dbb` 包含原 HEAD 及开始执行时工作区的源信封改动、review 和本路线图。快照通过独立 Git index 生成，原 main 工作区与 index 未改动。

用户追加的验收要求覆盖原计划中的全量工程流程要求：各阶段依次实施、针对性验收通过后提交，再继续下一阶段；**不运行全量 Feature/Task 开发流程测试**，包括对应的长时真实模型工程流程。采用纯单元、局部集成、契约、指定浏览器/HTTP 与必要的资源探针。未运行的完整工程矩阵明确保留为未验证，不能声称已通过。

## 阶段记录

S1 结果见 [分层执行与验收](01-layering/EXECUTION.md)。后续阶段按实际验收追加记录。

S1 已提交 `cfc40c2`。S2 的实现与针对性验收见 [配置执行记录](02-configuration/EXECUTION.md)；下一阶段为 S3。

S2 已提交 `5336f89`。S3 的配置化运行资源、实际协议探针与限定验收见 [运行资源执行记录](03-runtime-resources/EXECUTION.md)。

S3 已提交 `09f2bb2`。S4 的双驱动控制实现及限定验收见 [调度执行记录](04-orchestration/EXECUTION.md)。

S4 已提交 `dd2b4e1`。S5 的独立经理工作台、配置工具及限定验收见 [经理执行记录](05-project-manager/EXECUTION.md)。

2026-11-14 用户补充收口顺序：六阶段全部完成并初步验收、提交后，先将工作分支全部合入本地 main，再在 main 做最终验收；worktree 不执行 Feature/Task 开发的端到端验收。合并前核对原工作区基线，保留已有改动。

S5 已提交 `37298c1`。S6 可执行规范与证据导出实现、初步验收见 [规范执行记录](06-specs-and-acceptance/EXECUTION.md)。真实验收仓库由用户明确指定为 `https://github.com/xfork24/devdesk-project-1.git`；用户同时要求本地产物、结果和验收报告。完整 Feature/Task 端到端验收只在合入 main 后执行。

合并核对发现 main 已新增 `6d7d2a0`（源信封及原生验收提交）；与本工作分支起始快照相比，源实现一致，额外变更集中于原验收脚本/证据校验与记录。合并会保留这些新改动及原 untracked 计划/review 文件，不用旧快照覆盖 main。

## 本地 main 最终验收执行

S6 已提交 `317c191`。六阶段与 main 原有改动在 `f01dec6a05308a11fe94016405829156e9fa05a5` 汇合，随后 main fast-forward 到该提交。原 main 的 34 份 untracked 计划/review 文件先逐文件核对、备份，再由合入的相同内容接管；没有丢弃原工作区。完整 Feature/Task 验收从此以后才在本地 main 运行。

本轮全部原始日志、失败数据、运行包、调用记录和产物归档保存在 `/Users/hengliu/workspace/devdesk-acceptance/engineering-capability-alignment-2026-11-14`。每轮归档保留一致项目 SQLite、登记产物和 SHA256 清单；认证目录与 worker 私有状态不进入导出包。失败归档不覆盖，修正归档另建目录并追加索引。

完整独立归档复核进一步发现 WAL 源的临时 sidecar 被列入清单，关闭连接后消失。`78011ec` 显式关闭连接并把备份转换为独立数据库后再生成清单；真实 WAL 回归通过。原有不完整清单保留，新的 stable 包重新导出并独立核对所有文件摘要、登记 Artifact、SQLite 完整性及表计数，不能仅凭原 `artifacts_verified` 标志宣称整个清单完整。

main 验收发现并修复了实际集成问题：真实 runner 显式选择 Agent 调度；浏览器旧 fixture 跟随当前导航和驱动查询；Docker 配方共享标签重建后仍保留既有内容地址镜像；原生模型接收完整 EvidenceLocation Schema；归档按生产 Artifact 的项目相对路径解析；历史任务从当时实际发送且摘要一致的消息恢复，避免当前渲染器重新生成历史合同。另补充有界恢复观察、运行脚本与包摘要保存、项目本地配置查询和长请求超时，未知变更结果均先核对，不能盲目重试。

实际经理验收还暴露了根控制器无法采用后继 Agent/Permission Context 的缺口。`eeac8ee` 在核对旧 writer 停止后建立新控制器身份、冻结新策略并转交唯一根租约；旧身份与策略保留，迟到调用和被替代控制器的恢复被拒绝。没有原地改写历史策略或放宽人工门禁。验收显式选择 1800 秒预算，其他授权不变。

验收期间另有独立工作合入 main：`e8010eb` 的精确引用说明与 `6a123c8` 的经理状态独立加载，均保留并纳入最终回归。

进一步核对独立包发现，权限预设会回退到编译时源码目录。`3f061df` 改为从包内资产生成数据集内的内容地址缓存，自定义来源仍独立选择；新增缓存损坏/移除恢复测试及架构门禁。14 项权限集成测试通过，1 项默认忽略；修复后的 Release 包在 OS 明确拒绝读取项目/worktree 源码目录时，通过实际权限 UI、预设重载与来源恢复场景。完整 Rust 自动工程验收将沿用该隔离。

2026-11-14 当前已完成：workspace 271 项通过、0 失败、17 项默认忽略；显式真实双配方/跨数据集镜像保留验证通过；独立 core/domain 构建、HTTP、Web、分层和归档复核通过。当前 main 的独立 Release 包在源码目录外通过全部 57 项浏览器测试。经理五项真实对话配置操作全部完成，另有 22 项独立持久化断言，以及五次成功 Native apply 对精确预演摘要和实际通信 journal 的关联核对。完整 Agent/Rust 工程流程的最终结果与限制以最终验收报告为准。

后续权限预设修复后的完整回归为 272 通过、0 失败、17 默认忽略。Agent 默认 Feature/Task 已完成同库恢复验收，PR #4/#6 远端合并与实测候选 SHA 一致，原始 TDD 与评审续读通过独立核对；最终 stable 归档 413 文件复核通过。首轮失败仍保留，完整 Rust 默认图继续验收。详见 [最终验收记录](FINAL_ACCEPTANCE.md)。


### main 可信运行事实修复与复验

真实 Rust 默认图在 Task 评审处正确拒绝，实际运行镜像符合要求，但 Agent 输入缺少可信运行事实。`7ef4008` 增加白名单投影、确切生产者 Turn/operation 停止证明及冻结任务字段，并在驱动诊断中显示未满足的完成门禁；没有批准、覆盖或放松原检查。274 Rust 测试通过（17 默认忽略）、新包 HTTP 通过、OS 拒绝源码读取时 4 项浏览器回归通过。旧拒绝与稳定归档保留，新的完整自动复验使用 `rust-default-flow-retry`。最终状态及证明边界以 [FINAL_ACCEPTANCE](FINAL_ACCEPTANCE.md) 为准。


### main 最终验收完成

最新代码 `7ef4008` 的全套 Rust 274 项通过（17 默认忽略），独立 Release 包在被测主 Host 拒绝读取源码目录时通过全套 57 项浏览器，HTTP/认证/重启通过。真实 Rust 默认 Feature/Task 父子图全部完成，42 次派发、42 次接纳、2 次完成均为 Driver，零经理 Controller/人工审批/豁免；PR #7/#8 与实测候选一致。真实 Agent 图经辅助恢复通过（PR #4/#6），经理五项配置创作通过。

原附加离线审计误读 source codec，保留非零退出日志，仅修正重跑审计；业务流程未重放或改动。最终核对 16 份原生任务、29 条生产者停止证明、两轮增量 Red/Green、两份大输入完整续读。9 份完整归档和 11 份历史不完整导出均逐项记录，认证副本已清理。阶段结论、具体包、故障记录及 A01–A12 覆盖边界见 [FINAL_ACCEPTANCE](FINAL_ACCEPTANCE.md)。全部工程端到端在合入本地 main 后进行；产品仓库未推送远端。
