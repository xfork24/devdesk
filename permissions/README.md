# Permission 权限与执行后端

Permission 是不可变修订的文件、网络和工具访问策略，分别绑定 Harness 和 Sandbox。预设位于 `resources/configuration/permissions/profiles/`；固定供应商和内核执行器位于 `backends/`，manifest 记录版本、平台、来源和摘要。

## 版本化 profile 与运行时绑定

设置中的“Permission 定义与版本绑定”使用 v2 JSON。Harness profile 必须声明 filesystem、network、tools、approval；Sandbox profile 只接受 filesystem、network。示例见 `../resources/configuration/permissions/profiles/permission-harness-read-only-v2-r1.json`；Schema 为 `schemas/permission-profile.schema.json`。文件和 UI 使用同一规范化与校验入口。

UI 保存追加权限修订，原绑定保持不变。“应用到该目标版本”将确切 Harness/Sandbox 资源修订绑定到所选 profile。绑定也保存不可变修订和执行合同，并以 CAS 更新活动分配；可再次选择旧 profile 实现回退。新目标没有自动全权限绑定，需要显式配置。目标的配置修订、供应商版本、镜像身份和平台分别核验。

项目选择独立的 Permission Context；Agent 和 Capability 不保存权限。Context 选择 Sandbox 与绑定方式，并按 Workflow 修订及步骤 key 声明目录和工具授权。运行记录实际采用的绑定引用，后续恢复按原源修订重建权限视图。

预设只使用一个当前来源目录，默认为发布包同目录中的 `resources/configuration/permissions/profiles/`；开发环境没有包内目录时使用源码预设。设置页通过“打开文件夹”浏览内核所在机器上的目录，选择后替换当前来源，选择在重启后保留。“重新加载预设”只读取当前目录；“重新加载内置预设”读取内置目录，并在成功后恢复为当前来源。自定义目录不会在启动时自动重新导入，目录丢失时仍可进入设置恢复内置来源。切换或重新加载不删除已有权限版本、不改变活动绑定，也不改动目录文件。UI 保存写入权限库，导出提供所选修订及前序依赖，不改写源码或安装目录。同一修订内容冲突、同版本不同 ID、缺少前序或非法字段均拒绝，批次不部分入库。

绑定预检只证明目标兼容，返回 `scope_checked: false`；“实际执行范围预览”可以传入 ExecutionScope。正式执行仍由协调器解析真实范围并冻结，客户端预览不构成写入授权。规则无法由目标执行合同落实时返回 `permission_unsupported`；v2 受限 profile 的 `on_request` 在无法维持最大授权范围时拒绝。

## 当前默认与授权范围

系统只接受 v2 完整 profile。内置 Harness 与 Docker Sandbox 的活动默认绑定分别使用 `permission-harness-full-access-v2-r1` 与 `permission-sandbox-full-access-v2-r1`；默认 Permission Context 显式追随目标绑定。用户新建的目标必须显式应用 profile 后才能执行。

全权限允许供应商原生工具在容器内运行，无需审批。Docker 提供普通用户、非特权、独立命名空间和授权挂载；宿主 HOME、Docker socket 与未授权目录不挂载。工程阶段的工作树、测试目录与只读 Review 视图由冻结 ExecutionScope 限制。

所有执行从独立 Permission Context 解析选择，实际采用的绑定作为执行事实固定。完整权限只在内存中生成，不保存展开闭包。Profile 使用源定义封套，身份与修订只在外层声明一次，正文仅含 target_kind、policy 及底层协议版本。

工具网络与模型连接分别处理：Sandbox 禁网会影响模型连接；需要模型联网而工具禁网时，在 Harness profile 上设置禁网并选择支持该限制的执行器。

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
