# Ubuntu Web 真实工程验收计划

日期：2026-11-08。状态：待切换 Ubuntu 执行，本文不代表验收通过。

执行者：Codex。依据：[真实工程接入计划](../real-engineering-integration/PLAN.md)、[平台与凭据补充计划](../web-platform-and-credentials/PLAN.md)、[macOS R0–R8 验收](../real-engineering-integration/R8-acceptance.md)。操作入口见 [Ubuntu Web 验收说明](../../ubuntu-web-acceptance.md)。

## 目标与边界

在真实 Ubuntu 宿主上验证：Web 讨论 → Requirement/Design → TaskPlan/TestPlan 与正式 Review → 双 Harness Docker 实施和测试 → GitHub PR → Feature 与主干接纳；同时验证凭据、安装复用、会话隔离、取消/恢复和运行包。

优先 Ubuntu 24.04 的实际开发桌面环境，记录准确版本和 CPU 架构；若使用其他版本，按实际环境报告。本轮不构建 macOS native 客户端，也不扩展 Harness 版本、语言配方、Archive 或 data 搬迁范围。Linux 容器、旧原型 Linux 报告和 macOS 本轮证据均不能替代 Ubuntu 宿主验收。

允许修复在 Ubuntu 发现的实际兼容性问题，并增加必要的针对性回归；修改后重跑受影响步骤。不得用 Sim 回退、篡改历史绑定、放宽领域门禁、管理员 bypass 或扩大容器权限来获得通过。

## 执行约定

- 使用独立的 Ubuntu 验收数据目录；不得启动旧 macOS 数据集或复制旧凭据引用、macOS 二进制、安装 ID、Docker image ID。
- 使用专用测试仓库。可继续使用已授权的私有仓库 `xfork24/devdesk-integration-test`；先核实当前 GitHub 账户有权限。若不可用，按已有授权创建新的私有测试仓库并记录名称。禁止将 `xfork24/devdesk` 作为被管理项目。
- 完整 live 会调用真实模型、创建提交/PR 并合入测试仓库；使用新报告目录和随机用例前缀，保留之前的验收历史。
- 所有报告单独放到 `docs/plans/ubuntu-web-acceptance/<run-id>/`，不覆盖 macOS 的 R3–R8 文件。保存命令、退出码、版本、截图及经检查的日志；密钥、OAuth token、完整凭据库不得进入 Git、报告或聊天。
- 只停止/删除有验收归属回执的 Host、容器和 worktree；不清理用户其他镜像、卷或容器。崩溃测试只针对唯一匹配数据目录和端口的专用 Host，不能终止 Codex 桌面应用。
- 任一必要前置条件缺失，记录阻塞并继续可独立执行的步骤。需要网页登录、解锁凭据库或系统安装授权时告知用户具体原因；未完成的步骤保持待验收，不能将 skip 计为通过。

## U0：接续源码、依赖与基线

- [ ] 拉取包含本计划的提交，记录 `git rev-parse HEAD` 和工作区状态；保留用户已有修改，不强制覆盖。记录 `/etc/os-release`、`uname -m`、Rust/Node/npm/Python/Git/gh/Docker 版本。
- [ ] 安装仓库固定 Rust 1.99.1、Node 24、C 构建工具、Python 3、Git、GitHub CLI、Docker Engine、`libsecret-tools`，以及桌面 Secret Service（例如 GNOME Keyring）。确认当前用户拥有 Docker 权限，当前桌面用户的 D-Bus 和凭据库可用；不能仅凭 SSH 中存在 `secret-tool` 判定可用。
- [ ] 安装并登记匹配版本的本机 Codex。必须能识别实际 Linux 原生可执行文件；若 PATH 是 npm/JS 启动器，定位对应平台的原生二进制并验证发现逻辑，不能伪造 ELF 或安装信息。DSH 使用固定 Docker 运行时，不要求额外复制一套宿主 SDK。
- [ ] 安装 Web 依赖和 Chromium 所需系统依赖，完成离线基线：

```bash
npm --prefix web ci
(cd web && npx playwright install --with-deps chromium)
./scripts/check.sh
```

通过条件：Rust 全工作区、debug/release HTTP、复制后的 standalone Host、debug/release Chromium 全部退出 0，保存日志。依赖安装需要的系统授权在实际发生时处理。

## U1：本机镜像、设置与 Linux 凭据

- [ ] 在 Ubuntu 当前架构构建固定版本镜像，检查 Docker inspect 的 OS/architecture/不可变 ID，并登记到全局 registry：

