# Permission 权限与执行后端

Permission 是不可变修订的文件、网络和工具访问策略，分别绑定 Harness 和 Sandbox。预设位于 `presets/`；固定供应商和内核执行器位于 `backends/`，manifest 记录版本、平台、来源和摘要。

## 版本化 profile 与运行时绑定

设置中的“Permission 定义与版本绑定”使用 v2 JSON。Harness profile 必须声明 filesystem、network、tools、approval；Sandbox profile 只接受 filesystem、network。示例见 `presets/harness-read-only-v2.json`；Schema 为 `schemas/permission-profile.schema.json`。文件和 UI 使用同一规范化与校验入口。

UI 保存追加权限修订，原绑定保持不变。“应用到该目标版本”将确切 Harness/Sandbox 资源修订绑定到所选 profile。绑定也保存不可变修订和执行合同，并以 CAS 更新活动分配；可再次选择旧 profile 实现回退。新目标没有自动全权限绑定，需要显式配置。目标的配置修订、供应商版本、镜像身份和平台分别核验。

Agent 可以选择追随目标的活动绑定，或固定某个绑定修订。新讨论采用当前选择；追随目标的已有讨论在接受下一轮输入前创建带 Snapshot 的后继 Session，保留原 Session、原生线程与执行快照。切换期间输入和停止回执持久化，失败可通过“恢复权限切换与原输入”继续；不自动取消活动工作，不重新发送已接受的输入。工程 Feature 配置冻结确切权限闭包，后续 Run 与技术重试继续使用原锁。

预设来源包括数据集的 `permissions/presets/`，以及发布包同目录中的预设；开发环境没有包内目录时使用源码预设。还可以在 UI 登记普通绝对目录。新增文件后点击“重新加载预设”，无需编译或重启 Host；重新加载不会自动改绑。UI 保存写入权限库，导出提供所选修订及前序依赖，不改写源码或安装目录。同一修订内容冲突、同版本不同 ID、缺少前序或非法字段均拒绝，批次不部分入库。

绑定预检只证明目标兼容，返回 `scope_checked: false`；“实际执行范围预览”可以传入 ExecutionScope。正式执行仍由协调器解析真实范围并冻结，客户端预览不构成写入授权。规则无法由目标执行合同落实时返回 `permission_unsupported`；v2 受限 profile 的 `on_request` 在无法维持最大授权范围时拒绝。

升级会为可证明等价的旧 Agent 追加固定绑定投影，不修改旧 Agent 和权限原始字节；legacy 动态边界及无法等价转换的审批配置继续使用旧合同。完整契约与验证范围见 [实施计划](../docs/plans/permission-runtime-bindings/PLAN.md)及[实施记录](../docs/plans/permission-runtime-bindings/EXECUTION.md)。

## 默认与受限执行

新数据集默认使用 `container-full-access`：Harness 采用供应商原生全权限和无需审批，Docker 提供普通用户、非特权、独立命名空间及授权挂载。容器层和临时空间可写，临时文件可执行。全权限以整个容器为信任范围，工具可以读取该容器内的状态和模型凭据；宿主 HOME、Docker socket 和未授权目录不挂载。

旧数据集迁移为 `legacy-restricted-v1`，保留原有工具禁网和状态隔离方式。新默认不会改变旧 Session/Run；修改绑定只影响后续执行。正式工程阶段的 worktree、测试目录和只读 Review 视图另由冻结 ExecutionScope 限制。

## 旧配置与覆盖

Web 设置中的“旧权限配置兼容入口”保留 v1 预设、修订与全局配置。旧非组合路径中，项目和角色可分别覆盖，优先级为目标绑定、项目、角色、本次执行；旧 Agent 组合固定自身权限。省略字段继承，显式字段替换，列表整体替换；null 和未知字段拒绝。v2 选择通过命名的追随或固定绑定解析，不使用这条隐式覆盖链。

```json
{"harness":{"overrides":{"filesystem":{"mode":"read_only"}}}}
```

上述对象可以作为项目或角色的 `permission_overrides`。工具网络与模型连接分别处理：Sandbox 禁网会影响模型连接；需要模型联网而工具禁网时，在 Harness 上设置禁网并选择支持该限制的执行器。

目前网络 allowlist 返回 `permission_unsupported`，普通 bridge 不提供域名过滤。DSH 工具禁网要求受限文件模式以及显式的 `bash`/`ask_user_question` 工具范围；文件 FullAccess 会跳过其原生沙箱，因此不能声称 wrapper 仍然生效。Docker 不理解工具名，工具规则应绑定 Harness。

DSH 原生 `workspace-write` 始终允许会话 cwd 写入；`scoped` 中比 ExecutionScope 挂载更窄的写规则在预检时拒绝。限定测试写目录、实施阶段只读测试目录由冻结挂载落实。需要任意 worktree 子目录规则时，当前可使用 Codex 原生命名权限 profile。

## 后端与平台

Codex 0.167.0 全权限使用 App Server externalSandbox；受限配置使用原生命名权限 profile。DSH 0.2.2-rc.1 使用原生 permission preset、sandbox-policy 和 approval；额外的工具过滤器读取同一份只读冻结策略。

`docker-default-v1` 使用 Docker 默认 seccomp/AppArmor，无需安装专用嵌套策略。`docker-nested-v1` 使用 `docker/seccomp/nested-v1.json`；启用 AppArmor 的 Linux 宿主还需安装命名 profile：

```bash
./scripts/platform/linux/install-linux-sandbox-policy.sh --check
sudo ./scripts/platform/linux/install-linux-sandbox-policy.sh --install
./scripts/check/doctor.sh --permission-backend docker-nested-v1
```

安装器核对并加载 `devdesk-harness-v1`，保留 Docker 默认策略。Moby 基线和许可证位于 `docker/upstream/`。修改内核策略内容必须发布新后端版本，不能覆盖旧运行引用的资产。

## 构建与校验

```bash
./scripts/check/check-permissions.sh
docker build -f containers/dsh/Dockerfile -t devdesk/dsh:0.2.2-rc.1-permissions-v1 .
```

根目录 `.dockerignore` 只将 DSH Dockerfile 和权限后端复制进构建上下文。新镜像以新的不可变 image ID 登记；已有冻结镜像继续使用原身份。运行包携带 `permissions/` 和相关 Schema，运行时策略也编译嵌入 Host，无需源码目录。

实施与平台证据见 [执行报告](../docs/plans/permission-standardization/EXECUTION.md)。
