# Permission 领域模型与运行时权限标准化实施计划

日期：2026-11-08。状态：待实施。范围：Permission 领域对象、Harness 与 Sandbox 绑定、原生权限映射、DSH 与 Docker 策略归一、迁移、Web 配置及验收。各阶段完成状态须以本计划目录内的实际报告和证据更新。

将分散在角色、运行绑定、DSH 扩展和 Docker 参数中的权限统一为可修订、可覆盖、可冻结的策略。新建配置默认允许容器内当前用户可访问的本地资源和正常出站网络，由 Docker 提供外层隔离；既有执行保留原权限及其真实保证。统一的是权限语义、绑定和执行证据，供应商原生配置与内核策略仍由对应适配器解释。

## 一 目标与范围边界

1. `Permission` 成为独立领域实体，可附加到领域对象 `Harness`、`Sandbox`；不是仅给配置 JSON 改名，也不是附加到同名执行 trait 上的可变全局状态。
2. 支持文件、工具网络与工具调用规则，明确默认、覆盖、冲突、能力不足和最终执行者。审批、容器隔离、资源配额分别建模。
3. Codex、DSH 优先使用固定版本支持的原生权限接口；自定义扩展只补足原生无法表达且本产品实际需要的规则。
4. `containers/dsh/devdesk-policy/` 与 `containers/security/` 中的策略按统一目录、Schema、后端标识及摘要登记，清除独立维护的隐藏权限规则。
5. 完整贯通定义、数据库、运行锁、适配器、API、Web、打包和故障恢复。修改默认值不得重解释旧 Run、Session 或容器。

本轮不构建多租户 IAM、组织 RBAC、通用 Rego/Cedar 引擎、宿主机特权执行、任意策略脚本上传或凭据代理。命名为 `Allowlist` 的网络策略只有在相应执行器可强制落实时才允许启用；Schema 中存在该类型不代表每个后端都支持。

## 二 当前实现与需要替换的入口

| 当前入口 | 当前行为 | 标准化目标 |
|---|---|---|
| `domain/agents.rs`、`conversation.rs` | `ToolPolicy` 将角色工作类型、文件写范围绑定在一起 | 拆分业务执行范围与 Permission；保留角色、归属和正式 Run 门禁 |
| `host/workspace.rs` | 解析写路径、只读目录及配置摘要 | 统一解析两层绑定、覆盖、执行范围和能力，冻结结果 |
| `adapters/harness.rs` | 手写 Codex 权限 TOML、审批参数和 DSH profile | 从冻结 Permission 编译供应商原生配置 |
| `dsh/devdesk-policy/index.mjs` | 写死 `bash`、`ask_user_question` 白名单及升权拦截 | 使用冻结的工具规则和审批策略；全权限模式不继续套用旧白名单 |
| `dsh/devdesk-policy/bwrap.sh` | 无条件隔离网络并隐藏 state、协调配置及 proc | 转为受限后端能力；按所选 profile 使用，不进入默认全权限路径 |
| `security/nested-sandbox.json` | 放行内层 namespace/mount 所需调用 | 登记为版本化嵌套隔离后端，仅需要内层沙箱时启用 |
| `security/devdesk-harness-v1.apparmor` | 专用宿主 AppArmor 策略 | 保留命名安装、内容校验和平台适用范围，与所选后端关联 |
| `security/moby-default.json`、`LICENSE.moby` | 上游策略基线与许可证 | 保留原字节、来源 commit 和派生关系 |
| `adapters/docker.rs` | 固定只读根、noexec 临时区、挂载、网络及安全参数 | 由 Permission 执行计划与独立隔离配置共同生成 ContainerSpec |
| `scripts/py/doctor.py`、策略安装器 | 无条件按既有嵌套运行方式预检 | 按实际选用的后端检查，不要求全权限模式安装不需要的 AppArmor 策略 |

## 三 领域对象及职责

### Permission 定义与绑定

