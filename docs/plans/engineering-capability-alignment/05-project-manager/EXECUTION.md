# S5 执行与阶段验收

增加独立项目经理工作台、项目全部业务记录类别的分页/分块查询、产物读取、配置目录/历史/使用方、实际资源报告；新增有限且有 Schema 的原生工具，没有任意 RPC/SQL/文件读取工具。Project 由物理项目服务和 Controller 绑定，拒绝跨项目参数；遮蔽已知系统凭据字段。概览不冒充正文，摘要/游标校验读取连续性。

经理配置写工具复用 S2，扩展实例 create/rebind/remove、Skill 包导入、权限绑定。全局配置服务和项目快照 inventory 经 ports/Host 装配，业务判定仍在 core。全局写要求独立冻结 grant，跨库 stage/apply/status/cancel 保留 Actor、原候选及恢复回执。配置写与 UI 使用同一协调锁；批量源/生成事实再做完整依赖验证。预演 principal 固定会话/epoch，不固定模型 Turn；应用保留真实调用来源。自身权限目标禁止经理换绑，自身实例变化在回执后撤权、暂停并要求后继会话。工作台与根驱动独立，切换 Rust 不撤销工作台。

出厂 PM Capability/Agent/授权上下文发布后继修订，新工作台 Skill/工具在资源包声明；没有 Rust builtin ID 特权。项目 UI 增加无 Feature 时的工作台、精确 Agent/授权选择、会话/工具回执与暂停/后继入口。

初步验收：

- 8 项限定集成测试：五个创作场景实际 native admission → preview/apply → SQLite/回执；大 Unicode 材料及目录续读；跨项目/未授权全局拒绝；冻结授权下共享发布；迟到调用拒绝；实例稳定预演/换绑/移除；Rust 控制根时工作台创作；跨库 Actor/采用；Skill 包与权限绑定组合。使用明确脚本 Harness，无模型调用、无 Feature/Task 开发流程。
- 配置变更、组合及 Schema 针对性回归 34 项；原生 Controller 重启/切换回归 2 项通过。
- 无 Feature 的经理工作台浏览器入口、Web production build、分层和目录自洽检查、workspace test target no-run 编译。

日志 `/tmp/devdesk-s5-*`。脚本 Harness 证明真实调用链/持久化，不代表供应商对话质量；真实模型五场景和全部授权故障组合留待最终验收。没有运行 worktree 上的完整 Feature/Task 开发端到端验收。
