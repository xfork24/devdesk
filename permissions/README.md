# Permission 权限与执行后端

Permission 是不可变修订的文件、网络和工具访问策略，分别绑定 Harness 和 Sandbox。预设位于 `presets/`；固定供应商和内核执行器位于 `backends/`，manifest 记录版本、平台、来源和摘要。

## 默认与受限执行

新数据集默认使用 `container-full-access`：Harness 采用供应商原生全权限和无需审批，Docker 提供普通用户、非特权、独立命名空间及授权挂载。容器层和临时空间可写，临时文件可执行。全权限以整个容器为信任范围，工具可以读取该容器内的状态和模型凭据；宿主 HOME、Docker socket 和未授权目录不挂载。

旧数据集迁移为 `legacy-restricted-v1`，保留原有工具禁网和状态隔离方式。新默认不会改变旧 Session/Run；修改绑定只影响后续执行。正式工程阶段的 worktree、测试目录和只读 Review 视图另由冻结 ExecutionScope 限制。

## 配置与覆盖

通过 Web 设置中的“运行权限 Permission”选择预设、创建新修订、设置 Harness/Sandbox 绑定和审批、预览最终权限。项目和角色可分别覆盖，优先级为目标绑定、项目、角色、本次执行。省略字段继承，显式字段替换，列表整体替换；null 和未知字段拒绝。

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
./scripts/install-linux-sandbox-policy.sh --check
sudo ./scripts/install-linux-sandbox-policy.sh --install
./scripts/doctor.sh --permission-backend docker-nested-v1
```

安装器核对并加载 `devdesk-harness-v1`，保留 Docker 默认策略。Moby 基线和许可证位于 `docker/upstream/`。修改内核策略内容必须发布新后端版本，不能覆盖旧运行引用的资产。

## 构建与校验

```bash
./scripts/check-permissions.sh
docker build -f containers/dsh/Dockerfile -t devdesk/dsh:0.2.2-rc.1-permissions-v1 .
```

根目录 `.dockerignore` 只将 DSH Dockerfile 和权限后端复制进构建上下文。新镜像以新的不可变 image ID 登记；已有冻结镜像继续使用原身份。运行包携带 `permissions/` 和相关 Schema，运行时策略也编译嵌入 Host，无需源码目录。

实施与平台证据见 [执行报告](../docs/plans/permission-standardization/EXECUTION.md)。