| 对象 | 内容及不变量 |
|---|---|
| `Permission` | 稳定身份、不可变修订 ID、版本、名称、前序修订、Schema 版本及 `PermissionPolicy`；同一修订禁止原地修改 |
| `PermissionPolicy` | 文件、网络、工具三个维度；完整策略中的字段必须明确，不能依赖运行时隐含默认 |
| `PermissionRef` | 精确引用修订，不使用运行时自动漂移的 latest 引用 |
| `PermissionBinding` | 绑定目标、PermissionRef 和显式覆盖；Harness 与 Sandbox 分别持有，可复用同一策略修订 |
| `PermissionPatch` | 只表达创建下一次执行时的覆盖，不作为完整权限使用；拒绝未知字段和歧义 null |
| `ResolvedPermission` | 基础修订、覆盖来源、最终策略、执行范围、策略摘要和解析器版本；进入运行历史后不可变 |
| `PermissionCapabilities` | 固定后端版本能够落实的规则、覆盖的工具类别及限制，用于启动前校验 |
| `PermissionEnforcementPlan` | 每条规则由谁落实、供应商原生配置、隔离 profile 引用和摘要、已知平台条件；包含编译器版本 |
| `PermissionDecision` | 启动校验、明确授权/拒绝或后端返回的拒绝证据；不伪称记录了未被观测的每一次文件访问 |

`Harness`、`Sandbox` 作为持久化配置和历史模型引用绑定；`ExecutionInstance`、`RuntimeBinding`、`RuntimeExecution` 及正式 Run 锁保存相应解析结果或不可变引用。`RuntimeInstallation` 继续表示共享二进制/镜像，不因权限不同重复登记或复制安装。

### 权限维度

- 文件：`FullAccess`、`ReadOnly`、`Scoped`。Scoped 规则仅使用执行环境中的绝对目录和 `Deny / Read / ReadWrite`，首版不支持任意 glob 或权限脚本。最具体目录规则覆盖父规则，同一路径冲突取 Deny；符号链接及路径重定向由实际执行后端保证，不能仅靠字符串前缀判定安全。
- 网络：`Enabled`、`Disabled`、`Allowlist`，明确目标和作用范围。Harness 绑定约束其工具；Sandbox 绑定约束整个执行环境，包括模型连接。普通 bridge 网络不能冒充域名 allowlist，入站发布端口与出站访问分别配置。
- 工具：默认 `AllAvailable`；受限配置使用显式允许/拒绝集合，拒绝优先。工具可用性和插件安装仍由 Harness 功能配置决定，允许某工具不负责安装它，也不授予它宿主访问权。

### 独立的配套对象

- `ApprovalPolicy`：`Never / OnRequest`，属于 Harness 的交互行为。Never 表示不发审批请求，不能把被拒绝的操作自动升级为允许；正式 Delivery/Review 与运行时审批无关。
- `ExecutionScope`：领域授权的项目、代码快照、worktree、阶段、测试目录和资源归属。Permission 不能创建其他项目访问权、改写交付门禁或绕过独立 Reviewer 身份校验。
- `SandboxIsolation`：用户/组、根文件系统可写性、挂载、命名空间、capabilities、seccomp、AppArmor、设备与凭据注入方式。与 Permission 共同决定实际边界。
- `ResourceLimits`：CPU、内存、进程数、临时空间及超时；不将配额不足误报为 Permission 拒绝。
- `NativePermissionOptions`：按 provider 和版本校验的扩展，补充公共模型未覆盖的供应商选项；不能用任意 YAML、shell 或未知字段覆盖受控配置。与公共权限冲突时启动前报错。

策略、覆盖和执行计划均不保存密钥值，只引用运行时凭据身份。当前 DSH profile 的每一项 `disabled` 配置须明确归为访问控制或功能装配：前者由 Permission 编译，后者由 Harness 功能配置管理，不能保留未解释的隐藏权限开关。

## 四 默认值 覆盖与冻结规则

### 新配置的默认行为

| 项目 | 默认选择 |
|---|---|
| Harness Permission | 文件 FullAccess、工具网络 Enabled、工具 AllAvailable |
| Harness 审批 | Never |
| Sandbox Permission | 对已授权暴露的资源允许本地访问，允许正常出站网络 |
| Sandbox 隔离 | 非特权、普通用户、独立 PID/网络空间、cap-drop、no-new-privileges，不挂载宿主 HOME 或 Docker socket |
| 容器可写空间 | 新默认 profile 使用可写容器层及可写临时空间；已授权挂载按执行范围设置读写，不继承旧策略无条件 readonly/noexec |
| 受限执行 | 通过明确的只读、Task、测试作者等覆盖或 profile 启用；独立测试容器继续采用无模型凭据、无网络的专用绑定 |

