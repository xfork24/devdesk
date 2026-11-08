# Ubuntu Web 真实工程验收计划

日期：2026-11-08。状态：Ubuntu 24.04.5 / x86_64 的 U0–U6 已完成并通过。全部组 full（显式续跑）、最终 verify-existing、完整安装复用及源码外 Linux 发布包均通过；AppArmor Docker 宿主使用用户明确批准的专用策略。

本轮最终证据：[Ubuntu 完成报告](ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。[首次失败报告](ubuntu-20261108T095147Z/acceptance.md)及历次失败原始文件保留，不改写为成功；最终 full、verify-existing、安装探针退出 0。

续验目录：[resume-20261108T102305Z](ubuntu-20261108T095147Z/resume-20261108T102305Z/acceptance.md)。原始失败证据保留。

执行者：Codex。依据：[真实工程接入计划](../real-engineering-integration/PLAN.md)、[平台与凭据补充计划](../web-platform-and-credentials/PLAN.md)、[macOS R0–R8 验收](../real-engineering-integration/R8-acceptance.md)。操作入口见 [Ubuntu Web 验收说明](../../ubuntu-web-acceptance.md)。

## 目标与边界

在真实 Ubuntu 宿主上验证：Web 讨论 → Requirement/Design → TaskPlan/TestPlan 与正式 Review → 双 Harness Docker 实施和测试 → GitHub PR → Feature 与主干接纳；同时验证凭据、安装复用、会话隔离、取消/恢复和运行包。

优先 Ubuntu 24.04 的实际开发桌面环境，记录准确版本和 CPU 架构；若使用其他版本，按实际环境报告。本轮不构建 macOS native 客户端，也不扩展 Harness 版本、语言配方、Archive 或 data 搬迁范围。Linux 容器、旧原型 Linux 报告和 macOS 本轮证据均不能替代 Ubuntu 宿主验收。

允许修复在 Ubuntu 发现的实际兼容性问题，并增加必要的针对性回归；修改后重跑受影响步骤。不得用 Sim 回退、篡改历史绑定、放宽领域门禁、管理员 bypass 或扩大容器权限来获得通过。

2026-11-08 用户明确批准并安装 `devdesk-harness-v1` 专用 AppArmor 策略，允许 bwrap 在限定准备路径中的挂载操作，作为原计划禁止扩大权限条款的已授权例外。外层仍非 root、cap-drop=ALL、只读根、no-new-privileges；未修改 docker-default、宿主 sysctl 或其他容器。策略 SHA256 为 `3af35216df62da5ff6bfbe54b47506b219f26ccc99c944d84b826d66870cdb2c`，实际允许/拒绝边界均已验证。

## 执行约定

- 使用独立的 Ubuntu 验收数据目录；不得启动旧 macOS 数据集或复制旧凭据引用、macOS 二进制、安装 ID、Docker image ID。
- 使用专用测试仓库。可继续使用已授权的私有仓库 `xfork24/devdesk-integration-test`；先核实当前 GitHub 账户有权限。若不可用，按已有授权创建新的私有测试仓库并记录名称。禁止将 `xfork24/devdesk` 作为被管理项目。
- 完整 live 会调用真实模型、创建提交/PR 并合入测试仓库；使用新报告目录和随机用例前缀，保留之前的验收历史。
- 所有报告单独放到 `docs/plans/ubuntu-web-acceptance/<run-id>/`，不覆盖 macOS 的 R3–R8 文件。保存命令、退出码、版本、截图及经检查的日志；密钥、OAuth token、完整凭据库不得进入 Git、报告或聊天。
- 只停止/删除有验收归属回执的 Host、容器和 worktree；不清理用户其他镜像、卷或容器。崩溃测试只针对唯一匹配数据目录和端口的专用 Host，不能终止 Codex 桌面应用。
- 任一必要前置条件缺失，记录阻塞并继续可独立执行的步骤。需要网页登录、解锁凭据库或系统安装授权时告知用户具体原因；未完成的步骤保持待验收，不能将 skip 计为通过。

## U0：接续源码、依赖与基线

- [x] 接续已包含本计划的提交 `a620e92df36e536d66f74a67b31cd7e7c4670c48`，初始工作区干净；已记录系统、架构和全部工具版本。见本轮 `context.json`、`environment.log`、`dependencies-after-install.log`；未覆盖用户修改。
- [x] 固定 Rust 1.99.1、Node 24、C 编译器、Python 3、Git、gh、Docker、libsecret-tools 和 GNOME Keyring 均可用；用户已完成系统安装及 GitHub 登录，Docker 权限、Secret Service 元数据与实际测试条目读写通过。
- [x] 本机 Codex 0.167.0 为真实 Linux/x86_64 ELF，经设置页发现、校验摘要并登记。当前 PATH 使用原生独立安装，未伪称实测了 npm 启动器发现。DSH 使用固定 Docker 镜像。
- [x] Web 依赖和 Chromium 已就绪，完整离线基线通过。`--with-deps` 首次被 sudo 密码阻塞；浏览器安装退出 0，现有系统库实际运行两轮 Chromium 全部通过，首次失败日志保留：

```bash
npm --prefix web ci
(cd web && npx playwright install --with-deps chromium)
./scripts/check.sh
```

通过条件：Rust 全工作区、debug/release HTTP、复制后的 standalone Host、debug/release Chromium 全部退出 0，保存日志。依赖安装需要的系统授权在实际发生时处理。

## U1：本机镜像、设置与 Linux 凭据

- [x] 已在 Ubuntu 本机构建两个固定版本 Linux/amd64 镜像，检查不可变 ID 并经 Web 设置页登记；用户已完成 GitHub 登录：

```bash
docker build -t devdesk/codex:0.167.0 containers/codex
docker build -t devdesk/dsh:0.2.2-rc.1 containers/dsh
gh auth login
```

固定 Node 基础镜像已知包含 amd64/arm64 manifest，但这不是 Ubuntu 构建通过的证据。不得把 macOS 登记的镜像摘要作为本次本机安装结果。

- [x] 已创建唯一验收目录 `data/ubuntu-20261108T095147Z` 及同名报告目录，预检退出 0，专用 Host 使用 51751 端口，归属见本轮 Host 回执。后续终端恢复相同绝对路径：

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

本轮完整工程证据位于报告根的 `resume-20261108T102305Z/full-20261108T103202Z/`。同轮 `verify-existing` 的 `--reports` 应指向该目录；新 full 使用新目录。失败 full 需要继续时显式 `--resume`，它核对上下文及成功日志摘要，并保留旧报告；不能用于已通过的 full。

上述代码用于创建新轮次；继续本轮时令 `DEVDESK_UBUNTU_RUN=ubuntu-20261108T095147Z`，恢复已记录的绝对数据路径。重跑 full 时另设新的报告子目录，保留本轮失败文件。Host 命令前台运行；验收命令另开终端并恢复相同变量。若创建了其他专用仓库，先修改 `DEVDESK_UBUNTU_REPO`。端口冲突时调整 Host 与所有 `--url`，不能杀死无关服务。

- [x] 在 Web 设置页登记本机 Codex、完成官方网页登录并确认登录状态；登记两个 Linux 镜像。默认 Harness 为 Codex，可切换 DSH，再切回 Codex。
- [x] 用户通过设置页保存 DeepSeek 官方 API key，实际写入 Linux Secret Service。校验缺失/更新/删除及正常读取；测试条目和正式验收条目分离，不能删除用户已有 key。
- [x] 退出并重新启动专用 Host，检查凭据仍可用。设置页状态查询不读取秘密、不阻塞；锁定/不可用的 Secret Service 返回明确错误，无明文文件后备、无 Sim 回退。实际锁定测试仅操作专用测试 collection；无法安全隔离时用受控后端不可用测试，并明确记录锁定场景未实测。

通过条件：预检全部必要项目通过，OAuth/DSH 实际可用；凭据不出现在 argv、SQLite、报告或浏览器持久存储中。Linux 不安装 macOS 签名证书/凭据助手。

本轮结果：本机/镜像登记、默认 Codex → DSH → Codex 和历史冻结绑定通过；两供应商实际登录/调用及重启后读取通过。隔离测试条目完成保存/更新/删除、重启与不可用 D-Bus；未锁定用户 collection，真实锁定场景未实测。

## U2–U4：资源、讨论、规划与真实交付闭环

- [x] 执行下面的完整入口，保存控制台输出和退出状态。它覆盖真实 Git/Docker 测试、双供应商原生讨论、浏览器采纳、正式规划、双 Task 实施、Review、合入以及 U5 恢复，不需在同一轮另行重复运行相同资源测试：

```bash
./scripts/check-live.sh --mode full --providers codex,dsh \
  --root "$DEVDESK_UBUNTU_DATA" \
  --github-repo "$DEVDESK_UBUNTU_REPO" \
  --url http://127.0.0.1:51751 \
  --reports "$DEVDESK_UBUNTU_REPORTS" \
  --allow-host-restart
```

- [x] U2：核实两供应商多轮续用各自原生 Session；项目/角色之间 Session 独立。每次执行冻结 provider/version/model/role/credential/installation，切换默认设置不改写历史绑定。
- [x] U2：实际 Docker 工作进程为非 root、受限挂载、只读根文件系统，无 Docker socket/用户 HOME；测试进程无模型和 GitHub 凭据。Ubuntu 的 user namespace/AppArmor/bwrap 失败必须诊断原始原因，不能改成 privileged/unconfined 或直接在宿主运行修改工具。
- [x] U3：真实浏览器先讨论并采纳 Project Requirement；Requirement/Design/TaskPlan/TestPlan、正式 Review、initial Snapshot、Task worktree 的追踪链完整，未采纳消息不能自动变成正式领域对象。保存浏览器截图和命令回执。
- [x] U4：两个不同供应商 Task Attempt 同时启动，各用独立 worktree/容器/Session；逐阶段 test_author → Red → implement → Green → docs → verify，保存断言失败的 Red 与同代码版本的 Green/verify。
- [x] U4：独立 Reviewer 接受已实际完成，技术失败保留并通过新 Review 修正；失败实施通过新 Attempt 修正，未覆盖证据。质量退回/人工 Override 由契约和 Web 回归覆盖，本轮未实际触发。递归 Delivery 来源、测试配方和固定版本证据完整。
- [x] U4：Task → Feature、Feature → main 候选只有原目标一个父提交；CAS/远端 lease、旧目标失效和重新验证成立。最终前端接纳触发真实命令，GitHub PR 的 head/mergeCommit/目标与候选一致，只有 merged 后 owner 才完成。

通过条件：完整 full 报告 `passed: true`，双 Harness 均真实执行，Feature completed，真实 PR/候选/最终 Snapshot 可复核。不得将“已发起 PR”视为最终通过。

首次完整入口因 docker-default 下 bwrap 挂载权限失败退出 1，原证据保留。用户批准并加载命名策略后，真实 Git 2/2、Docker 4/4、CommandTests 2/2 通过；最终全部组 full 经显式 `--resume` 19/19 通过。双供应商初始 Attempt 实际并行，DSH 后续修正另建；最终两 Task、独立 Review、单父候选和真实 PR #6/#8/#9 merged，Feature completed。精确 SHA、Run/Artifact 与截图见完成报告。

## U5：复用、取消、崩溃与持久化恢复

- [x] 检查 full 中 recovery 步骤：真实取消先中断再停止/封存/撤销写许可；在两个供应商输入已消费后 SIGKILL 专用 Host，另一个项目保持 active，重启先处理全部项目的遗留执行者。
- [x] 确认未知输入未自动重发，原 external IDs 保留，通过显式取消/新操作继续；所有实际启动执行者有停止、导出封存和释放回执。未取得 worker 回执的启动失败单独记录，不伪造容器或协议证据。
- [x] 完整流程建立跨项目/角色引用后，运行真实安装复用探针：

```bash
python3 tests/live/probe_installations.py \
  --url http://127.0.0.1:51751 --root "$DEVDESK_UBUNTU_DATA" \
  --reports "$DEVDESK_UBUNTU_REPORTS"
```

两套镜像分别并发登记四次返回同一安装 ID；跨项目/角色引用同一对应安装，Docker 公共层实际共享。不同版本并存/摘要冲突由 registry 契约测试补充，不伪称已安装新的真实版本。

- [x] 运行证据复核，重新查询数据库、Artifact 哈希、Git 与实际 GitHub 状态：

```bash
./scripts/check-live.sh --mode verify-existing --providers codex,dsh \
  --root "$DEVDESK_UBUNTU_DATA" \
  --github-repo "$DEVDESK_UBUNTU_REPO" \
  --url http://127.0.0.1:51751 \
  --reports "$DEVDESK_UBUNTU_REPORTS"
```

通过条件：复用探针和复核均通过，重启后全局配置/每项目 SQLite 隔离与历史保持完整。verify-existing 不能替代首次 full。供应商限流、落库前故障、导出/清理失败等未实际触发的外部故障，只能标注相应契约注入证据。

首次安装探针/verify-existing 缺少完整前置证据而失败，原结果保留。建立真实跨项目/角色引用及完整闭环后，两者均退出 0。实际 consumed-input SIGKILL 恢复、另项目 active、后台执行者回收、不重发与显式取消通过；最终 331 个 Artifact 哈希一致，79 个执行者均封存释放。

## U6：独立运行包与交付报告

- [x] `./scripts/package.sh` 退出 0，生成 `x86_64-unknown-linux-gnu` 包及 SHA256；最终包内容与摘要见续验目录 `package.json`，父目录 `U6-package.json` 为历史产物。
- [x] 将包解压到源码目录外，从非源码工作目录启动包内 Host，使用专用数据目录、独立端口或先优雅停止原验收 Host；检查嵌入式 Web、HTTP/SSE、凭据读取、已有项目查询，以及两个 Harness 各一轮真实会话。之后确认执行者释放和重启可读。
- [x] 已检查包中只有一份 Host/嵌入 Web 与容器配方，无 macOS helper、验收数据、OAuth/key 或签名私钥；宿主依赖见运行说明。
- [x] 本轮独立 `acceptance.md` 记录实际环境、命令、安装/镜像、截图、包和阻塞；未产生的 Session/Run/Execution、Artifact、PR/候选不填造假值。
- [x] 本轮报告逐项填写 A01–A22，并映射 H01–H09/INV01–INV13，区分实际外部验证、契约测试与待验。
- [x] 若进行了修复，重跑相关离线/真实场景，并在最终源码上完成必要回归；更新本计划、平台补充计划、README 和报告索引，只有全部必要步骤通过后才标记 Ubuntu 已验收。

最终包内 Host 已在源码外通过 HTTP/SSE、Linux 系统凭据、双 Harness 各两轮真实续聊、重启后凭据/已有项目/Snapshot 查询与 Chromium 检查；所有执行者释放。文档收尾重打包按 Host/配方/策略/安装器逐字节等价核验并复测 HTTP；保留独立专用 Host 供用户查看。

## 最终验收条件

U0–U6 全部完成；双供应商真实闭环、Linux 系统凭据、安装复用、取消/跨项目恢复、独立运行包均有 Ubuntu 实机证据，full/verify-existing/安装探针及必要回归退出 0。任何必要项仍阻塞或失败时，保留真实结果和复现步骤，不将当前计划或 macOS 成果写作 Ubuntu 已通过。
