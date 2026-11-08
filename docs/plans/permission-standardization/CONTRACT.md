# Permission 实施契约

本契约固定本轮序列化、执行主体和支持范围；运行结果见同目录执行报告。

## 对象与边界

Permission 保存稳定 identity、不可变 id/version/supersedes 和完整 policy。PermissionRef 精确引用 revision_id。Binding 包含可选精确引用和 overrides；作为目标基础绑定时必须提供引用，在覆盖层中可以仅提供字段覆盖。

policy 的 filesystem 使用 full_access、read_only 或带绝对目录 rules 的 scoped；network 使用 enabled、disabled 或 allowlist；tools 使用 all_available 或带 allow/deny 的 restricted。字段未知、显式 null、路径穿越及过量规则拒绝。工具 deny 优先，文件最具体目录规则优先，同路径 deny 优先；规范化排序后的语义参与配置摘要。

层级为目标绑定、项目、角色、本次执行。指定新引用时从该修订重新开始，再应用本层字段覆盖。仅字段覆盖保留此前其他维度，列表整体替换。每次解析记录来源；完整策略和原生编译选项进入运行锁，历史修订禁止删除和原地更新。

审批、ExecutionScope、SandboxIsolation、ResourceLimits 独立。Harness 限制工具访问；Sandbox 限制整个环境。领域代码授权不能被 Permission 放大。新默认普通用户、cap-drop/no-new-privileges、容器可写层与 exec 临时空间；ReadOnly Review、测试写路径和被保护元数据通过挂载继续约束。独立测试使用无模型连接、只读代码导出、无网络、无模型凭据的固定策略。

## 固定后端能力

| 后端 | 本轮支持 | 明确不支持或限制 |
|---|---|---|
| Codex 0.167.0 | 原生 full/externalSandbox、只读、限定 worktree 写入、工具网络开关、never/on-request | 通用工具白名单和域名 allowlist 不支持；legacy 原生功能集由原配置等价映射 |
| DSH 0.2.2-rc.1 | 原生 preset/文件模式、动态工具规则、never/ask；受限 runner 可隔离工具网络和状态 | 工具禁网必须启用受限文件模式并限制为 bash/ask_user_question；FullAccess 跳过原生沙箱；scoped 限制必须已由 ExecutionScope 挂载落实，否则拒绝 |
| Docker | 当前用户、注册范围挂载、只读/可写容器层、bridge/none、临时空间、资源额度及明确隔离 profile | 不提供域名过滤、不理解模型工具名；整个环境禁网无法维持模型连接 |

FullAccess 以整个容器为信任范围，不声称隔离同容器模型凭据。legacy 受限策略保留相应状态隔离，实际验证使用假凭据；旧运行缺少标准快照时保持缺失，不反填成功保证。

NativePermissionOptions 是编译器生成、带 provider/version 的原生选项快照，用于复核实际 profile、sandbox mode 和 approval policy。本轮不接收任意供应商 YAML/TOML 或未验证字段；用户通过标准 Permission 字段覆盖，后端能力不足明确拒绝。

## 持久化与命令

全局 v4 保存共享修订和带 CAS revision 的配置。项目 v32 保存权限及覆盖，v33 将快照保存到每个 RuntimeExecution。共享修订追加复制至项目；运行定义按配置摘要区分，镜像安装不复制。原始全局 v3/项目 v31 及更旧数据沿顺序脚本升级，原始数据集的默认映射为 legacy。

permission.create/revise/bind 使用 command_id，配置版本检查与结果落库在同一事务；重放先返回原结果，跨配置/项目预留复用 ID 拒绝。预检无模型、容器或 PR 副作用。修改默认值、项目/角色绑定不会影响已创建 Session、Run 和技术重试；后端版本不兼容时仅核对/停止原对象，不自动重发输入。