FullAccess 不授予 root 身份，也不使普通用户自动获得修改 root 所有目录的能力。需要额外用户身份或系统能力时，显式配置 SandboxIsolation 并做兼容性校验。

正式 Review 使用冻结的只读代码视图，Developer 的测试目录和 Tester 的写范围仍受 ExecutionScope 约束。这些约束在最终策略中显示为领域边界，不能继续隐藏在 `ToolPolicy` 条件分支里。修改角色默认 Permission 可以改变后续授权，但不能用权限配置接受一个不合法的正式阶段请求。

### 两种组合规则

1. 同一层按“目标绑定 → 项目覆盖 → 角色覆盖 → 本次 Session/Run 显式覆盖”逐字段解析；未提供继承、显式提供替换，列表整体替换，空列表清空该层列表。来源必须可查询。
2. Harness 与 Sandbox 不互相覆盖。对同一工具操作，最终能力为两层策略、ExecutionScope 和操作系统实际许可的交集。
3. 工具需要的能力与外层限制冲突时，预检返回资源、规则、限制层及修改入口。只有其中一层更宽泛属于正常收紧；不能把 FullAccess 显示为所有操作一定成功。
4. 必须执行但后端无法落实的规则返回 `permission_unsupported`；配置互相矛盾返回 `permission_conflict`；实际执行被拒绝返回 `permission_denied`，同时保留原始错误分类。不得悄悄降级或自动退回 Sim。
5. Session/Run 创建时解析并冻结两层结果及编译产物摘要。技术重试复用原锁；策略变更通过新绑定及新执行生效，需要改变容器参数时创建新容器。恢复先核对原对象，不能向已经运行的容器套用新策略。
6. 覆盖入口只接受经过 Host 验证的显式配置命令；模型输出、仓库文件和工具参数不能修改 Permission 定义、外层隔离或正式执行范围。

### 凭据与信任边界

默认全权限 profile 以整个容器为信任范围：同容器工具可能读取该容器内的 Harness 状态、环境及模型凭据。不能继续声明旧受限 profile 的 `/state` 不可读保证。宿主凭据库和未挂载数据仍在容器外，日志、回执与前端继续脱敏。

受限 profile 若声明工具不能读取凭据，必须覆盖所有已启用的文件、进程和工具入口，并有真实 canary 测试；无法保证时拒绝启用该声明。将模型凭据移到独立代理/执行环境作为后续单独工程，不作为本轮默认全权限落地的隐含前置条件。

## 五 DSH 与 Docker 策略的标准目录

目标结构如下；Schema 与目录在 P0 固定，实施时同时更新全部读取和打包入口：

```text
permissions/
  README.md
  presets/
    container-full-access.json
    read-only.json
    task-implementation.json
    test-author.json
    isolated-tests.json
    legacy-restricted-v1.json
  backends/
    manifest.json
    dsh/
      package.json
      tool-policy.mjs
      bwrap-runner.sh
    docker/
      seccomp/nested-v1.json
      apparmor/devdesk-harness-v1.apparmor
      upstream/moby-default.json
      upstream/LICENSE.moby
schemas/
  permission.schema.json
  permission-backend.schema.json
```

预设是带稳定 key 和版本的权限数据；后端 manifest 登记 provider/version、适用平台、能力、模板/文件摘要、上游来源及隔离 profile ID。发布时校验 manifest，打包保留所有被引用文件。系统预设使用稳定种子身份，不随每次启动创建新修订；用户修改生成独立修订。

| 原文件 | 实施方式 |
|---|---|
| `containers/dsh/devdesk-policy/index.mjs` | 移至 `backends/dsh/tool-policy.mjs`；从只读挂载的冻结策略读取工具规则，不再维护第二份白名单。原生工具配置能够等价执行时优先使用原生方式 |
| `containers/dsh/devdesk-policy/bwrap.sh` | 移至 `backends/dsh/bwrap-runner.sh`；限定为需要网络/状态/proc 隔离的受限后端。所用参数必须来自已校验执行计划，不能拼接模型输入 |
| `containers/dsh/devdesk-policy/package.json` | 扩展若保留则随 DSH 后端保存包元数据、版本及兼容范围，Dockerfile 使用统一构建上下文复制 |
| `containers/security/nested-sandbox.json` | 原字节移入 `backends/docker/seccomp/nested-v1.json`，初次标准化不顺便更改 syscall 权限 |
| `containers/security/devdesk-harness-v1.apparmor` | 原字节移入对应 AppArmor 目录，保留已安装策略名及摘要核对；改动权限内容必须发布新后端版本 |
| `containers/security/moby-default.json`、`LICENSE.moby` | 原字节迁移到 upstream，保留来源 commit 和许可证，不作为可编辑用户 Permission |
| `containers/security/README.md` | 当前说明归并至 `permissions/README.md`，解释预设、执行后端、信任范围及平台安装；旧入口保留简短导航 |