```bash
docker build -t devdesk/codex:0.167.0 containers/codex
docker build -t devdesk/dsh:0.2.2-rc.1 containers/dsh
gh auth login
```

固定 Node 基础镜像已知包含 amd64/arm64 manifest，但这不是 Ubuntu 构建通过的证据。不得把 macOS 登记的镜像摘要作为本次本机安装结果。

- [ ] 创建唯一验收目录和报告目录。在同一终端设置下列非秘密变量；后续终端使用相同绝对路径：

```bash
export DEVDESK_UBUNTU_RUN="ubuntu-$(date -u +%Y%m%dT%H%M%SZ)"
export DEVDESK_UBUNTU_DATA="$PWD/data/$DEVDESK_UBUNTU_RUN"
export DEVDESK_UBUNTU_REPORTS="$PWD/docs/plans/ubuntu-web-acceptance/$DEVDESK_UBUNTU_RUN"
export DEVDESK_UBUNTU_REPO="xfork24/devdesk-integration-test"
mkdir -p "$DEVDESK_UBUNTU_REPORTS"
./scripts/doctor.sh --output "$DEVDESK_UBUNTU_REPORTS/preflight.json"
cargo build --locked -p devdesk-host
"$PWD/target/debug/devdesk-host" --data "$DEVDESK_UBUNTU_DATA" --port 51751
```

Host 命令前台运行；验收命令另开终端并恢复相同变量。若创建了其他专用仓库，先修改 `DEVDESK_UBUNTU_REPO`。端口冲突时调整 Host 与所有 `--url`，不能杀死无关服务。

- [ ] 在 Web 设置页登记本机 Codex、完成官方网页登录并确认登录状态；登记两个 Linux 镜像。默认 Harness 为 Codex，可切换 DSH，再切回 Codex。
- [ ] 用户通过设置页保存 DeepSeek 官方 API key，实际写入 Linux Secret Service。校验缺失/更新/删除及正常读取；测试条目和正式验收条目分离，不能删除用户已有 key。
- [ ] 退出并重新启动专用 Host，检查凭据仍可用。设置页状态查询不读取秘密、不阻塞；锁定/不可用的 Secret Service 返回明确错误，无明文文件后备、无 Sim 回退。实际锁定测试仅操作专用测试 collection；无法安全隔离时用受控后端不可用测试，并明确记录锁定场景未实测。

通过条件：预检全部必要项目通过，OAuth/DSH 实际可用；凭据不出现在 argv、SQLite、报告或浏览器持久存储中。Linux 不安装 macOS 签名证书/凭据助手。

## U2–U4：资源、讨论、规划与真实交付闭环

- [ ] 执行下面的完整入口，保存控制台输出和退出状态。它覆盖真实 Git/Docker 测试、双供应商原生讨论、浏览器采纳、正式规划、双 Task 实施、Review、合入以及 U5 恢复，不需在同一轮另行重复运行相同资源测试：

```bash
./scripts/check-live.sh --mode full --providers codex,dsh \
  --root "$DEVDESK_UBUNTU_DATA" \
  --github-repo "$DEVDESK_UBUNTU_REPO" \
  --url http://127.0.0.1:51751 \
  --reports "$DEVDESK_UBUNTU_REPORTS" \
  --allow-host-restart
```

- [ ] U2：核实两供应商多轮续用各自原生 Session；项目/角色之间 Session 独立。每次执行冻结 provider/version/model/role/credential/installation，切换默认设置不改写历史绑定。
- [ ] U2：实际 Docker 工作进程为非 root、受限挂载、只读根文件系统，无 Docker socket/用户 HOME；测试进程无模型和 GitHub 凭据。Ubuntu 的 user namespace/AppArmor/bwrap 失败必须诊断原始原因，不能改成 privileged/unconfined 或直接在宿主运行修改工具。
- [ ] U3：真实浏览器先讨论并采纳 Project Requirement；Requirement/Design/TaskPlan/TestPlan、正式 Review、initial Snapshot、Task worktree 的追踪链完整，未采纳消息不能自动变成正式领域对象。保存浏览器截图和命令回执。
- [ ] U4：两个不同供应商 Task Attempt 同时启动，各用独立 worktree/容器/Session；逐阶段 test_author → Red → implement → Green → docs → verify，保存断言失败的 Red 与同代码版本的 Green/verify。
- [ ] U4：独立 Reviewer 接受或退回均保留；拒绝后用新 Attempt 修正，不能覆盖失败证据。递归 Delivery 来源、测试配方和固定版本证据完整。
- [ ] U4：Task → Feature、Feature → main 候选只有原目标一个父提交；CAS/远端 lease、旧目标失效和重新验证成立。最终前端接纳触发真实命令，GitHub PR 的 head/mergeCommit/目标与候选一致，只有 merged 后 owner 才完成。

