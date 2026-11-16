# main 剩余改动整理与定向验证

日期：2026-11-16。整理基线为 `026c05d51fd17c90d0e5d4a6a97e7e8e9dc7b29c`；本次只复核 main 已有未提交内容及其必要修正，不合并另外两个正在开发的并发/运行性能分支。

## 内容与完整性

| 范围 | 提交的内容 |
|---|---|
| 运行诊断与经理状态 | Docker create 分别捕获 stderr，仅返回白名单错误类别；daemon 可达且容器确实不存在时才归类为 technical/not_applied。经理投影真实失败原因，已失败输入不自动重发，显式恢复建立后继会话。经理指令遵循冻结步骤的确认策略。 |
| Host 启动器 | macOS 独立签名副本继续保留；新增启动、信号与退出的结构化日志，不自动转为后台服务。 |
| 权限恢复 | 同一次工程快照加载中复用完全相同的权限选择与作用域；下一次加载清除此类结果并重新核对资产。保持 128 项 memo 与单条 64 KiB 保留限制。 |
| 执行页 | 默认进入项目经理，保留显式 detail 深链与前进/后退；启动入口、阶段布局与移动端内边距同步更新，相关审批测试指定正确面板。 |
| 设计与阶段报告 | 并发 P0–P6、运行性能 F0–F6 继续标记待实施；同步当前约束与文档入口。Feature 自主执行报告保留 17:41:38 的阶段结论，不宣称完整交付成功。 |

整理前备份全部 39 个文件及二进制补丁。原有源码、文档、四份方案/合同均保留；审计快照及导出产物按仓库约定归档到本地证据目录，不将运行数据推送远端。报告中的附件链接已更新，原始报告也完整归档。核对 57 次调用（56 成功、1 拒绝）、3 次人工审批、0 条额外业务输入；9 份产物的 canonical JSON SHA-256 全部匹配冻结的 value_digest。

定向浏览器检查发现一处既有测试竞态：`workflow.step.confirm` 只携带整行 revision，后台观察更新会使其过期。测试改为使用 `query.workflow_instance` 的活动计划、control_version、execution_id、input_digest 与实际 outputs；未放宽产品版本校验，未增加命令重试。

## 验证结果

- `check-style.sh` 通过：rustfmt、ESLint、TypeScript、Prettier、Web 构建、workspace/all-targets clippy `-D warnings`。测试修正后再次检查该文件的 ESLint、Prettier 及前端 TypeScript，通过。
- Rust 定向 19 项通过：source_views 6、Docker 单元 3、进程捕获 3、经理启动失败/重启去重/审批唤醒/待发送合并/未知效果不重试各 1、权限规则 2。权限测试中 4 项真实外部入口默认 ignored；其中本次有关的 Docker create 拒绝用例另显式运行并通过，共 20 项通过。
- 真实 Docker create 拒绝探针使用本机已登记的 `devdesk/managed-codex:9cd5a40e69bab95a3e1b39e5`，不启动 worker、不调用模型，确认容器不存在及 NotApplied 分类。
- debug Chromium 15 个定向场景通过：执行布局 2、Feature 创建与审批 4、Model 审批 3、配置加载/草稿/SSE/目录/账号隔离 5、Workflow 编辑与确切结果确认 1。最后一项修正后连续 3 次通过。桌面与移动端截图已检查。
- Host HTTP 合同通过：认证、Host/Origin、静态资源、数据路径、进程锁、幂等、Artifact、SSE、关闭/重启持久化与项目隔离。使用专用临时数据集。
- 启动器探针通过：实际签名 C 夹具正常退出、SIGTERM/SIGINT/SIGHUP 转发及退出码、资源复制和临时副本清理；非 macOS exec 路径另用 mock 核对。
- 权限资产、权限 Schema、出厂目录、架构检查通过；CI 门禁与配置缓存 Node 测试合计 13 项通过；相关 Markdown 链接已核对。

未运行完整 workflow/Feature 交付回归、完整 Rust/workspace 测试或全量浏览器套件；未恢复原现场流程，未调用付费模型。上述定向结果不替代 P0–P6/F0–F6 的容量与持续运行验收。

原始证据：`/var/folders/y2/x2npypvx45d_btcdpbpw5cvr0000gn/T/devdesk-main-review-20261116-w5h_q1ny`。保存原始文件清单、补丁、报告与产物副本、Rust/浏览器/HTTP/启动器日志、截图及内容摘要。