容器镜像只包含后端代码和支持文件；每次执行的权限通过不可变、只读配置注入。需要更改镜像内容时构建并登记新的 image ID，禁止用新文件替换已经冻结的旧镜像或后端身份。

更新 `include_bytes!`/`include_str!`、适配器 identity 哈希、Dockerfile 构建上下文、构建文档、`package.sh`、`package-source.sh` 及其 `scripts/py/package-source.py` 实现、预检、安装器和所有探针。历史验收 JSON/日志保持原字节，当前文档给出旧路径到新路径的对应关系；若旧执行仍引用原后端，迁移保留其资产直至该兼容路径结束。

## 六 供应商与隔离后端的映射

| 目标 | 全权限默认 | 受限配置与执行条件 |
|---|---|---|
| Codex 0.167.0 App Server | `turn/start` 使用 `externalSandbox`、`networkAccess: enabled` 和 `approvalPolicy: never`；创建/恢复线程时保持一致 | 使用该版本原生权限 profile/沙箱协议，覆盖 `thread/start`、`thread/resume` 和 `turn/start` 的优先级。不得同时混用冲突的 `default_permissions`、`sandbox_mode` 配置 |
| DSH 0.2.2-rc.1 SDK | 原生 `danger-full-access` permission preset，approval 为 never；默认不装载旧受限 runner 和白名单 | 通过原生 preset、sandbox-policy、approval 服务配置；自定义 runner/工具扩展只落实剩余规则。验证 SDK profile 实际挂载的服务及新建/恢复 Session 行为 |
| Docker | 正常出站网络、按显式隔离配置提供可写资源，采用默认 seccomp/AppArmor 路径 | 只读挂载、无网络、嵌套 seccomp 和命名 AppArmor profile 按执行计划选择；不能把需要嵌套能力的条件扩展到所有容器 |
| Sim | 使用同一策略解析与拒绝语义 | 标记为模拟决策，不声称验证了 Linux 内核、真实网络或凭据隔离 |

Codex 的 externalSandbox 是声明外层执行环境负责隔离，不能把其中的权限描述当作新的内核限制。DSH 的原生文件模式不表达网络限制；Docker 网络关闭又会同时阻止模型连接。因此“模型可联网、工具不可联网”必须选择具有工具级网络执行能力的受限后端，而不是将整个 Harness 容器设为 none。

网络 allowlist、读取拒绝、工具类别过滤等规则逐项声明支持情况。普通 Docker bridge、仅提示模型遵守规则或只禁用一个 shell 入口，都不能作为这些规则已经落实的证据。

## 七 存储 协议与兼容迁移

- 全局库保存 Permission 修订、预设及配置绑定；项目库追加复制被引用的不可变修订，保存项目覆盖、解析结果和运行证据。活跃/历史项目的权限不能依赖全局 latest 状态。
- 为当前全局 v3、项目 v31 增加顺序迁移；预期新增 `sql/global/004_permissions.sql`、`sql/032_permission_bindings.sql`，如有并发新增迁移则顺延。按显式列映射更新存储和旧单库拆分逻辑。
- 现有 `ToolPolicy`、路径及固定策略映射为等价 legacy Permission。新数据集和新建 Harness/Sandbox 默认 full access；既有配置和其后续会话继承迁移后的显式策略，改选新默认须产生新绑定，不能升级时静默提权。
- 历史运行能读取、核对和清理；无法恢复完整策略证据时标记 `legacy / enforcement_unknown`，不得伪造 full access 或 canary 通过。技术重试需原适配器和后端兼容，不满足时通过显式新执行继续。保留旧资源的识别与回执契约，使版本变化不妨碍对已确认归属的旧容器核对、停止、封存和清理；不得为恢复重新派发未知输入。
- 提供具备 command_id、expected_revision 和归属校验的权限创建/修订、绑定更新、预检和查询接口。纯预检不启动模型、不执行工具、不创建 PR、不安装宿主策略。
- `schemas/rpc.schema.json` 引用公共权限结构；独立 Schema 用于预设、后端 manifest 和导入校验。展示 requested、resolved、实际执行后端及拒绝原因，避免仅回传用户输入。
- Web 提供预设选择、覆盖、两层最终权限预览和来源、旧历史读取及能力不足提示；已有系统凭据授权流程不并入 Permission 审批。