通过条件：完整 full 报告 `passed: true`，双 Harness 均真实执行，Feature completed，真实 PR/候选/最终 Snapshot 可复核。不得将“已发起 PR”视为最终通过。

## U5：复用、取消、崩溃与持久化恢复

- [ ] 检查 full 中 recovery 步骤：真实取消先中断再停止/封存/撤销写许可；在两个供应商输入已消费后 SIGKILL 专用 Host，另一个项目保持 active，重启先处理全部项目的遗留执行者。
- [ ] 确认未知输入未自动重发，原 external IDs 保留，通过显式取消/新操作继续；所有实际启动执行者有停止、导出封存和释放回执。未取得 worker 回执的启动失败单独记录，不伪造容器或协议证据。
- [ ] 完整流程建立跨项目/角色引用后，运行真实安装复用探针：

```bash
python3 tests/live/probe_installations.py \
  --url http://127.0.0.1:51751 --root "$DEVDESK_UBUNTU_DATA" \
  --reports "$DEVDESK_UBUNTU_REPORTS"
```

两套镜像分别并发登记四次返回同一安装 ID；跨项目/角色引用同一对应安装，Docker 公共层实际共享。不同版本并存/摘要冲突由 registry 契约测试补充，不伪称已安装新的真实版本。

- [ ] 运行证据复核，重新查询数据库、Artifact 哈希、Git 与实际 GitHub 状态：

```bash
./scripts/check-live.sh --mode verify-existing --providers codex,dsh \
  --root "$DEVDESK_UBUNTU_DATA" \
  --github-repo "$DEVDESK_UBUNTU_REPO" \
  --url http://127.0.0.1:51751 \
  --reports "$DEVDESK_UBUNTU_REPORTS"
```

通过条件：复用探针和复核均通过，重启后全局配置/每项目 SQLite 隔离与历史保持完整。verify-existing 不能替代首次 full。供应商限流、落库前故障、导出/清理失败等未实际触发的外部故障，只能标注相应契约注入证据。

## U6：独立运行包与交付报告

- [ ] 执行 `./scripts/package.sh`，核对 `dist/devdesk-0.2.0-<actual-target>.tar.gz` 及 SHA256；记录 Linux target 和包内容。
- [ ] 将包解压到源码目录外，从非源码工作目录启动包内 Host，使用专用数据目录、独立端口或先优雅停止原验收 Host；检查嵌入式 Web、HTTP/SSE、凭据读取、已有项目查询，以及两个 Harness 各一轮真实会话。之后确认执行者释放和重启可读。
- [ ] 包中只有一份 Host/嵌入 Web 与容器配方；Linux 包没有 macOS helper，没有验收数据、OAuth/key、签名私钥、按项目/角色重复打包的运行时。运行所需 Docker/gh/Secret Service 等宿主依赖明确写入说明。
- [ ] 完成独立 Ubuntu 报告 `docs/plans/ubuntu-web-acceptance/<run-id>/acceptance.md`：记录源码提交、实际系统/CPU/依赖、命令退出码、安装 ID/镜像摘要、Session/Run/Execution、资源释放、Artifact、PR/候选 SHA、浏览器截图、包 SHA256 和未覆盖边界。
- [ ] 按 macOS R8 的 A01–A22 逐项填写 Ubuntu 证据和结果，区分实际外部验证、契约注入与待验证；同时映射原设计 H01–H09/INV01–INV13，不扩大原计划范围。
- [ ] 若进行了修复，重跑相关离线/真实场景，并在最终源码上完成必要回归；更新本计划、平台补充计划、README 和报告索引，只有全部必要步骤通过后才标记 Ubuntu 已验收。

## 最终验收条件

U0–U6 全部完成；双供应商真实闭环、Linux 系统凭据、安装复用、取消/跨项目恢复、独立运行包均有 Ubuntu 实机证据，full/verify-existing/安装探针及必要回归退出 0。任何必要项仍阻塞或失败时，保留真实结果和复现步骤，不将当前计划或 macOS 成果写作 Ubuntu 已通过。