## 八 实施阶段与交付

### P0 固定契约和基线

- [ ] 形成字段级对象、序列化、覆盖优先级、默认 profile、角色 ExecutionScope 和错误码契约。
- [ ] 用仓库固定版本核验 Codex 协议 Schema、DSH SDK profile/配置 Schema，保存不含凭据的能力清单及来源；核实受限模式的最低后端能力。
- [ ] 保存当前离线基线、legacy 策略摘要及未提交修改清单。

交付：本目录内契约、能力矩阵与基线报告。通过条件：每个策略维度均有明确执行者，默认 full access 与旧受限模式没有混淆。

### P1 纯领域模型与解析器

- [ ] 新增 `crates/domain/src/permission.rs`，接入对象、规则、绑定、Patch、解析结果及校验；拆分现有 ToolPolicy 的执行范围职责。
- [ ] 在 core 形成统一解析/执行计划接口，provider 专属字段由对应编译器处理，domain 不依赖 Docker 或供应商 SDK。
- [ ] 完成覆盖、路径冲突、非法配置、执行范围、摘要确定性及不支持能力的行为测试。

通过条件：同一输入得到相同完整策略，权限覆盖不能改写其他项目或正式领域门禁。

### P2 存储 迁移与运行锁

- [ ] 落实全局/项目库迁移、预设种子、不可变修订复制及幂等配置命令。
- [ ] 给 Harness、Sandbox 与运行绑定接入 Permission，冻结两层策略和编译版本。
- [ ] 验证旧库、旧单库拆分、未完成执行、重启恢复及技术重试的等价行为。

通过条件：新配置默认 full access，已有执行和已有显式默认不变，迁移失败可重试且不删除原始数据。

### P3 策略资产与发布路径归一

- [ ] 按第五节迁移 DSH、seccomp、AppArmor 及上游文件，建立并校验两个 Schema、预设和后端 manifest。
- [ ] 更新编译嵌入、镜像构建、包内容、预检、安装器、探针及当前文档，保留历史证据字节。
- [ ] 先以 legacy 绑定验证新目录下行为等价，再启用新默认 profile。

通过条件：只有一处规范化权限数据源；迁移的安全策略初始内容摘要一致，源码外运行包可以解析所有选定后端。

### P4 Harness 与 Docker 执行贯通

- [ ] Codex 通过原生 App Server 权限运行；DSH 通过原生 preset/服务运行，受限补充执行器消费同一冻结策略。
- [ ] Docker 从执行计划生成挂载、网络及隔离参数，验证真实容器身份与策略摘要；复用容器前核对配置。
- [ ] 全权限路径不再无条件启用 bwrap、禁网 wrapper、固定两工具白名单或嵌套 AppArmor 前置条件。
- [ ] 能力不足或冲突在派发模型任务前反馈；实际拒绝、模型传输失败、资源不足保留不同错误分类。

通过条件：两供应商的默认本地写入及工具联网均实际成功，显式覆盖也能由其声明的后端实际执行。

### P5 API 与 Web 配置

- [ ] 接入权限修订、绑定、覆盖、预检、查询和历史页面；展示最终两层结果、来源和执行后端。
- [ ] 支持全权限默认、只读/禁网覆盖、恢复已有配置及修订冲突；拒绝从仓库配置或模型输出注入外层策略。
- [ ] 更新领域、协议、架构、真实工程、系统凭据及运行包说明。

通过条件：用户可以通过 Web 完成配置、预览、执行和核对，页面所示权限与冻结回执一致。

### P6 跨平台真实验收与交付

- [ ] 运行下节矩阵及完整回归，分别在 macOS Docker Desktop 和 Ubuntu AppArmor 宿主记录实际结果。
- [ ] 验证新默认不依赖嵌套 profile；受限 profile 使用其必要的命名策略，并验证允许/拒绝两侧。
- [ ] 验证源码外发布包、安装复用、取消、崩溃、策略变更后的旧运行恢复和权限冻结。

交付：验收报告、实际命令/退出码、能力矩阵、容器配置摘要、脱敏协议、Web 截图及剩余限制。平台未执行或能力未落实时保持待验收，不合并为“全支持”。

## 九 验收矩阵与执行方式

| 场景 | 必须证明的行为 |
|---|---|
| 两供应商全权限默认 | 工具修改授权 worktree、使用可写临时区、访问受控网络探针；无需运行时审批 |
| 原生权限配置 | 启动/恢复/轮次配置一致，DSH 没有隐含旧白名单，Codex 未被旧 TOML profile 覆盖 |
| 只读和限定写 | 指定写入成功，越界写入被真实后端拒绝；读取拒绝仅在宣称支持时验收 |
| 网络分层 | 工具禁网时模型仍可通信；整个 Sandbox 禁网时明确模型传输也不可用 |
| 网络 allowlist | 有支持后端时验证允许、拒绝和绕过路径；没有支持时在预检拒绝该配置 |
| 两层冲突 | Harness 全权限不能写 Docker 只读挂载；错误指明限制层；拒绝扩大到其他项目 |
| 全权限信任范围 | 使用假凭据 canary 证明容器内可见范围，报告准确声明；不得对真实密钥做泄漏实验 |
| 受限凭据边界 | 文件工具、shell、进程及已启用其他工具均不能读取专用假凭据，不能只测 bash |
| 工程阶段 | Developer/Test 作者/Reviewer 的代码视图、测试保护、固定 SHA、正式 Review 及 PR 流程成立 |
| 生命周期 | 改默认、修订 Permission、切项目、取消、Host 崩溃和重启不改旧锁，不重复未知副作用 |
| 资产与平台 | 模板漂移、缺失 profile、错误版本、未安装 AppArmor 返回准确错误；默认模式不误要求安装 |
| 迁移与发布 | 旧数据及 legacy 执行可读/可核对；独立运行包包含完整权限资产，不依赖源码目录 |

离线回归使用 `cargo test --workspace --locked`、`./scripts/check.sh` 及针对性的 Schema/资产校验。真实资源测试扩展 `crates/adapters/tests/` 和 `tests/live/`；凭据 canary、挂载和网络探针优先使用不调用模型的可控程序，再以两供应商实际工具调用验证完整路径。

真实工程闭环沿用 `scripts/check-live.sh --mode full --providers codex,dsh`，显式指定专用数据集、测试仓库、Host URL 和本计划的新报告目录；恢复组显式使用 `--allow-host-restart`。执行实施计划不构成对任意仓库、管理员策略安装或真实模型支出的额外授权，具体操作沿用会话已有授权及仓库执行边界。

## 十 完成条件

- [ ] Permission 已贯通定义、绑定、覆盖、冻结、执行、历史和 Web，而不是仅新增未使用的类型。
- [ ] 新建默认全权限和显式受限覆盖均经两供应商真实工具验证，平台结果逐项可复核。
- [ ] DSH 与 Docker 安全配置归入统一资产体系，没有活动的旧路径硬编码或另一套固定权限列表。
- [ ] legacy 策略、历史副作用、凭据信任范围与当前策略准确区分，迁移及源码外运行包通过。
- [ ] 当前长期文档已同步，原始验收证据保持不变，未支持的能力没有显示为已生效。

## 依据

- 仓库：[架构与 ADR](../../design.md)、[真实工程接入](../../real-engineering.md)、[协议](../../protocol.md)、[计划索引](../README.md)。
- [OpenAI App Server](https://learn.chatgpt.com/docs/app-server)：externalSandbox、审批与轮次覆盖；[配置参考](https://learn.chatgpt.com/docs/config-file/config-reference)：原生权限 profile 与网络执行条件。
- [DSH permission presets](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/interaction/permission-presets/README.md)、[DSH sandbox](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sandbox/sandbox/README.md)：文件模式、审批组合及其能力范围。
- [Docker 容器配置](https://docs.docker.com/engine/containers/run/)、[seccomp](https://docs.docker.com/engine/security/seccomp/)、[AppArmor](https://docs.docker.com/engine/security/apparmor/)。外部文档核对日期为 2026-11-08；实施使用固定版本的实际 Schema 和行为核验，不将上游 master 的新能力直接视为本项目已支持。
